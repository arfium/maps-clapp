// Preview the window in a plain browser — `npm run dev`, then open http://localhost:1426.
//
// The clapp playbook says: look at your design before you ship it, on a machine that may
// have no window server. The SwiftUI clapps did that with an offscreen `ImageRenderer`; the
// Tauri equivalent is this — stub the one call the webview makes (`run_cmd`) with a canned
// snapshot, so the whole UI renders in a browser tab with no Rust and no window.
//
// The MAP still needs the network: MapLibre fetches its tiles from OpenFreeMap either way.
// That is the point — this previews the panels against a real basemap, which is the only
// way to find out that a translucent panel is unreadable over a coastline.
//
// It is DEV ONLY: `main.tsx` imports it behind `import.meta.env.DEV`, which Vite replaces
// with `false` in a production build, so the fixture is dropped from the shipped bundle.
//
// EVERY NUMBER BELOW IS REAL — the places came from a live Photon query around the Eiffel
// Tower and the route from a live Valhalla answer (4.193 km, 829.79 s, its own six-digit
// polyline decoded). A fixture made of "Place 1 / Place 2" flatters a layout that real
// names like "Café des 2 Moulins" would break.

import type { State } from "./bridge";

const PLACES = [
  {
    "id": "N5905667307",
    "name": "CAFéS",
    "kind": "cafe",
    "address": "Rue des Saints-Pères 28, Paris, Île-de-France, France",
    "lat": 48.8557833,
    "lon": 2.330632,
    "country": "France"
  },
  {
    "id": "N251699776",
    "name": "Café de Flore",
    "kind": "cafe",
    "address": "Rue Saint-Benoît, Paris, Île-de-France, France",
    "lat": 48.8541444,
    "lon": 2.3326307,
    "country": "France"
  },
  {
    "id": "N689144551",
    "name": "Café du Cadran",
    "kind": "cafe",
    "address": "Rue Daunou 1, Paris, Île-de-France, France",
    "lat": 48.8690244,
    "lon": 2.3325864,
    "country": "France"
  },
  {
    "id": "N1828149078",
    "name": "Café M",
    "kind": "cafe",
    "address": "Rue du Faubourg Montmartre, Paris, Île-de-France, France",
    "lat": 48.8761627,
    "lon": 2.3401745,
    "country": "France"
  },
  {
    "id": "N9907698037",
    "name": "Cafe",
    "kind": "cafe",
    "address": "Rue Quincampoix 15, Paris, Île-de-France, France",
    "lat": 48.8597847,
    "lon": 2.3498489,
    "country": "France"
  },
  {
    "id": "N408952536",
    "name": "Café des 2 Moulins",
    "kind": "cafe",
    "address": "Rue Cauchois, Paris, Île-de-France, France",
    "lat": 48.8849197,
    "lon": 2.333654,
    "country": "France"
  }
] as const;

const ROUTE = {
  "mode": "drive",
  "km": 4.193,
  "secs": 829.79,
  "from": "Eiffel Tower",
  "to": "Louvre Museum",
  "shape": [
    [
      2.29348,
      48.859039
    ],
    [
      2.296244,
      48.860952
    ],
    [
      2.299785,
      48.862122
    ],
    [
      2.301381,
      48.862261
    ],
    [
      2.302446,
      48.86239
    ],
    [
      2.305957,
      48.862463
    ],
    [
      2.310419,
      48.862575
    ],
    [
      2.311856,
      48.862816
    ],
    [
      2.314556,
      48.862866
    ],
    [
      2.315407,
      48.862873
    ],
    [
      2.318641,
      48.862631
    ],
    [
      2.319123,
      48.862741
    ],
    [
      2.320049,
      48.863994
    ],
    [
      2.320638,
      48.863904
    ],
    [
      2.32759,
      48.861666
    ],
    [
      2.330696,
      48.860683
    ],
    [
      2.332942,
      48.860201
    ],
    [
      2.333695,
      48.860033
    ],
    [
      2.337975,
      48.859246
    ],
    [
      2.33996,
      48.859204
    ],
    [
      2.340755,
      48.860773
    ],
    [
      2.340318,
      48.861144
    ]
  ],
  "steps": [
    {
      "instruction": "Drive northeast on Quai Jacques Chirac.",
      "km": 2.014,
      "secs": 386.1,
      "at": 0
    },
    {
      "instruction": "Turn left onto Pont de la Concorde.",
      "km": 0.19,
      "secs": 41.6,
      "at": 84
    },
    {
      "instruction": "Turn right onto Quai des Tuileries.",
      "km": 1.562,
      "secs": 302.4,
      "at": 101
    },
    {
      "instruction": "Turn left onto Rue de l'Amiral de Coligny.",
      "km": 0.231,
      "secs": 48.8,
      "at": 150
    },
    {
      "instruction": "Turn left onto Rue de Rivoli.",
      "km": 0.195,
      "secs": 50.9,
      "at": 165
    },
    {
      "instruction": "Your destination is on the left.",
      "km": 0,
      "secs": 0,
      "at": 175
    }
  ]
} as const;

const SNAPSHOT: State = {
  ok: true,
  rev: 1,
  view: { lat: 48.8584, lon: 2.2945, zoom: 14, name: "Gros-Caillou, Paris" },
  query: "cafes",
  results: PLACES as unknown as State["results"],
  selected: PLACES[1] as unknown as State["selected"],
  route: ROUTE as unknown as State["route"],
  reach: null,
  pins: [
    { name: "Eiffel Tower", lat: 48.8584, lon: 2.2945, note: "meet here at 9" },
    { name: "Gare du Nord", lat: 48.8809, lon: 2.3553, note: "" },
  ],
  busy: null,
  said: null,
  agents: [{ id: "1001", name: "Claude", avatar: null }],
};

/** Stand in for Tauri's bridge: answer every command with the same canned snapshot, and
 *  bump `rev` so the map surface treats each one as newer than the last. */
export function installPreview() {
  let rev = 1;
  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {
    invoke: async (_cmd: string, args: Record<string, unknown>) => {
      const req = (args?.req ?? {}) as { cmd?: string };
      // `view` is the window telling the core where it moved; the core answers with state,
      // and answering with a NEW rev here would fight the user for the camera.
      if (req.cmd === "view") return { ...SNAPSHOT, rev };
      return { ...SNAPSHOT, rev: ++rev, ok: true, message: `preview: ${req.cmd ?? "state"}` };
    },
  };
}
