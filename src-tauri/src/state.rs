//! The single source of truth. Both surfaces — the window and the agent's CLI — call
//! these same methods, which is why they cannot drift.
//!
//! Pure state and logic: no I/O, no networking, no platform code. The world lives in
//! `geo.rs` and the wiring in `app.rs`; what is here is *what both surfaces are looking
//! at* and the rules about it. That is what makes those rules testable, and the tests at
//! the bottom are where they actually live.
//!
//! The one thing here that no previous clapp had is a **continuous** piece of state: the
//! camera. A human dragging the map changes it sixty times a second, and an agent does not
//! want sixty notifications about it — see [`AppState::look_at`] and [`worth_announcing`].

use crate::geo::{km_between, Place, Reach, Route};
use clappkit::{AgentRow, Emit};
use serde_json::{json, Value};

/// Who drove a change. Only a human's action signals — the agent already knows its own
/// writes, and echoing them back would wake it in a loop.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Actor {
    Human,
    Agent,
}

impl Actor {
    pub fn of(caller: &Option<String>) -> Actor {
        if caller.is_some() {
            Actor::Agent
        } else {
            Actor::Human
        }
    }
}

/// Where the shared map is pointed.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub lat: f64,
    pub lon: f64,
    pub zoom: f64,
    /// What this area is called, when we know — filled by a reverse lookup in `app.rs`,
    /// never guessed here.
    pub name: String,
}

impl Default for View {
    /// The whole world, which is the only honest starting point for a map that has not
    /// been told where its human is. No geolocation prompt on first launch: this app has
    /// no business knowing where you are until you tell it.
    fn default() -> View {
        View { lat: 20.0, lon: 0.0, zoom: 1.6, name: String::new() }
    }
}

/// A place the human decided to keep.
#[derive(Clone, Debug, PartialEq)]
pub struct Pin {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub note: String,
}

#[derive(Default)]
pub struct AppState {
    view: View,
    /// What produced the current results, echoed into the window's search box.
    query: String,
    results: Vec<Place>,
    /// Index into `results`, or a place opened directly (`goto`) with no result list.
    selected: Option<Place>,
    route: Option<Route>,
    reach: Option<Reach>,
    pins: Vec<Pin>,
    /// What is in flight, so both surfaces can show the same spinner with the same words.
    busy: Option<String>,
    /// The last thing that happened, in one sentence. Both surfaces show this one string
    /// rather than each inventing its own phrasing.
    said: Option<String>,
    /// The camera the agent was last told about — the baseline [`worth_announcing`] judges
    /// the next move against.
    announced: Option<View>,
    /// The id of the agent that last drove the CLI, so a signal can target it.
    last_agent: Option<String>,
    agents: Vec<AgentRow>,
}

impl AppState {
    pub fn new() -> AppState {
        AppState::default()
    }

    pub fn set_agents(&mut self, rows: Vec<AgentRow>) {
        self.agents = rows;
    }

    pub fn view(&self) -> &View {
        &self.view
    }

    pub fn selected(&self) -> Option<&Place> {
        self.selected.as_ref()
    }

    pub fn results(&self) -> &[Place] {
        &self.results
    }

    pub fn pins(&self) -> &[Pin] {
        &self.pins
    }

    /// Where a search should look first: wherever the map already is, once it is close
    /// enough for "here" to mean anything. Zoomed out to a continent, "airport" has no
    /// local answer and biasing would only add noise.
    pub fn bias(&self) -> Option<[f64; 2]> {
        (self.view.zoom >= 6.0).then_some([self.view.lat, self.view.lon])
    }

    /// How far out "nearby" reaches: half of what is on screen.
    ///
    /// That is the honest reading of the word — somebody looking at one neighbourhood means
    /// this neighbourhood, and somebody looking at a whole city means the city.
    ///
    /// Bounded at both ends, and the upper bound is not cosmetic: `atm` inside 20 km of
    /// Kadıköy is tens of thousands of features and the query timed out, which reads to a
    /// user as "the button is broken". 5 km is the largest radius that answers reliably for
    /// the densest categories, and past that "nearby" is not what anybody meant anyway.
    pub fn radius_m(&self) -> u32 {
        ((screen_km(&self.view) * 1000.0 / 2.0) as u32).clamp(300, 5_000)
    }

