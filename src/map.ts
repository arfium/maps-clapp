//! The map: one MapLibre instance, and a function that makes it show a snapshot.
//
// This file follows MapLibre's own hard-won rules, the ones their agent-skills repo exists
// to stop people rediscovering:
//
//   * **No `Marker`.** A Marker is a DOM element, and a few hundred of them make a browser
//     crawl. Every point here is a feature in a GeoJSON source drawn by a circle or symbol
//     layer — that is GPU work, and it scales to tens of thousands.
//   * **The map is created once and never re-styled.** `setStyle` tears down and rebuilds
//     every source and layer, and `map.remove()` leaks. Updates go through `setData` on a
//     source that already exists.
//   * **Our layers go UNDER the basemap's labels.** Appending to the top of the stack hides
//     the place names, which are the most useful thing on a map.
//   * **Glyphs and sprite come from the style.** Missing either is the number-one cause of
//     a map that renders roads and no text at all. OpenFreeMap hosts both itself.
//
// The camera is a two-way binding and therefore the one genuinely tricky thing here: the
// core can move the map (`goto`), and the human can move it by dragging. `apply` marks its
// own camera changes so that the `moveend` they cause is not reported back as if the human
// had done it — otherwise the two surfaces would push each other around forever.

import maplibregl, { Map as MLMap, MapGeoJSONFeature } from "maplibre-gl";
import type { State } from "./bridge";

/// OpenFreeMap's Liberty style: OpenStreetMap data, no API key, no registration, no limit,
/// and it hosts its own glyphs and sprite. Attribution is required and MapLibre adds it
/// from the style automatically — do not remove the control.
const STYLE = "https://tiles.openfreemap.org/styles/liberty";

/// How long after the last camera movement we call it settled. The core hears about the
/// view once per settle, not once per frame — 250 ms is under the threshold of feeling
/// laggy and well over the gap between two drag events.
const SETTLE_MS = 250;

/// Below this much difference we consider the map already to be where the state says, and
/// leave it alone. Without it, floating-point round-tripping through the snapshot would
/// nudge the camera on every push.
const SAME_PLACE = { deg: 1e-4, zoom: 0.01 };

const EMPTY_FC = { type: "FeatureCollection", features: [] } as const;

/// How much of the map's left edge the floating panel covers (its width plus its margins).
/// Framing keeps shapes clear of it — capped against the actual canvas, see `padding`.
const PANEL_CLEARANCE = 380;

/** Will an animated camera move actually happen?
 *
 * MapLibre animates with `requestAnimationFrame`, which browsers stop delivering while a
 * document is hidden — a minimised or fully occluded window, or a headless preview. An
 * animated `flyTo` issued then does not run *and does not finish*: the map silently stays
 * where it was, so an agent's `goto` would be swallowed and the human would restore the
 * window to the wrong place. When nothing will animate, jump instead. */
function animates(): boolean {
  return typeof document === "undefined" || document.visibilityState === "visible";
}

export type MapEvents = {
  /** The human moved the map and it has settled. */
  onMove: (lat: number, lon: number, zoom: number) => void;
  /** The human clicked a result; `n` is 1-based, the number both surfaces show. */
  onPick: (n: number) => void;
};

export class MapSurface {
  private map: MLMap;
  private ready = false;
  /** Set while `apply` is driving the camera, so the resulting `moveend` is not reported
   *  back to the core as a human's move. */
  private driving = false;
  private settle: ReturnType<typeof setTimeout> | null = null;
  private lastRev = -1;
  /** The last snapshot we framed the camera for, so a resize can frame it again. */
  private framed: State | null = null;
  /** Has the human moved the map since we last framed it? Then it is theirs, not ours. */
  private humanMoved = false;

