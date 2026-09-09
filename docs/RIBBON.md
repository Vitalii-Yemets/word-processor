# The ribbon, button by button

Every button on every tab, what it does when it is pressed, and where that falls
short of the button Word draws in the same place. Written by walking the ribbon
in the code — the tabs, their groups, and the command each item runs — and then
following each command to what it actually does.

## What this found

**Nothing is decoration.** Two hundred and seven buttons, and two of them do
nothing but say so: Header Row and Banded Rows on the Table Design tab. Every
other one runs something real.

**The gap is depth, not breadth.** Word's small buttons are usually the top of a
menu — Bullets drops a library of bullet shapes, Line Spacing drops a list and a
dialog, Accept drops four ways of accepting. Here they are one action each: the
common one, done straight away. That is a fair first version and it is not what
Word does, so each of them is written down below and gathered into numbered work
at the end.

**The dialogs are strips.** Word answers a button that needs more than a click
with a dialog; this answers with a strip along the top of the window, because
there is no dialog machinery yet. That is item **C9** in the roadmap, and it is
why so many rows below say "a strip".

Legend: **✓** does what Word's does · **≈** does the common case, where Word
offers more · **✗** does nothing yet.

---

## File

| Button | What it does | Beside Word's |
| --- | --- | --- |
| New | a new empty document, asking about unsaved work first | ✓ |
| Open | the system's open dialog, and opens what it is given | ≈ Word lists recent documents and places |
| Save | writes the file, or asks where to put it | ✓ |
| Save As | the system's save dialog | ≈ Word offers the formats it can write; here it is `.docx` |
| Print | the Print page | ✓ — see **A3** |
| Info | the document's properties, in a strip | ≈ Word's Info page also shows protection, versions and inspection |
| Close | closes the document, asking about unsaved work | ✓ |

The File tab here is a tab like the others. Word's is the backstage: a page of
its own, with Info, Recent, New from template, Save As, Print, Share, Export and
Account down the side. That is item **C8**.

## Home

### Clipboard

| Button | What it does | Beside Word's |
| --- | --- | --- |
| Paste | pastes text, formatted blocks and pictures from the system clipboard | ≈ Word offers paste options — keep formatting, merge, text only — and the little button after pasting: **C10** |
| Cut, Copy | to the system clipboard, formatting and all | ✓ |
| Format Painter | picks up the formatting at the caret and paints the next selection with it | ≈ in Word a double click keeps the brush until Escape |

### Font

| Button | What it does | Beside Word's |
| --- | --- | --- |
| Font box | drops the families on the machine, and applies one | ≈ Word's box can be typed into and previews as the pointer moves down the list |
| Size box | drops the sizes, and applies one | ≈ the same |
| Grow, Shrink | steps up or down Word's own ladder of sizes | ✓ |
| Change Case | cycles: sentence, lower, upper, title | ≈ Word drops a menu of five and toggles case as well: **C11** |
| Clear All Formatting | takes the direct formatting off the selection | ✓ |
| B, I, U, S | bold, italic, underline, strikethrough | ≈ Word's U drops a list of underline styles and colours |
| Subscript, Superscript | sets the vertical alignment of the run | ✓ |
| Text Effects | a list of the effects a run can carry | ≈ Word's is a gallery with submenus per effect |
| Highlight | a palette of highlight colours | ✓ |
| Font Color | a palette, theme colours and standard | ≈ Word also has More Colors and a gradient submenu |

There is no Font dialog behind the group — item **C2**.

### Paragraph

| Button | What it does | Beside Word's |
| --- | --- | --- |
| Bullets | turns a bulleted list on or off | ≈ Word drops a library of bullet characters: **C11** |
| Numbering | turns a numbered list on or off | ≈ Word drops a library of number formats: **C11** |
| Multilevel List | steps the caret's paragraph to the next level | ≈ Word drops a library of list definitions: **C11** |
| Decrease, Increase Indent | half an inch at a time, as Word does | ✓ |
| Sort | sorts the selected paragraphs | ≈ Word opens a dialog: sort by what, ascending or descending, has a header row |
| Show/Hide ¶ | shows the formatting marks | ✓ |
| Align left, centre, right, justify | sets the paragraph's alignment | ✓ |
| Line and Paragraph Spacing | cycles single, 1.15, 1.5, double | ≈ Word drops a menu, with space before and after and the dialog: **C11** |
| Shading | a palette, applied to the paragraph | ✓ |
| Borders | a list of which edges to draw | ≈ Word's last item is the Borders and Shading dialog |