    pub fn busy(&mut self, what: Option<&str>) {
        self.busy = what.map(str::to_string);
    }

    pub fn say(&mut self, sentence: impl Into<String>) {
        self.said = Some(sentence.into());
    }

    // ── camera ───────────────────────────────────────────────────────────────

    /// Point the map somewhere, and decide whether that is worth telling the agent about.
    ///
    /// Called for every settled camera (the window debounces its `moveend`), so it is on
    /// the hot path of somebody dragging a map around.
    ///
    /// The signal is `view`, and it is **buffered**: what the human is looking at right now
    /// is not news to be delivered at some later turn, it is the context their next
    /// sentence is about. "What's the name of this street?" only makes sense if the view
    /// rides the prompt that asks it. A queued `context` signal would arrive describing
    /// somewhere they have already left.
    ///
    /// Buffered still does not mean "every twitch" — [`worth_announcing`] is the filter, so
    /// what rides along is the area they settled on, not the eleven they scrolled past.
    pub fn look_at(&mut self, lat: f64, lon: f64, zoom: f64, actor: Actor) -> Vec<Emit> {
        let moved = View { lat, lon, zoom, name: String::new() };
        // A pan does not rename the area; only a lookup does (`name_view`). Keeping the old
        // name until then beats blanking the label on every drag.
        let same_area = !worth_announcing(&self.view, &moved);
        self.view = View { name: if same_area { self.view.name.clone() } else { String::new() }, ..moved };

        if actor == Actor::Agent {
            return Vec::new();
        }
        let baseline = self.announced.clone().unwrap_or_default();
        if !worth_announcing(&baseline, &self.view) {
            return Vec::new();
        }
        self.announced = Some(self.view.clone());
        vec![Emit {
            id: "view".into(),
            target: Vec::new(),
            payload: json!({
                "lat": round6(self.view.lat),
                "lon": round6(self.view.lon),
                "zoom": round2(self.view.zoom),
                "name": self.view.name,
            }),
        }]
    }

    /// Attach a name to the current view (from a reverse lookup done outside).
    pub fn name_view(&mut self, name: String) {
        self.view.name = name;
    }

    /// Frame a place: centred, and zoomed to fit its extent when it has one. A country and
    /// a coffee shop are both "a place" and want very different zooms.
    pub fn frame(&mut self, p: &Place) {
        self.view.lat = p.lat;
        self.view.lon = p.lon;
        self.view.zoom = zoom_for(p);
        self.view.name = p.name.clone();
    }

    // ── results and selection ────────────────────────────────────────────────

    pub fn set_results(&mut self, query: String, places: Vec<Place>) {
        self.query = query;
        self.selected = places.first().cloned();
        self.results = places;
    }

    /// Open one result by its 1-based number, the way both surfaces show them.
    pub fn select(&mut self, n: usize, actor: Actor) -> Result<Place, String> {
        let p = self
            .results
            .get(n.wrapping_sub(1))
            .cloned()
            .ok_or_else(|| match self.results.len() {
                0 => "there are no results to open — search for something first".to_string(),
                len => format!("there are only {len} results; {n} is not one of them"),
            })?;
        self.open(p.clone(), actor);
        Ok(p)
    }

    /// Open a place, wherever it came from.
    ///
    /// The human's side of this is a `buffered` signal: what they opened rides their next
    /// prompt, so they can just say "how far is that from the station?" without repeating
    /// themselves. The agent's own selection signals nothing.
    pub fn open(&mut self, p: Place, actor: Actor) -> Vec<Emit> {
        self.frame(&p);
        self.selected = Some(p.clone());
        if actor == Actor::Agent {
            return Vec::new();
        }
        vec![Emit {
            id: "place.opened".into(),
            target: Vec::new(),
            payload: json!({
                "name": p.name,
                "kind": p.kind,
                "address": p.address,
                "lat": round6(p.lat),
                "lon": round6(p.lon),
            }),
        }]
    }

