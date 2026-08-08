#!/usr/bin/env sh
# `npm run build` — build the SHIPPABLE binary. The word "build" means the same thing
# in every clapp: run this and the release binary is current, with the frontend
# EMBEDDED (not fetched from a dev URL). It prints that binary's path.
#
# RE-ENTRANCY, and why this is a script rather than one line of package.json:
# src-tauri/tauri.conf.json calls a `beforeBuildCommand` to produce the frontend. When
# that command is `npm run build:web` there is nothing to guard; when it is
# `npm run build` — which is what it was, and still is in some of these repos — the
# Tauri CLI calls THIS SCRIPT back and an unguarded `npm run build` would recurse
# forever. Two independent markers say "you are that inner call": TAURI_ENV_PLATFORM,
# which the Tauri v2 CLI exports into before*Command, and CLAPP_FRONTEND_ONLY, which
# lib.sh's tauri_build() sets. Either one means: build the frontend and stop.
#
# Once every tauri.conf.json says `npm run build:web`, the guard is dead code and can
# go — but it costs two lines and it makes `npm run build` correct under both.
set -eu
. "$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)/lib.sh"

if [ -n "${TAURI_ENV_PLATFORM:-}" ] || [ -n "${CLAPP_FRONTEND_ONLY:-}" ]; then
  exec npm run --silent build:web
fi

tauri_build
printf '%s\n' "$ROOT/src-tauri/target/release/$(manifest connector.cli)$(exe_suffix)"
