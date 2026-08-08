//! The GUI process — and the one place a command is carried out, whoever asked.
//!
//! Both surfaces land in [`apply`]: the window through Tauri's `run_cmd`, the agent
//! through clappkit's IPC relay. They share a state, so they cannot drift; and because
//! **every** answer is the fresh snapshot plus a sentence about what just happened, the
//! terminal and the window are always describing the same moment.
//!
//! Three details make the sync feel live rather than merely correct:
//!
//!   * anything that goes to the network pushes state **twice** — once the instant it
//!     starts (so the agent's query types itself into the window's search box and the map
//!     starts spinning) and once when the answer lands;
//!   * every snapshot carries a monotonic `rev`, so the reply to a slow call can never roll
//!     the window back over a newer pushed one (clappkit's `snapshot::with_rev`);
//!   * the camera is the one piece of state that moves continuously, so the window sends it
//!     only when it settles, and the app names the area only when the move was big enough
//!     to be worth mentioning ([`name_the_view`]).

use crate::geo::{Geo, Mode, Place};
use crate::state::{plural, Actor, AppState, Pin};
use crate::CLI;
use clappkit::app::Reply;
use clappkit::{Control, WindowPolicy};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Manager, State};
use tokio::sync::Mutex;

pub type SharedState = Arc<Mutex<AppState>>;

/// The app's own mark, embedded so a bare executable can set its Dock/taskbar icon at
/// runtime; the packaged macOS depot also wraps it in a real `.app`.
const ICON_PNG: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/icon.png"));

/// How often the agent strip is refreshed from the control pipe's roster. Clatch pushes
/// roster changes to the pipe, not to us, so this is a cheap read of an in-memory list —
/// and the window only repaints when it actually differs.
const ROSTER_POLL: Duration = Duration::from_secs(3);

/// Everything a command needs, in one handle, so the two entry points pass the same thing.
#[derive(Clone)]
pub struct Ctx {
    pub state: SharedState,
    pub control: Control,
    pub geo: Arc<Geo>,
    pub app: AppHandle,
    /// The last thing written to disk, so the common case — a command that changed nothing
    /// worth keeping — costs a string comparison instead of a write.
    pub saved: Arc<Mutex<String>>,
}

/// Where the view and the pins live between runs.
const SAVE_FILE: &str = "session.json";

pub fn run() {
    let mut initial = AppState::new();
    if let Ok(text) = std::fs::read_to_string(clappkit::paths::data_file(CLI, SAVE_FILE)) {
        if let Ok(v) = serde_json::from_str::<Value>(&text) {
            initial.restore(&v);
        }
    }
    let state: SharedState = Arc::new(Mutex::new(initial));

    let geo = match Geo::new() {
        Ok(g) => Arc::new(g),
        Err(e) => {
            // Building an HTTP client only fails if TLS itself will not start, and without
            // it there is no map to search. Say which, and stop.
            eprintln!("{CLI}: cannot start the HTTPS client: {e}");
            std::process::exit(1);
        }
    };

    let control = tauri::async_runtime::block_on(clappkit::connect_or_die(CLI));

    tauri::Builder::default()
        .setup({
            let state = state.clone();
            let control = control.clone();
            let geo = geo.clone();
            move |app| {
                let handle = app.handle().clone();
                clappkit::app::apply_icon(&handle, ICON_PNG);
                let ctx = Ctx {
                    state,
                    control,
                    geo,
                    app: handle.clone(),
                    saved: Arc::new(Mutex::new(String::new())),
                };
                // Managed AFTER the window exists, because it holds the AppHandle every
                // command pushes state through.
                app.manage(ctx.clone());
                spawn_ipc(ctx.clone());
                spawn_roster(ctx);
                Ok(())
            }
        })
        .invoke_handler(tauri::generate_handler![run_cmd, asset])
        .run(tauri::generate_context!())
        .expect("error while running maps");
}

/// The one command the webview calls: a `{ "cmd": … }` envelope — the same envelope the
/// agent's CLI sends over the socket, answered by the same function.
#[tauri::command]
async fn run_cmd(req: Value, ctx: State<'_, Ctx>, app: AppHandle) -> Result<Value, String> {
    // Window verbs (`focus`, `quit`) are the app process itself, never the state's business.
    if let Some(resp) = clappkit::app::window_cmd(&app, &req, WindowPolicy::default()) {
        return Ok(resp);
    }
    let reply = apply(&ctx, &req, None).await;
    clappkit::app::push_state(&app, reply.snapshot);
    Ok(reply.resp)
}

