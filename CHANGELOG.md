# Changelog

## 0.1.1

The release the field reports built: category words are radius searches and
speak Turkish; results wear category icon discs (map, list and suggestions,
one drawing); the panel's controls are drawn icons; the category enum lives
in the core and rides every snapshot, so `maps nearby` (bare), `status` and
`--help` all teach the same chips the window shows; the Dock inset ships as
a committed artifact so packaging needs no tool and no key.

## 0.1.0 — first release

A world map on one screen a human and an agent share.

### The app

- **Search that answers the question asked.** "Find a place by name" and "what is near
  here" are two questions, and an index only answers the first: asked for cafes at Shibuya
  Crossing, Photon returns the one called "Cafe Shakey's" 2.8 km away in Shinjuku and
  misses the two hundred with Japanese names on the street outside. So `nearby` is a radius
  query against live OpenStreetMap (Overpass), ordered by distance, with the radius taken
  from what is on screen; `find` stays with the index and re-ranks it.
- **A ranking of our own**, because relevance is not prominence. "Tokyo Station" typed near
  Shibuya came back as four bus stops in Odaiba — perfectly good text matches, and closer
  than the answer. Results are now scored on what kind of thing they are, whether the name
  echoes the query, the index's own opinion, and distance (log-scaled, so a cafe 200 m away
  does not outrank an airport). And when nothing that came back is even *called* what was
  asked for — which is what happens across scripts, where 東京駅 has no English handle in
  the index — the query falls through to Nominatim rather than confidently routing you to
  the wrong station.
- **Routes and reachable areas** from Valhalla: turn-by-turn steps in whole sentences,
  distance and duration, and an isochrone you can draw around anywhere. Both surfaces say
  out loud that the times carry **no live traffic**, because no open routing service has
  it and a quietly optimistic ETA is the one lie a map must not tell.
- **Pins that survive** searches, routes and `clear all` — they are the one thing the human
  deliberately kept, so only `clear pins` takes them.
- **Export** to GeoJSON or GPX: pins, results, the route, the reachable area.
- **Thirteen verbs**, each of them also a control in the window, over one shared state.

### Trips

`route` used to take two strings, guess a place for each and throw the previous answer
away — so there was no such thing as a waypoint, and if the guess was wrong there was
nothing to do about it but retype the name more precisely. There is no spelling of
"Taksim" that means the square rather than the metro station.

So a route is now a **trip**: an ordered list of stops in the shared state, with the line
computed *from* them. `route "A" "B" "C"` plans one, `--add` extends it, `--rm <N>` prunes
it, and every stop may be a name, an address, a coordinate, `#3` (a result already on
screen) or one of your pins. Valhalla is asked once for the whole trip, so it optimises
across the stops and answers with one leg per hop — each with its own distance, duration
and turns.

And an ambiguous name is neither guessed nor refused: the candidates go into the result
list both surfaces already share, the stop shows as "choosing", and `maps select <N>` — or
a click on the row — fills it and finishes the route. Several ambiguous stops queue up
naturally, because the placeholders *are* the queue. How you travel became shared state
too: an agent routing on foot no longer leaves the window showing "drive".

### Walking it

`route --optimize` reorders the middle stops for the shortest journey (ends fixed) —
probed before parsing, and measured on the demo trip: five Istanbul stops went from 18 km
walked to 10, 2h49 to 1h56. `next`/`back` walk the legs with a shared cursor: the window
highlights and frames the active leg, the agent's `maps next` moves the same cursor, and
a human's press rides their next prompt as the buffered `leg` signal. Turn instructions
arrive in the OS language (probed: tr-TR answers in Turkish). A selected place grows
hours/phone/website a beat later, with an "open now" that refuses grammar it cannot read.

### The window

- **The camera stops fighting you.** Every snapshot used to re-frame, which looked right
  and was unusable: with a route drawn, panning along it to read the next turn made the
  reply to your own pan snap the camera back to the route's bounding box. The map now moves
  when the *subject* changes — a new route, a different place opened — and panning is not a
  change of subject.
- **The panel hides**, because a map app is mostly map. What it covers is also what framing
  keeps clear of, so hiding it gives shapes the whole window.
- **Categories answer instantly.** The dots for fuel stations and pharmacies are already in
  the tiles on screen — OSM tagged them, OpenMapTiles carries the classification — so a tap
  is answered from what is already drawn, in the same frame, and the complete list with
  addresses replaces it a moment later. Those results go through the shared state like
  everything else, so the agent sees the head start too.
- **The session survives a restart**: the view and the pins come back, the search does not.
  Reopening in the mid-Atlantic was not a fresh start, it was amnesia — and it made every
  category button fail, because "what is nearby" has no answer in an ocean.
- **Dark follows the system**, map and panel both — and fixing it surfaced a layering
  truth: "before the first symbol layer" buried the route under the whole city in the dark
  style (its first symbol is `water_name`, before any road). Shapes now anchor above the
  last geometry layer, below every name — which also lifted routes out from under
  liberty's bridges.
- **`atm` used to time out.** "Nearby" reached as far as 20 km, which in a city is tens of
  thousands of features; the radius is capped at 5 km and Overpass gets a budget it can
  actually meet.

### The camera

The first clapp with a piece of state that moves *continuously*. A human dragging the map
changes it sixty times a second and an agent does not want sixty notifications, so the
window reports only a settled camera and the core decides whether the move was worth
mentioning — half a screen-width of panning, or two zoom levels, judged against the last
view the agent was actually told about so that accumulated drift is still reported. The
same threshold decides whether to spend a request naming the area, which bounds that cost
to a handful of lookups for somebody exploring a city.

### Keys, quotas, manners

Nothing to configure: OpenFreeMap for tiles, Photon for search, Nominatim for exact
addresses, Valhalla for routing — no accounts, no keys, no quota. Those servers are other
people's, so every call passes a gate with a floor between requests (1.1 s for Nominatim,
whose policy sets it), a TTL cache, and a per-question lock that collapses two surfaces
asking the same thing into one request. No background polling anywhere.

Nominatim's policy forbids attaching it to a typeahead, so the typeahead is Photon's and
Nominatim is only ever a fallback for an address the fuzzy index could not place.

### Things that went wrong on the way

- **`fitBounds` with padding wider than the canvas does nothing, silently.** The first
  snapshot lands while the container is still at MapLibre's 400×300 default, so the very
  first frame was lost every time. Padding is now derived from the actual canvas, and the
  camera re-frames once the container reaches its real size — unless the human has moved
  the map since, in which case the view is theirs.
- **`map.on("load")` may never fire** on a style whose sources are still settling, even
  with the sprite loaded, tiles drawn and `addLayer` perfectly legal. Every layer this app
  draws was therefore never created. It installs on `styledata` now.
- **Animated camera moves do not run while the document is hidden**, because
  `requestAnimationFrame` stops — an agent's `goto` into a minimised window would be
  swallowed and the human would restore it to the wrong place. When nothing will animate,
  the map jumps.
- **MapLibre only measures its container once.** A `ResizeObserver` keeps the canvas on the
  window, which a resizable window needs anyway.

### Built on

MapLibre GL JS, bundled rather than fetched from a CDN. clappkit as a public submodule; the
clatch crates vendored at their pinned tag, so CI needs no secret and no key. Released for
macOS and Windows.