There is no Paragraph dialog behind the group — item **C3**.

### Styles and Editing

| Button | What it does | Beside Word's |
| --- | --- | --- |
| The style gallery | shows the document's styles and applies one | ≈ Word's gallery scrolls, previews on hover, and has a pane behind it: **C4** |
| Find, Replace | the find strip, with case and whole-word | ≈ Word has Advanced Find, wildcards, and search by formatting |
| Select | selects the whole document | ≈ Word drops a menu: all, objects, text with similar formatting: **C11** |

## Insert

| Button | What it does | Beside Word's |
| --- | --- | --- |
| Cover Page | a gallery of cover pages, inserted at the front | ✓ |
| Blank Page, Page Break | inserts the break | ✓ |
| Table | the grid, dragged to a size | ≈ Word also has Insert Table (dialog), Draw Table, a spreadsheet and Quick Tables |
| Pictures | a file dialog, and the picture goes in at the caret | ≈ Word also has online pictures and stock images |
| Shapes | a list of shapes, drawn where the caret is | ≈ Word's is a gallery by category, and the shape is drawn by dragging |
| SmartArt | a list of diagram arrangements | ≈ Word's gallery is far larger |
| Chart | a list of chart kinds, with a data sheet behind it | ≈ Word's gallery is larger and its data sheet is a spreadsheet |
| Screenshot | a list of the windows on screen | ✓ |
| Video | a strip for the address | ≈ Word previews the video |
| Link, Remove Link | a strip for the address | ≈ Word's dialog links to places in the document, to e-mail, and to a new document |
| Bookmark | a strip for the name | ≈ Word's dialog lists and deletes them |
| Cross-reference | a list of what can be pointed at | ✓ |
| Comment | starts a comment on the selection | ✓ |
| Header, Footer | galleries of ready-made ones | ✓ |
| Page Number | opens the footer for editing | ≈ Word drops a menu — top, bottom, margins, current position — each a gallery: **C11** |
| Format Page Numbers | the numbering strip: format and where to start | ✓ |
| Text Box | draws one and puts the caret in it | ≈ Word has a gallery of ready-made boxes |
| Signature Line | a strip for the signer | ✓ |
| Quick Parts | a list of fields to drop in | ≈ Word also has building blocks, document properties and an organiser |
| Date & Time | inserts today's date as a field | ≈ Word's dialog offers the formats and whether it updates |
| WordArt | a list of the looks | ✓ |
| Text from File | a file dialog, and the text goes in | ✓ |
| Equation | starts an equation | ≈ Word has a gallery of ready-made equations and a tab of symbols |
| Symbol | a grid of characters | ≈ Word keeps the recently used and has More Symbols: **C5** |

## Design

| Button | What it does | Beside Word's |
| --- | --- | --- |
| Themes | the document themes, applied whole | ✓ |
| Colors, Fonts, Effects | the theme's three parts, each a list | ✓ |
| Paragraph Spacing | cycles the line spacing of the document | ✗ **wrong thing**: Word's sets named spacing sets — Compact, Tight, Open, Relaxed, Double — on the style set: **C13** |
| Set as Default | keeps the theme for new documents | ✓ |
| Watermark | a gallery, and a custom one in a strip | ✓ |
| Page Color | a palette | ✓ |
| Page Borders | the same border list the paragraph uses | ≈ Word opens Borders and Shading on the page tab, with art borders and which pages: **C14** |

## Layout