/// The window's image resolver — here, only an agent's avatar (an absolute path Clatch
/// resolved). Declared in this crate because `generate_handler!` cannot see a command
/// defined in another one.
#[tauri::command]
fn asset(path: String) -> Option<String> {
    clappkit::app::asset(&path)
}

fn spawn_ipc(ctx: Ctx) {
    clappkit::app::spawn_ipc(
        ctx.app.clone(),
        CLI,
        WindowPolicy::default(),
        move |req, caller| {
            let ctx = ctx.clone();
            async move { apply(&ctx, &req, caller).await }
        },
    );
}

/// Keep the window's agent strip current. It repaints only on a real difference, so this
/// is a comparison every three seconds and nothing else.
fn spawn_roster(ctx: Ctx) {
    tauri::async_runtime::spawn(async move {
        let mut shown: Vec<String> = Vec::new();
        loop {
            tokio::time::sleep(ROSTER_POLL).await;
            let rows = ctx.control.roster();
            let ids: Vec<String> = rows.iter().map(|a| a.id.clone()).collect();
            if ids == shown {
                continue;
            }
            shown = ids;
            ctx.state.lock().await.set_agents(rows);
            push(&ctx).await;
        }
    });
}

/// One command, whoever asked.
///
/// Every arm produces `extra` — the sentence and any command-specific fields — and
/// [`reply`] merges that over the fresh snapshot. So every answer, to either surface,
/// carries the whole shared state; `maps status` needs no special support anywhere.
pub async fn apply(ctx: &Ctx, req: &Value, caller: Option<String>) -> Reply {
    let cmd = req.get("cmd").and_then(Value::as_str).unwrap_or("");
    let arg = |k: &str| req.get(k).and_then(Value::as_str).map(|s| s.trim().to_string());
    let text = |k: &str| arg(k).unwrap_or_default();
    let actor = Actor::of(&caller);
    ctx.state.lock().await.remember_agent(&caller);

    let extra = match cmd {
        // Just the snapshot: `status`, and the window's first paint.
        "state" => json!({ "ok": true }),

        // The window telling us where the human dragged to. Already settled and debounced
        // on that side — this is not called per frame.
        "view" => {
            let (lat, lon, zoom) = (num(req, "lat"), num(req, "lon"), num(req, "zoom"));
            let emits = ctx.state.lock().await.look_at(lat, lon, zoom, actor);
            let announced = !emits.is_empty();
            ctx.control.emit_all(emits);
            // One reverse lookup per *announced* view, not per pan: the same threshold that
            // decides the agent hears about a move decides whether we spend a request
            // naming it. A human exploring a city costs a handful of calls, not hundreds.
            if announced {
                name_the_view(ctx, lat, lon).await;
            }
            json!({ "ok": true })
        }

        "goto" => {
            let q = text("q");
            if q.is_empty() {
                bad("go where? give me a place, an address or a coordinate")
            } else {
                begin(ctx, &format!("looking for {q}")).await;
                let bias = ctx.state.lock().await.bias();
                match ctx.geo.place(&q, bias).await {
                    Ok(p) => {
                        let mut s = ctx.state.lock().await;
                        s.busy(None);
                        let emits = s.open(p.clone(), actor);
                        s.say(format!("{} — {}", p.name, coords(&p)));
                        drop(s);
                        ctx.control.emit_all(emits);
                        json!({ "ok": true, "message": p.label(), "place": p })
                    }
                    Err(e) => fail(ctx, e).await,
                }
            }
        }

        // The window answering its own category tap from the tiles it has already drawn.
        //
        // The core does not fetch anything here: it takes the list, because a result the
        // human is looking at has to be a result the AGENT can see too — a head start that
        // only one surface knew about would be exactly the drift this app exists to avoid.
        // The `nearby` that follows a moment later replaces it with the complete answer.
        "seed" => {
            let places: Vec<Place> = req
                .get("places")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(seeded_place).collect())
                .unwrap_or_default();
            if places.is_empty() {
                json!({ "ok": true })
            } else {
                let n = places.len();
                let mut s = ctx.state.lock().await;
                s.set_results(text("q"), places);
                s.busy(Some("looking properly"));
                drop(s);
                json!({ "ok": true, "message": format!("{n} already on the map") })
            }
        }

        "find" | "nearby" => {
            let q = text("q");
            if q.is_empty() {
                bad(if cmd == "find" {
                    "find what? give me something to look for"
                } else {
                    "nearby what? try cafes, pharmacy, fuel, station…"
                })
            } else {
                begin(ctx, &format!("searching for {q}")).await;
                let (bias, radius) = {
                    let s = ctx.state.lock().await;
                    (s.bias(), s.radius_m())
                };
                let found = if cmd == "find" {
                    ctx.geo.find(&q, bias).await
                } else {
                    // "Nearby" needs a somewhere to be near. The map's centre is that,
                    // and at world zoom there isn't one — say so rather than searching
                    // the planet for the nearest cafe.
                    match bias {
                        Some(at) => ctx.geo.nearby(&q, at, radius).await,
                        None => Err(anyhow::anyhow!(
                            "nearby needs somewhere to be near — go to a place first, or zoom in"
                        )),
                    }
                };
                match found {
                    Ok(places) => {
                        let n = places.len();
                        let mut s = ctx.state.lock().await;
                        s.busy(None);
                        s.set_results(q.clone(), places);
                        let message = if n == 0 {
                            format!("nothing matches \"{q}\" around here")
                        } else if cmd == "nearby" {
                            let near = s.view().name.clone();
                            let where_ = if near.is_empty() { "here".into() } else { near };
                            format!("{n} {q} near {where_}, nearest first")
                        } else {
                            format!("{n} result{} for \"{q}\"", plural(n))
                        };
                        s.say(message.clone());
                        drop(s);
                        json!({ "ok": true, "message": message })
                    }
                    Err(e) => fail(ctx, e).await,
                }
            }
        }

        "select" => {
            let n = req.get("n").and_then(Value::as_u64).unwrap_or(0) as usize;
            let mut s = ctx.state.lock().await;
            match s.select(n, actor) {
                Ok(p) => {
                    s.say(format!("{} — {}", p.name, coords(&p)));
                    drop(s);
                    json!({ "ok": true, "message": p.label(), "place": p })
                }
                Err(e) => {
                    drop(s);
                    bad(&e)
                }
            }
        }

        "route" => {
            let (from, to) = (text("from"), text("to"));
            let mode = Mode::parse(&text("mode")).unwrap_or_default();
            if to.is_empty() {
                bad("route to where? `maps route \"<from>\" \"<to>\"`")
            } else {
                begin(ctx, &format!("routing to {to}")).await;
                match resolve_pair(ctx, &from, &to).await {
                    Err(e) => fail(ctx, e).await,
                    Ok((a, b)) => match ctx.geo.route(&a, &b, mode).await {
                        Ok(r) => {
                            let message = format!(
                                "{} — {} {} from {} to {} ({} steps, no live traffic)",
                                duration(r.secs),
                                distance(r.km),
                                mode.label(),
                                r.from,
                                r.to,
                                r.steps.len(),
                            );
                            let mut s = ctx.state.lock().await;
                            s.busy(None);
                            s.set_route(r);
                            s.say(message.clone());
                            drop(s);
                            json!({ "ok": true, "message": message })
                        }
                        Err(e) => fail(ctx, e).await,
                    },
                }
            }
        }

        "isochrone" => {
            let minutes = req.get("minutes").and_then(Value::as_u64).unwrap_or(10) as u32;
            let mode = Mode::parse(&text("mode")).unwrap_or(Mode::Walk);
            let from = text("from");
            begin(ctx, "working out how far you can get").await;
            match anchor(ctx, &from).await {
                Err(e) => fail(ctx, e).await,
                Ok(at) => match ctx.geo.reach(&at, minutes, mode).await {
                    Ok(r) => {
                        let message = format!(
                            "{} minutes {} from {}",
                            r.minutes,
                            mode.label(),
                            r.from
                        );
                        let mut s = ctx.state.lock().await;
                        s.busy(None);
                        s.set_reach(r);
                        s.say(message.clone());
                        drop(s);
                        json!({ "ok": true, "message": message })
                    }
                    Err(e) => fail(ctx, e).await,
                },
            }
        }

        "pin" => {
            let name = text("name");
            let note = text("note");
            let where_ = text("at");
            match anchor(ctx, &where_).await {
                Err(e) => fail(ctx, e).await,
                Ok(p) => {
                    let name = if name.is_empty() { p.name.clone() } else { name };
                    let mut s = ctx.state.lock().await;
                    let emits =
                        s.pin(Pin { name: name.clone(), lat: p.lat, lon: p.lon, note }, actor);
                    let n = s.pins().len();
                    let message = format!("pinned {name} — {n} pin{} on the map", plural(n));
                    s.say(message.clone());
                    drop(s);
                    ctx.control.emit_all(emits);
                    json!({ "ok": true, "message": message })
                }
            }
        }

        "unpin" => {
            let n = req.get("n").and_then(Value::as_u64).map(|n| n as usize);
            let mut s = ctx.state.lock().await;
            match s.unpin(n, actor) {
                Ok(emits) => {
                    let left = s.pins().len();
                    let message = format!("{left} pin{} left", plural(left));
                    s.say(message.clone());
                    drop(s);
                    ctx.control.emit_all(emits);
                    json!({ "ok": true, "message": message })
                }
                Err(e) => {
                    drop(s);
                    bad(&e)
                }
            }
        }

        "pins" => json!({ "ok": true }),

        "clear" => {
            let mut s = ctx.state.lock().await;
            match s.clear(&text("what"), actor) {
                Ok((message, emits)) => {
                    s.say(message.clone());
                    drop(s);
                    ctx.control.emit_all(emits);
                    json!({ "ok": true, "message": message })
                }
                Err(e) => {
                    drop(s);
                    bad(&e)
                }
            }
        }

        "export" => export(ctx, arg("path"), &text("format")).await,

        other => bad(&format!("unknown command: {other}")),
    };

    save_if_changed(ctx).await;
    reply(ctx, extra).await
}

