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

- [ ] **A5. Printing on Linux.** CUPS, the same page images, through the same
  layer.
  *Waiting for **H6**, the Linux window.* There is nothing on Linux to print
  from yet, and nothing in the build container to print to, so the code could
  not be run even once. What the Linux build can already do is write the PDF
  that CUPS takes as its own input.
  *Done when:* it prints from the Linux build.

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

- [ ] **B6. Reusing the pages that did not move.** The paragraphs are no longer
  measured again, but they are all still *placed* again: a keystroke walks every
  page of the document to work out where each line goes, which is the quarter of
  a second a thousand pages cost. Keep the pages before the change, lay out from
  the paragraph that changed, and stop as soon as the pagination lands where it
  landed before.

  *Deferred, and here is what it is measured against.* A person types about ten
  characters a second, so a keystroke has about a hundred milliseconds before it
  is felt:

  | Pages | A keystroke |
  | --- | --- |
  | 10 | 2 ms |
  | 100 | 15 ms |
  | 300 | 65 ms |
  | 1000 | 220 ms |

  Up to a few hundred pages there is room to spare, and beyond that there is
  not. The work itself is the hardest left in the layout: `layout_body` is one
  pass over shared state — where the text has got to down the page, which
  column and which page, the floating drawings, the list counters, the section
  and its footnotes — and starting in the middle means being able to save all
  of that at a paragraph boundary and take it up again. Getting it wrong does
  not crash; it quietly draws the wrong page.

  *Come back to it when* a document of several hundred pages is actually being
  edited — the corpus of **K1** will say whether that happens — or when the
  lag is felt. The guard is already written: `tests/incremental.rs` holds the
  engine to giving what a fresh engine gives.

  *Done when:* a keystroke costs the same on a thousand pages as on ten, and
  the pages are identical to a full relayout.

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

- [ ] **D24. The rest of what a chart is.** What **D4** left, named:
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

- [ ] **D25. The layout language, and editing a diagram.** `layout1.xml` is
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

- [ ] **D26. A shape drawn in the theme's colours.** `a:schemeClr` where a
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

- [ ] **D27. Drawing ink, and the media a document carries inside it.** What
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

- [ ] **E13. The other scripts that reorder.** Bengali, Gurmukhi, Gujarati,
  Oriya, Tamil, Telugu, Kannada, Malayalam and Sinhala. They share the shape of
  the rules **E1** now has — a syllable, a letter the rest hangs on, pieces
  drawn in an order of their own — and differ in every detail of them: where
  the hook goes (some put it at the front, some after the base, some at the
  very end), which consonants take a form below the line, and the vowel signs
  written in two pieces, one either side of the consonant, which have to be
  split before the font is asked anything. Each needs its own reading of the
  same tables, and the block of each is a hundred and twenty-eight characters
  to be categorised by hand as Devanagari's was.
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

- [ ] **E14. Automatic hyphenation.** Breaking a word nowhere anybody said it
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

- [ ] **E15. Vertical writing.** Japanese set down the page rather than across
  it: `w:textDirection` on a section, a frame or a table cell, and
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
  Worth doing after the engine's own axes are a thing it can be asked about
  rather than a thing it assumes.
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
- [ ] **E16. Line breaking with the standard's own classes.** UAX #14 names
  about forty and this program keeps seventeen, folding the rest into them by a
  list in `tools/unicode/generate.rs`. What the folding costs, named: the half
  of LB9 that gives a combining mark the class of the letter it is drawn on —
  the other half, that a mark may not begin a line, is kept; LB30b, which
  forbids a break between an emoji and its skin tone, is approximated by
  making the tone a non-starter; B2, the em dash, allows a
  break before it as well as after and here allows only after; SY, the solidus,
  is broken after even between two digits, where LB25 forbids it; and the
  Korean jamo are letters, so a syllable spelled out in them is never broken
  anywhere. The item is the standard's own class set and its pair table, and
  the rules written against them rather than against a fold.
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
- [ ] **E17. Cutting down a PostScript font.** A PDF carries the fonts it
  needs, and the TrueType ones are cut down to the glyphs the document uses; a
  PostScript one goes in whole, which is a megabyte where twenty kilobytes
  would do. Cutting one means rebuilding the `CFF` table: keeping the
  charstrings that are wanted, following the subroutines they call, renumbering
  what is left against the bias, and writing the INDEXes and dictionaries back
  out. The glyph numbering must not move, because the page refers to it.
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
- [ ] **E18. The rest of what a colour glyph can be.** Three things **E10**
  left. `COLR` version 1: the gradients, the transforms and the compositing
  modes a layered glyph may be drawn with, which a font using them loses here
  and shows flat. `sbix`: Apple's table of pictures per glyph per size, the
  same idea as `CBDT` with the sizes listed rather than indexed. `SVG `: a
  drawing per glyph, which needs the drawing language rather than a table —
  `wp-svg` is already here and would be what reads it. Each needs a font of
  its kind on the build image to be held to, which is the first thing to
  settle; and with the layers understood, a layered emoji can go into a PDF as
  its layers rather than as its base glyph.

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
- [ ] **F7. The compound rules.** A dictionary may say how words join as a
  pattern over flags rather than as a flag on each word: `COMPOUNDRULE n*1t`
  is what makes `11th` and `21st` right and `11st` wrong. English uses it only
  for the ordinal numbers; Hungarian and Korean use it for the language. The
  reader keeps the flags and ignores the patterns, so a word a pattern would
  allow is underlined. Reading the patterns is a small matcher over the flags
  of each piece, and the pieces are already found for the simple flags.
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
- [ ] **F8. Grammar rules for other languages.** The engine in `wp-grammar`
  takes a pattern over words and says what it found; only English has a list.
  Each further language is a list of its own — the mistakes that show in a few
  words side by side, in that language, with what Word calls each — and a
  reader of the language to say which are mistakes. Russian, German and French
  first, being the languages this program's dictionaries already know.
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
- [ ] **G8. Writing the binary `.doc`.** Word 97-2003 Document in Save As:
  the compound file written — header, FAT, directory, mini stream — and a
  document in it with one piece of text, its formatting pages and bin
  tables, a stylesheet of the styles used, the font table, the two lists,
  tables, pictures in the data stream, links as fields, and the section's
  page. Reading it back is not the test; Word opening it is, and until
  something that is not this program can be made to open one here,
  LibreOffice reading it is what stands in.

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
- [ ] **H2. The clipboard formats Word uses.** `CF_HTML`, RTF, and images, so
  that copying between this and Word keeps the formatting.
