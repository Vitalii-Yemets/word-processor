# Word Processor

An open-source word processor that reads and writes the same file formats as
Microsoft Word and aims for the same feature set.

**Status: it is a word processor.** A `.docx` opens in a window with a ribbon,
and Microsoft Word opens back everything this program writes. The archive is
unpacked, the XML parsed, the styles resolved, the fonts read from the machine
and shaped, the glyph outlines rasterized, the pages laid out and every pixel of
the window drawn by code in this repository.

What the document model holds is most of what the format can carry: sections
with their own paper, margins, columns and headers; styles, lists, tables and
tab stops; pictures, shapes, charts, equations and diagrams; footnotes,
captions, cross-references, a table of contents, citations and an index;
comments, tracked changes, a document comparison and a mail merge.

What the window does is what Word's window does. The ribbon and its tabs, the
rulers, the navigation pane, the status strip, find and replace, the mini
toolbar over a selection, the menu the right button opens, the tooltips, and the
letters Alt puts over the ribbon. The mouse behaves the same way too: a double
click takes a word and a third takes the paragraph, the margin selects lines,
text can be carried somewhere else, Ctrl and the wheel zooms, and the middle
button starts the scroll that follows the pointer.

Printing goes to a real printer through the same layout that draws the screen.

Deliberate gaps, named rather than hidden: there is no grammar checker, the
Indic scripts are drawn without the reordering they need, and several features
are modelled to the depth a document needs rather than the depth Word's dialogs
offer. Each such limit is stated in the module that owns it. See
[docs/ROADMAP.md](docs/ROADMAP.md) for the plan and the current position.

`word-processor --picture <document.docx|-> <image.png> [width height]` draws the
whole window into a PNG instead of onto a screen, which is how the interface is
checked on a machine with no display.

## Principles

These constraints are deliberate and shape every decision in the codebase.

**Written from scratch, in Rust, with zero third-party crates.** Nothing but the
Rust standard library. Compression, ZIP, XML, the package layer and the document
model are all implemented here, and so are the fonts, text shaping, layout,
rasterization and the interface. The only external code the binary touches is
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

**Multilingual from the ground up.** Not a translation added at the end. The text
engine lays out bidirectional scripts and shapes the ones that need shaping:
Arabic and Syriac join, and ligatures are taken wherever a font offers them. The
Indic scripts need reordering within a syllable as well and are not shaped yet.
This has to be designed in from the start; it cannot be retrofitted.

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

Click in the text to put the caret there, then type. The ribbon along the top is
where the commands are; Alt puts a letter over each of its tabs and lets it be
worked from the keyboard alone. Enter splits a paragraph, Backspace joins one
onto the last, Ctrl+S saves. The wheel scrolls, Ctrl and the wheel zooms, and
right-clicking opens a menu about whatever is under the pointer.

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
| `wp-shape` | Turning characters into the glyphs that draw them: joining, ligatures |
| `wp-bidi` | The Unicode bidirectional algorithm, for mixed-direction text |
| `wp-break` | Where a line of text may be broken |
| `wp-segment` | Where one character ends and the next begins, and one word and the next |
| `wp-image` | Decoding the image formats a document can carry |
| `wp-svg` | SVG path data, turned into outlines the rasterizer can fill |
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
  compatibility mode, reads exactly the same words, and agrees on the page count.
  It also opens a document this editor was typed into, and sees the paragraph a
  keypress created carrying the style it should

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
