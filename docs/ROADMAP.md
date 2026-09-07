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

## Stage 1 — Container and markup primitives

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

### 1.2 ZIP and OPC packaging

ZIP reader and writer, including Zip64 for large documents, and the Open
Packaging Conventions layer on top: content types, relationships, parts, and
part naming rules.

*Proven by:* a `.docx` produced by Word opens, and every part comes out
byte-identical to what the system `unzip` extracts.

### 1.3 XML

A pull parser and a writer with namespace support, entity handling, encoding
detection, and exact whitespace preservation (`xml:space`). Word is strict about
what it will reopen, so output has to be faithful, not merely well-formed.

*Proven by:* parsing every XML part of a corpus of real documents, re-serializing
it, and comparing the result to the input.

### 1.4 Unicode character database

Generated, committed tables: general category, script, bidirectional class,
line-break class, grapheme and word boundaries, case mappings, normalization
data. These are the foundation of Stage 4 and are needed this early because the
XML layer already depends on some of them.

*Proven by:* the conformance test files published with the Unicode standard.

---

## Stage 2 — Document model

The full WordprocessingML object model: body, paragraphs, runs and their
properties, sections, styles, numbering and lists, fonts, settings, themes,
headers and footers, footnotes and endnotes, comments, bookmarks, fields,
tables, drawings, content controls, and tracked revisions.

The defining requirement is **lossless round-tripping**. Anything not yet
modelled — an element, an attribute, a whole part — is retained exactly as it
came in and written back unchanged. Without this the editor would quietly damage
documents, which would make it unusable for real work no matter how good the
rest is.

*Proven by:* opening and saving a corpus of documents and comparing the result
semantically; then reopening the saved file in Word to confirm it reports no
problems.

---

## Stage 3 — Fonts

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

## Stage 6 — Rendering

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

## Stage 7 — Platform shells

Thin layers over the operating system, written against the raw ABI.

- **Windows:** window and message loop, presentation, printing through the
  spooler, clipboard, drag and drop, IME, HiDPI, and UI Automation for
  accessibility
- **Linux:** X11 and Wayland, printing through CUPS, clipboard, IME, and AT-SPI

*Proven by:* automated interaction tests driving a real window.

---

## Stage 8 — Editing core

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

## Immediate next step

Stage 1.2 — the ZIP and OPC layer, so a real `.docx` can be opened and its parts
listed. `wp-deflate` already provides everything it needs.
