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
  because the numbering model has no other starting number yet.

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

- [ ] **C43. The rest of the Layout dialog.** What **C38** left: the Scale
  boxes, which need a picture's original size kept beside it; Relative position
  and Relative width, which are percentages of a frame and a second way of
  writing `wp:positionH`; the side the text wraps on, which is
  `wrapSquare/@wrapText`; and Move object with text, Allow overlap and Lock
  anchor, which are three flags on `wp:anchor` that nothing yet reads.

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

- [ ] **D11. Text in a metafile.** The records that draw words, the fonts they
  name and how those are matched against the fonts actually present, the
  alignment, and the escapement that turns a label on its side.
  *Done when:* a metafile with words in it draws them where an independent
  player puts them.
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
  offers one flat list of them; and the adjustments — the yellow handles that
  make a rounded corner rounder or an arrow's head wider — are not read, so each
  shape is drawn at the proportions the format uses when nothing says otherwise.

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

- [ ] **D13. The shape effects.** `a:effectLst`: the outer and inner shadow, the
  glow, the soft edge and the reflection, drawn as effects on a shape rather
  than the approximation the letters use. A real blur is the piece of work
  underneath all of them.
  *Done when:* a shape with each effect is drawn as Word draws it.

- [ ] **D14. Three dimensions.** `a:scene3d` and `a:sp3d`: the bevels, the
  extrusion and its depth, the material, the lighting and the camera. Word
  draws these flat when it cannot manage them, and so could this — but a bevel
  is the one most documents use and is worth drawing properly.
  *Done when:* a shape with a bevel and a depth is drawn with them.

- [ ] **D15. Block arrows.** The twenty-eight arrows of Word's gallery: the four
  straight ones, the bent and the curved, the striped and the notched, the
  chevron and the pentagon, and the circular arrow.
  *Done when:* each is drawn, and each is drawn the way round its name says.

- [ ] **D16. Flowchart shapes.** The twenty-eight boxes a flowchart is drawn
  with: the process, the decision, the terminator, the document, the stored
  data, and the rest.
  *Done when:* each is drawn.

- [ ] **D17. Stars, banners and callouts.** The stars from four points to
  thirty-two, the explosions, the ribbons and scrolls, and the twenty callouts —
  the rectangular, rounded, oval and cloud bubbles, and the line callouts with
  their bends.
  *Done when:* each is drawn, and a callout's tail points where its adjustment
  says.

- [ ] **D18. Lines and connectors.** The straight, elbow and curved connectors,
  their arrowheads at either end, and the routing that keeps an elbow out of the
  shapes it joins.
  *Done when:* two shapes joined by each kind of connector stay joined when
  either is moved.
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
