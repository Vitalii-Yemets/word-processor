# Word Processor

An open-source word processor that reads and writes the same file formats as
Microsoft Word and aims for the same feature set.

**Status: early development.** Stage 0 (build environment) and the first part of
Stage 1 (the DEFLATE layer) are complete. See [docs/ROADMAP.md](docs/ROADMAP.md)
for the full plan and current position.

## Principles

These constraints are deliberate and shape every decision in the codebase.

**Written from scratch, in Rust, with zero third-party crates.** Nothing but the
Rust standard library. Compression, XML, fonts, text shaping, layout,
rasterization and the GUI are all implemented here. The only external code the
binary touches is the operating system's own ABI (Win32 on Windows, X11/Wayland
on Linux), declared directly with `extern "system"` rather than through a
binding crate.

**Windows first, Linux supported.** The core carries no operating-system
dependency at all — it turns a document into a pixel buffer with nothing but
`std`. Platform shells are thin, so a third platform is a matter of writing one
more shell.

**Everything builds in Docker.** Nothing is installed on the developer's machine.
A single container holds the Rust toolchain and the mingw-w64 linker used to
cross-compile the Windows executable.

**Files survive a round trip.** Word documents contain more than any single
program models. Parts and elements this editor does not yet understand are
preserved verbatim on save, so opening a document here and saving it never
destroys work done elsewhere.

**Multilingual from the ground up.** Not a translation added at the end: the text
engine handles bidirectional scripts, complex shaping, and script-specific line
breaking, and the interface is localizable and mirrors for right-to-left
languages. This has to be designed in from the start; it cannot be retrofitted.

## Building

Requires only Docker.

```powershell
.\x.ps1 image      # build the container image (once)
.\x.ps1 test       # run the test suite
.\x.ps1 check      # clippy, warnings treated as errors
.\x.ps1 win        # release build of the Windows .exe -> ./dist
.\x.ps1 linux      # release build for Linux -> ./dist
```

On a Linux or macOS host use `./x.sh` with the same commands.

## Layout

| Path | Contents |
| --- | --- |
| `crates/wp-deflate` | DEFLATE, zlib, CRC-32, Adler-32 |
| `tools/` | Development scripts (test fixture generation) |
| `docs/` | Roadmap and design notes |
| `dist/` | Build output, copied out of the container |

## Specifications

The formats are public and the implementation follows them directly:

- **ECMA-376** — Office Open XML, the `.docx` format
- **[MS-OI29500]** — Microsoft's documented deviations from ECMA-376
- **[MS-DOC]**, **[MS-CFB]** — the legacy binary `.doc` format
- **RFC 1950 / 1951 / 1952** — zlib, DEFLATE, gzip
- **ISO/IEC 14496-22**, OpenType — font files
- **UAX #9, #14, #15, #29** — bidirectional text, line breaking, normalization,
  text segmentation

One thing no specification covers: Word's own layout algorithms. Justification,
table autofit, footnote balancing and widow/orphan handling are behaviour, not
format, and are matched by comparing rendered output against Word.

## License

MIT. See [LICENSE](LICENSE).
