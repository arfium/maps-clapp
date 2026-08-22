#!/usr/bin/env node
// The check nothing else performs: does clatch.json still describe THIS code?
//
// `clatch validate` reads the manifest, the compiler reads the code, and neither can
// tell you they disagree — which is how a manifest ends up declaring a verb the CLI
// dropped, or the CLI answering one the manifest never declared (and which therefore can
// never be granted: `connector.commands` is the permission grain, `Bash(whatsapp <name>:*)`).
// The same goes for signals: an id the app emits but does not declare is REFUSED at the
// control pipe, silently, at runtime.
//
// Pure text + JSON, no toolchain: it runs on a bare runner where `cargo build` cannot
// (clappkit is a path dependency that CI has no copy of), and it is the whole of this
// repo's CI gate until clappkit is published.
//
// usage: node scripts/check-manifest.mjs

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const read = (rel) => fs.readFileSync(path.join(ROOT, rel), "utf8");
const json = (rel) => JSON.parse(read(rel));

const problems = [];
const check = (cond, msg) => { if (!cond) problems.push(msg); };
const same = (what, ...pairs) => {
  const [, first] = pairs[0];
  for (const [where, v] of pairs) check(v === first, `${what}: ${where} is ${v}, expected ${first}`);
};

const manifest = json("clatch.json");
const pkg = json("package.json");
const conf = json("src-tauri/tauri.conf.json");
const cargo = read("src-tauri/Cargo.toml");
const mainRs = read("src-tauri/src/main.rs");
const cliRs = read("src-tauri/src/cli.rs");
// Every Rust module, concatenated: which file builds a signal is an app's own business
// (chess emits from game.rs, clock from store.rs, telegram/whatsapp from state.rs), and a
// gate that named one file would pass by simply not looking at the others.
const coreRs = fs
  .readdirSync(path.join(ROOT, "src-tauri/src"))
  .filter((f) => f.endsWith(".rs"))
  .map((f) => read(path.join("src-tauri/src", f)))
  .join("\n");

/** One `key = "value"` out of a Cargo.toml section, e.g. cargoField("[[bin]]", "name"). */
const cargoField = (header, key) => {
  const lines = cargo.split("\n");
  const start = lines.findIndex((l) => l.trim() === header);
  if (start < 0) return undefined;
  for (const line of lines.slice(start + 1)) {
    if (line.trimStart().startsWith("[")) break;
    const hit = line.match(new RegExp(`^\\s*${key}\\s*=\\s*"([^"]+)"`));
    if (hit) return hit[1];
  }
  return undefined;
};

// ── 1. the manifest's own required shape (protocol.md §1) ────────────────────────────
check(manifest.manifestVersion === 1, "manifestVersion must be 1");
check(manifest.protocol === 2, "protocol must be 2 (the control-pipe major this app targets)");
for (const f of ["id", "name", "description", "version"]) {
  check(typeof manifest[f] === "string" && manifest[f].length > 0, `clatch.json: ${f} is required`);
}
const launchKeys = Object.keys(manifest.launch ?? {}).filter((k) => k !== "args");
check(launchKeys.length > 0, "clatch.json: launch needs at least one per-OS command");
// The advertised platforms ARE the launch keys, and Windows will not run an
// extensionless image — a `windows` launch without `.exe` is a claim that cannot hold.
if (manifest.launch?.windows) {
  check(manifest.launch.windows.endsWith(".exe"), "launch.windows must name a .exe");
}

// ── 1b. every advertised platform is a platform the release actually ships ───────────
// The OS keys in `launch` ARE the advertised platforms (clappkit/docs/format.md
// § Distribution), and the launcher only finds that out at install, in front of a user:
// `no .clapp for linux-x64 in this release`. The release workflow's matrix is the list of
// depots that will exist, so the two are checkable against each other right here.
//
// Architecture is NOT in the manifest — the asset name is the only place it is decided —
// so this compares operating systems and nothing else.
const releaseYml = "\.github/workflows/release.yml";
if (fs.existsSync(path.join(ROOT, releaseYml))) {
  const shipped = new Set(
    [...read(releaseYml).matchAll(/^\s*target:\s*(macos|windows|linux)-\w+/gm)].map((m) => m[1]),
  );
  for (const os of launchKeys) {
    check(shipped.has(os), `clatch.json advertises \`launch.${os}\`, which ${releaseYml} never builds ` +
      "— drop the key or ship the depot");
  }
}

// ── 2. the four places the version and the identity are written ──────────────────────
same("version",
  ["clatch.json", manifest.version],
  ["package.json", pkg.version],
  ["src-tauri/Cargo.toml", cargoField("[package]", "version")],
  ["tauri.conf.json", conf.version]);
