# Word Processor

An open-source word processor that reads and writes the same file formats as
Microsoft Word and aims for the same feature set.

**Status: early development.** There is a window. A `.docx` can be created,
opened, read, edited, saved, and now drawn on screen — the archive unpacked, the
XML parsed, the styles resolved, the fonts read from the machine, the glyph
outlines rasterized and the window filled, all by code in this repository.

It is a viewer so far: editing works in the layers underneath, but there is no
caret on screen yet. See [docs/ROADMAP.md](docs/ROADMAP.md) for the plan and the
current position.

## Principles

These constraints are deliberate and shape every decision in the codebase.

**Written from scratch, in Rust, with zero third-party crates.** Nothing but the
Rust standard library. Compression, ZIP, XML, the package layer and the document
model are all implemented here, and so are the fonts, text shaping, layout,
rasterization and GUI still to come. The only external code the binary touches is
the operating system's own ABI (Win32 on Windows, X11/Wayland on Linux), declared
directly with `extern "system"` rather than through a binding crate.

**Windows first, Linux supported.** The core carries no operating-system
dependency at all — it turns a document into a pixel buffer with nothing but
`std`. Platform shells are thin, so a third platform is a matter of writing one
more shell.

**Everything builds in Docker.** Nothing is installed on the developer's machine.
One container holds the pinned Rust toolchain and the mingw-w64 linker used to
cross-compile the Windows executable.

**Files survive a round trip.** Word documents contain more than any single
program models — a macro project, an embedded font, a chart, a content control,
somebody else's tracked changes. An opened document is held as an element tree
that keeps all of it, and an edit rewrites only the nodes it must. A document
opened and saved untouched comes back byte for byte identical; one that is
edited differs only where it was edited. This is tested, not merely intended.

**Multilingual from the ground up.** Not a translation added at the end: the text
engine will handle bidirectional scripts, complex shaping, and script-specific
line breaking, and the interface is localizable and mirrors for right-to-left
languages. This has to be designed in from the start; it cannot be retrofitted.

## Trying it

Requires only Docker.

```powershell
.\x.ps1 image      # build the container image (once)
.\x.ps1 test       # run the test suite
.\x.ps1 check      # clippy, warnings treated as errors
.\x.ps1 win        # release build of the Windows .exe -> .\dist\wp.exe
.\x.ps1 linux      # release build for Linux -> ./dist
```

On a Linux or macOS host use `./x.sh` with the same commands.

The windowed application:

```powershell
.\dist\word-processor.exe                 # open with a sample document
.\dist\word-processor.exe mine.docx       # open a file
```

Mouse wheel or the arrow keys scroll, Page Up and Page Down move a screen at a
time, Escape closes.

`wp` is a command line front end for everything the window does not expose yet:

```powershell
.\dist\wp.exe new demo.docx          # write a document showing what the model covers
.\dist\wp.exe info demo.docx         # list the parts, content types and relationships
.\dist\wp.exe text demo.docx         # print the text
.\dist\wp.exe outline demo.docx      # print the structure with formatting
.\dist\wp.exe roundtrip a.docx b.docx        # open and save, checking nothing changed
.\dist\wp.exe replace a.docx b.docx old new  # replace text, across run boundaries
.\dist\wp.exe append a.docx b.docx "a line"  # add a paragraph at the end
.\dist\wp.exe render a.docx page 150         # draw the pages as PNG images
.\dist\wp.exe fonts                          # list the fonts on this machine
```

The editing commands report which parts of the package changed. Exactly one
should: everything else must come out as it went in.

## Layout

| Crate | Contents |
| --- | --- |
| `wp-deflate` | DEFLATE, zlib, CRC-32, Adler-32 |
| `wp-zip` | ZIP archives, including Zip64 |
| `wp-xml` | XML pull parser and writer, namespace-aware |
| `wp-opc` | Parts, content types, relationships |
| `wp-docx` | The WordprocessingML document model and styles |
| `wp-font` | TrueType and OpenType parsing: metrics, character mapping, outlines |
| `wp-raster` | Anti-aliased path filling, a pixel canvas, and PNG output |
| `wp-layout` | Finding fonts, breaking text into lines, drawing a page |
| `wp-shell` | The window and its event loop, written against the Win32 ABI |
| `wp-app` | The windowed application |
| `wp-cli` | The `wp` command line front end |

`tools/` holds development scripts, `docs/` the roadmap, `dist/` the build output
copied out of the container, and `corpus/` is an ignored directory for real Word
files to compare against.

## Testing

Every layer is tested against an implementation that had no part in writing it,
because "it reads what it writes" proves only internal consistency:

- DEFLATE streams from the system `gzip`, at three compression levels — this is
  what covers dynamic Huffman codes, which Word emits and our encoder does not
- Archives from `zip`, and archives of ours checked by `unzip -t`
- Every single-bit corruption and every truncation of a valid file, which must
  produce an error rather than a panic or a half-read document
- Microsoft Word itself, driven through its automation interface: it opens a
  document written here without a repair prompt, in the current mode rather than
  compatibility mode, reads exactly the same words, and agrees on the page count

## Specifications

The formats are public and the implementation follows them directly:

- **ECMA-376** — Office Open XML: Part 1 for WordprocessingML, Part 2 for packaging
- **[MS-OI29500]** — Microsoft's documented deviations from ECMA-376
- **[MS-DOC]**, **[MS-CFB]** — the legacy binary `.doc` format
- **RFC 1950 / 1951 / 1952** — zlib, DEFLATE, gzip
- **PKWARE APPNOTE** — the ZIP format, including Zip64
- **ISO/IEC 14496-22**, OpenType — font files
- **UAX #9, #14, #15, #29** — bidirectional text, line breaking, normalization,
  text segmentation

One thing no specification covers: Word's own layout algorithms. Justification,
table autofit, footnote balancing and widow/orphan handling are behaviour, not
format, and are matched by comparing rendered output against Word.

## License

MIT. See [LICENSE](LICENSE).
