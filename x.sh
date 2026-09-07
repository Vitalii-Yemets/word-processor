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
  shell)    run bash ;;
  win)
    run cargo build --release --target x86_64-pc-windows-gnu
    run bash -lc 'mkdir -p /work/dist && cp -v /work/target/x86_64-pc-windows-gnu/release/*.exe /work/dist/ 2>/dev/null || echo "(no binaries yet)"'
    ;;
  linux)
    run cargo build --release
    run bash -lc 'mkdir -p /work/dist && find /work/target/release -maxdepth 1 -type f -executable -exec cp -v {} /work/dist/ \; 2>/dev/null || true'
    ;;
  *)
    cat <<'USAGE'
Usage: ./x.sh <command>

  image      rebuild the docker image
  build      cargo build inside the container
  test       cargo test inside the container
  check      cargo clippy, warnings treated as errors
  fmt        cargo fmt
  fixtures   regenerate the gzip interop fixtures
  win        release build of the Windows .exe -> ./dist
  linux      release build for Linux -> ./dist
  shell      interactive bash inside the container
USAGE
    ;;
esac