| Button | What it does | Beside Word's |
| --- | --- | --- |
| Margins | Word's four presets | ≈ Word's last item is Custom Margins |
| Orientation | portrait or landscape | ✓ |
| Size | the named paper sizes | ≈ Word's last item is More Paper Sizes |
| Columns | one, two or three | ≈ Word offers left, right and More Columns |
| Breaks | page, column and section breaks | ✓ |
| Line Numbers | none, continuous, restart each page or section | ✓ |
| Hyphenation | none or automatic | ≈ Word also has Manual and Hyphenation Options |
| Indent left, right | says to drag the ruler instead | ✗ the boxes are drawn but cannot be typed into: **C12** |
| Position, Wrap Text | where a drawing sits and how text goes round it | ✓ |
| Bring Forward, Send Backward | one step in the drawing order | ≈ Word drops a menu: to front, forward, in front of text |
| Selection Pane | lists the drawings and picks one | ✓ |

Spacing before and after, on the same group in Word, has no boxes here either —
part of **C12**.

## References

| Button | What it does | Beside Word's |
| --- | --- | --- |
| Table of Contents | writes one from the headings | ≈ Word has a gallery and a Custom Table of Contents dialog |
| Update Table, Remove Table | rewrites or takes it away | ≈ Word asks whether to update page numbers only |
| Insert Footnote, Endnote | starts one and puts the caret in it | ✓ |
| Next Footnote | goes to the next note | ≈ Word drops a menu of four ways to step |
| Delete Note | takes the note at the caret away | — Word has no such button; this is an addition |
| Insert Citation, Add Source, Manage Sources | a strip each, and the sources are kept in the document | ≈ Word has citation styles — APA, MLA, Chicago — and this has one |
| Bibliography | writes one from the sources | ✓ |
| Insert Caption | a strip: label, number, where it goes | ✓ |
| Cross-reference, Page Reference | a list of what can be pointed at | ✓ |
| Table of Figures | writes one from the captions | ✓ |
| Mark Entry, Insert Index, Update Index | marks and writes the index | ✓ |
| Mark Citation, Table of Authorities | marks and writes the table | ✓ |

## Mailings

| Button | What it does | Beside Word's |
| --- | --- | --- |
| Envelopes, Labels | a strip each, and a document is made | ≈ Word's dialogs know printer trays and label makers |
| Start Merge | letters, envelopes, labels or a directory | ✓ |
| Select Recipients | a file of recipients, read into the document | ≈ Word reads Outlook contacts and databases as well |
| Edit List | lists the recipients and edits them | ✓ |
| Highlight Merge Fields | shades them so they can be seen | ✓ |
| Address Block, Greeting Line | inserts the field | ≈ Word's dialogs preview and choose the form of the name |
| Insert Merge Field | a list of the columns | ✓ |
| Rules | the merge rules a field can carry | ✓ |
| Match Fields | maps the columns onto the names Word uses | ✓ |
| Preview Results, Previous, Next | shows one recipient's letter | ✓ |
| Check for Errors | reports what would go wrong | ✓ |
| Finish & Merge | writes the merged document | ≈ Word can also print or send as mail |

## Review

| Button | What it does | Beside Word's |
| --- | --- | --- |
| Spelling | goes to the next mistake and offers corrections | ≈ Word's Editor is a pane, with grammar and style |
| Show Marks | the wavy lines, on or off | ✓ |
| Word List | reads a dictionary from a file | ≈ Word manages several custom dictionaries in a dialog |
| Word Count | says how many words | ≈ Word's dialog counts pages, characters, paragraphs and lines |
| Check Accessibility | lists what would stop somebody reading it | ✓ |
| Translate | translates the selection | ≈ Word translates through a service; this does what it can offline |
| Language | sets the language of the selection | ≈ Word has proofing language and language preferences |
| New, Delete, Previous, Next, Show Comments | the comment machinery | ✓ |
| Track Changes | records what is edited | ≈ Word's button drops a menu, and can lock tracking with a password |
| Show Markup | shows or hides what was tracked | ≈ Word's menu chooses what to show: insertions, formatting, whose |
| Reviewing Pane | the comments pane | ≈ Word's pane lists every change as well |
| Accept, Reject | the change at the caret | ≈ Word's buttons drop menus — accept and move to next, accept all shown |
| Accept All, Reject All | every change in the document | ✓ |
| Compare | compares with another document and marks the differences | ≈ Word also combines documents and has a dialog of options |
| Block Authors, Restrict Editing | protection | ≈ Word's is a pane, with passwords and exceptions: **J1** |