    pub fn set_route(&mut self, r: Route) {
        self.route = Some(r);
        // A route and a reach are two answers to different questions; showing both at once
        // is two overlapping polygons and no clarity.
        self.reach = None;
    }

    pub fn set_reach(&mut self, r: Reach) {
        self.reach = Some(r);
        self.route = None;
    }

    // ── pins ─────────────────────────────────────────────────────────────────

    /// Keep a place. Pinning the same spot twice updates it rather than stacking a
    /// duplicate on top of itself where nobody can see there are two.
    pub fn pin(&mut self, pin: Pin, actor: Actor) -> Vec<Emit> {
        match self.pins.iter_mut().find(|p| same_spot(p.lat, p.lon, pin.lat, pin.lon)) {
            Some(existing) => *existing = pin,
            None => self.pins.push(pin),
        }
        self.pins_changed(actor)
    }

    /// Remove a pin by 1-based number, or all of them.
    pub fn unpin(&mut self, n: Option<usize>, actor: Actor) -> Result<Vec<Emit>, String> {
        match n {
            None => self.pins.clear(),
            Some(n) => {
                if n == 0 || n > self.pins.len() {
                    return Err(match self.pins.len() {
                        0 => "there are no pins to remove".to_string(),
                        len => format!("there are only {len} pins; {n} is not one of them"),
                    });
                }
                self.pins.remove(n - 1);
            }
        }
        Ok(self.pins_changed(actor))
    }

    fn pins_changed(&self, actor: Actor) -> Vec<Emit> {
        if actor == Actor::Agent {
            return Vec::new();
        }
        vec![Emit {
            id: "pins.changed".into(),
            target: Vec::new(),
            payload: json!({
                "count": self.pins.len(),
                "pins": self.pins.iter().map(|p| json!({
                    "name": p.name, "lat": round6(p.lat), "lon": round6(p.lon), "note": p.note,
                })).collect::<Vec<_>>(),
            }),
        }]
    }

    // ── the rest ─────────────────────────────────────────────────────────────

    /// Clear one layer, or everything except the pins.
    ///
    /// Pins are the only thing here a human deliberately *kept*, so `clear` never takes
    /// them by accident — `clear pins` says it out loud.
    pub fn clear(&mut self, what: &str, actor: Actor) -> Result<(String, Vec<Emit>), String> {
        match what.trim().to_ascii_lowercase().as_str() {
            "results" | "search" => {
                self.results.clear();
                self.query.clear();
                self.selected = None;
                Ok(("cleared the results".into(), Vec::new()))
            }
            "route" => {
                self.route = None;
                Ok(("cleared the route".into(), Vec::new()))
            }
            "reach" | "isochrone" => {
                self.reach = None;
                Ok(("cleared the reachable area".into(), Vec::new()))
            }
            "pins" => {
                let n = self.pins.len();
                self.pins.clear();
                Ok((format!("cleared {n} pin{}", plural(n)), self.pins_changed(actor)))
            }
            "" | "all" => {
                self.results.clear();
                self.query.clear();
                self.selected = None;
                self.route = None;
                self.reach = None;
                Ok(("cleared the map — the pins are still there".into(), Vec::new()))
            }
            other => Err(format!(
                "clear what? results, route, reach, pins, or all — not \"{other}\""
            )),
        }
    }

    // ── what survives a restart ──────────────────────────────────────────────

    /// The two things it would be rude to forget: where the human had the map, and what
    /// they kept.
    ///
    /// Results and routes are deliberately NOT here — they are answers to a question asked
    /// at a moment, and restoring a stale route would be pretending to know something.
    /// The view is different: reopening the app in the mid-Atlantic when you were last
    /// looking at Kadıköy is not a fresh start, it is amnesia. It also made every category
    /// button fail on launch, because "what is nearby" has no answer in the middle of an
    /// ocean.
    ///
    /// Pure: the file I/O belongs to `app.rs`, so these stay testable.
    pub fn saved(&self) -> Value {
        json!({
            "view": {
                "lat": round6(self.view.lat),
                "lon": round6(self.view.lon),
                "zoom": round2(self.view.zoom),
                "name": self.view.name,
            },
            "pins": self.pins.iter().map(|p| json!({
                "name": p.name, "lat": p.lat, "lon": p.lon, "note": p.note,
            })).collect::<Vec<_>>(),
        })
    }

