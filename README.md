# maps-clapp

**A world map on one screen you and your agent share.** Wherever either of you looks,
searches or routes, the other is looking at it too.

```sh
clatch install arfium/maps-clapp
clatch run com.arfium.maps
```

## The agent side

```sh
maps goto "Shibuya Crossing"
maps nearby "cafes"
maps route "Galata" "Taksim" "Sultanahmet" --mode walk   # a trip, not three throwaway routes
maps route --optimize                                    # 18 km zigzag → 10 km, same stops
maps next                                                # light up the next leg
```

The window flies to Shibuya, drops the cafes nearest-first, and draws the walk. The place
*you* open by hand rides along on the agent's next prompt, so "how far is that from the
station?" needs no antecedent.

Fifteen verbs — the rest are `find`, `select`, `back`, `isochrone`, `pin`, `pins`, `clear`,
`export`, `status`, `focus`, `close`. `maps --help` is the whole manual.

**Nobody guesses at an ambiguous name.** "Taksim" is a square and a metro station, so the
candidates land in the shared list, the trip shows a visible gap, and `maps select 2` — or
a click — fills it and finishes the route.

## No keys, no account, no quota

| what | who |
|---|---|
| basemap + style | [OpenFreeMap](https://openfreemap.org/) (OpenStreetMap via OpenMapTiles) |
| search by name, reverse | [Photon](https://photon.komoot.io/) |
| exact addresses, cross-script names | [Nominatim](https://nominatim.openstreetmap.org/) — fallback only, 1 req/s |
| what is near here, by category | [Overpass](https://overpass-api.de/), with a mirror |
| routes, isochrones | [Valhalla](https://valhalla.openstreetmap.de/) (FOSSGIS) |

Search and nearby are two different services on purpose, because they answer two different
questions. An index matches *names*, so asking one for "cafes" at Shibuya Crossing returns
the cafe called "Cafe Shakey's" three kilometres away and misses the two hundred with
Japanese names on the street outside. A radius query answers what was actually asked.

It works on first launch with nothing to configure. **Those servers belong to other
people**, so every call passes a gate: a floor between requests, a TTL cache that answers
repeats for nothing, and a per-question lock so both surfaces asking the same thing at once
cost one request. No background polling of any kind.

**Route times carry no live traffic.** No open routing service has it, and both surfaces
say so rather than quietly presenting a rush-hour estimate as fact.

## Build

```sh
npm run pack
```

`npm run verify` builds, packages, validates, and proves the window and the CLI are talking
over the socket.

## Attribution

Map data © OpenStreetMap contributors, ODbL. Tiles and style by OpenFreeMap and
OpenMapTiles. The attribution control in the window is a licence condition, not a
decoration — it is never hidden. See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
