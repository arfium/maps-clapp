# Changelog

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
