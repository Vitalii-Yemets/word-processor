#!/usr/bin/env bash
# Makes the installer: the setup program with the program and the command
# line carried after it, into ./dist beside them.
#
# Why a step of its own: the installer carries the program, so it can only
# be made once the program is built. It is made by the setup program built
# for the machine this runs on, since a Windows one does not run here.
#
# Run inside the container, after the release build: ./x.sh win and
# ./x.sh linux (and x.ps1's) run it.
#
#   bash tools/pack-installer.sh win|linux

set -euo pipefail

case "${1:-}" in
  win)
    built=/work/target/x86_64-pc-windows-gnu/release
    suffix=.exe
    packer=(cargo run -q --release -p wp-setup --)
    ;;
  linux)
    built=/work/target/release
    suffix=
    packer=("$built/word-processor-setup")
    ;;
  *)
    echo "usage: $0 win|linux" >&2
    exit 2
    ;;
esac

mkdir -p /work/dist
"${packer[@]}" --pack "$built/word-processor-setup$suffix" \
  "/work/dist/word-processor-setup$suffix" \
  "$built/word-processor$suffix" "$built/wp$suffix"