- [ ] **H3. Drag and drop.** Between programs as well as within the document,
  and dropping a file onto the window.
- [ ] **H4. Accessibility.** UI Automation on Windows, AT-SPI on Linux: a screen
  reader has to be able to read the document and drive the ribbon.
- [ ] **H5. High DPI and several monitors.** Per-monitor scaling, and the window
  moving between monitors of different scales without redrawing wrongly.
- [ ] **H6. The Linux shell.** X11 and Wayland: window, input, clipboard,
  presentation. The rest of the program is already portable.
- [ ] **H7. Files the way an operating system means them.** Recent documents,
  file associations, the shell's open and save dialogs, autosave and recovery
  after a crash.

## I — The language of the interface

- [ ] **I1. Message catalogues.** Every string in the interface comes from a
  catalogue rather than from the code.
- [ ] **I2. A mirrored interface.** For Arabic and Hebrew the whole window
  reverses: the ribbon, the panes, the scrollbar, the dialogs.
- [ ] **I3. The user's locale.** Dates, numbers, paper sizes and measurement
  units — inches or centimetres — as the system says.

## J — Protection, collaboration and the rest of Word's features

- [ ] **J1. Document protection.** Read-only, filling in forms only, tracked
  changes forced on, and the password behind them.
- [ ] **J2. Encryption.** Opening and writing the encrypted `.docx` Word makes,
  which is an OLE compound file with the package encrypted inside it.
- [ ] **J3. Digital signatures.**
- [ ] **J4. Compare and merge, finished.** Word's three-way merge and its
  compare view.
- [ ] **J5. Mail merge, finished.** The data sources, the field mapping, the
  preview, and the merge to a document, to a printer or to mail.
- [ ] **J6. Building blocks, Quick Parts and templates.** Including the
  `Normal.dotm` a person's own defaults live in.
- [ ] **J7. Macros.** A VBA interpreter is a language implementation; it is
  listed here so that the decision not to write one is a decision and not an
  oversight.

## K — Proving it against Word rather than against ourselves

- [ ] **K1. A corpus of real documents.** Files Word itself wrote, kept outside
  the repository, and a harness that opens, saves and compares every one of
  them.
  *Done when:* `./x.sh corpus` reports how many round-trip byte for byte and
  what differs in the rest.
- [ ] **K2. Page images compared against Word's.** A fidelity score per
  document rather than a pass or a fail, tracked over time so that it can be
  seen to improve.
- [ ] **K3. The Unicode conformance suites** run against the text engine.
  **E7** has replaced the hand-written tables, so there is now something worth
  holding to the standard's own answers. What is needed is the files:
  `BidiTest.txt` and `BidiCharacterTest.txt`, `LineBreakTest.txt`,
  `GraphemeBreakTest.txt` and `WordBreakTest.txt`, and `NormalizationTest.txt`.
  The character database in the build image is Perl's, and Perl does not carry
  those — they are test data rather than character data. So this is the same
  decision as **E2**, **E6** and **E14**: where data of that kind comes from
  and where it lives.

---

## The order of the work

Printing first (**A**), because it is the one thing a word processor cannot be
without and there is none of it. Then speed (**B**), because it has to be fixed
while the documents are still small enough to work with, and because every
later item makes the layout do more. Then the interface (**C**), which is the
largest body of work but also the most divisible: each dialog is finished on its
own and shows up immediately.

**D** to **K** are ordered by how often a real document needs them, and that
order is a judgement rather than a rule: a document that will not open because
of a metafile picture moves **D2** to the front of the queue.

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

**What it cannot do yet** is print, and that is where the work goes next.

[MS-DOC]: https://learn.microsoft.com/openspecs/office_file_formats/ms-doc/
[MS-CFB]: https://learn.microsoft.com/openspecs/windows_protocols/ms-cfb/
