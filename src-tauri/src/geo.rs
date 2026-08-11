//! The world, spoken to over four open services.
//!
//! All four are HTTPS + JSON, so this is Rust and nothing else — no sidecar, no vendored
//! runtime (the playbook's rule §2). None of them takes an API key, which is the whole
//! reason they were chosen: this app works on first launch with no account to create, no
//! card to enter and no quota to watch.
//!
//!   search   GET  photon.komoot.io/api            typeahead by name, reverse
//!   precise  GET  nominatim.openstreetmap.org     exact addresses, used only as fallback
//!   nearby   POST overpass-api.de/api/interpreter everything of a kind within a radius
//!   routing  POST valhalla1.openstreetmap.de      turn-by-turn, isochrones (FOSSGIS)
//!
//! Search and nearby are deliberately two different services, because they are two
//! different questions — see [`Geo::nearby`], which is where an index stops being the
//! right tool.
//!
//! **These servers belong to other people.** Photon is komoot's demo ("fair use, extensive
//! usage will be throttled"), Nominatim's policy is a hard 1 request/second with a
//! mandatory identifying User-Agent and an explicit ban on wiring it to a typeahead,
//! Overpass runs a real query per call and is the most expensive thing here by far, and
//! FOSSGIS asks published apps to identify themselves with `X-Client-Id`. So every call
//! here goes through a [`Service`]: a gate that keeps a floor between requests, a TTL cache
//! that answers repeats for free, and a per-key lock so two surfaces asking the same
//! question at the same moment produce one request, not two. The rate has a ceiling no
//! caller can raise — not the window, not the agent, not both at once.
//!
//! The provider seam is deliberate. [`Geocoder`] and [`Router`] are traits with one
//! implementation each today; the day FOSSGIS closes its door or komoot asks us to stop, a
//! replacement is a new struct in this file and nothing else in the app moves. That is the
//! lesson from Ferrostar, which is a navigation SDK that pointedly *is not* a routing
//! engine, basemap or search service — it orchestrates whoever provides them.

use anyhow::{anyhow, bail, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const PHOTON: &str = "https://photon.komoot.io";
const NOMINATIM: &str = "https://nominatim.openstreetmap.org";
const VALHALLA: &str = "https://valhalla1.openstreetmap.de";
/// Overpass, in the order they are tried. The main instance answers "busy" (429 or 504)
/// often enough that treating that as a failure of the app would be wrong — it is the
/// normal state of a free service running real queries for everybody. kumi.systems runs a
/// public mirror of the same API, so a busy answer costs a second attempt, not the feature.
const OVERPASS: [&str; 2] = [
    "https://overpass-api.de/api/interpreter",
    "https://overpass.kumi.systems/api/interpreter",
];

/// Who we are, on every request. Nominatim's policy makes this mandatory and rejects
/// library defaults; the others simply deserve to know who is calling.
const UA: &str = concat!(
    "maps-clapp/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/arfium/maps-clapp)"
);

/// FOSSGIS asks apps that ship to end users to identify themselves on Valhalla calls.
const CLIENT_ID: &str = "maps-clapp";

/// Long enough for a cold routing graph, short enough that a wedged call cannot hold the
/// agent's turn (or the window's spinner) open indefinitely.
const TIMEOUT: Duration = Duration::from_secs(20);

/// How many results a search keeps. One screenful the human can actually read, and the
/// same number the agent is told about — a caller's `-n` trims what the *terminal* prints
/// and never the shared set.
pub const RESULT_LIMIT: usize = 40;

/// The floor between two requests to each service, in the units their operators asked for.
///
/// Nominatim's is the one that is not ours to choose: their usage policy says one request
/// per second, so this is 1.1s — the honest reading of a limit measured on someone else's
/// clock. The other two are fair-use, and these gaps keep a burst (a human dragging the map
/// while an agent loops over queries) from ever becoming one.
const PHOTON_GAP: Duration = Duration::from_millis(300);
const NOMINATIM_GAP: Duration = Duration::from_millis(1100);
const VALHALLA_GAP: Duration = Duration::from_millis(400);
/// Overpass answers by running a query against the live OSM database, so one call there
/// costs a great deal more than one call anywhere else here. It is the widest gap for that
/// reason, and `nearby` is the only thing that ever uses it.
const OVERPASS_GAP: Duration = Duration::from_millis(1500);
/// Overpass's own budget, and the client's. The query asks the server for `OVERPASS_BUDGET`
/// seconds and we wait a little longer than that, so a slow answer is still an answer —
/// giving up first would mean paying for the work and discarding it.
const OVERPASS_BUDGET: u32 = 25;
const OVERPASS_TIMEOUT: Duration = Duration::from_secs(30);

/// How long an answer stays good. Places move on the scale of months, so a repeat search
/// inside ten minutes is free; an address for a fixed coordinate is good for hours; a route
/// has no live traffic in it (see [`Route`]), so it is as good in ten minutes as it is now.
const SEARCH_TTL: Duration = Duration::from_secs(10 * 60);
const REVERSE_TTL: Duration = Duration::from_secs(6 * 60 * 60);
const ROUTE_TTL: Duration = Duration::from_secs(10 * 60);

// ─────────────────────────────────────────────────────────────────────────────
// What the rest of the app sees
// ─────────────────────────────────────────────────────────────────────────────

/// A place on earth: what it is called, what kind of thing it is, and where.
///
/// Deliberately flat and provider-agnostic — nothing downstream should be able to tell
/// whether this came from Photon, Nominatim or a future replacement.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Place {
    /// Stable across searches for the same feature, so a selection survives a re-query.
    pub id: String,
    pub name: String,
    /// A human phrase, not a tag: "cafe", "railway station", "peak".
    pub kind: String,
    /// One address line, already composed and de-duplicated.
    pub address: String,
    pub lat: f64,
    pub lon: f64,
    pub country: String,
    /// `[west, south, east, north]` when the source knows the feature's extent. A city
    /// wants to be framed, not centred at zoom 18.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extent: Option<[f64; 4]>,
}

impl Place {
    pub fn label(&self) -> String {
        if self.address.is_empty() {
            self.name.clone()
        } else {
            format!("{} — {}", self.name, self.address)
        }
    }
}

/// How to travel. Valhalla calls these "costing models"; the app calls them what a person
/// would.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Drive,
    Bike,
    Walk,
}

impl Mode {
    pub fn costing(self) -> &'static str {
        match self {
            Mode::Drive => "auto",
            Mode::Bike => "bicycle",
            Mode::Walk => "pedestrian",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::Drive => "driving",
            Mode::Bike => "cycling",
            Mode::Walk => "walking",
        }
    }

    /// Forgiving on purpose: an agent writing `--mode car` or `--mode foot` means something
    /// obvious, and refusing it teaches nothing.
    pub fn parse(s: &str) -> Option<Mode> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" => None,
            "drive" | "car" | "auto" | "driving" => Some(Mode::Drive),
            "bike" | "bicycle" | "cycling" | "cycle" => Some(Mode::Bike),
            "walk" | "foot" | "pedestrian" | "walking" | "hike" => Some(Mode::Walk),
            _ => None,
        }
    }
}

/// One instruction on a route.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    /// Valhalla writes these as whole sentences ("Turn left onto Pont de la Concorde."), so
    /// they are passed through rather than rebuilt from a maneuver code.
    pub instruction: String,
    pub km: f64,
    pub secs: f64,
    /// Index into [`Route::shape`] where this step begins — that is how the window
    /// highlights the leg you hover.
    pub at: usize,
}

/// A way from one place to another.
///
/// **The time in here has no live traffic in it.** No open routing service has that, and
/// pretending otherwise would be the one lie a map must not tell — so the number is a
/// free-flow estimate and both surfaces say so.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Route {
    pub mode: Mode,
    pub km: f64,
    pub secs: f64,
    /// Every stop's name, in order — two for a simple route, more for a trip.
    pub stops: Vec<String>,
    /// `[lon, lat]` pairs, in GeoJSON order so the frontend can hand it straight to a
    /// `LineString` without transposing anything.
    pub shape: Vec<[f64; 2]>,
    /// One per consecutive pair of stops. A two-stop route has one leg, and everything
    /// downstream can stop caring how many stops there were.
    pub legs: Vec<Leg>,
}

impl Route {
    pub fn from(&self) -> &str {
        self.stops.first().map(String::as_str).unwrap_or("")
    }

    pub fn to(&self) -> &str {
        self.stops.last().map(String::as_str).unwrap_or("")
    }

    pub fn steps(&self) -> usize {
        self.legs.iter().map(|l| l.steps.len()).sum()
    }
}

/// One hop of a trip: the stretch between two consecutive stops.
///
/// A route between two places has exactly one of these, so a trip is not a special case of
/// a route — a route is a trip with one leg.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Leg {
    pub from: String,
    pub to: String,
    pub km: f64,
    pub secs: f64,
    /// Index into [`Route::shape`] where this leg begins — how the window highlights the
    /// leg being walked without a second geometry.
    pub at: usize,
    pub steps: Vec<Step>,
}

/// How far you can get from a point in N minutes.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reach {
    pub minutes: u32,
    pub mode: Mode,
    pub center: [f64; 2],
    pub from: String,
    /// A single outer ring of `[lon, lat]`, GeoJSON order.
    pub ring: Vec<[f64; 2]>,
}

