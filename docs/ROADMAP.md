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

## How the work is tracked

The remaining work is numbered. A numbered item is finished when it runs, is
covered by tests, passes `./x.sh check` on both targets, and is committed; the
item is then ticked here in the same commit. Anything found on the way that is
not part of the item is written down as a new one rather than done quietly.

---

# Part one — what is built

The foundations. Each of these is in the repository, tested, and used by the
program as it runs today.

## Stage 0 — Build environment ✅

A Docker image holding the Rust toolchain and the mingw-w64 linker; a Cargo
workspace; `x.ps1` / `x.sh` as the only entry points. Nothing is installed or
built on the developer's machine, and cross-compilation to a Windows `.exe`
works from the same container that builds for Linux.

## Stage 1 — Container and markup ✅

### 1.1 Compression ✅ — `wp-deflate`

DEFLATE (RFC 1951) decoder and encoder, zlib wrapper (RFC 1950), CRC-32 and
Adler-32. The decoder tolerates hostile input: it never panics on corrupt data
and can cap its output to stop decompression bombs.

*Proven by:* streams produced by the system `gzip` at three compression levels
decode correctly (this is what covers dynamic Huffman codes, which Word emits and
our encoder does not); `gzip` accepts and correctly decodes streams we produce;
every single-bit corruption and every truncation of a valid stream is rejected
without a panic.

### 1.2 ZIP and OPC packaging ✅ — `wp-zip`, `wp-opc`

ZIP reader and writer, including Zip64 for large documents, and the Open
Packaging Conventions layer on top: content types, relationships, parts, and
part naming rules.

*Proven by:* archives written by the system `zip` at three compression levels
read correctly, including entry names it writes as unflagged UTF-8; archives we
write pass `unzip -t` and extract identically; and a package survives open-and-
save byte for byte, parts it does not model included.

*Also proven:* Microsoft Word opens a document written here without a repair
prompt and without compatibility mode, and reads exactly the same words from it.

### 1.3 XML ✅ — `wp-xml`

A pull parser and a writer with namespace support, entity handling, encoding
detection, and exact whitespace preservation (`xml:space`). Word is strict about
what it will reopen, so output has to be faithful, not merely well-formed.

*Proven by:* parse, write and parse again produces the same event stream for
documents covering namespaces, entities, CDATA, comments and every script tested;
entity definitions in a document type declaration are never expanded, which
closes both the billion-laughs expansion and external entity file disclosure;
every truncation and bit-flip of a valid document is refused without a panic.

## Stage 2 — Document model ✅ for what the editor does

An element tree that keeps every element, attribute and comment it was given,
whether or not this program models it. On top of it: paragraphs and runs and
their properties, styles with inheritance, numbering and lists, sections,
headers and footers, footnotes and endnotes, comments, bookmarks, hyperlinks,
fields, tables, drawings and pictures, shapes, charts, equations, content
controls, tracked revisions, themes, settings and document properties.

The defining requirement is **lossless round-tripping**. Anything not modelled —
an element, an attribute, a whole part — is retained exactly as it came in and
written back unchanged.

*Proven by:* an edit to a document whose body also holds a content control,
unknown markup with a comment inside it, and a tracked deletion leaves all of
that byte for byte intact, and changes no other part of the package; an
unmodified document is written back from its original bytes and comes out
identical.

*Not yet proven:* nothing has been tested against a document Word itself
produced. That needs a corpus of real files, which cannot live in the repository
— see `corpus/`, and item **K1** below.

## Stage 3 — Fonts ✅ for the tables that occur

TrueType and OpenType parsing in `wp-font`: the table directory, metrics, the
character mapping in the formats that occur, glyph outlines including composite
glyphs, kerning, and the `GSUB`/`GPOS` tables the shaper needs. Font discovery
and matching on both platforms, with per-character fallback. Outlines are
rasterized in `wp-raster` with scanline anti-aliasing.

No font files are shipped. Typefaces are data with their own licences, and the
ones Word uses belong to Microsoft; the editor reads whatever is installed on
the system, which is what Word does too.

Outlines are read from both tables a font may keep them in: `glyf` and `CFF`,
the quadratic kind and the PostScript one.

A variable font — one file that is a whole family — is read as the family it is:
the axes, the named instances, and the deltas that move the outlines and the
widths along them.

A colour font draws in colour: the layers of a `COLR` glyph in the palette its
font names, and the pictures of a `CBDT` one as pictures, on the screen and in
a PDF.

*Not done:* the rest of what a colour glyph can be — gradients, `sbix`, `SVG` —
which is item **E18**, and cutting a PostScript font down for a PDF, which is
**E17**.

## Stage 4 — Text engine — the part every document needs ✅

- **Bidirectional text** (UAX #9) — `wp-bidi`: the explicit embeddings and
  isolates, the weak and neutral types, paired brackets (N0), the implicit
  levels, the reordering of a line, and the characters drawn mirrored in it.
- **Shaping** — `wp-shape`: Arabic and Syriac joining forms and ligatures
  through the font's own `GSUB` table.
- **Line breaking** (UAX #14) — `wp-break`: where a line may be broken, the CJK
  rules included.
- **Segmentation** (UAX #29) — `wp-segment`: grapheme cluster and word
  boundaries, which is what the caret steps by and what a double click selects.
- **Normalization** (UAX #15) — `wp-normal`: the two ways of writing an accented
  letter, made one, for every character Unicode gives a canonical decomposition.

The tables all four of those search are generated from the character database by
`tools/generate-unicode-tables.sh` and committed, so they cover every character
and the program still builds from its own source alone.

The rest of the text engine is items **E1** to **E18**.

## Stage 5 — Layout and rendering — what the program draws today ✅

Line-by-line layout with fonts read from the machine: runs, tab stops with
leaders, alignment and justification, indentation, line and paragraph spacing,
borders and shading, keep-with-next and keep-together, widow and orphan control,
page and column breaks, sections with their own page setup, columns, headers and
footers with first-page and even-page variants, page numbering per section, line
numbering, footnotes and endnotes, tables with merged cells and rows that split
across pages, floating objects with square, tight, through, top-and-bottom and
behind-text wrapping, pictures, shapes, charts, equations, and the fields that
have to be worked out while laying out — `PAGE`, `NUMPAGES`, `TOC`, `REF`,
`SEQ`, `DATE`, `STYLEREF` and the rest.

Rendering: an anti-aliased path rasterizer, a pixel canvas with alpha blending,
image decoding, SVG-style path data for shape geometry, text effects, and a PNG
encoder. Everything on screen — the document and the interface alike — is drawn
by this and nothing else.

## Stage 6 — The window and the editor ✅ for one platform

A Windows shell written against the Win32 ABI directly, with no binding crate:
window, message loop, keyboard, mouse, wheel, timers, the caret blink, the
system clipboard, and the finished image presented through GDI. It is the only
crate allowed `unsafe`.

The editor on top of it: caret and selection that belong to the document,
clicking and dragging, double click by word, keyboard movement by character,
word, line, paragraph and page, typing, deleting, splitting and joining
paragraphs, multi-level undo and redo that merges by word, cut, copy and paste,
formatting by range, find and replace, zoom, split view, and the view modes.

The interface: a ribbon with contextual tabs, a style gallery, a navigation
pane, rulers with tab stops and indents, a status bar, a mini toolbar, key tips,
menus and popups, and the strips that stand in for dialogs. All of it drawn on
the same canvas as the document, so `--picture` can write the whole window to a
PNG on a machine with no display — which is how it is checked.

---

# Part two — the work that remains

Ordered by what a person using the program notices first. Every item says what
it is, what finishing it means, and how it is proven.

## A — Printing

A word processor that cannot print is not one. Nothing of this exists yet.

- [x] **A1. Laying a page out for a device rather than a screen.** The layout
  engine works in pixels at a screen resolution; a printer is 600 or 1200 dots
  per inch and has a hardware margin the paper cannot be drawn in. Lay out at a
  given resolution, and know the printable area.
  *Done when:* the same document laid out at 96 and at 600 dpi breaks its lines
  and its pages in exactly the same places.

- [x] **A2. The Windows spooler.** `OpenPrinter`, `StartDocPrinter`,
  `StartPagePrinter`, the device context, and the page image handed over — all
  declared with `extern "system"` like the rest of the shell. Printer
  enumeration, the default printer, paper sizes, orientation, duplex, copies,
  collation, and the printer's own margins.
  *Done:* the system print dialog chooses the printer and hands back its device
  context, which is what settles enumeration, the default, the paper, the
  orientation, the duplex and the copies — those belong to the driver and the
  driver is asked for them. The document is laid out for the printer's own
  resolution and sent a band of rows at a time, because a page of A4 at six
  hundred dots to the inch is a hundred and forty megabytes. The band the
  printer grips the sheet by is read from the driver and left out of the image,
  so the text lands where it was laid out rather than a quarter of an inch down
  and across.
  *Not yet proven:* nobody has printed a sheet with it. The container has no
  printer; the geometry is tested, the pressing of the button is not.
  *Left for A3:* printing a page range or a selection, and scaling a document
  to paper of a different size — both belong to the dialog, and offering them
  and then ignoring them would be worse than not offering them.

- [x] **A3. Print preview and the print dialog.** Word's is a whole view: page
  thumbnails, zoom, page ranges, what to print (document, markup, styles),
  pages per sheet, and the settings that belong to the printer rather than the
  document.
  *Done:* Ctrl+P turns the window into the Print page, as Word's does — the
  settings down the left and the document as it will come out beside them. The
  preview is the document laid out again rather than a photograph of the
  window, so what is shown is what will be printed. The printers are listed
  from the spooler and chosen by name; the copies, which pages (all, this one,
  or a typed list like `1-3, 8, 12-`), collated or not, how many pages to a
  sheet, and whether the tracked changes are printed all take effect. The
  paper, the orientation and the margins are the document's own page setup,
  changed from here as from the Layout tab, exactly as in Word. The warning
  that the margins fall where the printer cannot reach is shown when they do.
  *Not offered, rather than offered and ignored:* printing on both sides (see
  **A6**) and printing a selection (see **A7**).

- [x] **A6. Printing on both sides.** Telling a printer to turn the paper over
  means handing the driver a `DEVMODE` with its duplex field set, which means
  `DocumentPropertiesW` and a structure of a hundred and fifty-six bytes laid
  out exactly.
  *Done:* the driver is asked for its own settings, the duplex field and the
  bit that says it means something are written where the API documents them,
  and the settings go back with the request for a device context. Nothing is
  written past the size the driver said it filled in. The setting is offered
  only where the printer says it can turn the paper over — `DC_DUPLEX` — so it
  is never a choice that does nothing.
  *Not yet proven:* nobody has printed a sheet on both sides, for the same
  reason as **A2**: no printer in the container.

- [x] **A7. Printing a selection.** Word prints what is selected and nothing
  else, laid out on its own rather than as the pages it happens to fall on.
  *Done:* the selection is taken as blocks — the same ones a copy would put on
  the clipboard, formatting and all — and laid out as a document of its own on
  the document's own paper. The preview shows what will come out: one sheet
  with the selected paragraphs at the top of it. The setting is offered only
  when there is a selection, as Word offers it.

- [x] **A4. PDF export.** The PDF file format, the graphics operators, and font
  subsetting — embedding only the glyphs used, because a document may not carry
  a whole licensed typeface. Text has to stay text: selectable, searchable,
  with the right character codes.
  *Done:* `wp-pdf` writes the pages the layout engine produces — text, rules,
  pictures with their transparency, shapes and chart paths — with each font cut
  down to the glyphs the document actually draws and a table saying which
  character each glyph stands for. The Print page offers "Save as PDF" where
  Word offers "Microsoft Print to PDF", and honours the same settings; the
  command line has `wp pdf in.docx out.pdf`.
  *Proven by:* the file is read back by the tests themselves — the streams are
  decompressed, the instructions are read, and the glyph numbers are put
  through the file's own character table. What comes out is what was typed, in
  Latin, Cyrillic and Greek. The cut-down font is parsed again by the same font
  reader that reads the ones on the machine, and every letter the page draws
  still has its outline while the hundreds it does not draw have none.
  *Not yet proven:* no PDF reader has opened one. There is none in the
  container.

- [x] **A5. Printing on Linux.**
  **CUPS, spoken to in its own protocol.** A server with a socket, and
  everything a printer on Linux is asked goes to it as an HTTP request
  carrying an IPP message — RFC 8010 for the bytes, RFC 8011 for the
  operations, CUPS's own two for listing the printers and naming the
  default. No library is linked: the socket is opened, the bytes are
  written and read, as the X server is spoken to next door. The server is
  found where CUPS keeps it — its socket file, then its port — or where
  `CUPS_SERVER` says. Which printers there are, which is the default,
  whether one can turn the sheet over, and what paper one is set up for
  are four questions, each a message and a reply; a printer that cannot be
  asked is a printer that cannot be opened, and the program says so rather
  than guessing.
  **The same page images, through the same layer.** The Print page draws
  the pages a band at a time at the printer's own resolution and hands the
  bands to the shell, on Linux as on Windows. CUPS takes a document and
  not a device context, and the one document every CUPS takes is a PDF; so
  the bands are gathered into pages and the pages into a PDF of pictures,
  each page a picture at the printer's resolution laid on the paper where
  the printable part is, and that is the job. The paper is the printer's
  default medium, whose size is in its name — `iso_a4_210x297mm`,
  `na_letter_8.5x11in` — at its default resolution, less the smallest
  margins it says it keeps to; the sheet is turned over by the `sides` the
  job asks for. A job the queue would not take is said so on the status
  line rather than reported as printed, on both systems: `EndDoc`'s answer
  was thrown away before.
  *Proven by:* seven tests. A request laid out byte by byte as the RFC
  says; a reply read with every value of a set and a collection's members
  stepped over; a medium's size read from its name; the paper worked out
  from the medium, the resolution and the margins; a chunked body joined
  and a refusal refused; and, against a pretend CUPS on a port of the
  test's own — one that answers the four questions as a server with one
  printer would and keeps what it is sent — the whole way through: the
  printer listed, the default named, both sides offered, the printer
  opened on Letter at three hundred dots, two pages sent as bands, the job
  taken, and what arrived a PDF of two pages whose cross-reference table
  points at every object; and, with no server on the port, no printers,
  no default and nothing opened.
  *Not done, and named here:* nobody has printed a sheet with it — the
  container has no CUPS and no printer, which is the same sentence
  **A2** ends with. The pretend server is this program's, and holds the
  client to the RFC's bytes and to a document a reader can walk; a real
  queue is the proof this item asks for. There is no system dialog to
  choose a printer with, so choosing is the default printer, and a job goes
  with one copy and the sides asked for and nothing else: the paper, the
  tray and the quality are the printer's defaults. A server that wants a
  password is not answered.

## B — Speed on a document that is not a toy

The program lays out the whole document on every edit and remembers the whole
element tree on every undo step. On two pages nothing shows; on three hundred it
will crawl. This has to be fixed before the document gets bigger, not after.

- [x] **B1. A corpus and a measurement.** Documents of 10, 100 and 1000 pages,
  generated rather than committed, and a benchmark that says how long opening,
  typing, scrolling and saving take.
  *Done:* `./x.sh bench [pages]` builds a document of that many pages and times
  the four waits — opening the file, laying it out, typing one letter, and
  saving — and then says what the one that matters costs: a keystroke, which is
  typing plus laying the document out again.

  As it stands, on the machine this was written on:

  | Pages | A keystroke |
  | --- | --- |
  | 10 | 8.5 ms |
  | 100 | 115 ms |
  | 1000 | 5.96 s |

  Two things are wrong with that table. A hundred pages at a tenth of a second
  a letter is already too slow to type into. And the growth is worse than the
  document is long — ten times the pages costs fifty times the time — so
  something in the layout is quadratic and will be found in **B2**.

- [x] **B2. Incremental layout.** A keystroke relays the paragraph it changed
  and the pages after it only as far as the change reaches — typically one page.
  *Done in two halves.*

  **The quadratic terms.** Four places asked the document a question whose
  answer meant walking the whole document, and asked it once per paragraph or
  once per page: the text of a paragraph, the bookmarks, the page numbers, and
  the line numbering. Walking a thousand-page document a thousand times is what
  made ten times the pages cost fifty times the time.

  **What each paragraph measured to is kept.** Typing a letter changes one
  paragraph; everything the layout knows about the other eleven thousand is
  still true. Measuring them again — asking the font for every glyph, its width
  and its kerning, working out which way each piece reads — was most of what a
  keystroke cost. The answer is now kept with the paragraph it was worked out
  from and used again while that paragraph is unchanged. Anything whose answer
  can move on its own is measured afresh every time: a field says a different
  thing on a different page, a note carries a number worked out while laying
  out, a picture is a shared handle rather than something to copy.

  | Pages | Was | Now |
  | --- | --- | --- |
  | 10 | 8.5 ms | 2 ms |
  | 100 | 115 ms | 15 ms |
  | 1000 | 5.96 s | 250 ms |

  *Proven by:* `tests/incremental.rs` — whatever the engine has been through,
  the pages it gives are the pages a new engine gives from the same document:
  after an edit, after a paragraph is deleted and every later one shifts, after
  the resolution changes, after the paper colour changes, and for a document of
  fields whose answers move.

  *What is left,* and it is a smaller thing than it was: the placement pass
  still walks every page on every keystroke, which is the 250 ms on a thousand
  pages. Reusing the pages before the change and stopping once the pagination
  settles again is **B6**.

- [x] **B3. Undo without whole snapshots.** Snapshots are correct and cannot be
  subtly wrong, which is why they are there; they are also a copy of the
  document per keystroke. Keep them for structural edits and record text edits
  as what changed.
  *Done, and still snapshots.* Recording an inverse for every operation is
  where a single missed case corrupts a document three undos later; keeping a
  copy of what was there cannot be wrong. So what changed is the *size* of the
  copy: typing and deleting change one paragraph and nothing else, so those
  steps keep that paragraph and put it back where it sat. Anything that changes
  the shape of the document, and anything inside a gesture — where only the
  first change is recorded and the rest may be anywhere — still keeps the whole
  tree.

  A hundred letters typed into a thousand-page document: 250 MB and 215 ms
  before, nothing measurable and 23 ms now. The program used a gigabyte to hold
  a document somebody had been typing into for a minute.

  *Proven by:* the property the roadmap promised for Stage 8 — two hundred
  edits worked out from a fixed number, then undone one at a time, and the
  document comes back byte for byte what it was; and the same forwards, with
  redo.

- [x] **B4. Caching what is measured.** Shaping and measuring the same run over
  and over is most of the layout time. Cache per (face, size, text) and throw
  the cache away when a font changes.
  *Done as part of **B2**,* and per paragraph rather than per run — which is
  the same saving with one comparison instead of one per run, and no cache to
  grow without bound: there is one entry per paragraph of the document, and it
  is replaced when that paragraph changes.

- [x] **B5. Drawing only what changed.** A caret blink redraws the whole window
  today.
  *Done:* the pixels under the caret are kept when it is drawn, so a blink puts
  them back and draws the caret again — a few hundred bytes copied instead of
  every glyph on the page rasterized afresh. A blink went from eleven
  milliseconds to two hundred nanoseconds, which is fifty thousand times less
  work, twice a second, for as long as the window is open.

  While anything floats over the page — a list, the mini toolbar, a tip — a
  blink is an ordinary repaint, because those are drawn after the caret and
  putting back what was under it would put it back over them.

  *Proven by:* a blink off and a blink on leave the window byte for byte what a
  full repaint leaves, and a blink under an open list does repaint.

- [x] **B6. Reusing the pages that did not move.** The paragraphs are no longer
  measured again, but they are all still *placed* again: a keystroke walks every
  page of the document to work out where each line goes, which is the quarter of
  a second a thousand pages cost. Keep the pages before the change, lay out from
  the paragraph that changed, and stop as soon as the pagination lands where it
  landed before.
  *Done.* The editor gives the engine its pages back, and the engine keeps
  the ones that did not move. Every pass over the body leaves a checkpoint at
  the start of each block it could be taken up from — outside a run of
  paragraphs kept together, not one a run was moved back to — holding what
  the placement carries there: where the text has got to on which page and
  in which column, how much of the page is drawn, the list counters, the
  drawings floating so far, the outline depth. The next pass compares the
  new body with the old, block by block from both ends, widens the run that
  changed by one block each way for the paragraphs that ask about their
  neighbours' styles, takes up from the last checkpoint before it with the
  pages cut back to that moment, and places blocks until a checkpoint past
  the change looks exactly as it looked last time. Then it stops: the rest
  of that page and every page after are the old ones, moved over untouched,
  their paragraph numbers moved along when a paragraph was added or taken
  away in front of them. A footnote's room is the page's, so pages that move
  to a new number are kept only when the room there is the same; a section
  that begins on an even or an odd page is not kept past a change that
  moved the count by one. See `wp-layout/src/layout/again.rs`.

  What the placement depends on besides the body is compared whole before
  any of that — the resolution, the colours, what is shown, the footnote room,
  the pages the bookmarks fall on, the numbers the notes and the sequence
  fields carry, the sections and their paper — and any difference lays the
  body out from the start. A style edited, a picture replaced, a chart's
  figures changed all alter how a paragraph looks without altering the
  paragraph: every such change writes a part of the package, the package now
  counts its writes, and the count is one of the conditions. It also throws
  away what was measured, which nothing did before — a style edited under a
  kept engine drew the old style until the paragraph itself changed.

  The pages given back have to be this engine's last: every page carries a
  stamp saying which pass of which engine drew it, and pages with any other
  stamp — another window's, an earlier pass's, another engine's — are laid
  out afresh. One pass where there were up to four: the body is laid out with
  the footnote room and the bookmark pages the last layout ended with, which
  on a keystroke are still right, and only laid out again when they moved.
  Bookmarks and note marks find their pages through one index of the lines
  rather than a walk of every line per mark.

  | Pages | Was | Now | After a paragraph break |
  | --- | --- | --- | --- |
  | 10 | 2 ms | 0.7 ms | 1.0 ms |
  | 100 | 15 ms | 2.5 ms | 3.3 ms |
  | 300 | 65 ms | 11 ms | 13 ms |
  | 1000 | 250 ms | 42 ms | 46 ms |

  Twenty blocks placed of twelve thousand for a letter, thirty-four for a
  paragraph break, whatever the length. A table is fitted only when it is
  placed, so the cost **C29** named is now paid for the tables that moved.

  *Proven by:* `tests/again.rs` — twenty-seven documents put through an
  edit and handed back, each held to the pages a new engine gives: a word
  typed, a paragraph split, a paragraph deleted, an edit at the start, near
  the end, before a table, inside a table, before a heading kept with the
  next, in a list, one that adds a page, one that takes a page away, twelve
  edits in a row, a footnote before and after the change, a footnote pushed
  onto the next page, a `PAGEREF` whose bookmark moved, a `SEQ` field with a
  figure added before it, a section break inserted, a change before one, an
  odd-page section with a page added before it, a header on the kept pages,
  a style edited, the resolution changed, a stranger's pages and stale pages
  handed back.

  *Not the same on a thousand pages as on ten,* which is what this asked for,
  and here is the forty milliseconds: the model of the body is built from the
  tree on every layout (15 ms on a thousand pages), the bookmarks and the
  sections are found by walking the tree (4 ms each), the running heads are
  put on every page and the lines numbered (6 ms), and the letter itself is
  put into the tree by walking to its paragraph (9 ms, which is the
  document's and not the layout's). All of it grows with the document and
  none of it with what changed; all of it is under the hundred milliseconds
  a keystroke has, with the text placed in five. Also not done: a change the
  pagination never recovers from — a paragraph break in a document with no
  slack on any page after it — is placed to the end, as Word does, but Word
  does it in the background while the person keeps typing; here it is done
  before the window is drawn. Drawings floating beside the text are compared
  page by page and their pages moved along, but no test puts one through an
  edit.
- [ ] **B7. Searching in one pass.** `find_all` asks for each paragraph's text
  from the root of the tree, so a search over N paragraphs visits about N²/2
  of them: a document of ten thousand short paragraphs makes fifty million
  visits to learn that a word is not there, and the interface waits for it.
  Use the sequential walk `position::paragraphs` already has, and look at
  Find Next and Replace All for the same shape. Review R22.
- [ ] **B8. A large table that can be typed in.** With a table of three
  hundred rows every keystroke costs 0.4–0.7 s — inside the table and in the
  paragraphs before and after it, because the table is one block to
  **B6**'s checkpoints and is laid out twice, and `tablefit` walks every
  cell's text on every layout. A cache of row measurements keyed on the
  row's content, laying out only the row that changed, and checkpoints
  inside the table. Review T59; the cost of measuring was named in **C29**.

## C — The interface Word has

The ribbon is there and its arrangement matches Word's. What is missing is the
depth behind it: the dialogs, and the buttons that are drawn but do nothing.

- [x] **C1. Which buttons do nothing.** Walk every tab, every group, every
  button; list what is drawn, what it does, and what it should do.
  *Done:* [RIBBON.md](RIBBON.md) — two hundred and seven buttons, one row each.
  What it found is not what this item expected. **Nothing is decoration**: two
  buttons do nothing and say so, and every other one runs something real. The
  gap is depth. Word's small buttons are usually the top of a menu — Bullets
  drops a library of bullet shapes, Accept drops four ways of accepting — and
  here they are one action each: the common one, done straight away. Everything
  that falls short is now **C10** to **C17** below.

- [x] **C2. The Font dialog.** Every character format Word has, the two tabs,
  the preview, Set As Default.
  *Done:* it took three layers, because most of what the dialog sets had
  nowhere to go.

  **The file.** Eleven character properties the model did not hold: `w:dstrike`,
  `w:caps`, `w:smallCaps`, `w:vanish`, the underline's own colour, and the four
  measurements of the Advanced tab — `w:w`, `w:spacing`, `w:position`, `w:kern`
  — each stored in a different unit, plus the OpenType features, which are
  newer than the standard and live in Microsoft's `w14` namespace beside the
  text effects. `wp-docx/src/typography.rs` is the new module; the round-trip
  test in `wp-docx/tests/document.rs` names every one of them, so a property
  forgotten in the reader or the writer shows as a document that does not come
  back as it went in.

  **The drawing.** All of it is honoured rather than merely stored: capitals
  and small capitals are drawn without changing the text (and a letter whose
  capital is two letters — ß is SS — draws two glyphs that both point at the
  one character, so a click still lands where the text says); hidden text takes
  up no room and draws nothing, and comes back when the marks are shown; the
  scale stretches the outline rather than only the room after it, in the window
  and in a PDF alike (`Tz`, with the gaps divided out of it); the spacing is
  added per letter; the position lifts without shrinking; kerning is used at or
  above the size the document names. The OpenType features go through
  `wp_shape::shape_with`, which asks the font for exactly the tags requested and
  nothing else — a person who turned on tabular figures did not ask for
  ligatures as well. `wp-layout/tests/character.rs` holds every one of these to
  a difference that can only come from the property having been honoured.

  **The dialog.** Tabs and a preview were added to the machinery of **C9**: a
  `Field::Tab` marker rather than a list of lists, so a field keeps the same
  number whichever tab is showing, and `LayoutEngine::sample_line`, which draws
  the sample through the same style resolution and the same shaping as the
  document — a preview drawn a second way would drift from the first, and a
  preview that lies is worse than none. Ctrl+Tab walks the tabs, as it does in
  every dialog Word has. Reached by the launcher in the corner of the Font group
  (new: `Group::launcher`, which **C3** to **C6** will use) and by Ctrl+D.
  Set As Default writes into `w:docDefaults`, the bottom of the inheritance
  chain, so it reaches every paragraph that never said otherwise — this document
  only, because there is no template yet to write it into. That is **J6**.

  The arrangement is Word's too — Font, Font style and Size across the top,
  three colours under them, the effects in two columns inside a box, the
  preview in a box of its own. That took the row and group machinery of
  **C18**, which was written for this and is what **C3** onwards will use.
- [x] **C3. The Paragraph dialog.** Indents and spacing, line and page breaks,
  the preview, tab stops from inside it.
  *Done:* both tabs, in Word's arrangement, on the machinery of **C18**.

  **Indents and Spacing** — alignment and outline level under "General"; the two
  indents, Word's "Special" list (which is one property under two names: a
  first line pushed in is a positive first-line indent, one pulled out a
  negative) and mirror indents under "Indentation"; before, after, the six line
  spacings and "Don't add space between paragraphs of the same style" under
  "Spacing". **Line and Page Breaks** — widow control, keep with next, keep
  lines together and page break before under "Pagination"; suppress line
  numbers and don't hyphenate under "Formatting exceptions".

  **The model** gained the four Word sets here that it did not hold:
  `w:contextualSpacing`, `w:mirrorIndents`, `w:suppressLineNumbers` and
  `w:suppressAutoHyphens`. Contextual spacing is honoured in the layout as well
  as stored — the space is dropped where one such paragraph meets another of
  the same style, which is what makes a bulleted list read as a list rather
  than as a column of paragraphs with gaps between them.

  **The preview** is a shape rather than text, as Word's is: grey bars for the
  lines, at the indents, spacing and alignment being asked about, with the
  paragraphs either side drawn faintly — spacing is only visible against
  something and an indent only against a margin.

  **Tabs.** The Tabs button hands over to a Tabs dialog of Word's shape: the
  stops as a list, a position typed, an alignment and a leader chosen, and Set,
  Clear and Clear All, which change the list and leave the dialog standing. A
  double click on a stop in the ruler opens it too — which is what Word does,
  and which replaced the menu that had been standing in for it. That menu said
  so in its own comment.

  **Set As Default** writes into `w:docDefaults`, as the Font dialog's does.
- [x] **C4. The Styles pane and Manage Styles.** Applying, creating, modifying,
  the style inspector, what is in use, and the whole style chain shown.
  *Done:* a pane down the right-hand side, and three dialogs behind it.

  **The pane** (`chrome/stylespane.rs`) lists every paragraph style, each drawn
  in its own formatting — the same reason the gallery does it and the ribbon's
  bold button is a bold letter B. The one in force is marked with a bar down
  its left, the ones the document actually uses with a dot on the right. Under
  the list: Word's "Show Preview" tick box, an Options row that switches
  between all styles and the ones in use, and Word's three buttons — New,
  Inspect, Manage. Clicking a style applies it. Opened by the launcher in the
  corner of the Styles group and by Ctrl+Alt+Shift+S, and it takes its width
  out of the page rather than covering it.

  **New Style and Modify Style** are one dialog, as Word's two are: one begins
  from the formatting where the caret is and the other from what the style
  already says. It asks the name, what the style is based on and what follows
  it, and hands the rest to the **Font** and **Paragraph** dialogs through
  Word's Format menu — a second, smaller copy of those buttons would be a
  second place to be wrong. Answering one of them comes back here, carrying
  what it put on the paragraph into the style.

  A style is written into `styles.xml` by editing its element rather than
  replacing it, so everything this program does not model survives. And a style
  is read into the dialog from its own properties rather than the resolved
  ones: showing what it inherits would write all of that into the style and cut
  it off from what it is based on.

  **The Style Inspector** shows what the selection is formatted with, split the
  way Word splits it — what the paragraph style gives and what the text says on
  top of it — with the whole chain behind it named in order, "Normal ▸ Title".
  That chain is the answer to "why is this bold?", which is the only question
  the inspector exists for.

  Not done, and not part of this item: Word's Manage Styles dialog has four
  tabs of its own — Edit, Recommend, Restrict, Set Defaults. Recommend and
  Restrict are about which styles a person is allowed to use, which belongs
  with **F4** (protection); Set Defaults is what the two Set As Default buttons
  of **C2** and **C3** already do.
- [x] **C5. Insert Symbol and Special Characters.** The grid, the subsets, the
  recently used, the shortcut keys, AutoCorrect from inside it.
  *Done:* Word's dialog, both tabs, in place of the short popup list that had
  been standing in for it.

  **Symbols** — the font, the subset, a grid of sixteen across and eight down,
  the character code, and the row of recently used ones. The grid is new
  machinery in the dialogs (`Field::Grid`): the arrows walk it in two
  directions and it scrolls to keep the picked cell in sight, a click picks a
  cell, and Insert puts the character in and leaves the dialog standing — a
  person putting in three symbols should not open the dialog three times.

  **The subsets** are Unicode's own blocks by their own names, twenty-one of
  them, which is what Word divides its grid by. A block is a range of numbers
  rather than a list of characters, so the ones nothing on the machine can draw
  are left out: a grid of empty boxes is worse than a short grid.

  **Special Characters** is Word's list of the eighteen people ask for by name,
  each with the keys that put it in. Those keys work — Ctrl+Alt+C, Ctrl+Alt+T,
  Ctrl+Alt+. , the three dashes and Ctrl+Shift+Space — which is the difference
  between a dialog that documents this program and one that describes some
  other program. The space bar had to become a key the shell reports for the
  last of them; an ordinary space still arrives as typing.

  The **AutoCorrect** button waited on there being an AutoCorrect list to add
  to — a button that opened an empty dialog would have been worse than no
  button. **C19** made the list, and the button is there now: it opens the
  AutoCorrect dialog with the chosen character already in the "With" box.
- [x] **C6. Table properties.** Table, row, column, cell and alt text; borders
  and shading; autofit rules.
  *Done:* Word's dialog, in place of the flat list that had been standing in
  for it — a list whose own comment said Word had tabs and that a person would
  have to guess which. That was true, and the answer was not to invent a
  different dialog: somebody who knows Word knows the row settings are under
  Row.

  **Table** — preferred width as a percentage, indent from the left, alignment.
  **Row** — height, whether that height is a floor or a ceiling (Word offers
  both and they are not the same thing: text that does not fit an exact height
  is cut off), whether the row may break across a page, and whether it repeats
  as a header. **Cell** — preferred width and where the text sits up and down.
  **Alt Text** — the title and the description, which is the only tab whose
  absence is invisible to the person filling it in.

  Six properties the model did not hold: the table's own width and indent, a
  row's `w:cantSplit`, a cell's width, and the two halves of the alt text.

  **Borders and shading** is Word's button at the foot, and it hands over to
  the borders menu the ribbon already has — applying what the dialog said on
  the way, so a border lands on the table the dialog was describing.

  Two things are not here and are worth naming. Word has a fifth tab,
  **Column**, which sets the preferred width of a whole column: the file has no
  such property — a column's width is the widths of its cells — so it would
  mean walking every row, and it is not what the item asked for. And Word's
  **autofit rules** are under its Options button: fixed width, fit to contents,
  fit to window. Those want the table layout to measure its contents, which is
  **E3**.
- [x] **C7. Options.** The dialog behind File → Options, and the settings in it
  that this program actually honours.
  *Done:* three tabs — General, Display, Proofing — and every switch on them is
  one the program obeys.

  Word's Options has ten categories and several hundred settings, most about
  things this program does not have. A dialog offering them all would be a
  dialog where most of the switches do nothing, which is worse than a short
  one: a switch that does nothing is a lie told once per person who tries it.
  So what is missing is named here rather than drawn as a dead switch.

  **General** — a dark window, the zoom documents open at, and the unit
  measurements are shown in. **Display** — the formatting marks, the gridlines,
  the rulers, the navigation pane, and the white space between pages (Word's
  wording, and the opposite of what the editor keeps, which is whether the
  pages are joined). **Proofing** — whether spelling is marked as you type.

  All of them are written to the settings file and read back, so they follow
  the person from one document to the next. Five of the nine were not
  remembered before.

  **The unit is the one with teeth.** Word's "Show measurements in units of"
  has to reach every box in the program, and it could not while each dialog
  converted for itself — the same two functions were written three times, all
  of them assuming inches. They are now one module, `measure`, which knows the
  five units Word offers and is asked for the unit once. Page Setup, the
  Paragraph dialog, Table Properties and the Tabs dialog all go through it, and
  the marks beside the boxes follow. Points stay points where Word keeps them
  in points: the space above a paragraph, the size of type, the position of
  text off its line. Somebody who asked for centimetres did not ask for their
  type size in centimetres.

  Also: one icon that is not from the Fluent set, because the set has no gear
  and Word's Options needs one. Drawn the same way as the rest, as filled
  paths, and said so where it is declared.

  Not honoured and so not offered: **Save** (there is no autosave — **G2**),
  **Language** (**F6**), **Customize Ribbon** and **Quick Access Toolbar**
  (**C20** below), **Add-ins** and **Trust Center** (neither exists).
- [x] **C8. The File tab.** Word's backstage: Info, Recent, New from template,
  Open, Save As, Print, Share, Export, Close.
  *Done:* `chrome/backstage.rs` draws it and `editor/backstage.rs` fills it in.
  Pressing File no longer drops a page of buttons under the ribbon — the tab is
  a blue button, as Word's is, and it opens a window of its own over everything
  with the blue rail down the left and nine places on it: Info, New, Open, Save,
  Save As, Print, Export, Close, Options. Escape and the arrow at the top go
  back; the arrow keys walk the rail, over the places that have a page and past
  the ones that do not, so no arrow key can save or close a document.

  Four of them do their work and leave rather than drawing a page of their own:
  **Print** opens the page built in **A3**, **Options** the dialog built in
  **C7**, **Close** starts a new document after asking about unsaved changes,
  and **Save** saves — staying in the backstage when it could save silently and
  going back to the document when it had to ask where, which is what Word does.

  **Info** is what the document is: where it lives, how big the file is, its
  pages, words, characters and paragraphs, when it was made and last saved and
  by whom — and under that the seven properties Word's Info panel holds, each a
  line that opens the strip that takes one line of typing. The popup list that
  used to stand in for that panel is gone.
  **Open** is Browse and then the documents opened lately, most recent first;
  the list is kept in the settings file (`recent.0`, `recent.1`, …, up to
  Word's fifty), written whenever a document is opened or saved, and a document
  opened again moves up the list rather than appearing on it twice.
  **Save As** is Browse and then the folders those documents came from, each
  named once.
  **Export** writes the PDF — the same writer the Print page's "Save as PDF"
  printer uses, so there is one PDF and not two.
  **New** offers the blank document, which is the only thing there is to make.

  Not here, and named rather than drawn as a dead page: **Share** (there is
  nowhere to share to — no account, no service), **New** from a gallery of
  templates (**J6**, which is where a template first has to exist), and Info's
  **Protect Document**, **Inspect Document** and **Manage Document**, none of
  which this program can do yet. Word's **Account** and **Feedback** are about
  a subscription and a place to send it, and there is neither.
- [x] **C9. Real dialogs rather than strips.** The strips stand in for dialogs
  because there was no dialog machinery. Build the machinery — a window, a
  focus ring, tab order, default and cancel buttons, keyboard everything — and
  move them over.
  *Done:* `chrome/dialog.rs` is the machinery — a panel over a dimmed document
  with a caption, a measured label column, six kinds of field (a heading, a
  line it tells you, a text box, a number box with its unit, a tick box, a list
  that drops open), Tab and Shift+Tab round the fields and on to the buttons,
  Space to tick, the arrows inside a list, Enter for the button in bold, Escape
  to cancel. `editor/dialogs.rs` asks the questions. A dialog is modal the way
  Word's is: while one is up the document behind takes neither a key, a click
  nor the wheel.
  Three questions moved over: **Word Count** (Word's six counts and Word's tick
  box, which counts notes and text boxes as well and is remembered between
  openings), **Page Setup** (the margins typed rather than chosen from a list —
  reached by Layout ▸ Margins ▸ Custom Margins…, as in Word) and **Bookmark**
  (a name, and the names already in the document). The strips they used to be
  are gone.
  The rest of the strips move over as their dialogs are built: each is named in
  its own item (**C2** to **C6**), because moving a strip is the small half of
  building the dialog Word has.

- [x] **C10. The paste Word has.** Paste options — keep source formatting, merge
  formatting, keep text only, and a picture where the clipboard holds one — and
  the little button that offers them again after pasting.
  *Done:* all four, and the button. A paste is a guess about whether the words
  should arrive dressed as they were or dressed as their surroundings, and Word
  does not ask first — it pastes the likeliest way and leaves a small "(Ctrl)"
  button at the end of what it put down. Pressing it, or pressing Control on
  its own, opens the four; choosing one takes the paste back and puts it down
  the other way, which is why a paste is one thing to undo whichever way it
  went. The button goes at the next thing done — a key, a click elsewhere,
  Escape — because going on without it is an answer.

  **Keep Source Formatting** brings the runs and the shape of the paragraphs
  both; a paragraph the paste made carries the shape it was copied with, and
  the paragraph it lands in keeps its own unless there was nothing in it to
  disagree. **Merge Formatting** brings the emphasis — bold, italic, underline,
  the two strikethroughs, superscript and subscript — and drops the font, the
  size, the colour and the style, so the text takes its surroundings.
  **Picture** lays the copied paragraphs out against this document, draws them
  at twice the screen's resolution and puts the result in as a PNG: a
  photograph of the text, which cannot reflow. **Keep Text Only** is the words.
  The first two are `wp_docx::clipboard::Formatting`; the picture is
  `editor/paste.rs`, because only that layer can lay a document out and
  rasterize it.

  Reached from the button, from the right-click menu (which offers the four
  outright, as Word's does, whenever the clipboard holds formatting to decide
  about) and from Ctrl+Shift+V for the words alone. Control pressed and let go
  with nothing in between is a new event from the shell, `Event::ControlKey`,
  built the same way as the Alt that shows the key tips — the only way to tell
  that Control from the one in Ctrl+S is to watch what happens while it is
  held.

  Not here: the split button on the ribbon's Paste, whose lower half drops the
  same four. That is **C11**, which is about every split button at once. Paste
  Special — pasting as HTML, as RTF, as an embedded object — needs those
  formats on the clipboard first, which is **H2**.
- [x] **C11. The menus behind the buttons.** A dozen buttons do the common thing
  where Word drops a menu: bullets and numbering (a library of shapes and
  formats), multilevel lists, line spacing (with space before and after),
  Change Case (five choices, not a cycle), Page Number (top, bottom, margins,
  current position), Select, Next Footnote, Accept and Reject, Bring Forward and
  Send Backward, Track Changes, Show Markup.
  *Done:* eleven of them, in `editor/menus.rs`, on new ribbon machinery that
  knows the difference between Word's two kinds of button. A **split button**
  runs a command from its face and drops a list from its arrow — Bullets puts
  bullets on, the arrow beside it asks which bullet — and a **plain dropdown**
  has no command of its own, because there is no such thing as "the case".
  `ribbon::MENUS` is the table of which is which, `Ribbon::press_at` is the one
  place the line between the two halves is drawn, and a large button is divided
  across rather than down, as Word divides one.

  The three buttons that used to cycle no longer do: Change Case, Line Spacing
  and Multilevel List each ask once. A person who wants small capitals should
  be able to ask for them rather than press a button five times, and a recorded
  macro that said "Change Case" could not say which case it meant.

  **Bullets** and **Numbering** are real libraries: picking a mark writes a list
  definition into `word/numbering.xml` with that mark, and finds the one that is
  there already rather than writing a second — two identities for one list is
  two counters, and the second half of a numbered list would start again at one.
  That is `Document::list_shaped`, and it is what the **Multilevel** gallery
  uses too, with all three levels given at once. **Line Spacing** offers Word's
  six spacings and the room above and below a paragraph, which say Add or Remove
  depending on what is there. **Page Number** puts one at the top, at the foot
  or where the caret is (as a `PAGE` field), formats them or takes them away.
  **Select** has Select All and the Selection Pane. **Next Footnote** has all
  four ways to step through the notes. **Accept** and **Reject** have Word's
  four each, including "and Move to Next" — which needed
  `Document::paragraphs_with_revisions`, because a count says whether there are
  changes and not where the next one is. **Track Changes** has the switch and
  Word's Lock Tracking, which is the document's own restriction to tracked
  changes written down.

  The style gallery now gives up tiles before any group is given up altogether:
  it is the widest thing on the Home tab, and losing the whole Styles group on a
  narrow window would take the styles off the tab they are used from.

  **Bring Forward**, **Send Backward** and **Show Markup** were drawn here and
  did not do what their names said: the first two needed an order among drawings
  that the model did not carry, and the third needed comments and formatting
  revisions to be markable apart from insertions and deletions. **C21** did
  both. **C22** put **Select Text with Similar Formatting** on the Select menu
  with the selection it needed behind it; **Select Objects** turned out to need
  something else again — a drawing that can be selected — which is **C27**.
- [x] **C12. The boxes on the Layout tab.** The indent boxes are drawn and
  cannot be typed into — pressing them says to drag the ruler instead. Spacing
  before and after has no boxes at all. Both are measurements a person types.
  *Done:* four boxes, in Word's arrangement — the indents in one column and the
  room above and below beside them — and every one of them takes the keyboard.
  `editor/boxes.rs` is what happens in one: pressing it puts the caret in with
  the value ready to be replaced, typing replaces it, Backspace rubs out, Enter
  applies and lets go, Escape lets go without applying, Tab applies and moves
  to the next box, and a press anywhere else applies it — a number typed and
  then left is a number meant. Only what a measurement is made of gets in at
  all, so a stray letter cannot leave a box holding something unreadable.

  Each box has Word's two little arrows as well, which step an indent by a
  tenth of an inch and the room round a paragraph by six points, from whatever
  is in force rather than from nothing. The up and down keys do the same while
  the box has the keyboard.

  An indent is shown and read in whatever unit Options was set to; the room
  above and below is in points however that was set, which is Word's own
  division and the one **C18**'s `measure` module was written round. Before
  this the boxes said centimetres whatever the setting was.

  The ribbon learned two things for it: `Item::NewColumn`, so a group can start
  a second column outright rather than waiting to overflow into one (Word's
  Paragraph group is two columns of two, and left to wrap it came out three and
  one), and `Press::Type`/`Press::Step`, so a press can land in a box or on one
  of its arrows.
- [x] **C13. Design ▸ Paragraph Spacing does the wrong thing.** It cycles the
  line spacing of the document. Word's sets a named spacing set — Compact,
  Tight, Open, Relaxed, Double — on the style set, changing space before and
  after as well as the lines.
  *Done:* the button had been sharing `Command::LineSpacing` with the Home
  tab's, which is why it did the wrong thing — one command cannot be two jobs.
  It has its own now, and drops Word's six sets: No Paragraph Space, Compact,
  Tight, Open, Relaxed and Double, each shown with what it does, and Custom
  Paragraph Spacing at the foot.

  What each set writes is Word's own numbers, and they go into `w:docDefaults`
  rather than onto the paragraphs. That is the difference between the document's
  spacing and a change to every paragraph in it: a paragraph that was given its
  own spacing keeps it, and every paragraph that never said otherwise follows.
  Reading them back is what marks the set in force, through the new
  `Styles::document_paragraph_defaults`.

  Custom Paragraph Spacing opens the Paragraph dialog, which has Set As Default
  on it — the same job Word's ends in, through the dialog this program already
  has for it.
- [x] **C14. Page Borders.** Opens the same list of edges a paragraph border
  uses. Word opens Borders and Shading on its page tab: art borders, which pages
  they go on, and the distance from the edge.
  *Done:* the button had `Command::Borders` on it — the paragraph one — so it
  put a border round the paragraph the caret was in. It has a command of its
  own now and opens Word's Borders and Shading on its Page Border tab.

  A page border is written in a different place from a paragraph's and made of
  different decisions, which is why it is a module of its own:
  `wp_docx::pageborders` reads and writes `w:pgBorders` inside the section's
  properties — the four edges, which pages of the section carry one
  (`w:display`), whether the distance is measured from the paper or from the
  text (`w:offsetFrom`), and how far in it sits (`w:space`, in whole points, as
  far as thirty-one). The dialog holds all of that, with Word's Options folded
  into it rather than hidden behind a second dialog: there are two fields in it.
  Apply to offers the whole document or this section, and the whole document
  writes into every section rather than only the caret's.

  The layout draws it, which is the half that makes it real: it is measured
  from the sheet rather than from anything laid out, it is the same on every
  page the section asks for, and it is there on a page with no text at all.

  Drawing it turned up a gap worth closing at the same time: a border's style
  was written down faithfully and drawn as a plain line whatever it said, so a
  list offering five styles would have been four rows of lie. `draw_border_edge`
  draws a double as two lines with a gap, a dotted as a row of squares and a
  dashed as a row of longer ones, out of the plain rectangles a decoration is —
  and paragraph borders go through it too, so they gained their styles as well.

  **C23** finished the job: every one of Word's twenty-five line styles is drawn
  as itself, and the **Shadow** and **3-D** settings are on the dialog and
  drawn. Word's **Art** border gallery is a hundred and sixty pictures Word
  ships, and what of it can be drawn from shapes rather than copied is **C28**.
- [x] **C15. The Table Design tab.** Header Row and Banded Rows do nothing and
  say so — the only two buttons in the program that do. The tab is also missing
  the table styles gallery, shading, the border styles and the border painter,
  and the first-column and banded-column switches.
  *Done:* those two buttons could not have done anything on their own, and
  saying so was the honest half of the answer. Ticking Header Row does not shade
  the first row; it says the *style* may treat the first row specially, and a
  style that says nothing about first rows changes nothing — in Word too. What
  was missing was everything on the other side of that sentence, and it is here
  now:

  **The switches** are all six Word has, and they write `w:tblLook`
  (`model::TableLook`) — including the two the file writes upside down, as
  `w:noHBand` and `w:noVBand`. **The styles** carry conditional formatting:
  `w:tblStylePr` per part, read into `styles::Conditional` and `TablePart`, and
  resolved by `Styles::resolve_table_cell`, which applies the parts weakest
  first so a header row still looks like one where it crosses the first column.
  **The layout** works out which parts each cell is in — `parts_of`, which
  counts the bands from the first row that is not the header, as Word counts
  them — and draws the cell's colour behind it. Cells had no colour at all
  before this: `w:shd` on a `w:tcPr` was neither read nor drawn, and a banded
  table has nothing to be made of without it.

  **The gallery** offers five of Word's own styles by Word's identifiers and
  names, so a table given one here arrives in Word as the style it says it is.
  A style is written into `styles.xml` when it is first used, because a table
  can only name a definition that is there — and one the document already
  carries is left exactly as it is, since a document from Word brings Word's own
  and overwriting it would change how that document looks in the program it was
  made in.

  **Shading** colours the cell rather than the paragraph inside it when the
  caret is in a table, which is what Word's does and what a table style does.

  Not done: Word's **Border Styles** gallery and its **Border Painter**, the pen
  that paints a chosen line onto the edges it is dragged along. The painter is a
  mode rather than a command, like the format painter, and the gallery is the
  line styles of **C23** over again. Both are **C24**.
- [x] **C16. The rest of the Table Layout tab.** Select, View Gridlines, Draw
  Table and Eraser, AutoFit, the height and width boxes, Text Direction, Cell
  Margins, Sort, Repeat Header Rows, Convert to Text, and Formula. And nine
  alignments where there are three.
  *Done:* six of the twelve, and the nine alignments. The other six each need
  something the program does not have yet and are **C25** below, named there
  with what each of them wants.

  **Nine alignments** where there were three. A cell has two questions to
  answer — where the text sits across it and where it sits up and down it — and
  three buttons could only answer the first: a person looking for "align middle
  centre" found a tab that did not have it. Each of the nine answers both in one
  press, and in one gesture, so one undo takes the whole answer back. The
  pictures are drawn by hand, because the Fluent set has no drawing of text
  sitting in one of nine places in a cell.

  **Select** takes the cell, the column, the row or the whole table. What it
  selects is text — a table is paragraphs like everything else — so it needed
  `Document::cell_paragraphs` and `table_paragraphs`, which say which paragraphs
  a part of a table covers.

  **View Gridlines** draws the boundaries of a table that has no lines of its
  own. They are faint, they are on the screen only, and they are never printed —
  the printer and the PDF writer lay the document out with engines of their own
  and neither turns them on. That is what makes them gridlines rather than
  borders.

  **The height and width boxes** are the Cell Size group, on the machinery
  **C12** built: typed into, stepped by their arrows, and in the unit Options
  was set to.

  **Repeat Header Rows** writes `w:tblHeader`, which is what makes the first row
  come back at the top of every page the table runs onto.
  *Corrected on 29 September 2026:* the flag is written and read back, and
  nothing in the layout reads it, so the header does not come back; the
  review of tables found it (T42), and **C62** is where it will.

  **Convert to Text** turns the table back into paragraphs, one per row with the
  cells tabbed apart, which is Word's own separator and what makes the result
  convertible back.
- [x] **C17. The rest of the Header & Footer tab.** Header from Top, Footer from
  Bottom, and Insert Alignment Tab.
  *Done:* all three, in a Position group where Word puts them.

  **Header from Top** and **Footer from Bottom** are two more boxes on the
  machinery of **C12**. They are not margins — a margin says where the text
  starts, and these say where the furniture sits in the space above and below
  it — and they share one element in the file, `w:pgMar`, so writing one must
  not take the other with it. The layout already honoured them; what was
  missing was any way to say what they should be.

  **Insert Alignment Tab** is `w:ptab`, which was neither read nor written
  before. It is a tab that goes to the middle of the line or to its far end
  whatever the tab stops say, and it is what a header with a title on the left
  and a page number on the right is made of — such a header keeps its shape when
  the margins move, because there is no stop that would have to move with them.
  The layout tells it apart from an ordinary tab by what the item carries: an
  ordinary tab looks its target up among the stops, and this one is told where
  it is going.

- [x] **C18. The way a dialog is laid out.** Every dialog here put one field
  per row with its label down the left. Word's put related fields side by side
  with their labels above them, and group the rest inside boxes with a caption
  on the edge.
  *Done:* two markers in the one flat list of fields, so that a field keeps the
  same number however the rows are arranged — the same reason the tabs of
  **C9** are a marker rather than a list of lists.
  `Field::Columns(n)` puts the next *n* fields across one row; a row of tick
  boxes is noticed and drawn without the empty line a label above each one
  would leave. `Field::Group(caption)` draws Word's rectangle with its caption
  on the top edge, holding everything until the next group or the next tab, and
  sets its contents in from the panel's edge.
  One place decides the rows — `Dialog::rows_of` — and both the drawing and the
  measuring go through it, because a panel measured one way and laid out
  another is a panel with its buttons on top of its last field.
  Two dialogs rebuilt on it: the **Font** dialog is now Word's arrangement —
  Font, Font style and Size across the top, three colours under them, the seven
  effects in two columns inside a box, the preview in a box of its own — and
  **Page Setup** has its four margins two by two inside "Margins" with the
  paper under them inside "Paper". The rest follow as they are built.
  Also added: `Renderer::draw_within`, so a font with a long name stops at the
  edge of its box instead of running out over the field beside it.

- [x] **C19. AutoCorrect.** There was none at all: no list of replacements, no
  replacing as you type, and so nothing for the button in **C5** to add to.
  Word's is four tabs — AutoCorrect (the replacement list, plus the five tick
  boxes: two initial capitals, first letter of a sentence, day names, the Caps
  Lock fix), AutoFormat As You Type (straight quotes to curly, ordinals to
  superscript, fractions, hyphens to dashes, automatic lists), AutoFormat, and
  Actions.
  Most of it is one mechanism: watch what was typed since the last word
  boundary, and replace it. The mechanism is the item; the tables are what goes
  on top.
  *Done:* the rules are in `autocorrect`, where they are decided about text and
  nothing else; `editor/correcting` is where they meet a document with a caret
  in it, and wraps each correction in a gesture of its own so that one undo
  takes it back and leaves what was typed. Both of Word's tabs that have
  anything behind them are drawn, reached from Options ▸ Proofing, with the
  Exceptions dialog behind them; the lists and the switches are kept in the
  settings file. The replacement list is ours and not Microsoft's — twenty-two
  misspellings, none of which could be a surname.
  *Not done, and named here rather than drawn as a dead switch:* Word's two
  "Automatically add words to list" boxes, which put a word on an exception
  list when a correction is undone straight after it is made; its Math
  AutoCorrect tab, which needs the equation editor; its AutoFormat tab, which
  reformats a whole document at once; and its Actions tab, which offers to look
  a name up in an address book. Numbered lists begun by typing start at one,
  because the numbering model has no other starting number yet. **F5** did the
  two boxes, the lists that begin where the typing did, and the little box
  under a correction; the three tabs are still named there.

- [x] **C20. Customize Ribbon and the Quick Access Toolbar.** Two of the
  categories **C7** leaves out, and they are one job: both are a person saying
  which commands go where.
  The ribbon here is a static table — `RIBBON_GROUPS` and its neighbours — so
  customising it means that table becoming a starting point rather than the
  whole truth, with what a person changed kept beside it in the settings.
  Word's Quick Access Toolbar is the row of small buttons in the title bar,
  which exists here with three fixed commands on it.
  *Done:* `chrome::customise` keeps what was changed rather than a copy of the
  ribbon — which groups are switched off, which tabs were reordered, what was
  added where, and what is on the toolbar — so a group added to this program
  later still turns up on the ribbon of somebody who customised it a year ago.
  The two pages are the last two tabs of Options, laid out as Word lays them
  out: every command on the left, the toolbar or the ribbon on the right, and
  Add, Remove, Move Up, Move Down and Reset along the bottom. The right-hand
  list is a new kind of dialog field — a tree that folds open, with a tick box
  against each group — because the nine tabs and their fifty groups are not a
  list anybody could scroll. Everything is written into the settings file, and
  a file that says nothing says it in no lines at all.
  *Not done, and named here rather than drawn as a dead control:* hiding a whole
  tab, renaming a tab or a group, making a new tab or a new group, and taking
  off a command that came with the group — that last one would mean the table in
  `chrome::ribbon` no longer describing the ribbon, and there is nowhere yet to
  say "this button of Word's is not shown". Word's Import/Export of a
  customisation file is missing too.
- [x] **C21. The order things are drawn in, and which marks are shown.** Two
  gaps **C11** found, both of them a missing distinction rather than a missing
  button.
  Word's **Bring Forward** and **Send Backward** move one drawing in front of
  or behind another. A drawing carries that order in `wp:anchor
  relativeHeight`, which is read here as nothing and written as the same number
  for every drawing, so two that overlap are drawn in the order they happen to
  appear in the document. It needs the number on the anchor, the layout drawing
  images and shapes in one sequence rather than all the images and then all the
  shapes, and the four commands that move a drawing through it.
  Word's **Show Markup** switches comments, insertions and deletions, and
  formatting changes on and off one at a time. There is one switch here, and it
  covers insertions and deletions: comments leave no mark in the text to hide,
  and a formatting change (`w:rPrChange`) is neither recorded nor drawn.
  *Done:* the number is read, written and counted up as drawings are added, so
  a new one goes on top the way Word's does. A page hands out its drawings in
  one sequence — pictures and shapes together, ordered by what their anchors
  say — and both the screen and the PDF draw that sequence. `behindDoc` is
  honoured as well, which it was not: a drawing in front of the text is drawn in
  front of it, with the words inside a shape drawn after its fill. Bring Forward
  and Send Backward are Word's two split buttons: the face moves the drawing one
  place through the pile, and the arrow offers to move it the whole way or out
  of the pile altogether.
  Show Markup is Word's menu, with a tick against each of the three kinds.
  Formatting changes needed the whole of `w:rPrChange` behind them — recording
  one when a person formats text while changes are being tracked, writing it,
  reading it back, marking the text in its author's colour, and accepting or
  rejecting it. Rejecting puts back exactly what the run's properties said
  before, which is what the record holds.
  *Not done, and named here rather than drawn as a dead switch:* Word's
  balloons, which say down the margin what each change was — the marks here say
  that something changed and not what; `w:pPrChange`, the same record for a
  paragraph's own formatting rather than a run's; and the Ink line of Word's
  menu, which needs a pen. A picture still cannot float — `Picture` carries no
  anchor, so a picture read from Word keeps its position in the file and is laid
  out in the line of text. That is **C26**.
- [x] **C22. A selection of more than one stretch.** Word can hold several
  separate stretches of text selected at once: Ctrl and a drag adds to the
  selection, and its Select menu uses it for "Select Objects" and "Select All
  Text With Similar Formatting". Here a selection is one anchor and one caret,
  so there is nowhere to put the second stretch.
  It reaches further than the two menu entries: Find All, formatting applied to
  every heading at once, and a column selection made with Alt all want it.
  *Done:* a selection is the stretch being dragged now plus the ones dragged
  before it. Ctrl and a drag adds one; Ctrl and a click goes on taking the
  sentence, the two told apart by whether the drag ever moved. Everything that
  works on a selection works on all of them at once and as one undo step:
  character formatting, whether a format reads as on, clearing formatting,
  deleting — last stretch first, so that taking one out does not move the ones
  still to go — copying, and the paragraph commands, which touch a paragraph
  once however many stretches land in it. Two stretches that overlap are one.
  **Select All Text With Similar Formatting** is on the Select menu, and is the
  payoff: it finds every run set the way the one at the caret is set — the
  typeface, the size, the colour, and whether it is bold, italic or underlined —
  and selects them all, ready to be changed in one press. Runs that touch and
  look alike are one stretch, because a document's run boundaries are not
  something anybody put there on purpose.
  **Alt and a drag** takes a rectangle of text, which is one stretch per line
  and could not be held at all before this.
  *Not done:* **Select Objects**, which the entry above assumed was the same
  problem and is not. It is a mode in which a click selects a drawing rather
  than text, and it needs a selection that is not text at all — handles, a drag
  that moves a drawing, and the Arrange commands acting on what is selected
  rather than on what the caret is beside. That is **C27**.
- [x] **C23. The rest of what a border can look like.** Three gaps **C14**
  found, all of them about drawing rather than about the file.
  Word's **Shadow** and **3-D** settings were the same box drawn with a drop
  shadow or a bevel, which was a way of drawing a line this program did not
  have; and its **line styles** are longer than the five that were drawn —
  `dotDash`, `dashDotStroked`, `wave`, `doubleWave`, `triple` and the rest were
  read and written faithfully and drawn as the nearest of the five.
  *Done:* `wp-layout/src/borders.rs` draws every one of Word's twenty-five line
  styles as itself, out of plain rectangles: the layered ones share the band out
  between lines and gaps, the broken ones repeat a pattern of marks, a wave is a
  column of marks whose height follows a sine, and the bevelled ones are two
  half-bands, one lighter than the border's colour and one darker. Which edge of
  the box a line is now reaches the drawing, because a raised box is lit from
  the top left and an edge that did not know which it was would look flat.
  Shadow and 3-D are `w:shadow` and `w:frame` on each edge, read, written and
  drawn, and Word's Setting column has them back — they are not other kinds of
  box, which is why picking one ticks the same four edges Box does.
  *Not done:* **Art** borders, which are **C28** below.

- [x] **C28. The art borders, as far as they can be drawn.** Word's Art gallery
  is about a hundred and sixty repeating pictures — apples, hearts, rope,
  people — written as `w:top w:val="apples"` and drawn from artwork Word ships.
  That artwork is Microsoft's, it is not licensed for reuse, and this program
  draws nothing it did not make: see the note at the top of `chrome/icons.rs`,
  which says the same thing about Word's button art. So "a document with an art
  border looks the same here as it does there" is not a thing this project can
  promise, and **C23** was wrong to say it would.
  *Done:* the part of the gallery that is geometry rather than pictures.
  `wp-layout/src/artborders.rs` draws twenty-nine of Word's names as what they
  say they are, out of the same rectangles every line style is made of: rows of
  black dashes, dots and squares; the white ones, which are a bar with the marks
  cut out of it, so that "white" is whatever colour the paper is and stays right
  in a dark window; the wide ones, which are bands of lines; a checkerboard, a
  checked bar and quadrants; triangles, shark's teeth and grey diamonds; a
  sawtooth, a zigzag, a zigzag of stitches, a wave, and the Greek wave, which is
  the one of them that is exactly rails and risers; crosses, hatching that leans
  either way, squares eclipsing and nested and shadowed, and a gradient. Grey is
  the ink at half its opacity, because grey is halfway to a paper whose colour
  nothing here knows.
  `wp-docx/src/art.rs` holds the whole enumeration, not only the part drawn,
  because **an art border says its width in whole points where a line says it in
  eighths of one** — the same attribute, two units, and a program that read them
  the same way would draw a twenty-point border of apples two and a half points
  wide. `Border::width_points` knows the difference.
  The **Art gallery is on the Page Border tab**, beside the line styles. Both
  lists write `w:val`, so they are kept in step: picking a pattern is picking a
  border and the line style stops being what is drawn, picking a line style puts
  the art back to none, and the widths beside them change to the unit that
  belongs to whichever kind is chosen — keeping the width in front of a person's
  eyes and moving only the unit under it.
  A document carrying one of Word's pictures **keeps it**: the gallery shows it
  and says it is kept, so that opening the dialog and pressing OK cannot quietly
  turn somebody's border of apples into a plain line, and it is drawn as a plain
  line of its width rather than as some other picture.
  *Fixed on the way:* a page border asked for on a document that had none was
  put zero points from the edge of the paper, because nothing filled in the
  distance Word uses when it is not told one. `PageBorders::default` is now
  twenty-four points, as Word's is.
  *Not done:* the corner motif. Word turns a corner with a piece drawn for the
  purpose; here the two edges simply meet, and at a wide border the pattern
  doubles up in the corner square. And the pictures, which are not this
  project's to draw.
- [x] **C24. The Border Styles gallery and the Border Painter.** The last two
  things the Table Design tab is missing, and one job: Word's gallery picks a
  line — a style, a width and a colour — and the painter is the pen that puts
  that line on whichever edge it is dragged along.
  The painter is a mode rather than a command, like the format painter: one
  press arms it, and it stays armed until it is pressed again or Escape is
  pressed. What it needs beyond that is a way to say which edge of which cell
  the pointer is nearest, which nothing here works out yet.
  The gallery is the line styles of **C23** over again, so the two are worth
  doing together.
  *Done:* both, in `editor/borderpainter.rs`. The gallery offers each of the
  line shapes that read clearly at a table's scale, in several weights and in
  each of the colours the borders dialog offers; picking one arms the pen, as
  picking one does in Word. The pen is put down by pressing the button again or
  by Escape, and a press that lands nowhere near an edge is an ordinary press —
  a pen out must not swallow every click in the document.
  Which edge the pen is on needed the cells as rectangles, which nothing
  recorded: a cell's edges are nowhere in the text. The layout keeps them now
  (`PlacedCell`), and `Document::set_cell_edge` puts a line on one edge of one
  cell and leaves the other three alone — the only thing that could be said
  before was the whole table's borders.
  Found while doing it: a table's lines were drawn as plain rectangles, so the
  gallery would have offered double, triple and wave and drawn all three as one
  thick line. They go through the same code a paragraph's and a page's do now,
  which is what **C23** wrote.
- [x] **C25. Draw Table and the Eraser.** Word's two table pens: the one that
  draws a line through a cell and the one that rubs a line out. Both are modes,
  like the Border Painter of **C24**, and both needed the same missing thing —
  which edge of which cell the pointer is nearest — which **C24** built.
  *Done:* a line drawn down a cell makes two cells of it, a line drawn across it
  makes two rows, and the eraser joins the cells either side of whatever line it
  is pressed on. The first needed `split_cell_across`, which widens the table's
  grid and gives every other row's cell one column more so that nothing but the
  drawn cell looks different; the second needed `split_cell_down`, which is a
  whole new row with every other column merged down across the two, because a
  table has no way to say that one cell is two rows tall. The eraser is
  `merge_cells` on the two cells either side, because rubbing out the line
  between two cells and merging them are the same thing.
  A tap draws nothing, a pen at the edge of the table says so rather than doing
  something surprising, only one pen is in hand at a time, and Escape puts
  whichever it is down.
  *The rest of what the Table Layout tab wants* was listed here as though it
  were one item and is five: **C29** AutoFit, **C30** Text Direction, **C31**
  Cell Margins, **C32** Sort, **C33** Formula. Each needs something different
  and none of them is the others' work.

- [x] **C26. A picture that floats.** `Picture` carried no anchor, so only a
  shape can float. A picture read from a Word document where it floats keeps its
  anchor in the file — nothing is lost on saving — but it is laid out in the
  line of text, which is the wrong place, and the whole Arrange group is about
  a drawing it cannot act on. **C21** made the order among drawings work and
  found this while doing it: a picture and a shape cannot be overlapped, because
  a picture cannot be anywhere but in the line.
  The pieces are all there: `anchor::read_anchor` reads one, `place_float` lays
  one out and reserves the room round it, and `Wrap` is honoured. What is
  missing is the field on `Picture`, the reader filling it in, the writer
  putting it back, and the layout sending a picture down the same path a shape
  goes down.
  *Done:* `Picture` carries an anchor, the reader fills it in, and a floating
  picture goes down the same path a floating shape does — `place_float` was
  split into the part that works out where a drawing goes, which is the same
  for both, and the part that draws it, which is not.
  Changing a picture's anchor is surgery rather than a rewrite, in
  `wp-docx/src/floating.rs`. A shape is read into the model and written back out
  of it; a picture's element holds a great deal the model does not — the crop,
  the effects, the colour it was recoloured to — and rebuilding it would throw
  all of that away. The wrapper is renamed between `wp:inline` and `wp:anchor`
  and its own attributes and children changed, and the graphic below it is never
  touched.
  Both kinds go through one door now: `anchor_here` and `set_anchor_here` answer
  for a shape and for a picture, so every command in Arrange stopped having to
  ask which it was. The pile is one pile, so a picture laid over a shape is over
  it or under it.
  *The one thing that is still the caret's doing:* which drawing a command acts
  on. A caret between a picture and a shape is beside both, and the shape
  answers. That is the same limit `arrange.rs` has always had and is **C27**.

- [x] **C27. A drawing that can be selected.** Every command in the Arrange
  group acted on the drawing nearest the caret, because there was no other way
  to say which drawing was meant: a drawing could not be selected here at all.
  **C22** found this while making a selection able to hold several stretches:
  that is a selection of *text*, and a drawing is not text.
  *Done:* the second kind of selection, in `editor/handles.rs`. `chosen_drawing`
  is the place one drawing is at — one thing, chosen or not, beside the stretches
  of text rather than among them. Clicking a drawing chooses it, the eight
  handles are drawn round it, dragging its body moves it and dragging a handle
  resizes it. A drawing in the line of text starts floating when it is dragged,
  because a drawing in the line has no position of its own to change; resizing
  one leaves it in the line, because it has a size there. One drag is one thing
  to undo however many moves it is made of. A drawing just inserted comes up
  chosen, which is what Word does and what anybody who has just made a shape
  wants.
  Choosing and the caret give each other up: moving the caret or typing drops
  the drawing, and a press in the text does too. They are the same choice made
  twice — what the next command is about — and both drawn at once would be the
  program saying two things.
  **Select Objects** is on the Select menu and is a mode, like the format
  painter: while it is in hand a press chooses a drawing and never puts the
  caret in the text, the menu shows it as on, and Escape puts it down.
  The **Selection Pane** lists pictures as well as shapes now — it listed only
  shapes — and picking one out of it chooses the drawing rather than merely
  moving the caret near it.
  The model learned to name a drawing exactly rather than "the one beside the
  caret": `drawing_place_here` settles which of two neighbours is meant, and
  `shape_at`, `anchor_at`, `drawing_size_at`, `set_anchor_at`,
  `set_drawing_size_at` and `replace_shape_at` all take that place. That closes
  the limit **C26** left behind: a caret between a picture and a shape no longer
  always answers for the shape, because a drawing clicked is one drawing.
  *Not done:* more than one drawing at a time — shift-clicking a second, and the
  rubber band Word's Select Objects drags round several — which is **C34**,
  together with the Align, Group and Rotate buttons that are what having several
  is for. A corner handle shows the sideways resize pointer rather than a
  diagonal one, because the shell offers no diagonal. The pane shows the name
  the file gives a picture, which for a picture carrying a description is the
  description: the model keeps one field where the format has two.

- [x] **C29. AutoFit.** Word's three: fit to contents, fit to window, fixed
  column width. All three belonged together: a menu with two live rows would
  have been worse than none.
  *Done:* the AutoFit button is on the Table Layout tab where Word's is, it
  drops Word's three rows and shows which of them is in force, and each does
  what Word's does. `wp-layout/src/tablefit.rs` is how wide the columns come
  out; `wp_docx::model::TableFit` is the one decision the file says in two
  places — `w:tblW`, the width the table would like to be, and `w:tblLayout`,
  whether its columns may be worked out at all.
  **Fitting to contents** wanted the layout to measure what is in every cell
  with no width to break it against, which nothing did: every measuring pass
  this program has is given a width first. So a cell is measured by building its
  items and never breaking them into lines, which gives the two numbers any
  table algorithm needs — how wide it *must* be, its widest single item, and how
  wide it *would like* to be, all of them in one line. A column takes the
  largest of each over its cells. If every column can have what it wants it
  does, and the table is as wide as its contents; if they cannot, each gets what
  it must have and the rest is shared out in proportion to what each still
  wanted.
  A cell that **states a width still gets it**: `w:tcW` is a preferred width and
  Word writes one on every cell of every table it makes, so a stated width is a
  floor and the content is what can push a column past it. That is why a table
  from Word keeps the shape it had there — and it is why **AutoFit Contents**
  is the command that *clears* those preferences. With nothing preferred the
  text alone decides, which is Word's own mechanism and the whole reason the
  command has anything to do. Tables this program inserts now state their widths
  too, as Word's do; they did not, and without that a new table would have hugged
  its empty cells the moment it was made.
  **Fixed column width** keeps the widths in front of you rather than the ones
  the file was last written with: the editor reads the columns off the page it
  has drawn and writes those into the grid and into every cell before fixing
  them, so the table does not jump when it is frozen. The model could not do it
  — it has never seen a page.
  *Fixed on the way:* a cell width written as a percentage was read as
  twentieths of a point, so a cell asking for half the table asked for a hundred
  and twenty-five points instead. Percentages are left to the layout now.
  *What it costs:* a pass over the text of every cell of every fitted table,
  every time the document is laid out again — items only, no line breaking, but
  not free. Nothing is cached; that is **B6**'s business when a large document
  turns up to measure it against.

- [x] **C30. Text Direction in a cell.** `w:textDirection`, which turns a cell's
  text through a right angle — what the headings of a narrow column are set in.
  *Done:* Word's button is on the Table Layout tab beside the nine alignments,
  and it cycles the way Word's does: across, then reading downwards, then
  reading upwards, then across again.
  **The text is laid out straight and turned afterwards.** A turned cell is laid
  out into a box as long as the row is tall, on a page of its own, and that page
  is then mapped onto the real one a right angle over. Breaking a line, spacing
  it, aligning it and numbering it are the same work whichever way up the text
  is, so the layout never learns about angles at all: `Frame` in
  `wp-layout/src/layout.rs` is the whole of the turn, and only the drawing and
  the questions a line is asked about the page go through it.
  The row is **as tall as the turned text is long**, which is the measuring pass
  asking a turned cell the other question: not how tall it came out but how far
  along. A column of turned headings comes out as narrow as one line is deep
  rather than as long as the heading is, which is the whole reason a heading is
  turned — so **C29**'s fitting had to learn the same distinction.
  A **click lands where it is aimed** inside a turned cell, the **caret lies the
  other way** there, and so does a **selection band**: a line keeps its own
  coordinates and carries the frame that maps them onto the page, so everything
  that asks a line a question about the page — a click, a caret, a band — goes
  through one place. The letters themselves are turned about their own origins
  as they are drawn, which is what `draw_transformed` did for a watermark and
  now does per glyph.
  Which glyphs are turned is kept as spans on the page rather than as a field on
  every letter: a document is mostly text the ordinary way up, and a hundred
  thousand words should not each carry a field saying so.
  *Not done:* a picture inside a turned cell is put in the right place and drawn
  the way up it was, where Word turns it too. The three vertical East Asian
  values — `lrTbV`, `tbRlV`, `tbLrV` — are not turns this program makes: they
  read as the ordinary way up and stay in the file exactly as they came. And the
  up and down arrow keys inside a turned cell step by the page's idea of up and
  down rather than the cell's.

- [x] **C35. Where the text sits down a cell.** Found while doing **C30**: the
  Alignment group's nine buttons answer two questions — where the text sits
  across the cell and where it sits up and down it — and only the first of them
  was drawn. `w:vAlign` was written faithfully and Word showed it; here the text
  stayed at the top of the cell whichever of the nine was pressed, so six of the
  nine looked like the three above them.
  *Done:* the model carries `w:vAlign` on the cell, and the measuring pass — the
  one **C29** and **C30** already needed — hands the placing pass each cell's own
  height as well as the row's. The room left over is the difference, and the
  text is moved down by half of it for the middle and all of it for the bottom.
  A cell that fills its row has none and does not move, which is why a table
  where every cell is the same height looks exactly as it did.
  *Changed on the way:* the button set the whole row's cells and now sets the one
  the caret is in, which is what Word does with no selection — and what the
  other half of the same question, Text Direction, already did. One of the two
  changing a row and the other a cell was a difference nobody could have
  guessed.
  *Not done:* where the text sits in a cell whose text is **turned**. It fills
  the length it asked for, so there is nothing left over to move it in; Word
  moves it along the line instead, which is a different question from this one.

- [x] **C31. Cell Margins, and the room between cells.** Word's dialog holds
  four margins and a tick box for spacing between cells. Two of the margins were
  read and used; the top and the bottom were two constants in the layout, and
  the spacing was neither read nor laid out.
  *Done:* all four are the document's. `w:tblCellMar` is read and written whole,
  a side nobody states is Word's own default — a little at each side and nothing
  above or below — and the two constants are gone. A cell's own `w:tcMar` is
  honoured as well, side by side: a cell that states one margin and says nothing
  about the other three gets its own for the one and the table's for the rest,
  which is what the format means by a preference.
  **The room between cells is a geometry and not a number**, which was the
  expensive half. Half of `w:tblCellSpacing` goes on each side of every cell, so
  the gap between two of them is the whole of it and the gap between a cell and
  the edge of the table is half — the grid stays the table's geometry rather
  than something the spacing has moved. The cells stop touching, each is drawn
  with a border of its own, the paper shows between them, and the row is taller
  by the spacing while the cells inside it are not.
  Word's **Table Options** is folded into the Table tab of Table Properties
  rather than hidden behind a button, the way its page-border Options already
  is: the four margins, and "Space between cells" with the measurement beside
  it. Ticking it with nothing typed leaves the room Word's own dialog starts
  at, so the tick always does something.
  *Not done:* Word's **Cell Options**, which sets one cell's margins rather than
  the table's. A document that arrives with `w:tcMar` on a cell is laid out with
  it, but there is no way to put one there from here. And a spacing written as a
  percentage of the table is read as none: the model keeps twips, and Word's own
  dialog cannot ask for a percentage either.

- [x] **C32. Sorting the rows of a table.** The Sort command sorted paragraphs
  and nothing else, by replacing their text with the same text in another order
  — which ordered the words and threw away everything the paragraphs were
  formatted with.
  *Done:* `wp-docx/src/sorting.rs`. **Nothing is rewritten**: the `w:tr` elements
  are read to find out what they say and the same elements are put back in
  another order, so every cell keeps its width, its shading, its borders and the
  formatting of every run in it. Sorting a run of paragraphs moves the `w:p`
  elements the same way, which is the flaw above fixed rather than kept.
  **Word's three keys**: by a column, and where two rows agree by a second, and
  where they agree again by a third. Each has its own column, its own reading —
  words, a number, a date — and its own direction. Rows that are equal on every
  key keep the order they came in, which is what makes the third key mean
  anything.
  A **number is read out of whatever else is in the cell**, as Word's is:
  "£1,234.50 (est.)" sorts as 1234.5, and a cell with no number in it sorts
  before every cell that has one. A **date** reads the day first — what the
  United Kingdom and most of the world write, and what this program's own
  language setting says — with `2024-03-04`, the one form nobody can misread,
  read as itself.
  The **header row** is a tick and not a guess: a table whose first row names the
  columns and one whose first row is data look alike to a program, and sorting
  the names into the middle of the figures is the kind of mistake nobody
  forgives.
  The same dialog serves both places, because Word's asks the same three
  questions in both and only the list of columns differs: the columns of the
  table, or "Paragraphs" and the fields the tabs separate. Sort is on the Data
  group of the Table Layout tab now as well as on the Home tab, where Word has
  it.
  *Not done:* Word's **Options** inside the Sort dialog — separating fields at
  commas or at something else rather than at tabs, sorting with case counted,
  the sorting language, and "sort column only". Each of those is a row in a
  dialog behind the dialog, and none of them is what sorting a table is for.

- [x] **C33. Formula.** `=SUM(ABOVE)` and its kin, as a field.
  *Done:* `wp-docx/src/formula.rs` reads the instruction and does the
  arithmetic. An expression is an expression — brackets, `+ - * /`, a per cent
  sign, comparisons — over numbers, cells named the way a spreadsheet names them
  (`B2`, `A1:C3`), and Word's four words for the cells around this one:
  `ABOVE`, `BELOW`, `LEFT` and `RIGHT`, each running until the first blank cell,
  which is where a column of figures starts. The functions are `SUM`, `AVERAGE`,
  `COUNT`, `MIN`, `MAX`, `PRODUCT`, `ABS`, `INT`, `ROUND`, `MOD`, `SIGN`, `IF`,
  `AND`, `OR`, `NOT`, `TRUE` and `FALSE`.
  A cell holds words and a formula wants a number, so the number is **read out of
  whatever the cell says** — the same reading a column sorted as numbers gets
  (**C32**), because a cell must not mean one thing to sorting and another to
  adding up. The `\#` picture is applied to the answer, and the seven formats
  Word's dialog offers are in the list beside the box.
  **The answer is worked out at every layout and never read back from the file.**
  A formula that answered with what it said last time would be wrong the moment
  a figure above it changed, and a person who has just corrected a number should
  not have to know that a field needs updating.
  Word's **Formula dialog** is on the Data group of the Table Layout tab where
  Word's is: the formula, guessed at the way Word guesses — `=SUM(ABOVE)` under a
  column of figures, `=SUM(LEFT)` beside a row of them — a list of number
  formats, and a list of functions that types into the formula rather than
  deciding anything of its own.
  *Not done:* a formula that answers with **text**. Word's `IF` can choose
  between two pieces of writing as well as between two numbers, and one that
  asks for that is answered with nothing. **Bookmarks** as operands, and the
  `DEFINED` that asks about one, which is the "Paste bookmark" list beside the
  functions. And the answer **kept in the file** is the one from when the formula
  was put there: what is shown is worked out afresh, but a program that cannot do
  arithmetic reads the older one, exactly as it reads Word's.

- [x] **C34. More than one drawing at a time, and Align.** Word's Arrange group
  had three buttons this one did not, and **C27** is why: two of them are about
  more than one drawing, and the selection it built held one. This is that
  selection widened, and the one of the three that needed nothing else.
  *Done:* `chosen_drawings` is a list. **Shift and a click** adds one to it or
  takes it out again; a **band** swept round a handful under Select Objects
  takes every drawing it touches; each of them carries its own eight handles; a
  **drag on any of them moves all of them**, as one thing to undo; and every
  command in Arrange — Wrap Text, Position, the two pile menus — acts on all of
  them while each keeps where it sits.
  **Align** is a menu of eleven on the Arrange group, which is Word's: the six
  alignments, the two that spread them out at even gaps, and the three at the
  foot that say what the eight are measured against. Those three are a mode and
  not commands: picking one changes nothing until an alignment is pressed, and
  the menu shows which is in force.
  Lining a drawing up is done by **moving it by a difference** rather than by
  working out a position: a drawing's anchor counts from the text, the paper or
  the paragraph, and which of those is its own business — but a distance is the
  same distance whatever it is measured from, and the layout has already worked
  out where every drawing is on the page. One drawing has nothing to line up
  with, and is told so rather than left wondering.
  *Not done:* Word also lets the arrow keys nudge the drawings that are chosen,
  which is the keyboard's half of the same gesture; and its band takes only the
  drawings it wholly encloses when Ctrl is held, where this one always takes
  what it touches.

- [x] **C36. Rotate.** `a:xfrm/@rot`, in sixtieths of a thousandth of a degree,
  read and written; the geometry turned through the angle as it is drawn and the
  pixels of a picture turned with it; Word's Rotate menu — right ninety, left
  ninety, flip vertically, flip horizontally — and the rotation handle above the
  drawing's top edge. A button that stored an angle and drew the shape the way up
  it always was would be a button that does nothing, which is why the drawing
  and the file are one item and not two.
  *Done:* `rot`, `flipH` and `flipV` are read and written for both kinds of
  drawing — a shape through its model, a picture by changing its own transform
  where it stands, which is the same promise **C27** made about anchors: nothing
  a picture's element says that this program does not model is thrown away. A
  picture with no `a:xfrm`, and even one with no `pic:spPr` to put one in, is
  given what it lacks rather than refusing to turn.
  The page carries the angle in radians, and the renderer turns the fill, the
  outline, the shadow and the words inside a shape about its middle together;
  a picture's pixels go through a turned sampler that walks the destination and
  asks each pixel which part of the picture it stands for, so a photograph at an
  angle is no coarser than the same photograph straight. Mirroring is done
  before the turn, which is the order the format states and the order anybody
  would do it with a sheet of paper. The shadow is offset after the turn,
  because the light does not turn with the shape.
  The **Rotate** menu is four rows on the Arrange group, and the **round handle**
  above the top edge turns a drawing by dragging: the sweep is measured from
  where the pointer started, not from straight up, so taking hold of the handle
  does not itself move anything. A whole drag is one thing to undo.
  *Not done:* Word's fifth row, More Rotation Options, opens the Layout dialog,
  which this program has not got — see **C38**. Word snaps the handle to
  fifteen degrees while Shift is held, and the shell does not yet say whether a
  modifier is down during a drag. The eight handles that change the size, and
  the box round a chosen drawing, stay square to the page when the drawing is
  turned, where Word turns them with it — so a turned drawing is resized along
  its own axes while the pointer moves along the screen's.

- [x] **C38. The Layout dialog.** Word's Size, Position and Text Wrapping in one
  three-tabbed dialog, reached from More Rotation Options, from More Layout
  Options under Position and Wrap Text, and from the Size group's launcher. It
  holds the exact height and width of a drawing, its scale as a percentage with
  a lock that keeps the two in step, the rotation in degrees, the position
  measured from any of the frames an anchor can count from, and the wrapping
  with the distance from the text on each of the four sides. Everything in it
  exists in the model already — **C27** built the anchors, **C36** the angle,
  and dragging a handle already sets the size — so this is the dialog and not
  what is behind it.
  *Done:* the three tabs, behind the three doors Word puts them behind — More
  Layout Options at the foot of the Position menu and of the Wrap Text menu, and
  More Rotation Options at the foot of Rotate. Every box reads what the drawing
  says and changes it, and the page agrees: Position holds how the drawing is
  placed along each axis and what that is measured from, Text Wrapping the style
  and the room on each of the four sides, Size the height, the width, a lock
  that works one out from the other, and the angle in degrees. One answer is one
  thing to take back, and cancelling changes nothing.
  *Not done:* Word's Scale boxes, which are a percentage of the picture's
  original size — the original is the decoded picture's own size and nothing
  keeps it, so a percentage would be a percentage of nothing. Its Relative
  position and Relative width, which measure in percentages of a frame rather
  than in inches. Its Wrap text side — both sides, left only, right only,
  largest only — which the format writes as `wrapSquare/@wrapText` and this does
  not read. And the three tick boxes at the foot of Position: Move object with
  text, Allow overlap and Lock anchor. See **C43**.

- [x] **C37. Group.** `wpg:wgp`: several drawings written as one, with a
  coordinate space of its own — the group states the rectangle it covers and the
  rectangle its children are measured in, and every child is placed through that
  mapping. It is not a command on a selection but a kind of drawing the model has
  never held, which is why it is here rather than beside Align.
  Word's Group, Ungroup and Regroup are the three commands, and a group holds
  shapes, pictures and other groups.
  *Done:* a group is read into a model of its own and laid out through **both
  rectangles**, so a group Word resized draws its members at the proportion Word
  drew them: where a member sits in the inner rectangle is where it sits in the
  outer one, as a fraction of each. A group inside a group is the same thing one
  rectangle further in, and recurses. Everything in a group answers to the
  group's one place in the text, so a press anywhere on it takes hold of the
  whole, and the handles go round all of it: a drawing drawn in several pieces is
  folded back into the one rectangle that holds them.
  A group's element is **carried through and never rebuilt** — a group holds
  pictures, and a picture rebuilt from what is modelled would lose its crop, its
  effects and its recolouring. So Group and Ungroup move the members' elements
  themselves: into a `wpg:wgp` whose inner and outer rectangles start out the
  same, and back out again with the place each one had. The rectangles are
  measured on the page and handed down, because a drawing's anchor counts from
  the text, the paper or the paragraph and which of those is its own business —
  the same reasoning **C34**'s Align follows.
  Because it is a drawing like any other, Wrap Text, Position, the two pile
  menus, Align and **C36**'s Rotate act on a group without knowing it is one, and
  dragging a size handle scales what is inside it the way Word's does: the
  rectangle it is drawn in changes and the one its members are measured in does
  not. Two bugs that only a group could show were fixed on the way — turning a
  drawing wrote the angle onto every transform under it rather than the
  outermost, and a drawing that held a group was read as the first shape in it.
  *Not done:* Word lets a second click reach into a group and choose one drawing
  inside it; here a press on a group takes the group. A `wpg:graphicFrame` — a
  chart or a table inside a group — is not read as a member and is passed over.
  Drawings on two pages are not grouped, because there is no origin to measure
  both from. Regroup remembers one group at a time and forgets it when the
  document is closed, which is Word's behaviour, but Word also offers it greyed
  out rather than saying so afterwards.

- [x] **C39. Using a table with the mouse and the keyboard.** The commands about
  tables were all there and working, and the table itself could not be used:
  reported as "tables are broken and impossible to use". Four things were wrong,
  and each of them was wrong everywhere rather than in some corner.
  *Done:* a **press lands in the cell it was aimed at**. Every cell of a row is
  at the same height, and a click was answered by the first line level with the
  pointer — which is the first cell of that row, whichever cell was pressed. The
  page now answers a point with the cell that holds it and then with that cell's
  own lines, so the caret goes where it was put; the same answer serves a text
  box beside text, which had the same fault.
  **Tab moves by cell**, and Shift+Tab back, taking what is in the cell it lands
  on so that a table is filled in by typing and pressing Tab. At the last cell it
  adds a row. It typed a tab character before, which is what Ctrl+Tab is for and
  is now.
  **The arrows move by row.** Down went to the next line of the *document*,
  which after the first cell of a row is the second cell of that row — the arrow
  appeared to do nothing three times before reaching the row below. Down and up
  now go to the cell under or over the caret, and off the end of the table to the
  paragraph after or before it, while a cell of several lines is still moved
  through line by line.
  **A drag across cells takes the rectangle between them**, cell by cell and
  whole, as Word does — not the stretch of text from one to the other, which runs
  through every cell written in between. Shift and a press does the same, and so
  does the Select menu, whose Column took the whole table before. The block is
  drawn as the cells themselves, empty ones included; Delete empties them and
  keeps them, and typing empties them and types in the first.
  **The lines can be dragged**: a column's line moves between its two columns
  and the last one makes the table wider, a row's bottom makes it taller, one
  drag is one thing to undo, and the caret stays where it was. The pointer says
  so over a line, and a row whose cells are merged offers nothing — its cells and
  the grid no longer answer to one another.
  **The bar beside a row takes the row**, and the band above a column takes the
  column. Two faults were found underneath: the selection bar compared a window
  coordinate with a page one and so was empty on every page that is not at the
  window's left edge — no click in it had ever selected a line — and a block of
  cells was read back from the text in them, so Merge Cells on a table somebody
  had just inserted merged the one cell the caret was in.
  *Not done:* Word's move handle at the top left of a table and its resize handle
  at the bottom right — **C40**. Double-clicking a column's line to fit it to its
  contents, and the ⊕ buttons Word shows between rows — **C41**.

- [x] **C40. The table's own handles.** Word draws two when the pointer is over a
  table: a four-arrows square just outside the top-left corner, which drags the
  whole table to another place in the document, and a small right angle just
  outside the bottom-right corner, which resizes the whole table — every column
  by the same proportion.
  *Done:* both, drawn outside the corners so they sit over the margin rather
  than over the text, and shown for the table the pointer is over or the table
  the caret is in. The move handle **moves the element** rather than rebuilding
  the table: it is taken out of one place in the tree and put into another, so
  a tracked change, a content control or a property from a later version of the
  format goes with it — the same reasoning **C37**'s groups follow. While it is
  dragged, a line shows where the table would land, because a table that moved
  under the hand would carry the text it passed through along with it. Pressing
  it rather than dragging takes the whole table, which is what Word's does.
  The resize handle scales every column by the same proportion through the same
  grid writing **C39** uses, and fixes the columns as Word does the moment one
  is settled by hand; one drag is one thing to undo.
  *Not done:* Word will drop a table into a cell of another table. This will
  not: the move is refused when the place it was dropped on is inside a
  different parent — inside the table itself, or in a cell — and says so rather
  than doing something surprising.

- [x] **C41. What the pointer offers between rows.** Two more of Word's mouse
  affordances on a table. Double-clicking the line at the right of a column fits
  that column to what is in it — the width of the widest cell, which is
  **C15**'s AutoFit for one column rather than for the table. And resting the
  pointer just outside a line between two rows, or between two columns, shows a
  small ⊕ which inserts a row or a column there when it is pressed: Word's
  quickest way to add one, and the reason its Insert group is rarely reached for.
  *Done:* both. The fit measures the widest line actually drawn in that column
  and adds the room the cell keeps clear either side of it, so it fits what is
  in the column rather than what the file says is in it — the same reasoning
  **C39**'s line dragging follows, and it fixes the columns as Word does once
  one has been settled by hand. A double click anywhere else in a cell goes on
  taking the word.
  The button appears beside the line the pointer is nearest, and only while it
  is near: down the left-hand side for the lines between rows and one at each
  end, along the top for the lines between columns. Pressing it puts the row or
  the column in on the side the line is — which is the whole point of aiming at
  a line rather than at a row.
  *Not done:* Word's button is a circle with a plus that grows a little as the
  pointer nears it, and is drawn on a short line the width of the table. This
  one is the circle and the plus.

- [x] **C43. The rest of the Layout dialog.** What **C38** left: the Scale
  boxes, which need a picture's original size kept beside it; Relative position
  and Relative width, which are percentages of a frame and a second way of
  writing `wp:positionH`; the side the text wraps on, which is
  `wrapSquare/@wrapText`; and Move object with text, Allow overlap and Lock
  anchor, which are three flags on `wp:anchor` that nothing yet reads.
  *Done:* the **Scale** boxes and the **side the text runs down**.
  Scale is a percentage of what a percentage can be of: a picture's own size,
  found by decoding the picture, which is the only place that number exists —
  the file records what a drawing is now and not what it was. A photograph
  brought down to fit the page shows as twenty per cent of itself, and typing
  100 puts it back. A shape has no original, so its boxes are a percentage of
  the size it is now, and the group says which it is rather than pretending.
  Two traps were found and are worth the words: a box shows a measurement
  rounded to two decimals, so reading it back gives a number a few hundred
  English Metric Units away from the one it was filled in with — every box
  looked changed, and the percentages never got a hearing. And a percentage box
  showing 25 could never be asked for 100 while "changed" meant "not a
  hundred". Both are now told by what the box *says* against what it was given.
  The side is `wrapSquare/@wrapText`, read, written and laid out: Left keeps
  the text to the left of the drawing whatever room is beyond it, Right the
  other way about, and Largest is what the layout already did. See **C45** for
  Both sides, which is the default and is not what is drawn.
  *Not done:* Relative position and Relative width — **C44**. Move object with
  text, Allow overlap and Lock anchor — **C46**.

- [x] **C44. Position and size as percentages.** Word's Relative position and
  Relative width in the Layout dialog, which measure a drawing's place and its
  width as a percentage of the page, the margin or the column rather than in
  inches. Written as `wp14:pctPosHOffset` and `wp14:sizeRelH` — the 2010
  extensions.
  *Done:* all four — the width and the height as a share of a frame, and the
  place along each axis as one. Read, written, laid out and in the dialog: a
  picture at half the page width is half of whatever the page is, which is the
  whole point of stating it that way, because the absolute size written beside it
  was right for the paper the document was last saved on.
  The extension markup is not `mc:AlternateContent` after all — Word writes the
  plain values and the extension side by side, and marks the extension's prefix
  ignorable so that a reader which does not know it passes over it. Both halves
  of that are now one helper, and the text effects **C22** wrote its own copy of
  are now through the same one.
  A share of a frame is a third kind of place, beside a distance and an
  alignment, so the box beside the list shows per cent under a per cent sign
  rather than inches under an inch mark. Dragging a drawing or aligning it gives
  the share up, as it already gave up an alignment: what a drag hands over is a
  distance.

- [x] **C45. Text down both sides of a drawing.** `wrapText="bothSides"`, which
  is what Word writes unless told otherwise: a line beside a drawing is broken
  into a piece each side of it. The layout gave a line one left edge and one
  width, so what it drew was the wider side alone — Word's "Largest only" —
  whatever the file said.
  *Done:* a line is laid out in pieces, one for each stretch of room beside the
  drawings at its height, filled left to right. Nearly always one; a drawing
  with room either side of it is what makes it two. The pieces share a baseline
  and follow on from one another, so a caret, a click and a selection band all
  work as they already did — the page holds two lines where it held one, and
  everything that asks a page a question asks it of lines. A word too wide for
  the piece it is offered goes in the next piece rather than being cut; only the
  last piece has nowhere to pass it on to. A stretch narrower than about one
  letter is left empty, as Word leaves the sliver between a picture and the
  margin.
  *Found on the way, and the reason none of this had ever been seen:* a floating
  drawing was making the line it is anchored in as tall as the whole drawing.
  The width was already left out — a floating drawing takes no room on its line
  — but the height was not, so the text after the anchor began *below* the
  drawing and nothing was ever laid out beside one. Square wrapping wrapped
  nothing, in any document, since it was written. One test had been passing on
  the strength of a short last line.
  *Not done:* Word measures the pieces of a justified line together, so the
  spaces come out the same width on both sides of a drawing; each piece here is
  justified within itself.
  *Corrected on 29 September 2026:* the first line of a paragraph is laid out
  before the drawing anchored in it is registered, and runs under the drawing
  where the lines after it go round; found by two reviews (R21, #10), and
  **E19**'s.

- [x] **C46. The three flags at the foot of the Position tab.** Move object with
  text, Allow overlap and Lock anchor. Each is one attribute and none of them is
  only an attribute: allowing overlap means deciding what to do when two
  drawings want the same place — Word pushes the second one down — locking an
  anchor means refusing to move it when the text it hangs from moves, and moving
  with text means the opposite. A tick box that wrote a flag nothing obeyed
  would be a control that does nothing, which is why they were here rather than
  in **C38**.
  *Done:* two of the three, with what they mean.
  **Allow overlap** is `allowOverlap`, and unticking it pushes the drawing down
  until it lies clear of every drawing already placed — down and not sideways,
  because down is where a page has room and moving it across would take it away
  from the text it belongs beside. Each push can uncover another neighbour, so
  it is done until nothing is in the way, and counted so that a page crowded
  with drawings cannot become a loop.
  **Move object with text** is the same answer the vertical frame already gave:
  a drawing measured from the paragraph moves with it and one measured from the
  page does not. So the tick and the list above it are two faces of one thing,
  as they are in Word, and the tick decides when they disagree.
  Both flags were being *lost*: the model kept neither, and the writer put a
  constant in the file — every document with overlap turned off came back with
  it turned on.
  *Not done:* **Lock anchor** is kept and written now, so it survives a
  document, but it has nothing here to stop. See **C47**.

- [x] **C47. Where a dragged drawing belongs afterwards.** Word re-anchors a
  drawing to the paragraph it is dropped nearest, unless Lock anchor says
  otherwise — which is what makes a picture dragged down three pages stay there
  when the text above it grows, and what the lock is for. Dragging moved a
  drawing by a distance and left its anchor where it was, so a drawing dragged
  far from its paragraph was still tied to it and came back on the next edit.
  *Done:* both halves. When a drag of the body ends, each drawing it moved is
  re-hung on the paragraph its own top is nearest, and the distance it hangs at
  is worked out afresh from where it is drawn — so the paragraph changes
  underneath it and nothing else does. A locked anchor is left alone, which is
  the whole of what the lock does and what **C46** had nothing to stop.
  The drawing's element is moved rather than rebuilt, as a table's is: out of
  the run that held it, into a run of its own at the end of the paragraph it
  landed by, so everything about it this program does not understand goes with
  it. The run it came out of goes too when it held nothing else.
  *Not done:* Word will re-hang a drawing on a paragraph inside a table cell,
  and a drawing dropped over one here keeps the paragraph it had: a drawing in a
  cell is a different thing from a drawing beside it, and moving it there is not
  the same operation. The distance is measured from a frame of "paragraph" or
  "line"; from the page or the margin the distance says the same thing wherever
  the drawing hangs from, so nothing is recomputed.

- [x] **C42. A double click takes the space after the word.** Found while
  proving **C41**: Word's double click selects the word *and* the space that
  follows it, which is what makes deleting a word leave one space rather than
  two. Ours took the word alone. The other half of the same rule is Word's
  "smart cut and paste", which puts a space back when a word is pasted between
  two others — so the two belong together and are one item rather than a
  one-line change to `word_around`.
  *Done:* both halves, and they answer to one another. A double click takes the
  word and the spaces after it, so a word cut that way leaves one space behind
  and arrives somewhere else with the space it needs; a drag that began on a
  double click goes on taking words the same way. A double click in the space
  between two words now takes that space, and one on punctuation takes the run
  of marks: a double click that selected nothing at all looked broken, and Word
  takes the run of whatever kind of character is under it.
  And a paste that lands against a letter is spaced from it — in front, behind,
  or both — unless what was copied already ends or begins with a space, or is
  whole paragraphs rather than words. The paste and the spaces it asked for are
  one thing to take back, which is what the paste options need: choosing
  another one takes the paste back and puts it down again.
  *Not done:* Word's option to switch smart cut and paste off, which is in
  Options > Advanced behind a Settings button of its own, along with adjusting
  paragraph spacing on paste and adjusting table formatting on paste.

The items from **C48** on come from the reviews of 29 September 2026 — of
the interface (findings U01–U48), of tables (T01–T60) and of the model and
the files (R01–R22 and #1–#14) — which are kept outside the repository, in
`docs/.reviews/`, with the pictures they were made from. Their order of
work is in *The order of the work* at the end.

- [ ] **C48. Undo that tells the truth.** Two ways the history lies. After a
  save, undo restores the `modified` flag the step recorded, which is the
  old save point's: the tree says A, the flag says nothing changed, and the
  next save writes the package that still holds AB. And a step recorded in
  a header is put back into whichever tree is loaded: undo after leaving
  the header switches the part's name without loading its XML, so a
  paragraph of the header lands in the body, and the redo snapshot is taken
  from the wrong part too. Tie "saved" to a revision of the history rather
  than to a flag; load the part a step belongs to before putting it back.
  Reviews R12, R13.
- [ ] **C49. Merging and splitting cells without losing anything.** Merge
  Cells drops the text of every cell but the first when merging sideways,
  hides it in `vMerge` continuations when merging down, and leaves the caret
  in the next cell — or, when the whole table is merged, outside the table.
  Split Cells has no dialog, splits only what was merged, and after a
  whole-table merge leaves the lower rows' `vMerge` behind, a ragged table.
  Tab walks into the invisible continuations. Word keeps every cell's
  paragraphs in the merged cell, puts the caret there, splits any cell by a
  count of rows and columns, and Tab skips what is not drawn. Reviews T30,
  T31, T21, T60.
- [ ] **C50. Shift+arrows in a table select cells.** They extend a text
  selection through every cell between, in file order; the selection is
  drawn as cells but acted on as text, so Delete, typing, Enter and paste
  after Shift+Down empty the intermediate cells of their only paragraph — a
  file Word calls damaged — and Bold lands on cells the drawing never
  showed. Go through `select_cells`, as Shift+click does; refuse a text
  range that crosses a cell boundary in `remove_range` and `press_enter`;
  and hold to it that a cell always has a paragraph. Review T14.
- [x] **C51. Outline view draws.** Switching to Outline hangs the release
  build and panics the debug one — "attempt to add with overflow" in
  `fill_rect`, from a page given a height of `f32::MAX / 4` and a rectangle
  drawn the whole of it. Find the rectangle, clip it to the window, and add
  the test that was missing: every view draws. Review U32.
  *Done:* the rectangle was the paper itself, in `draw_pages`, not the level
  bar. The outline is laid out on the web's sheet with no end, and the
  engine ends that sheet where the text ends only when told it is the web's;
  the outline never said so, and everything that measured its page measured
  a quarter of the largest float — the paper overflowed, the side ruler
  walked to `i32::MAX` marks (the release hang: its count wrapped round and
  walked for ever) and the scroll bar ran the length of it. The outline is
  now finished as one sheet as the web page is: `View::is_one_sheet` in
  `editor/views.rs`, which `relayout` in `editor/mod.rs` hands the engine.
  What drew it is clipped as well: `Canvas::fill_rect` in
  `wp-raster/src/canvas.rs` brings a rectangle's edges within reach before
  it adds them up, so one running past what an i32 holds is clipped rather
  than overflowing, and the side ruler in `chrome/rulers.rs` walks only the
  marks beside the window.
  *Proven by:* `every_view_draws`, which chooses each view in turn and
  draws the window, and panicked on Outline before;
  `an_outline_is_one_sheet_as_long_as_its_text`;
  `a_page_far_longer_than_the_window_is_marked_only_beside_it`, which
  overflowed before; and in wp-raster
  `a_rectangle_whose_end_is_past_what_an_i32_holds_is_clipped`, plain,
  turned right to left and scaled, which overflowed before.
  *Not done, and named here:* being finished as the web's, the outline now
  shows the footnotes and endnotes after its text, under a rule, as the web
  page does; Word's outline shows none. The sheet is as wide as the window
  but set a desk's gap in from its left, so it runs that far past the right
  edge and the view scrolls sideways, as it did before. Word offers no ruler
  in outline view, and both are still drawn here. Only `fill_rect` of the
  canvas was hardened: `set_clip`, `draw_mask`, `blend`, `clamped` and the
  picture copies still add a size to a position in i32, which is safe for
  the sizes they are given and not for a position near `i32::MAX`. And
  what is drawn is not Word's outline: Word's is a view of its own, with an
  Outlining tab, ⊕ and ∘ marks beside headings and body text, and levels
  folded and dragged by them; this is the web's sheet showing the headings
  and their text to the chosen depth (the review's picture
  `word-outline-dark.png` is what it should come to look like).
- [ ] **C52. The pointer is an arrow over the furniture.** Over a dialog's
  body and its OK button the pointer is an I-beam, and over the desk beside
  the page too, because `cursor()` has no branch for the dialog or the
  panes and falls through to the nearest line of text. Over tabs, buttons,
  menus, galleries, the mini toolbar and the navigation pane it is a hand,
  which is a browser's answer and not Word's: Word shows an arrow over all
  of them, and a hand only over a link under Ctrl. Review U07.
- [ ] **C53. Dialogs that fit, read and stay put.** The Font dialog's sample
  is drawn in the document's automatic colour, black, on the dark theme's
  field, so it cannot be read; the Paragraph dialog is 716 px tall in a
  900 px window and covers the ribbon (Word's is 552); a dialog is not
  centred again when the window is resized, so its foot goes under the
  status bar; one dialog shows "0,00 cm" and "0.00" in the same column; the
  Style Inspector ignores the measurement unit; and the dialog's own
  heading promises a dimmed document that is not dimmed. Review U09. Word's
  own answer to the first — light dialogs in every theme — is **C76**'s.
- [ ] **C54. The keys Word has.** Ctrl+= zooms in; in Word it is subscript,
  and Ctrl+Shift+= superscript. Missing altogether: Ctrl+G and F5 (Go To,
  which **C59** builds), F4 (repeat), F8 (extend selection), Shift+F3
  (change case), Ctrl+F1 (collapse the ribbon, **C73**), F6 (move between
  panes), F10 (key tips), Ctrl+Q, Ctrl+1/2/5, Ctrl+[ and Ctrl+],
  Ctrl+Shift+< and >, Ctrl+W. Reviews U33, U34.
- [ ] **C55. The dialog launchers, as Word draws them and where.** Word's is
  a 16 px square with a border and an arrow pointing down and right, in the
  text colour; ours is a 9 px corner in the dim colour with the arrow
  pointing up and right, and easy to miss. Word has one on Clipboard, Page
  Setup, Footnotes and Tracking as well, opening the Clipboard pane, the
  Page Setup dialog (which exists), the Footnote and Endnote dialog and the
  Track Changes options; the first and the third do not exist here and are
  part of this item. Reviews U01, U02.
- [ ] **C56. The page in the dark theme, and the theme's colours.** In Word's
  black theme the page is dark, the same tone as the desk with a grey
  border, and View → Switch Modes turns the page white while the chrome
  stays black; Options has "Disable dark mode" to keep it white. Here the
  page is a lighter grey than the desk, and Switch Modes switches the whole
  theme. So: a flag of its own for the dark page, remembered; Switch Modes
  toggles that and the automatic text colour; the option in General. And
  the tones measured on Word's window: title bar, ribbon, desk, status bar,
  the pressed button (grey with a border, not the accent), the Backstage
  rail (grey with a blue mark, not solid blue), panes and menus. The
  comment at the head of `theme.rs` says Word keeps a white page in a dark
  window; Word offers both, and the comment goes with the code. Reviews
  U04, U05, U31 (the rail; the rest of U31 is **C8**'s and **J6**'s).
- [ ] **C57. A status bar that answers.** Word's is buttons: the page goes to
  the navigation pane's Pages, the word count to Word Count, the spelling
  icon to the Editor, the language to the Language dialog, the zoom figure
  to the Zoom dialog; three view buttons; "12 of 208 words" while text is
  selected; a right-click menu of twenty-five lines with the values beside
  them. Here the left half is text that ignores a click, and the menu has
  seven lines. Two things to find on the way: the same file counts 208
  words in Word and 236 here, and the language of a document that names
  none is English (United States) there and English (United Kingdom) here.
  Reviews U08, U39, U46.
- [ ] **C58. The style gallery as Word draws it.** Light tiles in every
  theme with "AaBbCcDc" set in the style's own font, size, colour and
  weight, the name under it with ¶ for a paragraph style, the current one
  framed rather than filled, six tiles at 1400 px with up, down and More,
  and More opening three rows and Create a Style, Clear Formatting, Apply
  Styles. And the live preview: the paragraph shown in a style while the
  pointer rests on its tile, and put back when it leaves — for this gallery
  and, in the same stroke, for fonts, colours and themes. Reviews U03, U24.
- [ ] **C59. Find and Replace, and Go To.** Word's Ctrl+F opens the
  navigation pane on its results; Ctrl+H the Find and Replace dialog with
  its three tabs — Find, Replace, Go To — and More. Here both go to one bar
  under the ribbon, and Go To does not exist by any name. Reviews U37, U45.
- [ ] **C60. "Tell me what you want to do" does something.** The box is
  drawn and takes a text pointer, and a click on it is ignored. Word's
  searches the commands and drops a list of them. Search the ribbon's
  groups and the menus behind the buttons, and run what is chosen. Review
  U25.
- [ ] **C61. A table pasted is a table.** A table from Excel or Word comes in
  through Rich Text or HTML as a `Block::Table`, and pasting drops every
  block that is not a paragraph and says "Nothing was pasted" — not even the
  text arrives unless Keep Text Only is chosen. Copying cells gives one
  paragraph a cell, and dragging cells moves text. Paste a table as a
  table, nested when the caret is in a cell; copy cells as cells; then
  Word's choices when pasting into a table — nest, merge, new rows,
  overwrite cells. Reviews T44, T52, T54; **H2**'s entry, which said tables
  come as their paragraphs, is corrected there.
- [ ] **C62. A row taller than what is left of the page, and the header
  again.** A row is placed whole on this page or whole on the next, so a
  row taller than a page goes off the bottom of the paper and is never
  seen; Allow row to break across pages is read and written and not laid
  out. And Repeat Header Rows writes `w:tblHeader`, which nothing in the
  layout reads, so the header never comes back — **C16** claimed otherwise
  and is corrected. Reviews T41, T42.
- [ ] **C63. The lines between cells.** A cell's own right border is written
  to the file and never drawn: `draw_row_borders` draws top, bottom and
  start only, so the Border Painter's vertical stroke on the right of a
  cell is invisible. Where two cells' borders meet, the stronger does not
  win as Word's rules say; the painter paints one edge a press rather than
  along a drag; and a vertically merged cell is laid out as several
  rectangles, so its vertical alignment, its selection and its caret are
  those of the pieces. Reviews T34, T46, T47.
- [ ] **C64. Backspace, Ctrl+Shift+Enter and Enter, as Word has them in a
  table.** Backspace on selected rows, columns or a whole table deletes
  them (Delete empties them); Ctrl+Shift+Enter splits the table before the
  row, and Split Table is on the Layout tab; Enter at the start of the
  first cell of a table that opens the document puts a paragraph above it.
  Here Backspace empties, Ctrl+Shift+Enter puts a column break in the cell,
  and Enter splits the cell's paragraph. Reviews T23, T27, T22.
- [ ] **C65. One step back for a dialog, and borders on the cells.** OK in
  Table Properties writes eleven undo steps and a table style two; and
  Borders… in that dialog, and Home → Borders with the caret in a cell, put
  borders on the paragraph rather than on the cells. Reviews T38, T39.
- [ ] **C66. Selecting and inserting by the count.** Dragging along the
  selection bar takes one column and along the top one row, because the
  anchor is a cell and not a whole row; inserting above with three rows
  selected inserts one; a width typed in Cell Size widens one cell and
  cannot narrow it, where Word sets the column; and the four pixels inside
  a cell's left edge are the column line, so a press there starts a drag
  and no press selects the cell. Reviews T06, T32, T35, T05.
- [ ] **C67. A table that reaches past the margin, and the drags with
  modifiers.** The last line and the size handle cannot take a table past
  the right margin: the other columns shrink instead. Shift, Ctrl and
  Ctrl+Shift on a border drag do nothing (only the column beside; the
  columns to the right in proportion; the columns to the right equal), and
  Alt shows nothing on the ruler; the size handle changes only widths, and
  Shift does not keep the proportions; a border cannot be dragged in a row
  with a merged cell, or for the selected cells alone. Reviews T17, T11,
  T16, T12, T13.
- [ ] **C68. The Table menu, and the commands the Layout tab is short of.**
  Insert → Table is a grid alone: no live preview on the page, no Insert
  Table dialog with AutoFit and defaults, no Draw Table from outside a
  table, no Convert Text to Table, no Quick Tables. Convert to Text offers
  no separator. Delete has no Delete Cells… with its shift, there is no
  Insert Cells…, no Distribute Rows. The context menu is five flat lines
  where Word's has Insert ▸, Delete Cells…, Split Cells…, Border Styles ▸,
  Text Direction…, Table Properties…, and the mini toolbar has no Insert and
  Delete. A table goes after the paragraph rather than at the caret,
  leaving an empty paragraph above it, and the ribbon stays on Home where
  Word opens Table Design. Reviews T01, T03, T33, T37, T02, T36.
- [ ] **C69. The table style gallery, and Borders and Shading.** Five names
  in a list where Word has a hundred tiles with pictures and a live
  preview, Modify, New, Clear; no Shading on the Design tab; Borders with
  three choices where Word has fourteen; no Border Styles gallery with pen
  style, weight and colour; no Borders and Shading dialog with its preview
  and Apply to. Reviews T29, T43.
- [ ] **C70. What the rulers know of a table, and a table that floats.** The
  rulers show no column and row markers; the move handle cannot make a
  table float or drop it into a cell; a floating table from Word
  (`w:tblpPr`) is laid out in the flow; Table Properties has no wrapping,
  Positioning, unit choice, Column tab, Previous and Next, or cell Options.
  Reviews T19, T18, T48, T40.
- [ ] **C71. The small things in tables.** The pointer over the handles and
  the column bar; ¤ for the cell mark and the end-of-row mark when marks
  show; a picture in a cell limited to the cell's width, not the page's;
  Alt+Shift+Up and Down moving rows, Alt+Home/End/PgUp/PgDn, Alt+5 on the
  keypad selecting the table, Ctrl+click adding a cell. Reviews T10, T50,
  T56, T25, T08, T07.
- [ ] **C72. Icons in Office's colours, and the arrows on the buttons.** The
  icons are Fluent's, monochrome; Word's are coloured, and Paste, Format
  Painter, Highlight, Shapes and Chart are not known at a glance. Word's
  own drawings are not licensed and are not copied: what can be done is
  colour where Word has it (Paste's board, the painter's bristles, the
  lists' and indents' blue, Shading's band), the menu arrow on Underline,
  Text Effects, Highlight, Font Colour, Shading, Borders and Pictures, the
  shapes where ours differ (Grow and Shrink Font, Change Case,
  Strikethrough, Sort), and the buttons that are not on Word's ribbon at
  all — Asian Layout, Remove Link, Format Page Numbers — taken off the top
  level. And the large buttons are 60–70 px wide against Word's 42–55,
  which is why the Insert tab's Text and Symbols groups are squeezed at
  1400 px where Word's are not. Reviews U06, U27.
- [ ] **C73. The ribbon's shape.** Picture Format and Shape Format contextual
  tabs when a drawing is chosen (Word opens the tab on insert), with the
  coloured heading Word gives contextual tabs; the ribbon collapsing — the
  chevron in its corner, Ctrl+F1, a double click on a tab; key tips as
  yellow badges under the tab names rather than grey ones over them, with
  digits over the Quick Access Toolbar; Undo's drop-down list of steps and
  the toolbar's customize arrow; Draw and Developer hidden by default as
  Word hides them. Reviews U26, U29, U28, U30.
- [ ] **C74. The pop-ups as Word has them.** The mini toolbar in two rows,
  in Word's order, placed above the selection rather than on it; the
  context menu with Paragraph… and Synonyms ▸ and without Styles, Bullets
  and Numbering; the bullet library as a grid of glyphs with None, Change
  List Level and Define New Bullet; the line spacing menu with its values
  ticked and Line Spacing Options…; the Layout Options button beside a
  chosen picture and the anchor mark; Paste Options as tiles with Set
  Default Paste…; ScreenTips light, with a bold title and a line of
  description. Reviews U17, U18, U21, U22, U23, U19, U20.
- [ ] **C75. The pointer's shapes, and three clicks.** Word's pointers that
  are not here: the mirrored arrow in the selection bar, the four-way move
  over a drawing and a table's handle, the diagonals over corner handles,
  the drag-text pointer, the hand over a link under Ctrl, Click-and-Type's
  alignment marks; ours has five shapes. A third click in the selection
  bar selects the document, not the paragraph. A double click in the
  header area did not open the header in a test and the plain wheel did
  not scroll — both to be verified first — and while a header is edited
  the body is not drawn at all where Word greys it. Reviews U10, U11, U12,
  U13, U15, U16.
- [ ] **C76. Read Mode, Page Setup and the dialogs' windows.** Read Mode is
  Word's two columns at 140 % with arrows either side and its own File,
  Tools and View, no caret; Page Setup has Margins, Paper and Layout with
  Gutter, orientation pictures, Multiple pages, a preview and Apply to; the
  Font dialog lists fonts, styles and sizes under the boxes and has Text
  Effects…; and dialogs are windows that can be dragged, light in every
  theme, with a system frame. Reviews U36, U44, U42, U41.
- [ ] **C77. Panes, rulers and contrast.** The navigation pane closed by
  default, three tabs, an 18 pt heading with ▾ and ✕ and a search box with
  its own menu; rulers off by default and Alt showing the margins' sizes
  during a drag; Windows' high-contrast setting. Reviews U38, U40, U47.

## D — Pictures and drawings

- [x] **D1. BMP, in its several forms.** The first of the four formats the
  original **D1** asked for, split out because each is a decoder of its own and
  because only some of them can be proved the way that item demanded. That proof
  is the point: every fixture is read back through GDI+, and the manifest says
  what GDI+ found rather than what this program computed. BMP is the format that
  proof is most thorough for, which is why it went first — see **D7**, **D8**
  and **D9** for the rest.
  *Done:* all five headers, from the twelve-byte one that says only the size and
  the depth to the hundred-and-eight-byte one that states its own colour masks;
  one, four, eight, sixteen, twenty-four and thirty-two bits to the pixel;
  palettes of three bytes an entry and of four; runs of one colour at eight bits
  and at four, including the jump, the absolute run and the rule that what a run
  never reaches is left as it was; colour masks of any width, with the channels
  widened by repeating their own top bits, which is what every other reader does
  and is the difference between agreeing with Windows and being one off; rows
  written upwards and rows written downwards; and a bitmap whose pixels are a
  whole PNG or a whole JPEG, which is handed to that decoder.
  Eight fixtures, seven of them assembled byte by byte because no encoder to
  hand writes them, every one read back through GDI+ and matched exactly — no
  tolerance at all, because nothing here is lossy.
  *Not done:* two bits to the pixel, which Windows CE wrote and nothing else;
  the colour space and gamma the fourth and fifth headers can carry, which
  Word ignores as well; and `BI_CMYK`, which is a printer's format and never
  appears in a document.

- [x] **D7. GIF.** The palette, LZW, transparency, interlacing, and the first
  frame of an animation — which is what Word draws for one.
  *Done:* both versions of the header; the global palette and a frame's own;
  LZW in the variant the format uses — codes packed from the bottom of each byte
  up, growing a bit wider as the table fills, running straight across the join
  between one sub-block and the next, and including the one case the
  specification names, where a code arrives for a string that is about to be
  added; interlacing, whose four passes are put back in order; the colour a file
  says is not to be drawn, which keeps its colour and loses only its alpha —
  throwing the colour away would come back as a dark halo the moment the picture
  were scaled; a frame smaller than the screen or offset within it, with what it
  does not cover left as nothing at all so the page shows through; and an
  animation, of which the first frame is the picture.
  Four fixtures. One is GDI+'s own GIF, which is what proves the compressor's
  side of LZW — a real stream with the codes widening, not the short one a test
  can write by hand. The other three are assembled, and their streams are real
  for all that: a clear code every second pixel keeps the table from filling, so
  no compressor is needed to write one a decoder cannot tell from compressed.
  *Not done:* the frames after the first, and with them the disposal methods
  that say what each leaves behind for the next. Word draws one frame and so
  does this; the rest are carried through in the file and saved back unchanged.

- [x] **D8. JPEG, the rest of it.** Baseline is done. Progressive — the
  coefficients spread over several scans, by spectral selection and by
  successive approximation — and the four-component pictures Adobe writes, where
  the colour is CMYK or YCCK and stored inverted.
  *Done:* the decoder was turned inside out first. It used to transform each
  block as it read it, which a progressive picture forbids: the first scans
  carry the top bits of the low frequencies and the scans after them add bands
  and bits, so nothing can be transformed until the last of them is read. Now
  both kinds gather coefficients and are transformed in one pass at the end,
  and they differ only in how the coefficients are filled in.
  All four kinds of progressive scan: the first bits of the first coefficient
  and one more bit of it, and the first bits of a band above it and one more bit
  of that. The last is unlike the other three — every coefficient already sent
  needs a bit whether or not the scan has anything new to say about it, and
  those bits are written in the gaps between the ones that do.
  Colour: three components are brightness and two differences unless Adobe's
  marker says they are red, green and blue, or the components name themselves
  after the three colours; four are ink, and Adobe writes ink inverted, which is
  why they multiply by the black rather than being subtracted from it — a reader
  that does not know shows a scanned page as a photographic negative.
  The fixture is assembled by the tool, because GDI+ writes baseline only. It
  reads progressive perfectly well, so the proof still comes from outside even
  though the file did not.
  *Not done:* arithmetic coding, which almost nothing produces, and the lossless
  and hierarchical modes, which nothing does. All three say so.

- [x] **D9. TIFF.** The tag directory, strips and tiles, the compressions a
  document carries, the photometric interpretations, the predictors, and planar
  configuration. The largest of the four by a distance, which is why it was
  last — and why the two fax codings came out of it into **D10**: they are a
  coding of their own, they belong to scanned pages, and nothing else in TIFF
  depends on them.
  *Done:* the directory, either way round the numbers are written; strips and
  tiles, which are the same thing at two sizes; one channel to a block or all of
  them together; one, two, four, eight and sixteen bits a sample; no
  compression, PackBits, LZW in the variant TIFF uses — packed the other way up
  from GIF's and growing a code early — and deflate under both of the numbers
  the format has given it; grey either way round, colour, a palette, and ink;
  the horizontal predictor; and a sample past the ones the colour needs, read as
  transparency when the file says that is what it is.
  Seven fixtures. Five are held to GDI+'s own reading. The other two are the
  forms GDI+ will not read back: a picture cut into **tiles**, which its codec
  does not do at all, and one with a **predictor**, which it hands back
  undone — each is held instead to the same picture in a form GDI+ does read, so
  a decoder that put the tiles in the wrong order or ignored the predictor would
  still be caught.
  *Not done:* the pages after the first — a TIFF may hold a whole scanned
  document, and a document shows one picture; `YCbCr` and the subsampling that
  goes with it, which is JPEG's colour model carried in a TIFF and is rare
  outside scanners; and strips that are whole JPEGs.

- [x] **D10. The fax codings.** CCITT modified Huffman, Group 3 in its one- and
  two-dimensional forms, and Group 4 — the codings a scanned page is written in,
  inside a TIFF and nowhere else a document carries.
  *Done:* both code tables in full — the sixty-four exact run lengths and the
  make-up codes for the rest, white and black entirely different because black
  runs are short and frequent and the short codes are spent on them; the
  make-up codes past 1728 that the wider papers added; the end-of-row code and
  whatever padding precedes it; and, for the two-dimensional forms, all seven
  modes.
  A row is decoded into **the places its colour changes**, and the row after it
  is read against that list — which is what the two-dimensional forms are
  entirely about. The awkward part is finding, for each step, the next place the
  row above changes *to the colour this row is not*: getting that parity the
  wrong way round shifts every run by one and is the classic way to write a
  decoder that works on blank pages and nothing else.
  Five points apiece would not catch much in a coding like this, so the three
  are held to each other as well: the same scanned page, a thousand pixels of
  it, written uncompressed and in both codings, must come back identical.
  *Not done:* Group 3 with the rows written against each other is proved by
  construction rather than against GDI+, which writes the one-dimensional form
  only; the uncompressed mode both groups allow as an escape, which is for lines
  so noisy the coding would make them longer; and `FillOrder`, which writes the
  bits of each byte the other way round and which nothing this side of a fax
  machine produces.
- [x] **D2. WMF and EMF: the shapes.** The metafile formats Word documents still
  carry. Not pictures but recordings of how one was drawn — take this pen, draw
  a line here, fill this polygon — which is why a document that held a diagram
  held one: it draws at any size, and in 1990 that mattered more than anything.
  *Done:* both record interpreters, over one set of state and one set of drawing
  — a pen, a brush, where the last line ended, the mappings, and which of the two
  fill rules is in force. Lines, polylines, polygons, several polygons at once,
  rectangles, rounded rectangles and ellipses; pens and brushes, including the
  two styles that mean *draw nothing* and the handful of objects the system
  provides rather than the file; paths collected between the records that begin
  and end one, and then filled, stroked, or both; and, for the newer format, the
  world transform in all four of the ways a record can change it.
  Three things were decided rather than transcribed. A metafile is **played back
  into pixels** at the size it says it is, because everything above this draws
  pictures and a picture is pixels — one road through the program rather than
  two, at the price of a metafile scaled up afterwards being no sharper than the
  canvas it was played onto. A **pen's width is in logical units** and is brought
  into pixels where it is drawn with, not where it is made: a file that draws at
  sixteen times the size with a pen sixteen times as wide means a line of the
  same thickness, and missing that blacks the picture out — which is exactly what
  it did until it was found. And a coordinate **names a pixel** rather than the
  corner between four, so everything is drawn half a pixel along; without that a
  line one pixel wide straddles two rows and comes out grey in both.
  The rasterizer gained the **even-odd rule** for this. A font outline draws a
  hole by winding the inner contour the other way and expects the two to cancel;
  a metafile expects every second layer to be a hole whichever way it was wound.
  Both answers are wanted and neither is wrong.
  The fixture is the one GDI+ is at both ends of: it recorded the metafile, and
  what the manifest holds is GDI+ playing that same file back. The tolerance is
  not nothing, as it is for the pixel formats — two rasterizers do not put the
  edge of a shape in quite the same place — so the points sampled are well inside
  a shape or well outside every one.
  *Not done:* text, which is its own piece of work and is **D11**; the pictures a
  metafile can carry inside itself; clipping regions; saving and restoring the
  state, which the newer files do around every drawing and which matters only
  where something is left changed afterwards; and the hatched brushes and dashed
  pens, which are drawn solid — a dashed line drawn solid is a line, where one
  drawn as nothing is a shape with a piece missing.

- [x] **D11. Text in a metafile.** The records that draw words, the fonts they
  name and how those are matched against the fonts actually present, the
  alignment, and the escapement that turns a label on its side.
  *Done when:* a metafile with words in it draws them where an independent
  player puts them.
  A picture decoder cannot know what fonts a machine has, and it should not:
  that is the business of whatever opened the document, which has a list of them
  already and a rule for what to fall back on. So the player **asks** — the
  caller hands in somewhere to get letter shapes from, and the one that already
  answers which face a paragraph is set in answers for a metafile too. A caller
  with nothing to offer gets the picture with its shapes and without its words,
  which is what `decode` does and says.
  Making a font and using one are two things, in both formats: a file that made
  a face and never took it up draws its words in whatever was in hand before,
  and the test that first said "the words were not drawn" was right — it had
  made the face and not selected it.
  The rest is the file's own reckoning. A height stated as a positive number is
  the whole line and not the letter. The point a record gives may be the left
  end of the words, the right or the middle, and their top, their bottom or the
  line they stand on. The escapement is in tenths of a degree and anticlockwise,
  which is the other way round from a canvas, so the sign changes once and is
  commented where it does.
  A face that cannot draw every letter of a run draws none of it. Half a word in
  one face and half in another is worse than a gap where the word was, and a gap
  is what a reader can see is a gap.
- [x] **D3. The preset shape geometries: the rectangles and the basic shapes.**
  A document does not carry the outline of a star; it carries the word `star5`
  and a box to fit it in, and every program that opens the document is expected
  to know what that means. There are about 180 of them. This is the first two
  sections of Word's own gallery, which is what a document is most likely to
  hold — the other sections are **D15** to **D18**, and the rest of what the old
  **D3** asked for is **D12** to **D14**.
  *Done:* twenty-eight new shapes on top of the twelve there were. All nine
  rectangles, which are one shape with its four corners treated three ways —
  squared, cut straight across, or taken round — so they are one helper and nine
  arrangements. The rest of the regular polygons up to twelve sides. The shapes
  of straight lines: the trapezoid, the parallelogram, the cross, the L and the
  half frame. And the ones made of arcs: the pie, the chord, the arc, the donut,
  the "no" symbol, the block arc, the can, the teardrop, the frame, the plaque,
  the moon and the heart.
  The three names a shape has — the one in the file, the one on the screen, and
  the shape itself — now come out of **one table**. They were three matches, and
  a shape added to two of them is a shape that draws and cannot be picked, or is
  picked and draws as a rectangle. Neither says so, which is why it is one table
  and a test that no two share a name.
  The tests draw each shape and look at it: every closed one has ink in its
  middle, the ring-shaped few have a hole there, a snipped corner is gone and a
  rounded one is not, and no shape reaches outside the box it was given. That
  last one caught the heart and the moon, both of which did.
  *Not done:* Word's gallery groups its shapes into sections and this program
  offers one flat list of them. The adjustments were not read either when this
  was written; they are **D20** and **D21** now, and they are.

- [x] **D12. Gradients and patterns.** `a:gradFill` in its three kinds — linear,
  radial and along a path — with the stops and the angle; and `a:pattFill`, the
  named hatchings.
  *Done:* a fill stopped being a colour. It was six hex digits or nothing, which
  could carry none of this — and three quarters of the shapes in a real document
  are not one colour: Word's own shape styles are gradients and its charts hatch
  their bars. So the model holds a **fill** now, of which one colour is one kind,
  and everything that reads or writes a shape goes through it.
  The rasterizer gained the other half of it: a path can be filled by a **rule
  that says what colour a place is** rather than by one colour. A gradient, a
  hatching and a picture used as a fill are all the same thing to it, and the
  rule is asked once per pixel the shape covers.
  Gradients run at any angle, with any number of stops, mixed in proportion
  between them — and outwards from the middle in rings or in rectangles. The
  angle is handled so that nought is the corner the line first meets and one is
  the last, which is what makes a gradient at forty-five degrees run corner to
  corner rather than stopping halfway.
  The hatchings are eight pixels by eight, in pixels of the page rather than
  fractions of the shape, which is what makes a hatched shape look the same at
  any size. The percentages are worked out from an ordered dither — the same
  sixty-four thresholds at sixteen weights — and the thirty-odd lines, crosses,
  checks and diamonds are written down.
  *Not done:* the picture and texture fills, which need the picture on the
  placed shape and a decision about tiling; the handful of named hatchings that
  are neither a percentage nor a line — they are drawn as the half-and-half
  dither, which is visibly a hatching of about the right weight rather than a
  shape pretending to be solid; the gradient's `scaled` flag and its tile
  rectangle; and a gradient exported to PDF, which is written as the colour at
  its middle because that is all a PDF content stream of this program's can
  carry so far.

- [x] **D13. The shape effects.** `a:effectLst`: the outer and inner shadow, the
  glow, the soft edge and the reflection, drawn as effects on a shape rather
  than the approximation the letters use. A real blur is the piece of work
  underneath all of them.
  *Done when:* a shape with each effect is drawn as Word draws it.
  The blur came first, because every one of them is made of it. Three passes of
  a box blur, which is what everything that blurs quickly does: one box on its
  own looks like a box, and three of them in a row are close enough to a
  Gaussian that the difference cannot be seen — and each pass costs one addition
  and one subtraction per pixel however wide the blur is, which is what makes a
  wide blur affordable at all.
  Each effect is then a few words over the shape's own coverage. The shadow is
  the coverage moved and blurred. The glow is the coverage blurred and then made
  stronger, because a plain blur is faint everywhere and a glow is solid against
  the shape. The inner shadow is the coverage turned inside out, moved, blurred
  and held back to the shape, which is the shadow of everything outside it laid
  within it. The soft edge is the shape drawn *through* its own blurred
  coverage. The reflection is the shape again, mirrored about the bottom of what
  was drawn and fading downwards.
  Mirrored about the bottom of what was **drawn**, not of the shape's own box:
  the path has the page's corner and the scroll in it already, and the first
  attempt reflected the shape onto the paragraph above it.
  The values are kept as the format states them — English metric units, sixtieths
  of a degree, hundred-thousandths — and turned into pixels only where drawing
  happens, because how big a pixel is depends on the zoom and a document read at
  one zoom and saved at another must not come out with different numbers in it.
  A shape in a run is boxed now. It carries everything a shape can carry and
  every other thing a run holds is a few words.
  *Not done:* a PDF fills in one colour, so what is written there is the shadow
  without its blur — the shape again, offset, in the shadow's colour. Drawing a
  blurred effect into a PDF means writing a picture of it, which is the
  F-series work on what a PDF can carry.

- [x] **D14. Three dimensions.** `a:scene3d` and `a:sp3d`: the bevels, the
  extrusion and its depth, the material, the lighting and the camera.
  *Done when:* a shape with a bevel and a depth is drawn with them.
  Two elements and not one, because one of them belongs to the shape and the
  other to the room it stands in. How thick a shape is and what its edge is
  rolled to are its own; where the scene is looked at from and lit from is
  shared by everything in it.
  The depth is the face again and again, stepped back the way the scene is
  turned, with the face laid over them. The sides of a solid seen flat on *are*
  the face swept along the depth, and sweeping a shape of curves and corners
  into a band means working out its silhouette from the direction of the sweep;
  stepping it back a pixel at a time fills the same area, and a pixel at a time
  because anything coarser leaves the sides striped.
  The bevel is the shape's own outline band at the width of the bevel, lit on
  one side and shaded on the other. Which half catches the light is worked out
  by moving the shape: shift it away from the light, and the edge it leaves
  uncovered is the edge the light falls on. What it is made of settles how hard
  that light is — metal takes a sharp edge and matte hardly shows one.
  A depth with no turn at all is a depth nobody can see, because it goes
  straight back. Word draws it that way too, and the bevel is what shows
  instead.
  *Not done:* the face itself is not turned. A shape rotated right round in Word
  is a shape seen at an angle, and drawing that is drawing a different shape —
  the outline of a solid seen from a corner — rather than the same one with
  something added. The camera is read, kept and written back, and what it is
  used for is the direction the depth goes in.

- [x] **D15. Block arrows.** The twenty-eight arrows of Word's gallery: the four
  straight ones, the bent and the curved, the striped and the notched, the
  chevron and the pentagon, and the circular arrow.
  *Done when:* each is drawn, and each is drawn the way round its name says.
  Each takes its head length and its shaft thickness from the shorter side of
  its box, as Word does, so two heads that cannot both fit meet in the middle
  and the arrow reads as a diamond — which is what Word draws too.

- [x] **D16. Flowchart shapes.** The twenty-eight boxes a flowchart is drawn
  with: the process, the decision, the terminator, the document, the stored
  data, and the rest.
  *Done when:* each is drawn.
  Eight of them have a line drawn inside the shape rather than round it — the
  two down a predefined process, the cross through an "or", the near side of a
  magnetic disk's lid. Those are drawn with the shape's outline and are no part
  of its area, so they do not change the fill and the text wrapping round the
  shape does not see them. Without them a predefined process is a process and
  a sort is a decision: two shapes under one drawing.

- [x] **D17. Stars and banners.** Word's own section: the two explosions, the
  ten stars from four points to thirty-two, the four ribbons, the two scrolls
  and the two waves.
  *Done when:* each is drawn.
  The callouts came out of this item and are **D19** now. They were in it
  because Word shows them next to each other, but a callout is a different
  piece of work: its tail points where the file says it points, and nothing
  here reads that yet — which is **D20**.
  What each star is, is two numbers: how many points it has and how far in the
  dips between them go. The second is the whole difference between a spiky star
  and a blunt one, and the format has an answer for each of the ten.
  The curves here are arcs and not quadratics. Several of these have an edge
  that reaches the side of the box and comes back — the apex of a wave, the bow
  of a curved ribbon — and the control point of a curve that touched the edge
  would sit outside it. A shape is measured by the points its path names, so
  that would read as a shape drawn outside its own box.

- [x] **D18. Lines and connectors: the shapes and their ends.** The line, the
  straight connector, the four elbows and the four curved ones; and the six
  things the format can draw at either end of a line — the triangle, the
  stealth, the diamond, the oval and the open arrow, or nothing.
  *Done when:* each is drawn, and an arrowhead points the way its own leg goes.
  Until now a document's connectors drew as **rectangles**: the preset was not
  one this program knew, and an unknown preset is drawn as a box of the right
  size in the right place. A page of a flowchart came out as a page of blue
  boxes over the shapes it joined.
  These are the first shapes here that enclose nothing. A closed shape is drawn
  by the band between it and a copy of itself inset all round; a line has no
  inside for that, so it is drawn by laying a band *along* it, with a patch at
  every turn. The patch is laid down the same way the pieces are and not as a
  square of its own: by the nonzero rule a patch wound against what it sits on
  cancels it, and the line comes out dashed at every turn — which for a curve,
  whose every step is a turn, is a dashed line. The first attempt did exactly
  that.
  A box of no size draws nothing, but that is a rule about area: a line with no
  height is a level line and one with no width is upright. Only a line with
  neither draws nothing — and a connector between two shapes standing side by
  side is exactly that level line.
  An arrowhead belongs to the *line* and not to the shape, which is why the
  format puts it inside `a:ln` beside the colour and the width, and why Word's
  gallery offers "Line", "Line Arrow" and "Line Arrow Double" as three things to
  insert that all insert the same shape. Which way one points comes from the
  line itself — the first two places its path names for the head, the last two
  for the tail — so an arrow on a bent connector points along its own last leg
  rather than along the diagonal of its box.
  *Not done:* the gallery here offers the shapes and not Word's three entries
  per shape, because an entry that sets an arrowhead is an entry that carries
  more than a preset name. And the half of this item about staying joined is
  **D22**.

- [x] **D19. Callouts.** The sixteen the format has: the rectangular, rounded
  and oval bubbles and the cloud; and the twelve line callouts, which are three
  shapes each drawn four ways — with no border, with a border, with an accent
  bar down the side of the words, and with both.
  *Done when:* each is drawn, each bubble has its tail, and the elbow of a line
  callout bends where the shape says it bends.
  A callout is the one shape here drawn partly **outside** its own box. The box
  is where the words go and the tail points at what they are about, which is
  somewhere else — so the rule every other shape keeps, that nothing is drawn
  outside the box it was given, is the rule these are for breaking, and the
  tests that say it now say it about everything but a callout.
  A bubble's tail is part of the same outline as its body and not a triangle
  laid over it: a triangle laid over it shows a line across the bubble where its
  base sits, and a bubble has no line across it. The tail leaves by whichever
  side its point is beyond, and the point is where the handles say — which is
  what **D20** was for.
  What "no border" means is that the shape is drawn with no band round its edge
  at all: the leader is drawn and the words are not ringed. So a shape now
  answers whether the line it is drawn with goes round its edge, and the six
  callouts with no border in their name say no.
  The cloud is a ring of round bumps, each drawn from where it crosses the bump
  behind it to where it crosses the one ahead. Round, so that the crossing can
  be worked out exactly and used by both bumps: two arcs that only nearly meet
  leave a nick in the outline and a chord across the inside. The bumps are
  counted from how far it is round the oval they sit on, so a cloud stretched
  wide gets more bumps rather than gaps between the ones it had. The cloud on
  its own was missing from the basic shapes and is drawn now too.
  *Not done:* a line callout's leader is drawn from the first pair of handles
  taken to the edge of the box, and the bends and the point from the pairs after
  it. That is what Word's own values describe, but the format states them
  through guides this program does not have, so a callout whose handles were
  dragged a long way in Word may bend at a different place here.

- [x] **D20. The values behind the handles.** `a:avLst`: read it, keep it on
  the shape, write back untouched every value nothing moved, carry it to the
  geometry, and use it in the presets whose handle is one number with one plain
  meaning.
  *Done when:* a rounded rectangle, a star, an arrow or a pie whose handle was
  dragged in Word opens as the same shape here, and a document saved by this
  program has every handle it came with, to the number.
  A shape is now a preset **and** its handles. That is a change to what a shape
  *is*, so it runs from the reader through the model, the layout, the renderer
  and the PDF writer alike, and the value is carried in the format's own unit —
  a hundred-thousandth of whatever that handle measures — from the file to the
  screen and back, so nothing is lost rounding it into something else and out
  again.
  A handle the document never mentioned is not a handle at zero. A rounded
  rectangle with no `adj` has round corners and one with `adj` at zero has
  square ones, and a test says so.
  What obeys its handle: the rounded rectangle, all seven of the snipped and
  rounded corner shapes, the ten stars, the eight straight block arrows in both
  their shaft and their head, and the pie, the chord and the arc, whose handles
  are angles.
  *Not done:* the rest read their handles and draw at the format's fallback —
  the can's lid, the donut's rim, the trapezoid's lean, the ribbons' panel, the
  notched and striped arrows, the elbows and the curved arrows, the teardrop,
  the plaque, the moon, the cross, the L and the half frame. Each of those
  states its handle through the format's own guide arithmetic, several values
  combining into one point, and this program has the proportions it draws from
  rather than those tables. Where the two agree the handle is used; where they
  do not, using it would draw a shape the number does not mean. That is **D21**
  along with the handles themselves.
- [x] **D4. Charts: several series, the key, and the numbers on the points.**
  A chart part with more than one `c:ser` in it, read and written whole, drawn
  the way Word draws it, with `c:legend` saying which series is which and
  `c:dLbls` putting the number on every point.
  *Done when:* a chart of two series opens here showing both, with a key naming
  them and the numbers on them, and Word opens the saved file showing the same.
  A chart was one series until today: one name, one run of numbers, one colour.
  The file format never said so — `c:ser` repeats, and a reader that takes the
  first and stops is a reader that silently drops half of what somebody drew.
  Now every series is read in the order the file gives them, and the categories
  are taken from the first, because they are the same for all of them and a file
  that disagrees with itself is believed at its first word.
  Several series change what drawing means. Columns and bars share one slot per
  category between them — Word calls it clustered, and it is how a chart of
  several series is read at all; the slot is the width the one column used to
  have, divided by the number of series. A line chart draws one line per series
  in its own colour, which is what the key then names. A pie draws the first
  series and no other: a pie of several series would be several pies, and the
  format has a chart type of its own for that.
  The key is measured before the plot is laid out and drawn after it. Measured
  first because the room it takes has to come off the plot — a key drawn over a
  plot laid out as though there were no key sits on the bottom row of numbers.
  Drawn after because it is drawn where the plot is not.
  A pie's key is the other key: it names the slices rather than the series,
  because a pie *is* one series and its slices are the categories. So a pie
  keeps its names down the side, where there is room for as many of them as
  there are slices, and every other chart puts its key in a row under the plot.
  And a pie typed into the chart bar is given that key without being asked: a
  pie with nothing naming its slices says nothing at all, which is what Word
  decides for the same chart.
  `c:dLbls` is written per series, which is what the format says, and asked of
  the whole plot when reading, which is what Word's own button means: a chart
  with the numbers on half its series is not something Word can make.
  *Not done:* the rest of what a chart is — see **D24**.
- [x] **D5. SmartArt: the parts a diagram is kept in.** `dgm:relIds` and the
  five parts behind it — the data model, the layout, the quick style, the
  colours, and the drawing Word made the last time it followed the layout.
  Read, drawn, and written.
  *Done when:* a diagram made here is SmartArt when Word opens it, and a
  diagram made in Word is drawn here as Word drew it rather than as an empty
  space.
  A diagram used to be a heap of shapes. The module said so at the top of the
  file — press the button, get four boxes with arrows between them, and Word
  opens them as drawings that will never re-lay themselves out. That was a
  stated trade, and this is the end of it: what is written now is the five
  parts, and what Word opens is a diagram it offers to restyle, recolour and
  retype.
  Reading one is the other half. The data model is the words and how they are
  related — points, connections, and an order — and it is read into a tree,
  because a tree is what it is: a hierarchy hangs its boxes under the first,
  and a row of them hangs everything under the document. The points the layout
  engine left behind and the connections that say which shape drew what are
  passed over; a connection that points at its own source is read and not
  followed for ever.
  What is drawn is the drawing. It is the fifth part, the one the schema has no
  room for — it hangs off the data model through the extension list — and it is
  shapes with places, colours and words in them. Drawing that is drawing what
  Word drew; the alternative is running the layout language and drawing
  something that nearly agrees with Word. So a diagram out of a Word document
  is drawn exactly as Word laid it out, and it is drawn as a group, because a
  group is what a drawing of several shapes is and the group is already placed,
  measured and drawn.
  The colours in that drawing are named and not stated: `a:schemeClr val="accent1"`
  with a lightening or a darkening written under it, which is how one colour
  list draws six boxes in six colours. Those are resolved here against the
  document's theme, shifts and all — shade, tint, and the ones said in hue,
  saturation and lightness, which the colour goes round into and back out of.
  A drawing read without that is a diagram drawn as a row of empty outlines.
  A tree is drawn with a stem out of the box above, a run across, and a drop
  into each box below — bars, and not any of the bent connectors, every one of
  which leaves its shape sideways because what it joins is one shape's side to
  another's.
  And a diagram is one drawing however many boxes it holds, so the
  accessibility check asks it for a description the way it asks a picture:
  the words in the boxes are not a description of what the diagram says.
  *Not done:* the layout language itself and everything that needs it — see
  **D25**.
- [x] **D6. Ink and media as the file keeps them.** `w14:contentPart` and the
  InkML part behind it; `wp15:webVideoPr` and the frame it marks. Read, drawn
  and written.
  *Done when:* a document somebody drew on opens here showing what they drew,
  and a video in a document shows its frame with the play sign over it and
  follows its address when pressed.
  The way in was a thing neither of them: **markup compatibility**. Word writes
  a drawing twice — once as what it means, and once as what a reader too old to
  know the first can draw instead — and wraps the pair in `mc:AlternateContent`.
  Nothing here read that. Everything from the shapes gallery arrives wrapped
  that way, so every shape and every text box Word itself made was invisible in
  this program: not misdrawn, not there. Now the choices are read in turn and
  the first that comes to anything is kept, and the fallback is read when none
  of them did — which is the format's own rule, asked the only way a reader can
  answer it.
  That had to be done twice, in two layers. The reader says what a run holds;
  the layer that counts the characters of a paragraph says where the caret may
  stand. A drawing counted by one and not by the other is every offset after it
  out by one, so both walk the alternatives the same way and both count ink as
  the one character it is.
  **Ink** is a part of its own, written in InkML, which is the W3C's format and
  not Microsoft's. A stroke is written mostly as how far the pen moved rather
  than where it is: the first point outright and the rest as differences, with
  a prefix saying which kind each number is — and a number with no prefix going
  on in whatever kind the last one for that channel was. That last rule is the
  whole of the encoding: without it every unprefixed number reads as a position
  and a line of handwriting comes out as a scribble round the origin. The
  prefix separates as well, which is why `'-4'-1` is two numbers.
  The pens are read too: the colour, the width in hundredths of a millimetre,
  and what makes a highlighter a highlighter — a flat tip and a raster
  operation that leaves what is under it showing. A highlighter is drawn
  see-through, so the words under it are still words.
  Drawn as a band along every turn the pen took, fitted into the room the file
  says the ink takes and centred in what is left: the fit is the same both
  ways, because handwriting stretched to fill a box is somebody else's
  handwriting.
  And written: the part, the relationship, the run that points at it, and the
  declaration that says a reader which does not know the extension may pass it
  over — without which a strict reader stops at the ink and the document does
  not open at all.
  **A video from the web** is not a video. It is a still of one, the address it
  plays from, and the markup that would embed a player: Word puts the frame in
  as an ordinary picture, the address on the drawing as a link, and an
  extension beside it saying the picture stands for something more. All three
  are read, the frame is drawn with the play sign over it — a fifth of the
  shorter side, dark and see-through, with the triangle set a little right of
  the middle because a triangle centred on its own box looks left of centre
  inside a circle — and Ctrl and a press follows the address. A picture carries
  its link inside itself rather than in an element round it, so a linked
  picture of any kind is followed now and not only a video.
  *Not done:* drawing ink here, playing anything, and media kept inside the
  document — see **D27**.

- [x] **D21. The adjust handles.** The yellow diamonds themselves: where each
  preset puts them, drawing them on the chosen shape, dragging one, the shape
  following under the pointer, the value written, and one drag being one undo.
  *Done when:* dragging a handle changes the shape and writes a value Word
  reads back as the same shape.
  A handle has two questions to answer — where it sits for the value the
  document gives, and what value it means when somebody drags it elsewhere — and
  both come out of **one** description: the line it slides along, and what the
  far end of that line is worth. Two answers written separately drift, and a
  handle that jumps out from under the pointer as it is taken hold of is exactly
  that drift.
  The same goes for what a handle is worth when the document says nothing. Every
  such value is now one number, in the format's own unit, asked for by the
  geometry that draws the shape and by the handle that drags it. A test writes
  each handle's own value into a document and checks the shape comes out
  unchanged — and it caught three places where the two had already drifted: a
  sixth against 16,667; a five-pointed star's dip against 19,098; and the arc,
  whose handles are 270 degrees and 0. That last one is not drift but a rule:
  **an arc sweeps forwards** from where it starts to where it stops, so 270 to 0
  is the quarter at the top and not three quarters drawn backwards.
  Nine more shapes obey their handles now: the can, the donut, the "no" symbol,
  the frame, the cross, the L, the half frame, the plaque and the block arc.
  *Not done:* the rest of the list under **D20** — the trapezoid's lean, the
  ribbons' panel, the notched and striped arrows, the elbows and the curved
  arrows, the teardrop, the moon, the chevron and the pentagon. And the
  callouts, which are a different thing: they obey the handles that say where
  their tail points, but there is no diamond to drag them by, because a tail is
  one handle that moves both ways at once and writes two values — and a handle
  here slides along a line and writes one.

- [x] **D22. Connectors that stay joined.** `wps:cNvCnPr` with `a:stCxn` and
  `a:endCxn`: which drawing each end of a connector is fastened to, and at which
  of that shape's connection points.
  *Done when:* two shapes joined by each kind of connector stay joined when
  either is moved, and Word opens the saved file with them still joined.
  A line drawn between two shapes is a line: move either shape and it stays
  where it was, pointing at nothing. A connector is *fastened* to them, and the
  file says so. So the box a connector was saved with is only the answer from
  the last time anybody worked out where it should be — and it is worked out
  again at layout time rather than believed. A pass of its own, after everything
  is placed, because a connector may be laid out before the shapes it joins and
  where it goes depends on where they went.
  Which way round it is drawn comes out of the same arithmetic. A connector runs
  from one corner of its box to the opposite one, so the box alone cannot say
  which corner is the start; an end fastened to a shape on the right is the same
  connector mirrored, and that is what the flips are for.
  The id is the one on the shape's own properties, `wps:cNvPr/@id`, and not the
  one on the drawing that wraps it: two different numbers live a few elements
  apart in the same file and only one of them is the one a connector names.
  The screen and the file are two different things, so the box is written back
  when a drawing is moved. A document moved about here and saved would otherwise
  open in Word with its connectors back where they used to be, which is the sort
  of thing that makes a program untrustworthy with somebody else's work. Asking
  again when nothing has moved changes nothing: a document that marked itself
  modified every time it was looked at would never stop asking to be saved.
  *Not done:* the routing, which is **D23**. A connector fastened to the right
  of one shape and the left of another that is further left will run back
  through the shape it came out of, because the elbow is drawn inside the box
  between the two points and there is nowhere else for it to go.

- [x] **D23. Routing an elbow round the shapes it joins.** A connector leaves
  each shape by the side it is fastened to and comes back to the other the same
  way, and the legs between are laid so that neither shape is crossed.
  *Done when:* a connector fastened to the right of one shape and the left of
  another standing to its left goes round both of them rather than back through
  the one it came out of.
  A route that leaves the start upwards is the same route with the two measures
  swapped, so it is worked out on its side and turned back at the end. That
  halves the cases, and what is left is three: the two ends facing each other
  with room between them, an end entered from above or below, and everything
  else — which goes round by a lane clear of both shapes, above them or below
  them, whichever is nearer. A route is taken only if no leg of it runs through
  either shape; the lane is what is left when none of the short ways is clear.
  A routed connector is drawn from its route and not from its preset, so the
  flips come off it: they say which corner of the box a preset starts from, and
  a route already says where every corner of it goes. A route drawn mirrored is
  a route drawn somewhere neither shape is, which is what the first attempt drew.
  The box a routed connector is given is widened to hold the whole route. A
  route may go outside the two points it joins, and the box is what the rest of
  the program believes about where a drawing is: half a connector would
  otherwise be outside anything anybody could take hold of.
  *Not done:* the file keeps the preset and the handles it came with. Word
  encodes a route by swapping the connector between `bentConnector2`, `3`, `4`
  and `5`, turning it a quarter when it leaves upwards, and putting the bends
  outside the box — and none of that is written back, so Word opens a document
  saved here and routes the connector its own way between the same two points.

- [x] **D24. The rest of what a chart is.** What **D4** left, named:
  the chart types beyond the four drawn today — stacked and hundred-percent
  columns and bars, area, scatter, bubble, doughnut, radar, surface, and the
  combinations Word offers as one chart; `c:dTable`, the table of the numbers
  drawn under the plot; `c:numFmt`, the number format on an axis and on a label,
  without which money is drawn as a bare number; the rest of what a label may
  say — the category name, the series name, the percentage, and the leader line
  drawn to a label that had to be moved off its slice; `c:spPr` on a series and
  `c:dPt` on a point, which is a chart whose colours the document chose rather
  than the palette; the scale on a value axis stated by the file rather than
  worked out from the numbers; and the workbook behind the chart in
  `xl/embeddings`, which is what Word opens when somebody asks to edit the data
  and what it rewrites the caches from.
  And the key's own place: `c:legendPos` is read and written and comes back
  unchanged, but every key that is not a pie's is drawn in a row under the plot
  whatever it says, because putting it at any of the four sides means laying the
  plot out four ways.
  *Done.* Ten kinds where there were four: column, bar, line, pie, doughnut,
  area, scatter, bubble, radar and surface, each read from its element and
  written back to it, the three-dimensional elements read as the flat kind
  they picture. Columns, bars, lines and areas are grouped as the file says —
  side by side, stacked, or stacked to the same height as shares — with
  negative numbers hanging below a nought line that is drawn where nought is.
  A series may be drawn as another kind than the chart's, which is Word's
  combination chart, and may stand against a second value axis down the
  right; the file's several plot elements come back as several, in order,
  with a series numbered by its place among all of them. A scatter has an x
  per point and a bubble a size, and both are typed as `x=y` and `x=y:size`
  from the chart bar, whose list is now Word's gallery in one column.
  The key goes where it is asked — right, left, top, bottom, the top right
  corner, or laid over the plot — and takes its room off the plot before the
  plot is laid out. A pie's key names its slices at whichever side it asks
  for. The table of the numbers is drawn under the plot with its columns on
  the plot's slots, its rows named and keyed, and the axis names left to its
  first row, as Word leaves them. A label says what it is asked to say — the
  number, the category, the series, the share — joined with commas, sits
  where `c:dLblPos` puts it, and on a slice it will not fit across is moved
  out past the edge on a leader line, which is the one Word draws. A series
  or a point coloured by the document keeps its colour, resolved against the
  theme when it names a theme colour; a chart with `c:varyColors` colours
  each point. The value axis takes the scale the file states — its ends and
  its step — and otherwise a scale at numbers a person would choose.
  **Number formats.** `wp-docx/src/numberformat.rs` reads the spreadsheet's
  format codes as far as a chart uses them: digits and thousands, forced and
  optional places, percent, the scale by a thousand, scientific notation,
  the words round a number, and the sections that give a negative or a nought
  its own form. `"$"#,##0.00` draws money as money, and an axis in `0%`
  draws shares as percentages.
  **The workbook.** Every chart made here carries the spreadsheet Word
  would have made, in `word/embeddings`, laid out as Word lays it out and
  pointed at from the chart part through `c:externalData`, so that Word's Edit
  Data opens on it. And it is read: a chart part that cached no numbers and
  says only where they are — `Sheet1!$B$2:$B$5` — has them read from the
  workbook, which is what Word does with such a file. A workbook is a package
  like the document, and is written and read with the same code.
  *Fixed on the way:* the chart bar's messages were never in the catalogue,
  so a German window asked for the numbers in English.
  *Proven by:* `wp-docx/src/chart.rs` — every kind and every grouping
  written and read back, a three-dimensional element read as flat, the key
  at every side, everything a label can say, labels a series keeps for
  itself, the table and the axes, colours chosen and colours named from the
  theme, a combination chart on a second axis, a scatter's x values and a
  bubble's sizes, a doughnut's hole, and numbers left to the workbook read
  from it; `numberformat.rs` — eleven shapes of code; `workbook.rs` — a
  workbook written and read back, cell addresses and references;
  `wp-layout/src/charting.rs` — every kind drawn inside its box with a key
  and labels, negatives below nought, stacks standing on one another and
  shares reaching the top, an area filled, points and bubbles, a radar's
  rings and spokes, a surface's bands, a doughnut's rings, the key at each
  side, the table under the plot, a money axis, chosen colours, a line over
  columns, and a slice's label moved out on its line; `tests/charts.rs` — the
  workbook in the package and the chart pointing at it, numbers read from it,
  and a theme colour resolved against the document's.
  *Not done, and named:* the three-dimensional kinds are drawn flat, without
  Word's perspective; a surface is drawn as its contour, seen from above,
  never as the wireframe; a scatter is points and never the smooth or
  straight lines Word can draw through them, and a radar is never filled;
  stock, pie-of-pie and bar-of-pie charts are read as the line and the pie
  they are made of; an axis title is neither read nor drawn; dates, times,
  fractions and the conditions in a format code (`[>100]`) are written as
  `General`; a label's separator is always the comma; the second value axis
  writes its numbers plainly, whatever format it asks for; a marker is
  always a square. The numbers of a chart are typed once, when it is made:
  there is no grid to edit them in afterwards, which is what the workbook is
  written for Word to offer.

- [x] **D25. The layout language, and editing a diagram.** `layout1.xml` is
  read here for one thing — the name of the arrangement it is — and the rest of
  it is not run: the algorithms, the constraints, the rules, the conditions and
  the `forEach` that walk the data model and place a shape for every point. A
  diagram whose file carries no drawing and whose layout is one of the hundred
  and thirty in the gallery is drawn as the list of what it says, which is the
  words in the right order and the wrong picture.
  That language is what the rest of SmartArt stands on: re-laying a diagram out
  when its words change, the text pane that types into it, promoting and
  demoting a box, adding and removing one, changing a diagram from one
  arrangement to another, and the two tabs Word shows when a diagram is
  selected. None of those can be done by moving shapes about, because what they
  change is the model and what draws the model is the layout.
  The quick style and the colour list are written and not read: what is drawn
  is the drawing's own colours, and a diagram whose file has lost its drawing
  is drawn in the theme's first accent whatever its colour part asks for. The
  drawing is never rewritten either — nothing here changes a diagram yet, and
  the moment something does, the drawing it was laid out into is stale.
  What a shape of a drawing may carry and this does not read: a gradient or a
  picture where the fill is, the `dsp:style` that names the theme's line and
  fill by index, and `dsp:txXfrm` — the rectangle the words go in, which is not
  the shape's own for a shape whose middle is not where its room is.
  And the layout definition written here is this program's own, simpler than
  the gallery's. What Word draws from it when the words change has not been
  checked against Word itself; the picture does not depend on it, because the
  drawing is written too.
  *Done.* **The language runs.** `wp-docx/src/diagram/language.rs` reads a
  layout definition and lays a data model out through it: the tree of
  `layoutNode`s, `alg` with its parameters, `shape`, `presOf`, the
  constraints — every `type` the definitions use, `for`/`forName`/`ptType`
  on the target, `refType`/`refFor`/`refForName`/`refPtType` on what it
  refers to, `op`, `val` and `fact`, and `w`/`h` in units of a sibling's
  share — the rules that let a font shrink to a floor, `varLst`, `forEach`
  with its axis, `ptType`, `st`, `cnt` and `hideLastTrans`, a named
  `forEach` called again by `ref`, and `choose`/`if`/`else` with the
  functions `cnt`, `depth`, `maxDepth`, `pos`, `revPos`, `posEven`, `posOdd`
  and `var` and their comparisons. The axes are all twelve: `self`, `ch`,
  `des`, `desOrSelf`, `par`, `ancst`, `ancstOrSelf`, `follow`, `followSib`,
  `preced`, `precedSib` and `root`, over every point type including the
  transitions. The algorithms are the ones the arrangements here use —
  `lin` in both directions, `composite`, `hierRoot` and `hierChild` with the
  elbow lines drawn between a parent and its children, `cycle` with its
  connectors turned along the ring, `snake` in rows that may turn back,
  `pyra`, `tx`, `sp` and `conn` — and a definition is sized, then placed,
  then its words fitted, so a box that is given its room before its words
  are measured gets the font the rules allow.
  **Six arrangements written in it.** The process, the vertical box list,
  the hierarchy, the cycle, the pyramid and the block list are each a
  definition in the language, written into `layoutN.xml` when a diagram is
  made, and read back out and run: nothing about a diagram made here is
  remembered outside its file. A file whose diagram has no drawing is laid
  out from whatever layout part it carries.
  **The drawing, read whole and rewritten.** `dsp:txXfrm` is read, and the
  words go in the rectangle it names; `dsp:style` names the theme's line,
  fill and font by index, and a shape that states no colour of its own is
  drawn in those; a gradient where the fill is is read as one. The quick
  style and the colour list are read now, `wp-docx/src/diagram/styles.rs` —
  the label's fill, line and text, the accent it names, and the three ways a
  colourful list hands its colours out, repeating, cycling and spanning — so
  a diagram whose drawing is lost is drawn in the colours its parts ask for.
  And when the words change the drawing is written again from the layout
  into the frame, so what the file carries is never stale.
  **Editing.** Choosing a diagram brings up Word's two tabs, *SmartArt
  Design* and *Format*, and opens the Text Pane, `chrome/textpane.rs` and
  `editor/textpane.rs`: the words as a bulleted outline, typed into, with
  Enter for a new box, Backspace at the start to join it to the one above,
  Tab to hang it under that one and Shift+Tab to lift it out, the arrows and
  Home and End to move about. The tab has Add Shape, Promote, Demote, Move
  Up, Move Down, Right to Left, Text Pane, the Layouts gallery, Change
  Colors, and Reset Graphic; *Format* has Larger and Smaller, which scale
  the frame and lay the diagram out again into it. A row of boxes asked to
  be a hierarchy hangs the rest under the first; a tree asked to be a
  process shows its deeper words as bullets in the box, as Word's does.
  Every one of those is one step of undo: a diagram's words and picture live
  in parts of the package, so the history keeps those parts with the tree
  and puts them back — `Kept::WithParts` in `wp-docx/src/history.rs`.
  *Proven by:* `language.rs` — the model and its points, every axis, `st`
  and `cnt`, the conditions, a process definition laying boxes across with
  arrows between, and words that shrink to the floor; `styles.rs` — the
  three ways of handing out colours and the label lookups; `diagram.rs` —
  each of the six arrangements measured, words centred in their box, the
  data model's tree written and read back, the drawing round trip, the
  style fallback, a gradient, the colour lists, and the rehanging;
  `tests/diagrams.rs` — the parts in the package; `editor/textpane.rs` —
  the tab and the pane on choosing, typing that changes the picture, Enter
  and Backspace, Tab and Shift+Tab, moving, Add Shape, the layout and the
  colours changed and the file saying so, Right to Left and Reset, Larger
  and Smaller, one undo per edit, and Escape and the other panes shutting
  it.
  *Not done, and named:* the definitions are this program's, simpler than
  the gallery's hundred and thirty, and none of the gallery's files are
  here to run — a file that carries one is laid out by it, and what that
  looks like has not been checked against Word, nor has what Word draws
  from the definitions written here. The algorithms are the seven the
  arrangements use, and not every parameter of those: `hierChild` follows
  `chDir` and not `linDir`, `secLinDir`, `chAlign` or `hierAlign`; `cycle`
  follows `stAng`, `spanAng` and `ctrShpMap` and not `rotPath`; `snake`
  follows `contDir`, `bkpt` and `bkPtFixedVal` and not `grDir` or `flowDir`;
  `lin`'s `fallback` is read past; and a `constr` whose `type` is one of the
  distances on a connector (`begPad`, `endPad`, `bendDist`) is kept but
  changes nothing. Words are measured by an estimate — six-tenths of an em a
  character, a leading of one and a fifth — not by the font, so a box's font
  size may differ from Word's by a step. A one-dimensional connector — the
  elbow between a parent and its children, the line between two shapes of a
  cycle — is drawn as bars a point wide, where Word draws it but not as the
  `bentConnector3` or the arrow Word draws; a connector's own preset is
  honoured only in a drawing Word wrote. The Format tab has no shape fill,
  outline or text effects of its own, and the SmartArt Styles gallery is
  Change Colors alone: the quick styles beyond the first are not offered.
  Three-dimensional and animation parameters are read past. The Text Pane
  does not select a box in the picture when a line is chosen, nor does
  clicking a box move the pane's caret.

- [x] **D26. A shape drawn in the theme's colours.** `a:schemeClr` where a
  colour is asked for. A diagram's drawing has this now — the slot and the
  shifts written under it, resolved against the document's theme — and every
  other shape does not: a shape Word filled with accent 1 rather than with
  four hex digits is read here as a shape with no fill and drawn as an outline.
  Word writes that fill for every shape from its own gallery, so this is most
  of the shapes in most documents.
  The resolving belongs where the theme is known, which is not where a shape is
  read: a fill is read out of an element with no package in reach. So the fill
  has to carry what the file said — the slot and its shifts — and be resolved
  when it is drawn, which is one more thing a fill can be and one more place
  that has to ask the theme. The arithmetic itself is done and tested; it is in
  the diagram module and belongs beside the theme.
  *Done.* **A colour that is what the file said.** `wp-docx/src/colour.rs`:
  a colour is a base — six hex digits, one of the theme's twelve slots, one
  of the format's hundred and forty named colours, a system colour with the
  value it was last seen with, or the placeholder a theme's style leaves
  open — and the shifts written under it, by name and value in the order
  written. It is resolved against a theme when it is drawn, not when it is
  read, and written back as it came, shifts and all, including the ones this
  program does not apply. The arithmetic moved here from the diagram module:
  shade, tint, the six in hue, saturation and lightness, the absolute three,
  the complement, the inverse and the grey. A fill's colours are these now —
  the solid, every stop of a gradient, both colours of a hatching — and so
  are the line round a shape, a shadow's, a glow's, and the sides and edge of
  a solid shape. `Paint`, the effects and the depth are worked out against
  the theme the layout engine now keeps whole, and a theme changed from the
  Design tab recolours every shape that named it, which is what Word does.
  **The fill a shape takes by number.** A shape from Word's gallery states
  no fill and no line: `wps:style` says `fillRef idx="1"` in accent 1 and
  `lnRef idx="2"` in accent 1 shaded by half, and the theme's format scheme
  says what fill style one and line style two are. So `Fill::Styled` is one
  more thing a fill can be — a number into the theme and a colour — and the
  theme reads `fillStyleLst` and `lnStyleLst` with the placeholder kept,
  fills the shape's colour in when asked, and writes them back so a theme
  Word made keeps its gradients. A shape carries which line style its width
  comes from when it states none, and the colour its words take when they
  name none — `fontRef`, the white on a gallery shape — which the words are
  laid out in. A shape made here is written as Word writes one: the style
  references and nothing in the properties, so it changes with the theme in
  Word as well. The line of a shape with one of its own over the style keeps
  its thickness and takes the style's colour when it states none; a line
  saying nothing anywhere is the theme's text colour, not black.
  *Proven by:* `colour.rs` — a stated colour, a named one with its shifts
  resolved against two themes, every shift, the round trip, the named and
  system colours, and the placeholder filled in with the shape's shifts
  first; `fills.rs` — a named colour kept as the name, a styled fill against
  the theme, nothing said against nothing; `theme.rs` — the format scheme's
  three fills and three lines; `shapes.rs` — a gallery shape written with
  the references and nothing in the properties, read back the same, and
  resolved to the blue and the darker blue against Office and to red against
  a red theme, and a line of the shape's own over the style; `shapeeffects.rs`
  — a glow named from the theme with its amount kept apart; `paint.rs` — a
  named colour, a styled fill, a theme whose second fill style is a gradient
  of the shape's colour; `editor/shapes.rs` — a shape from the gallery
  painted in the Office blue, red the moment Design ▸ Colors ▸ Red is
  chosen, its line a darker red, and the file saying the style and not a
  colour; a line from the gallery in the first accent at the first line
  style.
  *Not done, and named:* the alpha under a fill's colour is carried and
  written back but not drawn — a shape is drawn opaque; the gamma and
  single-component shifts are carried and change nothing. The theme's
  effect styles are still read as one shadow, not as the three lists, and
  `effectRef` is always written as the second. The background fill styles
  are neither read nor used. A text effect on words (`w14:`) has its own
  `schemeClr` and is still read as six hex digits. A chart's and a diagram's
  colours are resolved when they are read, as before, so a theme change
  redraws them only because their parts are laid out again. The words in a
  gallery shape are laid out from the top left, not centred as Word's
  `anchor="ctr"` and centred paragraph put them, which was so before this
  and is not a colour.

- [x] **D27. Drawing ink, and the media a document carries inside it.** What
  **D6** left.
  Nothing in this program draws ink: the strokes can be read, drawn and
  written, and the only way to make any is to hand the model a stroke. What is
  missing is Word's Draw tab — the pens and their colours and widths, the
  highlighter, the eraser that takes a whole stroke and the one that rubs part
  of it out, drawing with the pointer, and the two things Word does afterwards:
  ink to shape, and ink to text.
  The pressure channel is read past. A pen reports how hard it was pressed and
  Word draws a stroke that swells and thins with it; a stroke drawn here is the
  same width from end to end.
  Where ink goes is not where the file says. `w14:xfrm` carries an offset as
  well as a size, and only the size is used: ink is drawn in the line the run
  sits on, at the size the file gives it. A note written across a paragraph in
  Word is drawn beside that paragraph here.
  The relationship written for an ink part is the one Word uses for a content
  part. Nothing read here depends on it — a relationship is followed by its id
  — and Word is the reader that does; it has not been checked against Word.
  **Inserting a video** here still writes a link and not a frame, and says so:
  a frame is a still of the video, and this program does not talk to the
  network and cannot fetch one. What it can do, and now does, is show the frame
  a document already carries. The way out of that is the same way out of every
  other fetching: somewhere to say what this program may reach and what it may
  not.
  **Media kept inside the document** is not read at all: a sound or a film
  embedded as an object — `w:object` with an OLE object behind it, which is
  what Insert > Object makes — and `a:videoFile` or the 2010 media extension on
  a picture, which is what a video dragged into a document becomes. Each of
  those shows in Word as a picture with a control over it, and each plays when
  pressed. Nothing here plays anything: sound and moving pictures are a
  different machine from the one that draws a page, and what this program would
  honestly do first is show the frame and offer the file to whatever the person
  plays such things with.
  *Done.* **The Draw tab.** `editor/inking.rs`: Select, which is Select
  Objects; the Eraser, whose face takes a whole stroke at a touch and whose
  arrow offers the one that rubs out what it passes over and leaves the rest
  of the stroke in pieces; and the Pen, the Pencil and the Highlighter, each
  taken up from its face and each with its thicknesses and colours on its
  arrow — Word's own lists, half a millimetre of black for the pen, six of
  yellow for the highlighter, which is wide, flat and see-through. A stroke
  is drawn with the pointer and shown as it goes; when the button comes up
  it is written as Word writes ink: a part of its own in a drawing that
  floats across from the page's edge and down from the paragraph the stroke
  began beside, in an alternative that names the ink extension, one drawing
  per stroke. Drawing is not typing — the caret stays where it was — and
  every stroke, every rub-out and every conversion is one undo, the ink part
  kept with the tree.
  **Where ink goes is where the file says.** The form Word has written ink
  in since 2013 — a `w:drawing` whose graphic is the content part, anchored
  the way a picture floats — is read as ink, with its anchor, its size and
  its name, and placed by the same rule as a floating picture. Ink is a
  drawing of its own on the page now, `PlacedInk`, ordered among the
  pictures and shapes by its depth, chosen by a press, gathered by the
  band, listed in the Selection Pane, and written to a PDF.
  **Pressure.** The `F` channel is read as a share of the most it can say,
  kept with the stroke, written back, and drawn: a stroke swells and thins
  with it, from half the pen's width at no pressure to half as much again
  pressed hard. A pen the file says nothing about is drawn even, as before.
  **Ink to Shape and Ink to Text.** The strokes chosen become the shape
  they draw — a line when they go straight, and when they come back to
  where they began an oval, a triangle, a rectangle, a diamond, a pentagon
  or a hexagon by the corners left once the wobble is taken out — in the
  pen's colour and width, floating where the ink was; or the letters they
  spell, `editor/handwriting.rs`: block capitals and digits, read by the
  point-cloud method against templates drawn here, strokes that stand over
  one another making one character, a gap of half a character's height
  making a space, a bar an I among letters and a one among digits.
  **Media kept inside the document.** `wp-docx/src/embedded.rs`: a
  `w:object` counts as the one character it is and is a drawing where it
  stands, its preview shown as a picture; behind it the compound file is
  opened with `wp-ole`, the reader the `.doc` and the encrypted document
  already go through, and the file the packager wrapped in `\1Ole10Native`
  comes out with its name, or the object itself does, named after the
  program that made it. A picture that carries
  `a:videoFile` gives the media the same way, from the package or as the
  path it names. Ctrl and a press offers the file to be saved where the
  person chooses — not to whatever program plays such things, because a
  document does not get to say what this program runs — and the status bar
  says so while the caret is beside it.
  *Fixed on the way:* a diagram made larger or smaller was two undo steps
  rather than one, the parts and the frame apart; the icon catalogue's
  generator now knows a drawing of the program's own, so the open padlock
  survives a regeneration.
  *Proven by:* `wp-docx/src/ink.rs` and `tests/ink.rs` — pressure read and
  written, the floating form written as Word writes it and read back as ink
  rather than as a picture, the eraser leaving what it did not touch where
  it was and one undo putting a stroke back; `wp-layout/src/inking.rs` —
  the fit both ways; `embedded.rs` — the packaged file with its name, an
  object that is not a package offered as what it is, an embedded object
  with its preview counting one
  character and offering its file, a video inside or outside the document;
  `editor/inking.rs` — a stroke drawn with the pointer becoming ink that
  floats where it was drawn, the highlighter's look, the two erasers,
  Escape and Select putting the pen down, a drawn box becoming a rectangle
  and back in one undo, H and I drawn and read as "HI"; the shapes told
  apart and the pieces an eraser leaves; `editor/handwriting.rs` — every
  character read back from its own shape drawn larger and elsewhere,
  strokes gathered into characters and words.
  *Not done, and named:* a mouse reports no pressure, so ink drawn here is
  written without it; a pen that does is read past by the shell, which
  gives the window only where the pointer is. The pencil is drawn the same
  as the pen, without Word's grain. The ink written here has no fallback
  picture beside it for a reader older than the extension, and neither the
  form written nor the relationship type has been checked against Word.
  Lasso Select is the rectangular band; Ink to Math, the Drawing Canvas,
  Ink Replay and the ruler are not there. Ink to Text reads block capitals
  and digits only, from templates drawn here rather than from anybody's
  hand, and a scrawl is read as the nearest of them; Ink to Shape knows
  seven shapes, and an oval drawn wobbly may be read as the polygon it
  nearly is. The run form of 2010 keeps its ink in the line, its offset
  still unused. Nothing here plays a sound or a film. The packager's
  wrapping was tested against a file written here with `wp-ole`'s builder,
  not against one Word wrote.
  *Fixed after:* the first commit of this carried a compound-file reader of
  its own, written without looking for the one the repository had; it is
  gone, and `wp-ole` is used.

## E — The rest of the text engine

- [x] **E11. The lookups the shaper skipped.** Contextual, chaining contextual
  and extension: the three kinds of `GSUB` lookup that were read far enough to
  be passed over. Done before **E1** because the scripts that reorder are
  written almost entirely in them — an Indic engine standing on a shaper that
  cannot read a chaining rule would run features that do nothing.
  *Done when:* a rule that fires only in company fires only in company, and a
  font that writes its rules behind extension offsets is read like any other.
  A contextual lookup is not a substitution. It is a rule about surroundings —
  this glyph, but only between those two — and what it does is name *other*
  lookups and the places in the match to run them at. So the shaper needed a
  way to run a lookup at one place and nowhere else: a lookup let loose on the
  run would change every letter like the one the rule pointed at, which is the
  difference between a rule about a letter and a rule about a word.
  Three ways of writing each of them, and all six are read: by glyph, one rule
  per glyph that may begin a match; by class, so a rule about a whole set of
  letters is written once; and by coverage, one set per place, which is how a
  font says "any of these, then any of those". The chaining kind says the same
  with a before and an after — the before written nearest-first, so it is read
  backwards from where the match begins.
  A nested lookup may make the run shorter: two glyphs becoming one is what a
  ligature is. The places a rule named were counted before that happened, so
  what each one did to the length is carried along and added to the places
  still to come.
  An extension lookup is none of these: it is a lookup that says what kind it
  really is and points at that table with a thirty-two bit offset, which is how
  a font too big for sixteen-bit offsets is written. A reader that skips
  extensions skips most of what a large font says. One pointing at another is
  refused — the format forbids it, and a font that wrote one could send a
  reader round for ever; so could a rule naming itself, which is why a lookup
  stops after eight steps into another.
  And `ccmp` is applied now, before anything else, which is what the format
  asks of every shaper: it is where a font says that a letter and the mark on
  it are written as one glyph. The visible answer is in DejaVu: an `i` with a
  mark above it loses its dot, because two dots on one letter is not what
  anybody wrote. That rule is a chaining one, so nothing here could read it
  before today.
  The tables are built by hand in the tests, byte by byte, because no font to
  hand is written so that each rule can be seen on its own; and the composing
  is checked against whatever font the machine really has.
  *Not done:* where the mark then lands. See **E12**.

- [x] **E12. The positioning table.** `GPOS` and `GDEF`, which were not read at
  all: the tables were found and passed over, and the only kerning read was the
  old `kern` table that fonts written this century no longer carry.
  *Done when:* a pair the font kerns is set closer than its widths, and an
  accent is drawn over its letter rather than beside it.
  Two things came out of it and neither is decoration. The first is kerning:
  DejaVu keeps it in a class-based pair lookup and its old table says nothing,
  so every document drawn here was set at the plain widths however much trouble
  the designer took. The second is where a mark goes. A combining accent is
  drawn without moving the pen, so left alone it lands at the right-hand edge
  of the letter before it — which is exactly what the proof for **E11** showed.
  Where it belongs is written as two points, one on the letter and one on the
  mark, to be brought together.
  What is read: single and pair adjustment, by glyph and by class; mark onto a
  letter, mark onto a piece of a ligature, and mark onto another mark; and the
  extension lookups the rest is hidden behind in any large font. `GDEF` comes
  with it, because none of the mark rules can be followed without knowing which
  glyphs are marks: a mark's letter is the nearest thing before it that is not
  itself a mark, and the lookup flags that say "pass over the marks" — which is
  what lets a font kern the letters either side of an accent — cannot be obeyed
  without the same answer.
  The arithmetic that matters is the travel. A mark is drawn where the pen has
  already reached, so what its offset has to do is take that travel back and
  then put the mark's own point on the letter's — counting the kerning, which
  is why the kerning is applied first and the marks after it.
  The walk through a script, a feature and a lookup is the same in both tables,
  so it is written once now and both use it: two copies would be two things to
  keep in step, and the day they drifted one table would be read with the
  other's arithmetic.
  The layout carries it: a glyph has an offset from where the advances put it,
  the line places it there, and a PDF written from that page keeps it — the
  writer already begins a new run wherever a glyph goes backwards or sits off
  the baseline, which is exactly what a mark does.
  A run with a mark in it is now shaped whole rather than a character at a
  time. Where an accent goes cannot be seen one character at a time: the letter
  and the mark have to be in front of the shaper together.
  *Not done:* cursive attachment, which is what joins Arabic at the right
  height; the contextual and chaining kinds, which `GPOS` has its own copies of
  and which nothing here reads yet; the tables of per-size corrections, which
  are for screens of a stated number of dots to the inch; and the mark
  filtering sets, which are the other half of the flag that keeps one group of
  marks. Named here rather than given an item of their own: each is a lookup
  kind on the same walk, and the next script work — **E1** — needs none of
  them.

- [x] **E1. Indic reordering: Devanagari.** The syllable, which consonant of it
  the rest hangs on, and the order its pieces are drawn in — with the font's
  own forms asked for in the order the format lays down, around that.
  *Done when:* the vowel sign of कि comes out before the consonant it is
  stored after, and the र् of a cluster comes out at the end of it.
  Every other script this program draws is drawn in the order it is stored.
  Devanagari is not, and no substitution table can say so: a substitution
  replaces glyphs where they stand. The text has to be rearranged first.
  Two things move. The vowel signs written to the left — ि and ॎ — are stored
  after the consonant and drawn before the whole cluster, not merely before the
  consonant: क्कि is drawn sign, half-form, letter. And a syllable that begins
  with र् is not a syllable beginning with an R: that R is a hook drawn over
  the *end* of the syllable, after the letter it hangs on and after whatever is
  written under that, and before the vowel signs written to the right.
  Which letter the rest hangs on is its own question. It is the last letter of
  the syllable — except that an R at the end of a cluster is a tail drawn under
  the letter before it rather than a letter of its own, and except that the
  hook at the front is not a letter at all. A joiner written after the halant
  asks for the letter rather than the hook, and is given it.
  The forms are asked for in the order the format lays down and each of its own
  part of the syllable: the dot that makes another consonant and the conjuncts
  first, while everything is still where it was written; then the hook, asked
  of the first two glyphs and nowhere else; then the half forms, asked only of
  what stands before the letter the syllable hangs on, and the forms written
  under and after it, asked only of what follows — because a font will spell a
  half form out of any consonant and a halant, and would make one where a form
  below the line belongs. Then the order changes. Then the forms that depend on
  that order: `pres`, `abvs`, `blws`, `psts`, `haln`.
  A glyph goes where the character it came from goes, and a form made of two
  characters carries the first of them — which is how the reordering reaches
  glyphs that no longer stand one to a character.
  The script has two names in a font: the scripts that reorder were given new
  tags in 2005 and a font may carry either, so the new one is asked for first
  and the old one is the fallback. And `abvm`, `blwm` and `dist` join the
  positioning features, which is where a font says how far above a letter its
  marks go.
  *Proved by:* the splitting and the order, in twenty tests that spell out the
  rule each one stands for; and, through the shaper itself, that the pieces
  come out in the drawn order with every glyph still saying which character it
  came from.
  *Seen drawn:* not at first. The build image carried DejaVu and nothing else,
  and DejaVu has no Devanagari at all, so the half forms, the conjuncts and the
  hook were asked for and nothing answered. A font went into the image with
  **E2**, and with it the tests that had been skipping themselves run and the
  proof render shows कि, क्क, र्क, र्कि and हिन्दी drawn as they are read.
  *Also not done:* the other nine scripts written this way — see **E13** — and
  the two things reordering leaves behind: a syllable whose sign has nothing to
  hang on, which Word draws round a dotted circle and this draws as it stands;
  and where the caret lands inside a syllable whose letters are drawn out of
  order, which is a question of clusters rather than of glyphs.

- [x] **E13. The other scripts that reorder.** Bengali, Gurmukhi, Gujarati,
  Oriya, Tamil, Telugu, Kannada, Malayalam and Sinhala. They share the shape of
  the rules **E1** now has — a syllable, a letter the rest hangs on, pieces
  drawn in an order of their own — and differ in every detail of them: where
  the hook goes (some put it at the front, some after the base, some at the
  very end), which consonants take a form below the line, and the vowel signs
  written in two pieces, one either side of the consonant, which have to be
  split before the font is asked anything. Each needs its own reading of the
  same tables, and the block of each is a hundred and twenty-eight characters
  to be categorised by hand as Devanagari's was.
  *Done.* **One engine for ten scripts.** `wp-shape/src/indic.rs` is no
  longer Devanagari's alone. Every character of the ten blocks is told
  apart by hand — consonant, Ra, independent vowel, vowel sign and where
  it is drawn, nukta, halant, the dots that belong to the whole syllable,
  the letters a sign may hang on — and the syllable rules run over pieces
  rather than characters, because the vowel signs written in two parts are
  split first: Bengali's o and au, Oriya's ai, o and au, Tamil's o, oo and
  au, Telugu's ai, Kannada's ii, ee, ai, o and oo, Malayalam's o, oo and
  au, and Sinhala's ee, o, oo and au, whose second part is the al-lakuna
  and is a sign there and not a halant. Unicode's own decompositions, and
  the parts carry the character they came from, so a caret still lands
  between characters.
  **What each script answers its own way.** Which consonant the syllable
  hangs on: the last in the north, passing over one drawn as a tail or a
  sign of the letter before it — Ra everywhere, ya-phala in Bengali, the
  three beneath in Gurmukhi, ya, va, la and ra in Malayalam — and the
  first in Telugu, Kannada and Oriya, where the rest are written small
  beneath it, and in Sinhala, where a halant ends the letter unless a
  joiner asks the next to touch it. Where the hook goes: every piece of a
  syllable is given a place in one sequence — the signs before, the
  letters before the base, the base, the forms beneath, the signs above
  and below, the letters after, the signs after, the dots — and the hook
  takes the place its script gives it: before the signs after in
  Devanagari and Gujarati, after the signs below in Bengali, right after
  the base in Oriya and Malayalam, after everything but the dots in Tamil,
  Telugu, Kannada and Sinhala. Whether there is a hook at all: only when a
  joiner asks in Telugu and Sinhala, and only when the font can make one
  anywhere — a font with no rule for it draws the Ra as the letter it is,
  which is what every shaper does and what this one did not. Malayalam's
  hook is a character of its own, written first and drawn after the
  letter, and is placed so. A consonant the font draws before the base —
  Malayalam's ra — is found by asking the font's `pref` rule and moved in
  front of the base, after the signs written to the left.
  **The fonts are in the build image** — a Lohit for each of the eight and
  LKLUG for Sinhala — so the rules could be looked at: the proof render
  shows a line of each script with a sign written to the left or in two
  parts, a cluster of two consonants, and a hook where the script has one.
  *Proven by:* `indic.rs` — every block's consonants, vowels, signs and
  halant told apart; a sign in two parts split with both parts carrying
  their character and the first drawn first, in Bengali, Tamil, Malayalam
  and Sinhala; Telugu and Kannada hanging on the first consonant, Telugu's
  hook only when asked; the hook drawn where each script draws it;
  ya-phala, va and the rest passed over when the base is sought; Sinhala's
  halant ending the letter unless joined; a pre-base form drawn before the
  base; every character coming out exactly once in every script; and the
  Devanagari tests as they were. `tests/real_font.rs` — with the fonts on
  the machine, a sign in two parts drawn round the consonant in six
  scripts, a cluster drawn as fewer glyphs than it has letters in seven,
  the hook one glyph drawn after the letter in Bengali and Gujarati, and
  a Tamil font with no hook drawing the Ra as a letter.
  *Not done, and named:* none of this has been checked against Word, or
  against another shaper, glyph for glyph; the reading of each script's
  rules is this program's own, from the standard and the fonts, and the
  proof render is what was looked at. What a font says about which
  consonant takes a form below or after the base is not asked — the
  lists are written here per script, where a shaper that asks the font
  would follow the font — so a font that draws a consonant differently
  from the list may hang the syllable on the wrong letter. The Sinhala
  hook made with a joiner, Gurmukhi's tippi and addak beyond their
  places, Malayalam's chillu letters beyond being consonants, and the
  older way of writing each script that the old tags name are read only
  as far as the categories carry them.
- [x] **E2. Thai and Lao: the syllable, and where a line may be broken.**
  *Done when:* a paragraph of Thai wraps, and wraps where a syllable begins
  rather than in the middle of one.
  Thai is written with no spaces inside a sentence, and everything here classed
  it as ordinary letters — which meant no break was allowed anywhere in a Thai
  run, and a paragraph of it ran off the page rather than wrapping. That is the
  defect this closes.
  Where a *word* ends cannot be known without a dictionary: which of several
  readings of a run of letters is meant is a question about the language, the
  standard says so outright, and Word ships one to answer it. This program has
  none and inventing one is not a thing a program may do. What can be known
  without one is where a *syllable* begins, and a break is offered there.
  Every word boundary is a syllable boundary, so no break that ought to exist
  is missed; some that are offered fall inside a word, which a Thai reader
  would not choose. A line broken inside a word reads badly and a line that
  cannot be broken at all runs off the page, so the trade is made deliberately
  and written down here.
  What never breaks: a vowel or a tone mark from the consonant it hangs on, in
  either direction. The vowels written *before* their consonant — เ แ โ ใ ไ —
  are stored in the order they are drawn, so unlike Devanagari nothing has to
  move, but the consonant after one belongs with it. And the vowels written
  *after* — ะ and า — take room of their own on the line and look like letters,
  which is exactly the trap: a line beginning with one begins in the middle of
  a syllable. The proof render caught that one before this was written down.
  Against anything that is not Thai or Lao the standard resolves these to
  ordinary letters, so a Thai word joined to a Latin one is one word.
  The marks are marks to the shaper now, which sends a run of them to be shaped
  whole: a Thai font draws its marks so that they land right where the pen
  leaves them for most consonants, and says where the vowel goes instead for
  the few that reach up into it — ป has an ascender, and the vowel moves aside
  by a tenth of an em. The tone mark over it then follows the vowel rather than
  the consonant, which is the mark-on-mark rule of **E12** doing its work.
  The same went in for the vowels and tone marks of Devanagari, which were not
  in the table of marks either.
  **And the build image has fonts now.** It carried DejaVu, which has nothing
  for either script — so the rules of **E1** and of this could be written and
  tested and *nothing could be drawn with them to look at*. Lohit Devanagari
  and the TLWG Thai fonts are the smallest pair that answers, they go in beside
  the `zip` and `unzip` the tests are already held against, and neither is
  linked into the product or shipped with it. With them, the tests that skipped
  themselves run: a Devanagari conjunct comes out as fewer glyphs than it has
  letters, the hook comes out as one glyph drawn after its consonant, and the
  Thai stack is placed by the font. And the proof render shows कि, क्क, र्क,
  र्कि, हिन्दी and three lines of wrapped Thai — which is the half of **E1**
  that stage could not close.
  *Not done:* the dictionary, and with it word-level breaking, double-clicking
  a Thai word and counting the words of a Thai paragraph — all the same
  missing thing, named under **E6** where the same dictionary is wanted for
  Chinese and Japanese.
- [x] **E3. Hyphenation: the hyphens a document carries.** The optional hyphen
  and the non-breaking hyphen — the two marks a writer puts inside a word to
  say where it may break and where it may not — read, written, drawn and
  broken at. And every one of Word's hyphenation settings read and written.
  *Done when:* a word with an optional hyphen in it breaks there and shows a
  hyphen at the break, and shows nothing at all anywhere else.
  The optional hyphen is the awkward one, because what is drawn for it depends
  on where the line ends. In the middle of a line it is nothing: no ink, no
  width, and a document full of them is set exactly as a document without them.
  At the end of a line it is a hyphen. So the mark keeps its place in the run —
  the caret can be moved over it and a click lands beside it — and draws
  nothing; and the *line*, when it breaks at one, draws the hyphen. It belongs
  to the line rather than to any glyph of it, which is what the code says.
  The room for that hyphen is made before the line is settled rather than
  after. A line is measured by what it would cost if it ended at this piece,
  and a piece that ends with an optional hyphen costs a hyphen more — measured
  afterwards, the hyphen hangs in the margin.
  Word writes both marks as elements rather than as the characters they stand
  for — `w:softHyphen` and `w:noBreakHyphen` — and neither was read: a document
  from Word lost every one of them, which is to say it lost every place its
  writer had said the word may break. Each is read as the character it means
  and counts as one character of the text, so every offset after it is right.
  The settings are all there now: automatic on or off, the zone, whether words
  in capitals are left alone, and the limit on how many lines in a row may end
  with a hyphen — where nought means no limit, which is what Word writes for
  it.
  *Not done:* hyphenating a word nobody marked — see **E14**.

- [x] **E14. Automatic hyphenation.** Breaking a word nowhere anybody said it
  may break, which is what Word's Automatic does and what this cannot do yet.
  It needs pattern data: the standard way is Liang's algorithm, which holds a
  few thousand patterns per language — `hy3phen`, `.ad4der` — and takes the
  odd-numbered ones as the places a word may break. The algorithm is a few
  dozen lines. The patterns are the whole of it, they differ per language, and
  this program has none: it cannot reach the network to fetch any, and a set
  invented here would break words where no dictionary of the language says
  they break, which is worse than not breaking them at all.
  So what this wants first is a decision about where such data comes from and
  where it lives — beside the program, in the image, asked for at runtime —
  and that decision is the same one **E6** needs for the Chinese and Japanese
  word dictionary and **E2** for the Thai one. Three items, one missing thing.
  What waits on it: the hyphenation zone, which says how close to the margin a
  line must come before a word is broken; the limit on consecutive hyphens,
  which is read and written and not yet obeyed because nothing here makes
  enough hyphens for it to bite; leaving words in capitals alone; the
  paragraph's own "never break the words in this one"; and Word's Layout tab
  menu — Automatic, Manual, Hyphenation Options — which is not there at all,
  because a menu whose two commands do nothing is worse than no menu.
  *Done.* **Where the patterns come from: the machine.** The decision this
  waited on was the one the spelling checker had already made: none are
  shipped and none are made up, and what is read is whatever the machine
  has — `hyph_en_US.dic` and the rest, the files LibreOffice and TeX keep
  their patterns in, found beside the program or under `/usr/share/hyphen`
  and looked up by the run's language, region first and then language. The
  build image has the English and German ones, on the same terms as the
  dictionaries. `wp-dict/src/hyphenation.rs` reads them: the encoding on the
  first line, the minimums, the comments, the two levels of a German file.
  **Not Liang's search but the library's machine.** The files were written
  for `libhyphen`, which LibreOffice hyphenates with, and it does not apply
  every pattern that occurs in a word as TeX does: it walks the word through
  a trie of the patterns, falling back to the longest suffix it knows, and
  applies only the pattern that is exactly the state it is in. A German file
  read TeX's way broke *Zusammenarbeit* into single letters where the
  library breaks it where the language does; so the machine here is the
  library's, its defaults are the library's, and forty words in two
  languages come out letter for letter as the library gives them — the
  answers were asked of it, not guessed. A two-level file is read the way
  it means: the first level finds the seams of a compound word, and the
  second is asked about each part on its own.
  **The layout obeys the four settings.** A word is shaped once, whole, and
  its glyphs cut at the places the patterns allow, so the kerning and the
  ligatures inside it survive; a line may end at a cut, with the hyphen
  drawn as an optional hyphen's is, only when carrying the word over whole
  would leave more room at the margin than the zone, and only while fewer
  lines in a row have ended with a hyphen than the limit allows. Words in
  capitals are left alone when the document says so, a paragraph that says
  to leave its words alone is left alone, and a field's answer is never cut.
  **The menu.** None, Automatic, Manual… and Hyphenation Options…, and each
  does what it says. Automatic lays the document out again. The options
  dialog has the four settings and Word's Manual… button. Manual walks the
  document offering each word the automatic rule would break — asked of the
  layout itself, laid out again with the rule on, so what is offered by
  hand is what Automatic would do — shown with every place it may break and
  the rule's choice chosen; Yes puts the optional hyphen in, No moves on,
  Cancel keeps what was marked. A machine without the patterns is told so.
  *Proven by:* `hyphenation.rs` — the pattern split, Liang's own example,
  the ends left alone, non-letters refused, the encoding followed, a second
  level; `tests/real_hyphenation.rs` — twenty English and thirteen German
  words as the library breaks them, and the files found by language;
  `wp-layout/tests/autohyphenation.rs` — words whole without the setting,
  broken with it and every letter still drawn once, the paragraph's own
  refusal, the zone keeping a word whole, the limit kept, capitals left
  alone; `editor/hyphenation.rs` — Automatic and None through the menu,
  the dialog writing all four, and Manual offering a word, marking it on
  Yes, passing it on No and stopping on Cancel.
  *Not done, and named:* only the languages the machine has patterns for;
  Word's own hyphenation is a lexicon of its own, so where it and the
  patterns differ the patterns win here. The patterns that replace letters
  at a break (`ff` to `ff-f` in German) are not read, and such a word is
  broken where the plain patterns allow or not at all. A cut that falls
  inside a ligature is not made. The zone is measured to where the word
  would have begun, a space's width short of Word's margin. Manual
  hyphenation offers a word once and walks from the top each time, which
  is right but slow on a long document; Word's cursor among the breaks is
  a list here.
- [x] **E4. Ruby: the reading printed over a word.** `w:ruby` — Word's Phonetic
  Guide — read, drawn, written, and kept through an edit.
  *Done when:* a document with 漢字 and かんじ over it shows both, at the right
  sizes, lined up the way the file asks.
  A ruby is two pieces of text, not one: each half has its own font, size and
  colour, and only the lower one is part of the sentence. So the word under the
  reading is what the document says there — what a search finds, what the word
  count counts, what the caret walks through in two steps and not five — and
  the reading is an annotation about it.
  That last part is where the work was. A ruby is *one piece* of the paragraph:
  the reading takes no offsets at all, and the word under it is not a place to
  type into. Typing at the end of 漢字 writes the next word; it does not make
  the ruby longer. Without that rule the second ruby put into a line lands
  inside the first one's base — which is exactly what the proof render showed
  before the rule was written down, as four readings shared out one kana each
  between four words.
  Laying it out is putting two lines of different lengths one over the other.
  Whichever is narrower is spread inside the room the pair takes, four ways as
  the file may ask: centred, against either end, or distributed — between the
  letters, or with a gap at each end as well, which is Word's default and what
  keeps a one-kana reading off the edge. Both halves are shaped through the
  same machinery as everything else, at the size their own runs ask for; a
  reading shaped a second way would drift from the words beside it.
  And the line is told: a reading sits above the line, so the line has to be as
  tall as the reading — or it is drawn over the words of the line before.
  **A Japanese font went into the build image** with this, for the same reason
  Devanagari and Thai fonts went in with **E1** and **E2**: the rules could be
  written and tested and nothing could be drawn with them to look at. The proof
  shows the four alignments side by side, and a short reading spread over a
  long word.
  *Not done:* Word's Phonetic Guide dialog, which is how a person makes one
  — the reading can be put in from the model and not yet from the ribbon. And
  the other half of what this item used to be: see **E15**.

- [x] **E15. Vertical writing.** Japanese set down the page rather than across
  it: `w:textDirection` on a section, a text box or a table cell, and
  `w:eastAsianLayout` for the words inside it that are turned or squeezed.
  This is not a feature of the text engine, it is a second engine. Every line
  in this program runs left to right along a baseline, wraps at a width and
  stacks downwards; vertical writing runs top to bottom along a *column*, wraps
  at a height and stacks leftwards — and the page, the margins, the columns,
  the tables, the drawings, the caret and every click land in a space with its
  axes swapped. The rest of it is detail on top of that: which characters are
  turned on their side and which are not, the Latin word set sideways inside a
  vertical line, the two-digit number set upright in one square
  (`w:eastAsianLayout` with `w:vert`), and the punctuation that changes its
  corner of the square.
  *Done when:* a Japanese paragraph set Vertical stands in columns from the
  right edge leftwards, with the kana upright, the Latin lying down and the
  full stop in its corner; and the same engine, given its pages back, gives
  the same pages.
  **It was not a second engine after all.** The turned cell of the table
  work had already found the shape of it: lay the text out straight into a
  box as long as the line has to be, and turn the box. A section written
  down the page is the same thing with the page as the box — the paper with
  its sides swapped round the corner the turn is about, so that the top
  margin is where the lines begin and the right margin is where the first
  of them stands, the columns divide the page's height into bands as Word's
  do, and a paragraph's left indent is measured from the top. Everything
  the box held is turned onto the page afterwards: the letters, the lines
  the caret walks, the rules and bands, the cells, the pictures, the shapes,
  the ink. The one thing that is not a turn is the letters themselves —
  every ideograph and every kana stands upright in a line that reads
  downwards, and only the Latin lies on its side — and that is where the
  work was. Which characters stand is Unicode's Vertical Orientation
  property, reduced to its ranges; a letter that stands is asked of the
  font in its vertical form (`vert`: the full stop moves to its corner, a
  bracket turns to open downwards, the long vowel mark and the ellipsis
  stand only if the font has a form for them), takes along the line what
  the font gives it *downwards* (`vmtx`, or an em without it), and is drawn
  beside the pen rather than at it — centred on the column, its top where
  the vertical origin says (`VORG`, or the typographic ascender). The pen's
  place is what a click and a caret are measured against; the shift is only
  where the drawing lands. So a caret in a vertical line lies across the
  column, a click lands in the right character, and the arrow keys are read
  as what they do on the page: down is along the text, left is on to the
  next column, which is Word's way.
  **The engine can be given its pages back.** The pages it gives out are
  turned; the pages it goes on placing text into are the box. So a page
  given back is turned back into its box on the way in and turned again on
  the way out, and that round trip is exact — a subtraction from the page's
  edge, undone by a subtraction of two numbers a whisker apart — which is
  what keeps the pages of a document written down the page the pages a new
  engine gives, whatever the engine has been through. Footnotes go onto the
  box before it is turned, so they stand at the left of the page and run
  down it; headers and footers go onto the paper afterwards, the ordinary
  way up.
  **The six directions.** Word's Vertical (`tbRl`) is writing: the East
  Asian letters stand. `btLr` and a text box's `vert270` turn the whole line
  anticlockwise; a text box's `vert` turns it clockwise, ideographs and all;
  `tbRlV` and `tbLrV` — which Word draws alike, whatever the standard says
  of the first — read downwards with the lines going across to the right,
  Mongolian's way, which is the clockwise turn with its lines mirrored, and
  a frame carries that flip beside its turn. The frames compose, so a
  turned cell on a turned page is turned twice, and undo, so a page can be
  turned back. A section reading upwards is read and kept and drawn across,
  which is what Word does with it.
  **The words inside.** `w:eastAsianLayout`: Horizontal in Vertical sets a
  run upright and across inside the column — a year, a page number — in the
  room of one square along the line, squeezed to the square across as well
  when it asks to fit in the line; Two Lines in One sets a run as two
  half-height lines stacked inside the height of one, with the brackets it
  names round the pair, which in a vertical line is two columns side by
  side. Both are one piece no line breaks inside, and both are the same
  shifts the upright letters use. The Asian Layout menu on the Home tab has
  the two dialogs with Word's OK, Remove and Cancel; Text Direction on the
  Layout tab sets the section. A text box's `wps:bodyPr/@vert` is read,
  written and laid out the same way, its words turned with the box and
  then with the shape. A turned or upright letter is written into a PDF
  with a text matrix that says so, which turned cells were not before.
  *Proven by:* `wp-shape/src/vertical.rs` — which characters stand, which
  lie down, and which depend on the font; `wp-layout/tests/vertical.rs` —
  the direction written and read, the box of a turned page, the lines as
  columns from the right edge leftwards inside the margins, the kana upright
  and the Latin down with the full stop in its vertical form, a click and a
  caret agreeing down the column, the same pages after an edit as from a
  new engine, columns as bands, a year across the column in one square, two
  lines in one stacked inside the line; `wp-docx/src/eastasian.rs` — the
  attributes as Word spells them; `editor/vertical.rs` — the section set
  from the menu and back, the arrows walking the column, both dialogs
  writing and removing. The proof picture shows a page of it with the
  Japanese font the build image has.
  *Not done, and named:* `lrTbV`, horizontal with the East Asian letters
  turned, is read and kept and drawn across. A text box's direction has no
  button of its own yet. The underline of a vertical line is drawn on the
  left of the column, where the turn puts it; Word's side was not checked.
  A ruby's reading in a vertical line lies down with the Latin. A drawing
  anchored in a vertical section is placed in the box's frame, so a
  position measured from the page's top is measured from its right. Line
  numbers on a vertical page stand the ordinary way up. Mongolian text is
  set in the Mongolian direction but its letters are not joined, since the
  shaper has no rules for the script. The lower line of two lines in one
  has no room of its own along the line for the caret, which lands after the
  pair. The gallery's third entry is named here and not by Word, whose own
  words for the section directions were not to hand.
- [x] **E5. Case mapping with language tailoring.** The capital of a letter is
  not the same everywhere, so both places this program makes one — Word's
  Change Case, which rewrites the text, and `w:caps`, which only draws it —
  ask the document what language the letters are in first.
  *Done when:* the same eight letters give İSTANBUL in a Turkish run and
  ISTANBUL in an English one, from the same button.
  Four rules, and each is about the writing rather than about the letters.
  In Turkish and Azerbaijani the dotted and the dotless i are two letters, not
  two shapes of one: the capital of `i` is `İ` and the small letter of `I` is
  `ı`, and a Turkish word put into capitals the English way says a different
  word. Greek drops its accents in capitals — άνθρωπος is ΑΝΘΡΩΠΟΣ — and keeps
  the dialytika, which is not an accent but a mark saying two vowels are read
  apart. Lithuanian keeps the dot on an i under an accent where every other
  language drops it, so the dot is written back in as a mark of its own.
  And two rules belong to no language: ß becomes SS, one letter becoming two,
  and a sigma at the end of a word is written ς. Both of those the standard
  library already knows and both are left to it — the point of naming them here
  is that they must keep working under the three tailorings above, which is
  what the tests hold them to.
  The two halves have to agree. Change Case rewrites what the document says;
  `w:caps` leaves the text alone and draws capitals over it. A Turkish word
  drawn one way and written the other looks right until somebody turns the
  capitals off — so the drawing goes through the same rules, one letter at a
  time, and a test holds the letter-at-a-time answer to the word-at-a-time one.
  All five of Word's buttons ask: not only UPPERCASE and lowercase but
  Capitalize Each Word, Sentence case and tOGGLE cASE, each of which makes a
  capital or a small letter somewhere.
  *Not done:* the same question asked by everything else that folds case. A
  case-insensitive search folds with the standard's rules and not the
  document's, so searching a Turkish document for "istanbul" will not find
  "ISTANBUL" — the fold wants the same tailoring, and where it should come from
  for text that spans runs in two languages is the part worth thinking about
  rather than the arithmetic. And the rest of Lithuanian, whose full rules run
  to a dozen cases of which the two common ones are here.
- [x] **E6. Words in the scripts written without spaces.** What can be known
  without a dictionary — which is more than was here, and less than Word has.
  *Done when:* a page of Japanese is not counted as one word, and a double
  click in it takes what the rules say a word is.
  Han and hiragana were classed as letters, which made every rule that joins
  two letters join them: 私はガラスを食べられます came apart as three pieces
  instead of ten, a double click took five characters at once, and a page of
  Japanese counted as **one word**, because counting the gaps in a language
  that has none gives one.
  The standard says otherwise, and says the only thing that can be said without
  a dictionary: each ideograph and each hiragana is a word of its own, and
  katakana holds together — which is Japanese marking its own word boundaries,
  since a borrowed word is written in katakana from end to end. That is now
  what the rules say, and with it the double click, `Ctrl` and an arrow, a
  whole-word search and the word count all change together, because all four
  ask the same question.
  The counting moved to where the question is answered. It used to split on
  spaces in the drawing code; it is now one count of what the segmentation
  says, which is why it can be right for both kinds of language at once. It
  agrees with Word for Chinese and Japanese, where Word counts the characters
  too — a Japanese document shows a far larger count than an English one of the
  same length, and that is not a mistake in either program.
  *Not done:* the dictionary. 私 and は are one word to a reader and two to
  these rules, and telling those apart is a question about the language rather
  than about the letters — the same missing thing as the Thai word breaks of
  **E2** and the hyphenation patterns of **E14**. Three items, one decision:
  where data of that kind comes from and where it lives. Until then this
  program says what the letters say, and says it the same way everywhere.
- [x] **E7. The full Unicode tables.** The subsets written by hand for bidi,
  breaking, segmentation and normalization become generated, committed tables
  covering every character.
  *Done when:* a character in a script nobody thought to write down behaves as
  the standard says, rather than as though it were English.
  Four tables were written by hand, from the standard, one range at a time, and
  every one of them was a subset with a default underneath it. Everything the
  bidirectional table did not name read left to right. Everything the line
  break table did not name was a letter, so a page of ideographs from any plane
  but the first was one unbreakable word. Everything the grapheme table did not
  name stood on its own, so a caret walked between a Telugu consonant and its
  vowel sign. And the normalization table held Latin, Greek and Cyrillic, so a
  search for a Vietnamese word missed it if it had been typed the other way.
  They are now generated. `tools/unicode/generate.rs`, run by
  `tools/generate-unicode-tables.sh`, reads the character database and writes
  `crates/wp-bidi/src/tables.rs`, `crates/wp-break/src/tables.rs`,
  `crates/wp-segment/src/tables.rs` and `crates/wp-normal/src/table.rs`. The
  database comes from Perl, which carries the whole of it already parsed into
  files of ranges, and the build image carries Perl: nothing is downloaded and
  nothing is installed. The output is committed, so the program still builds
  from its own source and from no crates at all.
  Each table is written as the runs it falls into — where a run begins, and the
  value holding until the next one — which is what makes "every character"
  structurally true: there is no gap left for a character to fall into, and a
  test walks all 1,114,112 of them to say so.
  What that added, in the order it shows: the right-to-left scripts nobody had
  listed — N'Ko, Samaritan, Cypriot, Adlam — and the unassigned code points
  that take their block's direction, so a line of Hebrew with a hole in it
  still reads right to left; the ideographs above U+FFFF, the Yi syllabary and
  Hangul, which now wrap; the vowel signs of every Indic and South East Asian
  script, which now belong to their consonant; the digits of every script,
  which now hold together across a full stop; and the whole of canonical
  normalization — Vietnamese, the Hebrew and Arabic points, the characters that
  are simply another character, and the eleven thousand Hangul syllables, which
  are not a table at all but arithmetic and are done as such.
  A Korean font went into the build image with it. The line breaking rules now
  separate Hangul syllables, and without a font that draws them the one thing
  the generated table added there could not be looked at — the sample
  document's own Korean line drew nothing at all.
  *Not done:* the line breaking classes are still this program's own and fewer
  than the standard's. Each of UAX #14's is folded into the nearest of them
  by a list in the generator, so what the folding costs can be read off it, and
  **E16** is the rest.
  *Not done:* the compatibility decompositions. A superscript two is not a two
  and a ligature is not its letters; NFKC and NFKD change what the text says,
  and nothing here asks for them.
  *Not done:* the version. The database in the build image is Unicode 14.0.0,
  so characters added since are unassigned as far as these tables are concerned
  — which is the standard's own answer for them, but not the current one.
  The conformance suites were part of this item's wording and are **K3**, which
  is where they belong: they are separate files — `BidiTest.txt`,
  `BidiCharacterTest.txt`, `LineBreakTest.txt`, `GraphemeBreakTest.txt`,
  `WordBreakTest.txt`, `NormalizationTest.txt` — and Perl does not carry them,
  so running them needs the same decision about where data comes from that
  **E2**, **E6** and **E14** are waiting on.
- [x] **E16. Line breaking with the standard's own classes.** UAX #14 names
  about forty and this program kept seventeen, folding the rest into them by a
  list in `tools/unicode/generate.rs`. What the folding cost, named: the half
  of LB9 that gives a combining mark the class of the letter it is drawn on —
  the other half, that a mark may not begin a line, was kept; LB30b, which
  forbids a break between an emoji and its skin tone, was approximated by
  making the tone a non-starter; B2, the em dash, allows a
  break before it as well as after and here allowed only after; SY, the solidus,
  was broken after even between two digits, where LB25 forbids it; and the
  Korean jamo were letters, so a syllable spelled out in them was never broken
  anywhere. The item is the standard's own class set and its pair table, and
  the rules written against them rather than against a fold.
  *Done when:* the Consortium's `LineBreakTest.txt` for the version the
  tables are generated from passes, but for the cases this program tailors on
  purpose and says so.
  **The classes are the standard's, all forty-three by their own two-letter
  names**, generated from the database with nothing folded; beside them the
  East Asian width of every character, which is the one thing LB30 asks
  about a bracket, and the two sets the rules name — the South East Asian
  letters that are marks, which LB1 reads as CM, and the code points reserved
  for pictographs and not yet assigned, which LB30b keeps a skin tone with.
  LB1 is applied as the rules read the text, so the table still says what
  the database says.
  **The rules are LB2 to LB31 in the standard's order**, applied to a line
  whole rather than to a pair at a time: LB9 attaches a mark or a joiner to
  the character before it and the rules then see that character, LB10 makes
  a mark with nothing to attach to a letter, LB8 and LB14 to LB17 look back
  through a run of spaces, LB21a looks past the hyphen to the Hebrew letter
  before it, LB30a counts the regional indicators before a break so that a
  flag is never halved, and LB25 is the standard's own example of a
  tailoring — a number is `(PR|PO)? (OP|HY)? NU (NU|SY|IS)* (CL|CP)? (PR|PO)?`
  and nothing inside one is broken — because that, and not the bare pairs,
  is what the standard's test data expects: a per cent sign holds to a
  closing bracket only where there is a number for both to belong to. The
  pair the layout asks about where two runs meet is answered the same way,
  as a line of two.
  **The Thai tailoring stays.** The standard hands the South East Asian
  scripts to a dictionary; this program has none and breaks Thai and Lao
  where a syllable begins, which is where every word begins and some places
  besides, exactly as **E6** wrote it down. It is the one case in the
  Consortium's file that fails, on purpose, and the conformance command's
  own note says so.
  *Proven by:* `LineBreakTest.txt` 14.0.0, the version the tables come from,
  run through `./x.sh conformance`: 7654 cases, 7653 passed, the one the
  Thai tailoring; and the crate's own tests — the classes as the database
  gives them, a mark taking its letter's class, a skin tone held to its
  emoji and to nothing else, an em dash broken before as well as after, a
  solidus held inside a fraction and not outside one, a jamo syllable held
  and two broken apart, two regional indicators one flag, a Hebrew hyphen
  held, a narrow bracket held to its word and a wide one not, a run of
  spaces looked through, a joiner holding two emoji, a sign held to a number
  and to nothing else, with everything the fold's tests held to still held.
  *Not done, and named:* the file itself is not in the repository — it is
  test data, and **K3** says where it goes and why. The dictionary Thai
  wants is still the dictionary Thai wants.
- [x] **E8. CFF and CFF2 outlines.** PostScript-flavoured fonts, which a good
  many documents ask for.
  *Done when:* a document set in an `.otf` file is drawn, and written to a PDF
  that draws it too.
  There are two places a font may keep its outlines and this program read one
  of them. `glyf` holds quadratic curves over a grid of points, which is what
  every font a Windows machine ships with uses. `CFF` holds cubic curves as a
  program per glyph in a little stack language, which is what every font Adobe
  ever made uses, and most of the fonts a designer buys, and every `.otf` file
  whose signature reads `OTTO`. Asked for one of those, the reader answered
  that it could not.
  It reads them now. `crates/wp-font/src/cff.rs`: the INDEX and DICT structures
  the table is built from, the private dictionaries a glyph's subroutines live
  in, both kinds of glyph lookup — plain, and CID-keyed where the glyph itself
  says which dictionary it belongs to — and the charstring interpreter. That
  last is the work: the operators are all relative, several take any number of
  arguments and alternate between horizontal and vertical as they go, the width
  of the glyph may be hidden in front of the first operator's arguments so that
  the operator has to count what it was given, and a charstring may call
  subroutines numbered from the middle of their list outwards. The five
  spellings of a curve are here, the four flex operators, and `seac` — the old
  way of writing an accented letter, which names a letter and an accent rather
  than drawing either, and without which half the alphabet of half of Europe
  draws nothing.
  `CFF2` is the same language with the header and dictionaries rearranged. It
  is read at its default instance: `blend` keeps the values and drops the
  deltas, which is what the font says before any axis is moved. How many deltas
  there are is not something the operator says, so the store of variations is
  read far enough to count them — a reader that guesses unwinds the stack
  wrongly and draws rubbish rather than nothing.
  Three things had to change around it. An outline command can now be a cubic
  curve, and everything that draws one — the page, a metafile's text — handles
  it. The font catalogue looked for `glyf` to decide whether a face was usable,
  so not one of the thirty-five PostScript fonts in the build image was in the
  list at all. And the rasterizer measured how far a cubic strays from a
  straight line by its third difference, which is not what says so: a quarter
  of an O came out as five straight lines. It is measured properly now, against
  a stated tolerance of a tenth of a pixel. That was wrong for every cubic in
  every drawing as well, and had gone unnoticed because a drawing's curves are
  short and a letter's are not.
  A PDF gets the font whole. It cannot be cut down the way the other kind is —
  a glyph is a program sharing subroutines with the others, and taking some out
  means rewriting them — so it goes in entire, under the key that says what it
  is: `FontFile3` with `/Subtype /OpenType`, in a `CIDFontType0` descendant
  with no glyph map, because there the number in the text is the glyph already.
  A reader told the wrong kind draws a blank page.
  The URW set — the thirty-five fonts every PostScript printer has — went into
  the build image, because a reader of this kind cannot be believed without a
  real font somebody else produced: subroutines calling subroutines, flex, a
  width where the reader did not expect one. Every glyph of Nimbus Roman is
  drawn in a test, and a hand-built `CFF2` table with a `blend` in it covers
  what no font on the image has.
  *Not done:* cutting a PostScript font down for a PDF, which is **E17**. A
  document in one carries the whole typeface rather than the dozen glyphs it
  uses.
  *Not done:* moving an axis of a variable font, which is **E9**. `CFF2` is
  read at the instance the designer drew.
  *Not done:* hinting. The stem hints are counted, because the mask that
  follows them cannot be skipped without the count, and then thrown away — this
  program does not hint either kind of outline.
- [x] **E17. Cutting down a PostScript font.** A PDF carries the fonts it
  needs, and the TrueType ones are cut down to the glyphs the document uses; a
  PostScript one went in whole, which is a megabyte where twenty kilobytes
  would do. Cutting one means rebuilding the `CFF` table: keeping the
  charstrings that are wanted, following the subroutines they call, renumbering
  what is left against the bias, and writing the INDEXes and dictionaries back
  out. The glyph numbering must not move, because the page refers to it.
  *Done when:* a page set in a PostScript font carries a few kilobytes of it,
  and every reader draws the page as before.
  **Every glyph asked for is run, the way it would be drawn**
  (`crates/wp-font/src/subset.rs`), and every subroutine it reaches is noted
  with the place in the charstring its number was written. An accented letter
  written the old way — `seac`, four numbers and `endchar` — brings the letter
  and the accent it names. The subroutines nobody reached are dropped, the rest
  numbered again from nought, and every call rewritten with its new number,
  counted from the middle of the new list, whose bias is not the old one's.
  A call is rewritten only where its number was written as a number right in
  front of it and every run through it lands on the same new number — which is
  how every subroutinizer writes one; where that is not so, the subroutines
  keep their numbers and the ones nobody calls are emptied, and a font whose
  charstrings cannot be followed at all goes in whole as before.
  **The glyph numbering does not move — where the page refers to it.** A font
  of named glyphs is named by place, so every glyph up to the last one wanted
  stays where it was, drawing nothing if it was not wanted. A CID-keyed font —
  every Chinese, Japanese and Korean one — is named by CID, and its own table
  says which glyph a CID is: so the glyphs kept are written one after another,
  each dictionary keeping its own subroutines cut the same way, and the table
  gives each glyph the CID that is its place in the whole font, which is what
  the page has always said. The difference is a quarter of a megabyte or six
  kilobytes for a line of Japanese whose glyphs are spread across sixty-five
  thousand. The cut table goes into an OpenType file with the header, the
  metrics and the counts cut to match; the character map is left out, as it
  is from a cut TrueType font.
  **What went into the PDF, and why that.** A font of named glyphs goes in as
  that OpenType file. A CID-keyed one goes in as the bare table, under
  `CIDFontType0C`: the format allows either and says a reader finds the glyph
  through the font's table either way, but FreeType does that only for the
  bare table and takes the CID for the glyph's place inside an OpenType file —
  and Poppler leaves it to FreeType. The first build of this drew the
  Japanese in MuPDF and nothing at all in Poppler; the bare table draws in
  both, and is what Acrobat has always written.
  **Sizes.** The URW fonts go from eighty to a hundred and ten kilobytes to
  ten for a line of text; Noto Sans CJK, a collection of 19.4 megabytes, to
  6.7 kilobytes for a line of Japanese. A page of seven lines in five
  PostScript fonts, Japanese and Korean among them, is a PDF of 48 kilobytes.
  **Noto Sans CJK went into the build image** for this, the regular sans
  only: the URW set is named glyphs and nothing else there is CID-keyed, and
  the cutting of a font split between dictionaries could not be believed
  without one.
  *Proven by:* `tests/cutting.rs` — a Latin font cut to a line, its glyphs
  drawn as the whole font draws them and its widths the same; every one of
  the thirty-five URW fonts cut five ways, every kept glyph drawn as before; a
  font for Japanese cut to a line and to a stride through all its
  dictionaries, each glyph found by its CID the way a reader finds it;
  `subset.rs`'s own tests on tables built for them — only the subroutines
  reached kept and the calls saying their new numbers, an accented letter
  bringing its two halves, a call whose number was not written in front of it
  keeping every number, a CID-keyed font whose CIDs were not its places cut
  to its glyphs and each named by its old place, a global call that would
  need two new numbers keeping every number, Type 1 charstrings refused, a
  truncated table refused without a panic; `wp-pdf/tests/document.rs` — a
  word of Nimbus Roman carrying under sixteen kilobytes of it, and a line of
  Japanese carrying the bare table cut to its glyphs, byte for byte, with its
  words read back out. And checked outside the repository, in a container
  thrown away afterwards: fontTools, whose charstring interpreter is not this
  one, drew every glyph of every font cut into the sample PDF and found each
  the same as in the whole font; Poppler and MuPDF both drew the page.
  *Found along the way, and done:* a font taken from a collection had every
  table looked up in the collection's first font, so a TrueType `.ttc` —
  MS Gothic, SimSun, Meiryo — could not be cut and was left out of a PDF
  altogether, text and all; its tables are now its own. `endchar` with the
  four numbers of an accented letter and no width in front of it took the
  first of them for a width and drew the letter without its accent; it now
  takes a width only from one number or five, as FreeType does. And text
  copied out of a page came back with the Kangxi radical where the ideograph
  was, because the font draws both with one glyph and the radical comes first
  in its map: the character a person writes is now preferred.
  *Not done, and named:* a `CFF2` font — a variable PostScript font — still
  goes in whole, and whether a reader draws it at all is the reader's
  business: a PDF has no key for it, and carrying one means setting the
  instance and writing it back out as `CFF`. The strings and glyph names of a
  font of named glyphs are copied whole rather than cut. A charstring using
  the arithmetic operators is not followed, and its font goes in whole. A
  glyph that is a ligature has no entry in the text a reader copies out,
  because what each glyph says is read back out of the font's own map, which
  a ligature is not in.
- [x] **E9. Variable fonts.** The axes, the named instances, and the deltas.
  *Done when:* a document set in a weight that exists only as a place on an
  axis is drawn at that weight, and measured at it.
  A variable font is one file that is a whole family. It has axes — weight,
  width, slant, optical size — and a pile of deltas saying how every point of
  every glyph moves as each axis is turned, so the file holds every weight
  between Thin and Black rather than nine of them. Windows ships several and
  every font Google Fonts serves is one. This program read the outlines the
  designer happened to draw and ignored the rest: a document set in Thin came
  out Regular, and nothing said so.
  `crates/wp-font/src/vary.rs` reads the lot. `fvar` names the axes and the
  named instances — the places the designer thought worth a name, which is what
  a font menu lists. `avar` bends an axis between its ends, so that the middle
  of Weight is where the designer says rather than halfway. `gvar` holds the
  deltas for the outlines. `HVAR` holds them for the advance widths, because a
  heavier letter is a wider letter and text measured without that breaks its
  lines in the wrong places. And `CFF2`'s `blend`, which was reading its
  operands and throwing them away, now spends them.
  Two parts of it are not simply reading. A delta is not stored for every
  point: a font stores them where the shape needs them and leaves the rest to
  be worked out from their neighbours, within one contour, each coordinate on
  its own. Get that wrong and a letter tears open at every point the font did
  not trouble to mention. And a delta does not apply everywhere: each belongs to
  a region of the axes — from here, peaking there, to there — and how much of it
  applies is a product across the axes of how far in the setting is. Both are
  here, and both are held to a real font rather than to a reading of the table.
  A composite letter varies twice over. Each piece varies as a glyph of its own,
  and the composite has deltas saying where each piece then goes — as a letter
  grows heavier its accent moves up to clear it. A glyph also stops being the
  size it says it is once its points move, so a varied one is measured rather
  than believed.
  The catalogue lists every named instance as a face. That is what makes a
  document work: it asks for "Inter SemiBold", which is a place on an axis and
  not a file, and Word lists them the same way. The style is read off the
  instance's own name rather than off the file's flags, because a font with a
  slant axis is upright where it stands and still holds an instance called
  Italic.
  Inter went into the build image for it: nothing else there is a variable font,
  and nothing about this can be believed without one. Nine weights of it are
  drawn in a test, every glyph at every named instance, and the widths are held
  to grow with the weight. `CFF2` has no such font to be held to — there is none
  on the image — so the hand-built table grew an axis, and blending is checked
  at the default, halfway along and at the far end.
  *Not done:* `MVAR`, the table that varies the font's own metrics — the
  ascender, the descender, the underline. A line of Black is set to the line
  height of Regular. It is a small table and the store that reads it is already
  here; what is missing is the tags and where each goes.
  *Not done:* `STAT`, which says how the instances of a family relate to one
  another. Nothing here needs it yet: the names come from `fvar`.
  *Not done:* setting an axis to a place the designer did not name. The
  machinery takes any coordinates, but nothing in a `.docx` can say them and
  Word offers no way to ask — a document names a weight, and a weight is an
  instance.
- [x] **E10. Colour and bitmap glyphs.** Emoji, in colour, as Word draws them.
  *Done when:* a document with an emoji in it shows the emoji, on the screen
  and in the PDF, and a bare heart is still a heart.
  An ordinary glyph is an outline filled with the colour of the text. That is
  no use for an emoji: a yellow face with brown eyes is neither one shape nor
  one colour. Fonts answer it two ways and this program read neither, so a
  rocket came out as whatever monochrome outline some Latin font happened to
  hold — or as nothing.
  `crates/wp-font/src/colour.rs` reads both. `COLR` says a glyph is really
  several other glyphs drawn one on top of another and `CPAL` holds the
  palettes they are drawn in, which is what Windows does and therefore what
  Word draws. `CBDT` and `CBLC` hold a PNG per glyph per size, which is what
  Android does and what Noto Color Emoji is; a picture does not scale, so the
  font holds several sizes and the nearest large enough is taken and scaled.
  One palette entry number is not a colour at all: `0xFFFF` means "whatever
  colour the text is", which is how a layered glyph keeps part of itself in the
  document's own colour.
  Choosing the font is half of it. Two things decide, and they are Unicode's
  own: whether the character is drawn as a picture by default — a rocket is, a
  heart is not — and whether a variation selector after it overrides that.
  `wp_segment::drawn_as_emoji` is the first, generated from the character
  database like everything else there; the layout reads the selector. A run
  holding an emoji is therefore shaped in pieces, because shaping is a question
  about one font and the emoji comes from another than the run asked for.
  A line break had to be fixed for it. A combining mark was folded into "a
  letter", so a line could break between a character and the selector that says
  which face of it was meant — which split them into different runs and lost
  the answer. A mark is now what it is: something that may not begin a line.
  That is the half of UAX #14's LB9 that can be kept without knowing what the
  mark is drawn on, and it was wrong for every accent and every vowel sign as
  well, not only for emoji.
  A PDF gets the pictures as pictures. There is no outline to fill and no font
  to embed that would draw one, so an emoji written as text is missing from the
  page with nothing to say so; it goes in as a small image in the place the
  glyph would have stood, with the mask that says how see-through it is —
  without which every emoji would sit in a white square.
  Noto Color Emoji went into the build image, because none of this can be
  believed without a font that really does it. The layered side is held to a
  hand-built `COLR` and `CPAL` instead: there is no layered font to be had on
  this image, and Segoe UI Emoji belongs to Microsoft.
  *Not done:* `COLR` version 1 — the gradients, transforms and compositing a
  newer layered font may use. The list of plain layers is in the same place in
  both versions, so such a font still draws; what it loses is the shading. That
  is **E18**.
  *Not done:* `sbix` and `SVG `, the two other ways a font may hold a colour
  glyph — Apple's pictures and Adobe's drawings. Neither has a font on this
  image to be held to, and a reader nobody has ever run is a claim rather than
  a feature. Also **E18**.
  *Not done:* a layered emoji in a PDF. The font embeds like any other and the
  reader draws the glyph the font names as its base, which is one layer of
  several. Drawing them all means keeping every layer's glyph in the cut-down
  font and writing one text run per colour — small work, and not work to do
  blind.
- [x] **E18. The rest of what a colour glyph can be.** Three things **E10**
  left. `COLR` version 1: the gradients, the transforms and the compositing
  modes a layered glyph may be drawn with, which a font using them lost here
  and showed flat. `sbix`: Apple's table of pictures per glyph per size, the
  same idea as `CBDT` with the sizes listed rather than indexed. `SVG `: a
  drawing per glyph, which needs the drawing language rather than a table —
  `wp-svg` is already here and would be what reads it. Each needs a font of
  its kind on the build image to be held to, which is the first thing to
  settle; and with the layers understood, a layered emoji can go into a PDF as
  its layers rather than as its base glyph.
  *Done when:* a glyph of each kind draws on the screen and in a PDF as the
  format says it draws, held at points where the format says what colour it
  is.
  **The fonts, settled first.** No font of any of the three kinds is packaged
  for the build image's Debian, and data from anywhere else is not fetched
  (**K3**). fontTools is packaged, and is an implementation of the format
  that is not this one: so `tools/make-colour-fonts.py` has it write a
  `COLR` version 1 font, an `sbix` font and an `SVG ` font while the image is
  built, the way LibreOffice writes the `.doc` files the old-format readers
  are held to. Every glyph is in the private use area, so none stands in for
  a real emoji elsewhere, and what each glyph is drawn as is written down
  beside it in the script.
  **`COLR` version 1** (`crates/wp-font/src/paint.rs`): every one of the
  thirty-two paint formats read into a tree — layers, solid colours in the
  palette's colour or the text's, the three gradients and their runs of
  colour, a glyph's outline as a clip, another colour glyph drawn here with
  its own clip box, every transform turned into one matrix about its centre,
  and the twenty-eight composite modes — with the glyph's clip box. The
  angles of a sweep are written half a turn short, a bias of one, which is
  what lets the format's number reach a whole turn: fontTools writes them
  that way and so does every font made with it, though the draft of the
  specification this was first written from did not say so, and the first
  reading had every sweep start at -180°. A tree that points back into
  itself or grows past any real glyph is refused.
  **Painted** (`crates/wp-layout/src/colourglyph.rs`, on
  `crates/wp-raster/src/compose.rs`): each node is a picture the size of the
  glyph's box, kept only inside an outline, painted in a space its
  transforms move, put onto another by its mode — the Porter–Duff operators
  and the W3C blend modes, separable and not, premultiplied. The gradients
  are the standard's: a linear one's colours run parallel to its third
  point's line, a radial one is the two-circle gradient with the larger
  distance winning and nothing painted where the radius is not positive, a
  sweep runs anticlockwise from its first angle to its second and paints
  nothing past it; and their colours are mixed as the format requires, in
  linear light, premultiplied — which is why halfway from red to blue is 188
  of each and not 128, and why a test that expected 128 was the thing that
  was wrong.
  **`sbix`** is read into the same picture a `CBDT` glyph is — the nearest
  size, placed by its bottom edge, a glyph that is the same picture as
  another followed once, PNG, JPEG and TIFF decoded by the decoders already
  here. **`SVG `**: the document a glyph is in is found, decompressed with
  gzip where it is — `wp-deflate` reads gzip now, held to what the system's
  gzip writes — and the element `glyph` and its number drawn by a new
  `wp_svg::Document`: groups, `use`, every transform, rectangles with round
  corners, circles, ellipses, polygons and paths, fills of every CSS colour
  and of `currentColor`, linear and radial gradients in either unit and
  inheriting from the gradient they name, `opacity`, `fill-opacity`,
  `fill-rule`, `clip-path`, `style`. Every kind comes out as one
  `GlyphPicture` — pixels and where they go against the pen — which the
  screen draws and the PDF carries, cached by size and by the colour of the
  text, which a tree may name.
  **In a PDF**, a glyph drawn as a tree, a picture or a drawing goes in as a
  picture painted four times finer than the text, with its transparency; a
  glyph of layers goes in as its layers, one run of text each in its own
  colour, the layer glyphs carried in the cut font. What each stands for is
  said around it as the stretch's own text, and under a picture the glyph
  itself is written as text in no colour, which is what Poppler copies out.
  Checked outside the repository in a container thrown away afterwards:
  Poppler and MuPDF both draw a page of every glyph as the screen does, and
  Poppler copies each of them out as its character. That check found a fault
  in the test fonts rather than in the program — every glyph's left side
  bearing written as nought, which FreeType places outlines by — and the
  script now writes the true ones.
  *Proven by:* `wp-font/tests/newer_colour.rs` — every kind of tree, the
  transforms about their centres, the composites, the text's colour and the
  palette's opacity, the older layers in the same table, Apple pictures by
  size and by bottom edge with a duplicate and a TIFF, drawings found
  compressed or not; `wp-layout/tests/colour_glyphs.rs` — each glyph painted
  and held at points to the colour the format's own formula gives there, to
  within two levels: the three gradients and both extend modes, the turn,
  the skew, the composites, the text's colour, the borrowed glyph moved and
  clipped, half transparency, an Apple picture stood on the baseline, three
  SVG glyphs with their gradients, groups, uses and fades; `compose.rs` —
  the operators, the blend modes, the runs and the three shapes of gradient;
  `wp-svg`'s `document.rs` — colours, transforms, groups, gradients and their
  inheritance, `use`, clips, reach; the gzip reader against gzip; and
  `wp-pdf/tests/document.rs` — pictures drawn and masked, layers in order
  and in their colours with their glyphs carried, and every one said and
  written for copying. The proof picture shows every glyph of the three
  fonts.
  *Not done, and named:* a variable colour font's trees are painted at its
  default — the deltas a `COLR` table keeps for its variable paints are read
  past — and an `sbix` font's flag asking for its outlines to be drawn over
  its pictures is not read. An SVG glyph's strokes, text, filters, masks,
  patterns and embedded images are not drawn, nor a `viewBox` on its root;
  gradients in SVG are mixed in the ordinary scale, which is SVG's default,
  and `color-interpolation` is not read. In a PDF, a glyph of a font with no
  outlines to carry — Noto Color Emoji is pictures and nothing else — is
  said as its character only around the picture, which Acrobat reads and
  Poppler and MuPDF do not, so it does not copy out there; MuPDF turned the
  test fonts' private-use characters into U+FFFD, so what it copies of a
  real emoji was not seen. A turned line still draws a picture glyph as its
  layers, where it has them, and not otherwise.
- [ ] **E19. The first line beside a floating drawing.** The room for a line
  is worked out before `place_line` registers the drawing anchored in that
  line, so the first line of a paragraph that begins with a square-wrapped
  shape runs under the shape and the lines after it go round; Top and
  Bottom likewise places the first line before stepping down. Register the
  line's own drawings before its width is chosen, or lay the line out once
  more, with a bound on the passes. The wrapping test's helper skipped the
  first line, which is why it passed. Reviews R21, #10; **C45** did not
  name it.

## F — Proofing

- [x] **F1. Real dictionaries.** Reading the open dictionary formats — the
  affix rules and the word list — so that a language's inflections are known
  rather than a fixed list of words.
  *Done when:* "She walked quickly to the biggest houses" has nothing underlined
  in it, and none of those words is in any list.
  English has about fifty thousand words and about two hundred thousand forms
  of them. This program held a list and nothing else, so it underlined every
  plural, every past tense and every comparative anybody wrote — and a checker
  that underlines correct words teaches the reader to ignore the underlining,
  which is worse than no checker at all.
  A real dictionary is two files. The word list says `walk/DSG`: the word, and
  letters naming the rules it may take. The affix file says what each letter
  means — rule `G` puts `ing` on the end of anything not ending in `e`. Fifty
  thousand entries and a hundred rules cover the two hundred thousand forms,
  and a language with real morphology is possible at all.
  `crates/wp-dict` reads them. The format is Hunspell's, which is what
  LibreOffice, Firefox, Chrome and macOS all read and what every free
  dictionary is published in. A word is checked backwards: looked up as typed,
  and failing that, every rule that could have produced it is undone — take the
  ending off, put back what the rule stripped, and ask whether *that* is a word
  allowed to take the rule. A word may carry a prefix and a suffix at once, and
  a suffix may carry the right to another suffix, so the undoing goes two deep.
  The flags that say what a stem is are read too, and each of them is a wrong
  answer if it is not: a stem that is no word on its own, a spelling the
  language forbids though a rule would make it, a word that keeps its own case,
  a word that exists only inside a longer one. So is the rewriting the file
  asks for before a word is looked up, which every English dictionary uses for
  one thing — the curly apostrophe a word processor types is the straight one
  the word list holds, and without it every "don't" anybody writes is
  underlined.
  Compounding as far as the simple flags express it, which is what lets German
  write several nouns as one and have the result be a word.
  No dictionary is shipped, on exactly the terms no typeface is: they are data
  with their own licences. The program looks where the machine keeps them —
  beside itself, or where LibreOffice's are — and takes the one for the
  language being written in, or the one the reader opens by hand. Where there
  is none it checks no spelling, which is the honest answer for a program with
  no words.
  Two dictionaries went into the build image for the tests, because a reader of
  this kind cannot be believed against a dictionary written for the test: fifty
  thousand English stems are asked for one at a time, and the forms of them,
  and the misspellings that must still be caught.
  *Not done:* the suggestions. What to offer in place of a word nobody knows is
  **F2**, and the data it needs — the letters to try, the pairs to swap — is
  read and kept for it.
  *Not done:* the compound rules written as patterns rather than as flags,
  which is how a dictionary says "a digit, then a digit, then `th`". English
  uses them for the ordinal numbers, so `11th` is underlined and `eleventh` is
  not. Named as **F7**.
  *Not done:* dictionaries in an encoding other than UTF-8 or Latin-1. The file
  says which it is in, and one this cannot read is refused with the encoding
  named rather than read as rubbish — which would be a dictionary of words
  nobody ever typed.
- [x] **F7. The compound rules.** A dictionary may say how words join as a
  pattern over flags rather than as a flag on each word: `COMPOUNDRULE n*1t`
  is what makes `11th` and `21st` right and `11st` wrong. English uses it only
  for the ordinal numbers; Hungarian and Korean use it for the language. The
  reader keeps the flags and ignores the patterns, so a word a pattern would
  allow is underlined. Reading the patterns is a small matcher over the flags
  of each piece, and the pieces are already found for the simple flags.
  *Done when:* `11th`, `21st` and `1000th` are words by the English
  dictionary's own rules, and `11st`, `21th` and `1th` are not.
  The affix file's `COMPOUNDRULE` lines are read into rules of steps: a flag,
  and whether the piece carrying it comes once, may be left out (`?`) or may
  come any number of times (`*`). A flag longer than one character — a
  dictionary with `FLAG long` or `FLAG num` — is written in brackets, `(aa)` or
  `(1001)`, and read as the dictionary reads its flags everywhere else. The
  first `COMPOUNDRULE` line is how many follow and is not a rule, even where a
  count would also be a flag: English has a `1` flag and a rule count of `2`.
  A word is matched the way a pattern is: cut into pieces of at least
  `COMPOUNDMIN` letters, each a word of the list that is neither forbidden nor
  in need of an affix, and read piece by piece against the rule, keeping every
  place in the rule the pieces so far could have reached and giving the cut up
  the moment there is none — so a long run of figures costs a lookup per
  figure and no more. Only two pieces or more make a compound, and a word
  marked `ONLYINCOMPOUND` — English's `1th`, `2th`, `3th` — is a piece and
  nothing else. The pieces are looked up as written and then in small letters,
  as every other word is. No affix is stripped from a piece, which is what
  Hunspell does with these rules too.
  *Proven by:* `wp-dict`'s own tests, against the ordinals written out as
  English writes them and against rules made for the test: the count line
  that is a flag, a step left out, taken once, taken three times, a word that
  is only a piece, pieces too short by `COMPOUNDMIN` counted in letters rather
  than bytes (`haustür`), bracketed long and numbered flags, a forbidden word
  that may not be a piece; `real_dictionary.rs` against the English
  dictionary on the machine (`1st` to `2024th` right, `11st`, `12nd`, `13rd`,
  `21th`, `2rd`, `1th`, `3th`, `111st` wrong); and `wp-docx`'s checker
  against the same dictionary, which underlines `11st` and `22th` in a
  sentence and not `21st` or `11th`.
  *Not done:* the other compound options a language like Hungarian needs
  beside the rules — how many pieces a word may have (`COMPOUNDWORDMAX`), the
  letters that may not meet across a join (`CHECKCOMPOUNDPATTERN`), a piece
  repeated (`CHECKCOMPOUNDDUP`), the syllable counting of `COMPOUNDSYLLABLE`.
  Neither Hungarian nor Korean is in the build image, so the rules are proven
  against English and against rules written for the test, not against a
  language that writes all its compounds this way.
- [x] **F2. Spelling as Word does it.** As-you-type checking, the wavy line, the
  right-click list of suggestions, add to dictionary, ignore all, custom
  dictionaries, per-language settings, and the settings that turn it off.
  *Done when:* a right-click on a red underline offers the word that was meant,
  a French quotation in an English essay is checked as French, and a word added
  once is known in every document from then on.
  The wavy line was there and so was "add to dictionary"; what was missing was
  everything that makes a checker usable. A word underlined with nothing
  offered in its place is a word the writer has to spell for themselves, which
  is the one thing they could not do.
  The offers come from `wp_dict::Dictionary::suggest`. Nearly every misspelling
  is one slip — two letters the wrong way round, one left out, one too many,
  one struck for another — so every word one slip away is tried and the ones
  that are words are offered, in the order the slips happen, which is the order
  a reader wants them in: the first offer is the one that gets taken. Before
  those come the pairs the dictionary itself lists as common mistakes, and
  after them a space, because "thequick" is two words with the space
  forgotten. The offers keep the case of what was typed.
  The right-click menu puts them at the top, as Word does, then Ignore All and
  Add to Dictionary; the Spelling button walks the mistakes and offers the same.
  Ignore All lasts while the document is open. Add to Dictionary lasts for
  good: the word goes into `custom.dic` beside the settings, which is read back
  when the program starts and laid over every dictionary it loads — Word's
  `CUSTOM.DIC`, in the same place for the same reason.
  A document is not written in one language, and now each run is checked
  against the dictionary for the language it says it is in. The machine's
  dictionaries are found as languages turn up in the text, the one for the
  country first and any of the language failing that; a language with no
  dictionary is left unchecked rather than underlined from end to end. A run
  may also ask to be left alone altogether — `w:noProof`, for a line of code or
  a name in no language — which the Language list now offers as Word's dialog
  does, and the file keeps.
  The settings that turn it off are three, and they are Word's three. "Mark
  spelling mistakes as you type" is the program's and was there. "Hide spelling
  errors in this document only" is the document's, kept in its settings as
  `w:hideSpellingErrors` and honoured by whoever opens it next; its twin for
  the other marks, `w:hideGrammaticalErrors`, is read as well. And the one on
  the run, above.
  As you type means after every keystroke, and a document of fifty thousand
  words asked of the dictionary again at every keystroke is a document that
  lags. So each paragraph's mistakes are remembered against its text and its
  languages, and only a paragraph that is not what it was is checked afresh — a
  paragraph that moved keeps its answer and gets its new number.
  *Not done:* several custom dictionaries, and Word's dialog for choosing
  which of them a word goes into and which language each is for. There is one,
  it is for every language, and it lives where the settings do.
  *Not done:* Change All — putting the same correction in everywhere the same
  mistake was made. Each is corrected where it stands.
  *Not done:* the ranking Word gives its offers, which knows how common each
  word is. These are in the order the slips happen, which is right far more
  often than not and wrong where a rare word is one slip nearer than a common
  one.
- [x] **F3. Grammar.** A rule engine and the rules for at least one language,
  with the wavy line of its own colour and the explanation Word gives.
  *Done when:* "I could of gone" is underlined in blue, and a right-click says
  "Verb form" and offers "could have".
  A spelling checker sees nothing wrong with "could of": every word of it is
  spelled correctly. A reader sees it at once. The mistakes of this kind are
  the ones that survive a spelling check, and a document full of them is what
  a spelling check on its own produces.
  `crates/wp-grammar` is a rule engine and the rules for English. A rule is a
  pattern over words — a literal word, a choice of words, any word, a word
  beginning with a vowel sound — matched against a run of words with nothing
  but spaces between them, because punctuation ends the run: "I don't. No." is
  not a double negative. Each rule carries the name Word gives the mistake,
  which is the explanation shown, and a replacement where there is one right
  answer, written as a template over the words matched so that "He don't"
  becomes "He doesn't" and not "he doesn't".
  The rules: the article before a vowel sound, with the sounds that spelling
  hides — an hour, a university, a European, a one-off, an FBI agent, an 8;
  "could of" and its kin; the pronouns and the verbs that go with them, where
  the pronoun settles it; the double negative, pointed out and not rewritten,
  because which half to keep is the writer's to say; the words people confuse
  — "their is", "better then", "your welcome", "alot", "irregardless"; and the
  small "i". Every one of them is a mistake in the register a document is
  written in, and none of them needs a parser to find.
  They are applied where the text says it is English or says nothing, and not
  where it says otherwise or asks not to be checked: the same `w:noProof` the
  spelling honours. The document's `w:hideGrammaticalErrors` hides them and
  leaves the spelling marks, as Word does.
  The line is blue — its own colour in the theme, not the colour of the
  formatting marks — and red stays for spelling, so a glance says which is
  which. The right-click names the mistake, offers the one right answer where
  there is one, and offers to leave it be, which lasts while the document is
  open. The Spelling button walks these along with the rest.
  *Not done:* the grammar of the sentence. Whether a verb agrees with its
  subject when the subject is a noun rather than a pronoun needs to know which
  word is the subject, and that needs a parser and a part of speech for every
  word — a different order of thing, and one Word's own gets wrong often enough
  that half the people who write for a living turn it off. What is here is what
  a handful of words side by side can settle.
  *Not done:* the rules for any language but English. The engine is the same
  for every language; the rules are not, and each language's are a list of
  their own to be written by somebody who writes it. Named as **F8**.
  *Not done:* Word's style checks — passive voice, wordiness, clichés — which
  are advice rather than mistakes and are drawn in a third colour.
- [x] **F8. Grammar rules for other languages.** The engine in `wp-grammar`
  takes a pattern over words and says what it found; only English has a list.
  Each further language is a list of its own — the mistakes that show in a few
  words side by side, in that language, with what Word calls each — and a
  reader of the language to say which are mistakes. Russian, German and French
  first, being the languages this program's dictionaries already know.
  *Done when:* "в течении года", "seid Jahren" and "je ai" are underlined in
  blue in Russian, German and French text, each offering its correction, and
  none of them is underlined in a language it is not a mistake in.
  Each language is a module of its own in `wp-grammar` — English, Russian,
  German, French — and `check` takes the language a text is in: the rules of
  that language apply, only its primary tag counts (`de-AT` is German), and a
  language with no list is not checked at all. The checker in `wp-docx` runs
  every language a paragraph has, each over the stretches that say they are
  in it; a stretch that says nothing is English, as it was.
  Every rule is as narrow as the mistake, because a rule that fires on a
  correct sentence teaches the writer to ignore the blue line. So a rule names
  the words around the mistake that make it one. Russian: "в течении" before a
  span of time and not before "реки", "одел" before a garment and not before a
  child, "по приезду" and not "по окончанию" (which is right in "работы по
  окончанию"), "согласно графика", "обоими руками", "ложил", "ехай", "ихний",
  "более лучше", "никто иной как". German: "seid" before a span of time and
  not before "dem Ziel", the comparative before "wie" but not "mehr wie" (which
  is right in "nicht mehr wie früher"), "einzigste", "die selbe", "nichts desto
  trotz", "irgend jemand", "wieder Willen", "ein Paar Tage" but not "ein Paar
  Schuhe". French: the elision — "de", "le", "la", "je", "me", "te", "se",
  "ne", "que" and the words ending in it, before a vowel, but not before an h,
  which may be sounded, nor before "onze" or "oui", nor "la une", nor a letter
  named in capitals as in "de A à Z" — "si il", "ce est", "quelque un", "quand
  à moi", "sa va", "le plus meilleur", "malgré que", "pallier à", "voire même",
  "au jour d’aujourd’hui", and "quelque soit" pointed out without a correction,
  because which of "quel", "quelle", "quels" is right is for the noun after it
  to say.
  The engine gained two kinds of word in a pattern. A swap is a list of pairs:
  the word matches the first of a pair, and the correction may put the second
  in its place, which is how "ложил" becomes "клал" and "einzigste" "einzige"
  without a rule for each form. A test is a function the word is put to, which
  is how French says what begins with a vowel. A correction may also ask for a
  word in small letters, which is what one written onto another wants: "Das
  Selbe" is "Dasselbe".
  The names the rules give are English, as the interface's words are, and are
  translated as the interface is — a German reader of a French text sees
  "Elision" named in German. They are Word's names where Word's English
  grammar options have one for the same mistake ("Commonly confused words",
  "Comparisons", "Words split", "Verb form"), and plain where it has none
  ("Elision", "Word form").
  *Proven by:* every rule carries a sentence with its mistake in it, and the
  tests hold every rule of every language to finding that mistake in that
  sentence with the whole list running — which found an English rule that
  had never fired, "should of went", shadowed by "could of" and its kin — and
  to leaving nothing to find once every correction is made. Every corrected
  sentence is then looked up word by word in the dictionary for its language
  that the build image now carries, Russian and French as well as English and
  German, so that no correction puts a misspelling where a mistake was. The
  narrowness is held to the sentences where each rule must stay quiet ("в
  течении реки", "одела ребёнка", "ihr seid dem Ziel nahe", "ein Paar Schuhe",
  "le héros", "de A à Z", "à la une"). The checker's own test is one paragraph
  in four languages, each mistake found by its own language's rules and
  English words in a French stretch left alone.
  *Found along the way:* the spelling of German and French was mostly
  unreadable, and no test knew, because the one test that looked every word
  of a dictionary up looked up only English. Three things, all in `wp-dict`,
  all fixed here. A word listed twice was one word with both sets of flags,
  and German lists nearly every verb twice — once as a verb, once as a piece
  only a compound may use — so "gehen", "tun", "warten" were not words. Each
  entry is now asked on its own. A suffix that adds nothing was skipped as a
  rule that does nothing, and French makes every singular noun with one:
  "maison" is a stem that is no word until an affix is put on it, and the
  affix is an empty ending. It is a rule now. And the flags a suffix carries
  were read backwards for two suffixes in a row, and not at all for a suffix
  and a prefix — which is how French allows the "l’" of "l’homme" and the "j’"
  of "j’ai", through the suffix rather than the stem. Both now read the way
  Hunspell reads them. The whole-list test now looks up every entry of all
  four dictionaries, leaving out only the ones each affix file says are meant
  to fail — its forbidden misspellings and its compound-only pieces, which are
  a quarter of German's list — and at least ninety-seven in a hundred of the
  rest must be words. Of German's rest, every one is.
  Found as well: the names of the mistakes the checker reports — "Repeated
  word", "Not in the dictionary" and the grammar's own — were not in the
  message catalogue and came out in English in a German interface. They are
  listed now, with their German, and the status line translates them as the
  menu does.
  *Not done:* a reader of each language to go over its list. All three are
  written from the grammar books' lists of what is wrong, and held to each
  language's dictionary and to the sentences above where they must stay quiet
  — which is not the same as having been read by somebody who writes the
  language every day.
  *Not done:* the mistakes that need the sentence rather than a few words
  side by side — the French subjunctive after "après que", the German case
  after "wegen", Russian agreement of an adjective with a noun some words
  away. Those need a part of speech for every word, which is what the English
  list could not do either.
- [x] **F4. Thesaurus.** Shift+F7, the button on the Review tab, and Synonyms
  on the right-click menu: the words that mean what the word at the caret
  means, grouped by which of its meanings they share, with the opposites
  marked, and any of them put in place of the word with one choice.
  *Done when:* a right-click on "happy" offers "glad", and choosing it leaves
  "Glad" where "Happy" stood.
  The words come from whatever thesaurus the machine has, in the open format
  LibreOffice reads — `crates/wp-dict/src/thesaurus.rs` — on the same terms as
  the dictionaries: none is shipped, the one for the language of the word is
  used, and where there is none the program says so rather than showing an
  empty list. The file is eighteen megabytes, and an index beside it says where
  each word begins, so a word is read by seeking to it rather than by reading
  the file to find it. A thesaurus tells meanings apart — "bright" the lamp and
  "bright" the child — and so does the list, one heading per meaning with its
  part of speech.
  The Review tab's Proofing group now has Word's four: Spelling, Thesaurus, the
  marks, and the dictionary — the last renamed from "Word List", which it
  stopped being at **F1**. The function keys arrived with it, because Word has
  always had them and a person who has used Word reaches for them: F7 for the
  spelling, Shift+F7 for the thesaurus, F12 and Shift+F12 for Save As and Save,
  F1 for the help.
  *Not done:* the pane. Word's thesaurus is a pane down the side with a search
  box and a trail of the words looked up; this is a list under the button,
  which is what the program has for lists. A pane of its own is the same
  question as the styles and navigation panes already answered, and is worth
  asking again when the pane holds more than a list.
  *Not done:* a thesaurus for a language other than English on the build
  image, which is the same want as the dictionaries': the reading is the same
  for every language, and the file is not.
- [x] **F5. AutoCorrect and AutoFormat as you type.** The replacement table,
  the capitalisation rules, smart quotes, dashes, lists that start themselves,
  and the little box that lets a person undo one of them.
  **C19** made the mechanism and most of the rules; this is the rest of what
  Word does as you type, and the box.
  *Done:* the little box — Word's AutoCorrect Options button. Every correction
  is remembered as it is made (`correcting::Made`: where, what was typed, what
  was put, which rule); rest the pointer on the word and a box with a lightning
  bolt appears under it, drawn by the same `PasteBadge` as the paste button
  with a different drawing on it. Its list is Word's three lines: "Change back
  to “teh”" (or "Undo Automatic Capitalization", and so on by rule), "Stop
  Automatically Correcting “teh”" (or the rule's own line), and "Control
  AutoCorrect Options…". Change back is an undo while the correction is still
  the last thing done, and is reversed by hand afterwards — three words later,
  when Ctrl+Z would take the words first; Stop takes it back and turns the rule
  off, takes the pair off the list, or puts the word on the INitial CAps list,
  and writes the settings. The box forgets when the word is edited, when the
  correction is undone, at Escape, and when the next correction is made.
  Word's two "Automatically add words to list" boxes on the Exceptions tabs
  are real now: a capital undone straight after an abbreviation puts the
  abbreviation on the First Letter list, and two initial capitals undone put
  the word on the INitial CAps list, by Ctrl+Z or by the box.
  Numbered lists begun by typing start where the typing did: "7. " makes a
  list that begins at seven, through a `w:num` with a `startOverride` on the
  ordinary numbered list's definition (`numbered_list_starting_at`), reused
  when one begins there already. `*bold*` and `_italic_` take their marks away
  and put the formatting on. An address — a scheme, `www.`, or somebody at
  somewhere — becomes a link to itself. Three or more of one character on a
  line of their own, and Enter, become a line under the paragraph above: Word's
  six (`---` single, `___` heavier, `===` double, `***` dotted, `~~~` wavy,
  `###` triple with a thick centre); at the top of the document the line goes
  under the paragraph itself and a new one is made below it. Each of the four
  has its tick box on the AutoFormat As You Type tab and its switch in the
  settings file, and each is one gesture, so one undo takes it back.
  Undoing a gesture now puts the caret where it was before the gesture began
  rather than where the gesture's first change had moved it, which is what a
  correction taken back with Ctrl+Z needed: the caret after the space, not
  before it.
  `--picture … corrected` draws the box with its list open.
  *Not done, and named here:* Word's tables from `+---+---+` and Enter;
  its built-in heading styles from a line typed and entered twice, which
  Word itself ships switched off; its Math AutoCorrect tab, which waits for
  the equation editor; its AutoFormat tab, which reformats a whole document at
  once; and its Actions tab. The box appears when the pointer rests on the
  word; Word shows a thin blue bar first and the box when the pointer reaches
  the bar, which is one hover more than this does.
- [x] **F6. Translation.** What Word's Translate does, in so far as it can be
  done without sending the document to somebody else's computer.
  Word's button is a menu of three — Translate Selection, Translate Document,
  Translator Preferences — and all three send the text to Microsoft's servers.
  This program talks to nobody, so each does what can be done on the machine,
  and the module says why at the top.
  *Done:* `wp-dict::bilingual` reads a bilingual dictionary in dictd's
  format, which is what the free ones — FreeDict's, from the Ding and
  Wiktionary lists — are published in: the `.index` of headword, offset and
  length in dictd's base 64, and the `.dict.dz`, a gzip stream flushed every
  few kilobytes with a table of the pieces in its header, so an entry is read
  by inflating its piece and not the fifteen megabytes before it
  (`wp_deflate::inflate_piece`, a piece having no final block). An entry is
  read as the Ding dictionaries write one — the translations with their part
  of speech in angle brackets and their field in square ones, the note, the
  examples in quotes — and as it stands where a dictionary is written some
  other way. `installed()` finds them where dictd keeps them, by the two
  three-letter codes in the name, given back as the two-letter ones the
  document's languages use. The build image has FreeDict's English-German
  dictionary, on the same terms as the fonts and the spelling dictionaries:
  test data, never shipped; the real-file tests read entries from all over
  the file, including across piece boundaries.
  Translate Selection lists, under the button, what the selection is in the
  other language: the stretch as a whole where the dictionary has it ("give
  up"), else word by word, each word a heading with its senses under it —
  translation, kind of word, the dictionary's remark — and choosing a sense
  puts it in place of that word, in the word's case. The language it is from
  is the language of the text; the language it is to is what Translator
  Preferences says, from among the dictionaries the machine has, kept in the
  settings file, or the first dictionary from that language. No dictionary
  says so, and says which there are. Translate is on the right-click menu
  too, where Word has it. Translate Document is the glossary — a file of
  `source = target` lines applied through the document, which is the part of
  translating a document a machine does reliably — and says so.
  `--picture … translate` draws the list.
  *Not done, and named here:* machine translation of sentences, which
  nothing on a machine can do without a model, and which this program would
  not do by posting the document somewhere; a dictionary for another pair on
  the build image, which is the same want as the spelling dictionaries'; the
  Translator pane as a pane, with its own text box, for the reason **F4**
  gives for the thesaurus.
- [x] **F9. The rest of AutoFormat As You Type.** Word's list has three lines
  this program has not: a table made from `+---+---+` and Enter, its built-in
  heading styles from a line typed and entered twice (which Word itself ships
  switched off), and the thin blue bar the pointer rests on before the
  correction box opens, which is one hover more than this does. Named in
  **F5**.
  *Done when:* `+----+--------+` and Enter leave a table of two columns, the
  second twice the first, with the caret in its first cell; "Introduction",
  Enter, Enter leave a Heading 1 where that is switched on; and resting on a
  corrected word shows the bar, and the box only once the pointer reaches it.
  Tables: a line that begins and ends with a plus sign and has nothing but
  hyphens between each two becomes, at Enter, a table of one row in the line's
  place, a column for each run of hyphens, with the paragraph after it where
  Enter would have gone and the caret in the first cell. Each column is as
  wide as its run of hyphens was on the page — the distance between two plus
  signs as the line was laid out, in the line's own font — so the table's
  lines fall where the plus signs were, which is the point of drawing it that
  way; a line too long to be one line shares the width out by hyphens instead.
  `Document::replace_paragraph_with_table` puts it in, beside `insert_table`,
  which now shares the building of an empty table with it.
  Headings: Enter in an empty paragraph under a line that begins with a
  capital, does not end with punctuation, is one line long on the page and is
  a plain paragraph — no style, no list, not in a table — gives that line
  Heading 1, or Heading 2 for a line begun with one tab and so on to Heading 9,
  takes the tabs away, and leaves the caret where it was, on the empty
  paragraph under the heading: the second Enter made nothing. Off until it is
  switched on, as in Word, and never onto a style the document has locked.
  Both are one gesture, so one undo takes each back and leaves what was
  typed; both have their tick box on the AutoFormat As You Type tab, under
  Word's own names, and their switch in the settings file; and both are
  remembered like any other correction, so the box offers "Undo Automatic
  Table" and "Undo Automatic Heading Style" and to stop doing it. Taken back
  by the box after more typing, a table goes and its line of plus signs comes
  back; a table somebody has typed into is not offered back at all, because
  taking it back would take their words with it. A heading goes back to the
  plain line, tabs and all.
  The bar: resting the pointer on a corrected word shows a thin blue bar under
  its first letter, and nothing more; moving onto the bar — or near enough, a
  few pixels round it — turns it into the box. The box then stays while the
  pointer is on it or on the word, and goes with both when the pointer leaves;
  Escape puts either away.
  *Proven by:* `wp-app`'s tests type each one a keystroke at a time through
  the editor: the table's place, shape, caret and column widths — measured on
  the page and so under three thousand twentieths of a point for a line of
  fifteen characters, not spread across the page — and one undo; a table of
  one column; the switch; the box taking a table back after typing below it,
  and not offering one with a word in it. The heading, its level from a tab,
  one undo, the switch off by default, a sentence left alone, a line longer
  than a line left alone, and the box taking one back. The bar under the first
  letter touching the line, no box until the bar, the box staying on the word
  and going off both, Escape for either; and every older test of the box now
  reaches it through the bar. The recognisers have tests of their own, down to
  `+--++` and a capital in Cyrillic. `--picture … corrected-bar` draws the bar
  and `typed-formats` a heading and a table typed into the welcome document,
  both looked at.
  *Found along the way:* nothing the box says was in the message catalogue —
  "Undo Automatic Numbering", "Stop Replacing Fractions" and the rest came out
  in English in a German interface. They are listed now with their German,
  except the two that name the word they were about ("Stop Automatically
  Correcting “teh”"), which cannot be listed ahead of time and still come out
  in English. The dialog's note that the Exceptions dialog's two tick boxes
  were not drawn was out of date since **F5** and is gone.
  *Not done:* Word's exact heading rule, which it does not publish; this is
  the rule its help describes — a capital, no closing punctuation, one line —
  and a line Word would take and this would not, or the other way round, is
  possible.
- [x] **F10. Word's AutoFormat tab.** Reformatting a whole document at once
  rather than as it is typed: the command, the dialog that asks what to
  change, and the review of what it did. Named in **F5**.
  *Done when:* a text typed elsewhere with straight quotes, `- ` lists, a
  short line over a blank one and an address comes out of AutoFormat with
  curly quotes, real lists, a heading and a link; and with a review, every one
  of those is a tracked change that Reject All takes back.
  The command is Word's two, kept off the ribbon as Word keeps them and among
  the commands a person can put on it: AutoFormat…, which asks, and AutoFormat
  Now, which does not and is on Word's Ctrl+Alt+K. The dialog names the
  document and offers "AutoFormat now" or "AutoFormat and review each change",
  with Options… handing over to the AutoCorrect dialog open at its new
  AutoFormat tab.
  The tab is Word's: Apply — Built-in Heading styles, List styles, Automatic
  bulleted lists; Replace — the six rules the as-you-type tab has; Preserve —
  Styles. Its switches are its own, in the settings file beside the typing
  ones, and what it runs is the typing rules with the tab's switches in their
  place and none of the rules that are only about a word as it is typed — the
  replacement list, the capitals — which Word's AutoFormat does not apply
  either (`AutoCorrect::for_reformatting`).
  A paragraph's words are read as if they were being typed:
  `AutoCorrect::fixes_in` walks the text asking the quote rule at every quote
  and the word rules at every place a word ends, the end of the paragraph
  included, and gives back every change at once, none overlapping another;
  they are made from the last backwards, so each leaves the places of the rest
  where they were found. Then what each paragraph is: `- `, `* ` or `• ` in
  front makes a bulleted list and `1. ` or `1) ` a numbered one, `2. ` under
  `1. ` continuing it rather than starting another at two, the marker taken
  away as it is when typed; and a short line with a blank line under it — one
  line on the page, begun with a capital, not ended with punctuation — takes
  Heading 1, a level down for each tab in front of it, the blank line standing
  for the second Enter of the typing rule. Not in a table, not in a list
  already, and, while Preserve Styles is ticked, not a paragraph with a style
  of its own. It is one thing to undo.
  With a review, the same changes are made with changes tracked, and then
  Word's question: Accept All, Reject All, or Review Changes. Reject All is
  the AutoFormat undone. Accept All is that, and AutoFormat run again without
  tracking, which is the same result with no marks — and unlike accepting every
  change in the document it leaves somebody else's tracked changes for their
  own review. Review Changes leaves the marks and opens the Review tab, whose
  Accept and Reject go through them like any others; so does shutting the
  dialog, which loses nothing.
  *Proven by:* `wp-app`'s tests on documents built with no rules at all: every
  kind of change made in one pass and one undo taking all of it back; the tab's
  switches deciding and the typing switches having no say; no heading of a
  line with text straight under it or of a sentence; a styled paragraph kept,
  and not kept when the tab says not to; a review whose marks include
  insertions, deletions and formatting and which Reject All undoes to the
  letter; Accept All leaving no marks, and leaving another person's change
  undecided; Review Changes on the Review tab; a document with nothing to
  change told so and given no step to undo; Options… on the AutoFormat tab;
  Ctrl+Alt+K. `AutoCorrect::fixes_in` has its own tests over a sentence with
  every rule in it. `--picture … autoformat-dialog`, `autoformat-tab`,
  `autoformat-review` and `autoformat-marked` draw the two dialogs, the tab and
  a set of minutes after a review — struck straight quotes and curly ones put
  in, `3rd` raised, the dash, `*nobody*` bold with its stars struck, the link,
  the list markers struck and the lists drawn, the heading — all looked at.
  *Found along the way:* a paragraph's formatting was never a tracked change.
  A style, a list, an indent put on while changes were being tracked went on
  unrecorded, where Word records it as a `w:pPrChange` that Reject puts back;
  so a heading made in a tracked document could not be rejected. Every
  paragraph change is recorded now while tracking is on, through the same
  code a comparison already used to record a restyled paragraph, and the
  revision tests hold a style and a list to being recorded, rejected,
  accepted and kept through a save.
  *Not done:* the tab's "Other paragraph styles" and "Plain text e-mail
  documents", and the dialog's choice of General document, Letter or Email —
  Word's way of telling a letter's inside address and closing from a
  heading, which this does not attempt; a line is a heading here by the one
  rule above. Word's Style Gallery button on the review dialog. And a link
  made during a review is made outright rather than marked: taking it back is
  Reject All or the Insert tab's Remove Link, not the Review tab.
- [x] **F11. Math AutoCorrect.** The tab that turns `\alpha` into α as it is
  typed, and the list behind it. It waits on the equation editor, which is
  where the result has to go. Named in **F5**.
  *Done when:* `\alpha+` typed into the equation strip reads `α+` before the
  equation is finished, the tab's list can be added to and taken from, and
  `\alpha` in a paragraph is left alone until the tab says otherwise.
  The equation editor this waited on is the strip under Insert ▸ Equation,
  which takes the linear format a line at a time; its parser already knew the
  Greek letters by name when the equation was finished. What was missing was
  the list and the typing: the strip now makes a name its character the moment
  something that is not a letter follows it — `\alpha+` is `α+`, and `\beta`
  stays `\beta` until it is finished, since it could still be `\betaxyz` — so
  what the strip shows is what the equation will be.
  The list is Word's idea and its names are TeX's, which are Word's: the Greek
  alphabet with the `\var` twins (`\epsilon` ϵ and `\varepsilon` ε, `\phi` ϕ
  and `\varphi` φ, and so on), the operators, the relations, the arrows, the
  big operators and roots, and the odd letters mathematics writes with — a
  hundred and forty-five names. It is the settings file's, written whole like
  the replacement list, so a name taken off stays off.
  The tab is Word's second, between AutoCorrect and AutoFormat As You Type:
  "Use Math AutoCorrect rules outside of math regions", off as Word ships it;
  "Replace text as you type"; the two boxes and the list, with Add and Delete
  beside it working on this list and not the other, and keeping the name's
  case, since `\Delta` and `\delta` are two letters. With the first box
  ticked, a name finished in the text becomes its character like any other
  correction, and the box under it offers it back and offers to take the name
  off the list.
  *Proven by:* `wp-app`'s tests: names in the strip made characters as each is
  finished and not before, an unknown name left alone, the strip following the
  list's switch and a name added to it, and the equation put in; the text left
  alone by default and corrected with the switch on, the box taking it back;
  the tab adding and deleting on its own list in its own case and leaving the
  other list alone, and its two boxes read back; the list written to the
  settings file and read back whole. `AutoCorrect::math_word` has its own
  tests, among them that the name is the last backslash and letters only.
  `--picture … equation-typed` draws the strip part-way through
  `E = \sum \alpha_i \le \infty`, and `math-autocorrect` the tab, both looked
  at.
  *Not done:* the rest of Word's list — the accents, the double-struck and
  script alphabets, the brackets that build up, some hundred more names — any
  of which can be added on the tab; the ASCII pairs Word's list also has, such
  as `<=` for ≤, which here would turn every `<=` in a paragraph once the
  first box is ticked; and the Recognized Functions button, since the linear
  format here has no functions for `sin` to be set upright as.
- [x] **F12. Word's Actions tab.** The menu a word offers when the program
  recognises what kind of thing it is — a date, an address, a name — and the
  actions it hangs off that. Named in **F5**.
  *Done when:* with the Actions tab's box ticked, a right-click on "5 inches"
  offers "Convert to 12.7 cm" and does it, and a right-click on a date offers
  it written the other ways a date is written.
  Word's actions mostly hand what they recognise to somebody else: a date to
  the calendar, a telephone number to the address book, a stock symbol to a
  web page of prices, a person to the messenger. This program talks to
  nobody, and the module says so at the top. What is left is what can be done
  inside the document, and that is what is here. Word's Measurement Converter:
  a number with a unit after it — lengths, weights, volumes, speeds and
  temperatures, in their symbols and their names, singular and plural — is
  offered in the other system's units, the one or two that give a number a
  person would write, to three figures and with the decimal mark the writer
  used, or the machine's where they used none. And a date — in figures with
  hyphens, slashes or points, or with the month by name in English or in the
  machine's language — is offered written the machine's long way, its short
  way, and the international way. A unit has to end where a word does, so "5
  more" is not five metres of "ore", and "in" with a word after it is where
  something is rather than inches.
  The offers go on the right-click menu as Word's "Additional Actions", under
  a heading the way the synonyms are, and one chosen replaces what was
  recognised as one thing to undo. The Actions tab is the AutoCorrect
  dialog's last: "Enable additional actions in the right-click menu", off as
  Word ships it, and Date and Measurement Converter under "Available
  actions", each with its switch in the settings file.
  *Proven by:* `wp-app`'s tests: measurements found and not found — "5 more",
  "10 items", a number inside a word, "3 in a row" — conversions each way for
  every dimension with the decimal mark kept or taken from the machine, and
  amounts written as a person would; dates found in every form, day or month
  first as the machine writes them, the machine's own month names, and
  impossible or half dates refused; nothing recognised until the tab says so,
  nor away from the pointer. And through the menu: a conversion offered under
  the heading, chosen, and undone in one step; a date rewritten; nothing
  offered with the box off. `--picture … menu-actions` draws the menu over "5
  inches" with its two conversions, and with `german` in front, in German —
  looked at both ways.
  *Found along the way:* the right-click menu's own lines were never in the
  message catalogue — "Synonyms", "Add to Dictionary", "Insert Row", "Ignore
  All" and the rest came out in English in a German interface — because the
  list of messages is read from the source by the shape of the call, and a
  menu line names its command before its words. The reader now takes the
  words from there too, the menu looks a line indented under a heading up
  without its indent, and the German is in.
  *Not done:* Word's other recognisers — telephone numbers, financial
  symbols, instant-messaging contacts, places — and the Actions tab's
  Properties and More Actions buttons: each of them is a hand-off to another
  program or to the internet, which is what this program does not do.
- [x] **F13. The Translator as a pane, and a second pair of languages.** Word
  translates from a pane with its own box to type in, which is the same want
  **F4** names for the thesaurus, and it translates between more than the one
  pair on the build image — the same question as the spelling dictionaries':
  where the data comes from and where it lives. Machine translation of whole
  sentences is not part of this: it cannot be done on a machine without a
  model, and it will not be done by posting somebody's document somewhere.
  Named in **F6**.
  *Done when:* Translate Selection opens a pane with the word in its box and
  its senses under it, a German word typed into the box after turning the
  languages round is looked up in German, and French and Russian are among
  the languages the lists offer.
  The pane is down the right, where the others are and sharing the strip with
  them. At its top, From and To, each a list of the languages the machine has
  dictionaries for — To offering only what there is a dictionary into from
  the From — and the button between them that turns them round where the
  machine has the pair the other way. Under them the box, which holds the
  selection when Translate Selection opened the pane, or the word at the
  caret, or nothing; which takes the keyboard when it is pressed and gives it
  back when anything else is; and which is looked up as it changes. Under
  that what the dictionary said, as the list under the button said it: the
  whole stretch where the dictionary has it, else each word as a heading with
  its senses under it, a long sense wrapped rather than cut off. A sense
  chosen goes into the document — in place of its word while the box still
  holds what came from the document and the document still has it there, in
  the word's case, and at the caret when the box holds something typed. A
  language the machine has no dictionary from stays the language the words
  are in, and the pane says so and names the pairs there are, rather than
  reading them as something else. Translate Selection is the pane now; the
  list under the button it replaces is gone.
  Where the data comes from is where it came from before: dictd's folders, the
  folders the spelling dictionaries are looked for in, which include the one
  beside the program, and `%APPDATA%\dictd` on Windows. The build image now
  has German into English beside English into German, and FreeDict's English
  into French, French into English and English into Russian, which are small
  and written from Wiktionary — the other way FreeDict writes its entries.
  *Proven by:* `wp-app`'s tests against those dictionaries: Translate Selection
  opening the pane with the word, English into German, and a sense replacing
  the word; three words each listed and a sense replacing its own word; a
  word typed into the box — with a letter taken back — looked up, and its
  sense put in at the caret, the typing never reaching the page; the button
  turning English into German round and "Haus" found as "house"; the lists
  offering English, French and German, French into English finding "chat" a
  cat, and English into Russian answering in Cyrillic; no entry, and no
  dictionary from Italian, each said with the pairs there are; and the pane
  and the Text Pane taking each other's place and the page making room.
  `wp-dict`'s real-file tests read every one of the new dictionaries for words
  it must have and the word each must be. `--picture … translate` draws the
  pane opened from a word of the welcome document, and `translator-typed` the
  pane turned round with "Hund" typed in — both looked at.
  *Found along the way:* the German-English list sets a sense that begins with
  the field it belongs to in by a space — `[zool.] dog, dawg` under "Hund" —
  and the reader took translations only from the margin, so "Hund" was a mine
  car and a canine and never a dog. A labelled line where no translations have
  been read yet is the translations now; `wp-dict` has a test in that shape.
  *Not done:* the thesaurus as a pane, which **F4** named with this and is its
  own item's to do; the Translator's Document tab, which in Word posts the
  document to Microsoft; and more pairs than the image carries, which is a
  question of which dictionaries a machine has and not of the reader.

## G — The files Word can open

- [x] **G1. The other OOXML files.** `.docm`, `.dotx`, `.dotm`: templates and
  macro-enabled documents, which differ in their content types and their parts.
  The four are one package with one line of difference: the content type of
  the main part, which says whether the file is a document or a template and
  whether it may hold macros. A macro-enabled file carries them as
  `word/vbaProject.bin`, reached from the main part; the other two cannot,
  and Word refuses to write it into them.
  *Done:* `wp_docx::kinds` — `Kind` with its content type, extension and
  Word's label for each; `Document::kind`, `set_kind`, `has_macros`,
  `remove_macros`, `attached_template`, `attach_template`, `from_template`.
  `.dotm` was the one content type `wp-opc` did not know; it does now, and a
  relationship can be taken away. Saving picks the kind from the extension:
  the Save As list is Word's four in Word's order, opens on the kind the
  document is, and a name typed without an extension takes the extension of
  the type chosen, which is what "Save as type" means. A document saved as
  `.dotx` is a template from then on. Saving a document with macros as a
  kind that cannot hold them asks first, in Word's words, and then takes
  them out — the part, its type, and the relationship — rather than writing
  a file Word would not; saved as `.docm` or `.dotm` the macros survive the
  round trip untouched. The Open dialog offers the four together and apart.
  A template opened is a document made from it, which is Word's verb on a
  template: from the command line — the shell's New — and from the New page,
  which lists the person's own templates under Personal, from the folder
  Word saves them to (`Documents\Custom Office Templates`). The document is
  untitled, is a plain document whatever the template was (the macros stay
  in the template, which the document is attached to), is not counted as
  changed until something is typed, and remembers the template as Word
  writes it: `w:attachedTemplate` in the settings, through an external
  relationship to a `file:///` address. File ▸ Open on a template opens the
  template itself, for editing it, as Word's does. `wp new` writes whichever
  of the four the extension asks for and `wp info` names the kind.
  *Not done, and named here:* the styles updated from the attached template
  (Developer ▸ Document Template ▸ Automatically update), which is the other
  half of what the attachment is for; the Normal template — Word makes every
  blank document from `Normal.dotm` and keeps the person's defaults in it,
  where this program keeps them in the settings; a template's own building
  blocks, which are **J6**, and macros running, which is **J7**. The shell
  association that makes double-clicking a `.dotx` say New is the
  installer's to write, and there is no installer yet.
- [x] **G2. Plain text**, with encoding detection and the dialog Word shows when
  it is not sure.
  A `.txt` is bytes with no note of what they mean, and before Unicode every
  language had its own table of the bytes past ASCII. Word guesses where it
  can be sure and asks where it cannot, with a preview under each choice.
  *Done:* `wp-text`, a crate of its own: the single-byte code pages Word
  lists — the nine Windows ones, six ISO ones, KOI8-R and -U, and five DOS
  ones — as tables generated from Unicode's mapping files by
  `tools/generate-codepages.sh` (Python carries them; nothing is typed by
  hand, because a table typed by hand turns one letter into another
  somewhere and nobody notices), and UTF-8 and UTF-16 both ways round.
  Decoding never fails; encoding counts what the page cannot hold, and with
  substitution allowed stands the nearest plain character in — a straight
  quote for a curly one, a hyphen for a dash, e for é. Vietnamese, which
  keeps its tone marks as bytes of their own after the vowel, is written by
  taking the letter apart and putting it back together as far as the page
  has letters for, and read by putting it together again. `detect` is sure
  of a mark, of UTF-16 by the zeros between its letters, of ASCII, and of
  UTF-8 by sequences no other encoding makes by accident; anything else is
  a guess it says is a guess. Lines end however the file ended them.
  Opening a `.txt` — from the Open dialog, the Open page, or the command
  line — reads it as paragraphs in the Normal style, at once where the
  bytes say what they are, and through the File Conversion dialog where
  they do not: "Select the encoding that makes your document readable",
  the list with Windows (Default) and MS-DOS first — the machine's own two,
  asked of the system — and a preview that changes with the choice. Saving
  as text shows the dialog the other way round, every time, as Word does:
  the warning that formatting is lost, the encoding (the one the file was
  read from, first), Insert line breaks (where the page wrapped them, from
  the layout), End lines with CR/LF, CR or LF, Allow character
  substitution, a preview, and how many characters cannot be written. The
  dialog gained a field for lines of text to look at. `--picture … textopen`
  and `textsave` draw the two.
  *Not done, and named here:* the East Asian encodings — Shift-JIS, GBK,
  Big5, EUC-KR — which are tables of thousands and a stage of their own if
  wanted; Word's "Confirm file format conversion on open", which asks even
  when sure; Word's red marks on the characters the encoding cannot write,
  which this counts instead; opening a file of any extension as text
  (Word's "Recover Text from Any File"), where this goes by `.txt`.
- [x] **G3. RTF.** Read and write. It is the format everything else exports to.
  Every word processor since 1987 reads it and writes it, the clipboard
  carries formatted text as it, and a file from Word begins with a hundred
  lines of groups a reader has no use for and has to walk past.
  *Done:* `wp-rtf`, a crate of its own. A lexer that cuts the file into
  groups, control words with their numbers, control symbols and bytes; a
  reader that is a stack of states and one pass over the tokens, where a
  group inherits the formatting of the one it is in and gives it back at its
  end, and a group beginning `\*` that names a destination the reader does
  not know is skipped whole — which is what the star is for, and what lets
  Word's `\themedata`, `\latentstyles`, `\rsidtbl`, `\datastore` and the rest
  go by. The font table with each font's charset, which decides the code
  page of the text in that font over the document's `\ansicpg`, read
  through `wp-text`; `\u` with its stand-in skipped as `\uc` says, and a
  character past the plane put together from its two halves; the colour
  table; the stylesheet, with Word's names mapped to the styles every
  document here has; the list table and its overrides, so a paragraph's
  `\ls` becomes the bulleted or the numbered list by its first level's
  `\levelnfc`. Paragraph formatting (alignment, indents, spacing, line
  spacing with `\slmult`, keep and page-break flags, outline level, tab
  stops with their alignment and leader), character formatting (bold,
  italic, the underlines, strikes, size, font, colour, highlight by Word's
  sixteen, super- and subscript, caps, hidden, language), the special
  characters with control words of their own, tables from `\trowd`,
  `\cellx`, `\cell` and `\row` with their widths and grid, pictures from
  `\pict` as PNG or JPEG at their `\picwgoal` size, and `HYPERLINK` fields
  as links. `open` makes a document of it, putting the pictures in where
  their marks were and the links over their text, moved by what a picture
  measures in the text. The writer goes the other way: Word's header with
  the fonts, colours, styles and the two lists, `\'hh` for the Western
  page and `\u` with a question mark for the rest, the special characters
  as their control words, tables row by row, pictures as hex with
  `\picwgoal`, links as fields with the runs split at their edges, and the
  list text in its own group so a reader that knows no lists still shows
  the bullet. What is written is read back, formatting and all.
  The Open dialog offers Rich Text Format; Save As offers it; a `.rtf`
  opened is in Compatibility Mode and the caption says so, as Word's does;
  `wp text`, `render` and `pdf` read `.rtf` too. A file cut down from what
  Word 2016 writes reads to its text with its heading style, its bold run
  and its paragraph spacing.
  *Not done, and named here:* headers and footers, footnotes, sections and
  page setup, nested tables and cell merging, table borders and shading,
  paragraph borders and shading, drawings (`\shp`) and metafile pictures,
  fields other than links (the dates, the page numbers, the tables of
  contents are read as their result text), bookmarks, comments and
  revision marks, right-to-left text, character and table styles, and the
  clipboard as RTF, which is **H2**'s question. Each is one more destination
  or one more control word in the same reader.
- [x] **G4. HTML and MHT.** Read and write, including the mess Word itself
  writes.
  A page from Word is a head of a hundred lines of `<style>`, a body where
  every paragraph is `<p class=MsoNormal style='…'>` and every run a
  `<span style='…'>`, list items carrying `mso-list:l0 level1 lfo1` with
  their bullet inside `<![if !supportLists]>`, pictures as VML in a
  conditional comment with an `<img>` after for everyone else, and `<o:p>`
  round everything. The formatting is in three places at once — the class
  rule, the style attribute, the tag — and reading the page means folding
  the three in that order.
  *Done:* `wp-html`, a crate of its own. A tokenizer of HTML as it is
  written: tags with attributes quoted either way or not at all, comments
  and Word's conditional comments, `<style>` and `<script>` whose insides
  are not tags, the downlevel-revealed `<![if …]>` that Word hides list
  bullets in, and entities by number and by name (the Latin ones by their
  letter and accent). As much CSS as a document needs: rules for tags and
  classes from the `<style>` blocks, `style` attributes, Word's `@list`
  rules for which lists are bulleted, lengths in every unit, colours by
  name, hex and `rgb()`, and font families with their quotes off. A reader
  that is a stack of open elements, each carrying the character formatting
  in force inside it, and a paragraph being built: block tags begin
  paragraphs (headings to the heading styles, `MsoTitle` to Title), `<b>`,
  `<i>`, `<u>`, `<s>`, `<sup>`, `<sub>`, `<font>` and the CSS for weight,
  style, decoration, size, family, colour, background as Word's sixteen
  highlights, vertical-align, text-transform, font-variant and
  `display:none`; alignment, margins, text-indent, line-height as Word
  writes it, page breaks; `<ul>`, `<ol>` and `mso-list` to the two lists;
  tables from `<table>`, `<tr>`, `<td>` with widths; `<img>` with its size;
  `<a href>` as links; `<br>` as line and page breaks; whitespace folded as
  a browser folds it, kept in `<pre>`. The page's bytes are read in the
  charset its `<meta>` names, or the one they betray, through `wp-text`.
  `open_html` makes a document, fetching pictures from beside the page or
  from `data:` URIs; `open_mht` reads the single-file kind — MIME cut into
  its parts, quoted-printable and base64 decoded, pictures found by their
  `Content-Location` — with base64 and quoted-printable written from
  nothing. The writer goes the other way as Word's Web Page: a head with
  the style block naming the styles, paragraphs with their class and
  formatting, runs as spans (the style attribute in single quotes, as Word
  writes it, so a font name's double ones survive), lists as `<ul>` and
  `<ol>`, tables with widths, links, and pictures in a folder named after
  the page — `letter_files` for `letter.htm` — or as the parts of one file
  for the Single File Web Page. What is written is read back. The Open
  dialog offers web pages; Save As offers Web Page and Single File Web
  Page; the command line and `wp text`, `render` and `pdf` read both. A
  page cut down from what Word 15 writes reads to its text with its
  heading style, its bold run, its bulleted and numbered lists, its table,
  its picture and its link.
  *Not done, and named here:* Word's "Web Page, Filtered", which is this
  page with the `mso-` properties left out — the page written here is
  already nearly that, and the distinction is a tick box away; Web Layout
  view, which shows a page as a browser would rather than on paper;
  headers and footers, footnotes, comments, text boxes and shapes (VML),
  which a page from Word carries in conditional comments this reader walks
  past; nested tables, which fold into the cell they are in; cell merging,
  borders and shading; character and table styles; `@font-face`;
  right-to-left text; and the clipboard's `CF_HTML`, which is **H2**.
- [x] **G5. The binary `.doc`.** [MS-DOC] over [MS-CFB]: the compound file, the
  piece table, the formatting sprms. A project in itself, and the reason a
  twenty-year-old document can still be opened.
  *Done:* `wp-doc`, a crate of its own, reading. The compound file: the
  header, the DIFAT and the FAT, the directory, the mini FAT and the mini
  stream, and a stream's bytes by following its chain. The File Information
  Block: the version, the flags (encrypted, which table stream, complex),
  the lengths of the main text and what follows it, and the offset pairs by
  index, however many the version wrote. The sprms: a code that says what
  it is about and how long its operand is, so that the ones this reader
  does not know are stepped over, the table definition's two-byte length
  among them. The reading: the text through the piece table, one byte a
  character in the old code page with the places the format keeps for
  itself, or two; paragraphs cut at their marks and each looked up by the
  file position of its mark through the bin table and the formatting page
  (the two ways a page writes a length); runs cut where the character pages
  cut them; the stylesheet with each style's base, paragraph and character
  sprms, resolved base first and laid under the paragraph's own, and Word's
  names mapped to the heading and title styles; the font table by index;
  the list table and its overrides, with the levels that follow the table
  outside its stated length, telling bullets from numbers; tables from the
  in-table and row-end marks with the widths the row's definition gives;
  fields, HYPERLINK ones becoming links; pictures from the data stream —
  the header that says how big they are drawn, then the drawing container
  with the picture record inline or named in the drawing store: PNG, JPEG,
  TIFF, a DIB given back its fourteen bytes to be a bitmap, EMF and WMF
  inflated from the deflate they are squeezed with; the first section's
  page size and margins. Alignment, indents, spacing, line spacing, keep
  and page-break flags, outline level, list and level; bold, italic, the
  underlines, strikes, caps, hidden, size, font, colour by index and by
  value, highlight and character shading, super- and subscript and raised
  text, language. Encrypted files are refused by name.
  Held to files written by somebody else: LibreOffice, without its windows,
  joins the build image as a test tool, writing a page of everything a
  document holds to Word 97 and a document of this program's own through
  the same door; both read back with their text, styles, formatting, lists,
  table, picture, link and page. The Open dialog offers Word 97-2003
  Documents; a `.doc` opens in Compatibility Mode and the caption says so;
  the command line and `wp text`, `render` and `pdf` read it.
  *Not done, and named here:* writing, which is **G8**; headers and
  footers, footnotes, endnotes and comments, whose text follows the main
  text and whose tables the block names; sections past the first; nested
  tables, cell merging, borders and shading; drawings that are not
  pictures (the shape tables); bookmarks; fields other than links, read as
  their result; revision marks; the properties streams; the Word 95 and
  earlier layouts, whose block has no piece table; and encrypted files.
- [x] **G6. ODT.** Read and write, which is what an open format is for.
  *Done:* `wp-odt`, a crate of its own, over `wp-zip` and `wp-xml`. The
  package: `mimetype` first and stored, as the standard says, then the
  manifest, `content.xml`, `styles.xml`, `meta.xml` and the pictures
  under `Pictures/`. Reading: the fonts declared, the named styles from
  `styles.xml` and the automatic ones from `content.xml` folded parent
  first — an automatic style keeping the name and outline level of the
  style it builds on, so that a heading with a page break is still a
  heading; paragraphs and headings, with their alignment, indents, spacing,
  line spacing, keep and break flags; spans with their text style —
  bold, italic, the underlines, strike, size, font, colour, highlight,
  superscript and subscript, caps, small caps, hidden; tabs, line breaks,
  runs of spaces; lists by the element round the paragraph or the style's
  own, bulleted or numbered by the list style's level; tables with their
  column widths, cells of any blocks; pictures in frames with their size,
  from the package or inline as base64; links; the first page layout's
  size and margins; the title. Writing: the reverse of it, with one
  automatic style per distinct paragraph and text formatting, the two
  lists as list styles, tables with column styles, headings and the title
  as their named styles, links as the Internet link style, the page as the
  master page's layout.
  Held to the other implementation: LibreOffice writes a page of everything
  a document holds to ODT and this reader gets its text, styles,
  formatting, lists, table, picture, link and page back; LibreOffice reads
  a package this writer made, and the text it gives back is the text that
  went in — the bullet in front, which is the list proved — and the Word
  file it makes from it keeps the heading, the centring, the bold, the
  list and the table. Two LibreOffices starting at once fall over each
  other, so the tests take turns. The Open dialog offers OpenDocument
  Text and Save As lists it last, as Word does; an `.odt` is not in
  Compatibility Mode; the command line and `wp text`, `render` and `pdf`
  read it.
  *Not done, and named here:* headers and footers, footnotes, endnotes and
  comments; tracked changes; sections and columns; frames that are not
  pictures, shapes and text boxes; fields other than links; cell merging,
  cell borders and shading; nested tables; character styles by name (they
  are read into the run and written as automatic styles); bookmarks;
  tables of contents; the settings part.
- [x] **G7. PDF import.** Word does it; it is text extraction and reflow.
  *Done:* `wp-pdf` reads as well as writes. The file: objects by the
  cross-reference — the table, the stream with its predictor, the object
  streams, the chain of earlier ones — and, when that cannot be trusted,
  by reading the whole file for everything that looks like an object,
  which is how every reader repairs one; every kind of object, streams
  with their lengths taken or found; the filters — deflate, LZW, the two
  ASCII armourings, run lengths, the PNG and TIFF predictors. The fonts:
  simple ones through the standard, Windows and Macintosh encodings, the
  Symbol font's own, the dingbats, and the font's differences by glyph
  name; composite ones through their CMaps, embedded or identity, by
  codespace; ToUnicode tables, or the embedded program's own character
  map turned round when there is none; widths from the file, the
  standard fonts' known ones, or the program; what the name and the
  descriptor say — family, bold, italic — with the free fonts cut to
  Word's fonts' measurements given Word's names. The page: the content
  stream run for where every glyph lands and how big, through the
  graphics stack, forms, and the page's own turn; the fill colour; the
  rectangles filled and stroked; pictures placed — JPEG as itself, the
  rest, grey, RGB, CMYK or a palette with a soft mask, as PNG. The
  reflow: glyphs into lines by baseline, with the spaces the gaps mean;
  lines cut into columns and read column by column between the items
  that span the page; lines into paragraphs by the pitch, the short last
  line, the indent, the bullet or number, the change of size; a
  paragraph's alignment from its margins — centred on the page's centre
  line too, since the column's edge is only the longest line — its
  indents, its space after, justification from the stretched spaces;
  bullets and numbers as list items at the levels their columns rank;
  headings by size and weight, ranked; a word broken at a hyphen joined;
  a paragraph cut by a page end joined; rules under and through words as
  underline and strike-through; smaller glyphs above and below the line
  as superscript and subscript; a slant in the text matrix as italic;
  crossing rules as a table, its lines cut at the column edges into the
  cells, spans where a border is missing, the borders kept; a picture
  beside text as a character of its line, the others as paragraphs of
  their own; links from the annotations; the page from its box and the
  margins from where the text lies; the title from the information
  dictionary. Encrypted files are refused by name.
  Held to LibreOffice: it prints a page of everything to PDF and the
  reader gets back the paragraphs, the heading, every formatting, the
  justified indented paragraph, the lists, the table, the picture in its
  line, the link and the Unicode text; and a PDF this program wrote
  comes back with its heading, bold, bullet and centring — the writer
  now naming a font's style and weight so that a reader has them. The
  Open dialog offers PDF Files; opening one shows Word's notice and
  converts; the document keeps the file's name, and Save goes to Save As
  beside it as a Word document, the PDF untouched; the command line and
  `wp text`, `render` and `pdf` read it.
  *Not done, and named here:* encrypted files (RC4 and AES with the empty
  password); JPEG 2000, fax and JBIG2 pictures, and inline pictures;
  the predefined CJK CMaps; Type 3 glyph procedures; headers, footers
  and page numbers told from repeated lines; footnotes; tables drawn
  with horizontal rules only, or with none; text drawn rotated; the
  reading order of pages with more than two columns of unequal height;
  the "Don't show this message again" box on the notice; and the
  italic the layout cannot draw without an italic face, which is the
  layout's.
- [x] **G8. Writing the binary `.doc`.** Word 97-2003 Document in Save As:
  the compound file written — header, FAT, directory, mini stream — and a
  document in it with one piece of text, its formatting pages and bin
  tables, a stylesheet of the styles used, the font table, the two lists,
  tables, pictures in the data stream, links as fields, and the section's
  page. Reading it back is not the test; Word opening it is, and until
  something that is not this program can be made to open one here,
  LibreOffice reading it is what stands in.
  *Done when:* a document of one of everything is saved as a `.doc`,
  LibreOffice opens it and makes a `.docx` of it, and the `.docx` has the
  text, the formatting, the heading, the two lists, the table, the link, the
  picture and the margins that went in.
  `wp_doc::save` writes Word 97's file (`crates/wp-doc/src/write.rs`). In the
  `WordDocument` stream: the File Information Block — nine hundred bytes, its
  ninety-three offset pairs, and the four fields the format calls reserved
  that Word 97 still fills with where the text begins and ends — then the
  text as one piece of two bytes a character, then the section's
  properties, then the pages of character and of paragraph formatting, each
  packed as full as it will go. In `1Table`: the piece table, the two bin
  tables, the stylesheet, the font table, the section table, the field table,
  the two lists with their nine levels each and the overrides a paragraph
  names them by, and Word 97's five hundred bytes of document properties. In
  `Data`, each picture behind the sixty-eight bytes that say how big it is
  drawn, as a picture-frame shape whose one property names the store entry
  that follows it, holding the PNG or the JPEG itself.
  What the text holds: paragraphs, with alignment, indents twice over (the
  Word 97 sprms and the Word 2000 ones, as Word writes a `.doc`), spacing,
  line spacing, keeping, widow control, outline level, tab stops and list
  membership; runs, with bold, italic, strike, double strike, capitals, small
  capitals, hidden, underline of six kinds, size, font, colour, highlight,
  raised and lowered, position and language; tabs, line, page and column
  breaks, soft and non-breaking hyphens; tables of one level — cell marks,
  row marks carrying the cell edges and single lines round every cell, as a
  table made here has them; links as `HYPERLINK` fields with their three
  marks and their entries in the field table; and pictures in the line. The
  stylesheet has Normal, made of what the document's defaults resolve to,
  the headings in the nine places Word keeps for them, Default Paragraph
  Font, and every other style the text uses — with those they are based on —
  after the fifteen fixed places. A document that ends in a table gets the
  paragraph after it the format requires.
  In the program: Word 97-2003 Document is third in Save As, where Word has
  it; a `.doc` opened and saved goes back as one, in Compatibility Mode as
  before; a document with macros is asked about as a macro-free save is; and
  one with a password is refused with the reason, since the old format's
  encryption is not written here. `wp convert <in> <out>` writes whatever
  opens as whatever the output's extension says, `.doc` included.
  *Proven by:* `crates/wp-doc/tests/writing.rs` — a document with a heading,
  a paragraph of every kind of run formatting and a tab, two bulleted and two
  numbered items, a two-by-two table with its column widths, a link over a
  word, a PNG, Cyrillic, and margins of its own — written, checked to be a
  compound file with Word 97's block, read back by this program's reader,
  and then opened by LibreOffice and converted to a `.docx`, in which every
  one of those is found as it went in: the heading's style, the run
  formatting, the paragraph's alignment and indent, a bullet list and a
  decimal one, the table's widths to the twip, the link's address and word,
  the picture's bytes and size, and the margins. The editor's own test saves
  a `.doc`, opens it, and saves it back as one.
  *Found along the way:* every compound file this program wrote had its
  header's two version numbers the wrong way round — the minor where the
  major goes — and counted its directory sectors where version three must
  write nought. This program's reader looks at neither; LibreOffice refused
  the file outright, and did the same with LibreOffice's own `.doc` streams
  put into a compound file here, which is how the fault was found. The same
  header went on every encrypted `.docx` and every macro project in a
  `.docm` this program saved; they are all right now, and `wp-ole` holds the
  header to the format's layout. And a `.doc` opened and saved went back
  under its own name as a `.docx` package.
  *Not done:* Word itself opening the files — there is no Word here, and
  LibreOffice stands in, as the item says. And what the model has that this
  writer leaves out: headers and footers, footnotes and endnotes, comments,
  tracked changes, bookmarks, fields other than links, sections after the
  first, floating pictures, shapes, text boxes and charts, pictures that are
  not PNG or JPEG, tables in tables, merged and shaded cells, paragraph
  borders and shading, character spacing and scale, the summary information
  streams, macros, and a password.
- [x] **G9. The rest of the Rich Text reader.** `.rtf` opened and read its
  text, its formatting, its tables and its pictures; what it did not read
  was headers and footers, footnotes, sections and page setup, nested tables
  and cell merging, table and paragraph borders and shading, drawings
  (`\shp`) and metafile pictures, fields other than links, bookmarks,
  comments and revision marks, right-to-left text, and character and table
  styles. Each is one more destination or one more control word in the same
  reader. Named in **G3**.
  *Done when:* a file LibreOffice writes from a document of one of
  everything, and files cut down to the shape of what Word writes, open with
  each of those where it was.
  The reader keeps a stack of *stories* (`crates/wp-rtf/src/read.rs`): the
  document's own text, and a header, a footer, a note, a comment or the
  words in a text box, each read into paragraphs and tables of its own when
  its group begins and put where it belongs when the group ends. Sections
  end at `\sect` on the paragraph they end — one paragraph, not two, when a
  writer ends the paragraph first — each with its paper, margins, turn,
  columns, how it begins, where its header and footer sit, its page
  numbering, a first page of its own, and its own headers and footers of the
  three kinds; a section with none follows the one before it, and
  `\facingp` gives left and right pages their own. Footnotes and endnotes,
  starred or not, leave their mark in the sentence and their words,
  formatted, in the notes. Comments take their range from `\atrfstart` and
  `\atrfend`, and their author and date, Word's packed date decoded.
  Bookmarks are named ranges. Tracked insertions, deletions and changes of
  formatting carry their authors from `\revtbl`, their dates, and — for a
  change of formatting — what the formatting was before, from `\oldcprops`.
  Every field keeps its instruction on the runs of its result; a link is a
  link, one to a place (`\l`) goes to the bookmark, and a drawing wrapped in
  a `SHAPE` field is the drawing. Tables: cells merged across and down,
  ruled, shaded and aligned down the cell; the row's lines, header rows and
  heights; the table's style; tables inside cells to any depth, the inner
  row described after it; and the columns worked out from every edge any
  row has, so rows of different cells line up. Paragraph borders of every
  line Word draws, `\box`, and shading with a pattern's percentage mixed
  in. Pictures: EMF, WMF, bitmaps (given the file header they come without),
  data after `\bin`, and the scale a picture was shown at; a picture a
  drawing frames floats where the drawing was; and pictures in headers,
  footers, notes and comments go into those parts, with relationships of
  their own. Drawings: the shapes Word numbers, with their fill, line, size,
  turn, flips, name, description, where they float and how text wraps round
  them, and the words in a text box. Right-to-left paragraphs and runs,
  with the page's left and right — alignment, indents, borders — made the
  start and end the model keeps once the paragraph has said which way it
  runs. The stylesheet's paragraph, character and table styles are defined in
  the document with their formatting, what they are based on and what
  follows them, and `\s`, `\cs` and `\ts` name them. Read as well: character
  spacing, scale, raised and lowered text, kerning, underline colour, column
  breaks, the oldest writers' page number (`\chpgn`), and the picture of an
  embedded object. None of it is something to undo straight after opening.
  *Proven by:* `crates/wp-rtf/tests/libreoffice.rs` — a document with a
  footnote, an endnote, a bookmark, a comment, a tracked insertion and
  deletion, a field, a boxed and shaded paragraph, a right-to-left one, a
  character style and a paragraph style, a table with cells merged both ways,
  a shaded cell with lines of its own and a table inside a cell, a text box,
  a header, a footer with the page number, and a first page of its own, made
  here, saved as a `.docx`, converted to RTF by LibreOffice and opened: each
  is found where it went in. `crates/wp-rtf/tests/word.rs` — files in Word's
  shape: two sections with their pages, all their headers and footers, their
  numbering and how they begin; notes, comments and changes where they were;
  styles of the three kinds; Word's table with its row described twice, a
  merged header row, a style and a table inside a cell; cells merged down,
  saved and read back; five drawings and a framed picture; the metafiles, a
  bitmap and a picture after `\bin`; links to places and a table of
  contents; borders, shading and direction; left and right both ways round;
  a change of formatting saved and read back as one; pictures in a header, a
  note and a comment; and a link that begins with a picture.
  *Found along the way:* every footnote and endnote this program wrote
  carried, inside the note, a reference to a note — to itself — where Word
  writes the note's own mark, `w:footnoteRef`. LibreOffice would not open a
  file with one in it at all, and a note that came from Word showed no
  number where the notes are printed, because its own mark was not read.
  Both are right now. A table written from the model lost its cells'
  merges down, their own lines and their shading, which every reader of
  another format builds its tables through. A paragraph's side borders and
  a table's were written with the strict schema's names, `w:start` and
  `w:end`, which LibreOffice drops from a paragraph; they are written as
  Word writes them. A change of formatting kept only who made it; what the
  formatting was before is in the model now, read from a `.docx` and
  written back, so a run written out again from the model is still a change.
  A link that began at a picture was moved to before it when the picture
  went in. And LibreOffice's own RTF writes a change of formatting as its
  date in the text, with no control word before it — which is what it says,
  and what is read.
  *Not done:* pictures in text boxes, whose words are inside a drawing that
  no place in the document can be counted to; groups of drawings, WordArt,
  freeform drawings and Word 6's drawing objects (`\do`); a drawing's
  arrowheads, dashes, shadows and gradients; tracked changes to paragraph
  formatting, to paragraph marks, and moves; the replies to a comment; a
  field whose result runs over more than one paragraph — a table of
  contents — which keeps its result as text, the links in it still links;
  frames, character borders and shading, sections that run right to left,
  line numbering, and a note marked with something other than its number.
  And the RTF writer, which writes what **G3** says and none of this.
- [x] **G10. The rest of the web-page reader, and Web Layout view.** What a
  page from Word carried and this did not read: headers and footers,
  footnotes, comments, text boxes and shapes (VML) inside the conditional
  comments this reader walked past; nested tables, cell merging, borders and
  shading; character and table styles; `@font-face`; right-to-left text. And
  two of Word's own: "Web Page, Filtered", which is this page with the `mso-`
  properties left out and is a tick box away, and Web Layout view, which
  shows a page as a browser would rather than on paper. Named in **G4**.
  *Done when:* a page cut down to the shape of what Word writes, a single
  file of one with its headers beside it, and a page LibreOffice writes from
  a document of one of everything open with each of those where it was; a
  document saves as a filtered page; and Web Layout shows a document as one
  sheet the width of the window, the way a browser shows the page.
  The reader (`crates/wp-html/src/read.rs`) reads into *stories* as the RTF
  reader does: the page's own text, and a division that is a note, a comment
  or a header, read the same way and put where it belongs when it ends. A
  conditional comment is no longer a comment to walk past but what it holds:
  VML drawings — text boxes with their words, rectangles, rounded ones,
  ovals, lines and the shapes Word numbers, with their fill, line, size,
  turn, flips, where they float, what they are measured from and how text
  wraps round them — and pictures in frames at the size the VML gives, with
  the plainer copies Word writes for other readers (`v:shape`, `v:shapes`,
  `<![if !vml]>`) not read a second time; and the table styles Word keeps in
  a style block only Office reads. Every `<![if !support…]>` and
  `<![if !mso]>` is the fallback it is, and a tag inside one is only
  something for its end to close. Footnotes and endnotes as Word's Web Page
  writes them, as its filtered page does and as LibreOffice does, the
  number written for other readers left out; comments with their range,
  author, date and words; bookmarks, and links to them. Tables inside cells,
  `colspan` and `rowspan` made the spans and the cells merged down that the
  model has, the columns from every width the rows and `<col>` give, lines
  from Word's precise `mso-border-alt` over the rounded ones it writes for
  browsers, `border=1`, shading, alignment down the cell, header rows and
  heights. A paragraph's own lines and shading, and a division's — Word's
  box round several paragraphs — for each paragraph in it. Right-to-left
  paragraphs and runs from `dir` and `direction`, with the page's left and
  right made the start and end the model keeps, and `align` counted above
  the rules for a tag and below the element's own style. Word's styles —
  every `Mso` class and every rule with `mso-style-name`, paragraph,
  character and table, with what they are based on and what follows them —
  defined in the document and named by the paragraphs, runs and tables that
  use them. Sections from Word's `WordSection` divisions and their `@page`
  rules: paper, turn, margins, where the header and footer sit, columns, a
  first page of its own, and the headers and footers named in the file
  beside the page, fetched from beside it or from inside a single file, with
  the pictures in them. The colour of the page, and each run's language.
  `@font-face` becomes the document's font table — another name a font goes
  by, its kind of letter, its PANOSE numbers — which `wp-docx` now reads and
  writes (`crates/wp-docx/src/fonts.rs`), and which the layout goes on when
  the machine lacks a font: the other name first, then a face of the same
  kind, one with serifs for one with serifs and a typewriter's for a
  typewriter's, where before every missing font was drawn without serifs.
  That serves every `.docx` from Word as well, whose font table says the same.
  `wp_html::write_filtered` is Word's "Web Page, Filtered": the same page
  without the Office namespaces or any `mso-` property. Save As offers it
  after Web Page, told apart from it by the type chosen in the list, which
  the Windows dialog now says (`wp_shell::dialog::save_file_typed`); `wp
  convert <in> <out>.htm --filtered` writes it too. Web Layout lays the
  document out as one sheet the width of the window in one column, whatever
  its sections say, with no page breaks, no headers or footers, no borders
  round the paper and no line numbers, the notes after the text, and the
  sheet as long as its text and at least as tall as the window; it fills the
  window from its left edge with no desk round it, and the side ruler, which
  has no page to measure, is not drawn.
  *Proven by:* `crates/wp-html/tests/word.rs` — Word's notes, comment and
  bookmark; styles of every kind, the fonts, a right-to-left paragraph and
  Word's box round two paragraphs; Word's table with a cell across and one
  down, shading, precise lines and a table inside a cell; a single-file page
  of two sections with their pages, a header, a footer and a first-page
  header from the file beside it and a picture in the header; a text box, an
  oval and a picture from their VML, their copies not read twice; and a
  filtered page with nothing only Office reads. `crates/wp-html/tests/
  libreoffice.rs` — a document of one of everything as a `.docx`, written as
  a page by LibreOffice and opened: its notes where they were, its bookmark,
  its header, its page, its boxed and shaded paragraph, its right-to-left
  one, and its table with its merges, lines, colours and inner table.
  `crates/wp-layout/tests/web.rs` — page and column breaks, none in a table
  cell, a document on the web as one sheet in one column with no header and
  its note after the text, and missing fonts stood in for by what the table
  says. And the editor's own: Web Layout filling the window and scrolling no
  further than its text, and Save As offering the filtered page and writing
  one.
  *Found along the way:* a page break or a column break in a paragraph —
  every Ctrl+Enter, and every page break in a file from Word — never began a
  page or a column; the layout drew it as the end of a line. It does now, in
  the document's own text; a cell, a header or a note has no page to break.
  Web layout was paper as tall as a page can be, so the view scrolled
  almost for ever past the text; a document of several sections was laid out
  on each section's paper, in its columns, with its headers drawn off the
  side of the sheet; and the sheet sat on a desk wider than the window. The
  rulers measured the document's margins in every view, not the ones the
  view lays the text out with. And `wp convert` to a `.htm` wrote a Word
  package under that name.
  *Not done:* fonts a page fetches from elsewhere, which `@font-face` can
  name as well as describe; the tracked changes a page from Word marks with
  `<ins>` and `<del>`, which read as underlined and struck through; fields,
  whose results read as text; groups of drawings, WordArt and freeform
  drawings, and a drawing's arrowheads, dashes and gradients; pictures in
  text boxes; the comments LibreOffice writes, as comments of HTML's own,
  and its floating frames. On Linux the file dialogs do not say which type
  was chosen, so a filtered page is written there by `wp convert` only. And
  endnotes in print layout, which the layout counts but draws nowhere; Web
  Layout draws them after the text.
- [x] **G11. The rest of the `.doc` reader.** Headers and footers, footnotes,
  endnotes and comments; sections past the first; nested tables, cell merging,
  borders and shading; drawings that are not pictures; bookmarks; fields other
  than links; revision marks; the properties streams; the Word 95 and earlier
  layouts, whose block has no piece table; and encrypted files. Named in
  **G5**.
  *Done when:* a document of one of everything, written as Word 97 by
  LibreOffice, opens with each of those where it was; a Word 95 file opens as
  LibreOffice opens it; and a file encrypted each of the three ways Word
  encrypted one opens with its password — in the editor too, which asks for
  it as it asks for a `.docx`'s.
  The reader (`crates/wp-doc/src/read.rs`) reads the text's *stories* — the
  main text, the footnotes, the headers and footers, the comments, the
  endnotes and the text boxes — each where the block's lengths put it and cut
  into its entries by its own table of positions, each read the same way
  into paragraphs and tables of its own. Reading the main text notes where
  each of its characters ends up in the document, and the bookmarks, the
  comments' ranges and the sections are read through that. Footnotes and
  endnotes: their marks where they were, their words with them. Comments:
  who wrote them, what they cover (their own bookmarks, tagged), their words,
  and their dates where Word 2002 and after keep them. Bookmarks. Sections:
  every one, from the section table and its sprms over Word's own defaults —
  paper, turn, margins, where the header and footer sit, columns, how it
  begins, a first page of its own, page numbering — and each one's headers
  and footers, the even pages', the first page's, a missing one carrying the
  section before it on; facing pages from the document's properties.
  Tables (`crates/wp-doc/src/tables.rs`): the row's description from the
  sprms of the mark that ends it — the cells' edges, each cell's merge across
  and down, its four lines, its colour, where its text sits and which way it
  runs, the table's own lines, the row's height and whether it heads the
  table — Word 97's forms and Word 2000's both; the columns from every edge
  any row has; and tables inside cells, as deep as they go, from the depth
  each paragraph states and the marks that end an inner cell and row. A
  paragraph's lines and colour behind it, its tab stops, its direction; a
  run's direction, its spacing, its language however Word wrote it.
  Fields: every one's instruction on the runs of its result, `HYPERLINK`
  with a place in the document as a link to a bookmark, a field in a field's
  instruction left out of the text. Tracked insertions and deletions, with
  who and when, numbered as one change per person per time, and changes of
  formatting. Symbols, column breaks. Drawings (`crates/wp-doc/src/shapes.rs`):
  the Office drawing container in the table stream read whole — the store,
  each shape's number, kind and properties, the second table of properties
  Word 2000 keeps — and each floating drawing's place from its story's table:
  shapes with their fill, line, turn, flips, name and description, text boxes
  with their words from their own story, and pictures that float, their bytes
  from the store or from the main stream where the store says, all with where
  they are measured from, how they are aligned, how text goes round them and
  whether they are behind it; in the main text and in headers. The font
  table becomes the document's (`wp-docx`'s, from **G10**), and the two
  property streams its title, subject, author, keywords, comments, last
  author, category, company and dates (`crates/wp-doc/src/summary.rs`).
  Word 6 and Word 95: the block at its fixed places read into the same
  places Word 97's are (`crates/wp-doc/src/fib.rs`), everything in the one
  stream, text in one stretch where the block says or in pieces, the
  formatting pages' narrower entries and bin tables that list fewer pages
  than there are, the stylesheet's and the font table's one-byte names, and
  every one-byte sprm said again as Word 97's (`crates/wp-doc/src/old.rs`) —
  borders, row definitions, symbols and a picture's place with their operands
  grown; each byte of text read in the code page of its font; Word 6's
  numbered and bulleted paragraphs; headers as many as each section says it
  has. Encryption (`crates/wp-crypt/src/binary.rs`): Word 97's RC4, Word
  2002's RC4 through CryptoAPI and Word 95's exclusive-or, each stream
  deciphered from its first byte and the readable part of the main one kept;
  `wp_doc::open_with_password`, and the editor's password dialog, which a
  `.doc` now goes to as an encrypted `.docx` does; the document keeps the
  password, and is written back encrypted as a Word Document.
  *Proven by:* `crates/wp-doc/tests/libreoffice.rs` — a document of one of
  everything, as a `.docx`, written as Word 97 by LibreOffice and opened: its
  notes where they were, its bookmark, its comment over its word, its
  insertion, deletion and change of formatting with their author and date,
  its field; its boxed and shaded paragraph, its right-to-left one, its
  heading, its properties and its font table; its table with its merges,
  lines, colours and inner table; its rectangle, text box and floating
  picture; its two sections with their pages, columns, headers — one with a
  picture in it — footer with the page number and first page of its own.
  `crates/wp-doc/tests/encrypted.rs` — LibreOffice, through a macro of its
  own, writes a file with a password, which opens here; files encrypted the
  other two ways are made here by the specification from one LibreOffice
  wrote, LibreOffice opens them with the password, and so does this; the
  wrong password and none are told apart. `crates/wp-doc/tests/word6.rs` — a
  Word 95 file built byte by byte, and what LibreOffice reads in it is what
  this reads: a heading, bold and italic, alignment and indent, Cyrillic in
  its font's code page, a table, a bullet, a footnote, a comment, a
  bookmark, the margins and a header with a page number. And the editor's
  own: an encrypted `.doc` asks for its password, asks again on a wrong one,
  and opens on the right one keeping it.
  *Found along the way:* the key Word 97's RC4 makes was given to the cipher
  as nine bytes of its hash rather than sixteen, so no file encrypted that
  way — a `.doc`, or a package in that oldest scheme — would ever have
  opened; and the hash of the RC4 CryptoAPI verifier was read from four bytes
  too early, and the test beside it built its file the same way. Both are
  what LibreOffice settles now. The XOR table another implementation carries
  has one number wrong, which the rule its rows are built by shows. A table
  definition's length counts one more byte than follows it, and read at its
  word it swallowed the first byte of whatever sprm came next. Word 97's own
  indents — the older sprms, which every Word writes beside the newer — were
  not read, and the language of East Asian text overwrote the Latin one's.
  The section's turn was read, and written by **G8**'s writer, under the
  number of a different sprm; a section that said nothing of its side
  margins got an inch rather than Word's inch and a quarter. Headings were
  known by their English names only; they are known by Word's number for
  them now, in any language. `wp-docx` counted LibreOffice's continuation
  separator, which it numbers 1, as a footnote. The table of Office shape
  numbers was in the RTF and web-page readers twice, differing; it is one
  table in `wp-docx` now (`wp_docx::shapes::office_preset`).
  *Not done:* groups of drawings, WordArt and freeform drawings — among them
  every ellipse LibreOffice writes, which it writes as a freeform — and Word
  6's drawing objects; pictures in text boxes; Word 6's outline numbering of
  headings, and the ranges of its comments; tracked changes to paragraph
  formatting and to paragraph marks, and what a run's formatting was before
  a tracked change to it, which the file does not keep — rejecting one puts
  back the style's; a field whose result runs over several paragraphs, which
  keeps its result as text; notes marked with something other than their
  number, which read as numbered; frames; the document properties of a file
  that encrypts them with the rest; the styles as styles, which are laid into
  the formatting of what uses them as **G5** left them; and Word 2 and
  older, which are not compound files at all.
- [x] **G12. The rest of the ODF reader.** Headers and footers, footnotes,
  endnotes and comments; tracked changes; sections and columns; frames that
  are not pictures, shapes and text boxes; fields other than links; cell
  merging, cell borders and shading; nested tables; character styles by name;
  bookmarks; tables of contents; the settings part. Named in **G6**.
  *Done when:* a document of one of everything, written as OpenDocument by
  LibreOffice from a Word document, opens with each of those where it was;
  a package written by hand in the forms LibreOffice does not write from a
  Word document opens with them as well; and a character style this program
  writes to a package is one LibreOffice reads.
  The styles (`crates/wp-odt/src/styles.rs`) are folded parent first in
  every family the text names — paragraph, text, table, row, cell, frame,
  section — with the page layouts and the master pages; the masters' own
  automatic styles kept apart from the text's, since the two parts may use
  the same names for different things. The reader (`crates/wp-odt/src/read.rs`)
  reads *stories* as the RTF reader does: a note's words, a comment's, a
  text box's, a header's and a footer's, each read as the text is and put
  where it belongs, and every place in the text counted as the document
  will count it. Footnotes and endnotes with their marks where they were,
  each note begun with its own mark as Word's are. Comments with their
  author, date, words and the range to their end mark. Bookmarks, ranges
  and points. Tracked changes from the list at the head of the text:
  insertions and changes of formatting between their marks, deletions with
  their words put back where they were taken from, marked deleted and
  counting for nothing, and whether changes were being recorded. Every field
  Word has a name for — the page and the count of pages, the counts of words
  and characters, the dates and times, the title, subject, comments,
  keywords, author, last author and file name, a reference to a bookmark or
  to its page, a sequence — as the instruction on the runs of its result, and
  a link outside the text as a field too. Drawings: a frame holding a text
  box, a custom shape with its type — this format's own names, Word's
  presets as LibreOffice keeps them, the Office shape numbers — with its
  words, a rectangle, an ellipse, a line; their fill and line, and where
  they float: the wrap and the side, in front of the text or behind it, how
  they are aligned and what from, the room round them; a frame anchored to
  the page carried by the first paragraph after it. Pictures that float.
  Tables: cells merged across and down from the cells the format marks as
  covered, lines — a double one by its own lines' widths — colour, where the
  text sits and which way it runs; header rows, row heights, the indent,
  columns in groups and repeated. A paragraph's lines and colour, direction
  and contextual spacing, and its outline level; a run's letter spacing,
  and the right-to-left runs of a right-to-left paragraph. Sections: a page
  style beginning where a paragraph's style says, as a section on a new
  page, and a section of the format's own in columns as a continuous one;
  each with its paper, its margins as Word measures them — the header's
  height in the top margin, the header where this format's margin is —
  columns, page numbers and the number its first page takes, and its master
  page's headers and footers: left-hand pages' as the even pages', and a
  first page of its own whether the master says so itself or is a first
  page's style followed by another's. The named character styles defined as
  the document's, and named by the runs in them, the paragraph style's
  formatting giving way to them as Word's does. The table of contents
  gathered again where it was, from the headings, with the page numbers its
  entries showed. What `meta.xml` says of the document; from `settings.xml`
  the zoom it was looked at, which `wp-docx` now keeps
  (`Document::set_zoom_percent`), and whether it asks to be opened only for
  reading; and the font table from the font declarations, which the layout
  goes on for a font the machine lacks (**G10**). The writer
  (`crates/wp-odt/src/write.rs`) writes a run's character style as a named
  style of the format's, with the run's own formatting built on it.
  *Proven by:* `crates/wp-odt/tests/libreoffice.rs` — a document of one of
  everything, as a `.docx`, written as OpenDocument by LibreOffice and
  opened: its notes where they were, its bookmark, its comment over its
  word, its insertion, deletion and change of formatting with their author
  and date, its field; its boxed and shaded paragraph, its right-to-left
  one, its heading, its character style, its table of contents, its
  properties and its fonts; its table with its merges, lines, colours and
  inner table; its rectangle, text box and floating picture; its two
  sections with their pages, columns and headers — one with a picture in it
  — its footer with the page number and a first page of its own.
  `crates/wp-odt/tests/by_hand.rs` — a package in the forms LibreOffice does
  not write from Word: a header for left-hand pages and one for the first
  page, pages numbered from five in small numerals, a section of its own in
  two columns between two in one, a text box on the page, an ellipse behind
  the text, a date, a title, a reference to a bookmark's page, a table's
  header row and exact height, the zoom and the wish to be read only; and a
  character style written and read back. `crates/wp-odt/tests/real_document.rs`
  — LibreOffice reads the character style this writer writes.
  *Found along the way:* a covered cell was read as a cell of its own, so a
  cell across two columns came out as three. The writer wrote only a run's
  own formatting, so a Word document's character styles were lost when it
  was saved as OpenDocument, and with them whatever they made bold.
  *Not done:* groups of drawings, freeform drawings and connectors, and a
  drawing's turn; pictures in text boxes; a deletion of whole paragraphs,
  whose words are put back into one, and tracked changes to paragraph
  formatting; the replies to a comment and whether it is resolved; indexes
  other than the table of contents, whose entries are read as text, and the
  table of contents' own formatting of its entries, which is gathered again
  as this program lays one out; paragraph styles as styles, which are laid
  into the formatting of what uses them; fields Word has no name for —
  variables, chapters, conditional text — which read as their text; notes in
  a header or in a note; a table's own colour behind it and a row that may
  not break; the settings but the zoom and the wish to be read only; and the
  editor opening a document at the zoom it keeps.
- [x] **G13. The rest of the PDF reader.** Encrypted files (RC4 and AES with
  the empty password); JPEG 2000, fax and JBIG2 pictures, and inline
  pictures; the predefined CJK CMaps; Type 3 glyph procedures; headers,
  footers and page numbers told from repeated lines; footnotes; tables drawn
  with horizontal rules only or with none; text drawn rotated; and the
  reading order of pages with more than two columns of unequal height. Named
  in **G7**.
  *Done:* encryption (`crates/wp-pdf/src/read/crypt.rs`): the standard
  security handler in every revision — RC4 of forty to 128 bits, AES-128
  through crypt filters with the identity ones, AES-256 with revision 5's
  hash and revision 6's rounds (SHA-384 added to `wp-hash` for them) — the
  file's key from the user's password or the owner's, every string and
  stream deciphered with its object's key, the cross-reference streams and,
  when the file says, the metadata left as they are. The empty password
  opens the files that need none, which is most; one that needs a password
  asks for it on Open with the dialog an encrypted Word document asks with,
  and asks again when it is wrong (`wp_pdf::needs_password`,
  `open_with_password`); a file encrypted for particular people's
  certificates is refused by name. Pictures (`read/images.rs`): JPEG 2000
  (`read/jpx.rs`, `read/jpx/`) — the JP2 file or the bare codestream, tiles,
  the five progressions and their changes part-way, precincts, layers,
  packet headers kept apart, the markers before and after packets, every
  code-block style, both wavelets, both colour transforms, quantisation
  derived and given, the region-of-interest shift, and the JP2 header's
  colour space, palette and alpha; the MQ coder JPEG 2000 and JBIG2 share
  (`read/mq.rs`); fax codings through `wp-image`'s own decoder, the one
  TIFF uses, with the parameters' K, rows aligned on bytes (added for Group
  4), black taken as one or nought, and the rows the picture's height when
  the filter says none; JBIG2 (`read/jbig2.rs`, `read/jbig2/`) — the page's
  segments after the globals stream's, generic regions in all four
  templates with their pixels moved and typical prediction, or as Group 4;
  refinements in both templates; symbol dictionaries coded either way, a
  height class stored as it is or as Group 4, symbols refined from others
  and made of several, the contexts one dictionary leaves for the next;
  text regions coded either way — strips, the four corners, turned, the
  gaps offset, symbols refined in place, the symbols' own code table; the
  standard Huffman tables and a file's own; pattern dictionaries and
  halftones with their grey planes and the cells skipped; intermediate
  regions kept for a refinement; pages whose stripes say their height; a
  generic region of unsaid length. Inline pictures (`read/content.rs`):
  `BI` to `ID` read as the picture's dictionary, the abbreviations spelt
  out and a colour space named in the resources looked up, the data as
  long as its size makes it when it is not filtered and to the `EI` on its
  own when it is. The predefined Unicode CMaps of the four CJK collections
  (`read/cmap.rs`) — UCS-2, UTF-16, UTF-8 and UTF-32, across and down — the
  codes being the characters, with the printable ASCII's widths from the
  ids every collection gives it. Type 3 fonts (`read/font.rs`): glyphs named
  by their codes, as programs that make fonts name them — `a65`, `c65`,
  `G41`, `g0041` — and the size a font drawn in pixels really is, from its
  matrix and the boxes its glyph procedures declare. The reflow
  (`read/reflow.rs`, `read/reflow/`): a line in the top or bottom eighth
  that half the pages repeat, whatever its numbers, is the running header
  or footer — made from the first page with it, parts of one line at the
  alignment tabs, the numbers that go up with the page the page number and
  one that is the count of pages that — and taken off every page; the lines
  at a page's foot, under the note separator or smaller than the text or
  set apart from it, that start with a number the text has raised, are
  footnotes, the raised number their reference; rules across a table only
  make a table of the lines between them, its columns where the lines
  stand, a rule inside it a row's border; rows of short pieces that stand
  in the same columns make a table without rules — prose in columns and a
  list's markers do not; glyphs that run another way than across — up the
  page, down it, or down in columns as vertical writing does — are read
  along their own lines after the page's text; and a gap too narrow to be a
  column's edge by its width is one when the rows around it leave it empty
  too, which is what keeps three narrow columns of different heights apart
  and in order.
  *Proven by:* `crates/wp-pdf/tests/pictures.rs` — a picture coded by
  OpenJPEG through ImageMagick comes back exact where the coding loses
  nothing (colour, grey, a bare codestream, tiles and three resolutions in
  all five orders) and within two levels of OpenJPEG's own reading where it
  does (lossy, and in layers); a Group 3 and a Group 4 coding lifted from
  libtiff's TIFF read exactly; JBIG2 coded here with the standard's encoder
  — its arithmetic coder checked against the standard's own example to the
  byte — in generic regions of every template, a fax coding, text drawn
  with a dictionary's symbols at every corner turned and not, refined,
  made of others, coded with Huffman tables standard and sent, a halftone,
  a page refined and a page of stripes, each read here and by poppler
  (LibreOffice's PDF import runs it) the same; an inline picture with `EI`
  in its data, and one filtered. `tests/encrypted.rs` — LibreOffice's
  encrypted exports open with the password and without one, and a page
  encrypted here in each revision opens here and in poppler with the user's
  password and the owner's. `tests/fonts.rs` — a Type 3 font in pixels at a
  quarter of a point reads as ten-point "Hi"; a CJK font through
  UniGB-UCS2-H reads its characters. `tests/layout.rs` — a document with a
  header, "Page 1 of 3", a footnote, a table ruled across, one unruled, a
  cell turned up and three columns of different lengths, printed by
  LibreOffice, comes back with each. `crates/wp-app/src/editor/sealing.rs`
  — a PDF with a password asks for it, and again when it is wrong.
  *Found along the way:* `wp-image`'s fax decoder aligned a Group 3 row on
  its byte before looking for the end-of-row, and cut into the end-of-row's
  zeros when the padding before it was short — every such TIFF from libtiff
  came out wrong; it now takes the padding with the end-of-row. Poppler
  draws nothing from a JBIG2 dictionary with a height class stored uncoded,
  where jbig2dec reads it as this does.
  *Not done, and named here:* the older CJK encodings' predefined CMaps —
  Shift-JIS, GBK, Big5, EUC-KR codes — which need the tables of **G14**,
  and, through the Unicode CMaps, the collections' ids and so the widths of
  anything but ASCII, which take the font's default; the security handler
  for particular people's certificates; SASLprep for the AES-256
  passwords, which are taken as typed; JPEG 2000's Part 2, and a JP2's
  colour from an ICC profile, which is taken by its channels; JBIG2's colour
  extension; a footnote carried over to the next page, whose rest stays in
  that page's text; a header or footer only the first page or every other
  page has, which is text; and the "Don't show this message again" box,
  still named in **G7**.
- [x] **G14. The East Asian encodings, and the rest of opening a text file.**
  Shift-JIS, GBK, Big5 and EUC-KR are tables of thousands and a stage of
  their own. With them: Word's "Confirm file format conversion on open",
  which asks even when it is sure; Word's red marks on the characters an
  encoding cannot write, which this counts instead; and Word's "Recover Text
  from Any File", which opens a file of any extension as text. Named in
  **G2**.
  *Done:* `wp-text` reads and writes the four East Asian code pages —
  Shift-JIS (932), GBK (936), Unified Hangul, which is EUC-KR and more
  (949), and Big5 (950) — where a byte past ASCII is a character or the
  first of a pair: tables of their single bytes, of the row of pairs each
  first byte begins, and of the rows, generated with the single-byte ones by
  `tools/generate-codepages.sh` from the mappings Python carries (with the
  euro Windows' own GBK has at 0x80, which Unicode's CP936.TXT has too),
  and the code Windows writes a character with where a page has it twice.
  A pair that means nothing is the replacement character for its first
  byte, and what follows is read on its own; writing gives each character
  its bytes, or its stand-in with substitution allowed, or a question
  mark; and `Encoding::unwritable` says where in a text the characters are
  that an encoding cannot write. The four are named as HTML and mail name
  them — `shift_jis`, `gbk` and `gb2312`, `euc-kr` and `ks_c_5601-1987`,
  `big5` — and in the File Conversion list as Word names them. A file whose
  bytes past ASCII are not UTF-8 is still a guess and still asks, but the
  guess is an East Asian page where the bytes read as one without a fault
  and in its script — Japanese with its kana, Korean in Hangul, Chinese in
  its common ideographs, which the letters of the European pages do not
  make — preselected in the dialog. With the tables: RTF's fonts in those
  four character sets read two bytes a character, and a font's own name in
  its character set (`crates/wp-rtf/src/read.rs`); and the PDF reader's
  national CMaps — the Shift-JIS, GB and GBK, Big5, and Korean ones, and
  Japanese as EUC and as the seven-bit JIS rows and cells, through Shift-
  JIS's arithmetic — read their codes as the characters they are
  (`crates/wp-pdf/src/read/cmap.rs`), which **G13** named. Saving as text
  shows the text as it is in the preview, the characters the encoding
  cannot write in red, as Word's does (a dialog's lines field now takes
  marks). Options has an Advanced page with "Confirm file format conversion
  on open", remembered with the other settings; ticked, every file that is
  not a Word document is put to Word's Convert File dialog before it is
  read — a list of every kind this program reads, the kind the file is
  taken for chosen, taken by its extension or, where that says nothing, by
  its first bytes (`crates/wp-app/src/editor/conversion.rs`), and the file
  read as whichever is chosen. "Recover Text from Any File" is one of the
  kinds, and a type of the Open dialog: the file as text where it is text,
  and otherwise the runs of readable characters in it, single bytes and
  UTF-16, each a paragraph in the order they stand. A file of an extension
  this program does not know is taken by its bytes now, where it was read
  as a Word package whatever it was. `--picture - … convert` draws Convert
  File.
  *Proven by:* `crates/wp-text` — each page's own characters from their
  known bytes and back, half-width katakana and the euro included, a first
  byte with nothing after it and a pair that means nothing, where a text
  cannot be written, and each page guessed from its own text while Russian
  in Windows-1251 is not taken for GBK; `crates/wp-rtf/tests/word.rs` — a
  Japanese font named in Shift-JIS and its text as `\'hh` pairs;
  `crates/wp-pdf/src/read/cmap.rs` — every national CMap's codes to their
  characters, and the ids of their Latin; `crates/wp-app` — a file asked
  about and read as the kind chosen, a file read as what it is when the
  setting is off, text recovered from a binary file, and the save
  preview's marks.
  *Not done, and named here:* Word 6 and Word 95 documents from the East
  Asian editions, whose text is two bytes a character in the document
  itself; text files in EUC-JP, ISO-2022-JP, GB18030's four-byte sequences
  and Johab, which Word lists and this does not; the Convert File dialog's
  "Show all" box, the list here showing every kind always; and, on Linux,
  the Open dialog saying which type was chosen, which the system's dialog
  does not tell — there Recover Text is reached through Convert File.
- [x] **G15. A document that takes its styles from its template.** Word's
  Developer ▸ Document Template ▸ Automatically update: the attached template
  is read on opening and its styles are applied over the document's own.
  **G1** attaches the template and does not do this, which is the other half
  of what an attachment is for.
  *Done:* `wp-docx` says and sets whether a document takes its styles from
  its template (`links_styles`, `set_links_styles`: Word's `w:linkStyles`
  in the settings) and takes them (`update_styles_from`): every style the
  template has over the document's of the same identifier, or joining
  them, and the defaults they are built on; the lists the styles number
  with carried into the document's own numbering under numbers of its own,
  and on the next opening recognised as the same lists rather than carried
  again; the document's other styles left as they are
  (`crates/wp-docx/src/kinds.rs`). The editor takes them as a Word document
  that says so opens and its template can be read, and does not count that
  as a change (`crates/wp-app/src/editor/templates.rs`). The Developer tab
  has Word's Templates group and its Document Template button, which opens
  Templates and Add-ins: the template's path, Attach… to choose another,
  and "Automatically update document styles"; OK points the document at
  the template in the box, says whether it takes its styles, and when it
  does takes them at once, as Word does. `--picture … templates` draws it.
  *Proven by:* `crates/wp-docx/src/kinds.rs` — a template's red Heading 1,
  a style of its own and a list style numbering with arrows taken over a
  document's styles, the document's own style kept, the arrows the
  document's, and taking them again changing nothing;
  `crates/wp-app/src/editor/templates.rs` — a document that links its
  styles opening with its template's and unchanged, one that does not
  opening with its own, and the dialog attaching a template and taking its
  styles.
  *Found along the way:* taking a list style's numbering again on every
  opening would have given the document the same list once more each time;
  a list defined the same is now the one it keeps.
  *Not done, and named here:* the rest of Templates and Add-ins — global
  templates and add-ins, the XML Schema and XML Expansion Packs pages, and
  "Attach to all new e-mail messages" — which this program has nothing for;
  a template at a web address rather than a path, which is not fetched; and
  the Normal template and a template's building blocks, which stay named in
  **G1**.
- [x] **G16. An installer, and the file kinds it claims.** Double-clicking a
  `.docx` should open it here, and double-clicking a `.dotx` should make a new
  document from it; both are the shell's associations, and associations are
  the installer's to write. There is no installer. Named in **G1** and
  **H7**.
  *Done:* `wp-setup`, a crate of its own, and `word-processor-setup.exe` in
  `dist` beside the program. It is the setup program with the program and
  the command line after it — a ZIP archive, then where the archive starts
  and a mark that it is there — which is how installers have always been
  made: a program does not mind what follows it in its own file. `./x.sh
  win` makes it, the setup program built for the container putting the
  files after the Windows one (`tools/pack-installer.sh`); `./x.sh linux`
  makes the Linux one the same way. Run, it asks once and installs for the
  person running it, where Windows' own per-user installers go —
  `%LOCALAPPDATA%\Programs\Word Processor` — without an administrator: a
  manifest inside it says so, since Windows takes a program called setup to
  want one otherwise. A damaged download is found before anything is
  written; each file is written beside itself and moved over, so it is whole
  or not there; a program that is running is found on the first file and
  named; the files written are written down, and the setup program without
  the files is left beside them as `uninstall.exe`. Then the desktop is told
  (`wp_shell::install`): every kind the program opens, as Make Default
  registers them, now from one list in the shell (`wp_shell::files::KINDS`)
  that the window and the installer share; a template's two verbs as
  Word's — New, the default and so what a double-click does, making a
  document from it, and Open opening the template itself, which the program
  is told with `--open`; a document kind that nothing on the machine opens
  and the person has chosen nothing for is this program's, as it is any
  installer's, while a kind another program has is left to the Default Apps
  page, which the installer offers to open when it is done; Explorer's New ▸
  Word Document, an empty file, which the program opens as a blank document
  saved where the file is, as Word's does; a shortcut in the Start menu,
  made by the shell's own link object; and the entry in Settings ▸ Apps,
  with the version, the size, the folder and Uninstall, and nothing to
  modify or repair. Uninstall asks, and takes back that and nothing else:
  the files it wrote down and not whatever else is in the folder; the
  kinds, and an extension's program only where it was this one's, so that
  a kind it took goes back to having none; another program's entries and
  the person's choices untouched; the folder only if it is empty. It cannot
  remove itself while it runs, and leaves that to the command interpreter
  once it has stopped. `--quiet` asks nothing and says nothing, which is
  what the list's quiet uninstall runs. On Linux the same installer goes in
  `~/.local/share/word-processor`, and the desktop entry is both the menu
  and the kinds. Along the way, a double-clicked document that links its
  styles takes its template's, as one opened with File ▸ Open already did
  (**G15**'s, which only the Open command took).
  *Proven by:* `crates/wp-setup/tests/install.rs` — on Linux, end to end:
  the installer packed and run in a home of its own; the files, the
  uninstaller the installer without its files, the desktop entry starting
  the installed program, and the list with another program's lines kept;
  and then `gio`, the desktop's own tool reading what a file manager reads,
  naming this program for a Word document and starting the installed
  program with a document and with a template — a double-click without the
  mouse; installed again over itself with nothing listed twice; uninstalled,
  the files, the entry and its lines gone, a kind another program had
  before given back to it, a file of the person's own in the folder left
  there, and the folder gone when nothing else is in it; an installer
  carrying nothing refusing; `crates/wp-setup/src/lib.rs` — what is packed
  is what is carried, packing again replaces it, a damaged archive and a
  tail pointing past the file refused; `crates/wp-shell/src/linux/files.rs`
  — the list's other programs kept on a line in their order, taken off and
  given back; `crates/wp-app/src/editor/files.rs` — a Word document of no
  bytes opening blank, unchanged, and saving where it is. And the Windows
  half, run — for the first time anything of **H7**'s registry was — under
  Wine 10 in a container thrown away afterwards
  (`tools/check-installer-wine.sh`): the manifest in the installer and in
  the uninstaller; the files; every key — the template's New as the
  default verb and Open with `--open`, the unclaimed `.docx` taken, `.txt`
  left to the text file Wine has for it and the program only offered,
  ShellNew,
  RegisteredApplications, the Apps entry — and the Start menu shortcut
  naming the program; then the shell asked to open a document as a
  double-click asks it, which starts the installed program with the
  document and keeps it open, with a template making a new document from
  it, with Open on the template opening it, and with an empty `.docx`
  opening blank; then uninstalled, every key, the shortcut, the Apps entry
  and the folder gone, the uninstaller's own file too, and `.txt` still
  Wine's.
  *Found along the way:* registering on Linux put this program in place of
  another's in `mimeapps.list`'s Added Associations rather than beside it;
  now the others stay on the line, this one first where it takes the kind
  and last where it only can. A program in a folder with a space in its
  name would have broken the desktop entry's Exec line; it is quoted as the
  specification says. And the Windows build had never run anywhere; under
  the Wine of the build image's Debian, 8, it does not start at all — Rust
  asks `bcryptprimitives.dll` for random numbers before `main`, and Wine 8
  has none — nor from a folder shared from a Windows host, which Wine cannot
  map into memory. Neither is Windows; both are why the check runs where it
  does, on Debian 13 from the container's own disk.
  *Not done, and named here:* Wine is not Windows. Its HKEY_CLASSES_ROOT is
  the machine's classes without the person's, which Windows merges in, so
  the check copies the installer's keys across before asking the shell — on
  Windows the merge is the system's; and the Default Apps page, SmartScreen
  and Windows' own "how do you want to open this" are not Wine's to show.
  The installer's questions and messages — the message boxes, and on Linux
  the terminal or the desktop's dialog program — are English and are not
  run by the checks, which use `--quiet`. The program has no icon of its
  own, so the Start menu, the Apps list and every registered kind show the
  system's generic one. Not signed, so Windows warns about a download from
  an unknown publisher. No install for every user of the machine, which
  would want an administrator; no choosing the folder; `wp.exe` is
  installed beside the program but not put on the PATH. An upgrade is the
  installer run again, which replaces the files; nothing looks for a newer
  one. The installer and the uninstaller finding the program running and
  saying so is written against what Windows says of a file in use and is
  not run by either check.
- [ ] **G17. A budget for a hostile package.** Numbers taken from a file
  decide how much memory is asked for, with no ceiling but the type's. A
  package inflates every part into memory at once, so 64 parts of 256 MB of
  zeros pass the 512 MB per-entry limit together; a compound file's DIFAT
  chain is followed round a cycle for four billion steps and its FAT count
  reserves terabytes; an Agile encryption header's block size sizes a
  buffer and its spin count sizes the work, both before the password is
  even checked. One budget across the parts of a package, cycle checks and
  counts bounded by the file's length in the compound reader, and the
  encryption parameters checked against what is supported before a byte
  is allocated. Reviews R09/#2, R04, R05.
- [ ] **G18. A budget for a hostile picture or text file.** A 75-byte TIFF
  of one pixel with SamplesPerPixel in the millions allocates for the
  millions; an HTML cell with `colspan="4294967295"` resizes a vector to
  that; an RTF `\itap2147483647` pushes two billion table levels; a PDF of
  a million nested `[` and an XML of a hundred thousand nested `w:sdt`
  recurse until the stack goes. Limits checked before the allocation or
  the recursion, and a test each. Reviews R06/#3, R07, R08, R10, R11.
- [ ] **G19. Properties in the order the schema wants, and identifiers that
  are unique.** The writer has the ordered lists and does not always use
  them: `contextualSpacing` before `ind`, `strike` before `caps`, `w`
  before `spacing`, `tblBorders` and `tblLayout` before `tblCellSpacing`;
  `sectPr`'s order leaves out the header and footer references, so adding
  a first-page header puts `titlePg` between them. And every drawing,
  shape, chart, ink, diagram and group is written with `wp:docPr id="1"`.
  LibreOffice opens all of it; whether Word does is **K4**'s to say, and
  this item is not closed until it has. Reviews #8/R20, #9/R19, #12, #13.
- [ ] **G20. What the reader keeps and the writer drops.** A run's theme
  colour, theme font and text effect are read into the model and not
  written from it, so text pasted with Keep Source Formatting stops
  following the document's theme; a table's `tblInd` and `tblLook` are
  read and, when the table is built from the model, not written. Make the
  writer symmetrical with the reader and test it as a round trip through
  the model. Reviews #7/R17, #11.
- [ ] **G21. An unedited document saved as it came.** `Document::save` on a
  document nobody changed rebuilds the zip, so the file's bytes change on a
  save that changed nothing — a LibreOffice file of 5 580 bytes comes back
  as 6 093. Keep the original archive's bytes for a save without edits, and
  the original compressed streams of the untouched parts for a save with
  them; **K1**'s "identical" is then reachable for a file this program did
  not make. Review #4.

## H — The system around the window

- [x] **H1. IME.** Without it Chinese, Japanese and Korean cannot be typed at
  all: the composition window, the candidate list, and the text that is not yet
  committed shown in the document.
  *Done:* the shell takes the input method's messages — the composition
  starting, changing and ending, and the context being set with the
  input method's own composition window switched off, since the
  composition is shown in the document — and hands them on as events:
  the text composed so far with the caret's place in it and how each
  character stands (still being typed, converted, the clause being
  chosen, wrong), the text committed, and the end. The editor shows the
  composition where the text will go, in place of the selection, taking
  the formatting typing would take, with the caret where the input
  method puts it; every change replaces the last; a commit puts the text
  in as typed — one character at a time for a macro being recorded —
  and an end without a commit takes the composition out; the whole of
  it is one undo step. Under the composition: a dotted line under what
  is still being typed, a thin one under what is converted, a thick one
  under the clause whose conversion is being chosen, a red one under
  what could not be converted — Word's marks. The caret's place goes to
  the input method every time the caret is drawn, and the request for
  where a character is on screen is answered, so the candidate list
  opens beside the caret and keeps off the line. A box on the ribbon, a
  pane's search box and a dialog's field take what is committed, a
  character at a time. Along the way: a character past the basic plane
  — an emoji from the emoji panel — arrives as two halves and is put
  together, where before it was dropped. The composition cannot be seen
  from the build image, which has no input method: the state is tested
  through the events, and the marks are drawn in a picture.
  *Not done, and named here:* reconversion (the input method asking for
  committed text back to convert again); the composition shown inside a
  ribbon box, a search box or a dialog's field rather than only its
  result; the input method's own composition font; the Linux side,
  which is **H6**'s.
- [x] **H2. The clipboard formats Word uses.** `CF_HTML`, RTF, and images, so
  that copying between this and Word keeps the formatting.
  *Done:* a copy or a cut puts everything on the clipboard at once, each
  under its own format: the words as Unicode text; the same as Rich Text
  under "Rich Text Format" and as HTML under "HTML Format" — the page
  from the HTML writer with its pictures inline as data addresses, framed
  by the header of byte offsets the format asks for, the fragment marked
  inside the body; and a picture copied on its own as a PNG under "PNG"
  and as a device-independent bitmap under `CF_DIB` too, which is how it
  lands in a program that only takes pictures. The copied paragraphs are
  made a document of their own first, with the pictures they refer to
  brought along, so that the writers of the other formats can write them.
  A paste takes the richest thing it finds: what this program copied, when
  the clipboard still holds it; else Rich Text, which is what Word puts
  there and what says most; else HTML — read whole, from where its header
  says the page begins, so that the styles in its head reach the fragment,
  with an offset believed only where a tag really begins; else a PNG or a
  bitmap, given back the file header a `.bmp` starts with, as a picture at
  its own size and no wider than the text; else the words. Pictures in
  what another program put there are taken into this package before the
  paragraphs are pasted, through `Document::adopt_picture`, split out of
  `insert_picture` for the purpose, so the four paste options work on
  them as on anything else. Held to Word's own clipboard HTML — its
  namespaces, unquoted attributes, classes and empty `o:p` — and to Rich
  Text, HTML with a picture, and a bare bitmap, each pasted back with its
  formatting; and the copy checked in every format it goes out in. The
  build image has no clipboard, so the reading and writing are tested
  through the contents rather than the system.
  *Not done, and named here:* Word's own native format (the package it
  puts on the clipboard beside the others), which this program neither
  writes nor reads; tables pasted from Rich Text or HTML were said here to
  come as their paragraphs, and come as nothing — the paste drops the block
  and says nothing was pasted, which the review of tables found (T44) and
  **C61** takes up; a picture's alpha in the bitmap it goes out as is
  written but not every program reads it; the Linux clipboard, which is
  **H6**'s.
- [x] **H3. Drag and drop.** Between programs as well as within the document,
  and dropping a file onto the window.
  *Done:* the window is an OLE drop target, and gives OLE drags, with the
  three COM objects that takes — the drop target, the data object with
  its list of formats, and the drop source — written out against the
  documented tables, since no binding library is used. Dropped on the
  window: files from the desktop, of which a picture goes into the page
  where it landed, at its own size and no wider than the text, and a
  document — Word's kinds, the old binary one, Rich Text, OpenDocument, a
  web page, a text file, a PDF — opens, after the question about unsaved
  changes; and text another program is dragging, taken in the richest
  format it carries — Rich Text, HTML, a picture, the words — and pasted
  where it lands, with the paste options that follow a paste. While it is
  over the page the mark that shows where it would land follows the
  pointer, as for a drag inside the window; Control asks for a copy rather
  than a move, and the effect offered back to the source says which.
  Dragged out: text taken hold of in the selection and carried past the
  window's edge goes to the desktop as a drag, in every format a copy
  goes out in; a program that takes it as a move has the original come
  out, a copy leaves it, and a drag let go nowhere leaves it too. A drag
  that comes back into its own window lands the way a drag inside the
  window does — told afterwards, since the window is busy giving the drag
  while the drop happens. A modal dialog takes no drops. Tested through
  the events, since the build image has no desktop: text dragged in is
  pasted where it lands and the mark comes and goes; a picture dropped
  goes into the page; a document dropped opens.
  *Not done, and named here:* the mark following the pointer while this
  window's own drag is back over it (the window is busy giving the drag);
  a drag out of a file — the selection as a file the desktop could drop
  in a folder; dropping a document into the text as an object, which is
  what Word's Insert ▸ Object does; more than one document dropped at
  once, of which the first opens; the Linux side, which is **H6**'s.
- [x] **H4. Accessibility.** UI Automation on Windows, AT-SPI on Linux: a screen
  reader has to be able to read the document and drive the ribbon.
  *Done:* the Windows side. The window answers the system's request for
  its root provider with a UI Automation provider of its own — the root, a
  fragment for each control, and the patterns — written out as COM objects
  against the documented tables, since no binding library is used; the
  little of COM the shell speaks is in one place now, and the drag and
  drop of **H3** speaks it too. What the window says: the ribbon's tabs
  as tab items with their access keys, chosen or not, which a screen
  reader opens; the open tab's buttons as buttons named as their tips
  name them — "Bold", not the B on the face — with their shortcuts, the
  ones that are on or off as toggles that say which, which a screen reader
  presses; the document as a document with the keyboard, which is the Text
  pattern: the whole text, the selection, ranges that move and expand by
  character, word, paragraph and document, that are read back, compared,
  searched, given their rectangles on the screen line by line, and
  selected; and the status strip's buttons. Each control is found by its
  place on the screen, by its runtime id, and by walking from one to the
  next; the one with the keyboard is known; a press is announced as
  invoked; and every time the caret moves the screen reader is told the
  selection changed, so it reads what the caret is on. The editor gives
  all of it through the shell's accessibility methods on the application,
  and is held to: the tabs, the buttons and the document are described
  with their names, places and states; pressing the Insert tab opens it
  and pressing Bold makes the selection bold and the toggle say so; the
  text is read with its selection, offsets map to places and back, a
  selection is set from offsets, and a word has its rectangles. The
  screen reader itself cannot run in the build image, so the provider is
  held to compiling against the Windows ABI and the units tested.
  *Not done, and named here:* the Linux side, AT-SPI, which is **H6**'s
  and waits on a Linux shell; lines as the layout breaks them (the Text
  pattern's line unit is the paragraph); the attributes of a range (bold,
  font, size — every one answers "not supported"); the panes, dialogs,
  menus and lists as elements, which are reached today by the keyboard
  through the document's own key handling but are not described; the
  ribbon's boxes as edit controls; the rulers and the scroll bar; and
  announcing anything but the selection and a press — a dialog opening,
  a status message.
- [x] **H5. High DPI and several monitors.** Per-monitor scaling, and the window
  moving between monitors of different scales without redrawing wrongly.
  *Done:* the program declares itself aware of each screen's density —
  the per-monitor way with the frame drawn to match where Windows has it,
  the older way where it does not, each found by name so that the program
  still starts on the versions without them — and so is never stretched
  like a picture. Each window keeps the scale of the screen it is on:
  found when it comes up, from the screen the system put it on, and again
  when the system says the window moved to a screen of another density or
  the density changed, at which the window takes the size the system
  suggests and is drawn again. Everything the program draws is in the
  pixels of an ordinary screen, as every measurement in it has always
  been; the canvas scales them. That is the whole of the change above the
  shell: `Canvas::set_scale`, under which a rectangle is that many times
  bigger, a path — every letter, every icon — is scaled before it is
  rasterized and so comes out sharp, a picture is resampled to its bigger
  place, a mask and a pixel become blocks, a copy out and back covers the
  block, the clip scales, a shade is asked in the caller's coordinates,
  and the canvas reports the caller's size while its pixels are the
  screen's. Every coordinate the shell gives — the pointer, the size, a
  drop, a screen reader's point — is in the program's pixels, and every
  one it takes — the caret for the input method, a control's rectangle
  for the screen reader — is turned into the screen's. The frame of a
  maximised window is measured at the window's own density. A window
  opened by the program is asked for in its pixels and made in the
  screen's. Held to: a canvas at twice the scale draws the caller's
  rectangle, pixel and path as blocks and shapes of twice the size and
  gives back what was copied; the editor on a screen of twice the density
  draws the same window into twice the pixels, with the view the same size
  in its own, a point reading back the same through the scale, the ribbon
  found under the pointer where it was, and the controls described to a
  screen reader in the program's pixels; and a picture of the window at
  twice the density, `--picture … hidpi`, looked at.
  *Not done, and named here:* a screen reader's rectangles and a control's
  place while the window is being dragged between screens of different
  densities — they are right after the move, not during; the second view
  of a document on a screen of another density keeps its own scale but
  shares the document's layout, which is fine since the layout is in the
  program's pixels; the page rendered for printing is untouched, as it
  should be; a picture pasted or dropped is sized at ninety-six dots to
  the inch whatever the screen, which is what Word does; and the Linux
  side, which is **H6**'s.
- [x] **H6. The Linux shell.** X11 and Wayland: window, input, clipboard,
  presentation. The rest of the program is already portable.
  *Done, for X11:* the shell speaks the X protocol itself, over the
  server's socket, with no Xlib and no library of any kind — the
  connection and its cookie, atoms, windows, properties, selections,
  images, cursors, the keyboard map, the big-requests extension for a
  whole window's pixels in one request — so the same rule that keeps the
  Windows shell to the operating system's own ABI holds here. A window is
  made without the frame the window manager would put on it, since the
  program draws its own caption, and asks the window manager for what a
  frame would do: moving and resizing by the caption and the edges
  through `_NET_WM_MOVERESIZE`, minimising through `WM_CHANGE_STATE`,
  maximising through `_NET_WM_STATE`, closing through
  `WM_DELETE_WINDOW`, with the program's own close command going round
  through the same message so that the editor is asked about unsaved
  changes exactly as it is for the frame's button. Keys come as keysyms
  read off the server's own map, with Shift, Caps Lock and AltGr applied
  the way the server would, and as characters from the Latin-1, Latin-2,
  Greek, Cyrillic, keypad and Unicode keysym ranges; Alt and Control
  pressed alone are the menu and control keys; the wheel is buttons four
  to seven; a double click is by the server's time and the pointer's
  distance. The canvas is put on the window as the server's own pixel
  format, in bands under the request limit where there is no big-requests
  extension. The clipboard is the `CLIPBOARD` selection: text, HTML, RTF
  and PNG offered under their own targets and taken from another program
  under the same, with `TARGETS` answered. The screen and any window are
  photographed with `GetImage`; the windows for Word's Screenshot button
  come from the window manager's stacking list, or from the server's own
  tree where there is no window manager. The screen's density comes from
  `Xft.dpi` in the root window's resources. The desktop's own dialogs —
  open, save, save changes?, a question, an error — are shown through
  `zenity` or `kdialog`, whichever the desktop has, with the safe answer
  where it has neither. The image
  for tests carries Xvfb, an X server that draws into a file, and against
  it: a window opens at the size asked, paints, is read back off the
  server's own frame buffer — the shell's paint, not its own memory — and
  through the shell's screenshot, which agree; the window list names it;
  text put on the clipboard comes back through the selection; a program
  that refuses to close on the first request stays up until it agrees;
  and the whole editor — ribbon, rulers, page, fonts — comes up on it,
  is photographed, and closes from its frame's request. Both pictures
  looked at.
  *Not done, and named here:* Wayland, which is **H8** and now done; the X input
  method (XIM), so Chinese, Japanese and Korean cannot yet be composed on
  Linux — typed characters arrive, compositions do not; drag and drop
  between programs (XDND) — the events exist, nothing on Linux raises
  them; AT-SPI for a screen reader; a selection handed over in pieces
  (`INCR`), which is a clipboard content over the server's request limit
  — it is declined rather than taken; printing, which is **A5**'s;
  `xdg-open` for a link is used but not proven, there being no browser in
  the image; and Xft.dpi is the whole of density on X11, since X11 has
  nothing else to say — a screen with none set is an ordinary one.
- [x] **H7. Files the way an operating system means them.** Recent documents,
  file associations, the shell's open and save dialogs, autosave and recovery
  after a crash.
  *Done:* four things, and they are what a document being a file rather
  than a thing inside one program means.
  **The desktop's own list of documents opened lately.** A document opened
  or saved is put on it as well as on the program's own Open page — on
  Windows through the shell's recent documents, which is the taskbar's jump
  list and the Recent place in Explorer; on Linux by writing the bookmark
  file the specification names, `recently-used.xbel`, with the document's
  address, what kind of document it is and which program used it, newest
  first, the same document moving up rather than appearing twice, and
  every other program's bookmarks left as they were.
  **Which program opens which kind of file.** The program registers every
  kind it can read — the Word documents and templates, the older binary
  document, Rich Text, OpenDocument, plain text, a web page — so that Open
  With offers it for all of them, and asks to be the one that opens the
  documents among them and not the plain text or the web page, which
  already have programs and are not this one's to take. On Linux that is
  the whole of it: a desktop entry in `applications` naming the media
  types, and `mimeapps.list` saying which entry opens what, with the other
  entries in it untouched. On Windows the kinds are registered under the
  person's own classes — a program identifier apiece with its name, its
  icon and its command, the extension's Open With list, the program's
  supported types, and the capabilities the Default Apps page reads — and
  then that page is opened, because which program opens a kind is the
  person's choice and Windows keeps it where a program cannot write it.
  Word's own General page says whether documents open here, and its Make
  Default button does the above and then says what came of it.
  **The system's open and save dialogs**, which were already this program's
  only ones: `comdlg32` on Windows, the desktop's own dialog program on
  Linux.
  **A copy of the work that outlives the program.** Word's AutoRecover: a
  copy is written every ten minutes — the setting is on the Save page of
  Options, with how often, whether the last copy is kept when a document is
  closed without saving, and where the copies go — into a folder of the
  program's own, and only for a document with changes that are not on disk.
  Saving takes the copy away, because the work is now where the person put
  it, and so does closing properly, which is what makes a copy still lying
  there on the next start mean a run that did not end. The next start finds
  those and shows Word's Document Recovery pane, which takes the left of the
  window: each copy by name, when it was taken, the one being looked at
  marked, a cross that throws one away, and a Close that asks before it
  throws away the rest. Opening one brings the work back under its own name,
  pointing at the file it came from and holding changes that are not saved,
  so Save puts it where it belongs and closing asks.
  Held to: a copy is written for unsaved work and not for saved work; it is
  found and opened again as another run's, with the text that was typed; a
  save takes it away; the desktop's list gets a document that was saved,
  with its kind, and does not list it twice; registering makes this program
  the one that opens a document while leaving the plain text file with
  whatever had it and another program's choices alone; and pictures of the
  recovery pane and of the two Options pages, looked at.
  *Not done, and named here:* the Windows half — the registry and the
  shell's recent documents — is written against the documented calls and
  compiles for Windows, but nothing in the build container is Windows, so
  it has not been run; the Linux half is what the tests cover. Word's jump
  list has tasks and a pinned list of its own, which needs the shell's
  custom destination lists; this puts documents on the ordinary recent one.
  The pane's rows open and throw away, where Word's row menu also offers
  Save As and Show Repairs. Word can list the copies of never-saved
  documents at any time, through Manage Document; here they are offered at
  the next start and not after that. The time on a row is the clock's, in
  UTC, until dates in the person's own zone and form arrive with **I3**. A
  copy is always written as a Word document, so a text file recovered comes
  back as one until it is saved again. There is no way to say "stop opening
  documents with this program": registering can be undone from the
  desktop's own page, and taking another program's kinds away from it in
  its absence is not something this program should offer.
- [x] **H8. Wayland.** The Wayland shell alongside the X11 one, chosen at
  start by `WAYLAND_DISPLAY`.
  *Done:* the protocol is spoken over the compositor's socket, message by
  message, with no `libwayland` and nothing generated: the interfaces are
  written out with each opcode beside its name, and what is not written
  out is what this program does not say. A window is a surface with a
  window role — `xdg_wm_base`, `xdg_surface`, `xdg_toplevel` — which the
  program does not draw into until the compositor has said how big it is
  to be, because on Wayland that is the compositor's to say. The pixels
  go through memory both programs can see: a file made by `memfd_create`,
  sized and mapped, handed over as a descriptor. A descriptor cannot be
  written into a stream, so it goes beside the message as ancillary data
  — `sendmsg` and `recvmsg`, which with the four that make and map the
  file are the whole of what this program declares against the C library,
  the same rule the Windows shell follows with Win32.
  The window has no frame of the compositor's, since the program draws
  its own: the caption and the edges ask the compositor to move and
  resize through `xdg_toplevel`, and minimising, maximising and closing
  go the same way. The keyboard arrives as a file in xkb's own text
  format, which this program reads — the key names and what each
  produces at each level — rather than handing to `libxkbcommon`; Shift,
  Caps Lock and the third level work as they do everywhere, and a
  keyboard that appears or goes away is taken and let go as the seat says
  so. The pointer brings its own cursor, because a Wayland client has no
  other kind: the arrow, the I-beam, the resize arrows and the hand are
  drawn here and handed over as a surface. The clipboard is
  `wl_data_device`: text, HTML, Rich Text and PNG offered and taken
  through a pair of joined sockets. The density is the compositor's
  preferred scale, as a whole number or as a fraction where it offers
  one.
  Held to, against sway running with no screen in the build image: a
  window opens, is painted, and is photographed by `grim` — another
  client, so what is checked is what the compositor is showing rather
  than what this program believes it drew; `wtype` types into it and the
  keys arrive as characters; text put on the clipboard comes back; and
  the whole editor comes up the same way, takes "Hello from Wayland" into
  its document, refuses to close while that typing is unsaved, and closes
  when it is saved. Both pictures looked at. Two faults found by those
  tests and fixed: arguments were padded to eight bytes where the
  protocol pads to four, and a buffer the compositor had finished with was
  never marked free again — which stopped the window being redrawn after
  its second frame.
  *Not done, and named here:* a client cannot photograph the screen or
  list another program's windows on Wayland — that is the protocol's own
  rule, and Word's Screenshot button therefore finds nothing there; doing
  it properly needs the desktop's portal, which is D-Bus and is not in
  this program yet. Drag and drop between programs (`wl_data_device`'s
  half of it) and the input method (`zwp_text_input_v3`) are not written,
  so Chinese, Japanese and Korean cannot be composed on Wayland any more
  than on X11. Windows are not arranged side by side — Word's Arrange All
  — because a Wayland client cannot place its own window. Printing is
  **A5**'s. The keymap reader takes the levels and the names and not
  compose sequences or dead keys, which belong to an input method.
- [x] **H9. Composing Chinese, Japanese and Korean on Linux.** The input
  method: XIM on X11 and `zwp_text_input_v3` on Wayland. Typed characters
  arrive on both today and compositions do not, which means the languages
  half the world writes in cannot be typed there at all. **H1** is the
  Windows half and is done. Named in **H6** and **H8**.
  *Done:* both, raising the events **H1** made for Windows — the text
  composed so far with its caret and how each character stands, the text
  committed, and the end — so the editor, its ribbon boxes, panes and
  dialogs take them as they already did.
  **X11: the X Input Method protocol** (`crates/wp-shell/src/linux/xim.rs`),
  written out against the X Consortium's specification. The server is the
  one `XMODIFIERS` names, found as the owner of its name; the line to it is
  X's own — client messages between a window of each, a property for what
  does not fit in one, the pieces of a long message put together, and
  several messages the server adds to one property taken one at a time as
  their client messages come. On it: connect in this program's byte order,
  open the input method in its locale, agree on UTF-8 or compound text,
  ask which styles it offers and take on-the-spot — the text being composed
  drawn by the program where it will go, as Word's is — or else the input
  method's own window, or else only what it commits; an input context for
  each window, focused and unfocused with it. Each key the server asks for
  goes to it first, and the next waits while the server said to answer
  first; what comes back is the key again, typed as before, text committed,
  or the text being composed drawn, redrawn a range at a time and its caret
  moved — each answered as the protocol says. A server that switches on
  with a trigger key is sent nothing until one is pressed. The caret's
  place goes to the server when it moves, so its list of candidates opens
  beside it. Compound text is read: ASCII and the ISO 8859 halves this
  program has tables for, GB 2312, JIS X 0208 and KS C 5601 through the
  code pages that contain them, JIS X 0201, the UTF-8 segment, and the
  extended segments naming Big5, GBK or UTF-8. A server that goes away, or
  does not answer a key for three seconds, is let go: the keys that waited
  are typed and typing goes on without it.
  **Wayland: text-input, version 3.** On Wayland the input method is the
  compositor's and has the keys before this program does. The window with
  the keyboard says it takes ordinary text and where its caret is; each
  batch of what the input method sends — text committed, then the text
  being composed with its cursor as a range of bytes — is taken whole at
  `done`, the text committed going in and the composition replaced; a range
  that is not empty is shown as the part being chosen for.
  *Proven by:* `crates/wp-shell/tests/x_input_method.rs` — the X shell on
  Xvfb typing through uim-xim, a real X input method server, running byeoru,
  its Korean input method, with xdotool pressing Shift+Space and g k s r m
  f through the server's test extension: 한글 committed a syllable at a
  time, ㅎ 하 한 shown as the first was built and 그 as the second, the
  letters that spelled it never arriving as typed, and the space the input
  method gave back typed; `crates/wp-app/src/xserver.rs` — the same through
  the whole editor, the word in the document; `crates/wp-shell/tests/
  wayland.rs` — the Wayland shell on sway, with an input method of the
  test's own on sway's input-method protocol composing the same word:
  shown as it grows, a chosen range marked, each syllable committed, and in
  sway's own record of the conversation the window enabling text input and
  its caret rectangle arriving; `crates/wp-shell/src/linux/xim.rs` — the
  conversation message by message, a key held while one is unanswered, a
  server that goes quiet giving the keys back, the style chosen from those
  offered, and compound text in each of its sets.
  *Found along the way:* a server writes each message after the last in the
  same property and sends a client message for each; reading the whole
  property for the first lost the second — the caret message after the
  first letter — and the server then waited for an answer that never came.
  The first message is taken and the rest written back, as Xlib does.
  *Not done, and named here:* the text around the caret is not said to the
  input method on either — X's string conversion, Wayland's surrounding
  text — so an input method that reconverts committed text, or deletes
  around the caret, cannot; the composition's own font; an X input method
  that insists on over-the-spot, which is not taken; a Japanese or Chinese
  input method run against the shell, uim's Korean being the one in the
  build image — the protocol is the same, and the compound text of the
  other scripts is held to their code pages; Wayland's input-method popup
  for candidates, which is the input method's to make; and the Wayland
  test's input method is the test's own, its other end being sway's relay.
- [x] **H10. Drag and drop between programs on Linux.** XDND on X11 and
  `wl_data_device`'s half of it on Wayland. The events exist inside the
  program — **H3** built them for Windows — and nothing on Linux raises them.
  Named in **H6** and **H8**.
  *Done:* both directions on both, raising **H3**'s events and answering
  its one call, so the editor takes and gives drags on Linux as it does on
  Windows.
  **X11: XDND**, version 5, in the X shell. Every window says it takes
  drops. A drag coming over one is told, position by position, whether it
  would be taken and as a copy or a move — a move where the giver asks for
  one — while the window is told where it would land; let go, what it
  carries is asked for as the drop's own selection in the formats this
  program takes — the words, HTML, Rich Text, a picture — or, for files,
  their addresses, read back as paths with their escapes undone; and the
  giver is told it is done and what was done. Giving a drag: the formats
  offered, the selection taken, the pointer taken for the length of it; the
  window that takes drops under the pointer found through the window
  manager's frames, told the drag has come and where it is, one position
  at a time as it answers; let go where it was taken, the drop is made and
  the selection served until the taker says it has finished — a copy or a
  move — and let go on one of this program's own windows it is where it
  landed; Escape gives it up.
  **Wayland: the data device.** A drag over a window: which format would
  be asked for, and that a copy or a move would do, a move rather; where it
  is as it moves; let go, what it carries read, and the offer finished.
  Giving one: a source in every format the contents have, copy or move,
  started from the button press that began it; the compositor's answers
  followed until the drop is finished or cancelled; and come back over its
  own window, where it landed, without asking itself for what it carries.
  Along the way: reading a pipe the compositor hands over, and writing one,
  are one function each, for the clipboard and the drags alike, the last
  unsafe line in that code gone with it.
  *Proven by:* `crates/wp-shell/tests/x_drag_and_drop.rs` and
  `crates/wp-shell/tests/wayland.rs` — against GTK, which is somebody
  else's reading of both protocols (`tools/dnd-peer.py`, a window that
  gives a drag or takes a drop and says what it was), on Xvfb with xdotool
  moving the pointer and on sway with a virtual pointer of the test's own:
  text dragged from GTK dropped where it was let go, the place followed on
  the way, and GTK told it was taken; a file dragged from GTK arriving as
  its path, spaces and all; text dragged out taken by GTK — on X as a move,
  so the original is to come out; and a drag let go on its own window
  saying where. `crates/wp-app/src/xserver.rs` — the whole editor on Xvfb,
  text dragged from GTK going into the document.
  *Not done, and named here:* as on Windows, the mark following the
  pointer while this window's own drag is back over it, and a drag out of
  a file; a drop handed over in pieces (`INCR` on X), which a large picture
  would be, and which is refused as the clipboard refuses it — **H14**'s;
  a window that stands in for another (`XdndProxy`), which some desktops'
  own windows are; the ask action, which neither side offers; and on
  Wayland a picture of what is dragged under the pointer, where the
  compositor shows its own.
- [x] **H11. AT-SPI: what a screen reader on Linux is told.** The Windows
  half is **H4**; this is the same information over the other desktop's
  interface, which is D-Bus and is not in this program yet. Named in **H6**.
  *Done:* D-Bus, and AT-SPI over it, on both Linux shells.
  **D-Bus** (`crates/wp-shell/src/linux/dbus.rs`), written out against the
  specification: the wire format both ways for every type but the file
  descriptor, in either byte order, aligned as it says; a bus reached by the
  address the desktop gives, by path or by abstract name; the EXTERNAL
  authentication a local socket takes; a name taken; calls sent and their
  answers waited for while whatever else arrives is kept; and what has
  arrived handed over without waiting.
  **AT-SPI** (`crates/wp-shell/src/linux/atspi.rs`). Once the window is up
  the session bus is asked for the accessibility bus — which starts it, as
  a desktop does — and the application's tree is given to the registry
  there. The tree is what UI Automation is told on Windows, from the same
  accessibility methods on the application: the application, its window
  by its title, and each control — the ribbon's tabs as page tabs, chosen
  or not; its buttons and its toggles, on or off, with their access keys,
  each of which a screen reader presses; the document as document text
  with the keyboard, editable and of many lines; and words. Each object
  says its role, name, states, parent, children, interfaces and place, on
  the screen or in the window; the document is read by character, word,
  sentence and line, its caret and selection read and set, and any
  stretch's place given; the whole tree comes at once to a reader that
  asks for it that way. And the screen reader is told: when the caret
  moves and the selection changes, and — looked over after every press or
  selection it asks for and every third of a second otherwise — when a
  control is turned on or off, chosen, given the keyboard or renamed, and
  when controls come and go, as a tab chosen brings its own buttons, which
  is how a screen reader's copy of the tree stays the program's.
  *Proven by:* `crates/wp-shell/tests/x_accessibility.rs` and
  `crates/wp-shell/tests/wayland.rs` — a session bus of the test's own, the
  accessibility bus started on it as a desktop's is, and
  `tools/atspi-reader.py` reading the window through Atspi, the library
  Orca is written on, on Xvfb and on sway: the tree with each role, name
  and state; the document's text, caret, selection, a word at an offset
  and a character's place; a toggle pressed, the program asked to press it
  and nothing else, and the reader then knowing it is on; a selection made
  and the program asked for it; and the caret's move heard.
  `crates/wp-app/src/xserver.rs` — the whole editor read the same way: the
  ribbon's tabs with Home chosen, its toggles, and the document's text.
  *Not done, and named here:* one window is described, the one the program
  last heard from, where Word's several windows would each be a frame; on
  Wayland a place on the screen is the window's own, since Wayland does not
  tell a program where its window is; the offset at a point is not found,
  there being no way yet to ask the layout; the keys pressed are not told
  to the registry, which is how Orca echoes what is typed; and the controls
  are looked over whenever there is an accessibility bus, not only while a
  screen reader is listening. Orca itself is not run: Atspi is its library,
  and what Orca would say of what Atspi reads is Orca's. Lines as the
  layout breaks them, a range's attributes and the rest of **H4**'s list
  stay **H12**'s.
- [x] **H12. The rest of what a screen reader is told.** Lines as the layout
  breaks them rather than paragraphs; the attributes of a range — bold, font,
  size — which every one of today answers "not supported"; the panes,
  dialogs, menus and lists as elements of their own; the ribbon's boxes as
  edit controls; the rulers and the scroll bar; and announcing what happens
  rather than only where the selection is — a dialog opening, a status
  message. Named in **H4**.
  *Done:* on both platforms at once, since UI Automation and AT-SPI read
  the same accessibility methods on the application.
  **What an element can say** (`crates/wp-shell/src/lib.rs`): the element
  it is inside, what it holds, and — for what stands between two ends —
  the ends and where it is; twelve more kinds — dialog, pane, menu and
  menu item, list and list item, edit box, tick box, box with a list,
  scroll bar, ruler, status strip; and three more questions of the
  application: the document's lines, how the text at an offset is set and
  how far that goes, and a value written into an element.
  **What the editor says** (`crates/wp-app/src/editor/accessible.rs`),
  each part from the account the drawing used. The ribbon's every button:
  only the commands with a name to be found by had a number before, so the
  buttons that carry a picture and no word, the arrows in the groups'
  corners, the font and size boxes, the gallery of styles and a squeezed
  group's button were never told of; now they are, the gallery as a list
  of styles with the one in effect chosen and the squeezed group by its
  group's name. The measurement boxes as edit boxes and the font and size
  boxes as boxes with a list, each with what it shows; a value written
  into one is applied as the same thing typed or chosen would be. The
  panes: the navigation pane with its close button, its tabs, its search
  box — written into, it searches — and the rows of its list, the current
  heading or page chosen; Document Recovery, Styles, Restrict Editing,
  Signatures, XML Mapping, the Text Pane, the Translator and the
  comparison column by name. The strip across the top of the page, for
  finding and the rest of what it is opened for, with its fields, buttons
  and toggles; the rulers; the scroll bars with how far they have run and
  may, which a value written into moves; the status strip with its message
  as its value, what it says of the document, and its buttons. A dropped
  list as a menu called what it dropped from, its items with the one in
  effect chosen, the one lit by the keyboard having it, and choosing one
  doing what choosing it does. A dialog with its close button, tabs,
  buttons and fields, each by its kind — a text, number or password box
  (the password as dots), a tick box, a box with a list, a list with its
  rows (a row with a tick box being a tick box), a box of lines as a list
  of them, a caption — pressable and, where they hold something, written
  into. One element has the keyboard: whatever is in front, else a box
  being typed into, else the document. The lines are the layout's, run on
  from one another; a stretch's attributes are its run's — font, size,
  bold, italic, underline, strike-through, colour, highlight — with the
  run's extent (`Document::formatting_at`). The tips of the buttons that
  had none in the message list are in it now, and in German.
  **AT-SPI** (`crates/wp-shell/src/linux/atspi.rs`): the new kinds by
  AT-SPI's roles and states — modal, single-line, expandable, vertical or
  horizontal; the tree built from what each element is inside; Value, and
  EditableText on an edit box; Text on the boxes and the status strip;
  lines from the layout; the attributes, a run and the defaults. And heard:
  a dialog's window created and activated, the keyboard moving, names and
  values changing, a box's text replaced, controls coming and going under
  their own parent — the status strip's message as a polite live region.
  **UI Automation** (`crates/wp-shell/src/uia.rs`): the control types,
  dialog, live setting and toggle state; Value and RangeValue; navigation
  through what each element is inside; the text range's line unit from the
  layout and its format unit from the runs, and eight attributes — mixed
  where a range crosses runs; and raised, while a client listens: a window
  opened, the focus moved, the structure changed, a property changed, and
  a status message as a notification.
  **Waiting on both connections** (`crates/wp-shell/src/linux/wait.rs`):
  each Linux shell waited on its display alone, a tick at a time, so every
  question a screen reader asked waited for the tick — reading the editor's
  ribbon took thirty seconds. Both are waited on together now, and it
  takes a fifth of one; a bus that has gone is no longer waited on.
  *Proven by:* the editor's own tests — every control on every tab told
  of by a name, the boxes' values written and applied, a dialog's parts,
  focus and OK, a tree row's tick box pressed, a menu's items and one
  chosen, the rulers, scroll bar, status strip and navigation pane, lines
  and attributes. `crates/wp-shell/tests/x_accessibility.rs`, read through
  Atspi on Xvfb: the kinds, values and states, the tree by parent, a line
  and a run's attributes, a box written into, and a dialog opening heard —
  its window activated, the keyboard on its first field, the status
  message — and read. The Wayland shell answers from the same code, and
  `crates/wp-shell/tests/wayland.rs` still reads **H11**'s tree, text and
  events on sway through the new waiting; the new kinds are not asked of
  it there. `crates/wp-app/src/xserver.rs`: the whole editor — the
  font and size boxes with their values, the navigation pane and the find
  strip with the keyboard in its box, the rulers, the scroll bar, the
  status strip, the layout's line, the run's attributes, the pane's search
  box written into, and the Paragraph dialog opening heard with the
  keyboard on Alignment and read field by field.
  *Not done, and named here:* the UI Automation side is compiled against
  the Windows ABI and not run — there is no UI Automation client in the
  build image, and Wine's has not been tried. The panes other than the
  navigation pane are named and not read inside; the File page, the colour
  palette, the mini toolbar, the paste and correction buttons, the key
  tips, the information bar, the title bar's buttons and the zoom slider
  are not described; a dialog's dropped list is not, though its box says
  what is chosen. The rulers' markers are not elements; a stretch's
  attributes are the characters', not the paragraph's, its language or its
  style; a font that comes from nowhere but the program's default is not
  named. The offset at a point, several windows and the keys pressed are
  still as **H11** left them; and Orca, NVDA and Narrator themselves are
  not run.
- [x] **H13. The desktop portal.** D-Bus, and the two things on Wayland that
  cannot be done without it: a screenshot, which the protocol forbids a
  client to take for itself and which Word's Screenshot button therefore
  finds nothing for, and windows arranged side by side, which a Wayland
  client cannot do to itself either. Named in **H8**.
  *Done:* the screenshot through the portal, and the windows through the
  compositor, since the portal turned out to have nothing for them.
  **The portal** (`crates/wp-shell/src/linux/portal.rs`), over the D-Bus
  **H11** wrote: `org.freedesktop.portal.Screenshot`, asked on the session
  bus. The answer is not the call's reply but a signal on a request object
  whose path the program works out beforehand from its name on the bus
  and a token of its own, and listens for before it calls, so that a
  desktop that answers at once is heard; the signal says done, cancelled
  or failed, and where the picture was written, which is read as a PNG.
  Asked for the whole screen without questions, or — Word's Screen
  Clipping — for a rectangle the person drags out in the desktop's own
  way. On Wayland the whole screen is now the portal's picture, and
  Screen Clipping is offered under it whenever the portal says it takes
  screenshots; there are still no windows listed, since Wayland lists no
  one else's. The list is taken off the screen, and the window drawn
  again without it, before the picture is taken — on every platform: the
  picture had been taken at the press, of the window with the list still
  open over it. A clipping cancelled says no picture was taken.
  **Arranging** (`crates/wp-shell/src/linux/sway.rs`): the portal's own
  list of interfaces, read from a running one, has screenshots, screen
  recording, files, printing and the rest, and nothing that places a
  window; so Arrange All is asked of the compositor in the compositor's
  language, which for sway is i3's — a socket, framed messages, JSON (read
  by `crates/wp-shell/src/linux/json.rs`). The program's windows are
  found in sway's tree by its process number, made tiled, gathered in
  order into one container by marks, and laid across; other programs'
  windows stay where they were. On a desktop the program cannot ask,
  Arrange All says the desktop places the windows itself, where it said
  only one window was open.
  *Proven by:* `crates/wp-shell/tests/wayland.rs` on sway, with a session
  bus of the test's own and xdg-desktop-portal woken on it, choosing its
  wlr half for sway, which photographs with grim and lets a rectangle be
  dragged with slurp: the whole screen came back 640 by 480 with the
  window where it was, and a rectangle dragged by the test's own pointer
  came back 201 by 151 and all window — both pictures looked at; and two
  windows made floating by swaymsg, arranged, and read back by swaymsg as
  tiled, in one container laid across, side by side and equally wide
  across the whole screen. `crates/wp-app/src/wayland.rs`: the editor
  pressed through what it tells a screen reader — Insert, Screenshot,
  which offers the whole screen and Screen Clipping and no windows; the
  whole screen put in the document as a picture 1600 by 900; and a
  clipping waited for until the person pressed Escape to slurp, and
  nothing put in.
  *Not done, and named here:* the portal is asked with no parent window,
  since naming one takes `xdg_foreign`, which this program does not
  speak; while a clipping is dragged the program waits for it and draws
  nothing. Screen Clipping on X and on Windows, where the program would
  have to lay a window of its own over the desktop, is not written.
  Arranging is asked of sway alone: KWin, Mutter and the other Wayland
  compositors each have their own language or none — GNOME's has none
  open to a program — and there nothing moves. The portal of version
  1.16 offers screenshots only beside a permission dialog, which the
  test image has from xdg-desktop-portal-gtk.
- [x] **H14. The rest of the clipboard.** Word's own native format — the
  package it puts on the clipboard beside the others — which this program
  neither writes nor reads, so a paste between two copies of Word carries
  more than a paste from here does; and a selection handed over in pieces
  (`INCR` on X11), which today is declined rather than taken. Named in **H2**
  and **H6**.
  *Done:* both.
  **Word's own format** (`crates/wp-shell/src/embedded.rs`): what Word
  offers beside its text, Rich Text and HTML is the copy as an object —
  "Embed Source", which its Paste Special calls a Microsoft Word Document
  Object — with an "Object Descriptor" saying whose it is. The object is a
  compound file whose root names the class `Word.Document.12`, with the
  `\x01CompObj` stream naming it in words ([MS-OLEDS]), the `\x01Ole`
  stream of an embedded rather than linked object, and the document itself
  as a `.docx` package in a `Package` stream. A copy now goes that way too,
  from the package the copied paragraphs were already made into — which
  now carries this document's styles, their defaults and the lists they
  number with, so the Rich Text and HTML written from it do as well: until
  now a paragraph in a style of this document's went out in a new
  document's style of that name, or in none. A paste takes the package out
  of an object Word put there, by
  its class or its names, before anything else, since it is the whole of
  what was copied. `wp-ole` now reads and writes the class a storage
  names. A drag from Word carries the object as a storage, which a drop
  does not ask for; the drop is taken from the other formats as before.
  **Pieces on X** (`crates/wp-shell/src/linux/xshell.rs`): a selection
  handed over as `INCR` is taken — the property taken off asks for the
  first piece, each piece taken asks for the next, an empty one ends it,
  and a notification of a writing already taken is told from a piece by
  its type. And one bigger than a quarter of a megabyte is handed over
  that way: its size first, then a piece each time the program asking
  takes the last, its window's property changes asked for meanwhile and
  given back after, and given up on if half a minute passes untaken.
  *Proven by:* `crates/wp-shell/tests/x_clipboard.rs` against xclip on
  Xvfb, both ways round, three megabytes of lines each different: taken
  whole and in order from xclip, which hands that much over in pieces —
  the test fails with pieces declined, so it does — and given whole to
  xclip in pieces of this program's. `tools/check-clipboard-wine.sh` runs
  `crates/wp-shell/examples/ole-clipboard.rs` under Wine, whose OLE
  clipboard and storage are Windows' interfaces written by somebody else:
  the copy put on the clipboard by the shell was offered by OLE as a
  storage, which is how Word asks for it, and read there — the class
  Word's, the package the one copied, the streams' names and kind — and
  the descriptor read with Word's class and name; and a storage made by
  OLE's own writer, holding a package and put on the clipboard as Word
  puts one, was read back by the shell as the document in it. The editor's
  own tests: a copy's package carrying a style this document made, as
  this document has it; and a document on the clipboard pasted before the
  Rich Text beside it, with what Rich Text from here would not have said.
  The test of the editor read through AT-SPI was made to open its find
  strip before the window's first tick: it could be asked for the strip's
  parts in the moment before the strip was first drawn.
  *Not done, and named here:* Word itself has not pasted what this program
  copies, nor copied what this program pasted — there is no Word in the
  build image — so what is held to is the format as documented and as OLE
  reads and writes it. Whether Word pastes an object from another program
  as the document in it or as an embedded object is Word's to decide;
  Paste Special offers it either way. The lists a paragraph numbers with
  directly, rather than through its style, are not carried into the copy.
  Linux has no program that asks for Word's format, and none is offered
  there.
- [x] **H15. The rest of Word's recent documents.** Its jump list has tasks
  and a pinned list of its own, which needs the shell's custom destination
  lists; its row menu offers Save As and Show Repairs; and Manage Document
  lists the copies of never-saved documents at any time, where this offers
  them at the next start and not after that. Named in **H7**.
  *Done:* the row menu and Manage Document; and the jump list found, on
  looking, to need nothing more.
  **The jump list.** Looked at again, Word's has no tasks of its own —
  what people ask for is a way to pin a new blank document there, which
  it does not have — and its Pinned and Recent are the shell's own list of
  a program's documents, the one a document is put on through
  `SHAddToRecentDocs`, which **H7** does, pinned to by the person with the
  pin the shell draws, which it draws for a program registered to open the
  kind, which **G16** does. A custom destination list is for a program
  whose list is not that one, and Word's is; so none is written.
  **The row menu** (`crates/wp-app/src/chrome/recoverypane.rs`,
  `crates/wp-app/src/editor/recovery.rs`): each row of the Document
  Recovery pane drops Word's four — Open; Save As, which opens the copy
  and asks where to keep it, after which the copy goes, as a copy does
  when the work reaches the disk; Delete; and Show Repairs, which says
  whether the copy opens as it was written or is damaged past repair and
  why — in place of the cross that only threw one away.
  **Manage Document** (`crates/wp-app/src/editor/backstage.rs`): a line
  on the Info page under the signatures, saying how many copies of unsaved
  work are kept, which opens them in the same pane at any time — what a
  run that did not end left and what "don't save" left in this run, all
  but the copy of the document being edited now. On the way, a fault:
  the copy "don't save" left kept this run's name, so the next document
  edited in the same run was copied over it; now it is left under its own
  and the run goes on under a new one, counted, since two can come in one
  second.
  Also on the way, in a window read right to left: eleven parts of the
  window — the menus, the mini toolbar, the find strip, the table grid, the
  colour palette, the navigation, print, recovery and styles panes, the
  File page and the title bar — turned the pointer about once to light
  what was under it and then again in the function that looks, so they lit
  what was mirror-opposite; and a menu and the mini toolbar did the same
  twice over in the looking itself, so a press on a menu's row could miss
  it. Each is turned once now.
  *Proven by:* the menu's four, each doing what it says — a sound copy
  and one cut short read by Show Repairs, one thrown away by Delete, and
  one opened by Save As and kept where no dialog was there to say where;
  the pane with the menu dropped, looked at; a copy left by "don't save"
  still whole after the next document's copy, offered by Manage Document
  and not taken for a crash's; and a menu in a turned window finding and
  lighting the row under the pointer.
  *Not done, and named here:* the jump list is as the shell makes it and
  has not been seen on Windows, which the build image is not. Save As on
  the pane opens the copy and then asks, so a Save As answered with
  Cancel leaves the copy open as the document being edited. Show
  Repairs can only say what this program's own reading of the copy says;
  Word's repairs of a damaged file are Word's.
- [x] **H16. A save that cannot destroy the file it replaces.** The document
  is written straight over its own path with `std::fs::write`, which
  truncates first; a disk that fills or a process that dies during the
  write leaves an empty or partial file where the document was, and the
  message says only that saving failed. Write to a temporary file beside
  the target, finish and sync it, then rename it into place; clear the
  modified flag only after the rename. The same for every format that goes
  out through this path. Reviews #1, R16.
  *Done:* every file the program writes for a person over one that may
  already be there is written beside it and renamed into its place.
  **The helper** (`crates/wp-app/src/editor/replacing.rs`):
  `write_replacing(target, write)` makes a new file in the target's own
  folder, `.~Letter.docx.4120-7.tmp` — the name it will become, the
  process and a count, made with `create_new`, so no two saves share one
  and an old one is never written into — hands it to `write`, gives it the
  old file's mode on Linux, syncs it, closes it and renames it over the
  target, then syncs the folder on Linux; on any error the temporary file
  is removed and the target is as it was. `replace_with(target, bytes)` is
  the same for bytes in hand. A link is followed and left a link; a file
  marked read-only is refused, as writing into it was, rather than
  replaced by one that is not. On Windows `std::fs::rename` is
  `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING`, and `FileRenameInfoEx`
  with `FILE_RENAME_FLAG_REPLACE_IF_EXISTS` where that is denied: so the
  standard library's documentation and source say.
  **Where it is used:** Save and Save As in every kind
  (`crates/wp-app/src/editor/files.rs`): Word's four, `.doc`, `.rtf`,
  `.odt`, the single-file web page, and the web page with its pictures,
  now written pictures first and page last, so that a failure leaves the
  old page and not a new one missing its pictures. And plain text
  (`textfiles.rs`); a document signed over its own file
  (`signatures.rs`); Export to PDF (`printpane.rs`); a file the document
  carries, saved out (`links.rs`); the typed address list and the letters
  of Merge to Files (`mailings.rs`); the person's own template of building
  blocks (`ownblocks.rs`); and the AutoRecover copy and its sidecar
  (`autorecover.rs`), where a crash during the write is the very thing the
  copy is kept for. Everywhere the document is marked saved only once the
  helper has returned, which is after the rename. Left as they were, being
  nobody's document: the picture `--picture` draws of the window, and what
  the tests write for themselves.
  *Proven by:* a file replaced holding exactly the new bytes with nothing
  beside it, and one made where there was none; a write failing halfway,
  its temporary file seen beside the target meanwhile, leaving the old
  bytes, no temporary file and the error; two saves of one name at once,
  each with a file of its own; a name of 250 bytes still finding room for
  its temporary file; a read-only file refused and untouched; the mode
  kept; a link followed and left a link; Save over a file already there in
  six kinds — `.docx`, `.doc`, `.rtf`, `.odt`, `.htm`, `.mht` — marked
  saved, with a second name for each old file, in another folder, still
  holding the old bytes, which only a rename leaves, and nothing else in
  the folder; and a save refused, over a folder of the document's name,
  leaving the work marked unsaved and no temporary file.
  *Not done, and named here:* a rename puts a new file in the old one's
  place, and what belonged to the old file rather than to its bytes does
  not come with it. Another name for it, a hard link, keeps the old bytes,
  as the test shows. On Linux the owner and group become the saver's and
  extended attributes are lost; only the mode is copied. On Windows the
  access list, the hidden and system attributes, the creation time and
  the alternate streams, the mark of the web among them, are the new
  file's own; `ReplaceFileW` keeps those, and would need the shell's
  Windows calls. None of the Windows side has been run, only built: the
  build image is Linux. On Windows a file another program holds open is
  refused unless that program shares its deletion, where writing into it
  needed only shared writing; on Linux the rename goes through and that
  program goes on reading the old one. A folder where a person may change
  a file but not make one now refuses the save that writing in place let
  through. A temporary file left by a crash stays beside the document
  until somebody deletes it: nothing sweeps them up, since one may be
  another copy of the program's save in progress. The message on failure
  still says only "Cannot write", which is now the whole truth, but not
  that the file on disk is unchanged.
  Written in place still, not being documents: the settings file and the
  custom dictionary, both silent on failure and the next to go this way;
  and outside the window, `wp-cli`'s converted output, a macro's `Close`
  of a file it wrote, and the desktop's `mimeapps.list` and recent list,
  which `wp-shell` rewrites.
- [ ] **H17. What the clipboard carries within the program.** Copying a
  picture and pasting it reports success and pastes nothing, because the
  run writer skips `Picture`, `Chart`, `Ink`, `Diagram`, `Group` and
  `Math`: a cut-and-paste of a picture loses it, and a dragged one too. A
  selection copied over a tracked deletion takes the wrong stretch, because
  `slice` counts deleted runs the caret does not, and a paste turns the
  deleted words into plain text. And after pasting a shape the caret stays
  before it, because the length is counted in plain text where a shape is
  a byte to the caret. Reviews #5/R14, #6/R15, #14/R18.
- [ ] **H18. UI Automation's tables, as COM lays them out.** `RootVtbl` and
  `ItemVtbl` glue Simple, Fragment and FragmentRoot into one table and
  `QueryInterface` hands out one pointer for all three, where each inherits
  `IUnknown` on its own: a client calling `Navigate` on the Fragment lands
  in `get_provider_options` with a direction for an out-pointer, and writes
  through it. One table per interface, and the right pointer from
  `QueryInterface`, on the same object. Review R03; found against the
  Windows SDK headers.

## I — The language of the interface

- [x] **I1. Message catalogues.** Every string in the interface comes from a
  catalogue rather than from the code.
  *Done:* the catalogue is keyed by the English sentence, as `gettext`'s
  are and for the same reasons — the code still says what will be on the
  screen, a message nobody has translated falls back to something true
  rather than to a number, and changing the English makes a new message
  that has to be translated again instead of quietly keeping the old
  translation of a sentence that no longer exists. A message is asked for
  by `t("Align Left")`, which gives back a `&'static str`: the translation
  is leaked once, the first time it is wanted, so nothing that draws the
  interface had to change shape and no label is allocated on a repaint.
  What a dialog says is looked up where the dialog is drawn rather than at
  the several hundred places where dialogs are built — the labels,
  captions, headings and the words a list is made of go through the
  catalogue; the values do not, because a name typed into a box, a
  measurement or a font is the person's and has no translation. The same
  goes for the menus, the panes, the File page and the strip along the
  bottom. A sentence with something in it — "Page 3 of 8", "Saved {0}" —
  is one message with numbered places in it, so a language that puts the
  parts the other way round can.
  The catalogues are files: those that come with the program are built
  into it, and a person may put one of their own in the settings folder,
  which is read first — somebody whose language nobody has translated into
  can translate it themselves without a compiler. The language is chosen
  on Word's own Language page in Options and takes effect at once, without
  the restart Word asks for. `messages/en.txt` is the list of every message
  the program can say, written by a test that reads the source for `t(`
  and walks the tables the ribbon, the tabs and the panes are made of; the
  test beside it holds the list to exactly what the program says, so a
  message added and not listed fails, and so does a message listed after
  the code stopped saying it. There are 543 of them, and `messages/de.txt`
  translates all 543 into German, in Word's own German words.
  One catalogue is not a language: the pseudo-language writes every
  message in accented letters between brackets, which is how a picture of
  the window shows what did *not* come through the catalogue — anything
  still in plain English was written into the code where it is drawn.
  Held to: the catalogue reads back what it wrote, a message with no
  translation comes back in English, the pseudo-language marks everything
  it is given and leaves what is filled in alone, the message list matches
  the program, and three pictures — the window in German, the window in
  the pseudo-language, and Options in German — looked at. The strip of
  tabs in a dialog gives up the room between its tabs, and then their
  width, rather than running off the edge of the dialog in a language
  whose words are longer; the pane's tabs are clipped to their own quarter
  for the same reason.
  *Not done, and named here:* the document's own words are not the
  program's and are not translated — a built-in style name is shown as the
  document holds it, where Word shows "Überschrift 1" for the "Heading 1"
  in the file; the name of the document's language in the strip along the
  bottom is in English; and the fonts keep their own names, which is
  right. A consequence of keying by the English: a style or a heading
  whose name happens to be a message — a style actually called "Normal" —
  is shown translated in the places where names pass through the
  catalogue, which is what Word does with the built-in ones and not what
  it does with a style somebody named themselves. The letters Alt puts
  over the ribbon are English, where Word's are the language's. What the
  system's own dialogs say — the buttons of a message box, the question
  about saving changes, which the shell builds — is the system's language
  on Windows and English elsewhere. Numbers, dates and measurements are
  **I3**, and a window that reverses for Arabic and Hebrew is **I2**.
  German is the one language that comes with the program; the folder is
  how another arrives.
- [x] **I2. A mirrored interface.** For Arabic and Hebrew the whole window
  reverses: the ribbon, the panes, the scrollbar, the dialogs.
  *Done:* a catalogue says which way its language reads — `direction = rtl`
  at the top of the file, because that is a fact about the language and
  not about the person — and a window in such a language is drawn the
  other way round: the tabs from the right, the groups of the ribbon from
  the right and their buttons within them, the navigation pane on the
  right and the styles pane on the left, the scroll bar on the left, the
  rulers counting the other way, the strip along the bottom reversed, the
  dialogs with their labels on the right and their buttons on the left,
  and every menu dropped from the side it belongs to.
  **How.** The canvas turns what it is given, about the window's own
  width: what was at `x` is drawn at `width - x - its own width`. So none
  of the several hundred places that draw a button had to learn which way
  the window reads, and the one that forgot cannot be the one nobody
  noticed. Where a thing goes is turned; what it is made of is not — a
  picture on a button is the same picture, and a line of text moves as one
  piece rather than having its letters reversed, which the renderer does
  by moving the line's box and drawing the letters the way round they were
  laid out. A shape given as a path is the exception and is reflected: an
  arrow that points the way a person reads has to point the other way in a
  window read the other way.
  **What is not turned.** The page. An English document does not read
  backwards in an Arabic window, so what is on the paper is drawn exactly
  as it was laid out, at the place the turned window puts the paper. A
  document that reads right to left is laid out that way by the text
  engine itself, through `wp-bidi`: that is the document's own direction
  and has nothing to do with the interface's.
  **Being found again.** A button drawn on the right has to answer the
  pointer on the right. The furniture asks one place which way the window
  is — the same answer the canvas was given, set once where the window is
  painted — and turns the point before looking for what is under it; the
  page does not, because it was not turned.
  Held to: the Bold button is found where it is drawn in both windows and
  not where it used to be; the navigation pane answers on the left in one
  and on the right in the other; a click on the page lands on the same
  letter in both, and a press puts the caret where the pointer was. And
  two pictures — the window and the Options dialog, in the mirrored
  pseudo-language — looked at.
  *Not done, and named here:* no Arabic or Hebrew catalogue comes with the
  program, so the only language that turns the window is the mirrored
  pseudo-language, which is what the tests and the pictures use; a real
  one is a translation, which is the same work as **I1**'s German and not
  a different mechanism. The scroll bar's arrows and the resize cursors
  are turned with everything else because they are drawn as paths; the
  icons are not, which is right for a picture of a clipboard and wrong for
  the handful whose meaning is a direction — the indent buttons, the
  bullet arrows — and those are drawn from the icon catalogue rather than
  as paths, so they keep pointing the way they did. Word turns them.
- [x] **I3. The user's locale.** Dates, numbers, paper sizes and measurement
  units — inches or centimetres — as the system says.
  *Done:* the machine is asked, because a person who has told their
  computer they are in Germany has already said that a length is in
  centimetres, that a comma stands before a fraction, that paper is A4 and
  that a date is written with the day first. On Windows that is
  `GetUserDefaultLocaleName` and `GetLocaleInfoEx` — the measurement, the
  two marks a number is written with, the short date, the months and the
  days, the clock and the paper. On Linux it is the C library, which is
  where the answer lives: `setlocale` to take the machine's own settings,
  `localeconv` for the marks, and `nl_langinfo` for the measurement, the
  short date's order, the month and day names and the clock — with the
  paper read from `/etc/papersize`, since the library hands that back in a
  form that has changed between versions.
  What follows from it: lengths are shown and typed in the machine's own
  unit until Options says otherwise, and the rulers count in it — eighths
  of an inch or half centimetres, numbered every inch or every centimetre;
  a number carries the machine's own decimal mark, so a margin is `1.00"`
  in one place and `2,54 cm` in another; a count in the strip along the
  bottom has its thousands marked off; a new document is made on the paper
  the machine prints on; the date Insert puts in is in the machine's month
  names and order; and the Document Recovery pane shows when a copy was
  taken in the machine's date and clock, while the copy itself keeps the
  one form everything agrees on.
  This is not the language of the interface, which is **I1**'s and is
  chosen in Options: a person may well want a program in English on a
  machine set to German, as Word lets them, and then the words are English
  and the numbers are still German.
  Held to: the shell's own tests ask the C library as a German machine and
  as an American one — the image carries both locales for the purpose —
  and hold it to centimetres, a comma, the day first, "Januar", a
  twenty-four hour clock and A4 against inches, a point, the month first,
  "January", a twelve-hour clock and Letter; the pictures of Page Setup on
  each of those two machines, looked at.
  *Not done, and named here:* the first day of the week, which nothing in
  this program yet asks about — there is no calendar in it; the names of
  the months in a document's own field codes, which Word writes by the
  document's language rather than the machine's; and Word's Date and Time
  dialog, which offers a dozen forms of today and is not written — Insert
  puts in the long form and nothing offers the others. Numbers inside the
  document — a table's sums, a numbered list — are the document's and are
  written as the document says, not as the machine does.
- [x] **I4. The document's own words in the person's language.** Word shows
  the built-in style called "Heading 1" as "Überschrift 1" to a German, and
  names the document's language in the strip along the bottom in the
  program's language. Both are shown here as the document holds them, which
  is right for a style somebody named themselves and wrong for the built-in
  ones. Done when the built-in names are recognised and translated and a
  style of one's own is left alone. Named in **I1**.
  *Done:* both (`crates/wp-app/src/names.rs`).
  **The styles.** Word writes its own styles' names into every file in
  English, whatever it runs in, and shows them in its own language. Sixty-six
  of them are known here by the name the file gives them, however it is
  capitalised, with the name Word shows — "annotation text" is shown as
  Comment Text — and that name is a message, translated like any other:
  Standard, Überschrift 1, Titel, Zitat, Kein Leerraum, Listenabsatz and the
  rest to a German. A style of one's own is shown as it was named, where
  before every name was made title case. Wherever a style is named: the
  gallery, the style box, the Styles pane, the restrict-formatting list,
  Manage Styles and the Style Inspector. The style box, on the way, now
  says the style's name at all: it said the identifier the paragraph
  refers to it by, Heading1 for Heading 1.
  **The languages.** A document's language is named in the status strip
  and the Language list in the interface's language, a half at a time —
  the language, and where the name says one the country or the way of
  writing it — so "English" is translated once and not once for every
  country. The Language list is in the order its names take in that
  language, accents aside, as a dictionary orders words. Its last line, on
  the way, said "{2713} Do not check spelling or grammar" — the number of
  the tick written out rather than the tick — and now shows the tick, in
  the language.
  The tests' language is now each test's own: a test reading the
  interface in German had been the language of every other test running
  beside it, and one looking for an English menu item did not find it.
  *Proven by:* the built-in names known however written and a style of
  one's own left alone; in German, Überschrift 1, Standard, Kommentartext,
  "Englisch (Vereinigtes Königreich)", "Deutsch (Österreich)", and the
  Language list in German order; the style box saying Heading 1 and a
  style of one's own as it was named, in the gallery too; the German
  window, looked at. And the German names of every language and every
  country in the list held to Debian's German for the ISO lists
  (iso-codes, now in the build image as test data): all the countries
  agree, and the languages but five, where Word's German is kept and the
  test says so — Belarussisch, Galicisch, Mazedonisch, Nepalesisch and
  Punjabi where ISO's translators write Weißrussisch, Galizisch,
  Makedonisch, Nepali and Panjabi.
  *Not done, and named here:* Word's German for the rarer styles — Word
  has some hundreds of built-in styles, and the sixty-six here are the ones
  a document commonly has; the rest are shown as the file names them. A
  style name typed in German is not taken back to Word's English name,
  there being nowhere here a style is chosen by typing its name. The
  German names were checked against another's translation, not against a
  German Word.
- [x] **I5. The letters Alt puts over the ribbon, in the language.** Word's
  key tips are the language's; these are English whatever is loaded, so a
  German pressing Alt sees letters that do not match the words under them.
  Named in **I1**.
  *Done:* in any language but English, the letter over a tab and over a
  command is worked out from the words under it as that language has them
  (`crates/wp-app/src/editor/keytips.rs`) — the first letter of a word
  nothing else has, then a later one, then a digit — so a German sees D
  over Datei and I over Einfügen, not F and N. The tabs are lettered over
  every tab in one fixed order, so a tab's letter does not move when the
  table tabs come and go. English keeps Word's own letters over the tabs.
  A command's letter comes from the words on its button where it has any,
  which it had been taking from the button's tip, so that the letter is
  in the word the eye is on; and a squeezed group's button, and a style's
  tile, are lettered from the group's name and the style's, where they had
  been given digits for want of a word. A screen reader is told the same
  letters. On the way, the navigation pane's own sentences — no headings,
  no pages, nothing found, Page 3 — were drawn through the catalogue but
  never listed in it, so a German read them in English; they are now.
  *Proven by:* in English, H and N over Home and Insert; in German, every
  tab's letter and every command's on the Insert tab in the German word
  under it and no two alike, Einfügen's not N, and that letter opening it;
  the German ribbon with its letters, looked at.
  *Not done, and named here:* Word's own German letters. Microsoft's German
  help gives the English letters for the tabs — Start on H — where a
  German site gives others — Start on R — and neither lists them all, so
  there was nothing sure to copy; these are worked out, as the commands'
  always were. A letter only the language's keyboard has — Ü — is not
  offered, since the shells report a letter key by the Latin letter on it.
  The navigation pane's four tabs are too narrow for their German names.
- [x] **I6. A window that really reads right to left.** **I2** turns the
  window and proves it with a mirrored pseudo-language; what is missing is a
  real Arabic or Hebrew catalogue, which is a translation rather than a
  mechanism, and the handful of icons whose meaning is a direction — the
  indent buttons, the bullet arrows — which keep pointing the way they did
  because they are drawn from the icon catalogue rather than as paths. Word
  turns them. Named in **I2**.
  *Done:* a Hebrew catalogue (`crates/wp-app/messages/he.txt`), every one
  of the 1349 messages German has, offered among the interface languages
  as עברית and turning the window, as its first lines say it does: Word's
  own Hebrew words where they were known — בית, הוספה, עיצוב, פריסה,
  הפניות, דברי דואר, סקירה, תצוגה for the tabs — and plain Hebrew where
  not.
  **The icons.** **I2** had it the wrong way round: an icon is drawn as
  paths, and the canvas reflects every path about the window, so it was
  every icon that was turned — the clipboard, the brush, the magnifying
  glass back to front — and not the arrows alone. Now a picture of a thing
  is reflected about its own middle first, so that the two cancel and it
  comes out as drawn in its turned place, and only the icons whose meaning
  is the way the text is read are left to the canvas and turned, as Word
  turns them: the three lists, the two indents, undo and redo, the line
  spacing, previous and next. Aligning left and right are not among them:
  left is left in any language.
  **The lines of text.** A line of the interface — a button's name, the
  strip's page count, a message — had been turned round whole whenever it
  began in a script read from the right. That is right for a line of
  nothing but Hebrew and wrong for the rest: "עמוד 12 מתוך 40" said 21
  and 04, a word of English in a message was spelt backwards, and a bracket
  faced out, so the strip read ")בריטניה(" for the document's language.
  Such a line now goes through the whole bidirectional algorithm
  (`wp-bidi`, the one the page uses), with a bracket in a stretch read from
  the right drawn as the other end of its pair; and in a window read right
  to left it reads right to left whatever it starts with, as Windows reads
  a window with that reading order — "{0} נשמר" with an English name for
  the {0} is the name and then the word, read from the right, where a line
  going by its first letter would put the word first. A document's own
  words drawn alone, as a Font dialog's sample is, go the way their own
  first letter says.
  **The letters Alt puts over the ribbon** (**I5**) are worked out from
  the words under them, and a key is named by the Latin letter on it: a
  Hebrew word gives none, so a Hebrew window had a digit over nine things
  and nothing over the rest. Where the words shown have no Latin letter
  a command's letter is taken from its English words, and the tabs have
  Word's own English letters, which are the ones somebody who knows Word
  already has — H over בית, N over הוספה. And on the way, in English too:
  the Table Layout tab had Word's L, which is the Developer tab's, and the
  two are there together while the caret is in a table; it has A now.
  On the way: a password box showed "{2022}" for each letter typed, the
  number of the dot written out rather than the dot (since **J1**), and
  now shows the dot; and **I4**'s entry is ticked, which its commit did not.
  *Proven by:* in Hebrew, the window is turned, בית is Home and מודגש Bold,
  every tab has Word's letter and N opens Insert, every command's letter
  is in its English words, and more than nine things on it have a letter;
  no two tabs with the same letter, the table tabs included; a picture
  comes out as drawn in a turned window and undo turned; the page count
  keeps its digits in order, the brackets close round the country, a
  message of a window read from the right reads from the right and one of
  a window read from the left from the left, and English stays English;
  a password shown as a dot a letter. And the Hebrew window with its
  letters and its strip along the bottom, looked at.
  *Not done, and named here:* the Hebrew is this translator's and not
  Microsoft's: Word's Hebrew words were used where they were known, and
  nothing here was checked against a Hebrew Word. No Arabic catalogue: the
  mechanism is the same, the translation is not done. The letters over the
  ribbon in a Hebrew window are English letters, as a key is named by
  them; what a Hebrew Word puts there was not checked. In a box to type
  into, the caret is put as far from the start as the text before it is
  wide, measured from the side the window reads from, which is right for
  Hebrew typed there and wrong for English typed into a Hebrew window.

## J — Protection, collaboration and the rest of Word's features

- [x] **J1. Document protection.** Read-only, filling in forms only, tracked
  changes forced on, and the password behind them.
  *Done:* all four of Word's restrictions, each one true of the program and
  not only written in the file. **No changes** refuses every command that
  would alter the document and greys the buttons that would have run them.
  **Comments** refuses the same and lets a comment be made and taken away,
  which is the whole of what the restriction is for. **Tracked changes**
  switches the recording on, keeps it on — `set_tracking_changes(false)` is
  refused while it stands, which is what Word's Lock Tracking is — and lets
  everything else through, because everything else will be recorded.
  **Filling in forms** reads the form fields the document has, in
  `wp-docx/src/forms.rs`, and allows typing inside the answer of a text
  field and nowhere else, with a section let out of it where `w:formProt`
  says so.
  One rule, asked twice: `Command::is_allowed_under` is read by the ribbon
  to decide whether to grey a button and by the editor to decide whether to
  run one, so a control that looks pressable and a control that works are
  the same control. It is written as an allow-list, because a list of what
  is forbidden goes out of date the next time a button is added and does it
  silently.
  The password: `wp-hash` is a new crate carrying SHA-1 and SHA-512 written
  from the standard, and `wp_text::base64` is the one base64 in the
  workspace — the picture in a mail message and the hash of a password in
  an attribute are the same problem, and `wp-html` now asks it rather than
  carrying a second copy. The salt comes from the machine, through a new
  `wp_shell::random`: `/dev/urandom` on Unix and `RtlGenRandom` on Windows,
  because no arithmetic makes a byte nobody can guess. The hash is the
  format's own — salt and the password as UTF-16LE, then a hundred thousand
  turns of hashing the last answer with the turn number after it — written
  in the attributes Word writes now (`w:algorithmName`, `w:hashValue`,
  `w:saltValue`, `w:spinCount`) and read from those or from the ones Word
  2007 wrote (`w:cryptAlgorithmSid`, `w:hash`, `w:salt`,
  `w:cryptSpinCount`). A document naming a hash this program has not got
  keeps its restriction and refuses every answer, and says which hash it
  wanted rather than saying the password was wrong.
  Said plainly where it matters: the dialog carries Word's own sentence —
  the document is not encrypted, and anybody who can open the file can take
  the restriction off. The password stops a person, not a program. Making
  the bytes themselves unreadable is **J2**.
  *Proven by:* `wp-hash` against the standard's own vectors, the
  one-million-`a` case included; a password written, saved, reopened and
  found to open only for the word that made it, at the format's full
  hundred thousand turns; the password itself searched for in the saved
  bytes and not found; the Word 2007 attributes built in a test out of
  arithmetic done outside the code being tested, and read back and checked;
  a document protected here converted by LibreOffice, whose own writer puts
  the element back — and the password still opens it, which it could not do
  if LibreOffice had not read the attributes as a password; the form field
  a test builds the way Word writes one, found with its answer in the right
  place and a check box and a drop-down told apart from it; and the
  Restrict Editing dialog and a protected window photographed and looked
  at.
  *Not done, and named here:* **making** a form field — this program reads
  and fills in the ones a document has, and has no Developer tab, no
  command that puts a text box, a tick box or a drop-down into a document,
  and no way to tick a box or pick from a drop-down once one is there. That
  is **J8**. Limiting formatting to a selection of styles — `w:formatting`
  is read and written back so that a document does not lose it here, and
  nothing enforces it and no control offers it, which is **J9**. Word's
  other two passwords, the ones under Save As ▸ Tools ▸ General Options:
  **J10**. And the exceptions a restriction may carry — the stretches
  marked `w:permStart w:ed="everyone"` that stay editable inside a
  protected document — are read as blocked stretches (**F**'s Block
  Authors) and are not yet let through the restriction: **J11**.
- [x] **J2. Encryption.** Opening and writing the encrypted `.docx` Word makes,
  which is an OLE compound file with the package encrypted inside it.
  *Done:* an encrypted document opens, and a document can be encrypted. Not
  the same password as **J1**'s and worth keeping apart: that one stops a
  person lifting a restriction while the text sits in the file for anybody to
  unzip; this one means there is no text in the file at all, only a compound
  file holding one enciphered stream, and no program can read it without the
  word — this one included.
  Four crates, each with one job. **wp-cipher** is AES, written from the
  standard: the key schedule for all three key lengths, the block, and the two
  ways of using it the Office formats ask for — chained, and, for the older
  scheme, not chained at all. **wp-ole** is the compound file, which was a
  corner of the crate that reads `.doc` files and is now a crate of its own,
  because an encrypted document written yesterday is one of these too; it
  reads as it did and now writes, including the part of that format a writer
  is likeliest to leave out, the directory the FAT needs once it outgrows the
  hundred and nine sectors the header can name. **wp-hash** gained HMAC.
  **wp-crypt** is the scheme itself.
  Both schemes are read. **Agile**, which Office has written since 2010: the
  description is XML, the cipher is AES chained, the hash SHA-512, the
  password hashed a hundred thousand times over, the package cut into pieces
  of four thousand and ninety-six bytes each chained from a vector of its own,
  and an authenticated hash over the whole of it so that a byte changed by
  somebody who could not read the document is noticed rather than deciphered
  into nonsense. **Standard**, which Office 2007 wrote: a fixed run of bytes,
  SHA-1, fifty thousand turns, and the package enciphered block by block with
  no chaining. Only the first is written, because writing the second would be
  making a new document weaker on purpose.
  In the program: a file that turns out to be encrypted stops and asks for its
  password instead of failing as a broken zip, and asks again on a wrong one;
  a document opened that way remembers, so that saving it leaves it encrypted;
  and File ▸ Info carries Word's Encrypt with Password, where an empty box
  takes the encryption off. The salts come from the machine, fresh on every
  save, so two saves of the same document under the same password are not the
  same bytes. A machine that will not give them refuses to save rather than
  writing the document in the clear.
  *Proven by:* AES against FIPS 197's own three worked examples, one for each
  key length, and against SP 800-38A's chained ones, which the single-block
  examples say nothing about; HMAC against RFC 4231, the longer-than-a-block
  key included. The compound file written and read back at every length round
  the two boundaries that matter, with a storage inside it, with two hundred
  streams found by walking the directory tree the way a reader walks it rather
  than by looking through the entries, and with an eight-million-byte stream,
  which is past what the header alone can describe. A document laid out by
  hand from the specification and read — for the 2007 scheme and for the
  agile one, with the key's arithmetic done in the test rather than by the
  code under test, and in the shapes this program reads but never writes:
  SHA-1, a hundred-and-twenty-eight and a hundred-and-ninety-two bit keys, a
  spin count that is nobody's default, and a document with no integrity check
  at all. Then the round trips: a package of every awkward length, the text
  and the password both searched for in the sealed bytes and not found, a byte
  changed in the middle and caught, and a wrong password refused. The Encrypt
  Document dialog photographed and looked at.
  *Not done, and named here:* **no second opinion on the container.** Every
  piece of arithmetic here is held to somebody else's numbers, but whether
  Word itself opens a file this program encrypted has not been tried, because
  there is no Word here and LibreOffice 7.4 — what the build image carries —
  takes no password on its command line or through `--infilter`, so it cannot
  be handed one either. The test that would do it is written and skipped, and
  this is what **K1** is for. What is most likely to be wrong if anything is:
  the `\006DataSpaces` storage, which Office writes beside the two streams and
  this does not, on the reading that it belongs to the rights-management path
  rather than to a password. The older RC4 schemes Office used before 2007 are
  not read and are named rather than attempted. Nothing here is constant-time
  against an attacker who can watch the cache; what is defended is a file
  against somebody who has it and not the password.
- [x] **J3. Digital signatures.** The signature a `.docx` carries: read,
  checked, and written.
  *Done:* what signing a document means, and all of the arithmetic under it.
  A `.docx` is a zip, and a signature over it is not a signature over the
  zip — the zip's own bytes change every time a program writes one. What is
  signed is a list of every part with the hash of its bytes and the content
  type it was written under, and the signature is over that list. Change a
  part and its hash changes; take one away and the list names a part that is
  gone; add one and it is not in the list. So a program can say not merely
  that a document changed but **which part** of it did, which is what this
  says.
  Four crates, and each is arithmetic with a test to hold it to. **wp-rsa**
  is numbers too big for a machine word — long multiplication, long division
  and raising to a power modulo another number — and the signature scheme
  built on them: PKCS#1 version one point five, which is what every signed
  `.docx` uses. **wp-asn1** is DER, and the certificates and keys written in
  it: enough of a certificate to say who signed and to get their key, and
  enough of a key file to sign with. **wp-sign** is the two halves of an XML
  signature: canonical XML, which is the one way of writing a piece of XML so
  that it can be hashed at all, and the package signature itself. **wp-hash**
  gained SHA-256, which is what a signature is made over now.
  The relationship parts get the transform the format lays down, because they
  have to: a `.rels` part holds identifiers a program is free to renumber and
  an order that is nobody's business, so what is signed is what the
  relationships *say* — the ones the signature names, each with a
  `TargetMode` whether it had one or not, sorted by identifier — and not how
  they were written down.
  In the program: File ▸ Info says whether the document is signed and by
  whom, and pressing the line says what the signature says and whether it
  holds. Saving a document that has been edited takes its signatures off,
  which is what Word does and the only honest thing to do: a signature says
  the document is what it was, and after an edit that is not true. Signing
  itself is on the command line — `wp sign <in> <out> <cert.der> <key.der>` —
  because a certificate has to come from somewhere, and where it comes from
  is **J12**.
  *Proven by:* OpenSSL, xmllint and xmlsec1, which are now in the build image
  and had no part in writing any of this. Every hash against OpenSSL at every
  awkward length, the block boundary and the padding-needs-its-own-block case
  included, and HMAC too. RSA both ways round: what this program signs,
  OpenSSL accepts, and what OpenSSL signs, this program accepts, for SHA-1,
  SHA-256 and SHA-512 — and both refuse a changed message, a changed
  signature, the wrong hash and another key's signature. A certificate
  OpenSSL wrote, read back as the name, the dates, the serial and the key
  that were asked for. Canonical XML against xmllint on every shape that has
  ever been got wrong, the namespace context of a piece cut out of a document
  included. And the whole signature: **xmlsec1 accepts one this program
  wrote, and refuses it once a byte of it is changed**. Then the document
  itself, end to end on the command line: signed, read back as valid, a part
  changed behind its back and the change named, and the signature gone after
  an edit. The File page photographed and looked at.
  *Not done, and named here:* **whether to trust the certificate**, which is
  the question this does not answer. It says the signature is the signature
  of the certificate in the file; it does not say the certificate was issued
  by anybody, that its chain reaches a root the machine trusts, or that it
  has not been revoked. That is a question for the operating system's own
  store of who is trusted, and answering it out of a list of our own would be
  inventing a trust nobody granted. **J12** is that, and the rest of what
  comes with it: signing from the ribbon, the Signatures pane, the digital
  half of the Signature Line, XAdES, and countersignatures. Also not here:
  exclusive canonicalisation — nothing this reads or writes names it, and a
  canonicalisation nothing exercises is one that is quietly wrong — and
  signing with anything but RSA.
- [x] **J4. Compare and merge, finished.** Word's three-way merge and its
  compare view.
  *Done:* the two things Word's Compare button drops open onto, and the view
  that makes either of them readable.
  **Combine** is the three-way merge, and the third way is the point: given
  only two revisions there is no telling whether a sentence is in one and not
  the other because somebody added it or because somebody took it away, and
  the original settles it. What comes out holds both sets of changes, each
  marked with the name of whoever made it — read off each document's own
  record of who wrote it last, so that two people's work is two people's
  work and not one anonymous heap. Where the two of them did the same thing
  it is marked once; where they did different things in the same place both
  go in and the place is counted a **conflict**, because two people wrote two
  different things and only a person can say which. A merge that quietly
  chose one would be a merge that lost work without saying so.
  **The compare view** is the column Word puts beside the result: the
  document as it was above, the one it was compared with below, each laid out
  as a real page rather than described in words. It opens by itself after a
  comparison — which is when a person most needs it, because a document full
  of tracked changes is hard to read and the question they actually have is
  what it said before — and the page area makes room for it rather than being
  covered up.
  Under both: `Document::changes()`, which is every tracked change in the
  body with who made it, when, of what kind and over which words. Word's
  Reviewing Pane is a list of exactly that, and until now this program could
  count its changes and not say whose they were.
  *Proven by:* two people changing different paragraphs and both changes
  coming through, marked as theirs; two people writing the same thing marked
  once and by the first of them; two people writing different things in one
  place kept both and counted as a disagreement; a paragraph each of them
  added coming through as two additions; one taking a paragraph out while the
  other left it alone, and while the other rewrote it, which is the case that
  is a conflict and the case that is not; a combining surviving a save and an
  opening; and a combining undone in one press, because one thing done is one
  thing to undo. The column's width coming off the page area and going back.
  The menu photographed under its button, and the view photographed with the
  result in the middle and the two it came from beside it.
  *Not done, and named here:* comparing **formatting** — a paragraph whose
  words neither author touched but whose look one of them changed is not
  seen, and Word's compare options list a dozen such things (moves as moves
  rather than as a deletion and an insertion, case, white space, tables,
  headers and footers, notes, text boxes, fields). That is **J13**. Word's
  Compare dialog, with its two drop-downs and its options, is not written:
  the two commands ask for their documents through the system's own file
  dialog instead. And the two documents in the column do not scroll with the
  result — they show the page the caret is on, which is right when the
  comparison has not moved the pages about and approximate when it has.
- [x] **J5. Mail merge, finished.** The data sources, the field mapping, the
  preview, and the merge to a document, to a printer or to mail.
  *Done:* the end of a merge, which is what was missing. Finish & Merge was
  one command that wrote a file for each person; it is now Word's menu, with
  Word's three ways out of a merge and one more.
  **Edit Individual Documents** puts every letter into one new document, each
  starting on a page of its own, which is what a person most often wants:
  something to read through before a hundred sheets come out of the printer.
  **Print Documents** does the same and then opens the Print page on the
  result — Word prints them straight off; this puts them in front of the
  person first, with the paper still in the tray, because a merge that goes
  wrong goes wrong a hundred times. **Send E-mail Messages** hands each
  letter to whatever the machine uses for mail, through a new
  `wp_shell::mail`: the address comes from the column the list calls an
  e-mail address, and nothing is sent — what comes up is a message waiting
  for the person to look at and send, which is the only honest thing for a
  word processor to do with somebody else's address book. **One File for Each
  Letter** is what the command used to do, kept because Word gets there only
  by merging to a document and saving it a page at a time.
  And **who the merge is for**: Edit Recipient List was a list that dropped
  open and picked whom to preview; it is now Word's Mail Merge Recipients
  dialog, with a tick against every row. The tick is what it looks like — the
  merge, and every one of the four endings, leaves out whoever is unticked.
  Because the letters go where the letter is, this program having one window,
  merging into the window asks about unsaved work first, the same question
  New and Open ask.
  *Proven by:* three recipients merged into one document with all three names
  in it and two page breaks between them, not three and not one; a recipient
  unticked getting no letter while the two beside them do; the dialog
  starting with everybody ticked and one tick coming off; a merge with nobody
  on the list saying so rather than opening an empty menu, and a merge with
  everybody left out saying so rather than throwing the letter away. The
  address escaping that a `mailto:` needs, held to the standard's unreserved
  set, a space, a newline and a letter outside the alphabet. The Finish menu
  photographed under its button and the recipients dialog photographed with
  its ticks.
  *Not done, and named here:* the rest of Word's Mail Merge Recipients
  dialog — sorting, filtering, finding duplicates, finding a recipient, and
  validating addresses. The letters go to mail as their words: `mailto:` is
  how every desktop agrees to open a mail program and it carries text, so the
  formatting does not go and neither does an attachment. Word's three other
  data sources — Excel, Access, Outlook — are still not read, for the reason
  written in [`wp_docx::merge`]: they are two file formats and a mail system,
  and every one of them can write the delimited file this does read. Word's
  Type a New List, which makes a list of recipients without a file to start
  from, is not written.
- [x] **J6. Building blocks, Quick Parts and templates.** Including the
  `Normal.dotm` a person's own defaults live in.
  *Done:* a piece of a document saved under a name and put back in the next
  one, which is what a building block is and what makes it worth having. The
  format is the glossary document — a second document inside the package,
  holding `w:docPart` entries with a name, a gallery and a category — and it
  is read and written in `wp-docx/src/blocks.rs`.
  Where they live matters as much as how they are written, and it is the half
  that was missing: a block kept in the document it was saved from would be
  no use whatever. So they go into a template, and the template a person's own
  things live in is the one Word calls `Normal.dotm`, in the folder Word keeps
  personal templates in. A new blank document is made from it where there is
  one, which is what "a person's own defaults" means and what Word does.
  In the program, Word's Quick Parts menu: the pieces a person has saved
  first, then Save Selection to Quick Part Gallery and the Building Blocks
  Organizer, then the document properties that can be put in as fields, which
  is all that menu used to hold. Saving asks what to call it; the organiser
  lists what is there and puts one in or takes one away.
  *Proven by:* a block saved, the template written and opened again, and the
  block still there with its words, its gallery and its category; one put
  into another document where the caret is; one saved twice under a name
  ending as one block with the later content, because two blocks of one name
  is a menu where one can never be picked; one with no name refused; one
  taken away and not taken away twice; two galleries kept apart; a block
  holding a table coming back as a table; and the part written where the
  format says with the content type and the relationship it must have. The
  menu photographed under its button.
  Along the way the catalogue of what this program can say learnt that a file
  name is not a message: `Normal.dotm` is what the file on disk is called,
  and a translated one would point at nothing.
  *Not done, and named here:* **Set as Default does not write into the
  template.** The Font dialog's and the Design tab's buttons set a default for
  the document and for the next new one; Word writes them into `Normal.dotm`,
  so that they are a person's defaults in every program window and after a
  restart. That is **J14**. Word's AutoText gallery, which is the same
  machinery under a second name, is read and written by `wp-docx` and has no
  menu of its own. The galleries Word fills for itself — its cover pages, its
  page numbers, its watermarks — are deliberately not offered: they are
  Word's own content, shipped inside Word, and a gallery called "Cover Pages"
  with something else in it would be lying about what a person was picking.
  Nor is the rest of Word's organiser: the columns it sorts by, and editing
  a block's name, gallery, category or description after it is saved.
- [x] **J7. Somebody else's macros, kept and told about.** What a `.docm`
  carries, carried through this program without being lost and without being
  run behind anybody's back.
  *Done:* **a document's Visual Basic survives.** A `.docm` keeps its project
  in `word/vbaProject.bin`, and a word processor that quietly dropped it
  would be handing back a document somebody has to write again. It survives
  being opened, edited and saved, byte for byte, and goes only when the file
  is saved as a kind that cannot hold it — which Word refuses to do without
  asking, and so does this.
  **And the program says so.** A document carrying Visual Basic says so on
  the macro list, where a person looking for macros looks. A list that showed
  only what this program can record would read as a document with no macros
  in it, and that is the one thing it must not say.
  There is also a recorder, in `wp-app/src/editor/macros.rs`: the buttons
  pressed and the words typed, in order, played back. That is Word's Record
  Macro, minus the part where the recording is written out as Visual Basic.
  *Proven by:* a document carrying a Visual Basic project opened, edited and
  saved with the project's bytes unchanged; the same document saved as a
  macro-free kind, and the part, its content type and the relationship
  pointing at it all gone together; the macro list saying a document carries
  Visual Basic when it does and not saying it when it does not; and that line
  pressed, saying what happens to them.
  *Not done, and named here:* **nothing runs a macro yet.** This entry used
  to say that nothing ever would, and that was a decision taken here rather
  than asked for; it is withdrawn. Word's own behaviour is the specification:
  a document's macros do not run until the person opening it says so, and
  then they do. The first half of that is built and the second is **L1** to
  **L7**, where the language, the object model and the trust that gates them
  are set out in the order they have to be built.
  A document's macros are not listed by name — the names are inside the
  compound file, which this program can read without yet parsing the project
  in it — and that is **L1**.
- [x] **J8. Form fields, and the Developer tab they are made from.** The
  three legacy fields — text, tick box, drop-down — put into a document,
  filled in, and behaving: a tick box that ticks, a drop-down that drops.
  And Word's content controls, `w:sdt`, which are the same idea written
  again and are what a document made this decade uses.
  *Done:* both kinds, put in from a real tab and used with the mouse. The
  older kind is a run of runs — `w:fldChar` begin carrying `w:ffData`, the
  instruction, a separator, the answer, and an end — and `wp-docx/src/forms.rs`
  now writes that as well as reading it, ticks the box by setting
  `w:checked` and writing the character the box is drawn with, and answers a
  drop-down by number, which is how the format stores the answer.
  The newer kind is `wp-docx/src/controls.rs`: `w:sdt` wrapping ordinary runs,
  with the six sorts Word's tab offers — rich text, plain text, tick box,
  combo box, drop-down list and date. The tick box is the one Word did not
  have room for in the original namespace, so it is written in the `w14` one
  and that namespace is declared in the document it is used in.
  Both wrap the words rather than replacing them, so a reader that knows
  nothing of controls still sees the text.
  In the program: Word's Developer tab, which is where these are made. Four
  groups of it — Controls with the six buttons, Legacy with the menu of three,
  Protect with Restrict Editing, Code with Macros. A click on a tick box
  ticks it, a second click unticks it, and a click on either sort of list
  drops it open under the caret and writes in what was picked. A click
  anywhere else is still the caret's.
  *Proven by:* eight tests over the older fields and eleven over the newer —
  every sort put in and read back as the sort it was, a box ticked and
  unticked through a save and an open, a list offering what it was given and
  keeping the answer, words written into a text one and the control surviving
  it, a sort refused what is not its own, two side by side told apart and the
  right one of them ticked, and the XML checked to be the shape Word writes,
  namespace and all. Seven more over the tab itself: every button puts its own
  control in, every menu item its own field, a click ticks and unticks, a
  click on a list opens it and the chosen word arrives, and a click in
  ordinary text is not taken. The tab photographed with its four groups and
  six controls standing in a document.
  *Not done, and named here:* **a content control has no boundary drawn round
  it.** Word draws a faint tag at each end so a person can see where one
  starts; here it is visible only by what it does. **There is no Properties
  dialog**, so a control is named and given its list when it is made and not
  edited afterwards — the drop-downs are made with three example entries
  rather than the empty list Word makes, because a list that drops open onto
  nothing teaches nobody what it is for. Word's picture, repeating-section and
  building-block-gallery controls are deliberately absent: the first two need
  drawing and repeating machinery of their own, and the third is **J6**'s
  galleries offered a second way. Design Mode is not there, having nothing to
  show while there are no boundaries. Mapping a control to a piece of custom
  XML — Word's other Developer group — is not done and is not planned: it is
  a way of driving a document from a program, and this program is the one
  reading the document.
- [x] **J9. Limiting formatting to a selection of styles.** The top half of
  Word's Restrict Editing: `w:formatting` on the protection, the styles
  marked `w:locked` in `styles.xml`, the dialog that lists every style with
  a tick box, and direct formatting refused while it stands.
  *Done:* the restriction, written and enforced. It is two parts of the
  package and neither means anything alone — `w:formatting="1"` on
  `w:documentProtection` is the switch, and `<w:locked/>` against a style in
  `styles.xml` is what it acts on — which is why a document with locked
  styles and nothing enforced behaves as though nothing were locked. That is
  the format's rule and it is followed here. The hundreds of styles a
  `styles.xml` mentions without defining are locked as a body with
  `w:defLockedState`, or the restriction would be one anybody could walk
  round by applying a style the document had not got round to defining.
  The two halves of Word's dialog are independent, and this program had only
  ever written the other one: a restriction now says what may be edited, or
  what may be formatted, or both. A document may be one anybody may type in
  and nobody may format.
  While it stands: the Font group, the Paragraph group, the Format Painter,
  their two dialogs, table styles and table borders are refused, and the
  ribbon greys out exactly what would be refused, because it is the same
  question asked twice. The second tick box fixes the theme as well, which is
  formatting written once for the whole document. The style gallery and the
  Styles pane drop what cannot be applied rather than offering tiles that do
  nothing, and a locked style asked for by name is refused by `wp-docx`
  itself. Clearing a style is always allowed: a paragraph styled before the
  restriction went on must have a way out, or the restriction is a trap.
  Along the way the restriction stopped being rewritten from nothing every
  time it is set. The element in the settings is edited in place now, so
  everything about it this program does not model — which provider hashed the
  password, Word's Quick Style Set lock — survives being restricted again.
  *Proven by:* fifteen tests over the document — the unticked styles locked
  and the ticked ones not, a locked style refused and an allowed one applied,
  clearing always allowed, a lock meaning nothing until something is
  enforced, the marks left where they were when the restriction is lifted
  (which is what makes putting it back a matter of one tick), a formatting
  restriction standing with no editing restriction at all, both halves behind
  one password, a restriction that restricts nothing not written, the
  `w:locked` written where the schema says and not after the formatting, a
  password taken off leaving no hash behind, and an attribute this program
  does not model surviving. Twelve more over the program: the styles leaving
  the gallery, the refusals and their two different reasons, the ribbon
  greying out what the command refuses, typing still allowed under a
  formatting restriction, a dialog that restricts nothing saying so, and
  thirty ticks kept when a password is mistyped. The dialog photographed, and
  a document under the restriction photographed beside it with its Font group
  grey and its gallery cut to the three styles that were ticked.
  *Not done, and named here:* **Word's "Allow AutoFormat to override
  formatting restrictions"** — `w:autoFormatOverride` — is neither read nor
  offered; this program's autocorrect does not apply formatting, so there is
  nothing for it to override, and the day that changes the box will be
  wanted. **"Block Quick Style Set switching"** — `w:styleLockQFSet` — is
  kept through a round trip and not offered, because there are no Quick Style
  Sets here to block; offering the box would be a control that does nothing.
  The latent styles are locked together rather than listed one at a time:
  Word's dialog lists all three hundred and something, and a list nobody can
  read is not worth the scrolling. A locked style is left out of the gallery
  rather than shown with a padlock, which is what Word does. And this is a
  dialog where Word has a pane that stays open beside the document.
- [x] **J10. The passwords under Save As ▸ Tools ▸ General Options.**
  `w:writeProtection`: a document that asks to be opened read-only, and the
  password that lets it be opened for writing.
  *Done:* the third of the three passwords a `.docx` can carry, and the one
  that had no counterpart here. They are easy to confuse and they answer
  different questions: **J2**'s makes the file unreadable, **J1**'s says what
  may be done to the text once it is open, and this one says whether the file
  opens for writing at all. `wp-docx/src/readonly.rs` reads and writes
  `w:writeProtection` — the recommendation, and the hash, in both the sets of
  attributes the format has had — and it is written first in the settings,
  which is where the schema puts it.
  In the program, the question is asked at the door: a document that asks to
  be opened read-only says so as it opens, and the answer is a property of
  the window rather than of the document. A recommendation with no password
  is a request and can be declined, which is what makes it a request; one
  with a password is opened for writing by the password and read-only by
  anything else. A document opened read-only refuses every command that would
  change it, greys out the same ones on the ribbon, says (Read-Only) in the
  caption, and sends Save to Save As rather than writing over the file that
  asked not to be written. The File page carries Word's Always Open
  Read-Only, which sets it and is also the way back out.
  Along the way one rule was made of two: what restriction stands over the
  document is asked in one place now, so that a document read-only because it
  was opened that way and one read-only because it says so grey out the same
  buttons.
  *Proven by:* nine tests over the document — the recommendation and the
  password each surviving a save and an open, both together, the right word
  taken and a wrong one not, taking it off leaving nothing in the file, a
  password taken off a recommendation leaving the recommendation, the older
  attributes of a Word 2007 document read, the element written first as the
  schema says, and the restriction alongside it untouched. Eight over the
  program: a document that asks nothing not asked about, a recommendation
  honoured and declined, cancelling at the door, the right password and a
  wrong one, the caption and the ribbon both saying so, the File page setting
  it with the two passwords having to agree and the ticks kept when they do
  not, and the line on that page saying which of the four things is true.
  Both dialogs photographed.
  *Not done, and named here:* Word's General Options dialog also holds the
  password to open, which is **J2** and is on the File page here rather than
  in this dialog; the two are one dialog in Word and two lines in this
  program's File page, which is a difference worth naming. There is no
  information bar across the top of a read-only document with an Edit Anyway
  button on it — the File page line does that job, and the bar itself is
  furniture this program has not got.
- [x] **J11. The exceptions a restriction carries.** Word's Restrict Editing
  lets stretches of a protected document stay editable, by everybody or by
  named people, written as `w:permStart` and `w:permEnd`.
  *Done:* the markers were already read and written for Block Authors; what
  this adds is the group they can name, the rule that lets them through a
  restriction, and the shading that says where.
  `w:edGrp` is the attribute Word writes for an exception — `everyone` — and
  `w:ed` is the one it writes for a person. Both are read, and both are
  written now: Everyone May Edit puts the pair of markers round the selection
  naming everybody, and Block Authors puts them round naming whoever is at
  the keyboard.
  The rule turned out to be one rule read two ways, and that is what the code
  says: **inside a pair of markers, only the people they name may edit;
  outside them, whatever the document says.** In a document nobody has
  restricted that makes the marked stretch a lock — everywhere else is open,
  here is not. In a restricted one it makes it the way in. The program does
  not have to know which sort of document it is looking at, which is worth
  more than two rules that agree most of the time.
  What restriction stands over the caret is now asked in one place, and it
  answers three things in order: a document opened read-only (**J10**) holds
  everywhere; then a marked stretch, which admits or refuses whatever else is
  true; then the document's own restriction. The ribbon and the commands both
  ask it, so a button that greys out and a button that refuses are the same
  button, and a person standing in an exception sees the ribbon light up.
  A group that is not `everyone` — `administrators`, `owners`, `editors` —
  names people out of a directory that a program on one machine cannot
  enumerate, so it admits nobody, and whoever is refused is told which group
  was wanted rather than being told the document is protected.
  And the shading. Word paints a marked stretch, and it is not decoration: a
  restricted document with an exception in it looks exactly like one without,
  and a person would have to try typing in every paragraph to find where they
  are allowed. There is a new colour in both themes for it, translucent so
  that the words underneath stay readable.
  *Proven by:* twelve tests over the document — a stretch for everybody
  written and read back with `w:edGrp="everyone"` as Word writes it, one for
  a person admitting that person and no other, a group nobody can check
  admitting nobody and saying which group it was, the stretch a position
  falls in found at both its ends and not outside them, an exception standing
  beside the restriction it is an exception to, two stretches with different
  rules told apart, one running across a paragraph break, and nothing
  selected marking nothing. Nine over the program: typing arriving inside an
  exception and refused outside it in the same read-only document, a
  selection that runs off the end of one refused, somebody else's stretch
  shut with no restriction at all and saying whose it is, a person's own
  stretch still theirs, the button taking an exception off again, and the two
  ways a stretch can already be spoken for. The shading photographed on a
  restricted document, and the Protect group of the Review tab beside it.
  *Not done, and named here:* **Word's brackets.** It draws `[` and `]` at the
  ends of a marked stretch as well as shading it, and the shading here has no
  brackets. **A colour per person.** Word gives each editor their own colour
  and lists them in its pane; every stretch here is one colour, because there
  is no pane to hold the list and one machine has one person at it. Word's
  "Find Next Region I Can Edit" and "Show All Regions I Can Edit" are that
  pane's buttons and are not here either. Naming somebody other than the
  person at the keyboard — Word's list of people to give a stretch to — needs
  somewhere to keep a list of people, which this program has not got.
- [x] **J12. Where a certificate comes from, and the rest of signing.** The
  half of **J3** that is not arithmetic. Reading the certificate store the
  machine keeps, so that a person can sign without naming a file, and so that
  a signature can be said to be trusted rather than merely to be somebody's.
  *Done:* the two lists a signature needs, and the answer they make possible.
  **The roots the machine trusts** come from the machine, in
  `wp-shell/src/certificates.rs`, because they are the machine's to decide: a
  program carrying its own list of who may be trusted would be granting a
  trust nobody asked it to grant. On Windows that is `CertOpenSystemStoreW`
  on the ROOT store, through the same run-time loading the rest of that crate
  uses. On Linux there is no one answer, so the list is a list: the bundles
  and the directories every distribution puts them in, `SSL_CERT_FILE` and
  `SSL_CERT_DIR` first, read out of PEM.
  **The chain** is `wp-sign/src/trust.rs`. A certificate is a name and a key
  until somebody vouches for it, and the chain is the following of that: each
  certificate's issuer found by name — compared as the bytes it was written
  in, because two names that print alike need not be alike — its signature
  checked with the issuer's key, and the issuer required to say it may issue.
  That last is the one that carries the weight: without it anybody with any
  certificate could issue any other. A chain ends at a certificate the
  machine trusts, and says so by name, so that a person can see whose word
  they are taking.
  **A signature carries its chain** now, both ways: every `X509Certificate`
  in one is read, not only the first, and one this program writes carries
  everything between the signer and a root — but not the roots themselves,
  which the reader has already.
  In the program: File ▸ Info ▸ Digital Signatures is a dialog rather than a
  line in the strip along the bottom, because there is something to answer.
  It says of each signature whose it is, whether it holds, and what this
  machine makes of the certificate behind it — and it offers to sign, with a
  certificate out of a folder beside the one templates live in. Signing saves
  the document signed and reopens what was written, because a signature over
  a document nobody has seen would be worth nothing.
  *Proven by:* nine tests over the chain, against certificates OpenSSL made
  and OpenSSL's own verdict on the same chain: a root, an authority under it
  and a certificate under that trusted together; the same chain with the root
  not trusted, and with its middle missing; a root trusted as itself; a
  certificate issued by one that may not issue refused — and refused by
  OpenSSL too, which is how this is known to be right and not merely strict;
  a certificate with a byte changed breaking the chain; one outside its dates
  refused; a ring of certificates not walked for ever; and what a certificate
  says about itself read back. Four over the store: PEM taken apart, base 64
  against the standard's own examples, the machine's own list read in the
  build image, and nothing but certificates coming back. Six over the
  program: a certificate and its key found whether they are written as DER or
  as PEM, one with no key beside it not offered, a folder that is not there
  being no certificates rather than a crash, the dialog's shape, and a
  document signed here read back as holding and as issued by nobody this
  machine has heard of — which is the honest answer for a certificate made on
  the spot. The dialog photographed.
  *Not done, and named here:* **signing with a key Windows holds.** Windows
  keeps a person's certificates and their keys in a store of its own and
  hands out signatures without ever handing out the key; this signs with a
  key it can read, from a folder. That is **J24**, and it cannot be written
  against a system this program is not built on. **Revocation** — whether a
  certificate has been taken back since it was issued — means asking its
  issuer over a network, and is not done and not implied: the program says
  the chain reached a root and says nothing about what has happened since.
  Name constraints, policies and the rest of RFC 5280's path rules are not
  checked; the three that carry the weight are. Word's Signatures **pane**,
  the digital half of the Signature Line, countersignatures and XAdES are
  **J25**.
- [x] **J13. Everything else two documents can differ by.** **J4** compares
  the words. Word compares more: formatting, moves, and the list its Compare
  dialog offers.
  *Done:* what counts as a difference, asked rather than decided.
  **Formatting.** A paragraph nobody retyped and somebody re-styled has
  changed, and a comparison that only read the words would say the two
  documents were identical. What is compared is what the paragraph itself
  says — its style, its alignment, its indents, its spacing — and what each of
  its runs says where the runs line up. The old properties are kept in a
  `w:pPrChange` or a `w:rPrChange`, which is what rejecting the change puts
  back, and `w:pPrChange` is written now as well as read.
  **Moves.** A paragraph carried from one place to another came out of a
  comparison as a deletion and an insertion that happened to say the same
  thing, four pages apart, for a reviewer to work out for themselves. It is
  one change now: `w:moveFrom` where it was, `w:moveTo` where it went, and
  the marks that name the two halves of one move. Accepting one keeps where
  it went and drops where it was; rejecting it does the reverse. Only whole
  paragraphs, and only where the words are untouched — a paragraph that moved
  and was edited is an edit, and calling it a move would say nothing about
  the edit.
  **Case and white space.** Two of Word's tick boxes, and the two that change
  what "the same" means: with them off, "The Cat" and "the cat" are one
  paragraph and one space is two. The comparison is done on what the options
  say to compare and written back as what was actually there, so ignoring the
  case never changes anybody's capital letters.
  **The dialog.** Word asks before it compares, and so does this now: the two
  documents by name, the four tick boxes, and where the answer goes — a new
  document, which is Word's default and here means the result loses the file
  it came from, or this one.
  *Proven by:* fifteen tests over the document — a re-styled paragraph seen
  to have changed and its record rejected and accepted; formatting left alone
  when it is not being compared; a moved paragraph written as a move, with
  its marks, and accepted and rejected both ways round; a move not asked for
  coming out as a deletion and an insertion; an edited paragraph and a blank
  line not called moves; case and spacing noticed or not as asked, with the
  words unchanged either way; the words still winning when everything is on;
  and the key a comparison compares by. Four over the program: the dialog
  naming both documents and offering what Word offers, a tick changing what
  is compared, a comparison into a new document letting go of the file it
  came from and one into this document keeping it, and a cancelled dialog
  comparing nothing. The dialog photographed.
  *Not done, and named here:* **the parts of a document this compares are its
  paragraphs.** Word's dialog also offers headers and footers, footnotes and
  endnotes, text boxes, fields and comments, and each of those lives in a
  part or a stream of its own that this comparison does not walk at all. That
  is **J26**. Word's "Show changes at: character level", which marks the
  letters that changed inside a word rather than the word, is not offered:
  this marks whole words, which is Word's own default. And a move is a whole
  paragraph here, where Word will call a moved sentence a move.
- [x] **J14. A person's defaults, kept where a person's defaults go.** Set as
  Default — on the Font dialog, on the Paragraph dialog, on the Design tab's
  themes — wrote into the document and into a setting this program keeps.
  Word writes it into `Normal.dotm`, which **J6** makes and reads.
  *Done:* Set as Default means what it says. All three of Word's buttons
  write twice now — into the document, because the person is looking at it
  and expects it to change, and into the template, because that is what "from
  now on" means. A default that reached only the document open at the time
  was answering a different question from the one that was asked, and the
  person would have found out a week later when the next document came up in
  Calibri again. The template is read afresh, changed and written back, which
  is the care `Normal.dotm` needs: holding it open would be two windows
  writing over each other's defaults.
  With them, Word's **Manage Styles**, which the Styles pane's button now
  opens. Two of Word's four tabs are elsewhere in this program already —
  Restrict is the style list inside Restrict Editing (**J9**) and Edit is the
  Modify Style dialog — so what is here is the other two: **Set Defaults**,
  which is the third place Word offers Set as Default and the one that says
  plainly what the other two only imply, with its choice between this
  document and every document; and the **Organizer**, which is how a style
  gets out of the document it was made in and into the template, or back.
  A style is copied as it was written rather than read and written again: a
  style is full of properties this program does not model, and a copy that
  kept only what it understood would be a different style wearing the same
  name. Renaming changes the name and not the identifier, because everything
  that uses a style refers to it by that.
  *Proven by:* eight tests over the document — a style copied whole with what
  it said intact, one that is not there not copied, a copy onto a style of
  the same identifier replacing it and leaving one of it, a style taken away
  with the rest untouched, a rename keeping the identifier the paragraphs
  refer to and the name written where the schema wants it, and an empty name
  refused. Six over the program: the dialog showing what a document actually
  starts from rather than empty boxes, Set Defaults reaching the document,
  the Organizer listing both files, a style deleted and renamed, and one that
  is not there doing nothing. Both pages photographed.
  *Not done, and named here:* **Word asks and this does not.** Word's Set as
  Default puts up "Do you want to set the default font to Calibri 12? This
  will affect all new documents based on the NORMAL template", with two
  buttons; here the button on the dialog says which it is — Set as Default
  means every document and OK means this one — and Manage Styles asks the
  question outright with its Apply to. Word's Organizer also copies between
  two files neither of which is open, through a pair of Close File buttons
  that open any file; this copies between the document and the template,
  which is what the button is for nine times in ten. The Organizer's other
  three tabs — AutoText, toolbars and macros — belong to features that are
  **J6**'s, not built, and **L**'s.
- [x] **J15. The boundary round a content control, and its Properties.**
  Word draws a faint tag at each end of a control so a person can see where
  one starts and ends, and its Properties dialog names a control, gives it
  its list, and says whether it can be deleted or its contents edited.
  *Done:* both, and the two locks are enforced rather than merely written.
  **The boundary.** A tag at each end of every control, in a colour of its
  own in both themes: a short upright with a lip at the top and the bottom,
  which is Word's shape and reads as a bracket without being a letter. The
  reason it is there is the reason Word has one — a control is a box somebody
  is meant to fill in, and a box nobody can see is a box nobody knows to fill
  in.
  **Properties**, on the Developer tab beside the six controls that make one.
  The title a person reads and the tag a program uses, kept side by side
  because they are for different readers; the two tick boxes; and, for a list,
  the list itself. A control made and never changed was a control whose name
  was wrong for ever and whose list was whatever it was born with.
  **The locks do something.** `w:lock` is one attribute with four words in
  it, which is how the format writes two answers, and both are read and
  written. A control whose contents are locked takes no typing and greys out
  the ribbon while the caret is in it — by the document rather than by a
  restriction, so it holds whether or not anything else does, which is the
  point of it: a form's labels stay labels while its boxes are filled in. One
  that cannot be deleted is not deleted, whatever is selected round it, and
  says so by name.
  *Proven by:* five tests over the document — a control named after it was
  made, the two locks written and read back as the one word the format uses
  for each pair, the lock taken off leaving no element behind, a list given
  another list with the empty entries dropped and what it shows made one of
  what it offers, and a list of nothing refused. Six over the program: the
  dialog showing what the control says and writing back what it is told, a
  list control offered its list and a text one not, the list typed into the
  dialog becoming the list the control offers, the dialog refusing to open on
  no control at all, a locked control taking no typing while the rest of the
  document stays open, and one that cannot be deleted surviving a selection
  that covered it. The dialog photographed, and the boundaries in a document.
  *Not done, and named here:* **the tab with the control's name on it.** Word
  draws the title in a small grey tab above the control when the caret is in
  one; here the name is in the strip along the bottom, which says the same
  thing in a place a person has to look at rather than one they cannot miss.
  Word's Properties dialog also carries the placeholder text, a style for the
  contents, a date's format and calendar, and Add, Modify and Remove under
  the list where this takes the whole list at once. **L10**'s Design Mode is
  now unblocked: it has a boundary to draw.
- [x] **J16. The rest of Word's Restrict Editing pane.** The boxes under
  "Formatting restrictions" that **J9** left, and the styles its list did not
  reach.
  *Done:* **"Block Quick Style Set switching"**, which needed something to
  block. Word's Design tab opens with a gallery of style sets, and every one
  is a table of what the built-in styles should say: picking one changes
  every heading in the document at once without a paragraph being touched.
  That is what makes it worth having and exactly what makes it worth locking
  — a document that limited formatting and left the set free could be
  reformatted entirely in one click. So there are five sets, named for what
  they do and drawn here rather than borrowed from Word, and `w:styleLockQFSet`
  is read, written and enforced: the button greys out and the gallery does
  not open.
  **"Allow AutoFormat to override formatting restrictions"**,
  `w:autoFormatOverride`, which is the one box on that half of Word's pane
  that lets something through rather than shutting it. Typing `*word*` turns
  it bold, which is formatting by hand under another name; without the box
  the marks now stay as marks, and with it the correction goes through.
  **The latent styles, one at a time.** A `styles.xml` defines a few dozen
  and mentions a few hundred, and the mentioned ones are as real to somebody
  applying a style as the defined ones: a restriction that dealt only with
  the defined ones left three hundred doors open. **J9** locked them as a
  body; they are listed in the dialog now, each with its own tick, and each
  is locked by its own `w:lsdException` — made where the document had none,
  because saying something about one is what makes it an exception.
  **The padlock.** The gallery leaves a locked style out, because a tile that
  does nothing when it is clicked should not be there; the Styles pane is the
  list of everything the document has, so it keeps them and says which they
  are. Word draws a padlock; this says the word, because a padlock is a
  picture only a font that has one can draw.
  Along the way a style could not carry a border or a shading at all: the
  writer that puts formatting into a style did not write either, so the rule
  under a heading had to be written on every paragraph. Both are written into
  a style now, which is what a set of styles needs to say anything about
  shape.
  *Proven by:* eight tests over the document — both new attributes through a
  round trip and written the way Word writes them, the latent styles read one
  at a time with their own locked state, one locked and freed by name, one
  the document never mentioned given an exception of its own, and a locked
  latent style unavailable only while something is enforced. Ten over the
  program: the correction refused while formatting is limited and let through
  when the box says so, the style-set button greyed out and its gallery
  refused to open when the set is fixed and both working when it is not, every
  set having a name and a word about it, every heading a set names being
  written, the set in use being the one the styles match, a set past the end
  writing nothing, and the rule and the band arriving where the set says.
  The gallery photographed, and a document with a set applied.
  Along the way the catalogue of what this program can say learnt that a
  colour is not a message: six hexadecimal digits are the same colour in
  every language.
  *Not done, and named here:* **the pane itself.** Word's Restrict Editing is
  a task pane that stays open beside the document, where this has a dialog.
  What the pane is *for* — ticking Everyone and then selecting the text it
  applies to — is a button on the Review tab here (**J11**), and the rest of
  what stands in it is **J23**, which is where the pane belongs. The five
  style sets are this program's own five; Word ships seventeen, and they are
  its content rather than a format.
- [x] **J17. The information bar, and General Options as one dialog.** Word
  puts a bar across the top of a document to say something about it and offer
  one button; and its General Options holds both of the passwords a document
  can carry.
  *Done:* **the bar.** A strip under the ribbon in a colour of its own, with
  what is true of the document, one button, and a cross. It is furniture
  rather than a message for the reason Word made it furniture: what it says
  is true for as long as the document is open, and a message in the strip
  along the bottom is gone by the next keystroke — a person who starts typing
  into a read-only document and finds nothing arriving has to be told why
  *then*. Three things put one up: a document opened read-only, with Edit
  Anyway on it; one carrying Visual Basic, which says so and offers no
  button, because nothing here runs a macro and a button saying Enable
  Content would be a lie (**L6** is where that button arrives); and one just
  recovered, with Save As. Shutting it changes nothing about the document,
  which is Word's behaviour: the File page still says so.
  **General Options.** Word's dialog holds the password to open and the
  password to modify together, and they do different things — one makes the
  file unreadable, the other decides whether it opens for writing. They are
  in one dialog here now, each under a heading that says which it is, with
  the two lines that have to be said about the second: it does not encrypt
  anything, and anybody who can open the file can take it off. An empty box
  takes a password off, which is how every password box in this program says
  none. A mistyped password to open writes neither and asks again with what
  was typed kept.
  *Proven by:* seven tests over the program — the bar up on a read-only
  document and still there after a keystroke, gone when the document is
  opened for writing, shut by its cross without changing what is true, and
  taking exactly its own height from the page; both passwords written from
  one dialog and not mixed up; a mistyped password to open writing neither
  and asking again; and an empty box taking the encryption off. Three over
  the bar itself: every reason saying something, a reason with nothing to
  offer offering no button, and a bar that has never been drawn having no
  parts to press. The bar photographed, and the dialog.
  *Not done, and named here:* Word's bar also appears for a document marked
  as final and for one opened in Protected View, neither of which this
  program has. Its General Options carries two more boxes — Remove personal
  information on save, and the macro security button — of which the first is
  on the File page here and the second belongs to **L6**.
- [x] **J18. The rest of the Mail Merge Recipients dialog.** Sorting,
  filtering, finding duplicates, finding a recipient and checking addresses;
  and Word's Type a New List.
  *Done:* the four buttons under the list, and the list that can be made
  without a file.
  **Sorting** puts the rows in order on any column, either way round, and
  takes the ticks with them — a list somebody had unticked three people in
  and then sorted would otherwise quietly send three letters to the wrong
  people. A column of numbers sorts as numbers, which is the whole difficulty
  of sorting a list: as words, 10 comes before 9.
  **Filtering** leaves out everybody the words do not fit, in one column or
  in all of them, minding neither case nor where in the value the words fall
  — which is what somebody typing "lancre" into a box means. It works by
  ticking rather than by hiding, because the tick is what decides who gets a
  letter and a person should be able to see who was dropped.
  **Finding duplicates** leaves out the copies and keeps the first of each,
  which is what a person wants from it; two rows count as the same when every
  column says the same thing without regard to case or to the spaces round
  it, because that is how two lists merged by hand differ and mean the same
  person.
  **Checking addresses.** Word calls an add-in and says so plainly when there
  is none. What can be done here without one is the half that is arithmetic:
  a letter needs somebody to send it to and somewhere to send it, so a row
  with no name or nothing that can be addressed is named as such, and the
  dialog goes to the first one. Whether a street exists is not a question a
  program with no map can answer, and this does not pretend to.
  **Type a New List**, under a Select Recipients menu that now offers both
  ways of getting one. A person is typed into Word's own nine boxes, Add puts
  them on the list, and the list is saved as the delimited file this program
  already reads — Word writes a database of its own, which is a second format
  for the same nine columns.
  *Proven by:* eleven tests over the list — a column of words and a column of
  numbers each sorted as what they are, a filter minding neither case nor
  position, the copies found and the originals not, a row short of a name and
  one short of an address named, an email counting as somewhere to send it, a
  reorder, somebody typed from nothing bringing their own columns, and a list
  written out reading back as itself with its commas and quotation marks
  intact. Nine over the program: the dialog offering Word's four buttons and
  every column, sorting carrying the ticks with the rows, filtering leaving
  out what does not fit, duplicates left out and the original kept, the check
  naming what is missing and saying so when nothing is, a sort with no column
  saying what it wants, a list typed one person at a time with the boxes
  cleared between, adding nobody saying so, and both ways of getting a list
  reachable. Both dialogs photographed.
  Along the way a row shorter than the list's columns read as *no such
  column* rather than as an empty one, which is what it is: a list somebody
  added a column to has rows that stop short.
  *Not done, and named here:* Word's dialog is a table with a column of ticks
  and a heading over each column that drops its own filter; this is a list of
  people with the buttons under it. Its filter compares one value with one
  operator — equals, is blank, is greater than — where this asks whether the
  value holds the words. Its Find Recipient is a box that walks the list; the
  filter does that job here. And **J5** names the three other places Word
  gets a list from, of which Outlook's address book is the one this menu
  leaves out.
- [x] **J19. The rest of the Building Blocks Organizer.** The columns it
  sorts by, editing a block after it is saved, and Word's AutoText gallery.
  *Done:* **refiling one.** A piece saved in a hurry under "Block 1" in the
  wrong gallery is a piece nobody will find again, and saving it afresh means
  having the document it came from still open. The organiser has the four
  boxes now — name, gallery, category, description — and a Modify button that
  writes them back on to the block without touching what is inside it. It is
  the same block, filed differently. A name another block already has is
  refused, because two blocks of one name is a menu where one can never be
  picked.
  **The columns.** Word's organiser is a table whose headings sort it; this
  is a list with a box that says what to sort by — name, gallery or category
  — which is the same answer in the shape this program's dialogs have. Each
  row says all three, so the list can be read without picking anything. And
  it lists every gallery rather than only the one the Quick Parts menu shows,
  which is what makes it the place to find a piece filed in the wrong one.
  **AutoText.** Word's other gallery, the same machinery under an older name,
  and what a person puts in it is the thing they type every day — a sign-off,
  an address, a paragraph of terms. It has a menu of its own now, under the
  Quick Parts button where Word's is, listing what is in it and offering Save
  Selection to AutoText Gallery. Which gallery a saved piece goes to is
  whichever menu asked, so saving to one does not quietly file it under the
  other.
  *Proven by:* four tests over the document — a block refiled with its
  content intact and its old name no longer answering, a refiling onto
  another block's name refused while keeping its own name is not, a block
  that is not there and a nameless one both refused, and the gallery deciding
  which menu a block is on. Three over the program: the Quick Parts menu in
  Word's order with AutoText on it, the AutoText menu being what is in that
  gallery and the way to add to it, and a gallery shown by the name a person
  reads rather than the one the file uses. The organiser photographed with
  both galleries in it.
  *Not done, and named here:* Word's organiser also shows which template each
  block came from and what it does when it is inserted — its own paragraph,
  its own page — and can copy a block from one template to another. This
  program has one template, so there is one column's worth of that question
  and no other file to copy to. Word's AutoText also completes as a person
  types the first few letters of a block's name, which is a different feature
  and is not here.
- [x] **J20. Word's Compare dialog, and the pages that follow the result.**
  The two drop-downs, and the column beside the result showing the place a
  person is reading.
  *Done:* **the two drop-downs.** Word's dialog names both documents and lets
  either be something that is not open, which is the whole reason it has two:
  a person comparing last week's draft with this week's wants neither of them
  on the screen. Both lists hold this document and everything lately opened,
  with the file just picked at the end of the second and already chosen. The
  original is opened and becomes the result, because that is what a
  comparison is — the first document with what the second did to it marked on
  it — and a comparison whose original is not this document always goes into
  a new one, since this document was not in it.
  **The column follows what is being read.** It showed the page the caret was
  left on, which after a comparison is wherever the marking finished. It
  shows the page at the top of the window now, so that a person scrolling
  through a comparison sees the same place in all three.
  *Proven by:* three tests — the drop-downs offering this document and the
  ones lately open with the picked file chosen, a document compared with
  itself coming out unchanged (which is the pair being read back rather than
  assumed), and the column following the view rather than the caret. The
  dialog photographed.
  *Not done, and named here:* Word's drop-downs carry a Browse button apiece,
  so a file that is not on either list can be picked from inside the dialog;
  here the file is picked before the dialog opens, which is one step in a
  different order. Its Label changes with box names the author whose changes
  these are, where this signs them "Compare". And the two halves of the
  column show whole pages rather than scrolling within one, which is right
  while a page is small enough to read and would not be on a longer one.
- [x] **J21. The `\006DataSpaces` storage, and the encryptions before 2007.**
  What an encrypted file says was done to it, and the two schemes Office used
  before AES.
  *Done:* **the storage.** The format does not say "this file is encrypted".
  It says the stream called `EncryptedPackage` has had a transform applied to
  it, names the transform, and says what that transform needs to know —
  because the same machinery carries the rights-management path, where the
  transform is a different one and the key comes from a server. A file that
  said nothing about that was a file saying nothing about what was done to
  it; the four little structures are written now, in the shape the
  specification gives them, and read on the way in: a file naming a transform
  this program has not got is refused rather than half-deciphered, and one
  with no storage at all is taken at its word, as Word takes it. The compound
  file writer learnt to nest storages inside storages on the way, which this
  needed and nothing had needed before.
  **RC4.** A document somebody encrypted in Word 97 or Word 2003 is still
  their document, and a word processor that cannot open it has lost it.
  Reading a weak cipher is not endorsing it; refusing to read one is losing
  somebody's work to make a point. So both schemes are read and neither is
  ever written: the CryptoAPI one, which is the same header as the AES scheme
  with the cipher named differently and a key for every five hundred and
  twelve bytes; and Word 97's own, which has no header at all and whose key
  is MD5 all the way down. Sealing a document is still AES with a two hundred
  and fifty-six bit key and nothing else.
  *Proven by:* RC4 against RFC 6229's own test vectors and against OpenSSL
  both ways round at seven lengths — OpenSSL keeps it in a provider that has
  to be asked for by name, which is a fair comment on the cipher and no help
  to somebody with a document from 2003. MD5 against RFC 1321's seven
  examples and against OpenSSL at every awkward length round the padding.
  Then whole files, each assembled here byte by byte from the specification's
  own description and opened by code that had no part in assembling it: one
  encrypted the 2003 way at forty bits and at a hundred and twenty-eight, one
  the 1997 way, a wrong password refused rather than deciphering to nonsense,
  the storage found where the format says with the transform that means a
  password, a file whose transform is another one refused, and a file with no
  storage opened.
  *Not done, and named here:* whether Word opens what this writes is still
  unproven, and is **K1**: there is no Word in the build image, and
  LibreOffice 7.4 takes no password on its command line. What the storage
  changes is that a reader stricter than Word now has nothing to object to.
  The rights-management path itself — asking a server for a key — is not
  done and is not planned: it is a licence server, not a file format.
- [x] **J22. Word's own galleries of content.**
  **Two questions, not one.** Word's Page Number button asks where the number
  goes and then what it looks like. This program asked the first and answered
  the second for the person: Top of Page put a plain centred number at the
  top and that was the whole of it. So the three places open a gallery now,
  and carry the triangle every menu in the world puts on a row that opens
  another — a row that looks like it will act and instead opens a second list
  has misled whoever pressed it.
  **The gallery is this program's own.** Word ships twenty-odd arrangements
  called "Accent Bar 2" and "Vertical Outline 1", and **J6** would not put
  something else behind those names. These are nine, drawn here, named for
  what they draw: plain to the left, the middle and the right; "Page 1";
  "Page 1 of 4"; in brackets; between dashes; on a grey band; against a rule.
  A design is two decisions and nothing else — how the number reads and what
  is drawn round it — and everything else follows from where it was asked
  for, which is why the rule under a running head is drawn above a running
  foot: both face the text. Where the number goes into a sentence there is no
  paragraph of its own to align or to shade, so what is offered there is the
  readings and one of each, rather than three lines that would do the same
  thing.
  **And the half of a gallery that is the person's.** Every one of Word's is
  half its own content and half what somebody saved into it, and the second
  half was missing from all of them. The page-number galleries and the cover
  pages now list what the person has put there and end with the line that
  puts another one there. That needed the galleries to be named as the format
  names them — `pgNum`, `pgNumT`, `pgNumB`, `coverPg`, `watermarks` and the
  rest — so that a block saved here turns up in Word's own gallery rather
  than nowhere, and a block out of a document Word wrote turns up on the menu
  it belongs on. Save Selection carries which gallery asked, instead of the
  one flag that could only tell Quick Parts from AutoText, and the organiser
  can name every gallery there is: one that could not would have shown a
  saved page-number design as a Quick Part and quietly refiled it there the
  moment anybody pressed Modify.
  *Proven by:* fifteen tests over the designs and the gallery — every design
  holds a `PAGE` field, no two draw the same thing, the rule faces the text
  at either end of the page, the band is a band, a design chosen at the foot
  writes a footer and leaves the head alone and the other way about, a number
  at the caret goes into the text and touches neither, the separator does
  nothing, and the last line asks what to call what is being saved and files
  it under `pgNumB` rather than under Quick Parts. Pictures: the place menu
  with its three triangles, the gallery of nine under Bottom of Page, and the
  page itself with the band drawn at the foot and the rule at the head — a
  gallery offering an arrangement the page could not draw would be offering a
  picture of one.
  *Not done, and named here:* Word's fourth place, Page Margins, is **J27** —
  it is a text box anchored to the margin, not a header. Its
  "Remove Current Cover Page" is **J28**: nothing marks which blocks of the
  document are the cover page, so nothing can take them away again. The
  watermark gallery still has only its first half, and that is **J29**,
  because a watermark here is a word painted behind the sheet rather than
  content in the header, and a building block cannot be one. And the
  catalogue of messages misses anything that reaches the interface out of a
  table — which is why `messages/de.txt` carried German for thirty-five
  messages `en.txt` does not list, and why the page-number designs had to be
  walked into it by hand: **J30**.
- [x] **J23. Word's Restrict Editing pane, and what stands in it.**
  **A pane, because a dialog cannot be open while the selection is made.**
  That is the whole of why Word puts this down the side of the window, and it
  is what **J16** named: restricting a document is not one decision taken
  once. A person ticks a box, selects a paragraph, says who may edit it,
  selects another, looks at what they have done, and only then enforces any of
  it. The pane is Word's four numbered sections before enforcement — what
  formatting is allowed, what editing is allowed, who is let through anyway,
  and the button that starts it — and a different pane after: Your
  permissions, saying what this person may do and where. The two dialogs Word
  keeps are kept: the list of every style the document has, which is three
  hundred rows and would leave no window, and the box that takes a password.
  **The list of people, which the rest waited on.** The answer turned out to
  be that the document already knows. Every stretch with its own rule names
  somebody in its own marker, so the names in a document are the people it has
  been shared with; reading them back is how a document somebody else
  restricted shows its editors here. To those go the person at the keyboard
  and whoever has been typed into Word's More users…. There is no directory to
  ask and there is not going to be one, which is exactly what Word's own Add
  Users dialog assumes: it is a box that takes names separated by semicolons.
  **Several people on the same words.** The format gives a marker a single
  editor, so a stretch two people share is two pairs of markers round the same
  words. The rule that decides whether somebody may type read the first pair
  it found, which would have let one of them in and shut the other out; it
  reads all of them now, and a person is admitted when any pair covering the
  whole of what would change admits them. Unticking somebody takes off their
  pair and leaves everybody else's.
  **What the page shows.** Each stretch is washed over in the colour of
  whoever may edit it — the same colour their tracked changes are drawn in,
  because two places choosing a colour for one name would sooner or later
  choose two — with a bracket at each end, which is what says where a stretch
  begins and ends when two of them touch. The whole of it is behind Word's own
  tick box, because shading is help while a person is looking for where they
  may type and clutter once they have found it. Find Next Region I Can Edit
  walks them and comes round again.
  *Proven by:* twenty-one tests. The button opens a pane and not a dialog and
  shuts it again; the list is the group, then the person at the keyboard, then
  whoever the document names; a tick writes the markers round the selection
  and a second tick takes them off; two people on the same words are two pairs
  and both may edit; unticking takes off their pair and leaves somebody
  else's; nothing selected says what it needs rather than marking nothing;
  Find Next walks two stretches and comes round, says so when there is
  nowhere to go, and does not count a stretch that names somebody else; Show
  All turns the shading on and says how many; the tick box turns it off; More
  users… keeps what was typed and the same name twice is one name; the pane
  changes once something is being enforced; and a person's colour is the
  colour their changes are drawn in. The pane's own typography is measured
  rather than squinted at: a test draws it and checks the underline under a
  link lands below the words and not through them, which is what told me the
  strikethrough I thought I saw in a picture was a small font read at the
  wrong size. Pictures of both faces of the pane, and of a marked stretch
  washed and bracketed on the page.
  *Not done, and named here:* Word's Show All Regions I Can Edit **selects**
  every one of them at once. This program has one selection and no way to hold
  several, so the button turns the shading on and goes to the first instead;
  a selection of several stretches at once is **J31**, and it is wanted for
  more than this. The pane does not scroll, so a document with twenty editors
  runs off the bottom of it: **J32**. And Word's Formatting Restrictions
  dialog has All, Recommended Minimum and None above its list of styles, which
  is the difference between ticking three boxes and ticking three hundred:
  **J33**.
- [x] **J24. Signing with a key the system holds.**
  **What signing needs is not a key.** That was the thing to see. **J12** had
  a signer carrying a `PrivateKey`, which is fine as long as the key is
  something this program can have — and on Windows it is not, and is not meant
  to be. A person's certificates live in a store the system keeps; the system
  signs on their behalf and hands back a signature, never the key. That is the
  point of a store, and it is the only way a key on a smart card or in a TPM
  can be used at all. So the signer takes something that *will sign* — a
  trait, `Signs` — and a key read out of a file is one of those while the
  system is another, neither knowing about the other. Nothing about the
  signature changes: the same `SignedInfo`, the same PKCS#1 v1.5 padding over
  the same `DigestInfo`. Only the arithmetic happens somewhere else.
  **The store, read.** `CertOpenSystemStoreW` on MY, and every certificate in
  it that has a key behind it — asked as a property rather than by acquiring
  the key, because acquiring one can put a smart-card dialog on the screen and
  a list being drawn is no place for that. The name is the system's own
  display name, which is what a person recognises. Certificates with no key
  are left out: offering one would be offering to do something that cannot be
  done.
  **The signing.** `CryptAcquireCertificatePrivateKey` for a key of the newer
  sort, silently — a program should ask before it signs, not while it is
  signing — then `NCryptSignHash` with PKCS#1 padding and the hash's own name.
  The hash is made here and the padding is written there, which is the one
  place in the program where the two halves of a signature happen on opposite
  sides of a wall. Every function is found by name at run time, as the rest of
  this crate finds its newer ones: a program that will not start because a
  library is missing is worse than one that says it could not read the store.
  **Both lists are one list.** A person picking a certificate is picking who
  they are, not which of two mechanisms does the sums, so the store's and the
  folder's are offered together — the store's first, because on a machine that
  has one that is where the real certificate is. The line says where each came
  from, because the same certificate can be in both places and the two sign by
  different routes: one of them can want a card in the reader and the other
  cannot.
  *Proven by:* the whole of it compiles and lints clean for
  `x86_64-pc-windows-gnu`, which is the target it exists for. What can be run
  here is run here: the trait changed no signature — the same key over the
  same bytes, through the trait, is byte for byte the signature the key makes
  — the hash named to the system is the hash that was made, a machine with no
  store lists nothing rather than guessing, signing with a certificate no
  store holds is refused rather than returning an empty signature, and the
  line a person picks says which of the two places it came from. The rest
  stands on **J12**'s proof, because it is the same signature: what OpenSSL
  signs this program accepts, and what this program signs OpenSSL and
  `xmlsec1` accept.
  *Not done, and named here:* **whether the Windows calls work has not been
  run.** It cannot be, here: there is no Windows machine in this build and
  nothing is installed on the one this is written on, so what is proven is
  that it compiles for Windows and that everything either side of the system
  call is right. Running it — the store listed on a real machine, a signature
  made through it, and Word opening the result — belongs with **K1**, which is
  where this project's "against Word rather than against ourselves" lives. No
  amount of care here substitutes for it, and the entry says so rather than
  implying the calls have been seen to work.
- [x] **J25. The rest of what Word does with signatures.**
  **The pane, and why this one especially.** A person reading a list of
  signatures is reading the document at the same time — checking that the
  thing signed is the thing in front of them — and a dialog covers the
  document in order to tell you about it. The other reason is that the list
  has two halves that answer each other: what the document carries, and what
  it is still waiting for. A signature line somebody put in is a request; a
  signature made for that line is the answer to it; a request with nothing
  against it is what is left to do. The pane shows the two together and pairs
  them, which is the whole reason a signature line is worth having.
  **A line that can be named.** That pairing needed the line to have an
  identity, and it had none: it was four paragraphs and a border. Word marks
  its own with a picture carrying an `o:signatureline` element, which a reader
  that does not know the element sees as a picture with no selectable name.
  So each line is wrapped in a bookmark carrying an identifier — it prints the
  same, the name is still text, it survives a round trip through Word — and a
  signature made for that line writes the identifier into its `SetupID` and
  calls itself the kind of signature that is about a line. Double-clicking a
  line asks to sign it, which is Word's own shortcut.
  Bookmarks would not span paragraphs, as it turned out: the end mark was put
  at the end of the *first* paragraph whatever had been selected. Word's span
  anything, and so do these now — it was three lines, and it is why a mark
  round a four-paragraph signature line works at all.
  **What a signature says about itself.** A signature proves that whoever held
  a key signed some bytes. It does not say when, and it does not say which
  certificate — the certificate travels beside the signature and could be
  swapped for another with the same key. XAdES is the standard answer to both,
  and it is written now: the signing time and a digest of the signer's own
  certificate, inside `SignedProperties`, referenced from the signed
  information so that neither can be changed by anybody without the key.
  **One signature over another.** A second signature over a document says two
  people signed the same thing. A countersignature says something stronger:
  that this person saw *that signature* and signed it — a witness, an
  approval. What it covers is the first signature's value, so it cannot be
  lifted off and put on another signature or another document. It goes among
  the first signature's *unsigned* properties, which sounds alarming and is
  not: a signature never covers its own value, so writing beside that value
  takes nothing away from it, and the first signature holds exactly as well
  afterwards. What a countersigner gives is the capacity they signed in — a
  witness, an approver — which is what XAdES has a place for and what tells
  two countersignatures on one signature apart.
  *Proven by:* thirteen tests over the signature itself and seven over the
  pane. A countersignature is read back and holds; two of them sit beside each
  other and each says what it was signed as; one does not hold over a
  signature whose value was changed, which is the whole point of it. A
  signature made for a line says which line and one about the document says
  none; the signing time cannot be edited without the signature noticing. A
  line comes back by the name it was given, survives being written out and
  read back, and its mark does not show among a person's own bookmarks. The
  pane pairs a signature with the line it answered and stops that line being
  asked for, does not pair one that answered nothing, and refuses to sign an
  unsaved document rather than offering a button that cannot work. And the
  two independent checks that matter most still pass: what this program signs,
  `xmlsec1` accepts — XAdES reference, canonicalisation and all.
  *Not done, and named here:* **XAdES-T**, the timestamp. Everything above is
  the signer's own claim, including the time; making the time worth more than
  a claim means a timestamp from somebody else, which means asking a timestamp
  authority, which means a network. There is none here and there is not going
  to be one, so it is named rather than numbered: what would make it an item
  is a decision to let this program talk to a server, and that decision has
  not been asked for.
- [x] **J26. Comparing the rest of a document.**
  **One comparison, not six.** The thing to see was that marking a difference
  is the same work wherever the words are, and that work is written once —
  against a document. A header is not a document; a document with the header's
  paragraphs in it is. So a part is compared by making a document of it,
  comparing that, and taking the body back out: what comes out carries the
  tracked changes in the model, and a header can take them. Any other way
  would have meant a second copy of the comparison that only knew about
  headers, and a third that only knew about footnotes.
  **Headers and footers**, every kind a section can have — the ordinary one,
  the first page's, the even pages' — and section by section. A document that
  says something different on its first page says it whether or not anybody
  compared it, and this was the case the item was written for: a comparison
  that walked only the body called two documents the same when one of them had
  a different footer on every page.
  **Footnotes and endnotes**, paired by the identifier the format gives them
  rather than by the order they appear in, since reading order changes the
  moment somebody adds a paragraph above a mark. The mark that raises the
  number stays where it is: it is the first run and it is not words.
  **Comments**, paired the same way, and with who wrote them and when left
  exactly as they were — a comparison says what the comment came to say, not
  that somebody else wrote it.
  **Text boxes**, which turned out not to be compared at all and not to be
  obviously missing either: their paragraphs are in the body's own part, and
  the walk that visits every paragraph stops before reaching them, because
  what stands between a paragraph and a text box's paragraph is a run of
  elements from the drawing namespaces. So they are walked on their own.
  **Fields**, which is a different kind of difference: a field shows what it
  last worked out, so two documents can show the same word where one counts
  the pages and the other numbers them. Reading only what is on the page calls
  those the same. The instruction is part of what a paragraph is compared as
  now, and a paragraph that differs by nothing else has its field struck out
  and the new one put in.
  And a tick box apiece, which is nine in a dialog that had four.
  *Proven by:* seven tests over the parts. A different footer is a difference
  and the old words are struck out rather than thrown away; the footer is left
  alone when that box is not ticked; all three kinds of running head are
  compared and not only the ordinary one; a changed footnote is marked inside
  the footnote and keeps the number the reader sees; a changed comment is
  marked inside the comment and keeps its author; and two fields that read the
  same but say different things are a difference, unless the box says
  otherwise.
  *Not done, and named here:* a text box has **no identifier** — nothing in
  the format names one — so two documents' boxes are paired by the order they
  stand in. That is right for a revised copy of a document, which is what a
  comparison is of, and wrong for a copy with a box inserted in the middle,
  which would pair the rest of them one out. There is nothing in the file to
  do better with. And a paragraph whose **words and field both** changed has
  its words compared and its field left behind, because the machinery that
  puts changed words in takes text and not runs; the field-only case is the
  one the tick box is about and is the one that is handled.
- [x] **J27. Page numbers in the margin.**
  **Three things had to be true and two of them were not.**
  *The margin had to be somewhere.* The format names six frames a drawing can
  be measured from that are not the text area — the left margin, the right,
  the top, the bottom, the binding side and the other one — and this program
  read every one of them as "the margin", meaning the text area. That is the
  difference between a number standing *in* the margin and one against the
  edge of the text, and it was being thrown away on the way in. The six are
  read, written and laid out now, each as the band it is.
  *A running head had to be able to carry a drawing.* It could not. A head is
  laid out on a page of its own and slid into place once its height is known,
  and what slid into place was its words, its underlines and its pictures —
  the drawings were laid out and then dropped on the floor. They are carried
  over now.
  *And the drawing had to land where the paper is.* A head's words flow from
  nought downwards and are moved afterwards; a drawing it carries is anchored
  to the paper and must not be moved with them. So what the move will add is
  taken off while the drawing is placed, and the two cancel. A head's offset
  is known before anything is laid out; a foot's is not, since it depends on
  how tall the foot turns out, so a foot is laid out twice — once to learn
  that and once to place what it carries. Twice over a few lines is cheaper
  than a drawing in the wrong place.
  **What it comes to.** Page Margins stands on the place menu as Word's fourth
  place, with two designs behind it — the left margin and the right — because
  a margin has two sides and there is nothing else to decide about a number.
  The design is a text box holding the `PAGE` field, with no line round it and
  nothing behind it, anchored to the margin band across and to the middle of
  the paper down. It goes into the header, because a header is what every page
  has a copy of: the number is not in the header, the box is. And
  `pgNumMargins` is a gallery a person saves into like the other three.
  The rows of that menu were not translatable either — none of the four places
  was in the catalogue — so a place knows its own name now and the catalogue
  is gathered from the same table the menu is built from.
  *Proven by:* four tests over the place and its designs — the margin has
  designs of its own and saves into `pgNumMargins`, the box is anchored to the
  left or the right margin band and holds a `PAGE` field, choosing one writes
  a header carrying the box and leaves the foot alone, and all four places
  have a name and a gallery. Six more over the frames: each margin comes back
  as itself and not as the text area. And a picture of the page with the
  number standing in the right margin, which is the whole of what the item
  asked for and what no test would have caught if the drawing had been
  dropped on the floor again.
  *Not done, and named here:* **inside and outside** are read, written and
  laid out as left and right. They are the binding side and the other one, and
  which is which changes with the page in a document printed on both sides —
  so what is drawn is right for a document printed on one side and for the odd
  pages of one printed on both, and wrong for the even pages. Mirroring them
  is a question about the page rather than about the drawing, and belongs with
  the mirrored margins themselves.
- [x] **J28. Taking a cover page off again.**
  **Nothing could be taken off because nothing said what to take.** A cover
  page is a stretch of perfectly ordinary paragraphs — some empty ones, a
  title, a name, a date, and a page break — and once it is in the document
  there is nothing about it that says where it begins or ends. A program that
  guessed by looking at the words would take away the first page of every
  document that happens to start with a title.
  So one is wrapped in the mark Word wraps its own in: a `w:sdt` whose
  properties carry a `w:docPartObj` naming the gallery it came from. The block
  reader already sees through those marks, so the paragraphs are read, laid
  out, edited and saved exactly as before — the mark changes nothing about
  what the document is, and says what part of it the cover page is.
  Taking it off takes away that whole mark: the words, the room above them,
  and the page break that made the document proper start on a sheet of its
  own. Word's line stands at the foot of the gallery, and only when there is
  one to take off.
  *Proven by:* six tests. A document with no cover page says so and taking one
  off does nothing; one put in here carries the mark; the mark survives being
  written out and read back, which is what makes a cover page put in today one
  that can be taken off next week; taking it off leaves the document with the
  words it had and the paragraphs it had, to the one; it can be undone, since
  one act to a person is one thing to undo; and a document that merely begins
  with a title is not a cover page.
- [x] **J29. The watermark gallery's other half.**
  **The item was written against a wrong belief of mine**, and finding that
  out was most of the work. I had said a watermark here is a value painted
  behind each sheet and not content in the header. It is content in the
  header, and has been since it was written: Word's own VML shape, written
  where Word writes it and read back from there — which is why a watermark
  somebody else's Word wrote already came back as what it is. What was true
  is that nothing could be *saved* into the gallery, and the reason was one
  layer down.
  **The model drops what it cannot name.** A watermark is a VML shape with a
  text path in it, which is neither a picture nor a shape this program draws,
  so reading a header into the model threw it away. That did not matter while
  the model was only ever a view — editing goes through the tree — but it
  matters the moment something builds a part *from* the model. Two things do:
  saving a building block, and **comparing a header**, which **J26** had just
  taught this program to do. So comparing two documents whose headers differed
  quietly destroyed the watermark behind them. That was a bug I had put in a
  week's work earlier and had not noticed; it is fixed here, and the fix is
  the same one the gallery needed.
  What the model cannot name is now carried whole: read as the element it was,
  written back as the element it was, understood by nothing in between. A
  watermark survives a header being rebuilt, and so does anything else this
  program has no name for.
  **And the gallery has its other half.** The menu lists what a person has
  saved, and Save Current Watermark to Gallery puts the shape itself in —
  the shape, not a description of it, so what comes back out is the word, the
  angle and the colour that put it on again. Saved rather than "Save
  Selection", because a watermark is not in the text and cannot be selected,
  and offering to save a selection that could never be one would be offering
  something that never works.
  *Proven by:* six tests. A watermark comes out as content and reads back with
  its angle and its colour; one saved from one document goes on to another,
  which is what a gallery is for; a document with none has none to save; words
  that are not a watermark are not read as one. And the two that matter most:
  comparing a changed header keeps the watermark behind it, and what the model
  cannot name survives a body being rebuilt — the second said at the level the
  first happens at.
- [x] **J30. The messages that live in tables.**
  **Thirty-four messages the program says were in no list.** The German
  catalogue carried every one of them, written in by hand as each was added —
  which is exactly the work the list exists to make unnecessary, and exactly
  the evidence that it was not doing it.
  They came from four places. *Tables nobody walked*: the arrangements a cover
  page comes in, the quick style sets' names and their notes, the columns a
  new list of people starts with, and what the bar across the top says with
  what its button offers. Those are walked now.
  *Sentences picked by a condition and handed over as a variable* — the two
  halves of the password note, the three refusals a limited document gives.
  A message reached through a variable is one the catalogue never sees, so
  each is named where it stands.
  *Sentences said straight to the strip along the bottom*, which nothing was
  reading. Seven of those, each now named where it stands.
  And one that was being **thrown away on purpose**: the word "locked" beside
  a style nobody may use. The gatherer guesses that a single lowercase word is
  the program talking to itself — a key, a marker, an address — and that guess
  is right almost always. But a string inside `t(` is a message by
  construction: that call exists to look one up. So the guess is for the
  looser shapes, a label or a constant, where a program's own name and a
  person's words look alike, and what is asked for by name is taken at its
  word.
  **And a test, which is the point.** Every catalogue that travels with the
  program is now held to the list three ways: it lists nothing the program has
  stopped saying, it lists everything the program says, and it says every one
  of them in its own language rather than listing it and leaving it in
  English. None of those three shows up by using the program — the tests run
  in English, where a missing translation looks exactly like a translation
  that was not needed.
  *Proven by:* the test above, and the two catalogues agreeing exactly: nought
  in one that is not in the other, nought listed without being said. The three
  messages that had never been translated at all are translated.
- [x] **J31. A selection of several stretches at once.**
  **Written against a wrong belief of mine again**, and this is the second
  time in four items. I wrote "this program has one range and everything that
  wants several says so instead of doing it" while closing **J23**, and it has
  had several since long before that: Ctrl and a drag adds a stretch, Alt and
  a drag takes a rectangle as three, Select All Text With Similar Formatting
  selects every run that matches, typing over several empties all of them and
  types where the first was, formatting applies to each, copying joins them
  with a line break, and the drawing shades every one. The belief came from
  the one place it was true — the command I had just written and not finished.
  What was actually missing was that one command. **Show All Regions I Can
  Edit** turned the shading on and moved to the first, which is not what the
  words say and not what Word does. It selects every one of them now, so that
  a person can see how much of the document is theirs and whatever they do
  next is done to all of it — which is the whole of what a selection of
  several stretches is for. The line it says changed from "shaded" to
  "selected", because that is now what happened.
  *Proven by:* three tests, two of them new. It selects every stretch and not
  only the first; what is done next — bold, in the test — is done to all of
  them, since a selection that only looked selected would be a lie; and with
  one stretch it still says one and selects it. The rest of the item was
  already proven by the tests that were there: a Ctrl drag keeps the stretch
  before it, a Ctrl click still takes the sentence, an Alt drag over three
  lines is three stretches.
  *Not done, and named here:* nothing new. What this item taught is about me
  rather than about the program: **J29** and this one were both written from
  what I assumed the code did rather than from what it does, and both times
  the assumption was a fortnight stale. The roadmap is written as work is
  closed, and a sentence about another part of the program put into it at that
  moment is a sentence nobody checked.
- [x] **J32. The panes that do not scroll.**
  **It was two copies of the same idea and only one had been taught.** The
  Restrict Editing pane and the Signatures pane each carried their own pen —
  the same struct, the same six methods, written out twice — and neither knew
  what the Styles pane knew. So the fix was not to teach a third copy but to
  stop having copies: one pen, in one place, which every pane down the side of
  the window is drawn with, and the scrolling lives in it.
  **A pane is scrolled by where its pen starts.** Everything a pane draws is
  placed by running a number down the column, so beginning that number higher
  up the window moves the whole column and nothing else has to know. What
  stops it running away is how far the column reached the last time it was
  drawn, which is only knowable once it has been drawn — so the pane remembers
  it, and the offset is pulled back when the pane shrinks. Somebody who
  scrolls to the foot of a long list and then deletes most of it should not be
  left looking below the end of what is there.
  The caption does not scroll with the rest: it is the pane's own name and the
  way out, and a pane you cannot shut because you scrolled its cross away
  would be worse than one you cannot reach the bottom of. The wheel over a
  pane moves that pane rather than the document, and a bar down its edge says
  how much there is and where in it we are — but only when there is more than
  there is room for, since a bar beside a pane that fits is a bar saying there
  is something below when there is not.
  *Proven by:* eight tests. A pane that fits cannot be scrolled and is given
  no bar; one that does not scrolls as far as what is left below and no
  further, either way; a pane that shrinks pulls the view back with it; the
  bar appears only when it is wanted and moves down as the pane does, measured
  in pixels rather than looked at. And three at the other end: thirty people
  on an exceptions list in a four-hundred-pixel window overflows and scrolls,
  an empty pane in a tall window does not move, and the wheel over the pane
  leaves the document where it was.
  *Not done, and named here:* the **Print** page does not scroll either. It is
  not a pane down the side — it fills the window, and its settings are laid
  out by a different hand — so it is a different piece of work rather than one
  more caller of this one.
- [x] **J33. All, Recommended Minimum and None.**
  **Two of them are a loop; the third is the interesting one.** All ticks
  every style and None ticks none, and writing those took an afternoon of
  nothing. Recommended Minimum is the one worth having: it is the set the
  document itself puts forward as the styles it is meant to be written with,
  and the format records it — `w:qFormat` on each definition, and the same
  attribute on each `w:lsdException` for the hundreds a `styles.xml` mentions
  without defining. So the answer is read rather than invented: a document
  made from somebody's template gets that template's answer, and this program
  has never seen that template.
  A document that marks nothing at all gets the headings, Title and Normal,
  because a button that ticked nothing would be None under another name. That
  is a guess, it is the only one here, and it is made only where there is
  nothing to read.
  **Ticking every style is not a limit.** Pressing All turns the box above
  off, because a document claiming a formatting restriction that permits all
  three hundred styles is claiming a restriction that restricts nothing;
  pressing either of the other two turns it on, because somebody who has just
  said which styles they want should not have the answer thrown away by an
  unticked box. The dialog stays up afterwards — the buttons are a way of
  filling the list in, not a way of leaving — and the status line says how
  many styles are allowed, so that the difference between three and three
  hundred can be seen without counting ticks.
  *Proven by:* three tests. Each button ticks what it names — None exactly
  none, All exactly as many as the document has, Recommended exactly as many
  as the document recommends, never none and never more than there are; every
  recommended style is one the document has or mentions, and every style the
  document marks comes back; and All leaves the limit off while the other two
  turn it on.
  *Not done, and named here:* the recommended set can be read and not
  written. Word's Manage Styles has a Recommend tab where the marks
  themselves are moved about, and nothing here writes `w:qFormat` — a
  document's own idea of what it is for survives a round trip untouched, but
  cannot yet be changed from inside this program.
- [ ] **J34. A signature is only as good as what it covers.** The package's
  parts are checked against the first `Manifest` in the whole signature,
  signed or not: an unsigned `<Object><Manifest/></Object>` put before the
  real one, with the real `SignedInfo` and signature value left as they
  are, passes every part — a changed `document.xml` or `vbaProject.bin` is
  reported Good, and **L6**'s trust lets the macros run. And a trusted
  publisher is matched by the certificate's Subject string, which anyone
  can put on a certificate of their own. Take part references only from
  manifests inside the objects that were verified, refuse ambiguous
  identifiers, and trust a fingerprint or a public key, showing the Subject
  only. Reviews R01, R02.

## K — Proving it against Word rather than against ourselves

- [x] **K1. A corpus of real documents.**
  **The files cannot be committed, so the harness is a command and not a
  test.** A Word file carries personal data inside itself — the author's name,
  how long it was edited for, comments, tracked changes somebody accepted
  without removing — and a public repository is the wrong place for any of it.
  So the corpus lives in a directory git ignores and every machine has its
  own, and `cargo test` on a fresh clone would have nothing to run. A suite
  that passes because it found no files is a suite that says nothing. What the
  suite tests instead is the harness: that it finds documents and not
  everything else in the directory, that a file it cannot open is reported
  rather than skipped, and that it can tell the answers below apart.
  **"Byte for byte" is two questions wearing one coat.** A document can come
  back with every part identical and still not be the same file, because the
  zip around the parts has an order, a compression choice and a timestamp for
  each entry, and reproducing those is a different job from reproducing the
  XML. So there are three answers and not two — **identical**, every byte;
  **repackaged**, every part but not the wrapping; **differs**, and which
  parts — and a fourth for the formats that are converted on the way in. An
  old `.doc` or a `.rtf` saves as a package, which is not what it was, so the
  byte question cannot be asked of it at all; that it opened and could be
  written back is all that is reported, and nothing more is claimed.
  **A failure is a bug; a difference is a measurement.** A document that will
  not open, or opens and will not save, ends the command unhappily. A
  difference does not, because a harness that failed on those would be one
  nobody could run. The report ends with which parts differ across how many
  documents, most first, which is the number that says where the next piece of
  work is.
  *Done:* `./x.sh corpus` and `.\x.ps1 corpus`, over `./corpus` or a directory
  named after it. Also, quietly: the lock file Word leaves beside an open
  document — `~$name.docx`, a hundred and sixty bytes saying who has it —
  looks exactly like a document and is skipped, as are hidden files; documents
  filed in folders are found; and opening bytes is now a separate thing from
  reading a file, because the harness needs to keep the original to compare
  against.
  *Proven by:* eight tests. A document this program wrote comes back
  identical; a README, a note, a hidden file and a lock file are left alone
  while a document in a folder is found; a file that is not a document at all
  is reported as failed and says why; a Rich Text file is opened and not asked
  the byte question; the same parts in a different order are repackaged and
  not differing; a changed part is named while the rest are counted; an empty
  corpus says what to put in it rather than printing a zero; and the counts
  add up to the number of documents with the parts rolled up across them.
  *Found along the way:* a needless `mut` in the layout engine, dating from
  **J27**, which clippy rejects in either profile when it is asked afresh. It
  came out only because this item wanted a release build. Why the checks
  between J27 and here did not show it I do not know, and would rather write
  that down than invent a mechanism: a clean check is worth what it was
  actually run on.
  *Not done, and named here:* the corpus is empty until somebody fills it, so
  what this proves today is that the harness works and not that the documents
  do. Templates from before 2007 — `.dot` — are not in the list of kinds,
  because nothing here has been shown one.
- [x] **K2. Page images compared against Word's.**
  **A score, not a pass.** Two renderings of the same page are never
  identical: fonts are hinted, edges are softened, a letter lands a third of a
  pixel to the left. A comparison that demanded equality would fail on every
  document forever and tell nobody anything. So each page is reduced to ink —
  how dark each pixel is, white being none — and two pages are scored by how
  much of their ink falls in the same places: what they agree on over what
  either of them has. Identical pages score one, a blank page against a
  written one scores nothing, and everything real is in between.
  **Two numbers, because one of them is a lie on its own.** The exact score
  asks whether the ink lands on the same pixel, which a page drawn a pixel low
  fails while being perfectly right. The tolerant score asks the same question
  in squares four pixels across. It was a softened comparison first, and that
  did not work: blurring both pages moves the disagreement onto the softened
  edges and the ratio comes out where it started — 0.71 before and 0.71 after,
  which is what sent me looking for a different question rather than a wider
  brush. A coarser question forgives a letter a pixel out, because it is still
  in the same square, and does not forgive one in the wrong place or missing.
  **Pages are drawn at the reference's own resolution, rounded to a whole
  number of dots to the inch.** Working it back from how wide the reference
  image is gets within a fiftieth of a dot, and a fiftieth of a dot is enough
  to move a word onto the next line and everything after it down the page: a
  page held against a picture of itself scored 0.67 until the number was
  rounded, and 1.00 after. That is worth knowing for its own sake — this
  program's line breaking depends on the resolution it is drawn at, so a
  document is not one layout but one per resolution. Which of them matches
  Word is a question this harness can now be asked and has not been.
  **A document nobody has exported pages for is not a zero.** It is left out
  of the average and said so, because everybody's corpus will be mostly that,
  and scoring the unmeasured at nothing would drag the number to
  meaninglessness and hide the documents that really are drawn badly.
  *Done:* `./x.sh fidelity` and `.\x.ps1 fidelity`. Word's pages go in
  `corpus/reference/<document>/` as pictures named by page number, in any
  format this program reads. Every run appends to `corpus/fidelity.log` —
  when, which commit, how many documents and pages, both scores — and the
  report says which way it has moved since the run before. The commit is read
  out of `.git` by hand, through `HEAD`, the branch's own ref and
  `packed-refs`, rather than shelling out to a program the container may not
  have.
  *Proven by:* twelve tests. A page against itself agrees completely and two
  blank pages are not a disagreement about nothing; a blank page where there
  should be writing scores zero; a page a pixel out is forgiven by the coarse
  look and not by the exact one; the same page at two sizes scores the same,
  so the exporter's resolution is not what is being measured; ink is read over
  white however the picture was saved, so a page with an alpha channel and one
  without are the same page; images are paired with pages by the number in
  their names, so page ten comes after page nine; a page that was never laid
  out costs exactly the ink it holds; a document with no reference is not
  counted; the history line says when, what and how much, and the report says
  which way it moved; the commit is found whether its ref is a file or packed
  away, and a directory that is not a repository says so; and, end to end, a
  document measured against pictures of its own pages comes back at 100%,
  which exercises the walk, the pairing, the resolution and the rendering.
  *Not done, and named here:* getting the pages out of Word is by hand —
  export a PDF and turn its pages into images, or print to an image printer.
  Nothing here automates it, and on a machine with no Word there is nothing to
  automate. The comparison is of ink and not of colour, so a page whose text
  is the right shape in the wrong colour scores nearly full marks; that is a
  second measurement rather than a change to this one.
- [x] **K3. The Unicode conformance suites.**
  **The decision this was waiting on, made: data of that kind comes from
  whoever wants it, and lives in a directory git ignores.** The six files are
  test data rather than character data, so Perl's database in the build image
  does not carry them, and the build downloads nothing and installs nothing —
  which is the rule the whole project is built under and not a thing to bend
  for a test. So `unicode/` holds them, ignored but for a README saying which
  files and where from, exactly as `corpus/` holds real documents in **K1**.
  What the repository carries is everything else: the parsers, the runners,
  and tests of both written against fixtures of a few lines that the tests
  type out themselves. The harness is proven without the data; the data proves
  the engine.
  **A suite whose file is not there is missing — not passed, and not failed.**
  A fresh clone has six missing files, and a command that called that nought
  per cent would be lying about the engine. Nor is a line the parser cannot
  read counted as a failing case: that is a hole in this program's reading of
  the file rather than in the engine, and rolling the two together would hide
  whichever of them is smaller.
  **A number, not a gate.** `LineBreakTest.txt` will fail by the thousand,
  because line breaking here keeps seventeen classes where the standard has
  about forty and folds the rest — which is **E16**, written down, and a red
  build every morning would only say it again. What is wanted is the count,
  the first few lines that produce it, and the direction it moves in.
  `BidiTest.txt` is written in classes rather than characters, so each class
  is given a character to stand for it — `L` an A, `R` an aleph, `AN` an
  Arabic-Indic zero — which is how an engine that works on text runs a suite
  written about classes.
  *Done:* `./x.sh conformance` and `.\x.ps1 conformance`. Every run appends to
  `unicode/conformance.log` and the report says which way the number went
  since the run before, as **K2**'s does.
  *Proven by:* ten tests. Six missing files are reported missing and said
  where to get them; the three break suites are read and run, with the
  outermost marks dropped for line breaking because a break at either end of a
  line moves nothing; a case the engine gets wrong is counted and shown with
  its line number, so that a hundred per cent means something; a line the
  parser cannot read is counted apart; NFC and NFD are run six ways a row, as
  the file's own rule says; the bidirectional algorithm is run over real text
  and over classes; a class with no character to stand for it is not silently
  passed; and the history line says when, what and how much.
  *Found along the way:* seven grapheme cases typed out by hand to try the
  command with, and the engine matched all seven — the one that appeared to
  fail was my own expectation, since in Unicode 14 a virama is `Extend`, so
  `KA` and the virama are one cluster and the `TA` after it begins the next.
  Which is the argument for the whole item: a fixture typed by somebody who
  believes he knows the answer is worth less than the file the Consortium
  publishes.
  *Not done, and named here:* the files are not here, so the numbers are not
  either — what this proves today is the harness, the same as **K1** and
  **K2**. The version taken has to be the one the tables were generated from,
  Unicode 14.0.0; holding tables built from one version to another version's
  answers measures the gap between versions and calls it a bug, and the README
  says so. The `NFKC` and `NFKD` columns are read past, because compatibility
  normalization changes what the text says and nothing here asks for it.
  **E14** wants pattern data and is still open, but it is now the only one of
  these left without an answer to where data comes from: this is the shape.
- [ ] **K4. Word on this machine.** A copy of Word is installed here, without
  a licence: it opens and prints but does not edit, and its COM automation
  answers — `Documents.Open` and `ExportAsFixedFormat` were tried on
  `dist/demo.docx` and gave a two-page PDF. That is two questions **K1**
  and **K2** could not ask: does Word open a file this program wrote
  without offering to repair it, and what does Word's page look like. A
  script on the host (Docker cannot see Word) that takes a directory of
  documents, opens each in Word, records repair prompts and errors, prints
  to PDF, and hands the pages to **K2**'s comparison; a `WINWORD` that
  outlives `Quit` is ended by its PID. Run against what **G19**–**G21**
  write, and against the corpus. Not a test in `cargo test`: the machine
  with Word is not the build image.

## L — Visual Basic for Applications

Word runs the macros a document carries, once the person opening it has said
so. This program keeps them and does not run them; **J7** is that half, and
this section is the other. The order matters, because each of these is worth
having on its own and none of them is worth having before the one above it.

The thing being copied is not only the language. A macro that runs and does
something slightly different from what Word does is worse than a macro that
does not run, because nobody can tell which they have got; so every item here
is finished when what it does matches Word on documents that were written for
Word, and each names the tests that show it.

And nothing here changes what happens when a file is opened: **L6** is the
gate, it is built before anything is allowed to run, and until it exists the
interpreter is reachable only from the macro list a person opened themselves.

- [x] **L1. The project inside `vbaProject.bin`, read.**
  **The compression is the whole of the format's difficulty, and it is a
  page.** [MS-OVBA] §2.4.1: a signature byte, then chunks, then groups of
  eight tokens with a flag byte in front saying which of the eight are
  literals and which are copies. The one part anybody gets wrong is that a
  copy's offset field *grows as the chunk fills* — four bits while sixteen
  bytes have been written, twelve by the end — so a reader that fixes the
  width reads every copy in the file at the wrong place. A copy may also
  reach into what it is writing, which is how a run of one byte is written
  down, so the copying is a byte at a time and not a block move.
  **There is a compressor here as well, because a reader nothing can make a
  file for is a reader nobody has tested.** The repository holds no Word file
  — the corpus is every machine's own, **K1** — so `wp_vba::example` builds a
  project the way Word builds one, and both this crate's tests and the
  program's are handed one of those. It is also half of what editing a macro
  will need, which is **L5**.
  **The `dir` stream is walked by length rather than by understanding.**
  Every record is an identifier, a length and that many bytes, with exactly
  one exception — the project's version, whose length field does not count
  the version — so a project full of references to libraries nobody here has
  heard of is stepped over record by record instead of choking on the first
  one. That is what makes this survive files it was not written against.
  **In front of a module's text is a cache Word keeps for its own editor**,
  and the `MODULEOFFSET` record says how far. This reads past it and leaves
  it alone; a test puts something that is plainly not text in that cache and
  checks it does not come back as source. Names are written twice, once in
  the project's code page and once in UTF-16, and the second is taken where
  it is there, because it cannot be wrong about a character the code page has
  no room for. Which module belongs to the document itself is not in the
  `dir` stream at all — it says only procedural or not — so the `PROJECT`
  stream beside it, which names `ThisDocument`, is what tells a class and the
  document's own module apart.
  **A macro's name is found the way somebody scanning a listing finds it**:
  the modifiers a declaration may carry, then the word that says what it is,
  then the name. Enough to list them, and deliberately not enough to run
  them — the parser is **L2** and a far larger thing. What the list offers is
  what Word's offers: a public `Sub` that wants no arguments, because a
  private one is the module's own business and a function wanting an argument
  cannot be run from a list at all. It says **Show** where Word says Run,
  since running one is **L3** and a line that said Run and did not would be
  worse than no line.
  **A box that shows eight lines of a forty-line module and cannot reach the
  ninth is hiding what somebody asked to see.** So the dialog's box of lines
  now takes the keyboard and scrolls: the arrows a line, Page a boxful, Home
  and End the ends, and it opens at the line that declares the macro chosen,
  a third of the way down so that what is above it is in sight too. It is the
  one list in a dialog with a bar down its edge, because the others are
  picked from and the row they have picked says where in them a person is,
  while a box of lines has no picked row and would say nothing at all. The
  File Conversion preview, which is the other box of lines, got the scrolling
  with it.
  *Proven by:* seventeen tests. Fourteen in `wp-vba`: a stream written out by
  hand byte by byte reads as it was meant to, which is the one case that does
  not agree with this program by construction; what is compressed comes back,
  over one chunk and over four; the offset field's width at four sizes; a
  copy reaching before its chunk is refused; a project is read out of the
  bytes of the part; a module's text is read past the cache; the procedures
  are found where they are declared, with their line numbers; the list is
  what Word would offer to run; a comment that looks like a declaration is
  not one; a property is three words and not two; the document's own module
  is told from a class; and a part that is not a project says so. Three in
  the program: a document's macros are listed by module and name and the
  private one is not offered, choosing one shows the whole module opened at
  the right line, and the box reaches the lines below the ones it shows.
  A picture of the dialog, and `--picture … tab=view macros` is how to look
  at it again.
  *Found along the way:* two methods nothing used and a needless `mut`, none
  of which `cargo clippy --workspace --all-targets` reported and all of which
  a plain `cargo build --workspace` did. A method used only by tests is used
  as far as `--all-targets` is concerned, so the ordinary build of the
  program is a check of its own and is now run as one. Both dead methods were
  **J32**'s: one is `#[cfg(test)]` now and the other is gone.
  *Not done, and named here:* nothing is run, and nothing here is a step
  towards running anything on opening a file — **L6** is that gate and it is
  built before the interpreter is reachable at all. The references a project
  declares to other libraries are stepped over rather than listed. Forms are
  not read: a `UserForm` is a storage of its own and is **L7**. The source
  cannot be changed, which is **L5**. And a module written in a code page
  this program has no table for is read as Windows-1252, which is a guess and
  is made only where there is nothing better to use.
- [x] **L2. The language: reading it.**
  **A tree of the words themselves, not of named fields.** Every leaf is a
  token exactly as it was written, and every branch says what the words under
  it are, so writing the tree back out is walking it and putting each token
  down again with the spaces that came before it. That is the shape this
  needed and not the shape an interpreter will want, because the first thing
  that has to be provable is that nothing was lost: a tree of named fields
  can be built from this one, and a file cannot be built back from that one.
  **The spaces are kept, and one of them decides a meaning.** A type suffix
  and an operator are the same characters — `a&` is a Long and `a & b` joins
  two strings — and what tells them apart is that a suffix is written against
  its name and an operator beside it. The first version read `a & b` as a
  Long called `a` followed by a stray `b`, and the test that caught it is the
  reason the rule is now written down where the suffix is read. Three other
  things need looking ahead at: the underscore that continues a line, which
  belongs to the *next* token because the statement carries on; a `#`, which
  opens a date only when another one closes it on the same line, and is
  otherwise `#If` or a file number; and `&`, which begins a hexadecimal
  number when `H` or `O` follows it.
  **The left of an assignment is not an expression.** In Visual Basic `=` is
  the comparison as well, so reading a whole expression first swallows the
  assignment and leaves a line that compares two things and throws the answer
  away — which is exactly what the first version did, to every assignment in
  the test module. What may stand on the left is a name and whatever follows
  it: brackets, full stops, a bang.
  **A one-line `If` is three statements on one line.** `If a Then b = 1 Else
  b = 2` ends its first statement at the `Else`, which nothing else in the
  language does, so the reader carries a flag saying that an `Else` would end
  the statement being read — and it is a flag rather than a rule, because
  everywhere else `Else` begins a line of its own.
  **Fourteen levels of precedence, one function apiece, in the order they
  bind.** Written out rather than driven from a table of numbers, so that the
  precedence can be read off the file: `Imp`, `Eqv`, `Xor`, `Or`, `And`,
  `Not`, the comparisons, `&`, `+` and `-`, `Mod`, `\`, `*` and `/`, the
  minus in front of a thing, and `^`. The three worth knowing: a comparison
  binds tighter than `Not`, so `Not a = b` asks whether `a = b` is false;
  `Not` binds tighter than `And`; and a power binds tighter than the minus in
  front of it, so minus two squared is minus four.
  **A line nobody here understands is kept and complained about.** It becomes
  a branch holding its own words, so the module still comes back byte for
  byte, and a complaint goes on the list with its line number and what was
  expected, in the words Word's own editor uses: "Expected: End Sub". A
  parser that quietly swallowed what it did not know would report every
  module as read and be wrong about half of them — and the swallowing would
  be invisible, because the round trip would still pass.
  *Done:* `./x.sh vba` and `.\x.ps1 vba` read every module of every document
  in the corpus and ask the two questions of each: was it understood, and did
  it come back as it went in. The second is the stronger one — a parser can
  understand a line and still lose the tab in front of it — and the command
  ends unhappily on either, because both are this program's fault. In the
  program itself, the Macros dialog now says `Compile error: line 12:
  Expected: End Sub` where Word's editor would say it, and **L1**'s
  line-by-line scan for what looks like a procedure is gone: there is one way
  of finding a procedure now, and it is reading the module.
  *Proven by:* twenty-four tests. Seven of the lexer: what goes in comes out,
  for seven kinds of line; a continuation is not the end of a statement and a
  continued line still counts as a line; two quotes in a row are one quote;
  a hash is a date only when one closes it; numbers with their exponents and
  suffixes; a comment however it began, and a name that merely starts with
  `Rem`; and the case a word was written in is kept and not compared.
  Thirteen of the parser: a module using every form this item names — and a
  few besides — is read with no complaints, comes back byte for byte, and has
  nothing kept unread in it; what binds tighter than what, in seven pairs; a
  call without brackets; a one-line `If`; a full stop that begins an
  expression inside a `With`; a label and a line number; the compiler's own
  `#If` lines, whose insides are read as ordinary Visual Basic; four kinds of
  file statement, and a name that happens to be spelled `Get`; a syntax error
  that names its line; a block left open; a line nobody understands, after
  which the next line is still read; and the empty module. Three of the
  command, and one of the dialog that shows what could not be read.
  *Not done, and named here:* the corpus is empty on this machine, so what is
  proven today is the parser against what this program can write, and not
  against what people wrote — which is the whole reason the command exists
  and **K1** is the shape it follows. `#If` is read and not obeyed: what is
  inside one is parsed whether or not this build would have compiled it,
  because this is a reader and not a compiler. `Declare`, `Event`,
  `Implements`, `Attribute` and `Option` are kept as the words they are
  written with rather than taken apart, since nothing yet asks what is in
  them and taking them apart would be inventing a shape nobody reads.
  Nothing here checks that a name exists or that two types agree — that is
  not reading — and nothing runs a line of it, which is **L3**.
- [x] **L3. The language: running it.**
  **Most of Visual Basic is the Variant, so most of this item is the rules
  for one.** A macro rarely says what kind of thing a variable holds, so
  nearly every value turns into whatever the next operator needs, and the
  turnings are not obvious: `"3" + 4` is seven and `"3" & 4` is "34"; `Empty`
  is nought to a sum and "" to a join; `Null` swallows whatever it touches
  except a join, where it is nothing; `True` is minus one as a number; `/`
  always gives a Double and `\` and `Mod` round both sides first, which is
  why `7.6 Mod 3` is two. Currency is a whole number of ten-thousandths, so a
  tenth added a hundred times is exactly ten where a Double drifts. A date is
  a count of days from the thirtieth of December 1899 — a day chosen to agree
  with a spreadsheet that believed 1900 was a leap year, and kept ever since
  by every program that reads those files. All of it is written out once,
  with the reason beside each rule, rather than scattered through the
  interpreter.
  **The interpreter walks the tree the parser made.** There is no second tree
  and no byte code. That costs a little speed and buys the thing that
  matters: what runs is what the file says, and every value carries the line
  its words came from.
  **Arguments are passed back as well as in.** Visual Basic passes by
  reference unless told otherwise, so a procedure that changes what it was
  given has changed the caller's variable. That is done by remembering where
  each argument came from and writing the parameter back into it afterwards —
  and the first version wrote it into a frame it had just made, which is to
  say nowhere. The test that passes a variable to a `Sub` that doubles it is
  the one that caught it.
  **A macro that never stops is stopped.** There is a budget of statements,
  and running out of it is an error like any other, because a program that
  hangs the window it is running in is worse than one that says it gave up.
  The budget is spent at every statement *and* at every turn of every loop:
  `Do` and `Loop` on two lines with nothing between them runs no statements
  at all, and the first run of the suite hung on exactly that.
  **Running the language found two things the parser had got away with.** The
  condition on `Loop Until i >= 3` was being kept as four separate words,
  because the helper that closes a block takes the rest of the line as words
  — which round-trips perfectly and can be asked nothing at all, so the loop
  had no condition and never ended. And `For Input` in `Open … For Input As
  #1` is an ordinary name as far as the parser is concerned, so the mode was
  never read and every file was opened for writing. Both were invisible to a
  parser test and obvious to the first macro that used them, which is the
  argument for this item following that one.
  **Where it can be reached from, and only from there.** The macro list a
  person opened themselves: found the macro, pressed Run. Opening a document
  runs nothing at all, and every other way in waits for **L6**. What a macro
  showed is shown after it has finished rather than in the middle, because a
  message box put up while it runs would have to stop it to wait for an
  answer, and stopping in the middle is the debugger, which is **L5**. The
  dialog says so on its face: *Run reaches the language and not the document
  yet*, and a macro that touches the document stops on the first thing it
  touches and says which.
  *Proven by:* fifty-six tests. Twenty-seven macros with known answers: a
  function that answers with its own name; by reference and by value, side by
  side; recursion; five kinds of loop counting the same way; `Exit For`
  leaving only the loop it is in; arrays counted from where they were
  declared; `ReDim Preserve` keeping what was there by subscript and not by
  position; `Erase` leaving the shape and emptying the cells; `For Each` over
  what `Split` made; `Select Case` with lists, ranges and `Is`; the Variant
  rules from inside a macro; a variable of a declared kind holding that kind;
  `Static` remembering between calls; a module-level variable outliving one;
  constants and `Enum`; `On Error Resume Next` with `Err.Number` afterwards;
  `On Error GoTo` with `Resume Next` coming back; an error nobody watched for
  stopping the macro; `GoTo` and a label; `Option Explicit` refusing an
  undeclared name; what a macro shows going to the program and not the
  language; the library reached from a macro; a macro that never stops being
  stopped; a file written and read back; `Like` and its four patterns; a
  macro that is not there saying so; and a line that never parsed refusing to
  run. Eleven of the Variant's own arithmetic, seven of the dates, nine of
  the library — including one that walks the list of names against the
  functions, so the two cannot drift apart — and two of the button.
  *Not done, and named here:* **objects**. `New`, `TypeOf`, a `With` block's
  full stop, and every name that belongs to the document are refused with the
  reason, which is that the object model is **L4**. A `Type … End Type`
  parses and a variable of one does not run. `GoSub` and `Return` are refused
  by name: nothing written since procedures had names uses them.
  `Option Compare Text` is not honoured — comparisons are by the letters'
  own order. `Declare` cannot call into the system and there is no
  `CreateObject`, which is a decision rather than a gap: a macro that can
  reach the system through this program has made this program the way in.
  `Format` covers the named pictures and the ones made of `0`, `#`, a point,
  commas and the date letters, not every picture there is. `MsgBox` always
  answers OK, because nothing can ask yet.
  *And the honest part of "checked against what Word gives":* there is no
  Word on this machine to ask. Every answer here is the one the language
  documents, and each test says which rule it is showing rather than
  asserting a number nobody can check. Holding these against Word's own
  answers is what the corpus is for, and it is **K1**'s shape again: the
  macros that would settle it cannot be committed.
- [x] **L4. The object model: the document.**
  **This is the part a macro is actually written against.** Nobody writes
  `Left(s, 5)` in a Word macro for its own sake; they write
  `Selection.TypeText`, `ActiveDocument.Paragraphs(1).Range.Text`,
  `.Font.Bold = True`. **L3** was the floor and this is the building.
  **A property this program cannot answer says so and stops.** That is the
  rule the whole thing is written under and the difference between a program
  a macro can be trusted with and one it cannot. Word has some thousands of
  properties; this has the ones below. Every other one is refused *by name* —
  "Selection.Shading is not something this program does yet" — because a
  macro that reads a property and is quietly handed `Empty` carries on and
  writes the wrong thing into the document, and nobody afterwards can tell
  which property it was.
  **What is modelled:** `Application` (its name, its version, and the way
  through to the rest), `Documents`, the document itself (name, full name,
  path, saved, `Content`, `Range(a, b)`, `Save`), `Selection` and `Range`
  (text read and written, start and end, bold, italic, underline, style,
  `Font` with its name and size, `InsertBefore`, `InsertAfter`, `Delete`,
  `Select`, `Collapse`, `TypeText`, `TypeParagraph`, `Find`), `Paragraphs`
  and a paragraph (count, one of them, adding one, its range, its style, its
  alignment), `Styles`, `Bookmarks` (count, one by name or number, `Exists`,
  `Add`), `Comments`, `Tables` (how many, and how many rows and columns each
  has), and `Find.Execute` with the names Word writes it with.
  **Where a range is.** Word counts characters from the start of the
  document with every paragraph mark counting as one; this program counts
  paragraphs and offsets inside them. The two are translated at the edge, so
  a macro doing arithmetic on `.Start` and `.End` gets the numbers it expects
  — the second paragraph of a document beginning "one" starts at four.
  **Editing goes through the document's own editing**, with the selection
  moved to where the macro is working and put back afterwards. That is worth
  saying out loud rather than hiding: it means everything a macro does is
  something a person could have done by hand, and that a macro's changes go
  onto the same undo stack as anybody else's.
  **Two brackets, two meanings.** `ActiveDocument.Paragraphs(2)` is the
  second paragraph and `ActiveDocument.Range(0, 5)` is a method with two
  arguments — the same syntax for a collection being indexed and a method
  being called. Word tells them apart by its type library; here a member that
  hands back a collection hands the brackets on to it, which is the same
  answer without the library.
  **Named arguments, because that is how the lines are written.**
  `Selection.Find.Execute FindText:="a", ReplaceWith:="b",
  Replace:=wdReplaceAll` is how every real macro writes that line, so an
  argument now carries the name the macro gave it from the parser through the
  interpreter to the model — and to a macro's own procedures, which take
  theirs by name too. Word's own named numbers came with it: `wdReplaceAll`
  and its two dozen companions, because a macro that wrote one and was handed
  nothing would replace one thing instead of all of them and look as though
  it had half worked.
  **Nothing in the model is translated.** The names are the language's: a
  macro says `Selection` in every country, and a catalogue offering those
  words for translation would be offering to break every macro there is. The
  message list leaves that module alone, and the reason is written where the
  skipping happens.
  *Proven by:* nineteen tests. Eleven of the language's side, against a toy
  object model of three things — so that objects can be proved without a
  document, and the document's side without the language's: a name the
  program answers to; a collection in brackets; a member written to as well
  as read; `With` holding an object and a full stop meaning it; `For Each`
  over a collection the program owns; a named argument; an object standing
  for what it says where a string is wanted; `Set` and `Is`; a member nothing
  has, named rather than guessed at; a full stop outside any `With`; and a
  name the program does not answer to. Eight against a real document: a
  macro types into it the way a person would; reads its paragraphs and their
  text; walks them with `For Each` and changes one; sets a style that sticks;
  finds and replaces with named arguments and `wdReplaceAll`; turns bold on
  through `Font` and reads it back; counts characters the way Word counts
  them; and a property nobody modelled stops the macro and names itself,
  leaving what the macro had already done done and what came after it undone.
  *Not done, and named here:* **the corpus half of the "done when"**. Running
  other people's macros over a directory of their documents is exactly what
  **L6** is the gate for, and building it before the gate would be building
  the thing the gate exists to prevent. What works today is the one way in
  the block's own words allow: a person opens the macro list, finds the
  macro, presses Run. Sections, headers, footers, footnotes, fields and
  content controls are named in this item and are not reachable yet; tables
  answer how many rows and columns they have and not what is in a cell. And
  `Application.Name` says this program's own name rather than "Microsoft
  Word": a macro that asks is told the truth, and one that branches on the
  answer takes the other branch. That is a decision and not an oversight —
  the alternative is a program that lies about what it is, and every later
  difference then looks like a bug in Word.
- [x] **L5. The editor and the debugger.**
  **Stopping a macro in the middle is the whole of this item**, and an
  interpreter that walks a tree with ordinary function calls cannot be
  stopped in the middle — not without rewriting it as a machine with a stack
  of its own, or putting it somewhere it can wait. It is put somewhere it can
  wait: its own thread. The macro runs there, the document stays here, and
  every time the macro wants something of the document it asks down a channel
  and waits for the answer. Stopping is then not a mechanism at all — it is
  *the window not answering yet*. Press F5 and the answer goes back and the
  macro carries on from the statement it was about to run.
  That is also why it is safe. The macro's thread never touches the document:
  it holds the program and its own variables, and everything else is a
  question. What crosses the channel is values and handles — numbers, strings
  and which object — and the window is the only thing that ever holds the
  document. There is no lock to forget and nothing shared to race over. The
  language's whole side of it is one method: *before this statement, on this
  line, may I go on?*
  **The editor is a page and not a window.** Word opens a second application
  window with its own taskbar button; this program draws one window and
  everything in it, so the editor fills it, the way the Print page does and
  for the same reason. The project is down the left, a module at a time in
  the middle, and along the bottom the two things a debugger is for: the
  Immediate window, where a line is typed and answered, and the Watch, where
  what the stopped procedure can see is listed. A red dot in the margin is a
  breakpoint and the line it stopped on is shaded.
  **A breakpoint goes on a line the macro can reach.** Put one on a blank
  line or on a comment and it moves down to the next statement, because a
  breakpoint the macro would never reach is a breakpoint that does nothing,
  and Word moves it for the same reason.
  **What is typed is written back into the document's own project.** A module
  is a stream inside a compound file inside the document: a cache Word keeps
  for its own editor, and then the text, compressed. Writing an edited module
  back keeps every other byte exactly where it was — the cache, the `dir`
  stream that says where the text begins, the other modules, the project's
  own settings — and puts newly compressed text after the cache. A document
  that had no project at all becomes macro-enabled by it, because a `.docx`
  carrying macros is a file Word refuses to open and this program must not
  write one.
  *Done:* Alt+F11 opens it, and so does the last line of the Macros list. F5
  runs the procedure the caret is in; F5 again while it is stopped carries on;
  F8 runs a statement and stops again; F9 marks a line; Shift+F5 stops a
  macro where it stands; Escape and the cross shut the editor; Ctrl+S writes
  the project back without shutting it. The Immediate window answers `?1 + 1`
  with 2 and runs `Selection.TypeText "x"` on the document itself.
  *Proven by:* eleven tests. It opens on the project the document carries;
  what is typed goes in where the caret is, and Backspace joins two lines
  again; what was typed is written back, with the module nobody touched
  unchanged and the document marked as changed; F5 runs the procedure the
  caret is in and its message comes back; a breakpoint stops it on the right
  line with the variables listed, and F5 carries on to the end; F8 goes one
  statement at a time and the line it stops on moves; a breakpoint asked for
  on a blank line lands on the next statement; the Immediate window answers a
  question and runs a line; a line that will not read says so rather than
  running something else; the cross and Escape both shut it; and the line
  numbers of a procedure are what say which one F5 would run. A picture of
  the editor stopped on a breakpoint, with the watch beside it.
  *Not done, and named here:* the code is drawn in the window's own font
  rather than a monospaced one, because the chrome has one font and picking
  another is a change to the layout engine rather than to this. There is no
  selection in the code window: typing, the arrows, Home, End, Page and the
  mouse put the caret where it belongs, and Word's copy and paste inside the
  editor is not there. A module can be edited and one cannot yet be added,
  renamed or removed, and the project tree shows modules and not the classes,
  forms and references beside them. The Watch shows what the stopped
  procedure can see rather than what somebody asked to watch, and names in it
  are lower case, because that is how the interpreter keeps them. And `Stop`
  in a macro ends it rather than dropping into the debugger, which is what it
  does in Word; the breakpoint is the way in here.
- [x] **L6. The trust that gates all of it.**
  **A document can carry a program, and that is the whole difficulty.**
  Opening a file somebody sent is not supposed to be a decision, and a file
  that carries a program makes it one. The answer — Word's and this one — is
  that nothing a document carries runs until somebody says so, and that
  there is exactly one place where "somebody said so" is decided. Every way
  of running a macro asks it first: the Run button on the macro dialog, F5
  and F8 in the Visual Basic editor, and a line typed into the Immediate
  window. Nothing runs when a document is opened, and there is no other
  route in. When the events a document can raise arrive — **L7** — they will
  ask the same question, because there is only one to ask.
  **Four settings, which are Word's four.** Disabled without notification;
  disabled with the bar across the top that offers to enable them, which is
  Word's default and this program's; only the macros of a document signed by
  somebody trusted; and everything, which is the setting nobody should
  have. They live on a Trust Center page of the Options dialog, because they
  are about the person and not the document and follow them to the next one.
  Two lists say yes before the setting is asked: the folders whose documents
  are trusted — a folder trusts what is under it too, which is Word's own
  "subfolders" tick — and the people whose signatures are.
  **Enable Content is for this document, for as long as it is open.** The
  bar's button turns them on and nothing is written down; opening the same
  file tomorrow asks again, and opening another file now asks now. The bar is
  there only where there is something to offer: a document whose macros are
  already allowed has nothing to enable, and one whose macros are disabled
  without notification is not to be notified.
  **Where this differs from Word, said out loud.** Word checks the signature
  *on the project*: a macro project is signed separately from its document,
  and a trusted publisher's project runs inside a document nobody signed.
  This program checks the signature *on the document*, because that is the
  signature **J12** and **J24** taught it to read and to verify. A project
  signed the way Word signs one is a project this program does not yet know
  how to check, and under "only signed" it is refused rather than trusted.
  Trusting a folder trusts the folder the open document is in, from a button
  on the page; browsing for some other folder needs a folder picker the
  shell does not have yet.
  *Proven by:* nine tests. A document carrying `AutoOpen` does nothing at all
  when it is opened, which is the sentence this item is named after; by
  default the bar is there, running is refused with a sentence that says
  where to go, the bar's button is Enable Content, and pressing it is what
  turns them on — after which the same macro runs and the bar is gone;
  enabling is for this document only, and the next one is asked again; a
  document under a trusted folder is not asked; disabled without
  notification shows no bar and says why when a run is tried; enabling
  everything asks nothing; only-signed refuses an unsigned document, refuses
  a signed one until the signer is on the list, and allows it after — with a
  certificate OpenSSL made for the test and a signature this program's own
  code verified; F5 and the Immediate window are refused by the same
  sentence as the button; and the setting is remembered by a name that is
  not the one on the screen, falling back on asking for a name nobody wrote.
  A picture of the bar under the ribbon with the macro dialog in front of it.
  *Found along the way:* the ribbon page of the Options dialog already had a
  button called Remove, and a second button of the same name is one button
  as far as the dialog's answer is concerned; the Trust Center's is Remove
  Location. And a stash: this item was stopped halfway on request and put
  back a day later, which is what the numbering of the dialog's fields is
  for — the page had to move to the end, after the two pages of lists.
  *Not done, and named here:* a project's own signature, as above. Trusted
  publishers are read from the settings file and shown nowhere: there is no
  page to add one to or take one off, and the way onto the list is the file,
  because the dialog that Word offers — "trust all from this publisher" on
  the bar — wants the certificate's chain checked against the machine's own
  store first, which is **J24**'s work turned the other way round and is not
  a line of this item. Word's "Trust access to the VBA project object
  model" has nothing to gate yet.
- [x] **L7. Forms, and the rest of it.**
  **A project is every module at once.** Until now a macro ran in its own
  module and saw nothing else. Now the whole project is read together and a
  macro calls what another module offers — `Greet "World"`, or
  `Module2.Greet` by name — sees its `Public` variables and constants, and
  is refused its `Private` ones, which the module keeps to itself. Each
  module has its own module-level variables, so two modules that each
  `Dim Count` have two counts. A class module is a class: `New Counter` makes
  an object with the class's fields, `Class_Initialize` runs on it, its
  `Public` procedures and `Property Get`, `Let` and `Set` are its members,
  `Me` is itself, and a handle passed to a procedure is the same object on
  both sides, as a reference is. The language's own `Collection` is here
  too — `Add` with a key or a place, `Item` by number or key, `Count`,
  `Remove`, and `For Each` over it — because it is the one object every
  macro makes. `TypeName` names a class by its name and `IsObject` sees a
  handle.
  **A form is a window the macro designed, and it is read out of the file.**
  A `UserForm` is two things in the project: its code, a module like any
  other, and its design, a storage of its own beside the project written the
  way Office Forms writes every parent control — [MS-OFORMS], a stream for
  the form and its *sites* and a stream for the controls, each structure a
  version, a size, a property mask and the values the mask says are there,
  aligned to their own sizes. The reader walks the mask bit by bit, in the
  specification's order, taking what is needed to show the form and run its
  code — caption, size, and for each control its name, kind, place, size,
  caption or text, whether shown and enabled, its tab index and whether it
  is the button Enter or Escape presses — and stepping over the rest by the
  sizes the format gives. Labels, text boxes, check boxes, option buttons,
  toggle buttons, combo boxes, list boxes and command buttons are read as
  themselves. A writer of the same format sits beside the reader so that a
  project with a form can be built for a test and read back; and one test
  holds both to the specification by the bytes of a form with one button,
  worked out by hand from the spec's tables.
  **Showing one is the debugger's thread doing what it was built for.** A
  macro runs on its own thread and asks the window for everything (**L5**);
  `UserForm1.Show` is one more question, answered when something is done on
  the form. The window draws the form as the program draws its own dialogs —
  the same face, the same caption bar, four pixels to three points as Word
  shows it — at the places its author put the controls; typing goes into the
  box with the keyboard, Tab walks the tab order, Space ticks, Enter presses
  the default button and Escape the cancel button or shuts the form, a
  combo box drops open, a list is walked with the arrows. Each of those is
  sent back with what every control holds now, and the form's own code is
  run for it — `cmdOK_Click`, `txtWho_Change` — with the form as `Me`,
  `UserForm_Initialize` the first time the form is touched, `Activate` on
  each `Show`, `QueryClose` with a `Cancel` that is honoured when the cross
  is pressed, `Terminate` on `Unload`. The macro owns the form: the window
  is shown a copy and keeps only the caret and the focus between happenings.
  Controls answer `Text`, `Value`, `Caption`, `Visible`, `Enabled`, the
  four sides, `AddItem`, `RemoveItem`, `Clear`, `List`, `ListCount` and
  `ListIndex`, and take the ones that can be set.
  **And so is a message box.** `MsgBox` was a note collected and shown after
  the macro had finished, and `vbYesNo` always answered OK, which was a lie.
  Both are waited on now like a form: the box goes up with Word's buttons —
  `vbOKCancel`, `vbAbortRetryIgnore`, `vbYesNoCancel`, `vbYesNo`,
  `vbRetryCancel`, with `vbDefaultButton2` and `3` honoured — and the macro
  is told Word's number for the one pressed, `vbOK` to `vbNo`; Escape is the
  button that says no. `InputBox` is a line to type, and Cancel is nothing.
  The Run button on the Macros dialog runs a macro through the same thread
  as F5, so a form or a box put up from there waits for the person as it
  does in Word, and a macro stopped by a breakpoint from there opens the
  editor on the line.
  **The events a document raises, through the one gate.** Word has two
  names for one moment: the auto macro — `Sub AutoOpen` in any module, or
  `Main` in a module called `AutoOpen` — and the event procedure,
  `Document_Open` in `ThisDocument`; and `AutoNew`/`Document_New` and
  `AutoClose`/`Document_Close` beside them. Both run when the document is
  opened, made from a template, or closed, and both ask
  `macros_allowed` first — the question **L6** built and said these would
  ask. So an `AutoOpen` does nothing when a file from the post is opened,
  and runs when Enable Content is pressed, which is when Word runs it; a
  document in a trusted folder, or under the setting nobody should have,
  runs it on opening. The closing macros run before the question about
  saving, once per closing however many times the closing asks. A new
  document made from a template runs the *template's* `AutoNew`, and
  whether it may is the template's question — its folder, its signature —
  not the new document's, which has neither. A document's own module may
  answer for its content controls as well: `Document_ContentControlOnEnter`
  when the caret goes into one and `OnExit` when it leaves, with a `Cancel`
  that keeps the caret where it was; the caret is watched for crossing a
  control's edge only while the document has one of those two and its
  macros may run. The object model gained `ContentControls` on a document, a
  range and the selection, and a `ContentControl` with `Title`, `Tag`,
  `Type`, `Checked`, `LockContents`, `LockContentControl` and `Range`,
  writing into which goes through the control rather than round it.
  *Proven by:* eleven tests of modules and classes; four of the form's file
  format, one of them against the specification's bytes; four of forms
  running under a scripted window — filled in, refused a close, answered by
  a button, unloaded and shown again afresh; five of the form window's
  keyboard and mouse; three of a form put up from the Run button, typed
  into, ticked, and answered, shut with its cross, and put away when the
  document changes; two of message and input boxes answering with the
  button and the words; eight of the events — nothing on opening until
  content is enabled and then both names, a trusted document on opening, the
  closing macros, `Main` in a module of the macro's name, one that stops
  said on the status line while the other still runs, the caret into a
  control and out again, an exit cancelled, and a document with nothing to
  say not watched. Two pictures: a letter's form up over the document, and
  Word's Yes and No on a message box.
  *Found along the way:* seven old lines clippy had never named before came
  up on this run, in crates this item did not touch — a `from_` method
  taking `self`, a `3.14159` in a test, a comparison with `false` — and the
  toolchain has been the same since **J3**, so the cache that hid the dead
  code at **L1** is the likeliest reason; they are fixed rather than
  allowed.
  *Not done, and named here:* which runs first, `AutoOpen` or
  `Document_Open`, is not written down where it could be read; the auto
  macro goes first here and that is a choice. `AutoExec` and `AutoExit` are
  the Normal template's and there is no global template to hold them. An
  auto macro in a template attached to an existing document does not run on
  opening it — only the template's `AutoNew` for a document made from it.
  Of the content-control events, only entering and leaving are raised;
  `BeforeContentUpdate`, `BeforeDelete` and `AfterAdd` are not.
  `Class_Terminate` never runs, because handles are copied and nothing
  counts them; `Dim x As New Class1` makes the object at the declaration
  rather than on first use, which shows only after `Set x = Nothing`;
  `WithEvents` and `Implements` are stepped over. Frames and multi-pages on
  a form are storages of their own and their children are not read; images,
  spin buttons, scroll bars and tab strips are read as far as their site and
  drawn as empty boxes; fonts and colours are not read, so every control is
  drawn in the dialogs' one face; a text box has a caret at its end and no
  selection; `Controls` and `SetFocus` are not there. The Visual Basic
  editor shows a form's code and not its design, because a designer is not a
  line of this item. A form shown from the Immediate window says it has no
  window to be shown in, because that line still runs on the window's own
  thread. Everything above is held to the specification and to the writer
  beside the reader, not to a form Word wrote: the corpus is every machine's
  own, and a form in it is the first thing to try.
- [x] **L8. Mapping a content control to custom XML.**
  **Two views of one thing.** A document can carry data as well as words: an
  XML part of its own, of any shape its author likes, under `customXml/`.
  A content control bound to one node of it shows what the node says, and
  writing in the control writes the node. A program that fills the data in
  gets a document that reads as prose; a person who edits the prose leaves
  data a program can read back. That is what Word's XML Mapping pane is
  for, and it is on the Developer tab now, in a group called Mapping, where
  Word has it.
  **How a part is stored, which is how Word stores one.** Three things in
  the package: the data itself, `customXml/item1.xml`; a *datastore item*
  beside it, `itemProps1.xml`, carrying a GUID, because a control names the
  part it is bound to by that and not by a number that may change; and the
  relationship from the one to the other, with the document's own
  relationship reaching the item. The content types and the relationship
  types are the ones Word writes, and the GUID is made the way one is —
  hashed from the clock, the process and a count, with the bits set that
  mark a random one.
  **How a control is bound, which is how Word binds one.** `w:dataBinding`
  in the control's properties: an XPath, the prefixes it uses declared
  beside it, and the part's GUID. The paths Word writes are of one shape —
  every step an element by name with its number among its namesakes, and at
  most an attribute at the end — and that shape is what is followed; a
  path of another shape is not followed, and a control bound by one is left
  as it was found. A path made here counts elements and not the text
  between them, which a file with line breaks in it is full of.
  **When the two halves are brought together.** The node into the control
  when the document is opened, which is when Word does it — and not as an
  edit: the document is as it was opened, nothing is marked changed and
  there is nothing to undo. The control into the node as it is typed in:
  after every event that could have changed the document, the control the
  caret is in is written to its node if it is bound and its words have
  changed, which is one control found by the caret and nothing when the
  document has no binding at all. A tick box is bound to `true` and
  `false`.
  **The pane.** The parts the document carries, one of them chosen; the
  chosen part as a tree, an element to a row with its attributes set in
  under it and what each says beside it; and three buttons — bind the
  control the caret is in to the chosen node, or put a new one in and bind
  it, from a menu of the six kinds; add a part from an XML file; delete
  the chosen part. Word's binding is under a right-click on the node; here
  it is a button, because a button can be found. A control put in this way
  is titled with the node's name, as Word titles one.
  *Proven by:* six tests of the storage — a part added with its item and
  read back after a save, the package holding what Word looks for; a path
  leading to an element by number and to an attribute and nowhere for a
  prefix nobody declared; a bound control showing the node, writing back
  into it, following it after the node changed under the document, and all
  of it surviving a save; a tick box bound both ways; a part taken out with
  its item and its relationships; XML that does not parse refused — and
  four of the pane: opened from the Developer tab and listing the tree; a
  control put in from it showing the node and typing in it writing the
  node event by event, with the pane showing the new words; the control
  the caret is in bound, and a document saved and opened again showing
  what a program wrote into the node, unmarked; malformed XML refused with
  a reason. A picture of the pane beside a document with an order in it and
  the customer's name bound into the text.
  *Found along the way:* a line of interface text measures its width from
  the window's edge and not from where it began, which the form window
  (**L7**) had read the other way: the caret in a text box sat at the box's
  far end whatever was typed. Fixed here.
  *Not done, and named here:* the round trip through Word — the item is
  written where Word writes it and the binding the way Word writes it, and
  a document Word has opened and saved is the proof this item asks for and
  the corpus does not have. The pane shows a tree and not a schema: Word's
  "Add new part" also takes a namespace and a schema reference, and a part
  here has neither. A node's value cannot be edited in the pane, which is
  Word's way too. Repeating a section over repeated nodes is **L9**'s
  repeating-section control. A picture control bound to a node holding a
  picture is **L9** as well.
- [x] **L9. The three content controls that are not built.**
  **The picture control.** `w:picture` in the properties, a run with a
  `w:drawing` in the content: one more of the shape **J8** built, holding a
  picture rather than words. It is put in holding a picture — a placeholder
  the program draws, a grey card with a hill and a sun on it — because a
  control holding nothing has no width and nothing to click; and it is
  clicked to be given the picture it is for, which is asked for the way
  Insert Picture asks and takes the placeholder's place at its own size,
  brought down to the width of the text where it is wider. A picture is one
  character of the text, and the control's ends are counted the way the
  text counts them now, which they were not: a hyphen written as an element,
  a note's mark and a picture were each counted as nothing, so a control
  holding one ended a character early.
  **The repeating section.** The other shape of control: a `w:sdt` among the
  paragraphs of the body, with paragraphs inside it — which the reader has
  always stepped through, so a document carrying one laid out already; what
  was missing was knowing it was there and doing anything with it. A section
  is a list of items, each an `sdt` of its own, in the namespace Word added
  in 2013 and written with its own declaration as the tick box is. It goes
  round the paragraphs the selection covers, or the caret's, as one item,
  and only round paragraphs that are side by side, because a section round
  half a table is not something the format can say. Repeating is copying:
  Word draws a plus at the corner of each item, and pressing it puts a copy
  of the item after it, words and all; so does this, at the end of the
  item's last line. The right button offers Word's three — a copy before,
  a copy after, and taking the item away, which is refused for the last one
  as Word refuses it; the caret stays in the section when an item goes.
  **The building-block gallery.** `w:docPartList` naming a gallery and a
  category, round a paragraph that asks for a choice — in place of the
  caret's paragraph where that is empty, and after it otherwise. A click in
  it drops the blocks filed under its gallery in the person's own template
  (**J6**), and the one chosen puts its paragraphs in place of what the
  control held; chosen again, the old choice goes. A gallery with nothing
  filed in it says so. Word's nine controls are on the Developer tab now,
  in Word's order.
  *Proven by:* five tests of the storage — a section put round a selection
  as one item and read back after a save with the namespace declared; an
  item copied, found by paragraph, taken away, and the last one kept; a
  section refused round paragraphs that are not side by side; a gallery
  control asking, filled, filled again, and read back; one put in below a
  paragraph with words — and four of the program: the placeholder read back
  as a picture of its size; a picture control put in from the ribbon,
  counted as one character, given a picture and read back as a picture
  control with two pictures in the package; a section repeated from its
  plus, from its menu before, and taken away twice with the last one
  staying; a gallery control put in, its click the control's and saying
  the gallery is empty, and a block from a template in memory put in. A
  picture of the three on a page. And one of the object model (**L4**): a
  range counts accented letters as one character each, which it did not.
  *Found along the way:* the object model gave Word a byte offset where Word
  counts characters, so `Range(0, 4)` of "Café " was three letters and half
  of one and a paragraph's `End` was a character short for every accent in
  it; the two are translated at the edge now, as that entry said they were.
  *Not done, and named here:* read back by Word as what it is — the three
  are written as Word writes them and the corpus has no document Word has
  read back, which is what this item asks for. The Properties dialog of a
  repeating section — Word's "allow users to add and remove sections" and
  the name of the item — is not there; a section's items are copied and
  removed and nothing else. A repeating section bound to a repeating node of
  a custom XML part (**L8**), one item to a node, is not done; nor is a
  picture control bound to a node holding a picture. A gallery control
  offers the person's own template and not the galleries Word ships with —
  cover pages, headers, page numbers — which are **J6**'s and were never
  filed as blocks. The placeholder text of a control is drawn as text and
  not greyed, which is **L10**'s Design Mode.
- [x] **L10. Design Mode.**
  **A placeholder is a question, and the file says so.** A control put into
  a document holds the words that ask for an answer — "Enter text", "Choose
  an item", "Choose a building block." — until somebody answers. Word draws
  those grey, selects the whole of them when the control is clicked so that
  typing replaces them, and marks the control as showing them,
  `w:showingPlcHdr`, so that a program reading the file can tell a question
  from an answer. **J8** wrote the words and none of the rest. Now a new
  control says it is showing its placeholder; the placeholder is read grey
  for showing and plain for copying; a click in it selects it whole; typing
  over the whole of a control's words goes inside the control, where before
  the words fell off its edge into the paragraph; and the mark comes off
  the moment the words change — by typing, by an item chosen, by a block
  chosen, by a node bound (**L8**).
  **Design Mode is the button that turns the page into the drawing board.**
  On the Developer tab, lit while it is on. Every control shows its tags —
  the start tag with the control's title on it, or its kind where it has
  no title, and the end tag after it — instead of the faint brackets **J15**
  drew; and typing in a control showing its placeholder changes the
  placeholder itself, which stays a placeholder: that is how the words that
  ask are written in the first place. Off again, the same typing is an
  answer. The controls round paragraphs (**L9**) wear tags too, at the
  start of the first and the end of the last.
  *Proven by:* three tests. A click on a placeholder selects it and typing
  replaces it inside the control, the control stops asking, and a click in
  an answered control is the caret's; in Design Mode the click is the
  caret's, typing changes the placeholder, it stays one through a save and
  comes back grey; and the tag says the title, or the kind. A picture of a
  form in Design Mode: three controls tagged, two placeholders grey, one
  answered. One older test turned round: a text control used to leave every
  click to the caret, and now leaves the ones on an answer.
  *Found along the way:* typing over the whole of a control's words put the
  new words outside the control, because at the boundary between the words
  before it and its own the earlier piece won, as it should everywhere else;
  a selection that is exactly a control's content is answered through the
  control now.
  *Not done, and named here:* Word's tags sit in the line and push the words
  along; these hang just above the line, translucent so the line above shows
  through, because the layout does not know about them and a tag drawn over
  the words would hide what it is tagging. Word keeps a control's
  placeholder as a building block in the glossary and points at it with
  `w:placeholder`, so that a control emptied of its answer asks again with
  the same words; here the placeholder is the content while it is showing,
  and a control emptied of its answer is empty. The read back by Word of a
  control marked as showing its placeholder is, like the rest of block
  **L**'s, not something the corpus can show.

---

## The order of the work

Printing first (**A**), because it is the one thing a word processor cannot be
without and there is none of it. Then speed (**B**), because it has to be fixed
while the documents are still small enough to work with, and because every
later item makes the layout do more. Then the interface (**C**), which is the
largest body of work but also the most divisible: each dialog is finished on its
own and shows up immediately.

**D** to **L** are ordered by how often a real document needs them, and that
order is a judgement rather than a rule: a document that will not open because
of a metafile picture moves **D2** to the front of the queue.

Everything a finished item names as not done has an item of its own, in the
section it belongs to and in the same queue as the rest. A *Not done, and
named here* paragraph is the record of what a piece of work left behind; the
item it turns into is where that gets finished. Nothing is closed by deciding
against it.

**The queue from 29 September 2026.** Every item from the first queue was
closed, and four reviews of that day — the interface, tables, and two of the
model and the files, kept in `docs/.reviews/` outside the repository — gave
the next one. It is worked in this order, by harm first and then by what a
person who knows Word notices first:

1. *Crashes and lost data:* **C51**, **H16**, **C48**, **H17**, **C49**,
   **C50**, **J34**.
2. *Word as the judge:* **K4**.
3. *Files Word opens without a word:* **G19**, **G20**, **G21** — each
   checked with **K4**.
4. *The interface, the small things that matter:* **C52**, **C53**, **C54**,
   **C55**, **C56**, **C57**, **C58**, **C59**, **C60**.
5. *Tables:* **C61**, **C62**, **B8**, **C63**, **C64**, **C65**, **C66**,
   **C67**, **C68**, **C69**, **C70**, **C71**.
6. *The interface, the large things:* **C72**, **C73**, **C74**, **C75**,
   **C76**, **C77**.
7. *Hostile files:* **G17**, **G18**, **H18**.
8. *Layout and speed:* **E19**, **B7**.
9. *The tails:* the 176 *Not done* paragraphs of the closed items, read
   through one by one, the ones the items above do not already cover
   turned into items of their own, and the ones that need something this
   machine has not got — a printer, a licence — named as such.
10. *Last, by decision:* what needs the printer (**A**'s tails), and the
    Arabic catalogue and the rest of a window read right to left (**I**'s).

Each item is closed as before: tests, `./x.sh check` on both targets, the
entry here ticked in the same commit, a Windows build. An interface item
also gets a picture of ours beside the review's picture of Word, looked at.

---

## Where the work stands

**It is an editor, and it looks like Word.** A document is unpacked, parsed,
resolved against its styles, laid out with the fonts on the machine, rasterized
and shown in a window with a ribbon — and then clicked into, selected in, typed
in, formatted, searched, undone and saved. Word opens the result in the current
mode and reads the same words back.

The whole interface is drawn by the same rasterizer as the document. There is no
widget toolkit and no second way to put a pixel on screen, which is why the
window can be photographed without a window: `--picture` writes it to a PNG, and
that is how every part of it is checked.

Text is handled as the standards say it should be, not as English-only code
would: mixed Hebrew and English come out in the right order with their brackets
facing the right way, Arabic is joined by the font's own rules, lines break
where the language allows, the caret steps over a whole character however many
code points it took, and a search finds a word whichever way its accents were
typed.

A `.docx` can be created, opened, read, edited and saved. Saving a document that
was not edited reproduces it byte for byte. Editing it rewrites only the part
that changed, and inside that part only the nodes that changed — a content
control, a chart or a colleague's tracked change beside the edit comes through
untouched.

**It prints**, on Windows through the spooler and on Linux through CUPS, and
writes a PDF.

**What is in the queue is what four reviews found**, on 29 September 2026,
once every item of the first queue was closed: a save that can destroy the
file it replaces, an undo that lies after a save, a signature check that an
unsigned manifest gets past, tables that lose text when cells are merged, an
interface that a person who knows Word finds strange in forty ways and
tables in sixty. The order they are taken in is in *The order of the work*,
and the reviews themselves are in `docs/.reviews/`, outside the repository.
Behind them wait the tails the closed items named as not done, one item to
a tail.

[MS-DOC]: https://learn.microsoft.com/openspecs/office_file_formats/ms-doc/
[MS-CFB]: https://learn.microsoft.com/openspecs/windows_protocols/ms-cfb/
