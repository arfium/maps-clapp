//! The Maps window: look, search, route — next to an agent doing the same thing.
//!
//! Every control here sends the SAME command the agent's CLI sends (`bridge.ts` `Req`), so
//! there is no view state to fall out of step: a chip click *is* a `nearby`, a result row
//! *is* `select`, and both show up in the agent's next `maps status`. In return the window
//! adopts whatever the core pushes — which is how an agent's `maps goto "Shibuya"` flies
//! this map while you watch.
//!
//! The map itself lives in `map.ts` and is deliberately NOT a React component: MapLibre owns
//! a canvas and a lot of GPU state, and re-mounting it on a render is how you leak it. React
//! renders the panels; the map is handed each snapshot and gets on with it.

import { useCallback, useEffect, useRef, useState } from "react";
import { agentTint, cmd, prefetchAssets, useAsset, useSnapshot } from "@clappkit";
import { MapSurface } from "./map";
import {
  CATEGORIES, EMPTY, TILE_CLASSES, coords, distance, duration,
  type Agent, type Mode, type Place, type Req, type State,
} from "./bridge";

/** What the core said about the last thing we asked for. */
type Note = { text: string; bad?: boolean } | null;

/** The panel's width plus its margins — what the map has to keep clear of it. Kept in step
 *  with `.panel` in styles.css by hand, because the map needs the number before the panel
 *  has been laid out. */
const PANEL_WIDTH = 380;

