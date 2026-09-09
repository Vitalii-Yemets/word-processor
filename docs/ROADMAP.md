# Roadmap

The goal is a word processor that opens and saves the formats Microsoft Word
uses and offers the same set of features, built from scratch in Rust with no
third-party code.

This is a large undertaking — the working core alone is on the order of hundreds
of thousands of lines. The work is therefore split so that **every stage
produces something that runs and can be verified on its own**, rather than a
long stretch of nothing followed by everything at once.

Each stage lists what it delivers and how it is proven correct. "Proven" always
means an automated test, and wherever an outside reference exists — a real Word
document, the system `gzip`, a font file — the test is run against that
reference rather than against our own output.

---

## Stage 0 — Build environment ✅ complete

A Docker image holding the Rust toolchain and the mingw-w64 linker; a Cargo
workspace; `x.ps1` / `x.sh` as the only entry points. Nothing is installed or
built on the developer's machine, and cross-compilation to a Windows `.exe`
works from the same container that builds for Linux.

---

## Stage 1 — Container and markup primitives ✅ complete

Everything needed to open a `.docx` as a structured document rather than a blob.

### 1.1 Compression ✅ complete — `crates/wp-deflate`

DEFLATE (RFC 1951) decoder and encoder, zlib wrapper (RFC 1950), CRC-32 and
Adler-32. The decoder tolerates hostile input: it never panics on corrupt data
and can cap its output to stop decompression bombs.

*Proven by:* streams produced by the system `gzip` at three compression levels
decode correctly (this is what covers dynamic Huffman codes, which Word emits and
our encoder does not); `gzip` accepts and correctly decodes streams we produce;
every single-bit corruption and every truncation of a valid stream is rejected
without a panic.

### 1.2 ZIP and OPC packaging ✅ complete

ZIP reader and writer, including Zip64 for large documents, and the Open
Packaging Conventions layer on top: content types, relationships, parts, and
part naming rules.

*Proven by:* archives written by the system `zip` at three compression levels
read correctly, including entry names it writes as unflagged UTF-8; archives we
write pass `unzip -t` and extract identically; and a package survives open-and-
save byte for byte, parts it does not model included.

*Also proven:* Microsoft Word opens a document written here without a repair
prompt and without compatibility mode, and reads exactly the same words from it.

*Not yet proven:* nothing has been tested against a document Word itself
produced. That needs a corpus of real files, which cannot live in the repository
— see `corpus/`.

### 1.3 XML ✅ complete

A pull parser and a writer with namespace support, entity handling, encoding
detection, and exact whitespace preservation (`xml:space`). Word is strict about
what it will reopen, so output has to be faithful, not merely well-formed.

*Proven by:* parse, write and parse again produces the same event stream for
documents covering namespaces, entities, CDATA, comments and every script tested;
entity definitions in a document type declaration are never expanded, which
closes both the billion-laughs expansion and external entity file disclosure;
every truncation and bit-flip of a valid document is refused without a panic.

### 1.4 Unicode character database — not started

Generated, committed tables: general category, script, bidirectional class,
line-break class, grapheme and word boundaries, case mappings, normalization
data. These are the foundation of Stage 4 and are needed this early because the
XML layer already depends on some of them.

*Proven by:* the conformance test files published with the Unicode standard.

---

## Stage 2 — Document model — in progress

**Done:** an element tree that keeps every element, attribute and comment it was
given, whether or not this program models it, and editing that works on that
tree. Replacing text works across run boundaries, which it must: Word splits a
paragraph between runs wherever formatting changes, so a word is often stored in
pieces. Paragraphs can be appended and their style and alignment changed. An
edit rewrites only the part it touched, and only the nodes inside it that
changed.

**Still to do:** the full WordprocessingML object model: body, paragraphs, runs and their
properties, sections, styles, numbering and lists, fonts, settings, themes,
headers and footers, footnotes and endnotes, comments, bookmarks, fields,
tables, drawings, content controls, and tracked revisions.

The defining requirement is **lossless round-tripping**. Anything not yet
modelled — an element, an attribute, a whole part — is retained exactly as it
came in and written back unchanged. Without this the editor would quietly damage
documents, which would make it unusable for real work no matter how good the
rest is.

*Proven by:* an edit to a document whose body also holds a content control,
unknown markup with a comment inside it, and a tracked deletion leaves all of
that byte for byte intact, and changes no other part of the package; an
unmodified document is written back from its original bytes and comes out
identical.

*Also proven:* Word opens a document written here in the current mode, reads the
same 176 words this program reads, and lays it out on the same number of pages.

*Not yet proven:* nothing has been tested against a document Word itself
produced.

---

## Stage 3 — Fonts — partly done

**Done:** TrueType and OpenType parsing in `wp-font` — the table directory,
metrics, the character mapping in the formats that occur, and glyph outlines
including composite glyphs. Font discovery and matching on both platforms, with
per-character fallback. Outlines are rasterized in `wp-raster`.

