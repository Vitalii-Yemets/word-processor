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

*Not done:* CFF and CFF2 outlines (PostScript-flavoured fonts), variable fonts,
colour and bitmap glyph tables — items **E8** to **E10**.

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
  letter, made one, for the Latin alphabets of Europe, Greek and Cyrillic.

The rest of the text engine is items **E1** to **E7**.

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

  Not done: **AutoCorrect**. Word's dialog has a button that adds the chosen
  character to the AutoCorrect list, and there is no AutoCorrect in this
  program at all — no list, no replacement as you type, nothing to add to. A
  button that opened an empty dialog would be worse than no button. It is
  **C19** below, with what it would take.
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

  Not done, and named rather than drawn as dead rows: **Bring Forward** and
  **Send Backward** need an order among drawings that the model does not carry
  (`wp:anchor relativeHeight` is written as one number for all of them), and
  **Show Markup** needs comments and formatting revisions to be markable apart
  from insertions and deletions — both are **C21** below. Word's **Select
  Objects** and **Select Text with Similar Formatting** need a selection made of
  several separate stretches, which is **C22**.
- [ ] **C12. The boxes on the Layout tab.** The indent boxes are drawn and
  cannot be typed into — pressing them says to drag the ruler instead. Spacing
  before and after has no boxes at all. Both are measurements a person types.
- [ ] **C13. Design ▸ Paragraph Spacing does the wrong thing.** It cycles the
  line spacing of the document. Word's sets a named spacing set — Compact,
  Tight, Open, Relaxed, Double — on the style set, changing space before and
  after as well as the lines.
- [ ] **C14. Page Borders.** Opens the same list of edges a paragraph border
  uses. Word opens Borders and Shading on its page tab: art borders, which pages
  they go on, and the distance from the edge.
- [ ] **C15. The Table Design tab.** Header Row and Banded Rows do nothing and
  say so — the only two buttons in the program that do. The tab is also missing
  the table styles gallery, shading, the border styles and the border painter,
  and the first-column and banded-column switches.
- [ ] **C16. The rest of the Table Layout tab.** Select, View Gridlines, Draw
  Table and Eraser, AutoFit, the height and width boxes, Text Direction, Cell
  Margins, Sort, Repeat Header Rows, Convert to Text, and Formula. And nine
  alignments where there are three.
- [ ] **C17. The rest of the Header & Footer tab.** Header from Top, Footer from
  Bottom, and Insert Alignment Tab.

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

- [ ] **C19. AutoCorrect.** There is none at all: no list of replacements, no
  replacing as you type, and so nothing for the button in **C5** to add to.
  Word's is four tabs — AutoCorrect (the replacement list, plus the five tick
  boxes: two initial capitals, first letter of a sentence, day names, the Caps
  Lock fix), AutoFormat As You Type (straight quotes to curly, ordinals to
  superscript, fractions, hyphens to dashes, automatic lists), AutoFormat, and
  Actions.
  Most of it is one mechanism: watch what was typed since the last word
  boundary, and replace it. The mechanism is the item; the tables are what goes
  on top.
  *Done when:* typing "teh " gives "the ", a straight quote comes out curly, a
  hyphen between words becomes a dash, the list can be edited, and every one of
  those can be turned off.

- [ ] **C20. Customize Ribbon and the Quick Access Toolbar.** Two of the
  categories **C7** leaves out, and they are one job: both are a person saying
  which commands go where.
  The ribbon here is a static table — `RIBBON_GROUPS` and its neighbours — so
  customising it means that table becoming a starting point rather than the
  whole truth, with what a person changed kept beside it in the settings.
  Word's Quick Access Toolbar is the row of small buttons in the title bar,
  which exists here with three fixed commands on it.
  *Done when:* a command can be added to the toolbar and to a ribbon group, a
  group can be moved or hidden, the changes survive closing the program, and
  Reset puts it all back.
- [ ] **C21. The order things are drawn in, and which marks are shown.** Two
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
  *Done when:* two overlapping drawings can be reordered and stay that way
  through a save, and each of Word's three kinds of markup can be shown or
  hidden on its own.
- [ ] **C22. A selection of more than one stretch.** Word can hold several
  separate stretches of text selected at once: Ctrl and a drag adds to the
  selection, and its Select menu uses it for "Select Objects" and "Select All
  Text With Similar Formatting". Here a selection is one anchor and one caret,
  so there is nowhere to put the second stretch.
  It reaches further than the two menu entries: Find All, formatting applied to
  every heading at once, and a column selection made with Alt all want it.
  *Done when:* Ctrl and a drag adds a stretch, every command that works on the
  selection works on all of them, and the two entries Word's Select menu is
  missing here are on it.

