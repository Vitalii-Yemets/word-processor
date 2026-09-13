#!/usr/bin/env bash
# Regenerates the Unicode tables the text engine searches.
#
# Why: the tables for bidirectionality, line breaking, segmentation and
# normalization used to be written by hand, and a hand-written table is a
# subset - it holds the characters somebody thought of and nothing else. These
# are generated from the character database instead, and the result is
# committed, so that the program still builds from its own source alone.
#
# Where the data comes from: Perl carries the whole character database, already
# parsed into files of ranges, and the build image carries Perl. Nothing is
# downloaded and nothing is installed.
#
# Run: docker compose run --rm dev bash tools/generate-unicode-tables.sh
#
# Afterwards: cargo fmt --all, because the generator writes plain Rust and does
# not try to guess how rustfmt would lay it out.

set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
UCD=${UCD:-}
if [ -z "$UCD" ]; then
    for candidate in /usr/share/perl/*/unicore; do
        if [ -f "$candidate/version" ]; then UCD=$candidate; break; fi
    done
fi

if [ ! -f "$UCD/version" ]; then
    echo "no character database at $UCD - is this running inside the build image?" >&2
    exit 1
fi

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

rustc --edition 2021 -O -o "$WORK/generate" "$ROOT/tools/unicode/generate.rs"
"$WORK/generate" "$UCD" "$ROOT"