export default function App() {
  const { state, apply } = useSnapshot<State, Req>(EMPTY, { initial: { cmd: "state" } });
  const [note, setNote] = useState<Note>(null);
  const [text, setText] = useState("");
  const [mode, setMode] = useState<Mode>("drive");
  const [routeTo, setRouteTo] = useState("");
  const [panel, setPanel] = useState<"results" | "route" | "pins">("results");
  // The map is the point of a map app, so the panel gets out of the way on request. The
  // map is told, because framing has to use the width it actually has.
  const [hidden, setHidden] = useState(false);

  // "Nearby" needs somewhere to be near. Zoomed out to a hemisphere there is no such
  // place, and the category buttons used to fail with a sentence nobody reads — so they
  // say what they need instead of pretending to work.
  const somewhere = state.view.zoom >= 6;

  // Every action goes through here rather than `useSnapshot`'s `run`, because the core's
  // answer carries a sentence as well as the state — the export's path, the reason a place
  // could not be found — and dropping it would make the window quieter than the terminal
  // about the very same command.
  const say = useCallback(
    async (req: Req) => {
      try {
        const v = await cmd<State & { ok?: boolean; message?: string; error?: string }, Req>(req);
        apply(v); // an `ok: false` answer is ignored by the hook — the state survives
        const t = v.ok === false ? v.error : v.message;
        setNote(t ? { text: t, bad: v.ok === false } : null);
      } catch (e) {
        setNote({ text: String(e), bad: true });
      }
    },
    [apply],
  );

  // ── the map ───────────────────────────────────────────────────────────────
  const host = useRef<HTMLDivElement>(null);
  const surface = useRef<MapSurface | null>(null);
  // `say` is stable, but the map is built once and must not capture a stale closure, so the
  // handlers read through a ref.
  const sayRef = useRef(say);
  sayRef.current = say;

  useEffect(() => {
    if (!host.current || surface.current) return;
    surface.current = new MapSurface(host.current, {
      // The human dragged the map. This is a plain command like any other — the core
      // decides whether it is worth telling the agent about (state.rs `worth_announcing`),
      // because that is a rule about shared state, not about a canvas.
      onMove: (lat, lon, zoom) => void cmd<State, Req>({ cmd: "view", lat, lon, zoom }),
      onPick: (n) => void sayRef.current({ cmd: "select", n }),
    });
  }, []);

  useEffect(() => {
    surface.current?.apply(state);
  }, [state]);

  useEffect(() => {
    surface.current?.setPanelWidth(hidden ? 0 : PANEL_WIDTH);
  }, [hidden]);

  useEffect(() => {
    prefetchAssets(state.agents.map((a) => a.avatar));
  }, [state.agents]);

  useEffect(() => {
    if (!note) return;
    const t = window.setTimeout(() => setNote(null), note.bad ? 9000 : 6000);
    return () => window.clearTimeout(t);
  }, [note]);

  // The window's own copy of the search box follows the core, so an agent's `find` types
  // itself in here — but not while the human is mid-word.
  const typing = useRef(false);
  useEffect(() => {
    if (!typing.current) setText(state.query);
  }, [state.query]);

  useEffect(() => {
    if (state.route) setPanel("route");
    else if (state.results.length) setPanel("results");
  }, [state.route, state.results.length]);

  // Tapping a category answers itself first.
  //
  // The dots for fuel stations and pharmacies are already in the basemap tiles on screen —
  // OSM tagged them and OpenMapTiles carries that classification — so the honest first
  // answer costs nothing and arrives in the same frame as the click. The real query goes
  // out immediately behind it and replaces the list with something complete, addresses and
  // all. When the tiles have nothing to say (zoomed out, or a category the schema does not
  // carry), this simply does nothing and the server answers as before.
  const category = (c: string) => {
    const classes = TILE_CLASSES[c];
    const seen = classes ? (surface.current?.poisNearby(classes, 40) ?? []) : [];
    if (seen.length >= 3) {
      void cmd<State, Req>({
        cmd: "seed",
        q: c,
        places: seen.map(({ id, name, kind, lat, lon }) => ({ id, name, kind, lat, lon })),
      }).then(apply);
    }
    void say({ cmd: "nearby", q: c });
  };

  const search = (q: string) => {
    if (!q.trim()) return;
    typing.current = false;
    void say({ cmd: "find", q });
  };

  return (
    <div className={hidden ? "app hidden-panel" : "app"}>
      <div className="map" ref={host} />

      <button
        className="reveal"
        title="Show the panel"
        aria-label="Show the panel"
        onClick={() => setHidden(false)}
      >
        <Logo />
      </button>

      <aside className="panel">
        <header className="brand">
          <Logo />
          <div className="where">
            <strong>{state.view.name || "the world"}</strong>
            <span>{coords(state.view.lat, state.view.lon)} · z{state.view.zoom.toFixed(0)}</span>
          </div>
          <Agents agents={state.agents} />
          <button
            className="collapse"
            title="Hide the panel"
            aria-label="Hide the panel"
            onClick={() => setHidden(true)}
          >
            ‹
          </button>
        </header>

        <form
          className="search"
          onSubmit={(e) => {
            e.preventDefault();
            search(text);
          }}
        >
          <input
            value={text}
            placeholder="Search anywhere…"
            onChange={(e) => {
              typing.current = true;
              setText(e.target.value);
            }}
            onBlur={() => (typing.current = false)}
          />
          <button type="submit" disabled={!text.trim()}>
            Find
          </button>
          <button
            type="button"
            className="ghost"
            title="Fly straight to the best match"
            disabled={!text.trim()}
            onClick={() => {
              typing.current = false;
              void say({ cmd: "goto", q: text });
            }}
          >
            Go
          </button>
        </form>

        <div className="chips">
          {CATEGORIES.map((c) => (
            <button
              key={c}
              disabled={!somewhere}
              title={somewhere ? `What ${c} are around here` : "Find a place first — “nearby” needs somewhere to be near"}
              onClick={() => category(c)}
            >
              {c}
            </button>
          ))}
        </div>

        <nav className="tabs">
          <Tab id="results" now={panel} set={setPanel} n={state.results.length}>
            Results
          </Tab>
          <Tab id="route" now={panel} set={setPanel} n={state.route ? state.route.steps.length : 0}>
            Route
          </Tab>
          <Tab id="pins" now={panel} set={setPanel} n={state.pins.length}>
            Pins
          </Tab>
        </nav>

        <div className="scroll">
          {panel === "results" && (
            <Results
              state={state}
              somewhere={somewhere}
              onPick={(n) => void say({ cmd: "select", n })}
            />
          )}
          {panel === "route" && (
            <RoutePanel
              state={state}
              to={routeTo}
              setTo={setRouteTo}
              mode={mode}
              setMode={setMode}
              say={say}
            />
          )}
          {panel === "pins" && <Pins state={state} say={say} />}
        </div>

        <footer className="foot">
          <button onClick={() => void say({ cmd: "pin" })} title="Keep what is open, or where you are">
            Pin this
          </button>
          <button onClick={() => void say({ cmd: "isochrone", minutes: 15, mode: "walk" })}>
            15 min walk
          </button>
          <button className="ghost" onClick={() => void say({ cmd: "clear", what: "all" })}>
            Clear
          </button>
          <button className="ghost" onClick={() => void say({ cmd: "export" })}>
            Export
          </button>
        </footer>
      </aside>

      {state.busy && (
        <div className="busy" role="status">
          <i /> {state.busy}…
        </div>
      )}
      {note && <div className={note.bad ? "note bad" : "note"}>{note.text}</div>}
    </div>
  );
}

// ─────────────────────────────────────────────────────────────────────────────

function Tab(props: {
  id: "results" | "route" | "pins";
  now: string;
  set: (v: "results" | "route" | "pins") => void;
  n: number;
  children: React.ReactNode;
}) {
  return (
    <button
      className={props.now === props.id ? "on" : ""}
      onClick={() => props.set(props.id)}
    >
      {props.children}
      {props.n > 0 && <b>{props.n}</b>}
    </button>
  );
}