same("app id",
  ["clatch.json", manifest.id],
  ["tauri.conf.json identifier", conf.identifier],
  ["main.rs APP_ID", mainRs.match(/APP_ID:\s*&str\s*=\s*"([^"]+)"/)?.[1]]);

const cli = manifest.connector?.cli;
same("cli name",
  ["clatch.json connector.cli", cli],
  ["src-tauri/Cargo.toml [[bin]]", cargoField("[[bin]]", "name")],
  ["launch command", path.basename(launchKeys.map((k) => manifest.launch[k])[0], ".exe")]);
check(
  (manifest.connector?.cliBin ?? `bin/${cli}`) === `bin/${cli}`,
  "connector.cliBin must be the POSIX form bin/<cli> — the depot copy gains .exe on Windows " +
    "(scripts/lib.sh install_manifest); a repo manifest carrying .exe breaks macOS and Linux",
);

// ── 3. the declared commands vs the CLI that must answer them ───────────────────────
const declared = (manifest.connector?.commands ?? []).map((c) => c.name);

// Match arms, wherever the dispatch lives. Anchoring on one app's exact wrapper
// (`let req: Value = match verb {`) made this gate silently vacuous in any app that
// dispatches differently — and a vacuous check that still prints ✓ is worse than none.
// Only the block that dispatches the VERB — scanning the whole file also swept up the
// status printer's arms ("checkmate", "stalemate", …) and reported them as undeclared
// verbs. The binding differs per app (`let req: Value = match verb {`, `match verb {`),
// so anchor on `match verb {` alone.
const arms = new Set();
const dispatch = cliRs.match(/match verb \{([\s\S]*?)\n    \};?/);
check(dispatch !== null, "cli.rs: could not find the `match verb {` dispatch block");
for (const line of (dispatch?.[1] ?? "").split("\n")) {
  const head = line.match(/^\s*((?:"[^"]+"\s*\|\s*)*"[^"]+")\s*=>/);
  if (head) for (const v of head[1].match(/"([^"]+)"/g)) arms.add(v.slice(1, -1));
}

// The help text: any `const HELP…: &str = "…";` block (one app may split it in two), with
// or without the `\` line-continuation. Failing to FIND the help is itself a problem —
// otherwise every command below "is not documented" and the real fault goes unnamed.
// Both spellings a clapp uses: a plain escaped string (chess) and a raw string
// `r#"…"#` (clock, telegram, whatsapp). Missing the raw form made every declared verb
// look undocumented in three of the four apps.
const helpBlocks = [
  ...cliRs.matchAll(/const HELP[A-Z_]*: &str = r#"([\s\S]*?)"#;/g),
  ...cliRs.matchAll(/const HELP[A-Z_]*: &str = "\\?\n?([\s\S]*?)";/g),
].map((m) => m[1]);
check(helpBlocks.length > 0, "cli.rs: could not find a `const HELP: &str = \"…\";` block to check --help against");
const help = helpBlocks.join("\n");

for (const name of declared) {
  check(arms.has(name), `clatch.json declares \`${cli} ${name}\`, which cli.rs does not implement`);
  check(
    new RegExp(`^\\s{2}${cli} ${name}\\b`, "m").test(help),
    `clatch.json declares \`${cli} ${name}\`, which --help does not document`,
  );
}
for (const verb of arms) {
  if (["help", "-h", "--help"].includes(verb)) continue; // the manual itself is never a grant
  check(declared.includes(verb), `cli.rs implements \`${cli} ${verb}\`, which clatch.json does not declare ` +
    "(an undeclared verb is ungrantable, and invisible to the agent)");
}

// ── 4. the declared signals vs what the code actually emits ─────────────────────────
const declaredSignals = (manifest.connector?.signals ?? []).map((s) => s.id);
for (const s of manifest.connector?.signals ?? []) {
  check(["run", "context", "buffered"].includes(s.type), `signal ${s.id}: type must be run|context|buffered`);
}
// Signal ids written as literals anywhere in the core. Deliberately looser than matching
// one `Emit { id: "…".into() }` shape: an app may pick the id in a branch
// (`let id = if a.wake { "alarm.fired" } else { "alarm.quiet" };`) and a shape-matching
// scan would call that "never emitted" — a false alarm that teaches people to ignore the
// gate. This direction only has to prove the id EXISTS in the code.
const literals = new Set([...coreRs.matchAll(/"([a-z][a-z0-9]*(?:\.[a-z0-9]+)*)"/g)].map((m) => m[1]));
for (const id of declaredSignals) {
  check(literals.has(id), `clatch.json declares signal \`${id}\`, which appears nowhere in src-tauri/src ` +
    "(a declared-but-never-emitted signal is a promise to the agent that nothing keeps)");
}
// The other direction stays strict: an id built literally into an Emit but NOT declared is
// refused at the control pipe, silently, at runtime — the failure worth catching here.
const emitted = new Set(
  [...coreRs.matchAll(/Emit\s*\{\s*id:\s*"([^"]+)"\.into\(\)/g)].map((m) => m[1]),
);
for (const id of emitted) {
  check(declaredSignals.includes(id), `the code emits signal \`${id}\`, which clatch.json does not declare ` +
    "(clappkit refuses to send an undeclared signal, so it would silently never arrive)");
}

// ── verdict ─────────────────────────────────────────────────────────────────────────
if (problems.length) {
  console.error("✗ FAIL — the manifest and the code disagree:");
  for (const p of problems) console.error(`    · ${p}`);
  process.exit(1);
}
console.log(
  `✓ ${manifest.id} ${manifest.version} — ${declared.length} commands and ` +
    `${declaredSignals.length} signals, declared, documented and implemented`,
);