**Still to do:** everything below.


An OpenType and TrueType parser covering `cmap`, `glyf`/`loca`, CFF and CFF2,
`head`, `hhea`, `hmtx`, `maxp`, `name`, `OS/2`, `post`, `kern`, `GDEF`, `GSUB`,
`GPOS`, variable-font tables, and colour and bitmap glyph tables. Font
enumeration and matching on both platforms, with per-script fallback chains.

A glyph rasterizer: outline decoding, scanline anti-aliasing, subpixel
positioning, gamma-correct blending.

No font files are shipped. Typefaces are data with their own licences, and the
ones Word uses belong to Microsoft; the editor reads whatever is installed on
the system, which is what Word does too.

*Proven by:* glyph metrics and outlines compared against values read out of the
same font files by independent means; rendered glyphs compared against reference
images.

---

## Stage 4 — Text engine

This is where support for the world's writing systems actually lives.

- The Unicode bidirectional algorithm (UAX #9) for Arabic, Hebrew, and mixed text
- Script itemization and complex shaping: Arabic joining forms, Indic
  reordering, Thai and Lao clustering, Hangul composition, applied through the
  font's `GSUB` and `GPOS` tables
- Line breaking (UAX #14) including the CJK rules, and word segmentation (UAX #29)
- Hyphenation, with per-language pattern data
- Vertical writing modes and ruby annotations
- Normalization (UAX #15) and case mapping with language-specific tailoring

*Proven by:* the Unicode conformance suites, plus shaping output compared against
reference renderings per script.

---

## Stage 5 — Layout engine

The largest and hardest stage, and the one no specification describes.

- **Inline:** runs, tab stops, alignment, justification, character and paragraph
  spacing, kerning, superscript and subscript
- **Block:** indentation, spacing, borders, shading, keep-with-next, widow and
  orphan control, page and column breaks
- **Page:** sections, columns, margins, headers and footers, page numbering,
  mirrored margins
- **Tables:** Word's autofit and fixed-layout algorithms, merged cells, nested
  tables, rows split across pages
- **Floating objects:** anchoring and text wrapping — square, tight, through,
  top-and-bottom, behind and in front of text
- **Notes:** footnote and endnote placement and balancing across page breaks
- **Fields:** calculating `PAGE`, `NUMPAGES`, `TOC`, `REF`, `SEQ`, `DATE`,
  `STYLEREF` and the rest

ECMA-376 defines how these are *stored*, not how Word *lays them out*. The
algorithms have to be recovered by rendering documents and comparing against
Word's output, which is why this stage takes the longest.

*Proven by:* page-image comparison against Word's rendering of a growing corpus
of test documents, tracked as a fidelity score rather than a pass/fail.

---

## Stage 6 — Rendering — partly done

**Done:** an anti-aliased path rasterizer using signed-area accumulation, a
pixel canvas with alpha blending, and a PNG encoder. Enough to draw a page of
text and to compare rendered output against a reference image.

**Still to do:** everything below.


A 2D graphics engine written from scratch: path filling with nonzero and even-odd
rules, stroking and dashes, clipping, affine transforms, gradients, and alpha
compositing.

Image decoders for PNG, JPEG (baseline and progressive), BMP, GIF, TIFF, and the
WMF/EMF metafile formats Word documents still carry.

DrawingML: preset shape geometries, text boxes, effects, charts, SmartArt.

Output targets: the screen, the printer, PDF (with font subsetting), and image
export.

*Proven by:* rendered output compared against reference images; generated PDFs
validated and compared against Word's PDF export.

---

## Stage 7 — Platform shells — partly done

**Done:** a Windows shell in `wp-shell`, written against the Win32 ABI directly
with no binding crate: a window, a message loop, keyboard and wheel input, and
the finished image presented through GDI. It is the only crate in the project
allowed to use `unsafe`, and the surface is the declarations plus the few calls
that use them.

**Still to do:** everything below, and X11 or Wayland for Linux.


Thin layers over the operating system, written against the raw ABI.

- **Windows:** window and message loop, presentation, printing through the
  spooler, clipboard, drag and drop, IME, HiDPI, and UI Automation for
  accessibility
- **Linux:** X11 and Wayland, printing through CUPS, clipboard, IME, and AT-SPI

*Proven by:* automated interaction tests driving a real window.

---

## Stage 8 — Editing core — partly done

**Done:** a caret, placed by clicking and moved by the arrow keys, Home and End.
Typing inserts, Backspace and Delete remove, Enter splits a paragraph and
Backspace at its start joins it onto the last, and Ctrl+S saves. Every one of
those goes through the document model, so a file keeps everything this program
does not understand even after being typed into.

**Still to do:** everything below.


A piece-table text store, cursor and selection model, multi-level undo and redo,
IME composition, autocorrect and autoformat, and clipboard interchange in the
formats Word uses (`CF_HTML`, RTF, Unicode text, images).

*Proven by:* property-based tests — a random sequence of edits followed by full
undo must return the document to its exact original state.

---

## Stage 9 — User interface

A widget toolkit built on the rasterizer: ribbon, dialogs, menus, task panes,
rulers, scrollbars, status bar. Views for print layout, web layout, outline,
draft, and reading, with zoom, split windows, and a navigation pane.

Localization is part of the framework, not an afterthought: all interface text
comes from message catalogues, the layout mirrors for right-to-left languages,
dates and numbers follow the user's locale, and proofing settings are per
language — the same breadth of language support Word offers.

*Proven by:* interface screenshots rendered in several languages, including a
right-to-left one, compared against references.

---

## Stage 10 — Word features

Styles and formatting panes; find and replace with wildcards; spelling and
grammar checking with an engine of our own reading open dictionary formats;
thesaurus; tracked changes; comments; document compare and merge; mail merge;
tables of contents and indexes; bookmarks, hyperlinks, cross-references and
captions; equations (OMML, with mathematical layout); charts; document
protection and encryption; building blocks and templates; macros.

---

## Stage 11 — Remaining formats

`.docm`, `.dotx`, `.dotm`; the legacy binary `.doc` ([MS-DOC] over [MS-CFB]);
RTF; ODT; HTML and MHT; plain text with encoding detection; PDF export and
import.

---

## Where the work stands

**It is an editor.** A document is unpacked, parsed, resolved against its styles,
laid out with fonts read from the machine, rasterized and shown on screen — and
then clicked into, selected in, typed in, undone and saved. Word opens the result
in the current mode and sees the styles a keypress created.

The caret and the selection belong to the document rather than to the window, so
undo restores where the caret was along with what the text said. Undo keeps whole
snapshots of the element tree rather than inverse operations: a snapshot cannot
be subtly wrong three steps later, and correctness is worth more here than the
memory. Steps merge while typing and break at a space, so undo takes back a word
rather than a keystroke; typing over a selection, or pasting several paragraphs,
is one step because it was one thing the person did. Cut, copy and paste go
through the system clipboard, which the shell reaches with the same
`extern "system"` declarations as the window.

Formatting is applied to a range by splitting the runs at each end of it, because
a run is the only place the format can record character formatting — so making
half a word bold means the word becomes two runs. A split copies the run and
keeps opposite halves, so both sides come out carrying everything the original
had, including what this program does not model. Turning a format off writes it
off rather than leaving it out: saying nothing about bold inside a heading would
inherit the heading's bold straight back. Runs are only split where the split is
needed, so formatting the same range twice, or typing letter by letter with bold
chosen, does not leave one run per keystroke.

With nothing selected, a formatting key applies to what is typed next rather than
to nothing at all — and is forgotten as soon as the caret moves somewhere else,
because by then it is about a place the user has left.

A tab is stored as `w:tab`, the element Word uses, and counts as one character in
the text the caret moves through — so the caret can stand either side of one,
Backspace deletes it whole, and a selection reaches across it. The layout engine
sends a tab to the next stop rather than advancing it by a fixed amount: a tab is
a distance to a place, not a distance to travel. Stops are counted from the left
edge of the text area, half an inch apart, which is where the marks on the ruler
are drawn.

The window has a toolbar and a ruler. Both are drawn onto the same canvas as the
document, by the same rasterizer and out of the same fonts; there is no widget
toolkit under them and no second way to put a pixel on screen. A toolbar button
carries its own effect rather than a label — the bold button is a letter B set in
bold — which needs no translation in a program meant to be used in every language
Word supports. Because the whole interface is drawn onto a canvas, it can be
drawn without a window at all: `--picture` writes it to a PNG, which is how it is
checked on a machine with no display.

What is missing from the interface is a ribbon with tabs, a style gallery and
dialogs. What is missing from the text is the shaping engine, which is what
Arabic and the Indic scripts need to have their letters joined rather than drawn
in isolation.

A `.docx` can be created, opened, read, edited and saved. Saving a document that
was not edited reproduces it byte for byte. Editing it rewrites only the part
that changed, and inside that part only the nodes that changed — a content
control, a chart or a colleague tracked change beside the edit comes through
untouched.

The `wp` command line tool exercises all of it, and the Windows executable is
cross-compiled from the same container that builds for Linux.

## Immediate next step

Stage 1.4 and Stage 4: the Unicode tables and the shaping engine, which Arabic,
Hebrew and the Indic scripts need to be drawn correctly rather than as isolated
letters. That is the largest remaining gap between what this draws and what Word
draws, and everything above it — the ribbon, the style gallery, tables with real
cells — is work that can be done on top of a text engine that is already right.