/// Where a search should look first. A search for "airport" means a different thing in
/// Tokyo than in Toronto, and the map already knows which one the human is looking at.
pub type Bias = Option<[f64; 2]>;

// ─────────────────────────────────────────────────────────────────────────────
// The seam
// ─────────────────────────────────────────────────────────────────────────────

/// Turning words into places, and coordinates back into words.
#[allow(async_fn_in_trait)]
pub trait Geocoder {
    /// Free-text search. `tag` narrows to an OSM `key:value` when the caller wants a
    /// category rather than a name.
    async fn search(&self, q: &str, bias: Bias, tag: Option<&str>, limit: usize)
        -> Result<Vec<Place>>;

    /// What is at this coordinate.
    async fn reverse(&self, lat: f64, lon: f64) -> Result<Option<Place>>;
}

/// Getting from one coordinate to another.
#[allow(async_fn_in_trait)]
pub trait Router {
    /// Two stops or twenty — the router takes the whole trip, because splitting it into
    /// pairs and stitching the answers back together would give a different (and worse)
    /// route than asking once. A router optimises across the stops it is told about.
    async fn route(&self, stops: &[Place], mode: Mode) -> Result<Route>;
    /// Reorder the middle stops for the shortest journey (first and last stay fixed), and
    /// answer with the stops in their new order plus the route through them — one request,
    /// because the optimiser's answer IS a route and asking again would waste the work.
    async fn optimize(&self, stops: &[Place], mode: Mode) -> Result<(Vec<Place>, Route)>;
    async fn reach(&self, at: &Place, minutes: u32, mode: Mode) -> Result<Reach>;
}

// ─────────────────────────────────────────────────────────────────────────────
// Rate discipline
// ─────────────────────────────────────────────────────────────────────────────

/// Serialises callers and keeps a minimum gap between them. Held across the wait on
/// purpose: the queue IS the rate limit.
struct Gate {
    last: Mutex<Option<Instant>>,
    gap: Duration,
}

impl Gate {
    fn new(gap: Duration) -> Gate {
        Gate { last: Mutex::new(None), gap }
    }

    async fn pass(&self) {
        let mut last = self.last.lock().await;
        if let Some(t) = *last {
            let since = t.elapsed();
            if since < self.gap {
                tokio::time::sleep(self.gap - since).await;
            }
        }
        *last = Some(Instant::now());
    }
}