  constructor(container: HTMLElement, ev: MapEvents) {
    this.map = new maplibregl.Map({
      container,
      style: STYLE,
      center: [0, 20],
      zoom: 1.6,
      attributionControl: { compact: true },
      // The basemap's own language, rather than forcing English onto every label: a map of
      // Tokyo that says 東京 is the map the place actually uses.
      hash: false,
    });

    this.map.addControl(new maplibregl.NavigationControl({ showCompass: true }), "top-right");
    this.map.addControl(new maplibregl.ScaleControl({ unit: "metric" }), "bottom-left");

    // MapLibre measures its container ONCE, at construction, and then only watches the
    // window. Both of those are wrong here:
    //
    //   * at construction the container can still be 0×0 — in dev, Vite injects the
    //     stylesheet from JavaScript, so the first layout may happen after this runs, and
    //     the map would sit at its 400×300 default forever behind a full-size div;
    //   * the app's window is resizable, and the panel over the map is not, so the canvas
    //     has to follow the container rather than the window.
    //
    // Observing the container covers both, and costs one callback per resize.
    //
    // Re-framing after the resize matters as much as the resize itself: the first snapshot
    // usually arrives while the canvas is still at its 400×300 default, and a `fitBounds`
    // whose padding is wider than the viewport does nothing at all. So once the container
    // has its real size, frame again — but only if the human has not taken the map
    // somewhere themselves since, because then the frame is no longer theirs to move.
    new ResizeObserver(() => {
      this.map.resize();
      if (this.framed && !this.humanMoved) this.frame(this.framed);
    }).observe(container);

    // Dev only, and dropped from the shipped bundle with the rest of `import.meta.env.DEV`:
    // a handle on the map from the browser console, so "why is it not where I told it to
    // be" is a question you can answer by asking the map.
    if (import.meta.env.DEV) (window as unknown as Record<string, unknown>).__map = this.map;

    // `styledata`, not `load`.
    //
    // `load` promises "style parsed AND first frame painted", and it is what every tutorial
    // uses — but it depends on `isStyleLoaded()` flipping true, and that can stay false
    // indefinitely on a style whose sources are still settling. Observed here: sprite
    // loaded, tiles drawn, `addSource`/`addLayer` both perfectly legal, and `load` never
    // fired — so every layer this app draws was simply never created.
    //
    // `styledata` fires as soon as the style JSON is in, which is precisely the moment
    // adding sources and layers becomes legal, and it fires again on any later style
    // change. Hence the guard: install once, on whichever comes first.
    const boot = () => {
      if (this.ready) return;
      this.install();
      this.ready = true;
    };
    if (this.map.isStyleLoaded()) boot();
    else this.map.on("styledata", boot);

    this.map.on("moveend", () => {
      if (this.driving) {
        this.driving = false;
        return;
      }
      this.humanMoved = true;
      if (this.settle) clearTimeout(this.settle);
      this.settle = setTimeout(() => {
        const c = this.map.getCenter();
        ev.onMove(c.lat, c.lng, this.map.getZoom());
      }, SETTLE_MS);
    });

    // One click handler for the whole result layer, not one per point.
    this.map.on("click", "results", (e) => {
      const f = e.features?.[0] as MapGeoJSONFeature | undefined;
      const n = f?.properties?.n;
      if (typeof n === "number") ev.onPick(n);
    });
    for (const layer of ["results", "pins"]) {
      this.map.on("mouseenter", layer, () => (this.map.getCanvas().style.cursor = "pointer"));
      this.map.on("mouseleave", layer, () => (this.map.getCanvas().style.cursor = ""));
    }
  }

  /** Every source and layer this app draws, created once on style load. */
  private install() {
    const m = this.map;
    for (const id of ["reach", "route", "results", "pins", "selected"]) {
      m.addSource(id, { type: "geojson", data: EMPTY_FC as never });
    }

    // Anything that is not a label goes below the basemap's first symbol layer, so place
    // names stay on top of our shapes instead of under them.
    const firstSymbol = m.getStyle().layers?.find((l) => l.type === "symbol")?.id;

    m.addLayer(
      {
        id: "reach-fill",
        type: "fill",
        source: "reach",
        paint: { "fill-color": "#14C08A", "fill-opacity": 0.18 },
      },
      firstSymbol,
    );
    m.addLayer(
      {
        id: "reach-line",
        type: "line",
        source: "reach",
        paint: { "line-color": "#0B8A66", "line-width": 2, "line-dasharray": [2, 1.5] },
      },
      firstSymbol,
    );
    // A casing under the route line: a coloured line alone disappears over a coloured road.
    m.addLayer(
      {
        id: "route-casing",
        type: "line",
        source: "route",
        layout: { "line-cap": "round", "line-join": "round" },
        paint: { "line-color": "#FFFFFF", "line-width": 9, "line-opacity": 0.9 },
      },
      firstSymbol,
    );
    m.addLayer(
      {
        id: "route-line",
        type: "line",
        source: "route",
        layout: { "line-cap": "round", "line-join": "round" },
        paint: { "line-color": "#1268D4", "line-width": 5 },
      },
      firstSymbol,
    );

    // Points go above the labels: a result you cannot see is not a result.
    m.addLayer({
      id: "selected-halo",
      type: "circle",
      source: "selected",
      paint: {
        "circle-radius": 14,
        "circle-color": "#0E9F70",
        "circle-opacity": 0.25,
        "circle-stroke-width": 2,
        "circle-stroke-color": "#0B8A66",
      },
    });
    m.addLayer({
      id: "results",
      type: "circle",
      source: "results",
      paint: {
        "circle-radius": 7,
        "circle-color": "#0E9F70",
        "circle-stroke-width": 2,
        "circle-stroke-color": "#FFFFFF",
      },
    });
    m.addLayer({
      id: "results-label",
      type: "symbol",
      source: "results",
      layout: {
        "text-field": ["get", "label"],
        "text-font": ["Noto Sans Regular"],
        "text-size": 12,
        "text-offset": [0, 1.1],
        "text-anchor": "top",
        // Let MapLibre drop labels that would collide rather than drawing a smear. This is
        // the default, and it is the right one here.
        "text-optional": true,
      },
      paint: {
        "text-color": "#123",
        "text-halo-color": "#FFFFFF",
        "text-halo-width": 1.6,
      },
    });
    m.addLayer({
      id: "pins",
      type: "circle",
      source: "pins",
      paint: {
        "circle-radius": 8,
        "circle-color": "#E4572E",
        "circle-stroke-width": 2.5,
        "circle-stroke-color": "#FFFFFF",
      },
    });
    m.addLayer({
      id: "pins-label",
      type: "symbol",
      source: "pins",
      layout: {
        "text-field": ["get", "name"],
        "text-font": ["Noto Sans Medium"],
        "text-size": 12,
        "text-offset": [0, 1.2],
        "text-anchor": "top",
      },
      paint: { "text-color": "#7A2A12", "text-halo-color": "#FFFFFF", "text-halo-width": 1.8 },
    });
  }

