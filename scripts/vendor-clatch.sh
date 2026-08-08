#!/usr/bin/env sh
# Refresh vendor/clatch from the clatch repository at a tag.
#
# WHY THIS EXISTS. clappkit depends on four crates from github.com/arfium/clatch, a PRIVATE
# repository, over ssh://. That is fine on a developer's machine and impossible everywhere
# else: a CI runner has no key, and a checkout five years from now may have no repository.
# So the four crates are copied into this repo at the tag clappkit pins, and
# src-tauri/Cargo.toml's [patch] points cargo at the copy. `cargo build --offline --locked`
# is the test that it worked.
#
#   scripts/vendor-clatch.sh v0.4.3            # from the sibling checkout
#   CLATCH_REPO=/path/to/clatch scripts/vendor-clatch.sh v0.4.4
#
# After running it: bump the tag in clappkit's Cargo.toml too, or the patch will be
# redirecting a version nobody asked for — and re-run `npm run verify`.
set -eu
ROOT="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
TAG="${1:-}"
CLATCH="${CLATCH_REPO:-$ROOT/../../clatch}"
CRATES="clatch-core clatch-ipc clatch-pipe clatch-registry"

[ -n "$TAG" ] || { echo "usage: scripts/vendor-clatch.sh <tag>   (e.g. v0.4.3)" >&2; exit 1; }
[ -d "$CLATCH/.git" ] || { echo "no clatch checkout at $CLATCH — set CLATCH_REPO" >&2; exit 1; }
git -C "$CLATCH" rev-parse -q --verify "refs/tags/$TAG" >/dev/null \
  || { echo "$CLATCH has no tag $TAG" >&2; exit 1; }

rm -rf "$ROOT/vendor/clatch"
mkdir -p "$ROOT/vendor/clatch"
for c in $CRATES; do
  git -C "$CLATCH" archive "$TAG" "crates/$c" | tar -x -C "$ROOT/vendor/clatch"
done

# The crates use workspace inheritance, so they need a workspace root: upstream's, with
# `members` trimmed to what was copied and agent-engine (not vendored) dropped.
git -C "$CLATCH" show "$TAG:Cargo.toml" | awk -v tag="$TAG" '
  /^members  =/ { print "members  = [\"crates/clatch-core\", \"crates/clatch-ipc\", \"crates/clatch-pipe\", \"crates/clatch-registry\"]"; next }
  /^agent-engine =/ { next }
  { print }
' > "$ROOT/vendor/clatch/Cargo.toml.body"
{
  printf '# VENDORED — do not edit by hand. Copied verbatim from github.com/arfium/clatch at\n'
  printf '# tag %s by scripts/vendor-clatch.sh, so that building this app needs no access to a\n' "$TAG"
  printf '# private repository: no submodule token, no SSH key on a CI runner, nothing to\n'
  printf '# configure before `cargo build`. src-tauri/Cargo.toml [patch] points cargo here.\n'
  cat "$ROOT/vendor/clatch/Cargo.toml.body"
} > "$ROOT/vendor/clatch/Cargo.toml"
rm -f "$ROOT/vendor/clatch/Cargo.toml.body"

printf '\nvendored %s:\n' "$TAG"
for c in $CRATES; do printf '  %-18s %s\n' "$c" "$(du -sh "$ROOT/vendor/clatch/crates/$c" | cut -f1)"; done
printf '\nnow: (cd src-tauri && cargo build --offline --locked) && npm run verify\n'