/// One service's manners: its gate, its cache, and one lock per outstanding question.
///
/// The raw JSON is what gets cached rather than the parsed type, so one cache serves all
/// three services and a parser change never has to think about invalidation.
struct Service {
    http: reqwest::Client,
    gate: Gate,
    ttl: Duration,
    cache: Mutex<HashMap<String, (Instant, Value)>>,
    flights: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl Service {
    fn new(gap: Duration, ttl: Duration) -> Result<Service> {
        Service::with_timeout(gap, ttl, TIMEOUT)
    }

    fn with_timeout(gap: Duration, ttl: Duration, timeout: Duration) -> Result<Service> {
        Ok(Service {
            http: reqwest::Client::builder().timeout(timeout).user_agent(UA).build()?,
            gate: Gate::new(gap),
            ttl,
            cache: Mutex::new(HashMap::new()),
            flights: Mutex::new(HashMap::new()),
        })
    }

    async fn cached(&self, key: &str) -> Option<Value> {
        let mut c = self.cache.lock().await;
        match c.get(key) {
            Some((at, v)) if at.elapsed() < self.ttl => Some(v.clone()),
            Some(_) => {
                c.remove(key);
                None
            }
            None => None,
        }
    }

    /// Answer `key`, fetching with `f` only if nobody else already has.
    ///
    /// The double check is the whole point: the second caller waits on the per-key lock,
    /// and by the time it gets in, the first caller's answer is in the cache — so two
    /// surfaces asking the same question at the same moment cost one request.
    async fn get<F, Fut>(&self, key: String, f: F) -> Result<Value>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Value>>,
    {
        if let Some(v) = self.cached(&key).await {
            return Ok(v);
        }
        let flight = {
            let mut m = self.flights.lock().await;
            m.entry(key.clone()).or_insert_with(|| Arc::new(Mutex::new(()))).clone()
        };
        let _held = flight.lock().await;
        if let Some(v) = self.cached(&key).await {
            return Ok(v);
        }

        self.gate.pass().await;
        let out = f().await;
        // The flight entry goes whichever way the fetch went: leaving it behind on an
        // error would grow the map by one dead lock per failed question, forever.
        self.flights.lock().await.remove(&key);
        let v = out?;
        let mut cache = self.cache.lock().await;
        // A long session asks thousands of different questions; entries only left when
        // re-asked, so sweep the expired ones once the map gets big. O(n), rare.
        if cache.len() >= 512 {
            cache.retain(|_, (at, _)| at.elapsed() < self.ttl);
        }
        cache.insert(key, (Instant::now(), v.clone()));
        Ok(v)
    }
}

/// Turn a transport error into a sentence a person can act on. `reqwest`'s own Display is
/// a URL and a chain of causes, which tells a user nothing they can do about it.
fn transport(service: &str, e: reqwest::Error) -> anyhow::Error {
    if e.is_timeout() {
        anyhow!("{service} did not answer within {}s", TIMEOUT.as_secs())
    } else if e.is_connect() {
        anyhow!("cannot reach {service} — is this machine online?")
    } else {
        anyhow!("{service}: {e}")
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Photon — the search everything goes through
// ─────────────────────────────────────────────────────────────────────────────

/// komoot's Photon: OpenStreetMap in an OpenSearch index, built for search-as-you-type.
///
/// It is the typeahead *because* Nominatim may not be: their usage policy bans wiring a
/// public Nominatim to an autocomplete in as many words.
pub struct Photon {
    search: Service,
    reverse: Service,
}

impl Photon {
    pub fn new() -> Result<Photon> {
        Ok(Photon {
            search: Service::new(PHOTON_GAP, SEARCH_TTL)?,
            reverse: Service::new(PHOTON_GAP, REVERSE_TTL)?,
        })
    }
}

impl Geocoder for Photon {
    async fn search(
        &self,
        q: &str,
        bias: Bias,
        tag: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Place>> {
        let q = q.trim();
        if q.is_empty() {
            bail!("search for what? give me something to look up");
        }
        // Ask for more than we keep: `nearby` re-orders by distance below, and ordering a
        // truncated list would just be ordering the wrong list.
        let want = limit.clamp(1, RESULT_LIMIT).max(if tag.is_some() { RESULT_LIMIT } else { 1 });
        let mut url = format!("{PHOTON}/api/?q={}&limit={}", enc(q), want);
        if let Some([lat, lon]) = bias {
            url.push_str(&format!("&lat={lat:.5}&lon={lon:.5}"));
        }
        if let Some(t) = tag {
            url.push_str(&format!("&osm_tag={}", enc(t)));
        }

        let http = &self.search.http;
        let body = self
            .search
            .get(url.clone(), move || async move {
                http.get(&url)
                    .send()
                    .await
                    .map_err(|e| transport("the search service", e))?
                    .error_for_status()
                    .map_err(|e| transport("the search service", e))?
                    .json::<Value>()
                    .await
                    .map_err(|e| transport("the search service", e))
            })
            .await?;

        Ok(features(&body).iter().filter_map(photon_place).take(limit).collect())
    }

    async fn reverse(&self, lat: f64, lon: f64) -> Result<Option<Place>> {
        // Rounded to four decimals — about 11 metres — on purpose: the URL *is* the cache
        // key, and a camera that settles a few metres away is the same question. A map
        // being dragged asks it constantly.
        let url = format!("{PHOTON}/reverse?lat={lat:.4}&lon={lon:.4}");
        let http = &self.reverse.http;
        let body = self
            .reverse
            .get(url.clone(), move || async move {
                http.get(&url)
                    .send()
                    .await
                    .map_err(|e| transport("the search service", e))?
                    .error_for_status()
                    .map_err(|e| transport("the search service", e))?
                    .json::<Value>()
                    .await
                    .map_err(|e| transport("the search service", e))
            })
            .await?;
        Ok(features(&body).first().and_then(photon_place))
    }
}

/// Photon answers GeoJSON; everything we want is one `features` array deep.
fn features(body: &Value) -> Vec<Value> {
    body.get("features").and_then(Value::as_array).cloned().unwrap_or_default()
}

/// One Photon feature → a [`Place`].
///
/// Returns `None` rather than a half-place for anything without coordinates: a result you
/// cannot put on the map is not a result.
fn photon_place(f: &Value) -> Option<Place> {
    let p = f.get("properties")?;
    let c = f.get("geometry")?.get("coordinates")?.as_array()?;
    let (lon, lat) = (c.first()?.as_f64()?, c.get(1)?.as_f64()?);

    let s = |k: &str| p.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();
    let name = {
        let n = s("name");
        if !n.is_empty() {
            n
        } else if !s("street").is_empty() {
            join(" ", &[s("street"), s("housenumber")])
        } else {
            // A bare city/state/country result has its name in whichever field it is.
            first_nonempty(&[s("city"), s("district"), s("state"), s("country")])
        }
    };
    if name.is_empty() {
        return None;
    }

    // Photon's `extent` is [minlon, maxlat, maxlon, minlat] — note the interleave. Ours is
    // GeoJSON bbox order so nothing downstream has to remember this.
    let extent = p.get("extent").and_then(Value::as_array).and_then(|e| {
        let v: Vec<f64> = e.iter().filter_map(Value::as_f64).collect();
        match v[..] {
            [w, n, e_, s_] => Some([w, s_.min(n), e_, n.max(s_)]),
            _ => None,
        }
    });

    Some(Place {
        id: format!("{}{}", s("osm_type"), p.get("osm_id").and_then(Value::as_i64).unwrap_or(0)),
        kind: pretty_kind(&s("osm_key"), &s("osm_value")),
        // The name is already the first line; an address that repeats it reads as a stutter.
        address: address_line(
            &name,
            &[
                join(" ", &[s("street"), s("housenumber")]),
                s("district"),
                s("city"),
                s("state"),
                s("country"),
            ],
        ),
        country: s("country"),
        name,
        lat,
        lon,
        extent,
    })
}

/// `amenity:cafe` → "cafe"; `railway:station` → "railway station"; `place:city` → "city".
///
/// The value alone is the useful half almost always; the key is only worth keeping when it
/// adds a word a person would actually say.
fn pretty_kind(key: &str, value: &str) -> String {
    // `yes` is OSM's way of saying "this key applies", not a description of anything —
    // `building=yes`, `crossing=yes`. Printing it as the kind of a place produced the line
    // "渋谷駅前 / yes", which tells a reader nothing at all.
    let v = match value {
        "yes" | "no" | "unknown" => String::new(),
        other => other.replace('_', " "),
    };
    if v.is_empty() {
        return match key {
            "building" | "crossing" | "place" | "" => String::new(),
            k => k.replace('_', " "),
        };
    }
    match key {
        "railway" | "aeroway" | "waterway" | "natural" | "historic" => {
            format!("{} {v}", key.replace('_', " "))
        }
        _ => v,
    }
}

/// Compose the address line: drop empties, drop anything already said in the name, and
/// drop consecutive repeats (Photon often has city == district).
fn address_line(name: &str, parts: &[String]) -> String {
    let mut out: Vec<String> = Vec::new();
    for p in parts {
        let p = p.trim();
        if p.is_empty() || p.eq_ignore_ascii_case(name) {
            continue;
        }
        if out.iter().any(|q| q.eq_ignore_ascii_case(p)) {
            continue;
        }
        out.push(p.to_string());
    }
    out.join(", ")
}

fn join(sep: &str, parts: &[String]) -> String {
    parts.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(sep)
}

fn first_nonempty(parts: &[String]) -> String {
    parts.iter().find(|s| !s.trim().is_empty()).cloned().unwrap_or_default()
}

// ─────────────────────────────────────────────────────────────────────────────
// Nominatim — precision, and only when asked
// ─────────────────────────────────────────────────────────────────────────────

/// The authoritative OSM geocoder, used here as a *fallback* and nothing else.
///
/// Their usage policy is a hard 1 request/second and forbids attaching it to a typeahead,
/// so this is only reached when Photon found nothing at all — which is exactly the case
/// where a full street address ("221B Baker Street, London") beats a fuzzy index.
pub struct Nominatim {
    svc: Service,
}

impl Nominatim {
    pub fn new() -> Result<Nominatim> {
        Ok(Nominatim { svc: Service::new(NOMINATIM_GAP, SEARCH_TTL)? })
    }

    pub async fn lookup(&self, q: &str) -> Result<Vec<Place>> {
        let q = q.trim();
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let url = format!("{NOMINATIM}/search?q={}&format=jsonv2&addressdetails=1&limit=5", enc(q));
        let http = &self.svc.http;
        let body = self
            .svc
            .get(url.clone(), move || async move {
                http.get(&url)
                    .send()
                    .await
                    .map_err(|e| transport("the address service", e))?
                    .error_for_status()
                    .map_err(|e| transport("the address service", e))?
                    .json::<Value>()
                    .await
                    .map_err(|e| transport("the address service", e))
            })
            .await?;
        Ok(body.as_array().map(|a| a.iter().filter_map(nominatim_place).collect()).unwrap_or_default())
    }
}

fn nominatim_place(v: &Value) -> Option<Place> {
    let lat = v.get("lat")?.as_str()?.parse::<f64>().ok()?;
    let lon = v.get("lon")?.as_str()?.parse::<f64>().ok()?;
    let display = v.get("display_name").and_then(Value::as_str).unwrap_or("").to_string();
    let (name, rest) = match display.split_once(", ") {
        Some((a, b)) => (a.to_string(), b.to_string()),
        None => (display.clone(), String::new()),
    };
    if name.is_empty() {
        return None;
    }
    // [south, north, west, east] as strings — Nominatim's own order, not GeoJSON's.
    let extent = v.get("boundingbox").and_then(Value::as_array).and_then(|b| {
        let f: Vec<f64> = b.iter().filter_map(|x| x.as_str()?.parse::<f64>().ok()).collect();
        match f[..] {
            [s, n, w, e] => Some([w, s, e, n]),
            _ => None,
        }
    });
    Some(Place {
        id: format!(
            "{}{}",
            v.get("osm_type").and_then(Value::as_str).unwrap_or("N").chars().next().unwrap_or('N').to_uppercase(),
            v.get("osm_id").and_then(Value::as_i64).unwrap_or(0)
        ),
        name,
        kind: pretty_kind(
            v.get("category").and_then(Value::as_str).unwrap_or(""),
            v.get("type").and_then(Value::as_str).unwrap_or(""),
        ),
        address: rest,
        country: v
            .get("address")
            .and_then(|a| a.get("country"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        lat,
        lon,
        extent,
    })
}

/// What OSM knows about one place beyond its dot: the fields that turn "a cafe" into
/// "open until 22:00, +90 …, no steps at the door".
///
/// Fetched lazily on selection (one gated Overpass id-lookup, cached for hours) rather
/// than carried on every [`Place`] — forty results with hours would be forty queries
/// nobody asked for.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Detail {
    /// The [`Place::id`] this belongs to, so a slow answer for the last selection cannot
    /// dress up the current one.
    pub id: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub hours: String,
    /// Some(true|false) when [`open_now`] could read the hours; None when it could not —
    /// shown as the raw string then, never guessed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open: Option<bool>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub phone: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub website: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub cuisine: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub wheelchair: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Overpass — what is actually around here
// ─────────────────────────────────────────────────────────────────────────────

/// A radius query against the live OpenStreetMap database.
///
/// This exists because **a search engine cannot answer "what is nearby"**. Photon takes a
/// `lat`/`lon` bias, and it is genuinely useful for that, but the query still has to match
/// text: asking it for `cafe` with `osm_tag=amenity:cafe` at Shibuya Crossing returns
/// cafes whose *names* contain "cafe", the nearest of which was 2.8 km away in Shinjuku —
/// while several hundred unremarkable 喫茶店 sat inside 200 m, unmatched because their
/// names do not contain the English word.
///
/// Overpass asks the question the human actually asked: every node, way and relation
/// carrying this tag, within this radius. It is a database query rather than an index
/// lookup, so it is slower and far more expensive for the people hosting it — hence its
/// own wide gate, its own cache, and the fact that nothing else in the app touches it.
pub struct Overpass {
    svc: Service,
}

impl Overpass {
    pub fn new() -> Result<Overpass> {
        // Its own, longer client timeout. Overpass is given a server-side budget in the
        // query itself; giving up before that budget expires means paying for the work and
        // then throwing the answer away — and, because a timeout looks like a failure,
        // trying the mirror and paying for it twice.
        Ok(Overpass { svc: Service::with_timeout(OVERPASS_GAP, SEARCH_TTL, OVERPASS_TIMEOUT)? })
    }

    /// Everything tagged `key=value` within `radius_m` of a point.
    pub async fn around(&self, tag: &str, at: [f64; 2], radius_m: u32) -> Result<Vec<Place>> {
        let (key, value) = tag.split_once(':').unwrap_or((tag, ""));
        let filter = if value.is_empty() {
            format!("[\"{key}\"]")
        } else {
            format!("[\"{key}\"=\"{value}\"]")
        };
        // `nwr` covers nodes, ways and relations — a supermarket is usually a building
        // outline, not a point, and asking only for nodes silently misses most of them.
        // `out center` gives each one a single coordinate, which is what a pin needs.
        let q = format!(
            "[out:json][timeout:{OVERPASS_BUDGET}];nwr{filter}(around:{radius_m},{:.5},{:.5});out center {};",
            at[0],
            at[1],
            RESULT_LIMIT * 3
        );

        let http = &self.svc.http;
        // The query IS the cache key, deliberately without the endpoint in it: which
        // mirror answered is an implementation detail, and the answer is the same either
        // way.
        let body = self
            .svc
            .get(q.clone(), move || async move {
                // Only the LAST endpoint's failure is reported; the others are noise
                // about servers the user never chose.
                let mut last = anyhow!("no Overpass endpoint configured");
                for url in OVERPASS {
                    match ask_overpass(http, url, &q).await {
                        Ok(v) => return Ok(v),
                        Err(e) => last = e,
                    }
                }
                Err(last)
            })
            .await?;

        Ok(body
            .get("elements")
            .and_then(Value::as_array)
            .map(|els| els.iter().filter_map(overpass_place).collect())
            .unwrap_or_default())
    }
}

impl Overpass {
    /// The tags of one feature, by the id search gave us ("N123" / "W123" / "R123").
    pub async fn lookup(&self, id: &str) -> Result<Option<Value>> {
        let (kind, num) = match (id.chars().next(), &id[1..]) {
            (Some('N'), n) => ("node", n),
            (Some('W'), n) => ("way", n),
            (Some('R'), n) => ("relation", n),
            _ => return Ok(None), // a coordinate or a pin — nothing to look up
        };
        if !num.chars().all(|c| c.is_ascii_digit()) || num.is_empty() {
            return Ok(None);
        }
        let q = format!("[out:json][timeout:10];{kind}({num});out tags 1;");
        let http = &self.svc.http;
        let body = self
            .svc
            .get(q.clone(), move || async move {
                let mut last = anyhow!("no Overpass endpoint configured");
                for url in OVERPASS {
                    match ask_overpass(http, url, &q).await {
                        Ok(v) => return Ok(v),
                        Err(e) => last = e,
                    }
                }
                Err(last)
            })
            .await?;
        Ok(body
            .get("elements")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(|e| e.get("tags"))
            .cloned())
    }
}

/// One attempt at one Overpass endpoint.
async fn ask_overpass(http: &reqwest::Client, url: &str, q: &str) -> Result<Value> {
    let r = http
        .post(url)
        .body(q.to_string())
        .send()
        .await
        .map_err(|e| transport("the map database", e))?;
    // Overpass answers a load it cannot take with 429 or 504 and a plain-text body. Say
    // what that means rather than passing on a status code.
    match r.status().as_u16() {
        429 | 504 => bail!("the map database is busy right now — try again shortly"),
        s if !(200..300).contains(&s) => bail!("the map database refused ({s})"),
        _ => {}
    }
    r.json::<Value>().await.map_err(|e| transport("the map database", e))
}

/// One Overpass element → a [`Place`].
///
/// Unnamed features are dropped. OSM is full of them — every bench, every bollard, and
/// plenty of cafes — and a list of eleven things all called "cafe" is not a list anybody
/// can act on.
fn overpass_place(e: &Value) -> Option<Place> {
    let (lat, lon) = match (e.get("lat").and_then(Value::as_f64), e.get("lon").and_then(Value::as_f64)) {
        (Some(a), Some(b)) => (a, b),
        // A way or relation carries its point under `center` (that is what `out center` is).
        _ => {
            let c = e.get("center")?;
            (c.get("lat")?.as_f64()?, c.get("lon")?.as_f64()?)
        }
    };
    let tags = e.get("tags")?;
    let t = |k: &str| tags.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();

    // `name:en` is a courtesy to whoever is reading, but the local name is the one written
    // on the door — so the local name leads and the English one is not a replacement.
    let name = first_nonempty(&[t("name"), t("brand"), t("operator")]);
    if name.is_empty() {
        return None;
    }

    let kind = ["amenity", "shop", "tourism", "leisure", "railway", "aeroway", "highway"]
        .iter()
        .find_map(|k| {
            let v = t(k);
            (!v.is_empty()).then(|| pretty_kind(k, &v))
        })
        .unwrap_or_default();

    // A house number with no street is not an address, it is a digit. Japan tags plenty of
    // them that way (`addr:housenumber=6` with no `addr:street` at all), and printing "6,
    // 渋谷区" under a cafe reads as a mistake rather than as information.
    let street = match t("addr:street") {
        s if s.is_empty() => String::new(),
        s => join(" ", &[s, t("addr:housenumber")]),
    };
    Some(Place {
        id: format!(
            "{}{}",
            e.get("type").and_then(Value::as_str).unwrap_or("node").chars().next().unwrap_or('n').to_uppercase(),
            e.get("id").and_then(Value::as_i64).unwrap_or(0)
        ),
        address: address_line(&name, &[street, t("addr:suburb"), t("addr:city")]),
        kind,
        name,
        lat,
        lon,
        country: String::new(),
        extent: None,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Valhalla — routes and reach
// ─────────────────────────────────────────────────────────────────────────────

/// FOSSGIS's public Valhalla: a full planet graph, no key, fair use.
pub struct Valhalla {
    svc: Service,
}

impl Valhalla {
    pub fn new() -> Result<Valhalla> {
        Ok(Valhalla { svc: Service::new(VALHALLA_GAP, ROUTE_TTL)? })
    }

    async fn post(&self, path: &str, body: Value) -> Result<Value> {
        let key = format!("{path}:{body}");
        let url = format!("{VALHALLA}{path}");
        let http = &self.svc.http;
        self.svc
            .get(key, move || async move {
                let r = http
                    .post(&url)
                    .header("X-Client-Id", CLIENT_ID)
                    .json(&body)
                    .send()
                    .await
                    .map_err(|e| transport("the routing service", e))?;
                // Valhalla explains its own refusals ("No path could be found…"), and that
                // sentence is far more useful than "HTTP 400".
                if !r.status().is_success() {
                    let code = r.status();
                    let why = r.json::<Value>().await.ok();
                    let msg = why
                        .as_ref()
                        .and_then(|v| v.get("error"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    bail!(if msg.is_empty() { format!("the routing service refused ({code})") } else { msg });
                }
                r.json::<Value>().await.map_err(|e| transport("the routing service", e))
            })
            .await
    }
}

impl Router for Valhalla {
    async fn route(&self, stops: &[Place], mode: Mode) -> Result<Route> {
        if stops.len() < 2 {
            bail!("a route needs at least two stops");
        }
        let v = self.post("/route", trip_request(stops, mode)).await?;
        parse_trip(stops, mode, &v)
    }

    async fn optimize(&self, stops: &[Place], mode: Mode) -> Result<(Vec<Place>, Route)> {
        if stops.len() < 3 {
            bail!("two stops have only one order — add a third before optimising");
        }
        let v = self.post("/optimized_route", trip_request(stops, mode)).await?;
        // The visit order comes back as `original_index` per location. Probed live before
        // this was written: a zigzag of five Istanbul stops came back [0, 1, 3, 2, 4] —
        // the endpoint reorders, keeps the first and last fixed, and answers in exactly
        // /route's shape.
        let order: Vec<usize> = v
            .get("trip")
            .and_then(|t| t.get("locations"))
            .and_then(Value::as_array)
            .map(|ls| {
                ls.iter()
                    .filter_map(|l| l.get("original_index").and_then(Value::as_u64))
                    .map(|i| i as usize)
                    .collect()
            })
            .unwrap_or_default();
        let mut seen = order.clone();
        seen.sort_unstable();
        if seen != (0..stops.len()).collect::<Vec<_>>() {
            bail!("the routing service answered with a visit order that is not a permutation");
        }
        let reordered: Vec<Place> = order.iter().map(|&i| stops[i].clone()).collect();
        let route = parse_trip(&reordered, mode, &v)?;
        Ok((reordered, route))
    }

    async fn reach(&self, at: &Place, minutes: u32, mode: Mode) -> Result<Reach> {
        let minutes = minutes.clamp(1, 120);
        let body = json!({
            "locations": [{ "lat": at.lat, "lon": at.lon }],
            "costing": mode.costing(),
            "contours": [{ "time": minutes }],
            "polygons": true,
        });
        let v = self.post("/isochrone", body).await?;
        // The answer is a FeatureCollection; a `polygons: true` contour comes back as a
        // Polygon whose first ring is the outer boundary.
        let ring = features(&v)
            .iter()
            .find_map(|f| {
                let g = f.get("geometry")?;
                let outer = match g.get("type").and_then(Value::as_str)? {
                    "Polygon" => g.get("coordinates")?.get(0)?.clone(),
                    "MultiPolygon" => g.get("coordinates")?.get(0)?.get(0)?.clone(),
                    _ => return None,
                };
                let pts: Vec<[f64; 2]> = outer
                    .as_array()?
                    .iter()
                    .filter_map(|p| {
                        let a = p.as_array()?;
                        Some([a.first()?.as_f64()?, a.get(1)?.as_f64()?])
                    })
                    .collect();
                (pts.len() >= 4).then_some(pts)
            })
            .ok_or_else(|| anyhow!("the routing service drew no area around {}", at.name))?;

        Ok(Reach { minutes, mode, center: [at.lon, at.lat], from: at.name.clone(), ring })
    }
}

/// One request body for /route and /optimized_route — they take the same thing.
fn trip_request(stops: &[Place], mode: Mode) -> Value {
    json!({
        // Every stop is a `break`: Valhalla ends a leg and issues an arrival instruction
        // at each one. `through` would pass by without stopping, which is a different
        // journey and not the one somebody who added a stop is asking for.
        "locations": stops
            .iter()
            .map(|p| json!({ "lat": p.lat, "lon": p.lon, "type": "break" }))
            .collect::<Vec<_>>(),
        "costing": mode.costing(),
        "units": "kilometers",
        "directions_options": { "language": "en-US" },
    })
}

/// One trip answer, whichever endpoint produced it. `stops` must already be in the visit
/// order the answer describes.
fn parse_trip(stops: &[Place], mode: Mode, v: &Value) -> Result<Route> {
    let trip = v.get("trip").ok_or_else(|| anyhow!("the routing service sent no trip"))?;
    let raw = trip.get("legs").and_then(Value::as_array).cloned().unwrap_or_default();
    if raw.len() + 1 != stops.len() {
        // One leg per consecutive pair; anything else means the request and the answer
        // are describing different journeys.
        bail!("the routing service answered with {} legs for {} stops", raw.len(), stops.len());
    }

    let mut shape: Vec<[f64; 2]> = Vec::new();
    let mut legs: Vec<Leg> = Vec::new();
    for (i, leg) in raw.iter().enumerate() {
        let base = shape.len();
        let pts = leg
            .get("shape")
            .and_then(Value::as_str)
            .map(decode_polyline6)
            .unwrap_or_default();
        let mut steps = Vec::new();
        for m in leg.get("maneuvers").and_then(Value::as_array).cloned().unwrap_or_default() {
            let instruction =
                m.get("instruction").and_then(Value::as_str).unwrap_or("").trim().to_string();
            if instruction.is_empty() {
                continue;
            }
            steps.push(Step {
                instruction,
                km: m.get("length").and_then(Value::as_f64).unwrap_or(0.0),
                secs: m.get("time").and_then(Value::as_f64).unwrap_or(0.0),
                // Offset into the WHOLE trip's shape, not this leg's: the window draws one
                // line and highlights a stretch of it.
                at: base + m.get("begin_shape_index").and_then(Value::as_u64).unwrap_or(0) as usize,
            });
        }
        let sum = leg.get("summary");
        legs.push(Leg {
            from: stops[i].name.clone(),
            to: stops[i + 1].name.clone(),
            km: sum.and_then(|s| s.get("length")).and_then(Value::as_f64).unwrap_or(0.0),
            secs: sum.and_then(|s| s.get("time")).and_then(Value::as_f64).unwrap_or(0.0),
            at: base,
            steps,
        });
        shape.extend(pts);
    }
    if shape.is_empty() {
        bail!(
            "no way to get from {} to {} {}",
            stops[0].name,
            stops[stops.len() - 1].name,
            mode.label()
        );
    }

    let sum = trip.get("summary");
    Ok(Route {
        mode,
        km: sum.and_then(|s| s.get("length")).and_then(Value::as_f64).unwrap_or(0.0),
        secs: sum.and_then(|s| s.get("time")).and_then(Value::as_f64).unwrap_or(0.0),
        stops: stops.iter().map(|p| p.name.clone()).collect(),
        shape,
        legs,
    })
}

/// Valhalla encodes shapes as a Google polyline with **six** decimal digits, not the usual
/// five. Decoding it at 1e5 puts a route in the wrong ocean, silently — which is why the
/// precision is spelled out here instead of being a magic number.
fn decode_polyline6(s: &str) -> Vec<[f64; 2]> {
    let (mut lat, mut lon) = (0i64, 0i64);
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let delta = |i: &mut usize| -> Option<i64> {
            let (mut shift, mut result) = (0u32, 0i64);
            loop {
                let b = *bytes.get(*i)? as i64 - 63;
                *i += 1;
                result |= (b & 0x1f) << shift;
                shift += 5;
                if b < 0x20 {
                    break;
                }
                if shift > 60 {
                    return None;
                }
            }
            Some(if result & 1 != 0 { !(result >> 1) } else { result >> 1 })
        };
        let Some(dlat) = delta(&mut i) else { break };
        let Some(dlon) = delta(&mut i) else { break };
        lat += dlat;
        lon += dlon;
        out.push([lon as f64 / 1e6, lat as f64 / 1e6]);
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// The facade
// ─────────────────────────────────────────────────────────────────────────────

/// Everything the app asks the world, in one place.
///
/// The rest of the app never names a provider — it calls these. That is what makes the
/// seam real rather than decorative.
pub struct Geo {
    photon: Photon,
    nominatim: Nominatim,
    overpass: Overpass,
    valhalla: Valhalla,
}

impl Geo {
    pub fn new() -> Result<Geo> {
        Ok(Geo {
            photon: Photon::new()?,
            nominatim: Nominatim::new()?,
            overpass: Overpass::new()?,
            valhalla: Valhalla::new()?,
        })
    }

    /// Search by name, likeliest answer first (see [`rank`]).
    ///
    /// Falls through to Nominatim in the two cases the fuzzy index cannot serve: it found
    /// nothing at all, or everything it found is street furniture — a query that came back
    /// as four bus stops has not been answered. One request per second, once, on a
    /// deliberate lookup, is well inside Nominatim's policy; the typeahead never comes
    /// here.
    pub async fn find(&self, q: &str, bias: Bias) -> Result<Vec<Place>> {
        let hits = rank(q, bias, self.photon.search(q, bias, None, RESULT_LIMIT).await?);

        // Two shapes of "not answered", both seen against the live index:
        //
        //   * everything that came back is street furniture — four bus stops is not an
        //     answer to a place name;
        //   * nothing that came back is even *called* what was asked for. "Tokyo Station"
        //     near Shibuya returns real railway stations — 石神井公園, 原宿駅, a Disneyland
        //     platform — and 東京駅 is not among them at any depth, because the index has
        //     no English handle on it. Ranking cannot promote a result that is absent, and
        //     a station with an unrelated name is a *worse* wrong answer than none: it
        //     looks right enough to route to.
        let hopeless = hits.first().is_none_or(|p| is_street_furniture(&p.kind))
            || !hits.iter().take(5).any(|p| name_echoes(q, &p.name));
        if !hopeless {
            return Ok(hits);
        }
        match self.nominatim.lookup(q).await {
            Ok(more) if !more.is_empty() => {
                // Nominatim's answer leads, but the index's results are still results —
                // keep them underneath rather than throwing away a list the human may
                // have been about to pick from.
                let mut out = more;
                out.extend(hits);
                out.truncate(RESULT_LIMIT);
                Ok(out)
            }
            _ => Ok(hits),
        }
    }

    /// What is around here, **nearest first**.
    ///
    /// Two different questions wear this one word, and they need two different services:
    ///
    ///   * a **category** ("cafes", "pharmacy", "fuel") is a radius question, and only
    ///     [`Overpass`] can answer it — a search index matches names, so it finds the cafe
    ///     called "Cafe Shakey's" three kilometres away and misses the two hundred
    ///     unremarkable ones on this street;
    ///   * anything else is a *name* somebody wants found near here, which is exactly what
    ///     a biased [`Photon`] search is for.
    ///
    /// Either way the ordering is ours, because neither service sorts by distance.
    pub async fn nearby(&self, what: &str, at: [f64; 2], radius_m: u32) -> Result<Vec<Place>> {
        let hits = match osm_tag(what) {
            Some(tag) => {
                let found = self.overpass.around(&tag, at, radius_m).await?;
                // A radius that found nothing is a real answer at 300 m and a wrong one at
                // city scale, so widen once — but only once, because each of these is a
                // query against a live database somebody else pays for.
                if found.is_empty() && radius_m < 6_000 {
                    self.overpass.around(&tag, at, 6_000).await?
                } else {
                    found
                }
            }
            None => self.photon.search(what, Some(at), None, RESULT_LIMIT).await?,
        };

        let mut ranked: Vec<(f64, Place)> =
            hits.into_iter().map(|p| (km_between(at, [p.lat, p.lon]), p)).collect();
        ranked.sort_by(|a, b| a.0.total_cmp(&b.0));
        ranked.truncate(RESULT_LIMIT);
        Ok(ranked.into_iter().map(|(_, p)| p).collect())
    }

    /// One place for a phrase — **or the candidates, when there is no one place.**
    ///
    /// This is the difference between a router you can use and one you have to guess at.
    /// `route "Taksim"` used to take the top hit and go; if the top hit was the metro
    /// station and you meant the square, there was nothing you could do about it except
    /// retype the name more precisely, and there is no spelling of "Taksim" that means the
    /// square. So an ambiguous query is not an error and not a guess: it comes back as a
    /// list, the same list `find` produces, and the caller picks — `maps select 2`, or a
    /// click on the row. Two surfaces, one mechanism, no new concept.
    pub async fn resolve(&self, q: &str, bias: Bias) -> Result<Found> {
        if let Some(p) = parse_coords(q) {
            return Ok(Found::One(self.at(p).await));
        }
        let hits = self.find(q, bias).await?;
        match hits.len() {
            0 => bail!("nothing on the map matches \"{q}\""),
            1 => Ok(Found::One(hits.into_iter().next().unwrap())),
            _ => {
                // One clear winner is not ambiguity. The test is the *margin*: an exact
                // name match, or a score well clear of the runner-up, is an answer. A
                // photo-finish between a square and a metro station is a question.
                let scored = scores(q, bias, &hits);
                let clear = scored[0] - scored[1] >= DECISIVE
                    || hits[0].name.trim().eq_ignore_ascii_case(q.trim());
                if clear {
                    Ok(Found::One(hits.into_iter().next().unwrap()))
                } else {
                    Ok(Found::Many(hits))
                }
            }
        }
    }

    /// A coordinate, named by whatever is there. Split out because `resolve` and `place`
    /// both need it and neither should move the pin the caller actually asked for.
    async fn at(&self, p: [f64; 2]) -> Place {
        match self.photon.reverse(p[0], p[1]).await {
            Ok(Some(mut hit)) => {
                hit.lat = p[0];
                hit.lon = p[1];
                hit.extent = None;
                hit
            }
            _ => Place {
                id: format!("@{:.5},{:.5}", p[0], p[1]),
                name: format!("{:.5}, {:.5}", p[0], p[1]),
                kind: "coordinate".into(),
                lat: p[0],
                lon: p[1],
                ..Place::default()
            },
        }
    }

    /// One place for a phrase — the top hit, or a coordinate pair if that is what was typed.
    pub async fn place(&self, q: &str, bias: Bias) -> Result<Place> {
        if let Some(p) = parse_coords(q) {
            return Ok(self.at(p).await);
        }
        self.find(q, bias)
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("nothing on the map matches \"{q}\""))
    }

    /// What is at this coordinate, for naming the view the human dragged to.
    pub async fn whats_here(&self, lat: f64, lon: f64) -> Result<Option<Place>> {
        self.photon.reverse(lat, lon).await
    }

    pub async fn route(&self, stops: &[Place], mode: Mode) -> Result<Route> {
        self.valhalla.route(stops, mode).await
    }

    pub async fn optimize(&self, stops: &[Place], mode: Mode) -> Result<(Vec<Place>, Route)> {
        self.valhalla.optimize(stops, mode).await
    }

    /// What OSM knows about this place beyond its dot. `now` is (weekday 0=Monday,
    /// minutes since midnight) in the user's local time — passed in so the hours
    /// evaluation stays pure and testable.
    pub async fn detail(&self, p: &Place, now: (u8, u16)) -> Result<Option<Detail>> {
        let Some(tags) = self.overpass.lookup(&p.id).await? else { return Ok(None) };
        let t = |k: &str| tags.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();
        let hours = t("opening_hours");
        Ok(Some(Detail {
            id: p.id.clone(),
            open: open_now(&hours, now.0, now.1),
            hours,
            phone: first_nonempty(&[t("phone"), t("contact:phone")]),
            website: first_nonempty(&[t("website"), t("contact:website")]),
            cuisine: t("cuisine").replace([';', '_'], " "),
            wheelchair: t("wheelchair"),
        }))
    }

    /// Typeahead candidates for the window, and NOTHING else: Photon only, because
    /// Nominatim's usage policy forbids being wired to an autocomplete — the `find`
    /// fallback path must never run per keystroke. Reads no state, changes no state,
    /// signals nobody.
    pub async fn suggest(&self, q: &str, bias: Bias) -> Result<Vec<Place>> {
        if q.trim().len() < 2 {
            return Ok(Vec::new());
        }
        let mut hits = rank(q, bias, self.photon.search(q, bias, None, 10).await?);
        hits.truncate(6);
        Ok(hits)
    }

    pub async fn reach(&self, at: &Place, minutes: u32, mode: Mode) -> Result<Reach> {
        self.valhalla.reach(at, minutes, mode).await
    }
}

/// How much a place's *kind* suggests it is the thing somebody meant by a bare name.
///
/// This exists because a text index ranks by text, and "Tokyo Station" typed near Shibuya
/// returns four bus stops in Odaiba before anything with a platform. A bus stop called
/// 東京テレポート駅前 is a perfectly good text match and an obviously wrong answer, and no
/// amount of distance weighting fixes that — the fix is to know that a railway station is
/// a more likely destination than a bus stop.
fn prominence(kind: &str) -> f64 {
    let k = kind.trim();
    match k {
        "city" | "railway station" | "aerodrome" | "aeroway aerodrome" => 3.0,
        "town" | "train station" | "university" | "hospital" | "stadium" => 2.4,
        "suburb" | "village" | "museum" | "attraction" | "castle" | "park" | "peak"
        | "square" | "plaza" | "monument" | "viewpoint" => 1.9,
        // The long tail of street furniture. These are real features and terrible answers
        // to "take me to X".
        "bus stop" | "steps" | "footway" | "platform" | "bench" | "crossing" | "junction"
        | "traffic signals" | "street lamp" | "waste basket" | "parking entrance" => 0.3,
        "" => 0.8,
        _ if k.ends_with("station") => 2.2,
        _ => 1.0,
    }
}

/// Is this the kind of thing that should never win a bare-name search on its own?
fn is_street_furniture(kind: &str) -> bool {
    prominence(kind) <= 0.4
}

/// Does this name contain what was asked for, or the other way round?
///
/// Deliberately crude, and deliberately one-directional-agnostic: "Louvre" should echo in
/// "Musée du Louvre", and "Tokyo Station Hotel" should echo "Tokyo Station". It says
/// nothing across scripts — 東京駅 does not echo "Tokyo Station" — and that silence is
/// exactly the signal [`Geo::find`] acts on.
fn name_echoes(query: &str, name: &str) -> bool {
    let (q, n) = (query.trim().to_lowercase(), name.trim().to_lowercase());
    if q.is_empty() || n.is_empty() {
        return false;
    }
    n.contains(&q) || q.contains(&n)
}

/// What a phrase resolved to: one place, or the shortlist to choose from.
#[derive(Clone, Debug, PartialEq)]
pub enum Found {
    One(Place),
    Many(Vec<Place>),
}

/// How far ahead the best candidate has to be before we treat it as *the* answer rather
/// than as the top of a list. Tuned against the case that motivated it: "Taksim" returns a
/// square and a metro station of near-identical standing, and picking one for the user is
/// how you end up routing them to the wrong place with total confidence.
const DECISIVE: f64 = 0.8;

/// The scores [`rank`] sorted by, for callers that need to know *how* clearly it won.
fn scores(query: &str, bias: Bias, places: &[Place]) -> Vec<f64> {
    let q = query.trim().to_lowercase();
    let n = places.len().max(1) as f64;
    places
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let name = p.name.to_lowercase();
            // An exact name is the strongest signal there is, and it has to beat the
            // prominence table outright: searching "Taksim Meydanı" returned two railway
            // stations above the square that is literally called that, because a station
            // outranks a square by category and "Taksim" is contained in the query. A name
            // somebody typed in full is not a category question.
            let name_match = if name == q {
                2.5
            } else if name.starts_with(&q) {
                1.2
            } else if name.contains(&q) {
                0.7
            } else {
                0.0
            };
            let order = 0.5 * (1.0 - i as f64 / n);
            let near = match bias {
                Some(at) => -0.55 * (1.0 + km_between(at, [p.lat, p.lon])).log10(),
                None => 0.0,
            };
            prominence(&p.kind) + name_match + order + near
        })
        .collect()
}

/// Put the likeliest answer first.
///
/// Four signals, none of which is sufficient alone:
///
///   * what kind of thing it is ([`prominence`]) — the one the index cannot weigh;
///   * whether the name actually matches what was typed, which is worthless across
///     scripts ("Tokyo Station" vs 東京駅) and decisive within one;
///   * the index's own opinion, kept as a decaying bonus, because it is a search engine
///     and this is what it is for;
///   * distance, but only gently — log-scaled, so a cafe 200 m away does not outrank the
///     airport somebody clearly asked for.
fn rank(query: &str, bias: Bias, places: Vec<Place>) -> Vec<Place> {
    let mut scored: Vec<(f64, Place)> =
        scores(query, bias, &places).into_iter().zip(places).collect();
    // Descending, and stable enough that equal scores keep the index's order.
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));
    dedupe(scored.into_iter().map(|(_, p)| p).collect())
}

/// Drop the same place listed twice.
///
/// OSM maps one thing several ways — a station is a node, a building and two platforms —
/// so an index happily returns "Taksim (railway station)" three times, metres apart. That
/// is noise in a result list and *worse* in a list somebody is being asked to choose a
/// destination from: two identical rows is a question with no answer. Same name, same
/// kind, within [`SAME_THING_M`] of each other: keep the first, which ranking has already
/// put in the best position.
fn dedupe(places: Vec<Place>) -> Vec<Place> {
    let mut kept: Vec<Place> = Vec::with_capacity(places.len());
    for p in places {
        let dup = kept.iter().any(|q| {
            q.name.eq_ignore_ascii_case(&p.name)
                && q.kind == p.kind
                && km_between([q.lat, q.lon], [p.lat, p.lon]) * 1000.0 <= SAME_THING_M
        });
        if !dup {
            kept.push(p);
        }
    }
    kept
}

/// How close two features with the same name and kind have to be before they are one
/// place. Measured, not guessed: the two "Taksim (railway station)" entries the live index
/// returns are its two entrances, 168 m apart. Two genuinely different branches of a chain
/// are kilometres apart, so this has a lot of room either side.
const SAME_THING_M: f64 = 300.0;

/// The handful of categories worth a real OSM tag, because the plain word is ambiguous in
/// a search index ("bank" is a financial one and a river's edge; "pharmacy" hits brand
/// names). Anything not listed is searched as words, which is the better default — OSM has
/// thousands of tags and guessing wrong returns nothing at all.
fn osm_tag(what: &str) -> Option<String> {
    let w = what.trim().to_ascii_lowercase();
    let tag = match w.trim_end_matches('s') {
        "cafe" | "coffee" => "amenity:cafe",
        "restaurant" | "food" => "amenity:restaurant",
        "bar" | "pub" => "amenity:bar",
        "hotel" => "tourism:hotel",
        "supermarket" | "grocery" => "shop:supermarket",
        "pharmacy" | "chemist" => "amenity:pharmacy",
        "hospital" => "amenity:hospital",
        "bank" => "amenity:bank",
        "atm" => "amenity:atm",
        "fuel" | "petrol" | "gas station" => "amenity:fuel",
        "parking" => "amenity:parking",
        "toilet" | "wc" => "amenity:toilets",
        "school" => "amenity:school",
        "museum" => "tourism:museum",
        "park" => "leisure:park",
        "playground" => "leisure:playground",
        "gym" | "fitness" => "leisure:fitness_centre",
        "station" => "railway:station",
        "bus stop" => "highway:bus_stop",
        "airport" => "aeroway:aerodrome",
        "charger" | "ev charger" | "charging" => "amenity:charging_station",
        "viewpoint" => "tourism:viewpoint",
        _ => return None,
    };
    Some(tag.to_string())
}

/// "41.0082, 28.9784" or "41.0082 28.9784" → a point. Anything else is a name.
fn parse_coords(s: &str) -> Option<[f64; 2]> {
    let cleaned = s.trim().replace(',', " ");
    let n: Vec<f64> = cleaned.split_whitespace().filter_map(|t| t.parse::<f64>().ok()).collect();
    match n[..] {
        [lat, lon] if (-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon) => {
            // A bare pair of small integers is far more likely to be a house number or a
            // year than a coordinate, and guessing wrong sends the map to the Gulf of
            // Guinea. Require at least one to look like a real decimal degree.
            (cleaned.contains('.')).then_some([lat, lon])
        }
        _ => None,
    }
}

/// Great-circle distance in kilometres. Used for ordering, not navigation, so the spherical
/// approximation is well inside what it needs to be right about.
pub fn km_between(a: [f64; 2], b: [f64; 2]) -> f64 {
    const R: f64 = 6371.0088;
    let (lat1, lat2) = (a[0].to_radians(), b[0].to_radians());
    let dlat = lat2 - lat1;
    let dlon = (b[1] - a[1]).to_radians();
    let h = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * R * h.sqrt().asin()
}

/// Is a place open at (weekday, minute-of-day), by its raw OSM `opening_hours`?
///
/// The full grammar has months, holidays, sunrise offsets and exceptions; evaluating all
/// of it wrongly would be worse than not trying. So this reads the shapes that cover most
/// real tags — `24/7`, `Mo-Fr 09:00-18:00`, lists of day spans and time spans, `off` —
/// and answers `None` for anything else, which the surfaces show as the raw string. An
/// honest "here are the hours" beats a confident wrong "open".
pub fn open_now(hours: &str, weekday: u8, minute: u16) -> Option<bool> {
    let hours = hours.trim();
    if hours.is_empty() {
        return None;
    }
    if hours == "24/7" {
        return Some(true);
    }
    const DAYS: [&str; 7] = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];

    // Parse EVERYTHING first, evaluate after: a rule we cannot read anywhere in the tag
    // means the whole answer is a guess. "Mo-Fr 09:00-18:00; Dec 25 off" on a Tuesday
    // looks open — unless the Tuesday is Dec 25, which a weekday cannot tell us.
    enum Times {
        Off,
        Spans(Vec<(u16, u16)>),
    }
    let mut rules: Vec<([bool; 7], Times)> = Vec::new();

    for rule in hours.split(';') {
        let rule = rule.trim();
        if rule.is_empty() {
            continue;
        }
        // "Su off" has no digit, so the keyword must come off before the day/time split.
        let lower = rule.to_ascii_lowercase();
        let (days_part, times_part) = if let Some(head) =
            lower.strip_suffix("off").or_else(|| lower.strip_suffix("closed"))
        {
            (rule[..head.len()].trim(), "off")
        } else {
            match rule.find(|c: char| c.is_ascii_digit()) {
                Some(i) if rule[..i].trim().is_empty() => ("Mo-Su", rule),
                Some(i) => (rule[..i].trim(), rule[i..].trim()),
                None => (rule.trim(), ""),
            }
        };

        let mut days = [false; 7];
        for span in days_part.split(',') {
            let span = span.trim();
            // Case-insensitive: the live map writes "mo-su 09:00-21:00" as happily as
            // "Mo-Su" — found on the first supermarket tried, not in the spec.
            if let Some((a, b)) = span.split_once('-') {
                let a = DAYS.iter().position(|d| d.eq_ignore_ascii_case(a.trim()))?;
                let b = DAYS.iter().position(|d| d.eq_ignore_ascii_case(b.trim()))?;
                for (i, d) in days.iter_mut().enumerate() {
                    // Mo-Fr, and Sa-Tu wrapping the week end.
                    if (a <= b && i >= a && i <= b) || (a > b && (i >= a || i <= b)) {
                        *d = true;
                    }
                }
            } else if let Some(d) = DAYS.iter().position(|d| d.eq_ignore_ascii_case(span)) {
                days[d] = true;
            } else {
                // PH, SH, months, sunrise… — tokens whose truth needs a calendar we do
                // not have. Refusing the whole tag is the only answer that cannot be
                // wrong.
                return None;
            }
        }

        let times = if times_part.eq_ignore_ascii_case("off") {
            Times::Off
        } else {
            let mut spans = Vec::new();
            for span in times_part.split(',') {
                let (a, b) = span.trim().split_once('-')?;
                let m = |t: &str| -> Option<u16> {
                    let (h, min) = t.trim().split_once(':')?;
                    Some(h.parse::<u16>().ok()? * 60 + min.parse::<u16>().ok()?)
                };
                spans.push((m(a)?, m(b)?));
            }
            Times::Spans(spans)
        };
        rules.push((days, times));
    }

    // Later rules override earlier ones for the days they name — that is what the
    // semicolon means ("Mo-Sa 10:00-20:00; Su off").
    let mut answer = Some(false); // a day no rule mentions is closed
    for (days, times) in &rules {
        if !days[weekday as usize] {
            continue;
        }
        answer = Some(match times {
            Times::Off => false,
            Times::Spans(spans) => spans.iter().any(|&(start, end)| {
                if start <= end {
                    (start..end).contains(&minute)
                } else {
                    minute >= start || minute < end // 22:00-02:00, the bar shape
                }
            }),
        });
    }
    answer
}

/// Percent-encode a query component. Deliberately tiny — no dependency for the job of
/// escaping a search phrase.
fn enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fixture is a real prefix of a real Valhalla answer (Quai Jacques Chirac, by the
    /// Eiffel Tower), not an invented string — the whole point of the test is that the
    /// numbers coming back match the ones the service meant.
    #[test]
    fn a_valhalla_shape_decodes_at_six_digits_not_five() {
        let pts = decode_polyline6("}xbe|Aom~jCoCoE");
        assert_eq!(pts.len(), 2);
        assert_eq!(pts[0], [2.29348, 48.859039]);
        assert_eq!(pts[1], [2.293584, 48.859111]);
        // GeoJSON order: lon first. Getting this backwards puts Paris in South Sudan, and
        // decoding at 1e5 would put it in the Gulf of Guinea — both silently.
        assert!(pts[0][0] < pts[0][1], "shape points must be [lon, lat]");
    }

    #[test]
    fn a_torn_polyline_stops_rather_than_panics() {
        assert!(decode_polyline6("").is_empty());
        assert!(decode_polyline6("}xbe|A").is_empty(), "a lat with no lon is not a point");
    }

    #[test]
    fn coordinates_are_recognised_but_house_numbers_are_not() {
        assert_eq!(parse_coords("41.0082, 28.9784"), Some([41.0082, 28.9784]));
        assert_eq!(parse_coords("48.8584 2.2945"), Some([48.8584, 2.2945]));
        assert_eq!(parse_coords("  -33.8688,151.2093 "), Some([-33.8688, 151.2093]));
        assert_eq!(parse_coords("Baker Street"), None);
        assert_eq!(parse_coords("221 B"), None, "a house number is not a coordinate");
        assert_eq!(parse_coords("1984"), None);
        assert_eq!(parse_coords("91.5, 10.0"), None, "there is no latitude 91.5");
    }

    #[test]
    fn the_address_line_never_repeats_the_name() {
        let line = address_line(
            "Eiffel Tower",
            &["Avenue Anatole France 5".into(), "Paris".into(), "Paris".into(), "France".into()],
        );
        assert_eq!(line, "Avenue Anatole France 5, Paris, France");
        assert_eq!(address_line("Paris", &["Paris".into(), "France".into()]), "France");
    }

    #[test]
    fn a_photon_feature_becomes_a_place() {
        let f = json!({
            "properties": {
                "osm_type": "W", "osm_id": 5013364, "osm_key": "man_made", "osm_value": "tower",
                "name": "Eiffel Tower", "housenumber": "5", "street": "Avenue Anatole France",
                "city": "Paris", "state": "Île-de-France", "country": "France",
                "extent": [2.2933119, 48.8590453, 2.2956897, 48.8574753]
            },
            "geometry": { "type": "Point", "coordinates": [2.2945006, 48.8582599] }
        });
        let p = photon_place(&f).expect("a complete feature is a place");
        assert_eq!(p.id, "W5013364");
        assert_eq!(p.name, "Eiffel Tower");
        assert_eq!(p.kind, "tower");
        assert_eq!(p.address, "Avenue Anatole France 5, Paris, Île-de-France, France");
        assert_eq!(p.lat, 48.8582599);
        // Photon interleaves its extent; ours is [w, s, e, n] and must come out sorted.
        let e = p.extent.unwrap();
        assert!(e[0] < e[2] && e[1] < e[3], "extent came out as {e:?}");
    }

    #[test]
    fn a_feature_with_no_coordinates_is_not_a_place() {
        assert!(photon_place(&json!({ "properties": { "name": "Nowhere" } })).is_none());
        assert!(photon_place(&json!({
            "properties": {}, "geometry": { "coordinates": [1.0, 2.0] }
        }))
        .is_none(), "a place with no name is not worth showing");
    }

    #[test]
    fn kinds_read_like_words_a_person_would_say() {
        assert_eq!(pretty_kind("amenity", "cafe"), "cafe");
        assert_eq!(pretty_kind("railway", "station"), "railway station");
        assert_eq!(pretty_kind("place", "city"), "city");
        assert_eq!(pretty_kind("amenity", "charging_station"), "charging station");
    }

    /// The failure this ranking exists for, with the shapes the live API actually returned:
    /// "Tokyo Station" near Shibuya came back as bus stops in Odaiba above anything with a
    /// platform. Distance alone cannot fix it — the bus stops are *closer*.
    #[test]
    fn a_station_outranks_the_bus_stop_named_after_it() {
        let at = Some([35.65948, 139.70057]);
        let stop = Place {
            name: "東京テレポート駅前".into(),
            kind: "bus stop".into(),
            lat: 35.6270,
            lon: 139.7770,
            ..Place::default()
        };
        let station = Place {
            name: "東京駅".into(),
            kind: "railway station".into(),
            lat: 35.6812,
            lon: 139.7671,
            ..Place::default()
        };
        // The index's order is the wrong one; ours has to survive that.
        let out = rank("Tokyo Station", at, vec![stop.clone(), stop.clone(), station.clone()]);
        assert_eq!(out[0].name, "東京駅", "a station must beat a bus stop named after one");
    }

    #[test]
    fn a_closer_place_wins_only_when_it_is_the_same_kind_of_thing() {
        let at = Some([48.8584, 2.2945]);
        let near = Place { name: "Cafe A".into(), kind: "cafe".into(), lat: 48.859, lon: 2.295, ..Place::default() };
        let far = Place { name: "Cafe B".into(), kind: "cafe".into(), lat: 48.90, lon: 2.40, ..Place::default() };
        let out = rank("cafe", at, vec![far, near]);
        assert_eq!(out[0].name, "Cafe A", "between equals, nearer wins");
    }

    #[test]
    fn an_exact_name_match_beats_a_vaguely_similar_one() {
        let out = rank(
            "Louvre",
            None,
            vec![
                Place { name: "Louvre Hotel Annexe".into(), kind: "hotel".into(), ..Place::default() },
                Place { name: "Louvre".into(), kind: "museum".into(), ..Place::default() },
            ],
        );
        assert_eq!(out[0].name, "Louvre");
    }

    /// Two identical rows is a question nobody can answer.
    #[test]
    fn the_same_place_mapped_twice_is_listed_once() {
        let station = |lat: f64, lon: f64| Place {
            name: "Taksim".into(),
            kind: "railway station".into(),
            lat,
            lon,
            ..Place::default()
        };
        // The two entrances the live index actually returns: 168 m apart, one station.
        let out = rank("Taksim", None, vec![station(41.03805, 28.98555), station(41.03678, 28.98663)]);
        assert_eq!(out.len(), 1);

        // Two branches of the same chain across town are two places.
        let far = rank("Starbucks", None, vec![
            Place { name: "Starbucks".into(), kind: "cafe".into(), lat: 41.0, lon: 29.0, ..Place::default() },
            Place { name: "Starbucks".into(), kind: "cafe".into(), lat: 41.05, lon: 29.05, ..Place::default() },
        ]);
        assert_eq!(far.len(), 2);
    }

    #[test]
    fn street_furniture_is_recognised_as_a_non_answer() {
        assert!(is_street_furniture("bus stop"));
        assert!(is_street_furniture("steps"));
        assert!(!is_street_furniture("railway station"));
        assert!(!is_street_furniture("cafe"));
        assert!(!is_street_furniture(""), "an unknown kind is not proof of anything");
    }

    #[test]
    fn ambiguous_categories_get_a_tag_and_open_ones_do_not() {
        assert_eq!(osm_tag("cafes").as_deref(), Some("amenity:cafe"));
        assert_eq!(osm_tag("Pharmacy").as_deref(), Some("amenity:pharmacy"));
        assert_eq!(osm_tag("bookshop"), None, "unlisted words are searched as words");
    }

    #[test]
    fn modes_take_the_word_a_person_would_use() {
        assert_eq!(Mode::parse("car"), Some(Mode::Drive));
        assert_eq!(Mode::parse("FOOT"), Some(Mode::Walk));
        assert_eq!(Mode::parse("bicycle"), Some(Mode::Bike));
        assert_eq!(Mode::parse("teleport"), None);
        assert_eq!(Mode::Drive.costing(), "auto");
        assert_eq!(Mode::Walk.costing(), "pedestrian");
    }

    #[test]
    fn distance_is_close_enough_to_order_by() {
        // Eiffel Tower → Louvre, about 3.2 km as the crow flies.
        let d = km_between([48.8584, 2.2945], [48.8606, 2.3376]);
        assert!((d - 3.2).abs() < 0.3, "got {d} km");
        assert_eq!(km_between([1.0, 2.0], [1.0, 2.0]), 0.0);
    }

    #[test]
    fn a_query_is_escaped_rather_than_pasted_into_the_url() {
        assert_eq!(enc("cafe & bar"), "cafe%20%26%20bar");
        assert_eq!(enc("Kadıköy"), "Kad%C4%B1k%C3%B6y");
        assert_eq!(enc("plain-Text_1.0~"), "plain-Text_1.0~");
    }

    /// Hours evaluation must refuse what it cannot read — a confident wrong "open" is the
    /// one answer worse than none. Weekday 0 = Monday.
    #[test]
    fn opening_hours_reads_the_common_shapes_and_refuses_the_rest() {
        // 24/7 and the everyday shapes.
        assert_eq!(open_now("24/7", 3, 100), Some(true));
        assert_eq!(open_now("Mo-Fr 09:00-18:00", 0, 10 * 60), Some(true));
        assert_eq!(open_now("Mo-Fr 09:00-18:00", 0, 8 * 60), Some(false));
        assert_eq!(open_now("Mo-Fr 09:00-18:00", 5, 10 * 60), Some(false), "Saturday is not Mo-Fr");
        // Lists of days and of time spans.
        assert_eq!(open_now("Mo,We 09:00-12:00,14:00-18:00", 2, 15 * 60), Some(true));
        assert_eq!(open_now("Mo,We 09:00-12:00,14:00-18:00", 2, 13 * 60), Some(false));
        // Across midnight — the bar shape.
        assert_eq!(open_now("Fr-Sa 22:00-02:00", 4, 23 * 60), Some(true));
        assert_eq!(open_now("Fr-Sa 22:00-02:00", 4, 60), Some(true));
        assert_eq!(open_now("Fr-Sa 22:00-02:00", 4, 12 * 60), Some(false));
        // A day marked off.
        assert_eq!(open_now("Mo-Sa 10:00-20:00; Su off", 6, 12 * 60), Some(false));
        // Bare times apply to every day.
        assert_eq!(open_now("08:00-22:00", 6, 12 * 60), Some(true));
        // The live map's own spelling: lowercase days, found on the first supermarket
        // tried. Tuesday 19:47 inside 09:00-21:00 is open.
        assert_eq!(open_now("mo-su 09:00-21:00", 1, 19 * 60 + 47), Some(true));
        // The honest refusals: exotic grammar is shown, not guessed.
        assert_eq!(open_now("sunrise-sunset", 1, 700), None);
        assert_eq!(open_now("Mo-Fr 09:00-18:00; Dec 25 off", 1, 700), None);
        assert_eq!(open_now("", 1, 700), None);
    }

    /// The gate is the rate limit, so it has to actually make the second caller wait.
    #[tokio::test]
    async fn the_gate_keeps_its_floor() {
        let g = Gate::new(Duration::from_millis(120));
        let t = Instant::now();
        g.pass().await;
        g.pass().await;
        assert!(t.elapsed() >= Duration::from_millis(110), "took only {:?}", t.elapsed());
    }

    /// A cached answer must not consult the gate at all — otherwise a repeat costs the
    /// same wall-clock as a fresh call and the cache is decorative.
    #[tokio::test]
    async fn a_repeat_question_costs_no_request() {
        let svc = Service::new(Duration::from_millis(500), Duration::from_secs(60)).unwrap();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        for _ in 0..3 {
            let c = calls.clone();
            let v = svc
                .get("k".into(), || async move {
                    c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Ok(json!({ "n": 1 }))
                })
                .await
                .unwrap();
            assert_eq!(v["n"], json!(1));
        }
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1, "asked more than once");
    }

    #[tokio::test]
    async fn an_expired_answer_is_fetched_again() {
        let svc = Service::new(Duration::ZERO, Duration::from_millis(30)).unwrap();
        svc.get("k".into(), || async { Ok(json!(1)) }).await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        let v = svc.get("k".into(), || async { Ok(json!(2)) }).await.unwrap();
        assert_eq!(v, json!(2), "a stale entry must not be served");
    }
}