    /// Take back what [`saved`](Self::saved) wrote. Anything missing or malformed is
    /// skipped rather than fatal — a corrupt settings file must not cost you the app.
    pub fn restore(&mut self, v: &Value) {
        if let Some(view) = v.get("view") {
            let f = |k: &str| view.get(k).and_then(Value::as_f64);
            if let (Some(lat), Some(lon), Some(zoom)) = (f("lat"), f("lon"), f("zoom")) {
                if (-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon) {
                    self.view = View {
                        lat,
                        lon,
                        zoom: zoom.clamp(0.0, 22.0),
                        name: view.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                    };
                }
            }
        }
        if let Some(pins) = v.get("pins").and_then(Value::as_array) {
            self.pins = pins
                .iter()
                .filter_map(|p| {
                    Some(Pin {
                        name: p.get("name")?.as_str()?.to_string(),
                        lat: p.get("lat")?.as_f64()?,
                        lon: p.get("lon")?.as_f64()?,
                        note: p.get("note").and_then(Value::as_str).unwrap_or("").to_string(),
                    })
                })
                .collect();
        }
    }

    pub fn remember_agent(&mut self, caller: &Option<String>) {
        if caller.is_some() {
            self.last_agent = caller.clone();
        }
    }

    /// Everything on the map as GeoJSON — the one export format every other tool reads.
    pub fn geojson(&self) -> Value {
        let mut features: Vec<Value> = Vec::new();
        for p in &self.pins {
            features.push(point(p.lon, p.lat, json!({ "kind": "pin", "name": p.name, "note": p.note })));
        }
        for p in &self.results {
            features.push(point(
                p.lon,
                p.lat,
                json!({ "kind": "result", "name": p.name, "type": p.kind, "address": p.address }),
            ));
        }
        if let Some(r) = &self.route {
            features.push(json!({
                "type": "Feature",
                "geometry": { "type": "LineString", "coordinates": r.shape },
                "properties": {
                    "kind": "route", "mode": r.mode, "km": round2(r.km),
                    "minutes": round1(r.secs / 60.0), "from": r.from, "to": r.to,
                },
            }));
        }
        if let Some(r) = &self.reach {
            features.push(json!({
                "type": "Feature",
                "geometry": { "type": "Polygon", "coordinates": [r.ring] },
                "properties": {
                    "kind": "reach", "minutes": r.minutes, "mode": r.mode, "from": r.from,
                },
            }));
        }
        json!({ "type": "FeatureCollection", "features": features })
    }

    /// GPX, for the handheld devices and the route planners that only speak it. A route
    /// becomes a track; pins become waypoints.
    pub fn gpx(&self) -> String {
        let mut s = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <gpx version=\"1.1\" creator=\"maps-clapp\" xmlns=\"http://www.topografix.com/GPX/1/1\">\n",
        );
        for p in &self.pins {
            s.push_str(&format!(
                "  <wpt lat=\"{:.6}\" lon=\"{:.6}\"><name>{}</name></wpt>\n",
                p.lat,
                p.lon,
                xml(&p.name)
            ));
        }
        if let Some(r) = &self.route {
            s.push_str(&format!(
                "  <trk><name>{} to {}</name><trkseg>\n",
                xml(&r.from),
                xml(&r.to)
            ));
            for pt in &r.shape {
                s.push_str(&format!("    <trkpt lat=\"{:.6}\" lon=\"{:.6}\"/>\n", pt[1], pt[0]));
            }
            s.push_str("  </trkseg></trk>\n");
        }
        s.push_str("</gpx>\n");
        s
    }

    pub fn snapshot(&self) -> Value {
        clappkit::snapshot::with_rev(json!({
            "ok": true,
            "view": {
                "lat": round6(self.view.lat),
                "lon": round6(self.view.lon),
                "zoom": round2(self.view.zoom),
                "name": self.view.name,
            },
            "query": self.query,
            "results": self.results,
            "selected": self.selected,
            "route": self.route,
            "reach": self.reach,
            "pins": self.pins.iter().map(|p| json!({
                "name": p.name, "lat": round6(p.lat), "lon": round6(p.lon), "note": p.note,
            })).collect::<Vec<_>>(),
            "busy": self.busy,
            "said": self.said,
            "agents": self.agents.iter().map(|a| json!({
                "id": a.id, "name": a.name, "avatar": a.avatar,
            })).collect::<Vec<_>>(),
        }))
    }
}

