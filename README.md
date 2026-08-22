# Maps — a clapp

A world map on one screen you and your agent share. Wherever either of you looks, searches
or routes, the other is looking at it too.

```sh
clatch install github:arfium/maps-clapp
clatch run com.arfium.maps
```

Then, from the agent's side:

```sh
maps goto "Shibuya Crossing"
maps nearby "cafes"
maps route "Tokyo Station" --mode walk
maps route "Galata" "Karaköy" "Taksim" --mode walk   # a trip, not two throwaway routes
```

…and the human's window flies to Shibuya, drops the cafes on the map nearest-first, and
draws the walk. The place *they* open by hand rides along on their next prompt, so they can
just say "how far is that from the station?".

## What it is

One binary, two roles. `maps app` is the Tauri window Clatch launches; `maps <verb>` is the
CLI the agent drives. Both run one shared state, so there is no way for the two surfaces to
disagree — every answer either of them gets is the same full snapshot plus a sentence about
what just happened.

Fifteen verbs: `goto`, `find`, `nearby`, `select`, `route`, `next`, `back`, `isochrone`,
`pin`, `pins`, `clear`, `export`, `status`, `focus`, `close`. `maps --help` is the whole
manual.

## The two-minute demo

Plan a day with your agent, in one shared window:

```sh
maps goto "Galata Kulesi"                # the window flies there
maps route "Galata Kulesi" "Taksim Meydanı" "Sultanahmet Camii" "Cihangir" "Eminönü" --mode walk
maps route --optimize                    # 18 km zigzag → 10 km; 2h49 → 1h56, same stops
maps next                                # leg 1 lights up on their map, turns in their language
```

Then the human walks. Each time they press **Next** in the window, the new leg rides
their next prompt — "how far to the ferry?" is already about the right leg. When a stop's
name is ambiguous ("Taksim" is a square AND a metro station), nobody guesses: the
candidates land in the shared list, the trip shows a visible gap, and `maps select 2` — or
a click — fills it and finishes the route.

## No keys, no account, no quota

Everything it needs is open:

| what | who | cost |
| --- | --- | --- |
| basemap + style | [OpenFreeMap](https://openfreemap.org/) (OpenStreetMap via OpenMapTiles) | free, no key, no limit |
| search by name, reverse | [Photon](https://photon.komoot.io/) | free, fair use |
| exact addresses, cross-script names | [Nominatim](https://nominatim.openstreetmap.org/) | free, 1 request/second — fallback only |
| what is *near* here, by category | [Overpass](https://overpass-api.de/) | free, fair use — with a mirror to fall back on |
| routes, isochrones | [Valhalla](https://valhalla.openstreetmap.de/) (FOSSGIS) | free, fair use |

Search and nearby are two different services on purpose, because they are two different
questions: an index matches names, so asking one for "cafes" at Shibuya Crossing returns
the cafe called "Cafe Shakey's" three kilometres away and misses the two hundred with
Japanese names on the street outside. A radius query answers what was actually asked.

It works on first launch with nothing to configure. Those servers belong to other people,
though, so every call passes a gate that keeps a floor between requests, a TTL cache that
answers repeats for nothing, and a per-question lock so both surfaces asking the same thing
at once cost one request. There is no background polling of any kind.

**Route times have no live traffic in them.** No open routing service has traffic data, and
both surfaces say so rather than quietly presenting a rush-hour estimate as fact.

## Build it

```sh
git clone --recurse-submodules https://github.com/arfium/maps-clapp
cd maps-clapp
npm ci
npm run package      # → pkg/, the Clatch depot
npm run pack         # → com.arfium.maps-<os>-<arch>.clapp
```

`npm test` runs the Rust suite, `npm run check` asserts the manifest still describes the
code, and `npm run verify` proves the two surfaces talk to each other against real Clatch.

`npm run dev` opens the window's panels in a plain browser against a canned snapshot
(`src/preview.ts`) — real places and a real route, so the layout is judged on names like
"Café des 2 Moulins" rather than on "Place 1". The basemap still comes over the network,
which is the point: a translucent panel has to be readable over a coastline.

Released for **macOS and Windows**. The code is cross-platform and `launch.linux` stays in
the manifest because a source build works there — but nothing in CI builds or publishes it,
so treat that as unproven.

## Layout

```
src-tauri/src/
  geo.rs      the world: Photon, Overpass, Nominatim, Valhalla, and the ranking on top
  state.rs    the shared state, pure and tested — including the camera threshold
  app.rs      the wiring: one `apply` both surfaces land in
  cli.rs      the agent's surface, and the manual
  webview.rs  Windows: check for the WebView2 Runtime before opening a window
src/
  map.ts      MapLibre, and the rules for not making it slow
  App.tsx     the panel over the map
  bridge.ts   the wire, typed once
```

Everything else — the icon, the window verbs, the IPC relay, the snapshot revisions — is
[clappkit](https://github.com/arfium/clappkit), a public submodule. Read
`clappkit/docs/architecture.md` for how a clapp is put together and
`clappkit/docs/playbook.md` for the rules learned by getting them wrong.

## Attribution

Map data © OpenStreetMap contributors, ODbL. Tiles and style by OpenFreeMap and
OpenMapTiles. The attribution control in the window is a licence condition, not a
decoration — it is never hidden. See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