// ─────────────────────────────────────────────────────────────────────────────
// The pieces those arms lean on
// ─────────────────────────────────────────────────────────────────────────────

/// Announce that something is happening, before it happens.
///
/// This is the mid-flight push: the window adopts the query and starts spinning the moment
/// the agent asks, instead of sitting silent for a second and then jumping.
async fn begin(ctx: &Ctx, what: &str) {
    ctx.state.lock().await.busy(Some(what));
    push(ctx).await;
}

/// A failure is still a state change: the spinner has to stop, and both surfaces say the
/// same sentence about why.
async fn fail(ctx: &Ctx, e: anyhow::Error) -> Value {
    let why = e.to_string();
    let mut s = ctx.state.lock().await;
    s.busy(None);
    s.say(why.clone());
    json!({ "ok": false, "error": why })
}

fn bad(why: &str) -> Value {
    json!({ "ok": false, "error": why })
}

/// The place a verb should act on when the caller did not name one: what is open, else a
/// pin at the middle of the map. "Here" is a real answer, and asking again would be rude.
async fn anchor(ctx: &Ctx, named: &str) -> anyhow::Result<Place> {
    if !named.trim().is_empty() {
        let bias = ctx.state.lock().await.bias();
        return ctx.geo.place(named, bias).await;
    }
    let s = ctx.state.lock().await;
    if let Some(p) = s.selected() {
        return Ok(p.clone());
    }
    let v = s.view();
    Ok(Place {
        id: format!("@{:.5},{:.5}", v.lat, v.lon),
        name: if v.name.is_empty() { "the middle of the map".into() } else { v.name.clone() },
        kind: "coordinate".into(),
        lat: v.lat,
        lon: v.lon,
        ..Place::default()
    })
}

