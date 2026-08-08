# Third-party notices

This app is a client for open map services. It ships no map data of its own; everything you
see comes from the services below, under their licences.

## OpenStreetMap

All place names, addresses, roads and geometry ultimately come from **OpenStreetMap**,
© OpenStreetMap contributors, licensed under the
[Open Database License (ODbL) 1.0](https://opendatacommons.org/licenses/odbl/).

Attribution is a **condition of that licence**, not a courtesy. The window shows it through
MapLibre's attribution control, fed from the style itself; the control is styled to sit
comfortably and is never hidden or covered. Anything exported from this app that carries
OSM-derived geometry carries the same obligation onward.

## OpenFreeMap and OpenMapTiles

The basemap tiles and the `liberty` style are hosted by
[**OpenFreeMap**](https://openfreemap.org/), built with
[**OpenMapTiles**](https://openmaptiles.org/) from OpenStreetMap data. OpenFreeMap's
production setup is MIT licensed and its public instance is free for commercial use with no
key, no registration and no rate limit; attribution — "OpenFreeMap © OpenMapTiles Data from
OpenStreetMap" — is required and is what the control displays.

It is donation-funded. If this app is useful to you at any scale, consider sponsoring them.

## Photon

Place search, category search and reverse geocoding use
[**Photon**](https://github.com/komoot/photon) (Apache 2.0), on the public instance komoot
operates at `photon.komoot.io`. Their terms are fair use: "you are welcome to use the API
for your project as long as the number of requests stays in a reasonable limit." The app
paces and caches every call to keep it there.

## Nominatim

Exact address lookups that Photon could not place fall through to
[**Nominatim**](https://nominatim.org/) on the OSM Foundation's public instance, under
their [usage policy](https://operations.osmfoundation.org/policies/nominatim/): at most one
request per second, an identifying User-Agent on every call, and **no autocomplete**. This
app honours all three — the typeahead is Photon's, never Nominatim's, and the gate in
`geo.rs` is set to 1.1 s because the limit is measured on their clock, not ours.

## Valhalla and FOSSGIS

Routes and isochrones come from [**Valhalla**](https://github.com/valhalla/valhalla)
(MIT), on the public instance [FOSSGIS e.V.](https://valhalla.openstreetmap.de/) hosts.
Their fair-usage policy asks apps that ship to end users to identify themselves with an
`X-Client-Id` header; this app sends `maps-clapp`.

## MapLibre GL JS

The map is rendered by [**MapLibre GL JS**](https://github.com/maplibre/maplibre-gl-js),
BSD-3-Clause, bundled into the app rather than loaded from a CDN — a packaged clapp reads
its frontend from disk, and a `<script src="https://…">` would be a map that only works
while somebody else's CDN does.

Fonts (glyph SDFs) and the icon sprite are served by OpenFreeMap as part of the style.

## The mark

`assets/icon.svg` is this app's own drawing — a pin on a graticule. It deliberately does
not resemble the mark of any commercial map service. No vendor's logo is used or implied.

## Not affiliated

This is an independent client for public, open map services. It is not made by, endorsed
by, or affiliated with OpenStreetMap, the OSM Foundation, OpenFreeMap, komoot, FOSSGIS, or
MapLibre.
