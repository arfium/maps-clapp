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
// Tower, and the route from a live Valhalla answer for a THREE-STOP walk (Eiffel Tower →
// Musée d'Orsay → Louvre, 3.94 km, two legs, its own six-digit polyline decoded). A
// fixture made of "Place 1 / Place 2" flatters a layout that real names like "Café des 2
// Moulins" would break — and a two-stop fixture would flatter a trip editor that has to
// show legs.

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
  "mode": "walk",
  "km": 3.941,
  "secs": 3117.26,
  "stops": [
    "Eiffel Tower",
    "Musee d'Orsay",
    "Louvre Museum"
  ],
  "shape": [
    [
      2.294459,
      48.858369
    ],
    [
      2.294453,
      48.85884
    ],
    [
      2.29395,
      48.858818
    ],
    [
      2.294711,
      48.858565
    ],
    [
      2.294912,
      48.859218
    ],
    [
      2.294947,
      48.859507
    ],
    [
      2.295947,
      48.85949
    ],
    [
      2.296761,
      48.859942
    ],
    [
      2.298618,
      48.860462
    ],
    [
      2.300926,
      48.860871
    ],
    [
      2.301366,
      48.860925
    ],
    [
      2.302269,
      48.861022
    ],
    [
      2.302587,
      48.861078
    ],
    [
      2.303208,
      48.861078
    ],
    [
      2.303548,
      48.861096
    ],
    [
      2.305568,
      48.861081
    ],
    [
      2.306993,
      48.86105
    ],
    [
      2.308223,
      48.861096
    ],
    [
      2.309521,
      48.861125
    ],
    [
      2.311395,
      48.861121
    ],
    [
      2.312949,
      48.861141
    ],
    [
      2.313081,
      48.86121
    ],
    [
      2.314098,
      48.861083
    ],
    [
      2.315034,
      48.86097
    ],
    [
      2.316821,
      48.860867
    ],
    [
      2.317999,
      48.860799
    ],
    [
      2.319992,
      48.86065
    ],
    [
      2.320617,
      48.8612
    ],
    [
      2.323602,
      48.860331
    ],
    [
      2.324698,
      48.860162
    ],
    [
      2.325085,
      48.860392
    ],
    [
      2.325407,
      48.860562
    ],
    [
      2.325576,
      48.860704
    ],
    [
      2.32571,
      48.86067
    ],
    [
      2.325937,
      48.860534
    ],
    [
      2.326786,
      48.860255
    ],
    [
      2.329173,
      48.859507
    ],
    [
      2.329426,
      48.859609
    ],
    [
      2.330664,
      48.860692
    ],
    [
      2.331594,
      48.860488
    ],
    [
      2.333057,
      48.86024
    ],
    [
      2.333336,
      48.860526
    ],
    [
      2.333848,
      48.861018
    ],
    [
      2.334695,
      48.860776
    ],
    [
      2.337281,
      48.860301
    ]
  ],
  "legs": [
    {
      "from": "Eiffel Tower",
      "to": "Musee d'Orsay",
      "km": 2.913,
      "secs": 2268.22,
      "steps": [
        {
          "instruction": "Walk north.",
          "km": 0.058,
          "secs": 50.7,
          "at": 0
        },
        {
          "instruction": "Turn left onto the walkway.",
          "km": 0.029,
          "secs": 36.1,
          "at": 6
        },
        {
          "instruction": "Turn right onto the walkway.",
          "km": 0.026,
          "secs": 22.3,
          "at": 10
        },
        {
          "instruction": "Turn left onto the walkway.",
          "km": 0.085,
          "secs": 116.6,
          "at": 13
        },
        {
          "instruction": "Turn left onto the walkway.",
          "km": 0.124,
          "secs": 105.9,
          "at": 17
        },
        {
          "instruction": "Turn right onto the walkway.",
          "km": 0.012,
          "secs": 8.5,
          "at": 27
        }
      ]
    },
    {
      "from": "Musee d'Orsay",
      "to": "Louvre Museum",
      "km": 1.027,
      "secs": 849.04,
      "steps": [
        {
          "instruction": "Walk southeast on the walkway.",
          "km": 0.084,
          "secs": 65.3,
          "at": 35
        },
        {
          "instruction": "Take the stairs.",
          "km": 0.008,
          "secs": 5.6,
          "at": 37
        },
        {
          "instruction": "Continue on the walkway.",
          "km": 0.106,
          "secs": 88.4,
          "at": 38
        },
        {
          "instruction": "Turn left onto Rue du Bac.",
          "km": 0.178,
          "secs": 142,
          "at": 44
        },
        {
          "instruction": "Turn right onto Quai François Mitterrand.",
          "km": 0.182,
          "secs": 166.2,
          "at": 53
        },
        {
          "instruction": "Turn left onto the walkway.",
          "km": 0.117,
          "secs": 83.6,
          "at": 64
        }
      ]
    }
  ]
} as const;

const STOPS = [
  { id: "W1", name: "Eiffel Tower", kind: "tower", address: "Avenue Anatole France, Paris", lat: 48.8584, lon: 2.2945, country: "France" },
  { id: "W2", name: "Musée d'Orsay", kind: "museum", address: "Rue de la Légion d'Honneur, Paris", lat: 48.86, lon: 2.3266, country: "France" },
  { id: "W3", name: "Louvre Museum", kind: "museum", address: "Rue de Rivoli, Paris", lat: 48.8606, lon: 2.3376, country: "France" },
] as const;

const SNAPSHOT: State = {
  ok: true,
  rev: 1,
  view: { lat: 48.8584, lon: 2.2945, zoom: 14, name: "Gros-Caillou, Paris" },
  query: "cafes",
  results: PLACES as unknown as State["results"],
  selected: PLACES[1] as unknown as State["selected"],
  trip: STOPS as unknown as State["trip"],
  awaiting: null,
  mode: "walk",
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
