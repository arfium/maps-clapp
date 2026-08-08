# Contributing to whatsapp-clapp

This is a **clapp**: a Clatch app that puts your agents on your own WhatsApp. It is
Rust + Tauri v2 on the shared [`clappkit`](../../clappkit) crate, with a React/TypeScript
window and one bundled Node (Baileys) sidecar. The bar is: keep it minimal, keep it
correct, keep it honest about what has actually been proven. Small is a feature.

## Ground rules

- **KISS.** Plumbing that every clapp needs belongs in `clappkit`, not here. If you find
  yourself writing a data-dir resolver, an atomic save, a window verb or an IPC relay,
  stop — clappkit already has it, and a fifth copy is how the family drifted apart.
- **The contract is the Clatch spec.** The normative truth is
  [`docs/protocol.md`](../docs/protocol.md) (The Clapp Protocol), backed by the
  [Clatch repo](https://github.com/arfium/clatch)'s `reference/`. On conflict, the
  protocol wins.
- **The three must agree.** `clatch.json`, `src-tauri/src/main.rs`'s `APP_ID`, and the
  code must share the same app **id**, CLI **name**, and **signal** vocabulary — and
  `whatsapp --help` must document exactly the verbs `connector.commands` declares, since
  that list is the permission grain (`Bash(whatsapp <verb>:*)`). `clatch validate` checks
  the manifest against the files; nothing checks that the Rust matches it, so keep them
  in lockstep (see [`docs/TEMPLATE.md`](../docs/TEMPLATE.md)).
- **Build with `npm run build`, never a bare `cargo build --release`.** Tauri's
  `custom-protocol` feature is enabled by the Tauri CLI, not by cargo; a plain cargo
  release binary points the webview at the dev URL and opens a white window.
  `scripts/lib.sh` documents it and `scripts/package.sh` asserts against it.
- **It must build and validate.** `npm run verify` is the gate: build → package →
  `clatch validate pkg` → a real socket round-trip between the agent CLI and the running
  window, using the binary from the depot.
- **No silent failures.** Every dropped signal, denied command, dead sidecar or fallback
  is visible in an error, in the snapshot, or in `whatsapp status`. Fail-safe beats
  fail-open — and "Reaching WhatsApp…" forever with the reason discarded is the exact
  failure this repo has already shipped once.
- **Don't claim platforms you did not run.** The Windows arms here are reasoned against
  Clatch's own installer and transport, not executed. Say so.
- **Small, coherent PRs.** One concern per PR.

## Branches

`main` holds release code. Do daily work on short branches (`feat/…`, `fix/…`,
`chore/…`) and open a PR into `main`; CI (build + test + package on macOS, Windows and
Linux) is the gate. Releases are `v*` tags.

## Getting started

```sh
git clone https://github.com/arfium/whatsapp-clapp && cd whatsapp-clapp
npm install
npm run build                          # the shippable binary (frontend embedded)
npm run verify                         # the full gate, incl. a socket round-trip

# or drive it by hand, without a launcher (the dev hatch):
CLATCH_STANDALONE=1 bin/whatsapp app &
bin/whatsapp status                    # drive it like the agent would
npm run package                        # → pkg/, ready for `clatch install`
```

Prerequisites: a stable Rust toolchain, Node 22+, and the platform's Tauri v2 build
dependencies (macOS: Xcode Command Line Tools; Linux: `libwebkit2gtk-4.1-dev` and
friends). `npm run validate` / `npm run pack` need the `clatch` binary — put it on PATH
or set `CLATCH_BIN`. The sibling `clappkit` and `clatch` checkouts must sit beside this
repo, as `src-tauri/Cargo.toml` and `clappkit/Cargo.toml` declare by relative path.

`native/Sources/whatsapp/` is the original SwiftUI app, kept as the **behavioural spec**.
It is not built and not shipped; consult it when you need to know what the app used to
do, and change it in no other way.

Commit messages: imperative subject, body explains *why*.

## License

Apache-2.0. By contributing you agree your contribution is licensed under the same
terms (inbound = outbound). No CLA.
