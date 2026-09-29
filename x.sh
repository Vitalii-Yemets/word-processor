#!/usr/bin/env bash
# Same entry point for a Linux or macOS host.
set -euo pipefail
cd "$(dirname "$0")"

run() { docker compose run --rm dev "$@"; }

case "${1:-help}" in
  image)    docker compose build dev ;;
  build)    shift; run cargo build "$@" ;;
  test)     shift; run cargo test "$@" ;;
  check)    shift; run cargo clippy --all-targets -- -D warnings "$@" ;;
  fmt)      shift; run cargo fmt --all "$@" ;;
  fixtures) run bash tools/make-fixtures.sh ;;
  corpus)   shift; run cargo run -q --release -p wp-cli -- corpus "${1:-corpus}" ;;
  fidelity) shift; run cargo run -q --release -p wp-cli -- fidelity "${1:-corpus}" ;;
  conformance) shift; run cargo run -q --release -p wp-cli -- conformance "${1:-unicode}" ;;
  vba)      shift; run cargo run -q --release -p wp-cli -- vba "${1:-corpus}" ;;
  bench)    shift; run cargo run -q --release -p wp-cli -- bench "${1:-100}" ;;
  shell)    run bash ;;
  win)
    run cargo build --release --target x86_64-pc-windows-gnu
    run bash -lc 'mkdir -p /work/dist && cp -v /work/target/x86_64-pc-windows-gnu/release/*.exe /work/dist/ 2>/dev/null || echo "(no binaries yet)"'
    run bash tools/pack-installer.sh win
    ;;
  linux)
    run cargo build --release
    run bash -lc 'mkdir -p /work/dist && find /work/target/release -maxdepth 1 -type f -executable -exec cp -v {} /work/dist/ \; 2>/dev/null || true'
    run bash tools/pack-installer.sh linux
    ;;
  *)
    cat <<'USAGE'
Usage: ./x.sh <command>

  image      rebuild the docker image
  build      cargo build inside the container
  test       cargo test inside the container
  check      cargo clippy, warnings treated as errors
  fmt        cargo fmt
  bench      time what a person waits for, on a document of N pages
  fixtures   regenerate the gzip interop fixtures
  corpus     open, save and compare every real document in ./corpus
  fidelity   score the pages drawn for them against Word's own
  conformance  run the Unicode test suites in ./unicode against the engine
  vba        read every macro in ./corpus and write it back out
  win        release build of the Windows .exe and its installer -> ./dist
  linux      release build for Linux and its installer -> ./dist
  shell      interactive bash inside the container
USAGE
    ;;
esac