## D — Pictures and drawings

- [ ] **D1. The image formats a document carries.** PNG is done. JPEG baseline
  and progressive, GIF including animation's first frame, TIFF, BMP in its
  several forms.
  *Done when:* each decodes to the same pixels as an independent decoder for a
  set of test images.
- [ ] **D2. WMF and EMF.** The metafile formats Word documents still carry:
  a record interpreter drawing through the rasterizer.
- [ ] **D3. The rest of DrawingML.** The preset shape geometries that are not
  yet built, gradients, patterns, 3-D effects, and the shape effects Word draws.
- [ ] **D4. Charts.** The chart types beyond those drawn today, their axes,
  legends, labels and the data table behind them.
- [ ] **D5. SmartArt.** The diagram layouts, which are a language of their own
  in the file format.
- [ ] **D6. Ink and media.** What a document holds when somebody drew on it or
  put a video in it.

## E — The rest of the text engine

- [ ] **E1. Indic reordering.** Devanagari, Bengali, Tamil, Telugu and the rest:
  a syllable is reordered before it is drawn, and the rules differ per script.
- [ ] **E2. Thai and Lao clustering**, and the line breaking they need, which is
  by dictionary rather than by rule.
- [ ] **E3. Hyphenation.** Breaking inside a word, with pattern data per
  language, and Word's controls: automatic, manual, hyphenation zone, limit
  consecutive hyphens.
- [ ] **E4. Vertical writing and ruby.** Japanese set vertically, and the small
  annotations above it.
- [ ] **E5. Case mapping with language tailoring.** Turkish `i` and `İ`, German
  `ß`, Greek final sigma, and Word's Change Case following the paragraph's
  language.
- [ ] **E6. Word segmentation for Chinese and Japanese.** A dictionary, because
  there is no rule: it is what a double click selects and what the word count
  counts.
- [ ] **E7. The full Unicode tables.** The subsets written by hand for bidi,
  breaking, segmentation and normalization become generated, committed tables
  covering every character, checked against the conformance files.
- [ ] **E8. CFF and CFF2 outlines.** PostScript-flavoured fonts, which a good
  many documents ask for.
- [ ] **E9. Variable fonts.** The axes, the named instances, and the deltas.
- [ ] **E10. Colour and bitmap glyphs.** Emoji, in colour, as Word draws them.

## F — Proofing

- [ ] **F1. Real dictionaries.** Reading the open dictionary formats — the
  affix rules and the word list — so that a language's inflections are known
  rather than a fixed list of words.
- [ ] **F2. Spelling as Word does it.** As-you-type checking, the wavy line, the
  right-click list of suggestions, add to dictionary, ignore all, custom
  dictionaries, per-language settings, and the settings that turn it off.
- [ ] **F3. Grammar.** A rule engine and the rules for at least one language,
  with the wavy line of its own colour and the explanation Word gives.
- [ ] **F4. Thesaurus.**
- [ ] **F5. AutoCorrect and AutoFormat as you type.** The replacement table,
  the capitalisation rules, smart quotes, dashes, lists that start themselves,
  and the little box that lets a person undo one of them.
- [ ] **F6. Translation.** What Word's Translate does, in so far as it can be
  done without sending the document to somebody else's computer.

## G — The files Word can open

- [ ] **G1. The other OOXML files.** `.docm`, `.dotx`, `.dotm`: templates and
  macro-enabled documents, which differ in their content types and their parts.
- [ ] **G2. Plain text**, with encoding detection and the dialog Word shows when
  it is not sure.
- [ ] **G3. RTF.** Read and write. It is the format everything else exports to.
- [ ] **G4. HTML and MHT.** Read and write, including the mess Word itself
  writes.
- [ ] **G5. The binary `.doc`.** [MS-DOC] over [MS-CFB]: the compound file, the
  piece table, the formatting sprms. A project in itself, and the reason a
  twenty-year-old document can still be opened.
- [ ] **G6. ODT.** Read and write, which is what an open format is for.
- [ ] **G7. PDF import.** Word does it; it is text extraction and reflow.

## H — The system around the window

- [ ] **H1. IME.** Without it Chinese, Japanese and Korean cannot be typed at
  all: the composition window, the candidate list, and the text that is not yet
  committed shown in the document.
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
- [ ] **K3. The Unicode conformance suites** run against the text engine, once
  **E7** has replaced the hand-written tables.

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