/// Both ends of a route. An empty `from` means "from what is open, or from here", which is
/// what a person means when they say "how do I get to the station".
async fn resolve_pair(ctx: &Ctx, from: &str, to: &str) -> anyhow::Result<(Place, Place)> {
    let bias = ctx.state.lock().await.bias();
    let a = anchor(ctx, from).await?;
    let b = ctx.geo.place(to, bias).await?;
    Ok((a, b))
}

/// Put a name on the area the human moved to, and push it.
///
/// Deliberately best-effort: a map that shows no label is fine, a map that refuses to move
/// because a reverse lookup failed is not.
async fn name_the_view(ctx: &Ctx, lat: f64, lon: f64) {
    if let Ok(Some(p)) = ctx.geo.whats_here(lat, lon).await {
        let name = if p.address.is_empty() { p.name } else { p.address };
        ctx.state.lock().await.name_view(name);
        push(ctx).await;
    }
}

/// Write what is on the map, as GeoJSON or GPX.
async fn export(ctx: &Ctx, path: Option<String>, format: &str) -> Value {
    let s = ctx.state.lock().await;
    if s.pins().is_empty() && s.results().is_empty() && s.selected().is_none() {
        return bad("there is nothing on the map to export yet — search, route or pin something first");
    }
    let gpx = matches!(format.trim().to_ascii_lowercase().as_str(), "gpx");
    let features = s.geojson();
    let count = features["features"].as_array().map(Vec::len).unwrap_or(0);
    let body = if gpx { s.gpx() } else { format!("{features:#}\n") };
    drop(s);

    let ext = if gpx { "gpx" } else { "geojson" };
    let out = match path.filter(|p| !p.is_empty()) {
        Some(p) => std::path::PathBuf::from(p),
        None => clappkit::paths::data_dir(CLI).join(format!("map.{ext}")),
    };
    if let Some(dir) = out.parent() {
        if let Err(e) = tokio::fs::create_dir_all(dir).await {
            return bad(&format!("cannot create {}: {e}", dir.display()));
        }
    }
    match tokio::fs::write(&out, body).await {
        Ok(()) => {
            let path = clappkit::paths::simplified(&out).display().to_string();
            let message = format!("wrote {count} feature{} to {path}", plural(count));
            ctx.state.lock().await.say(message.clone());
            json!({ "ok": true, "message": message, "path": path })
        }
        Err(e) => bad(&format!("cannot write {}: {e}", out.display())),
    }
}

