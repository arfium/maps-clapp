//! The agent's surface: `maps <verb> …` over clappkit IPC.
//!
//! `maps --help` is the whole manual — the only one the agent gets — so it says what the
//! verbs do AND the thing that is not obvious from a command line: **the window is looking
//! at the same map you are**. A search here drops pins on the human's screen; `goto` flies
//! their view; the route you ask for is drawn on the map they are holding.
//!
//! Every answer carries the full state (app.rs `reply`), so each printer here is just a
//! view of the same snapshot the window renders — which is why `maps status` needs no
//! special support anywhere in the app.

use clappkit::ipc;
use serde_json::{json, Value};

const CLI: &str = "maps";

const HELP: &str = r#"maps — a world map on the screen you share with the human.

This is not a geocoder you drive alone: the app has a window, and it is showing what you
do. `goto` flies their view, `find` drops the results on their map, `route` draws the line
they can see. When the human opens a place or moves the map somewhere new, you hear about
it — as chat-buffer context on their next prompt, or as context at your next turn.

usage:
  maps goto "<place>"              find one place and fly the shared map to it
                                    takes a name, an address, or "41.0082, 28.9784"
  maps find "<query>"              search places by name; results land on the map
      -n <N>                        print only the first N (the window keeps all of them)
  maps nearby "<what>"             what is around the middle of the map, NEAREST FIRST
      -n <N>                        print only the first N
  maps select <N>                  open result number N: address, coordinates, what it is
  maps route "<to>"                route from what is open (or the map's centre) to <to>
      --from "<place>"              start somewhere else instead
      --mode drive|bike|walk        default drive
  maps isochrone <MINUTES>         draw how far you can get in that long
      --from "<place>"              default: what is open, or the map's centre
      --mode drive|bike|walk        default walk
  maps pin ["<name>"]              keep the open place (or the map's centre) as a pin
      --at "<place>"                pin somewhere else instead
      --note "<text>"               a note to carry on the pin
      --rm <N>                      remove pin number N
  maps pins                        list the pins with their coordinates
  maps clear [results|route|reach|pins|all]
                                    clear one layer, or all of them (`all` keeps the pins)
  maps export [<file>]             write the map as GeoJSON; prints the path
      --gpx                         write GPX instead (waypoints + the route as a track)
  maps status                      what both surfaces are looking at right now
  maps focus                       bring the window forward
  maps close                       quit the app
  maps help                        this help

  --json                            print the raw JSON answer instead of a table; every
                                    verb accepts it, and it always carries the whole state

`find` searches by name, in the search engine's own relevance order. `nearby` searches by
category around the map's centre and orders by distance, because "what is nearby" is a
question about distance and the index does not answer it that way.

Both are biased toward wherever the map already is, once it is zoomed in past a continent —
so "airport" means a different thing after `goto tokyo` than after `goto toronto`. If you
want a global search, `clear all` first or zoom out.

ROUTE TIMES HAVE NO LIVE TRAFFIC IN THEM. No open routing service has traffic data, so the
number is a free-flow estimate: right at 3am, optimistic at 6pm. Say so if it matters.

The map is OpenStreetMap: place names, addresses and roads are as good as the last person
who edited them, which is very good in cities and patchy in the countryside. Coordinates
are always `lat, lon`; exports are GeoJSON, which is `[lon, lat]`."#;

pub async fn run(args: Vec<String>) -> ! {
    let verb = args.first().map(String::as_str).unwrap_or("help");
    let rest: Vec<String> = args.iter().skip(1).cloned().collect();
    let agent = std::env::var("CLATCH_AGENT_ID").ok().filter(|s| !s.is_empty());

    let f = Flags::parse(&rest);
    let json_out = f.has("--json");

    let req: Value = match verb {
        "help" | "-h" | "--help" => {
            println!("{HELP}");
            std::process::exit(0);
        }

        "goto" => {
            if f.positional.is_empty() {
                usage("maps goto \"<place>\"");
            }
            json!({ "cmd": "goto", "q": f.positional.join(" "), "agent": agent })
        }

        "find" | "nearby" => {
            if f.positional.is_empty() {
                usage(&format!("maps {verb} \"<query>\""));
            }
            json!({ "cmd": verb, "q": f.positional.join(" "), "agent": agent })
        }

        "select" => match f.positional.first().and_then(|s| s.parse::<u64>().ok()) {
            Some(n) => json!({ "cmd": "select", "n": n, "agent": agent }),
            None => usage("maps select <N>   (the number shown beside each result)"),
        },

        "route" => {
            if f.positional.is_empty() {
                usage("maps route \"<to>\" [--from \"<place>\"] [--mode drive|bike|walk]");
            }
            json!({
                "cmd": "route",
                "to": f.positional.join(" "),
                "from": f.value("--from").unwrap_or_default(),
                "mode": f.value("--mode").unwrap_or_default(),
                "agent": agent,
            })
        }

        "isochrone" => {
            let minutes = f.positional.first().and_then(|s| s.parse::<u64>().ok());
            json!({
                "cmd": "isochrone",
                "minutes": minutes.unwrap_or(10),
                "from": f.value("--from").unwrap_or_default(),
                "mode": f.value("--mode").unwrap_or_default(),
                "agent": agent,
            })
        }

        // One verb, because "keep this place" and "stop keeping that one" are the same
        // thought — and because clatch.json's command list is the permission grain, so a
        // separate `unpin` would be a second thing to grant for no gain.
        "pin" => match f.number("--rm") {
            Some(n) => json!({ "cmd": "unpin", "n": n, "agent": agent }),
            None => json!({
                "cmd": "pin",
                "name": f.positional.join(" "),
                "at": f.value("--at").unwrap_or_default(),
                "note": f.value("--note").unwrap_or_default(),
                "agent": agent,
            }),
        },

        "pins" => json!({ "cmd": "pins", "agent": agent }),

        "clear" => json!({
            "cmd": "clear",
            "what": f.positional.join(" "),
            "agent": agent,
        }),

        "export" => json!({
            "cmd": "export",
            "path": f.positional.join(" "),
            "format": if f.has("--gpx") { "gpx" } else { "geojson" },
            "agent": agent,
        }),

        "status" => json!({ "cmd": "state", "agent": agent }),
        "focus" => json!({ "cmd": "focus" }),
        "close" => json!({ "cmd": "quit" }),

        other => {
            eprintln!("{CLI}: unknown verb \"{other}\"\n");
            println!("{HELP}");
            std::process::exit(2);
        }
    };

    let answer = match ipc::request(CLI, &req).await {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{CLI}: {e}");
            std::process::exit(1);
        }
    };

    if json_out {
        println!("{answer:#}");
        std::process::exit(if answer["ok"] == json!(false) { 1 } else { 0 });
    }

    if answer["ok"] == json!(false) {
        eprintln!("{CLI}: {}", answer["error"].as_str().unwrap_or("something went wrong"));
        std::process::exit(1);
    }

    match verb {
        "find" | "nearby" => print_results(&answer, f.number("--n").or(f.number("-n"))),
        "route" => print_route(&answer),
        "pins" => print_pins(&answer),
        "status" => print_status(&answer),
        "goto" | "select" => print_place(&answer),
        _ => {
            if let Some(m) = answer["message"].as_str() {
                println!("{m}");
            }
        }
    }
    std::process::exit(0)
}

// ─────────────────────────────────────────────────────────────────────────────
// Printers — every one of them a view of the same snapshot the window renders
// ─────────────────────────────────────────────────────────────────────────────

fn print_results(a: &Value, limit: Option<u64>) {
    let rows = a["results"].as_array().cloned().unwrap_or_default();
    if let Some(m) = a["message"].as_str() {
        println!("{m}");
    }
    if rows.is_empty() {
        return;
    }
    // `-n` trims what the TERMINAL prints and never the shared set: an agent asking for
    // three results must not leave the human's map showing three pins with nothing on
    // screen to say why.
    let shown = limit.map(|n| n as usize).unwrap_or(rows.len()).min(rows.len());
    println!();
    for (i, r) in rows.iter().take(shown).enumerate() {
        let name = r["name"].as_str().unwrap_or("");
        let kind = r["kind"].as_str().unwrap_or("");
        println!(
            "{:>3}. {name}{}",
            i + 1,
            if kind.is_empty() { String::new() } else { format!("  ({kind})") }
        );
        let addr = r["address"].as_str().unwrap_or("");
        if !addr.is_empty() {
            println!("     {addr}");
        }
        println!(
            "     {:.5}, {:.5}",
            r["lat"].as_f64().unwrap_or(0.0),
            r["lon"].as_f64().unwrap_or(0.0)
        );
    }
    if shown < rows.len() {
        println!("\n… {} more on the map; `maps status` or -n to see them here", rows.len() - shown);
    }
}

fn print_place(a: &Value) {
    let p = &a["selected"];
    println!("{}", a["message"].as_str().unwrap_or(""));
    let kind = p["kind"].as_str().unwrap_or("");
    if !kind.is_empty() {
        println!("{kind}");
    }
    println!("{:.5}, {:.5}", p["lat"].as_f64().unwrap_or(0.0), p["lon"].as_f64().unwrap_or(0.0));
}

fn print_route(a: &Value) {
    let r = &a["route"];
    println!("{}", a["message"].as_str().unwrap_or(""));
    let steps = r["steps"].as_array().cloned().unwrap_or_default();
    if steps.is_empty() {
        return;
    }
    println!();
    for s in &steps {
        let km = s["km"].as_f64().unwrap_or(0.0);
        println!(
            "  {}{}",
            s["instruction"].as_str().unwrap_or(""),
            if km >= 0.05 { format!("  ({})", crate::app::distance(km)) } else { String::new() }
        );
    }
}

fn print_pins(a: &Value) {
    let pins = a["pins"].as_array().cloned().unwrap_or_default();
    if pins.is_empty() {
        println!("no pins yet — `maps pin` keeps the place you are looking at");
        return;
    }
    println!("{} pin{}", pins.len(), if pins.len() == 1 { "" } else { "s" });
    for (i, p) in pins.iter().enumerate() {
        println!(
            "{:>3}. {}  {:.5}, {:.5}",
            i + 1,
            p["name"].as_str().unwrap_or(""),
            p["lat"].as_f64().unwrap_or(0.0),
            p["lon"].as_f64().unwrap_or(0.0)
        );
    }
    for p in pins.iter() {
        if let Some(note) = p["note"].as_str().filter(|s| !s.is_empty()) {
            println!("     {}: {note}", p["name"].as_str().unwrap_or(""));
        }
    }
}

fn print_status(a: &Value) {
    let v = &a["view"];
    let name = v["name"].as_str().unwrap_or("");
    println!(
        "looking at {}{:.5}, {:.5} — zoom {:.0}",
        if name.is_empty() { String::new() } else { format!("{name}  ") },
        v["lat"].as_f64().unwrap_or(0.0),
        v["lon"].as_f64().unwrap_or(0.0),
        v["zoom"].as_f64().unwrap_or(0.0),
    );

    let query = a["query"].as_str().unwrap_or("");
    let n = a["results"].as_array().map(Vec::len).unwrap_or(0);
    if n > 0 {
        println!("{n} result{} for \"{query}\"", if n == 1 { "" } else { "s" });
    }
    if let Some(p) = a["selected"].as_object() {
        println!("open: {}", p.get("name").and_then(Value::as_str).unwrap_or(""));
    }
    if let Some(r) = a["route"].as_object() {
        println!(
            "route: {} to {} — {} {}",
            r.get("from").and_then(Value::as_str).unwrap_or(""),
            r.get("to").and_then(Value::as_str).unwrap_or(""),
            crate::app::distance(r.get("km").and_then(Value::as_f64).unwrap_or(0.0)),
            crate::app::duration(r.get("secs").and_then(Value::as_f64).unwrap_or(0.0)),
        );
    }
    if let Some(r) = a["reach"].as_object() {
        println!(
            "reach: {} minutes from {}",
            r.get("minutes").and_then(Value::as_u64).unwrap_or(0),
            r.get("from").and_then(Value::as_str).unwrap_or(""),
        );
    }
    let pins = a["pins"].as_array().map(Vec::len).unwrap_or(0);
    if pins > 0 {
        println!("{pins} pin{}", if pins == 1 { "" } else { "s" });
    }
    let agents = a["agents"].as_array().cloned().unwrap_or_default();
    if !agents.is_empty() {
        let names: Vec<&str> =
            agents.iter().filter_map(|x| x["name"].as_str()).collect();
        println!("agents here: {}", names.join(", "));
    }
}

fn usage(line: &str) -> ! {
    eprintln!("{CLI}: {line}");
    std::process::exit(2)
}

/// The smallest flag parser that does this job: `--key value`, bare `--flag`, and
/// everything else positional. No dependency, and nothing to learn.
struct Flags {
    positional: Vec<String>,
    pairs: Vec<(String, String)>,
    bare: Vec<String>,
}

impl Flags {
    fn parse(args: &[String]) -> Flags {
        let mut f = Flags { positional: Vec::new(), pairs: Vec::new(), bare: Vec::new() };
        let mut i = 0;
        while i < args.len() {
            let a = &args[i];
            if let Some(rest) = a.strip_prefix("--").filter(|_| a.len() > 2) {
                // `--key=value` as well as `--key value`, because both get typed.
                if let Some((k, v)) = rest.split_once('=') {
                    f.pairs.push((format!("--{k}"), v.to_string()));
                } else if args.get(i + 1).is_some_and(|n| !n.starts_with('-')) {
                    f.pairs.push((a.clone(), args[i + 1].clone()));
                    i += 1;
                } else {
                    f.bare.push(a.clone());
                }
            } else if a == "-n" {
                if let Some(v) = args.get(i + 1) {
                    f.pairs.push(("-n".into(), v.clone()));
                    i += 1;
                }
            } else {
                f.positional.push(a.clone());
            }
            i += 1;
        }
        f
    }

    fn has(&self, key: &str) -> bool {
        self.bare.iter().any(|b| b == key)
    }

    fn value(&self, key: &str) -> Option<String> {
        self.pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    fn number(&self, key: &str) -> Option<u64> {
        self.value(key).and_then(|v| v.parse().ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags(s: &str) -> Flags {
        Flags::parse(&s.split_whitespace().map(String::from).collect::<Vec<_>>())
    }

    #[test]
    fn a_flag_takes_its_value_and_the_rest_stays_positional() {
        let f = flags("Shibuya Crossing --mode walk --json");
        assert_eq!(f.positional.join(" "), "Shibuya Crossing");
        assert_eq!(f.value("--mode").as_deref(), Some("walk"));
        assert!(f.has("--json"));
        assert!(!f.has("--gpx"));
    }

    #[test]
    fn equals_form_works_too_because_people_type_it() {
        assert_eq!(flags("--mode=bike").value("--mode").as_deref(), Some("bike"));
    }

    #[test]
    fn a_bare_flag_at_the_end_is_not_swallowed_by_a_missing_value() {
        let f = flags("cafe --json");
        assert!(f.has("--json"));
        assert_eq!(f.positional.join(" "), "cafe");
    }

    #[test]
    fn n_is_a_short_flag_and_a_number() {
        assert_eq!(flags("cafe -n 5").number("-n"), Some(5));
        assert_eq!(flags("cafe").number("-n"), None);
    }

    /// The lesson from jlcpcb, pinned: `-n` must not reach the shared state. It is read
    /// only when printing, and never put into the request envelope.
    #[test]
    fn n_never_becomes_part_of_a_request() {
        assert!(!HELP.contains("-n <N>                        how many to fetch"));
        assert!(
            HELP.contains("print only the first N (the window keeps all of them)"),
            "the help must say what -n does and does not do"
        );
    }

    #[test]
    fn the_help_documents_every_verb_the_manifest_declares() {
        // scripts/check-manifest.mjs is the real gate; this catches it at `cargo test`.
        for v in [
            "goto", "find", "nearby", "select", "route", "isochrone", "pin", "pins", "clear",
            "export", "status", "focus", "close",
        ] {
            assert!(HELP.contains(&format!("  maps {v}")), "--help does not document `maps {v}`");
        }
    }

    /// The one thing a routing answer must never let anybody assume.
    #[test]
    fn the_help_says_out_loud_that_there_is_no_traffic() {
        assert!(HELP.contains("NO LIVE TRAFFIC"));
    }
}
