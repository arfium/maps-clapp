# Operating Maps

You are driving a **map a human is looking at**. Everything you do here shows up on their
screen, and everything they do reaches you. That is the point of the app; use it.

`maps --help` is the manual. This file is the judgement around it.

## The loop that works

```sh
maps goto "Shibuya Crossing"      # flies their window there; names what is at the middle
maps nearby "cafes"               # what is around it, NEAREST FIRST, on their map too
maps select 3                     # opens one: address, coordinates, what it is
maps route "Tokyo Station"        # from what is open, drawn on the line they can see
maps pin "meet here" --note "9am" # kept; pins survive searches and routes
maps export                       # GeoJSON of everything on the map
```

`goto` when you want one place and the map moved to it. `find` when you want a list to
choose from. `nearby` when the question is "what is around here" — it is the only one that
orders by distance, because the search index does not.

## Read the state before you act

`maps status` is cheap and tells you what the human is looking at: where the map is
pointed and what that area is called, the current query and results, what is open, the
route or reachable area, the pins, and which agents are bound. Every other verb also
returns the whole state (`--json` prints it), so you rarely need a second call.

## What arrives from their side

- **A place in your chat buffer** (`place.opened`) — they opened something in the window.
  It rides their next prompt, so "how far is that from the station?" is about *that*
  place. Do not look it up again; you already know where it is.
- **A new view** (`view.changed`) — they moved the map somewhere genuinely different. It
  reaches you at your next turn. Small pans are deliberately not reported; if you get one,
  they went somewhere.
- **A pin change** (`pins.changed`) — they kept or dropped a place.

There is no fourth one. The app cannot make you take a turn — if the human wants you, they
say so in Clatch. What arrives here is context, not a summons.

## Things that will trip you up

- **Route times have no live traffic in them.** No open routing service has traffic data,
  so every duration is free-flow: right at 3am, optimistic at 6pm. If the answer depends on
  when they are travelling, say that it does.
- **Search is biased toward where the map already is** once it is zoomed past a continent.
  "airport" after `goto tokyo` is not the same question as "airport" after `goto toronto`.
  For a global search, `maps clear all` or move the view out first.
- **`nearby` needs somewhere to be near.** At world zoom it refuses rather than searching
  the planet for the closest pharmacy. `goto` somewhere first.
- **Coordinates are `lat, lon` everywhere you type them** — `maps goto "41.0082, 28.9784"`.
  Exports are GeoJSON, which is `[lon, lat]`. That flip is the classic way to put Istanbul
  in the Indian Ocean.
- **`-n` only trims what your terminal prints.** The window keeps every result either way;
  it is not a way to make the search cheaper.
- **`clear all` keeps the pins.** They are the one thing the human deliberately saved. Use
  `clear pins` when you actually mean it, and prefer to ask first.

## Where the data comes from, and what that costs

Four open services, no API keys, no accounts: **Photon** for search by name, **Overpass**
for "what is near here" (a radius query against live OSM — the only thing that can answer
it), **Nominatim** for an address, or a name, the fuzzy index could not place, and
**Valhalla** (FOSSGIS) for routes and isochrones.
None of them is ours. The app paces every call, caches answers and collapses duplicate
questions into one request, so you cannot break them by looping — but you can still be
rude. Ask for what you need, not for everything you might need.

The map is OpenStreetMap. Place names, addresses and roads are as good as the last person
who edited them: excellent in cities, patchy in the countryside, and occasionally out of
date about a business that closed. When precision matters, say where the number came from.