/// The caller's answer AND the snapshot to broadcast, taken from the same moment — so the
/// window can never be shown a different state than the caller was told about.
async fn reply(ctx: &Ctx, extra: Value) -> Reply {
    let snapshot = ctx.state.lock().await.snapshot();
    let mut resp = snapshot.clone();
    if let (Some(dst), Some(src)) = (resp.as_object_mut(), extra.as_object()) {
        for (k, v) in src {
            dst.insert(k.clone(), v.clone());
        }
    }
    Reply::new(resp, snapshot)
}

/// Write the view and the pins to disk when — and only when — they have changed.
///
/// Called after every command rather than from the handful of arms that happen to move the
/// camera, because that list was wrong the moment it was written: `goto` and `select` move
/// the view too, and remembering to add each new one is exactly the kind of bookkeeping
/// that quietly stops being done.
///
/// Best effort: a read-only home directory is a reason to lose a preference, never a
/// reason to fail the command the human actually asked for.
async fn save_if_changed(ctx: &Ctx) {
    let body = format!("{:#}\n", ctx.state.lock().await.saved());
    {
        let mut last = ctx.saved.lock().await;
        if *last == body {
            return;
        }
        *last = body.clone();
    }
    let path = clappkit::paths::data_file(CLI, SAVE_FILE);
    if let Some(dir) = path.parent() {
        let _ = tokio::fs::create_dir_all(dir).await;
    }
    let _ = tokio::fs::write(path, body).await;
}

/// Push the current state to the window without answering anybody — the mid-flight update.
async fn push(ctx: &Ctx) {
    let snap = ctx.state.lock().await.snapshot();
    clappkit::app::push_state(&ctx.app, snap);
}

/// One entry of a `seed` list. Everything is checked, because this arrives from the window
/// rather than from a service we parse — and a place with no coordinate is not a place.
fn seeded_place(v: &Value) -> Option<Place> {
    let s = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();
    let (lat, lon) = (v.get("lat")?.as_f64()?, v.get("lon")?.as_f64()?);
    let name = s("name");
    if name.is_empty() || !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return None;
    }
    Some(Place { id: s("id"), name, kind: s("kind"), lat, lon, ..Place::default() })
}

fn num(req: &Value, k: &str) -> f64 {
    req.get(k).and_then(Value::as_f64).unwrap_or(0.0)
}

fn coords(p: &Place) -> String {
    format!("{:.5}, {:.5}", p.lat, p.lon)
}

/// Distances and durations as a person says them. 4.19 km, not 4.193; 14 min, not 829.79 s.
pub fn distance(km: f64) -> String {
    if km < 1.0 {
        format!("{:.0} m", km * 1000.0)
    } else if km < 10.0 {
        format!("{km:.1} km")
    } else {
        format!("{km:.0} km")
    }
}

pub fn duration(secs: f64) -> String {
    let mins = (secs / 60.0).round() as i64;
    match mins {
        0 => "under a minute".to_string(),
        1..=59 => format!("{mins} min"),
        _ => {
            let (h, m) = (mins / 60, mins % 60);
            if m == 0 {
                format!("{h} h")
            } else {
                format!("{h} h {m} min")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distances_read_the_way_a_person_says_them() {
        assert_eq!(distance(0.42), "420 m");
        assert_eq!(distance(4.193), "4.2 km");
        assert_eq!(distance(128.4), "128 km");
    }

    #[test]
    fn durations_read_the_way_a_person_says_them() {
        assert_eq!(duration(829.79), "14 min");
        assert_eq!(duration(20.0), "under a minute");
        assert_eq!(duration(3600.0), "1 h");
        assert_eq!(duration(5400.0), "1 h 30 min");
    }
}
