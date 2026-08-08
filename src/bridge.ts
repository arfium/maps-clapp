//! The wire between this window and the Rust core, typed once.
//
// Every field here is emitted by `AppState::snapshot` (src-tauri/src/state.rs) and every
// command is answered by `apply` (src-tauri/src/app.rs). If the UI reads something the core
// never sends, that is a bug in one of the two — so they are kept in step deliberately
// rather than reaching for `any`.
//
// The commands are the SAME ones the agent's CLI sends over the socket. That is not a
// coincidence to be tidied up later: it is the reason a click and a typed verb cannot
// diverge.

export type Mode = "drive" | "bike" | "walk";

export type Place = {
  id: string;
  name: string;
  /** A human phrase, not an OSM tag: "cafe", "railway station". */
  kind: string;
  address: string;
  lat: number;
  lon: number;
  country: string;
  /** `[west, south, east, north]`, when the source knows the feature's extent. */
  extent?: [number, number, number, number];
};

export type Step = {
  instruction: string;
  km: number;
  secs: number;
  /** Index into `Route.shape` where this step begins. */
  at: number;
};

export type Route = {
  mode: Mode;
  km: number;
  secs: number;
  from: string;
  to: string;
  /** `[lon, lat]` — GeoJSON order, ready for a LineString with no transposing. */
  shape: [number, number][];
  steps: Step[];
};

export type Reach = {
  minutes: number;
  mode: Mode;
  center: [number, number];
  from: string;
  ring: [number, number][];
};

export type Pin = { name: string; lat: number; lon: number; note: string };

export type View = { lat: number; lon: number; zoom: number; name: string };

export type Agent = {
  id: string;
  name: string;
  avatar?: string | null;
};

export type State = {
  ok?: boolean;
  rev?: number;
  view: View;
  /** What produced the current results — echoed into the search box. */
  query: string;
  results: Place[];
  selected: Place | null;
  route: Route | null;
  reach: Reach | null;
  pins: Pin[];
  /** What is in flight, in words. Both surfaces show this same string. */
  busy: string | null;
  /** The last thing that happened, in one sentence. */
  said: string | null;
  agents: Agent[];
};

export const EMPTY: State = {
  view: { lat: 20, lon: 0, zoom: 1.6, name: "" },
  query: "",
  results: [],
  selected: null,
  route: null,
  reach: null,
  pins: [],
  busy: null,
  said: null,
  agents: [],
};

/** Every command the core answers (src-tauri/src/app.rs `apply`). */
export type Req =
  | { cmd: "state" }
  | { cmd: "view"; lat: number; lon: number; zoom: number }
  | { cmd: "goto"; q: string }
  | { cmd: "find"; q: string }
  | { cmd: "nearby"; q: string }
  | { cmd: "select"; n: number }
  | { cmd: "route"; to: string; from?: string; mode?: Mode }
  | { cmd: "isochrone"; minutes: number; from?: string; mode?: Mode }
  | { cmd: "pin"; name?: string; at?: string; note?: string }
  | { cmd: "unpin"; n: number }
  | { cmd: "clear"; what: string }
  | { cmd: "export"; path?: string; format?: "geojson" | "gpx" };

/** 4.193 → "4.2 km". Matches `app::distance` in Rust exactly — the two surfaces must not
 *  disagree about how far away something is. */
export function distance(km: number): string {
  if (km < 1) return `${Math.round(km * 1000)} m`;
  if (km < 10) return `${km.toFixed(1)} km`;
  return `${Math.round(km)} km`;
}

/** 829.79 → "14 min". Matches `app::duration` in Rust exactly. */
export function duration(secs: number): string {
  const mins = Math.round(secs / 60);
  if (mins === 0) return "under a minute";
  if (mins < 60) return `${mins} min`;
  const h = Math.floor(mins / 60);
  const m = mins % 60;
  return m === 0 ? `${h} h` : `${h} h ${m} min`;
}

/** 41.008234 → "41.00823". Five decimals is about a metre; more is noise on screen. */
export function coords(lat: number, lon: number): string {
  return `${lat.toFixed(5)}, ${lon.toFixed(5)}`;
}

/** The categories the search box offers as one-tap chips. They are ordinary `nearby`
 *  queries — the core maps the ambiguous ones onto OSM tags (geo.rs `osm_tag`), so this
 *  list is about what a person looks for, not about what OSM calls it. */
export const CATEGORIES = [
  "cafes",
  "restaurants",
  "hotels",
  "supermarket",
  "pharmacy",
  "fuel",
  "parking",
  "atm",
  "station",
  "park",
] as const;