/// Is this camera move a different *place*, or the same one seen slightly differently?
///
/// The test is relative to what is on screen: half a screen-width of panning, or two zoom
/// levels, is a new view; nudging the map to see round a label is not. Using a fixed
/// distance instead would be wrong at both ends — 5 km is nothing at zoom 3 and a different
/// city at zoom 16.
fn worth_announcing(before: &View, after: &View) -> bool {
    if (before.zoom - after.zoom).abs() >= 2.0 {
        return true;
    }
    km_between([before.lat, before.lon], [after.lat, after.lon]) > screen_km(after) / 2.0
}

/// Roughly how many kilometres of world fit across the window at this zoom.
fn screen_km(v: &View) -> f64 {
    const EQUATOR_KM: f64 = 40_075.0;
    // Web-Mercator: the whole world is 2^zoom tiles across, and a window is a few tiles
    // wide. The constant does not need to be exact — it sets a threshold, not a scale bar.
    (EQUATOR_KM * v.lat.to_radians().cos().abs().max(0.05)) / 2f64.powf(v.zoom)
}

/// How close to get to a place. A place with an extent gets framed by it; a place without
/// one is a point, and a point is a building.
fn zoom_for(p: &Place) -> f64 {
    let Some([w, s, e, n]) = p.extent else { return 16.0 };
    let span = (e - w).abs().max((n - s).abs());
    if span <= 0.0 {
        return 16.0;
    }
    // 360° of longitude is zoom 0; each halving is one level. The -1 leaves a margin so
    // the thing you asked for is not touching the window edge.
    (360.0f64 / span).log2().floor().clamp(2.0, 17.0) - 1.0
}

/// Two coordinates close enough to be the same pin — about 10 metres.
fn same_spot(a_lat: f64, a_lon: f64, b_lat: f64, b_lon: f64) -> bool {
    (a_lat - b_lat).abs() < 1e-4 && (a_lon - b_lon).abs() < 1e-4
}