function Results(props: { state: State; somewhere: boolean; onPick: (n: number) => void }) {
  const { state, somewhere, onPick } = props;
  if (!state.results.length) {
    return (
      <p className="empty">
        {somewhere
          ? "Nothing here yet. Search for a place, or tap a category to see what is around the middle of the map."
          : "Search for a place to start. The category buttons look around wherever the map is, so they wake up once you are somewhere."}
      </p>
    );
  }
  return (
    <ol className="results">
      {state.results.map((p, i) => (
        <li
          key={p.id + i}
          className={state.selected?.id === p.id ? "on" : ""}
          onClick={() => onPick(i + 1)}
        >
          <span className="n">{i + 1}</span>
          <div>
            <strong>{p.name}</strong>
            {p.kind && <em>{p.kind}</em>}
            {p.address && <span className="addr">{p.address}</span>}
          </div>
        </li>
      ))}
    </ol>
  );
}

function RoutePanel(props: {
  state: State;
  to: string;
  setTo: (v: string) => void;
  mode: Mode;
  setMode: (m: Mode) => void;
  say: (r: Req) => void;
}) {
  const { state, to, setTo, mode, setMode, say } = props;
  const from = state.selected?.name ?? state.view.name ?? "here";
  return (
    <div className="route">
      <form
        onSubmit={(e) => {
          e.preventDefault();
          if (to.trim()) say({ cmd: "route", to, mode });
        }}
      >
        <label>
          From <strong>{from}</strong>
        </label>
        <input value={to} placeholder="To…" onChange={(e) => setTo(e.target.value)} />
        <div className="modes">
          {(["drive", "bike", "walk"] as Mode[]).map((m) => (
            <button
              key={m}
              type="button"
              className={mode === m ? "on" : ""}
              onClick={() => setMode(m)}
            >
              {m}
            </button>
          ))}
        </div>
        <button type="submit" disabled={!to.trim()}>
          Route
        </button>
      </form>

      {state.route && (
        <>
          <div className="summary">
            <strong>{duration(state.route.secs)}</strong>
            <span>{distance(state.route.km)}</span>
            {/* The one thing a routing answer must never let anybody assume. */}
            <em title="No open routing service has traffic data">no live traffic</em>
          </div>
          <ol className="steps">
            {state.route.steps.map((s, i) => (
              <li key={i}>
                <span>{s.instruction}</span>
                {s.km >= 0.05 && <b>{distance(s.km)}</b>}
              </li>
            ))}
          </ol>
        </>
      )}
      {state.reach && (
        <p className="empty">
          Showing {state.reach.minutes} minutes {state.reach.mode} from {state.reach.from}.
        </p>
      )}
    </div>
  );
}

function Pins({ state, say }: { state: State; say: (r: Req) => void }) {
  if (!state.pins.length) {
    return <p className="empty">Nothing kept yet. “Pin this” keeps the place you are looking at.</p>;
  }
  return (
    <ol className="pins">
      {state.pins.map((p, i) => (
        <li key={i}>
          <div>
            <strong>{p.name}</strong>
            <span className="addr">{coords(p.lat, p.lon)}</span>
            {p.note && <em>{p.note}</em>}
          </div>
          <button className="ghost" title="Remove" onClick={() => say({ cmd: "unpin", n: i + 1 })}>
            ×
          </button>
        </li>
      ))}
    </ol>
  );
}

/** The agents in the room, by their own colour — the same tint Clatch gives them. */
function Agents({ agents }: { agents: Agent[] }) {
  if (!agents.length) return null;
  return (
    <div className="agents">
      {agents.map((a) => (
        <Avatar key={a.id} agent={a} />
      ))}
    </div>
  );
}

function Avatar({ agent }: { agent: Agent }) {
  const src = useAsset(agent.avatar);
  return (
    <span className="avatar" style={{ background: agentTint(agent.id) }} title={agent.name}>
      {src ? <img src={src} alt="" /> : agent.name.slice(0, 1).toUpperCase()}
    </span>
  );
}

/** The app's own mark, inline — the same pin as the icon, at panel size. */
function Logo() {
  return (
    <svg className="logo" viewBox="0 0 1024 1024" aria-hidden>
      <rect width="1024" height="1024" rx="230" fill="#0E9F70" />
      <path
        fill="#fff"
        fillRule="evenodd"
        d="M512 828C512 828 288 570 288 424a224 224 0 1 1 448 0C736 570 512 828 512 828ZM512 512a88 88 0 1 0 0-176a88 88 0 1 0 0 176Z"
      />
    </svg>
  );
}