  /** Make the map show this snapshot. Safe to call before the style has loaded. */
  apply(s: State) {
    if (!this.ready) {
      // Same reasoning as `boot` above: wait for the style, not for `load`.
      this.map.once("styledata", () => this.apply(s));
      return;
    }
    // Snapshots can arrive out of order (a slow reply behind a fast push); `rev` is
    // monotonic, so an older one is simply dropped.
    if (s.rev !== undefined) {
      if (s.rev < this.lastRev) return;
      this.lastRev = s.rev;
    }

    this.set("results", {
      type: "FeatureCollection",
      features: s.results.map((p, i) => ({
        type: "Feature",
        geometry: { type: "Point", coordinates: [p.lon, p.lat] },
        properties: { n: i + 1, label: p.name, kind: p.kind },
      })),
    });
    this.set("pins", {
      type: "FeatureCollection",
      features: s.pins.map((p) => ({
        type: "Feature",
        geometry: { type: "Point", coordinates: [p.lon, p.lat] },
        properties: { name: p.name, note: p.note },
      })),
    });
    this.set(
      "selected",
      s.selected
        ? {
            type: "FeatureCollection",
            features: [
              {
                type: "Feature",
                geometry: { type: "Point", coordinates: [s.selected.lon, s.selected.lat] },
                properties: {},
              },
            ],
          }
        : EMPTY_FC,
    );
    this.set(
      "route",
      s.route
        ? {
            type: "FeatureCollection",
            features: [
              { type: "Feature", geometry: { type: "LineString", coordinates: s.route.shape }, properties: {} },
            ],
          }
        : EMPTY_FC,
    );
    this.set(
      "reach",
      s.reach
        ? {
            type: "FeatureCollection",
            features: [
              { type: "Feature", geometry: { type: "Polygon", coordinates: [s.reach.ring] }, properties: {} },
            ],
          }
        : EMPTY_FC,
    );

    this.frame(s);
  }

  /** Point the camera at whatever this snapshot is about. */
  private frame(s: State) {
    this.framed = s;
    this.humanMoved = false;
    // A route or an area is about its whole shape, so frame it rather than centring on a
    // point somewhere along it.
    if (s.route && s.route.shape.length > 1) this.fit(s.route.shape);
    else if (s.reach && s.reach.ring.length > 3) this.fit(s.reach.ring);
    else this.goto(s.view.lat, s.view.lon, s.view.zoom);
  }

  private set(id: string, data: unknown) {
    (this.map.getSource(id) as maplibregl.GeoJSONSource | undefined)?.setData(data as never);
  }

  private goto(lat: number, lon: number, zoom: number) {
    const c = this.map.getCenter();
    if (
      Math.abs(c.lat - lat) < SAME_PLACE.deg &&
      Math.abs(c.lng - lon) < SAME_PLACE.deg &&
      Math.abs(this.map.getZoom() - zoom) < SAME_PLACE.zoom
    ) {
      return;
    }
    this.driving = true;
    // Fly for a short hop so the human can see where they were taken from; jump across
    // continents, because a four-second flight over the Atlantic is a delay, not a story.
    // And jump whenever an animation would not run at all — see `animates`.
    const far = Math.abs(c.lat - lat) > 20 || Math.abs(c.lng - lon) > 20;
    if (far || !animates()) this.map.jumpTo({ center: [lon, lat], zoom });
    else this.map.flyTo({ center: [lon, lat], zoom, speed: 1.4, essential: true });
  }

  private fit(coords: [number, number][]) {
    const b = new maplibregl.LngLatBounds(coords[0], coords[0]);
    for (const c of coords) b.extend(c);
    this.driving = true;
    this.map.fitBounds(b, { padding: this.padding(), duration: animates() ? 700 : 0 });
  }

  /** Keep the shape clear of the floating panel — but never ask for more padding than the
   *  canvas has room for. `fitBounds` with padding wider than the viewport does nothing at
   *  all, silently, which is how the very first frame used to be lost: the snapshot lands
   *  while the canvas is still at its 400×300 default and 380+70 does not fit in 400. */
  private padding() {
    const w = this.map.getCanvas().clientWidth || 0;
    const h = this.map.getCanvas().clientHeight || 0;
    const side = Math.max(16, Math.min(PANEL_CLEARANCE, w * 0.35));
    const edge = Math.max(12, Math.min(70, h * 0.12));
    return { top: edge, bottom: edge, left: side, right: edge };
  }
}