fn point(lon: f64, lat: f64, properties: Value) -> Value {
    json!({
        "type": "Feature",
        "geometry": { "type": "Point", "coordinates": [round6(lon), round6(lat)] },
        "properties": properties,
    })
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

pub fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

/// Six decimals is about 10 cm — past that a coordinate is noise pretending to be data,
/// and it makes every snapshot diff look like a change.
fn round6(v: f64) -> f64 {
    (v * 1e6).round() / 1e6
}

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geo::Mode;

    fn place(name: &str, lat: f64, lon: f64) -> Place {
        Place { id: name.into(), name: name.into(), lat, lon, ..Place::default() }
    }

    fn city(name: &str, lat: f64, lon: f64, span: f64) -> Place {
        Place {
            extent: Some([lon - span / 2.0, lat - span / 2.0, lon + span / 2.0, lat + span / 2.0]),
            ..place(name, lat, lon)
        }
    }

    #[test]
    fn both_surfaces_reach_the_same_state() {
        let mut s = AppState::new();
        s.open(place("Eiffel Tower", 48.8584, 2.2945), Actor::Human);
        assert_eq!(s.snapshot()["selected"]["name"], json!("Eiffel Tower"));
        s.open(place("Louvre", 48.8606, 2.3376), Actor::Agent);
        assert_eq!(s.snapshot()["selected"]["name"], json!("Louvre"));
        assert_eq!(s.snapshot()["view"]["lat"], json!(48.8606), "the camera follows the selection");
    }

    /// The rule that keeps an agent from waking itself: only a human's action signals.
    #[test]
    fn only_a_human_action_signals() {
        let mut s = AppState::new();
        assert_eq!(s.open(place("a", 1.0, 1.0), Actor::Human).len(), 1);
        assert!(s.open(place("b", 2.0, 2.0), Actor::Agent).is_empty());
        assert_eq!(s.pin(Pin { name: "p".into(), lat: 1.0, lon: 1.0, note: String::new() }, Actor::Human).len(), 1);
        assert!(s.pin(Pin { name: "q".into(), lat: 9.0, lon: 9.0, note: String::new() }, Actor::Agent).is_empty());
        assert!(s.look_at(10.0, 10.0, 12.0, Actor::Agent).is_empty());
    }

    #[test]
    fn opening_a_place_rides_the_humans_next_prompt() {
        let mut s = AppState::new();
        let e = s.open(place("Shibuya Crossing", 35.6595, 139.7005), Actor::Human);
        assert_eq!(e[0].id, "place.opened");
        assert_eq!(e[0].payload["name"], json!("Shibuya Crossing"));
    }

    // ── the camera ───────────────────────────────────────────────────────────

    /// A human nudging the map must not produce a notification per nudge. This is the whole
    /// reason the camera is treated differently from every other piece of state.
    #[test]
    fn small_pans_are_not_announced_but_a_new_city_is() {
        let mut s = AppState::new();
        // Arrive in Paris. The first move is always news — there is no baseline yet.
        assert_eq!(s.look_at(48.8584, 2.2945, 14.0, Actor::Human).len(), 1);
        // Now nudge around the neighbourhood: a few hundred metres at zoom 14.
        assert!(s.look_at(48.8590, 2.2950, 14.0, Actor::Human).is_empty());
        assert!(s.look_at(48.8570, 2.2960, 14.0, Actor::Human).is_empty());
        assert!(s.look_at(48.8600, 2.2930, 14.0, Actor::Human).is_empty());
        // Cross town to the Louvre — at zoom 14 that is most of a screen away.
        assert_eq!(s.look_at(48.8606, 2.3376, 14.0, Actor::Human).len(), 1);
    }

    #[test]
    fn zooming_right_out_is_a_new_view_even_without_panning() {
        let mut s = AppState::new();
        s.look_at(48.8584, 2.2945, 14.0, Actor::Human);
        assert!(s.look_at(48.8584, 2.2945, 13.0, Actor::Human).is_empty(), "one level is a nudge");
        assert_eq!(s.look_at(48.8584, 2.2945, 11.0, Actor::Human).len(), 1, "three levels is a new view");
    }

    /// The same absolute distance is a nudge on a world map and a different town up close.
    #[test]
    fn the_threshold_scales_with_the_zoom() {
        let far = View { lat: 48.0, lon: 2.0, zoom: 4.0, name: String::new() };
        let near = View { lat: 48.0, lon: 2.0, zoom: 15.0, name: String::new() };
        let moved = |v: &View, dlat: f64| View { lat: v.lat + dlat, ..v.clone() };
        // ~11 km north.
        assert!(!worth_announcing(&far, &moved(&far, 0.1)), "11 km at zoom 4 is nothing");
        assert!(worth_announcing(&near, &moved(&near, 0.1)), "11 km at zoom 15 is elsewhere");
    }

    #[test]
    fn the_announced_baseline_is_the_last_thing_the_agent_heard() {
        let mut s = AppState::new();
        s.look_at(48.8584, 2.2945, 14.0, Actor::Human);
        // Three nudges that each fall under the threshold, but together add up past it.
        // Judging against the last *announced* view rather than the last one seen is what
        // makes the drift get reported instead of vanishing a nudge at a time.
        s.look_at(48.8620, 2.3050, 14.0, Actor::Human);
        s.look_at(48.8650, 2.3150, 14.0, Actor::Human);
        let e = s.look_at(48.8680, 2.3250, 14.0, Actor::Human);
        assert_eq!(e.len(), 1, "accumulated drift must eventually be announced");
    }

    #[test]
    fn a_place_is_framed_by_its_extent_and_a_point_is_a_building() {
        let mut s = AppState::new();
        s.frame(&city("France", 46.6, 2.2, 10.0));
        let wide = s.view().zoom;
        s.frame(&city("Paris", 48.85, 2.35, 0.3));
        let city_z = s.view().zoom;
        s.frame(&place("Eiffel Tower", 48.8584, 2.2945));
        let point_z = s.view().zoom;
        assert!(wide < city_z, "a country must be further out than a city ({wide} vs {city_z})");
        assert!(city_z < point_z, "a city must be further out than a building");
        assert_eq!(point_z, 16.0);
    }

    // ── results, pins, clearing ──────────────────────────────────────────────

    #[test]
    fn selecting_out_of_range_says_what_is_actually_there() {
        let mut s = AppState::new();
        assert!(s.select(1, Actor::Human).unwrap_err().contains("no results"));
        s.set_results("cafe".into(), vec![place("a", 1.0, 1.0), place("b", 2.0, 2.0)]);
        assert_eq!(s.select(2, Actor::Human).unwrap().name, "b");
        assert!(s.select(7, Actor::Human).unwrap_err().contains("only 2"));
        assert!(s.select(0, Actor::Human).is_err(), "the numbering both surfaces show is 1-based");
    }

    #[test]
    fn a_search_selects_its_first_result_so_neither_surface_shows_an_empty_panel() {
        let mut s = AppState::new();
        s.set_results("cafe".into(), vec![place("First", 1.0, 1.0), place("Second", 2.0, 2.0)]);
        assert_eq!(s.selected().unwrap().name, "First");
    }

    #[test]
    fn pinning_the_same_spot_twice_updates_it() {
        let mut s = AppState::new();
        let p = |name: &str| Pin { name: name.into(), lat: 41.0082, lon: 28.9784, note: String::new() };
        s.pin(p("Sultanahmet"), Actor::Human);
        s.pin(p("Blue Mosque"), Actor::Human);
        assert_eq!(s.pins().len(), 1, "the same spot must not stack invisibly");
        assert_eq!(s.pins()[0].name, "Blue Mosque");
    }

    #[test]
    fn clearing_never_takes_the_pins_by_accident() {
        let mut s = AppState::new();
        s.set_results("cafe".into(), vec![place("a", 1.0, 1.0)]);
        s.pin(Pin { name: "home".into(), lat: 5.0, lon: 5.0, note: String::new() }, Actor::Human);
        s.clear("all", Actor::Human).unwrap();
        assert!(s.results().is_empty());
        assert_eq!(s.pins().len(), 1, "`clear all` must not take what the human kept");
        s.clear("pins", Actor::Human).unwrap();
        assert!(s.pins().is_empty(), "`clear pins` says it out loud, so it may");
    }

    #[test]
    fn clearing_something_that_is_not_a_layer_is_refused_with_the_list() {
        let e = AppState::new().clear("everything", Actor::Human).unwrap_err();
        assert!(e.contains("results") && e.contains("pins"), "the refusal must teach: {e}");
    }

    #[test]
    fn a_route_and_a_reachable_area_do_not_share_the_screen() {
        let mut s = AppState::new();
        s.set_reach(Reach {
            minutes: 10,
            mode: Mode::Walk,
            center: [2.0, 48.0],
            from: "here".into(),
            ring: vec![[0.0, 0.0]; 4],
        });
        assert!(s.snapshot()["reach"].is_object());
        s.set_route(Route {
            mode: Mode::Drive,
            km: 4.2,
            secs: 830.0,
            from: "a".into(),
            to: "b".into(),
            shape: vec![[2.0, 48.0], [2.1, 48.1]],
            steps: vec![],
        });
        assert!(s.snapshot()["reach"].is_null(), "the older answer must go");
    }

    // ── export ───────────────────────────────────────────────────────────────

    #[test]
    fn the_export_carries_every_layer_in_geojson_order() {
        let mut s = AppState::new();
        s.pin(Pin { name: "home".into(), lat: 48.0, lon: 2.0, note: "n".into() }, Actor::Human);
        s.set_results("cafe".into(), vec![place("Cafe", 48.1, 2.1)]);
        s.set_route(Route {
            mode: Mode::Bike,
            km: 1.0,
            secs: 300.0,
            from: "a".into(),
            to: "b".into(),
            shape: vec![[2.0, 48.0], [2.1, 48.1]],
            steps: vec![],
        });
        let g = s.geojson();
        let f = g["features"].as_array().unwrap();
        assert_eq!(f.len(), 3);
        // GeoJSON is [lon, lat] — the mistake that puts Paris in South Sudan.
        assert_eq!(f[0]["geometry"]["coordinates"], json!([2.0, 48.0]));
        assert_eq!(f[2]["geometry"]["type"], json!("LineString"));
    }

    #[test]
    fn gpx_escapes_names_rather_than_breaking_the_document() {
        let mut s = AppState::new();
        s.pin(Pin { name: "Bed & Breakfast <2>".into(), lat: 1.0, lon: 2.0, note: String::new() }, Actor::Human);
        let x = s.gpx();
        assert!(x.contains("Bed &amp; Breakfast &lt;2&gt;"), "{x}");
        assert!(x.starts_with("<?xml"));
    }

    // ── what survives a restart ──────────────────────────────────────────────

    #[test]
    fn the_view_and_the_pins_come_back_but_the_search_does_not() {
        let mut a = AppState::new();
        a.look_at(40.9906, 29.0217, 14.0, Actor::Human);
        a.name_view("Kadıköy".into());
        a.pin(Pin { name: "home".into(), lat: 41.0, lon: 29.0, note: "n".into() }, Actor::Human);
        a.set_results("cafe".into(), vec![place("Cafe", 41.0, 29.0)]);

        let mut b = AppState::new();
        b.restore(&a.saved());
        assert_eq!(b.view().name, "Kadıköy");
        assert!((b.view().lat - 40.9906).abs() < 1e-6);
        assert_eq!(b.view().zoom, 14.0);
        assert_eq!(b.pins().len(), 1);
        assert_eq!(b.pins()[0].note, "n");
        // A restored search would be an answer to a question asked at some other time.
        assert!(b.results().is_empty());
    }

    /// The reason this exists at all: a fresh launch in the mid-Atlantic makes every
    /// category button fail, because "what is nearby" has no answer in an ocean.
    #[test]
    fn a_restored_session_can_answer_nearby_immediately() {
        let mut a = AppState::new();
        assert!(a.bias().is_none(), "the default view is nowhere in particular");
        a.look_at(40.9906, 29.0217, 14.0, Actor::Human);

        let mut b = AppState::new();
        b.restore(&a.saved());
        assert!(b.bias().is_some(), "a restored session is somewhere");
    }

    #[test]
    fn a_corrupt_session_file_costs_nothing() {
        let mut s = AppState::new();
        let before = s.view().clone();
        s.restore(&json!({ "view": { "lat": "north", "lon": null }, "pins": "lots" }));
        assert_eq!(s.view(), &before, "garbage must be ignored, not adopted");
        s.restore(&json!({ "view": { "lat": 900.0, "lon": 0.0, "zoom": 5.0 } }));
        assert_eq!(s.view(), &before, "there is no latitude 900");
        s.restore(&json!({}));
        assert_eq!(s.view(), &before);
    }

    #[test]
    fn every_snapshot_is_newer_than_the_last() {
        let s = AppState::new();
        let a = s.snapshot()["rev"].as_i64().unwrap();
        assert!(s.snapshot()["rev"].as_i64().unwrap() > a);
    }

    #[test]
    fn a_world_view_does_not_bias_a_search_but_a_city_view_does() {
        let mut s = AppState::new();
        assert!(s.bias().is_none(), "at world zoom, \"airport\" has no local answer");
        s.look_at(35.68, 139.76, 12.0, Actor::Human);
        assert_eq!(s.bias(), Some([35.68, 139.76]));
    }
}