## View

| Button | What it does | Beside Word's |
| --- | --- | --- |
| Read Mode, Print Layout, Web Layout, Draft | the view modes | ✓ |
| Outline | the outline view, with levels | ✓ |
| Switch Modes | dark and light | — Word has this in Word 365 as well |
| Hide White Space | joins the pages | ✓ |
| Side to Side | pages side by side rather than down | ✓ |
| Ruler, Gridlines, Navigation Pane | on or off | ✓ |
| Zoom, 100%, One Page, Page Width | the zoom | ≈ Word's Zoom button opens a dialog with many pages |
| New Window, Arrange All, Split | windows | ≈ Word also has View Side by Side, Synchronous Scrolling and Switch Windows |
| Macros | lists what was recorded | ≈ Word records macros and edits them in Basic: **J7** |

## Help

| Button | What it does | Beside Word's |
| --- | --- | --- |
| Help, Training, What's New | a message | ≈ Word opens its help |
| Feedback | a message | ≈ Word sends it |

## Table Design (while the caret is in a table)

| Button | What it does | Beside Word's |
| --- | --- | --- |
| Header Row | **nothing**, and says so | ✗ **C15** |
| Banded Rows | **nothing**, and says so | ✗ **C15** |
| All, Outside, None borders | sets the table's borders | ✓ |

Word's tab also has the table styles gallery, shading, the border styles and the
border painter, and the first-column and banded-column switches. All of that is
**C15**.

## Table Layout (while the caret is in a table)

| Button | What it does | Beside Word's |
| --- | --- | --- |
| Insert Above, Below, Left, Right | inserts the row or column | ✓ |
| Delete Row, Column, Table | takes it away | ≈ Word's Delete is one button with a menu |
| Merge Cells, Split Cells | joins and divides | ≈ Word's Split Cells asks how many |
| Distribute Columns | evens them out | ✓ |
| Properties | the table properties strip | ≈ Word's dialog has five tabs: **C6** |
| Align left, centre, right | the text in the cells | ≈ Word has nine — three across by three down |

Word's tab also has Select, View Gridlines, Draw Table and Eraser, AutoFit,
height and width boxes, Text Direction, Cell Margins, Sort, Repeat Header Rows,
Convert to Text and Formula. That is **C16**.

## Header & Footer (while one is being edited)

| Button | What it does | Beside Word's |
| --- | --- | --- |
| Header, Footer | the galleries again | ✓ |
| Page Number, Format Page Numbers | as on Insert | ✓ |
| Date & Time, Document Info, Pictures | inserts into the header | ✓ |
| Go to Header, Go to Footer | moves between them | ✓ |
| Link to Previous | this section's header follows the last | ✓ |
| Different First Page, Different Odd & Even | the two variants | ✓ |
| Close Header and Footer | back to the document | ✓ |

Word also has Header from Top and Footer from Bottom, and Insert Alignment Tab.
That is **C17**.

---

## What this became

Everything above that is not a **✓** is one of these, and every one of them is in
the roadmap:

- **C2** the Font dialog · **C3** the Paragraph dialog · **C4** the Styles pane ·
  **C5** Symbol · **C6** Table Properties · **C7** Options · **C8** the File
  backstage · **C9** dialogs instead of strips
- **C10** the paste Word has
- **C11** the menus behind the buttons — bullets, numbering, multilevel, line
  spacing, change case, page number, select, next footnote, accept and reject,
  bring forward and send backward, track changes, show markup
- **C12** the boxes on Layout that cannot be typed into
- **C13** Design ▸ Paragraph Spacing does the wrong thing
- **C14** Page Borders opens the paragraph border list
- **C15** the Table Design tab
- **C16** the rest of the Table Layout tab
- **C17** the rest of the Header & Footer tab
