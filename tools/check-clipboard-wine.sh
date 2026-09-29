#!/usr/bin/env bash
# Runs the check of Word's own clipboard format under Wine: see
# crates/wp-shell/examples/ole-clipboard.rs.
#
# Why: Word's own format is an OLE object — a compound file offered as
# "Embed Source" — and what reads it on Windows is OLE's clipboard and OLE's
# storage, not this program. Nothing in the build image is Windows; Wine is
# an implementation of the same documented interfaces by somebody else, so
# the object this program puts on the clipboard is opened here by Wine's
# storage, and a storage Wine's makes is read here by this program. That is
# not Word, and a check that passes here has not been run on Windows; but
# it has been run.
#
# Wine is large and nothing else needs it, so it is not in the build image;
# see tools/check-installer-wine.sh for why it is Debian 13's. Run on the
# host:
#
#   docker compose run --rm dev bash -c 'cargo build --release \
#     --target x86_64-pc-windows-gnu -p wp-shell --example ole-clipboard \
#     && mkdir -p dist/checks \
#     && cp target/x86_64-pc-windows-gnu/release/examples/ole-clipboard.exe dist/checks/'
#   docker run --rm -v "$PWD:/work" -w /work debian:trixie \
#     bash tools/check-clipboard-wine.sh

set -euo pipefail

built=/work/dist/checks/ole-clipboard.exe
[ -f "$built" ] || { echo "no $built: build the example first" >&2; exit 2; }
# From the container's own disk: Wine maps a program into memory, and a
# folder shared from a Windows host does not map.
program=$(mktemp -d)/ole-clipboard.exe
cp "$built" "$program"

if ! command -v wine >/dev/null || ! command -v Xvfb >/dev/null; then
  apt-get update -qq
  DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends \
    wine64 wine xvfb procps >/dev/null
fi
export WINEDEBUG=-all WINEPREFIX=/tmp/wine-clipboard
rm -rf "$WINEPREFIX"
wineboot -i >/dev/null 2>&1
# The clipboard is the display's: a screen for it to be on.
Xvfb :58 -screen 0 800x600x24 >/dev/null 2>&1 &
screen=$!
export DISPLAY=:58
sleep 1

said=$(timeout 120 wine "$program" 2>/dev/null | tr -d '\r' || true)
echo "$said"
kill "$screen" 2>/dev/null || true

failures=0
expect() { # expect <what> <line>
  if grep -qxF "$2" <<<"$said"; then echo "ok: $1"; else echo "FAIL: $1"; failures=$((failures + 1)); fi
}
expect "the copy was put on the clipboard" "put: true"
expect "OLE's clipboard hands it over" "ole clipboard: true"
if grep -qxF "embed source: a storage from OLE" <<<"$said"; then
  echo "ok: OLE offers the object as a storage"
else
  expect "OLE's storage opens the object's bytes" "embed source: opened by OLE from its bytes: true"
fi
expect "the object says it is a Word document" "class: word document: true"
expect "the package in it is the one copied" "package: the same: true"
expect "its names say Word" "comp obj: names word: true"
expect "it is embedded, not linked" "ole stream: true"
expect "the descriptor says whose it is and what to call it" \
  "descriptor: class word: true, called: Microsoft Word Document"
expect "OLE made a compound file of its own" "made by OLE: a compound file: true"
expect "and the document in it is read back" "read back: the same: true"
[ "$failures" -eq 0 ] && echo "all passed" || { echo "$failures failed"; exit 1; }
