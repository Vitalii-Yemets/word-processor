//! WordprocessingML documents: reading, creating and editing a `.docx`.
//!
//! # How editing stays safe
//!
//! An opened document is held as an element tree that keeps everything it was
//! given — every element, attribute and comment, understood or not. An edit
//! changes the nodes it must and leaves the rest alone, so saving writes back a
//! document that differs only where the user changed it.
//!
//! This matters more than it sounds. A real `.docx` carries a macro project, an
//! embedded font, a chart, a content control, somebody else's tracked changes. A
//! model that understood only what it knew about would throw the rest away the
//! moment the user pressed save.
//!
//! A document that is opened and saved without being edited comes back byte for
//! byte identical, because nothing is re-serialized at all.
//!
//! # Example
//!
//! ```
//! use wp_docx::{Document, model::{Block, Body, Paragraph}};
//!
//! let mut body = Body::default();
//! body.blocks.push(Block::Paragraph(Paragraph::text("Hello, world")));
//! let bytes = Document::create(&body)?.save()?;
//!
//! // Reopen it and change one word.
//! let mut document = Document::open(&bytes)?;
//! assert_eq!(document.replace_text("world", "everyone"), 1);
//! assert_eq!(document.plain_text(), "Hello, everyone");
//! # Ok::<(), wp_docx::Error>(())
//! ```

#![forbid(unsafe_code)]

pub mod accessibility;
pub mod anchor;
pub mod appearance;
pub mod authorities;
pub mod bibliography;
pub mod bookmarks;
pub mod captions;
pub mod cells;
pub mod chart;
pub mod clipboard;
pub mod comments;
pub mod compare;
pub mod contents;
pub mod cover;
pub mod diagram;
pub mod edit;
pub mod effects;
pub mod fields;
pub mod figures;
mod format;
pub mod furniture;
pub mod gallery;
mod history;
pub mod languages;
pub mod links;
pub mod math;
pub mod merge;
pub mod model;
pub mod notes;
pub mod numbering;
pub mod page;
pub mod permissions;
pub mod position;
pub mod proofing;
pub mod properties;
mod read;
pub mod revisions;
pub mod rules;
pub mod search;
pub mod sections;
pub mod settings;
pub mod shapes;
pub mod signature;
pub mod stationery;
pub mod styles;
pub mod table_properties;
pub mod tables;
pub mod theme;
pub mod translate;
pub mod watermark;
pub mod words;

use history::History;
use wp_opc::{Package, Relationships, TargetMode};
use wp_xml::tree::{Element, XmlTree};

pub use format::CharacterFormat;
pub use history::EditKind;
pub use model::Body;
pub use numbering::{ListCounters, Numbering};
pub use position::TextPosition;
pub use read::W as WORDPROCESSING_NAMESPACE;
pub use styles::{Style, StyleKind, Styles};

use model::{
    nearest_stop, Alignment, Block, BreakKind, LineSpacing, NumberingReference, Paragraph,
    ParagraphBorders, ResolvedParagraphProperties, ResolvedRunProperties, Run, RunProperties,
    TabStop, Table, TableCell, TableRow,
};

/// Content type of the styles part.
const STYLES_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml";

/// Relationship type of the styles part.
const STYLES_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles";

/// Content type of the settings part.
const SETTINGS_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml";

/// Relationship type of the settings part.
const SETTINGS_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings";

/// Relationship type of the numbering part, which holds the list definitions.
const NUMBERING_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering";

/// Relationship type of an embedded picture.
const IMAGE_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";

/// Content type of the numbering part.
const NUMBERING_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml";

/// The list a created document uses for bullets.
pub const BULLET_LIST: i32 = 1;
/// The list a created document uses for numbers.
pub const NUMBERED_LIST: i32 = 2;
/// How many levels deep those lists are defined.
pub const LIST_LEVELS: i32 = 3;

/// Page width of A4 in twentieths of a point, the unit the format uses.
const A4_WIDTH_TWIPS: &str = "11906";
/// Page height of A4 in the same unit.
const A4_HEIGHT_TWIPS: &str = "16838";
/// English metric units in one inch: the unit DrawingML measures a picture in.
pub const EMU_PER_INCH: i64 = 914_400;

/// One inch of margin, in the same unit.
const MARGIN_TWIPS: &str = "1440";

/// Why a document could not be read or written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The package layer could not read or write the file.
    Package(wp_opc::Error),
    /// A part is not valid XML.
    Xml { part: String, source: wp_xml::Error },
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Package(error) => write!(f, "{error}"),
            Self::Xml { part, source } => write!(f, "part {part:?} is not valid XML: {source}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<wp_opc::Error> for Error {
    fn from(error: wp_opc::Error) -> Self {
        Self::Package(error)
    }
}

/// An open document.
#[derive(Clone, Debug)]
pub struct Document {
    package: Package,
    main_part: String,
    /// The document part itself, which `main_part` is normally the same as.
    ///
    /// They differ while a header or a footer is being edited: `main_part` is
    /// then that part, and this is what to go back to.
    document_part: String,
    tree: XmlTree,
    /// The document's style definitions, read once when it is opened.
    styles: Styles,
    /// The list definitions, read once alongside the styles.
    ///
    /// A paragraph names a list and a level; what that means lives here.
    numbering: Numbering,
    /// Where the caret is, and the other end of the selection if there is one.
    ///
    /// The caret lives here rather than in the interface so that undo can put
    /// it back where it was: restoring the text without the caret leaves it
    /// somewhere the user did not leave it.
    caret: TextPosition,
    anchor: Option<TextPosition>,
    /// Formatting chosen with nothing selected, waiting for the next thing
    /// typed.
    ///
    /// Pressing Ctrl+B before writing a word has to mean something, and the
    /// only thing it can mean is that the word about to be written is bold. It
    /// is dropped as soon as the caret is moved somewhere else, because by then
    /// it is about a place the user has left.
    pending: RunProperties,
    history: History,
    /// Whether the tree has been changed since it was read.
    ///
    /// While it is false, saving writes the original bytes straight back, which
    /// is what makes an untouched document come out identical.
    modified: bool,
    /// Whether every edit is recorded as a tracked change.
    ///
    /// Read from the settings when the document is opened and kept here: it is
    /// consulted on every keystroke, and re-reading a part of the package that
    /// often would be felt.
    pub(crate) tracking: bool,
    /// Whose name goes on the changes this program records.
    pub(crate) reviser: revisions::Reviser,
    /// How many gestures are under way, and whether the outermost has been
    /// noted yet. Counted rather than a flag, because gestures nest.
    ///
    /// See [`Document::begin_gesture`].
    gesture_depth: usize,
    gesture_noted: bool,
}

impl Document {
    /// Opens a `.docx` from its bytes.
    pub fn open(bytes: &[u8]) -> Result<Self, Error> {
        let package = Package::open(bytes)?;
        let main_part = package.main_document_part()?;

        let text = package
            .xml_part(&main_part)
            .ok_or_else(|| wp_opc::Error::MissingPart(main_part.clone()))??;
        let tree = XmlTree::parse(&text)
            .map_err(|source| Error::Xml { part: main_part.clone(), source })?;

        // The theme first: the styles resolve names against it, so it has to
        // exist before they are read.
        let theme = read_theme(&package, &main_part);
        let styles = read_styles(&package, &main_part, theme);
        let numbering = read_numbering(&package, &main_part);

        let mut document = Self {
            package,
            document_part: main_part.clone(),
            main_part,
            tree,
            styles,
            numbering,
            caret: TextPosition::default(),
            anchor: None,
            pending: RunProperties::default(),
            history: History::default(),
            modified: false,
            tracking: false,
            reviser: revisions::Reviser::default(),
            gesture_depth: 0,
            gesture_noted: false,
        };
        document.tracking = document.read_tracking_setting();
        Ok(document)
    }

    /// Builds a new document containing the given body.
    pub fn create(body: &Body) -> Result<Self, Error> {
        let tree = build_document(body);
        let xml = tree
            .to_xml()
            .map_err(|source| Error::Xml { part: "word/document.xml".to_owned(), source })?;

        let mut package = Package::empty();
        package.add_part("word/document.xml", wp_opc::MAIN_DOCUMENT_CONTENT_TYPE, xml.into_bytes());
        package.add_part("word/styles.xml", STYLES_CONTENT_TYPE, default_styles().into_bytes());
        package.add_part(
            "word/settings.xml",
            SETTINGS_CONTENT_TYPE,
            default_settings().into_bytes(),
        );
        package.add_part(
            "word/numbering.xml",
            NUMBERING_CONTENT_TYPE,
            default_numbering().into_bytes(),
        );

        // A package is navigated by relationships, not by filenames, so the main
        // document has to be pointed at from the package root.
        let mut root = Relationships::new("");
        root.add(wp_opc::OFFICE_DOCUMENT_RELATIONSHIP, "word/document.xml", TargetMode::Internal);
        package.set_relationships(&root)?;

        let mut document_relationships = Relationships::new("word/document.xml");
        document_relationships.add(STYLES_RELATIONSHIP, "styles.xml", TargetMode::Internal);
        document_relationships.add(SETTINGS_RELATIONSHIP, "settings.xml", TargetMode::Internal);
        document_relationships.add(NUMBERING_RELATIONSHIP, "numbering.xml", TargetMode::Internal);
        package.set_relationships(&document_relationships)?;

        let styles = read_styles(&package, "word/document.xml", crate::theme::Theme::default());
        let numbering = read_numbering(&package, "word/document.xml");

        Ok(Self {
            package,
            main_part: "word/document.xml".to_owned(),
            document_part: "word/document.xml".to_owned(),
            tree,
            styles,
            numbering,
            caret: TextPosition::default(),
            anchor: None,
            pending: RunProperties::default(),
            history: History::default(),
            modified: false,
            tracking: false,
            reviser: revisions::Reviser::default(),
            gesture_depth: 0,
            gesture_noted: false,
        })
    }

    /// The package behind the document, for inspecting its parts.
    #[must_use]
    /// The package, so parts beside the main one can be changed.
    pub(crate) fn package_mut(&mut self) -> &mut Package {
        &mut self.package
    }

    pub fn package(&self) -> &Package {
        &self.package
    }

    /// The name of the main document part.
    #[must_use]
    pub fn main_part(&self) -> &str {
        &self.main_part
    }

    /// The element tree of the main document part.
    #[must_use]
    pub fn tree(&self) -> &XmlTree {
        &self.tree
    }

    /// The element tree, for edits this crate does not offer directly.
    ///
    /// Taking this marks the document as changed, since there is no way to know
    /// afterwards whether it was.
    pub fn tree_mut(&mut self) -> &mut XmlTree {
        self.modified = true;
        &mut self.tree
    }

    /// Whether the document has been changed since it was opened.
    #[must_use]
    pub fn is_modified(&self) -> bool {
        self.modified
    }

    /// The document's content, as blocks.
    ///
    /// The part being edited, which is normally the document and is the header
    /// or the footer while one of those is open — and those have no `w:body`,
    /// so the blocks are read from whatever the root of the part turns out to
    /// be. See [`read::read_part`].
    #[must_use]
    pub fn body(&self) -> Body {
        read::read_part(&self.tree.root)
    }

    /// The whole document's text, with formatting removed.
    #[must_use]
    pub fn plain_text(&self) -> String {
        self.body().plain_text()
    }

    /// The document's list definitions.
    #[must_use]
    pub fn numbering(&self) -> &Numbering {
        &self.numbering
    }

    /// The bytes of an embedded picture, by the relationship it is reached
    /// through.
    ///
    /// The bytes rather than a decoded picture: this layer knows about
    /// packages and relationships, and nothing about PNG or JPEG.
    #[must_use]
    pub fn embedded_part(&self, relationship_id: &str) -> Option<&[u8]> {
        let relationships = self.package.relationships(&self.main_part).ok()?;
        let relationship = relationships.by_id(relationship_id)?;
        let target = relationship.resolved_target(&self.main_part)?.ok()?;
        self.package.part(&target)
    }

    /// The document's style definitions.
    #[must_use]
    pub fn styles(&self) -> &Styles {
        &self.styles
    }

    /// The styles, to be changed.
    pub(crate) fn styles_mut(&mut self) -> &mut Styles {
        &mut self.styles
    }

    /// What a paragraph's formatting actually is, once its style chain and the
    /// document defaults have been applied.
    #[must_use]
    pub fn resolve_paragraph(&self, paragraph: &Paragraph) -> ResolvedParagraphProperties {
        self.styles.resolve_paragraph(&paragraph.properties)
    }

    /// What a run's formatting actually is.
    ///
    /// The paragraph is needed as well as the run: a run inside a heading is
    /// bold because the *paragraph* style says so, not because the run does.
    #[must_use]
    pub fn resolve_run(&self, paragraph: &Paragraph, run: &Run) -> ResolvedRunProperties {
        self.styles.resolve_run(paragraph.style(), &run.properties)
    }

    /// The prefix this document uses for the WordprocessingML namespace.
    /// Marks the tree as changed, so saving writes it out again.
    pub(crate) fn mark_modified(&mut self) {
        self.modified = true;
    }

    pub(crate) fn prefix(&self) -> Option<String> {
        edit::prefix_for(&self.tree.root, WORDPROCESSING_NAMESPACE)
    }

    /// Replaces every occurrence of a string, returning how many were changed.
    ///
    /// The search works across run boundaries, which it has to: Word splits a
    /// paragraph's text between runs wherever formatting changes, so a word can
    /// easily be stored in two pieces.
    pub fn replace_text(&mut self, needle: &str, replacement: &str) -> usize {
        let replaced = edit::replace_text(&mut self.tree.root, needle, replacement);
        if replaced > 0 {
            self.modified = true;
        }
        replaced
    }

    /// How many paragraphs the document has, in reading order.
    #[must_use]
    pub fn paragraph_count(&self) -> usize {
        position::paragraph_count(&self.tree.root)
    }

    /// The text of one paragraph, measured the way a [`TextPosition`] is.
    #[must_use]
    pub fn paragraph_text(&self, index: usize) -> Option<String> {
        position::text_of(&self.tree.root, index)
    }

    /// Records the current state so a change can be taken back.
    ///
    /// Called before the tree is touched, never after: what undo restores is the
    /// state as it was, and after the change that state no longer exists.
    pub(crate) fn record(&mut self, kind: EditKind, ends_at: TextPosition, mergeable: bool) {
        // Inside a gesture only the first change is noted, so the whole of it
        // comes back in one step.
        let in_gesture = self.gesture_depth > 0;
        if in_gesture {
            if self.gesture_noted {
                return;
            }
            self.gesture_noted = true;
        }

        // Typing and deleting change one paragraph and nothing else, so one
        // paragraph is what is kept. A copy of the whole document per word
        // typed is fourteen megabytes on a thousand pages, and a person types
        // a word every second.
        //
        // Not inside a gesture, though: there only the first change is noted,
        // and the rest of the gesture may be anywhere in the document. What
        // one paragraph would keep is then not what undo has to put back.
        let one_paragraph = !in_gesture && matches!(kind, EditKind::Typing | EditKind::Deleting);
        let kept = one_paragraph
            .then(|| self.kept_paragraph(ends_at.paragraph))
            .flatten()
            .unwrap_or_else(|| history::Kept::Whole(self.tree.clone()));
        self.history.record(
            kept,
            &self.main_part,
            self.caret,
            self.modified,
            kind,
            ends_at,
            mergeable,
        );
    }

    /// One paragraph exactly as it is, for a step that changes only that one.
    fn kept_paragraph(&self, index: usize) -> Option<history::Kept> {
        let path = position::paragraph_path(&self.tree.root, index)?;
        let element = edit::element_at_path(&self.tree.root, &path)?.clone();
        Some(history::Kept::Paragraph { index, element: Box::new(element) })
    }

    /// The present state, kept the same way a step keeps it.
    ///
    /// Undo has to leave a way back, and the way back has to be of the same
    /// shape: a step that kept one paragraph is undone by putting that
    /// paragraph back, and redone by putting back the paragraph that is there
    /// now.
    fn kept_like(&self, shape: &history::Kept) -> history::Kept {
        match shape {
            history::Kept::Whole(_) => history::Kept::Whole(self.tree.clone()),
            history::Kept::Paragraph { index, .. } => self
                .kept_paragraph(*index)
                .unwrap_or_else(|| history::Kept::Whole(self.tree.clone())),
        }
    }

    /// Puts a kept state back where it came from.
    fn put_back(&mut self, kept: history::Kept) {
        match kept {
            history::Kept::Whole(tree) => self.tree = tree,
            history::Kept::Paragraph { index, element } => {
                let Some(path) = position::paragraph_path(&self.tree.root, index) else { return };
                if let Some(target) = edit::element_at_path_mut(&mut self.tree.root, &path) {
                    *target = *element;
                }
            }
        }
    }

    /// Begins a gesture: a run of changes that undo should treat as one.
    ///
    /// Dragging a marker on the ruler changes the document on every movement
    /// of the pointer. Somebody who drags it an inch and then presses undo
    /// means to put it back where it was, not to retrace the drag a hundredth
    /// of an inch at a time.
    ///
    /// # Why gestures nest
    ///
    /// Because the things they are made of are gestures too. Moving text is a
    /// deletion and a paste, and a paste is itself several paragraphs put in
    /// one after another — each of them wrapped, because each is one thing on
    /// its own. If the inner one ended the outer one, the second half of every
    /// compound edit would become its own undo step, and one undo would leave
    /// the document half changed. So they are counted: only the outermost end
    /// closes the gesture.
    pub fn begin_gesture(&mut self) {
        if self.gesture_depth == 0 {
            self.gesture_noted = false;
        }
        self.gesture_depth += 1;
    }

    /// Ends it. Changes after the outermost end are their own steps again.
    pub fn end_gesture(&mut self) {
        self.gesture_depth = self.gesture_depth.saturating_sub(1);
    }

    /// Puts text in, recording it as a tracked change when that is switched on.
    ///
    /// Every path that types goes through here, so a tracked change cannot be
    /// missed by one of them.
    pub(crate) fn write_text(&mut self, at: TextPosition, text: &str) -> bool {
        if self.tracking {
            let reviser = self.reviser.clone();
            return self.insert_tracked(at, text, &reviser);
        }
        let prefix = self.prefix();
        position::insert_text(&mut self.tree.root, at, text, prefix.as_deref())
    }

    /// Takes text out, or marks it as deleted when changes are being tracked.
    pub(crate) fn erase_range(&mut self, paragraph: usize, start: usize, end: usize) -> bool {
        if self.tracking {
            let reviser = self.reviser.clone();
            return self.delete_tracked(paragraph, start, end, &reviser);
        }
        position::delete_range(&mut self.tree.root, paragraph, start, end)
    }

    /// Inserts text at a position, as typing does.
    pub fn insert_text(&mut self, at: TextPosition, text: &str) -> bool {
        let prefix = self.prefix();
        self.record(EditKind::Structural, at, false);
        let _ = prefix;
        let changed = self.write_text(at, text);
        self.modified |= changed;
        changed
    }

    /// Removes a stretch of text from one paragraph.
    pub fn delete_range(&mut self, paragraph: usize, start: usize, end: usize) -> bool {
        self.record(EditKind::Structural, TextPosition::new(paragraph, start), false);
        let changed = self.erase_range(paragraph, start, end);
        self.modified |= changed;
        changed
    }

    /// Splits a paragraph in two, as pressing Enter does.
    pub fn split_paragraph(&mut self, at: TextPosition) -> bool {
        let prefix = self.prefix();
        self.record(EditKind::Structural, at, false);
        let changed = position::split_paragraph(&mut self.tree.root, at, prefix.as_deref());
        self.modified |= changed;
        changed
    }

    /// Joins a paragraph onto the one before it, as Backspace at its start does.
    pub fn merge_with_previous(&mut self, paragraph: usize) -> bool {
        self.record(EditKind::Structural, TextPosition::new(paragraph, 0), false);
        let changed = position::merge_with_previous(&mut self.tree.root, paragraph);
        self.modified |= changed;
        changed
    }

    // --- The caret and the selection ---------------------------------------

    /// Where the caret is.
    #[must_use]
    pub fn caret(&self) -> TextPosition {
        self.caret
    }

    /// Moves the caret, clearing any selection.
    ///
    /// Moving the caret also ends the current undo step: typing after moving
    /// somewhere else is a new thought, not a continuation of the last one.
    pub fn set_caret(&mut self, position: TextPosition) {
        self.caret = self.clamp(position);
        self.anchor = None;
        self.pending = RunProperties::default();
        self.history.break_merge();
    }

    /// Extends the selection to a position, keeping the other end where it is.
    pub fn extend_selection_to(&mut self, position: TextPosition) {
        if self.anchor.is_none() {
            self.anchor = Some(self.caret);
        }
        self.caret = self.clamp(position);
        self.pending = RunProperties::default();
        self.history.break_merge();
    }

    /// Selects everything.
    pub fn select_all(&mut self) {
        let last = self.paragraph_count().saturating_sub(1);
        self.anchor = Some(TextPosition::new(0, 0));
        self.caret = TextPosition::new(last, self.paragraph_text(last).unwrap_or_default().len());
        self.pending = RunProperties::default();
        self.history.break_merge();
    }

    /// Drops the selection, leaving the caret where it is.
    pub fn clear_selection(&mut self) {
        self.anchor = None;
    }

    /// The selected range, in document order, if anything is selected.
    #[must_use]
    pub fn selection(&self) -> Option<(TextPosition, TextPosition)> {
        let anchor = self.anchor?;
        if anchor == self.caret {
            return None;
        }
        Some(if anchor <= self.caret { (anchor, self.caret) } else { (self.caret, anchor) })
    }

    /// The selected text, with a line break between paragraphs.
    #[must_use]
    pub fn selected_text(&self) -> String {
        let Some((start, end)) = self.selection() else {
            return String::new();
        };

        if start.paragraph == end.paragraph {
            let text = self.paragraph_text(start.paragraph).unwrap_or_default();
            return slice(&text, start.offset, end.offset).to_owned();
        }

        let mut out = String::new();
        for index in start.paragraph..=end.paragraph {
            let text = self.paragraph_text(index).unwrap_or_default();
            let piece = if index == start.paragraph {
                slice(&text, start.offset, text.len())
            } else if index == end.paragraph {
                slice(&text, 0, end.offset)
            } else {
                &text
            };
            if index > start.paragraph {
                out.push('\n');
            }
            out.push_str(piece);
        }
        out
    }

    /// Removes whatever is selected, leaving the caret where it began.
    ///
    /// A selection spanning paragraphs takes the tail of the first, all of the
    /// ones between, and the head of the last, and then joins what remains —
    /// which is what deleting a stretch of text across a paragraph break means.
    pub fn delete_selection(&mut self) -> bool {
        let Some((start, end)) = self.selection() else {
            return false;
        };
        self.record(EditKind::Structural, start, false);
        let removed = self.remove_range(start, end);
        if removed {
            self.caret = start;
            self.anchor = None;
            self.modified = true;
        }
        removed
    }

    /// Deletes a range without recording history, for callers that already did.
    fn remove_range(&mut self, start: TextPosition, end: TextPosition) -> bool {
        if start.paragraph == end.paragraph {
            return self.erase_range(start.paragraph, start.offset, end.offset);
        }

        let first_length = self.paragraph_text(start.paragraph).unwrap_or_default().len();
        self.erase_range(start.paragraph, start.offset, first_length);
        self.erase_range(end.paragraph, 0, end.offset);

        // The paragraphs wholly inside the selection go from the back, so the
        // indices of those still to be removed do not shift.
        for index in (start.paragraph + 1..end.paragraph).rev() {
            position::remove_paragraph(&mut self.tree.root, index);
        }
        // What is left of the last paragraph joins what is left of the first.
        position::merge_with_previous(&mut self.tree.root, start.paragraph + 1);
        true
    }

    /// Keeps a position inside the document.
    fn clamp(&self, position: TextPosition) -> TextPosition {
        let count = self.paragraph_count();
        if count == 0 {
            return TextPosition::default();
        }
        let paragraph = position.paragraph.min(count - 1);
        let text = self.paragraph_text(paragraph).unwrap_or_default();
        let mut offset = position.offset.min(text.len());
        while offset > 0 && !text.is_char_boundary(offset) {
            offset -= 1;
        }
        TextPosition::new(paragraph, offset)
    }

    // --- Editing at the caret ----------------------------------------------

    /// Types text in, replacing the selection if there is one.
    pub fn type_text(&mut self, text: &str) -> bool {
        if text.is_empty() {
            return false;
        }

        // A tab and a line ending are not text: a tab is an element of its own,
        // and a line ending makes a new paragraph. Both go through the same
        // path a paste takes, so there is one place that knows how.
        if text.contains('\t') || text.contains('\n') || text.contains('\r') {
            return self.paste(text);
        }

        // Typing over a selection is one change, not two: taking it back has to
        // bring the replaced text straight back, the way it does in every other
        // editor. So the removal and the insertion share a single step.
        if let Some((start, end)) = self.selection() {
            self.record(EditKind::Structural, start, false);
            self.remove_range(start, end);
            self.anchor = None;

            let prefix = self.prefix();
            let _ = &prefix;
            if self.write_text(start, text) {
                self.caret = TextPosition::new(start.paragraph, start.offset + text.len());
                self.apply_pending(start, self.caret);
            } else {
                self.caret = start;
            }
            self.modified = true;
            self.history.break_merge();
            return true;
        }

        let at = self.caret;
        let ends_at = TextPosition::new(at.paragraph, at.offset + text.len());
        // Typing runs together into one undo step. A space closes the step it
        // ends rather than opening a new one, so taking back "one two" leaves
        // "one " and then "one" — a word at a time, which is what a person
        // means by undo.
        self.record(EditKind::Typing, ends_at, true);

        let prefix = self.prefix();
        let _ = &prefix;
        if !self.write_text(at, text) {
            return false;
        }
        self.apply_pending(at, ends_at);
        if text.contains(char::is_whitespace) {
            self.history.break_merge();
        }
        self.caret = ends_at;
        self.anchor = None;
        self.modified = true;
        true
    }

    /// Puts formatting chosen before typing onto the text just typed.
    ///
    /// The text is inserted into whatever run was already there and then split
    /// back out, rather than a new run being built for it. That keeps one code
    /// path for insertion, and the run the text lands in still contributes
    /// everything the pending change does not mention — its font, its language,
    /// its colour.
    fn apply_pending(&mut self, start: TextPosition, end: TextPosition) {
        if self.pending.is_empty() || start.paragraph != end.paragraph {
            return;
        }
        let change = self.pending.clone();
        let prefix = self.prefix();

        let Some(path) = position::paragraph_path(&self.tree.root, start.paragraph) else {
            return;
        };
        let Some(paragraph) = edit::element_at_path_mut(&mut self.tree.root, &path) else {
            return;
        };
        format::apply_to_range(paragraph, start.offset, end.offset, &change, prefix.as_deref());
    }

    /// Removes the character before the caret, or joins onto the last paragraph.
    pub fn backspace(&mut self) -> bool {
        if self.selection().is_some() {
            return self.delete_selection();
        }

        if self.caret.offset > 0 {
            let text = self.paragraph_text(self.caret.paragraph).unwrap_or_default();
            let start = previous_boundary(&text, self.caret.offset);
            let ends_at = TextPosition::new(self.caret.paragraph, start);
            self.record(EditKind::Deleting, ends_at, true);

            if self.erase_range(self.caret.paragraph, start, self.caret.offset) {
                self.caret = ends_at;
                self.modified = true;
                return true;
            }
            return false;
        }

        if self.caret.paragraph == 0 {
            return false;
        }
        let previous = self.caret.paragraph - 1;
        let join_at =
            TextPosition::new(previous, self.paragraph_text(previous).unwrap_or_default().len());
        self.record(EditKind::Structural, join_at, false);

        if position::merge_with_previous(&mut self.tree.root, self.caret.paragraph) {
            self.caret = join_at;
            self.modified = true;
            return true;
        }
        false
    }

    /// Removes the character after the caret, or pulls up the next paragraph.
    pub fn delete_forward(&mut self) -> bool {
        if self.selection().is_some() {
            return self.delete_selection();
        }

        let text = self.paragraph_text(self.caret.paragraph).unwrap_or_default();
        if self.caret.offset < text.len() {
            let end = next_boundary(&text, self.caret.offset);
            self.record(EditKind::Deleting, self.caret, true);

            if self.erase_range(self.caret.paragraph, self.caret.offset, end) {
                self.modified = true;
                return true;
            }
            return false;
        }

        if self.caret.paragraph + 1 >= self.paragraph_count() {
            return false;
        }
        self.record(EditKind::Structural, self.caret, false);
        if position::merge_with_previous(&mut self.tree.root, self.caret.paragraph + 1) {
            self.modified = true;
            return true;
        }
        false
    }

    /// Splits the paragraph at the caret, as pressing Enter does.
    ///
    /// With something selected this replaces it with the break, and the two
    /// together are one change, so one undo brings the selection back.
    pub fn press_enter(&mut self) -> bool {
        let at = match self.selection() {
            Some((start, end)) => {
                self.record(EditKind::Structural, start, false);
                self.remove_range(start, end);
                self.anchor = None;
                self.caret = start;
                start
            }
            None => {
                let at = self.caret;
                self.record(EditKind::Structural, TextPosition::new(at.paragraph + 1, 0), false);
                at
            }
        };

        let prefix = self.prefix();
        if position::split_paragraph(&mut self.tree.root, at, prefix.as_deref()) {
            self.caret = TextPosition::new(at.paragraph + 1, 0);
            self.anchor = None;
            self.modified = true;
            return true;
        }
        false
    }

    /// Puts a block of text in at the caret, replacing the selection.
    ///
    /// This is what pasting does. Line breaks in the text become paragraph
    /// breaks, and the whole paste is one change however many paragraphs it
    /// makes — a paste is one thing a person did.
    pub fn paste(&mut self, text: &str) -> bool {
        if text.is_empty() {
            return false;
        }

        let start = match self.selection() {
            Some((start, end)) => {
                self.record(EditKind::Structural, start, false);
                self.remove_range(start, end);
                start
            }
            None => {
                self.record(EditKind::Structural, self.caret, false);
                self.caret
            }
        };
        self.caret = start;
        self.anchor = None;

        // Text from another program can carry either line ending, or both.
        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        let prefix = self.prefix();
        let mut changed = false;

        for (index, line) in normalized.split('\n').enumerate() {
            if index > 0
                && position::split_paragraph(&mut self.tree.root, self.caret, prefix.as_deref())
            {
                self.caret = TextPosition::new(self.caret.paragraph + 1, 0);
                changed = true;
            }
            // A tab within the line is an element of its own, so the line goes
            // in as pieces of text with tabs between them.
            for (piece_index, piece) in line.split('\t').enumerate() {
                if piece_index > 0
                    && position::insert_tab(&mut self.tree.root, self.caret, prefix.as_deref())
                {
                    self.caret.offset += 1;
                    changed = true;
                }
                if !piece.is_empty()
                    && position::insert_text(
                        &mut self.tree.root,
                        self.caret,
                        piece,
                        prefix.as_deref(),
                    )
                {
                    self.caret.offset += piece.len();
                    changed = true;
                }
            }
        }

        if changed {
            self.apply_pending(start, self.caret);
            self.modified = true;
        }
        changed
    }

    /// Moves the caret one character back, crossing into the paragraph before.
    pub fn caret_left(&mut self, extend: bool) {
        let text = self.paragraph_text(self.caret.paragraph).unwrap_or_default();
        let target = if self.caret.offset > 0 {
            TextPosition::new(self.caret.paragraph, previous_boundary(&text, self.caret.offset))
        } else if self.caret.paragraph > 0 {
            let previous = self.caret.paragraph - 1;
            TextPosition::new(previous, self.paragraph_text(previous).unwrap_or_default().len())
        } else {
            return;
        };
        self.move_caret(target, extend);
    }

    /// Moves the caret one character forward.
    pub fn caret_right(&mut self, extend: bool) {
        let text = self.paragraph_text(self.caret.paragraph).unwrap_or_default();
        let target = if self.caret.offset < text.len() {
            TextPosition::new(self.caret.paragraph, next_boundary(&text, self.caret.offset))
        } else if self.caret.paragraph + 1 < self.paragraph_count() {
            TextPosition::new(self.caret.paragraph + 1, 0)
        } else {
            return;
        };
        self.move_caret(target, extend);
    }

    /// Moves the caret to the start of the word before it, as `Ctrl+Left` does.
    pub fn word_left(&mut self, extend: bool) {
        let target = self.word_start_before(self.caret);
        if target == self.caret {
            return;
        }
        self.move_caret(target, extend);
    }

    /// Moves the caret to the start of the word after it, as `Ctrl+Right` does.
    pub fn word_right(&mut self, extend: bool) {
        let target = self.word_start_after(self.caret);
        if target == self.caret {
            return;
        }
        self.move_caret(target, extend);
    }

    /// Moves the caret to the start of the paragraph, or of the one before it.
    ///
    /// `Ctrl+Up` in the middle of a paragraph goes to its start; pressed again,
    /// having got there, it goes to the start of the one above. That is what
    /// makes holding it walk up the document rather than stick.
    pub fn paragraph_up(&mut self, extend: bool) {
        let target = if self.caret.offset > 0 {
            TextPosition::new(self.caret.paragraph, 0)
        } else if self.caret.paragraph > 0 {
            TextPosition::new(self.caret.paragraph - 1, 0)
        } else {
            return;
        };
        self.move_caret(target, extend);
    }

    /// Moves the caret to the start of the paragraph after it.
    pub fn paragraph_down(&mut self, extend: bool) {
        if self.caret.paragraph + 1 >= self.paragraph_count() {
            // The last paragraph has nowhere below it, so the end of it is as
            // far down as this can go.
            let end = self.paragraph_text(self.caret.paragraph).unwrap_or_default().len();
            if self.caret.offset == end {
                return;
            }
            self.move_caret(TextPosition::new(self.caret.paragraph, end), extend);
            return;
        }
        self.move_caret(TextPosition::new(self.caret.paragraph + 1, 0), extend);
    }

    /// Deletes back to the start of the word, as `Ctrl+Backspace` does.
    pub fn delete_word_back(&mut self) -> bool {
        if self.selection().is_some() {
            return self.delete_selection();
        }

        let start = self.word_start_before(self.caret);
        if start == self.caret {
            return false;
        }
        // At the start of a paragraph there is no word behind the caret, only
        // the break — and taking that out is what ordinary backspace does.
        if start.paragraph != self.caret.paragraph {
            return self.backspace();
        }

        // Not mergeable: each press is its own step, so one undo brings back
        // one word rather than everything that was rubbed out in a row.
        self.record(EditKind::Deleting, start, false);
        if self.erase_range(self.caret.paragraph, start.offset, self.caret.offset) {
            self.caret = start;
            self.anchor = None;
            self.modified = true;
            return true;
        }
        false
    }

    /// Deletes forward to the start of the next word, as `Ctrl+Delete` does.
    pub fn delete_word_forward(&mut self) -> bool {
        if self.selection().is_some() {
            return self.delete_selection();
        }

        let end = self.word_start_after(self.caret);
        if end == self.caret {
            return false;
        }
        if end.paragraph != self.caret.paragraph {
            return self.delete_forward();
        }

        self.record(EditKind::Deleting, self.caret, false);
        if self.erase_range(self.caret.paragraph, self.caret.offset, end.offset) {
            self.anchor = None;
            self.modified = true;
            return true;
        }
        false
    }

    /// Where the word before a position begins, crossing into the paragraph
    /// above when there is nothing before it in this one.
    #[must_use]
    fn word_start_before(&self, from: TextPosition) -> TextPosition {
        if from.offset == 0 {
            if from.paragraph == 0 {
                return from;
            }
            let previous = from.paragraph - 1;
            let length = self.paragraph_text(previous).unwrap_or_default().len();
            return TextPosition::new(previous, length);
        }
        let text = self.paragraph_text(from.paragraph).unwrap_or_default();
        TextPosition::new(from.paragraph, words::previous_word(&text, from.offset))
    }

    /// Where the word after a position begins, crossing into the paragraph
    /// below when there is nothing after it in this one.
    #[must_use]
    fn word_start_after(&self, from: TextPosition) -> TextPosition {
        let text = self.paragraph_text(from.paragraph).unwrap_or_default();
        if from.offset >= text.len() {
            if from.paragraph + 1 >= self.paragraph_count() {
                return from;
            }
            return TextPosition::new(from.paragraph + 1, 0);
        }
        TextPosition::new(from.paragraph, words::next_word(&text, from.offset))
    }

    /// Moves the caret, extending the selection or dropping it.
    pub fn move_caret(&mut self, target: TextPosition, extend: bool) {
        if extend {
            self.extend_selection_to(target);
        } else {
            self.set_caret(target);
        }
    }

    // --- Undo and redo ------------------------------------------------------

    #[must_use]
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    #[must_use]
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// How many steps could be taken back.
    #[must_use]
    pub fn undo_depth(&self) -> usize {
        self.history.depth()
    }

    /// Edits another part of the package — a header or a footer — in place of
    /// the main document.
    ///
    /// # Why the whole tree is swapped
    ///
    /// A header is a body of its own: paragraphs, runs, tables, the lot. Every
    /// command in this program works on `self.tree`, so pointing that at the
    /// header makes all of them work on the header, from typing a letter to
    /// putting a table in. The alternative — a second caret model that says
    /// which body it is in — would touch every one of those commands.
    ///
    /// The tree being left is written back into the package first, so nothing
    /// of it is lost, and the history remembers which part each step belongs
    /// to so that undo can come back here.
    pub fn enter_part(&mut self, part: &str) -> bool {
        if part == self.main_part {
            return false;
        }
        let Some(Ok(text)) = self.package.xml_part(part) else { return false };
        let Ok(tree) = XmlTree::parse(&text) else { return false };

        self.flush_part();
        self.main_part = part.to_owned();
        self.tree = tree;
        self.caret = TextPosition::new(0, 0);
        self.anchor = None;
        self.pending = RunProperties::default();
        true
    }

    /// Which part is being edited, when it is not the document itself.
    #[must_use]
    pub fn part_being_edited(&self) -> Option<&str> {
        (self.main_part != self.document_part).then_some(self.main_part.as_str())
    }

    /// Goes back to editing the document itself.
    pub fn leave_part(&mut self) -> bool {
        if self.main_part == self.document_part {
            return false;
        }
        let part = self.document_part.clone();
        self.enter_part(&part)
    }

    /// Writes the tree being edited back into the package.
    ///
    /// Only when something has changed: an untouched part is left byte for
    /// byte as it arrived, which is the promise the whole program makes.
    fn flush_part(&mut self) {
        if !self.modified {
            return;
        }
        if let Ok(xml) = self.tree.to_xml() {
            self.package.set_part(&self.main_part, xml.into_bytes());
        }
    }
    /// Takes back the last change.
    ///
    /// A step that belongs to another part of the package — a header, say —
    /// brings that part back with it, because undoing an edit means being where
    /// the edit was.
    pub fn undo(&mut self) -> bool {
        let Some(shape) = self.history.next_undo() else { return false };
        let now = self.kept_like(shape);
        let Some((kept, part, caret, modified)) =
            self.history.undo(now, &self.main_part, self.caret, self.modified)
        else {
            return false;
        };
        self.restore(kept, part, caret, modified);
        true
    }

    /// Puts back a change that was taken back.
    pub fn redo(&mut self) -> bool {
        let Some(shape) = self.history.next_redo() else { return false };
        let now = self.kept_like(shape);
        let Some((kept, part, caret, modified)) =
            self.history.redo(now, &self.main_part, self.caret, self.modified)
        else {
            return false;
        };
        self.restore(kept, part, caret, modified);
        true
    }

    /// Puts a remembered state back, moving to its part if that is not the one
    /// being edited.
    fn restore(&mut self, kept: history::Kept, part: String, caret: TextPosition, modified: bool) {
        if part != self.main_part {
            // The tree being left has to reach the package, or the step that
            // put it there would be lost.
            self.flush_part();
            self.main_part = part;
        }
        self.put_back(kept);
        self.caret = self.clamp(caret);
        self.anchor = None;
        self.pending = RunProperties::default();
        self.modified = modified;
    }

    // --- Formatting ---------------------------------------------------------

    /// Whether a character format is on for what is selected, or for what would
    /// be typed next.
    ///
    /// "On" over a selection means on throughout it. A selection that is partly
    /// bold reads as not bold, which is why pressing Ctrl+B over it makes all of
    /// it bold rather than swapping the two halves over.
    #[must_use]
    pub fn format_is_on(&self, format: CharacterFormat) -> bool {
        if let Some(state) = format.read(&self.pending) {
            return state;
        }

        if let Some((start, end)) = self.selection() {
            let mut seen = false;
            for index in start.paragraph..=end.paragraph {
                let Some(paragraph) = self.paragraph_element(index) else { continue };
                let (from, to) = self.range_within(index, start, end);
                for resolved in format::resolved_in_range(paragraph, from, to, &self.styles) {
                    seen = true;
                    if !format.is_on(&resolved) {
                        return false;
                    }
                }
            }
            if seen {
                return true;
            }
        }

        format.is_on(&self.resolved_at_caret())
    }

    /// Turns a character format on or off.
    ///
    /// Over a selection this rewrites the runs it covers. With nothing selected
    /// it is remembered for the next thing typed, which is how a person turns
    /// bold on and then writes the word.
    pub fn set_format(&mut self, format: CharacterFormat, on: bool) -> bool {
        let change = format.change(on);

        let Some((start, end)) = self.selection() else {
            if format.is_on(&self.resolved_at_caret()) == on {
                // Asking for what the text here already is: nothing needs
                // remembering, and the next thing typed simply inherits it.
                format.clear(&mut self.pending);
            } else {
                self.pending = self.pending.overlaid_with(&change);
            }
            return true;
        };

        // An undo step is only worth recording when there is something to take
        // back. Bolding text that is already bold changes nothing, and should
        // leave nothing behind either.
        let anything_to_do = (start.paragraph..=end.paragraph).any(|index| {
            let (from, to) = self.range_within(index, start, end);
            self.paragraph_element(index)
                .is_some_and(|paragraph| format::range_needs_change(paragraph, from, to, &change))
        });
        if !anything_to_do {
            return false;
        }

        self.record(EditKind::Structural, start, false);
        let prefix = self.prefix();
        let mut changed = false;

        for index in start.paragraph..=end.paragraph {
            let (from, to) = self.range_within(index, start, end);
            let Some(path) = position::paragraph_path(&self.tree.root, index) else { continue };
            let Some(paragraph) = edit::element_at_path_mut(&mut self.tree.root, &path) else {
                continue;
            };
            changed |= format::apply_to_range(paragraph, from, to, &change, prefix.as_deref());
        }

        if changed {
            self.modified = true;
        }
        changed
    }

    /// Turns a format on if any of the selection lacks it, off if all of it has
    /// it — which is what pressing Ctrl+B means.
    pub fn toggle_format(&mut self, format: CharacterFormat) -> bool {
        let on = !self.format_is_on(format);
        self.set_format(format, on)
    }

    /// Sets the font of the selection, or of what is typed next.
    pub fn set_font(&mut self, name: &str) -> bool {
        let change = RunProperties { font: Some(name.to_owned()), ..RunProperties::default() };
        self.apply_character_change(&change)
    }

    /// Sets the size in points of the selection, or of what is typed next.
    pub fn set_size(&mut self, points: f32) -> bool {
        // The format stores half-points, which is how it manages half sizes.
        let change = RunProperties {
            size_half_points: Some((points * 2.0).round().max(2.0) as u32),
            ..RunProperties::default()
        };
        self.apply_character_change(&change)
    }

    /// Sets the colour of the selection, or of what is typed next.
    ///
    /// `None` means "automatic", which is not a colour: it is an instruction to
    /// use whatever reads against the paper.
    pub fn set_color(&mut self, hex: Option<&str>) -> bool {
        let change = RunProperties {
            color: Some(hex.unwrap_or("auto").to_owned()),
            ..RunProperties::default()
        };
        self.apply_character_change(&change)
    }

    /// The colour in use where the caret is, after inheritance.
    #[must_use]
    pub fn color_here(&self) -> Option<String> {
        if let Some(pending) = &self.pending.color {
            return Some(pending.clone());
        }
        self.resolved_over_selection().color
    }

    /// Sets the colour drawn behind the selection.
    ///
    /// The format takes a name from a fixed list here rather than a colour, so
    /// "none" is what turns it off.
    pub fn set_highlight(&mut self, name: Option<&str>) -> bool {
        let change = RunProperties {
            highlight: Some(name.unwrap_or("none").to_owned()),
            ..RunProperties::default()
        };
        self.apply_character_change(&change)
    }

    /// The highlight where the caret is, if there is one.
    #[must_use]
    pub fn highlight_here(&self) -> Option<String> {
        if let Some(pending) = &self.pending.highlight {
            return Some(pending.clone()).filter(|name| name != "none");
        }
        self.resolved_over_selection().highlight.filter(|name| name != "none")
    }

    /// Draws the selection with a shadow, an outline, a glow or a reflection.
    ///
    /// Passing [`effects::Effect::None`] takes the effect off again.
    pub fn set_text_effect(&mut self, effect: effects::Effect) -> bool {
        // The namespace has to be declared before anything in it is written,
        // and declaring it costs nothing in a document that never uses one.
        if effect != effects::Effect::None {
            effects::declare_namespace(&mut self.tree_mut().root);
        }
        let change = RunProperties {
            effect: Some(effects::TextEffect::plain(effect)),
            ..RunProperties::default()
        };
        self.apply_character_change(&change)
    }

    /// The effect on the text where the caret is.
    #[must_use]
    pub fn text_effect_here(&self) -> effects::Effect {
        if let Some(pending) = &self.pending.effect {
            return pending.effect;
        }
        self.resolved_over_selection().effect.map_or(effects::Effect::None, |value| value.effect)
    }
    /// Everything the run at the caret is formatted with, for the format
    /// painter to carry to somewhere else.
    #[must_use]
    pub fn run_formatting_here(&self) -> RunProperties {
        let resolved = self.resolved_over_selection();
        RunProperties {
            style: None,
            bold: Some(resolved.bold),
            italic: Some(resolved.italic),
            strike: Some(resolved.strike),
            underline: Some(resolved.underline),
            size_half_points: Some(resolved.size_half_points),
            color: resolved.color,
            highlight: resolved.highlight,
            vertical_align: Some(resolved.vertical_align),
            font: resolved.font,
            right_to_left: Some(resolved.right_to_left),
            language: None,
            // The painter carries what a run looks like, not what it was named
            // after: the colour and the font have already been resolved, and
            // carrying the name too would let the theme change them back.
            color_theme: None,
            font_theme: None,
            // An effect is part of the look, so the painter carries it — and
            // carries "no effect" too, so that painting plain text over a
            // glowing word takes the glow off.
            effect: Some(resolved.effect.clone().unwrap_or_default()),
        }
    }

    /// Lays a whole set of run formatting over the selection.
    pub fn apply_run_formatting(&mut self, formatting: &RunProperties) -> bool {
        self.apply_character_change(formatting)
    }

    /// The font in use where the caret is, after inheritance.
    #[must_use]
    pub fn font_here(&self) -> Option<String> {
        if let Some(pending) = &self.pending.font {
            return Some(pending.clone());
        }
        self.resolved_over_selection().font
    }

    /// The size in points where the caret is, after inheritance.
    #[must_use]
    pub fn size_here(&self) -> f32 {
        if let Some(pending) = self.pending.size_half_points {
            return pending as f32 / 2.0;
        }
        self.resolved_over_selection().size_half_points as f32 / 2.0
    }

    /// The formatting of the selection, or of the caret when nothing is
    /// selected.
    ///
    /// A selection of several fonts reports the first: a toolbar has to show
    /// something, and the first is what the eye reaches on the left.
    fn resolved_over_selection(&self) -> ResolvedRunProperties {
        if let Some((start, end)) = self.selection() {
            for index in start.paragraph..=end.paragraph {
                let Some(paragraph) = self.paragraph_element(index) else { continue };
                let (from, to) = self.range_within(index, start, end);
                if let Some(first) =
                    format::resolved_in_range(paragraph, from, to, &self.styles).into_iter().next()
                {
                    return first;
                }
            }
        }
        self.resolved_at_caret()
    }

    /// Applies a character change to the selection, or remembers it for the
    /// next thing typed.
    fn apply_character_change(&mut self, change: &RunProperties) -> bool {
        let Some((start, end)) = self.selection() else {
            self.pending = self.pending.overlaid_with(change);
            return true;
        };

        let anything_to_do = (start.paragraph..=end.paragraph).any(|index| {
            let (from, to) = self.range_within(index, start, end);
            self.paragraph_element(index)
                .is_some_and(|paragraph| format::range_needs_change(paragraph, from, to, change))
        });
        if !anything_to_do {
            return false;
        }

        self.record(EditKind::Structural, start, false);
        let prefix = self.prefix();
        let mut changed = false;

        for index in start.paragraph..=end.paragraph {
            let (from, to) = self.range_within(index, start, end);
            let Some(path) = position::paragraph_path(&self.tree.root, index) else { continue };
            let Some(paragraph) = edit::element_at_path_mut(&mut self.tree.root, &path) else {
                continue;
            };
            changed |= format::apply_to_range(paragraph, from, to, change, prefix.as_deref());
        }

        if changed {
            self.modified = true;
        }
        changed
    }

    /// Sets the style of every paragraph the selection touches.
    ///
    /// `None` removes the style, leaving the paragraph on the document default.
    pub fn set_paragraph_style_here(&mut self, style: Option<&str>) -> bool {
        self.change_paragraphs(|paragraph, prefix| {
            format::set_paragraph_style(paragraph, style, prefix);
        })
    }

    /// Sets the alignment of every paragraph the selection touches.
    pub fn set_alignment_here(&mut self, alignment: Alignment) -> bool {
        self.change_paragraphs(|paragraph, prefix| {
            format::set_paragraph_alignment(paragraph, alignment, prefix);
        })
    }

    /// Makes every paragraph the selection touches an item of a list, or takes
    /// it out of one.
    pub fn set_list_here(&mut self, list: Option<NumberingReference>) -> bool {
        self.change_paragraphs(|paragraph, prefix| {
            format::set_paragraph_numbering(paragraph, list, prefix);
        })
    }

    /// Which list the paragraph at the caret belongs to, if any.
    #[must_use]
    pub fn list_here(&self) -> Option<NumberingReference> {
        self.resolve_paragraph_here().numbering
    }

    /// Puts borders round every paragraph the selection touches.
    pub fn set_borders_here(&mut self, borders: &ParagraphBorders) -> bool {
        let borders = borders.clone();
        self.change_paragraphs(move |paragraph, prefix| {
            format::set_paragraph_borders(paragraph, &borders, prefix);
        })
    }

    /// The borders on the paragraph at the caret, after inheritance.
    #[must_use]
    pub fn borders_here(&self) -> ParagraphBorders {
        self.resolve_paragraph_here().borders
    }

    /// Sets the colour behind every paragraph the selection touches.
    pub fn set_shading_here(&mut self, fill: Option<&str>) -> bool {
        let fill = fill.map(str::to_owned);
        self.change_paragraphs(move |paragraph, prefix| {
            format::set_paragraph_shading(paragraph, fill.as_deref(), prefix);
        })
    }

    /// The colour behind the paragraph at the caret, if it has one.
    #[must_use]
    pub fn shading_here(&self) -> Option<String> {
        self.resolve_paragraph_here().shading
    }

    /// Moves every paragraph the selection touches in or out by an amount, in
    /// twentieths of a point.
    ///
    /// Never past the margin: an indent that has reached zero stops there
    /// rather than pushing the text off the left of the page.
    pub fn adjust_indent_here(&mut self, twips: i32) -> bool {
        let current = self.resolve_paragraph_here().indent_start;
        let wanted = (current + twips).max(0);
        if wanted == current {
            return false;
        }
        self.change_paragraphs(|paragraph, prefix| {
            format::set_paragraph_indent(paragraph, wanted, prefix);
        })
    }

    /// Sets all three indents of every paragraph the selection touches.
    ///
    /// This is what dragging a marker on the ruler does. The three are set
    /// together because they are one measurement to a reader — where the text
    /// begins, where its first line begins, and where it ends — and setting one
    /// at a time would need three undo steps for one drag.
    pub fn set_indents_here(&mut self, start: i32, first_line: i32, end: i32) -> bool {
        // Never past the left edge of the paper. The first line may still hang
        // outwards, so it is clamped against the indent rather than against
        // nothing, which is the rule Word follows too.
        let start = start.max(0);
        let first_line = first_line.max(-start);
        let end = end.max(0);
        if (start, first_line, end) == self.indents_here() {
            return false;
        }
        self.change_paragraphs(|paragraph, prefix| {
            format::set_paragraph_indents(paragraph, start, first_line, end, prefix);
        })
    }
    /// Sets the line spacing of every paragraph the selection touches.
    pub fn set_line_spacing_here(&mut self, spacing: Option<LineSpacing>) -> bool {
        self.change_paragraphs(|paragraph, prefix| {
            format::set_paragraph_line_spacing(paragraph, spacing, prefix);
        })
    }

    /// The line spacing of the paragraph at the caret, after inheritance.
    #[must_use]
    pub fn line_spacing_here(&self) -> Option<LineSpacing> {
        self.resolve_paragraph_here().line_spacing
    }

    /// Everything the paragraph at the caret resolves to.
    fn resolve_paragraph_here(&self) -> ResolvedParagraphProperties {
        let Some(paragraph) = self.paragraph_element(self.caret.paragraph) else {
            return self.styles.resolve_paragraph(&Default::default());
        };
        let direct = paragraph
            .child(Some(read::W), "pPr")
            .map(read::read_paragraph_properties)
            .unwrap_or_default();
        self.styles.resolve_paragraph(&direct)
    }

    /// Takes the direct formatting off the selection.
    ///
    /// Only what was written on the runs themselves: the paragraph's style
    /// stays, because clearing formatting means undoing what was applied by
    /// hand, not turning a heading into body text.
    pub fn clear_formatting(&mut self) -> bool {
        let Some((start, end)) = self.selection() else {
            self.pending = RunProperties::default();
            return true;
        };

        self.record(EditKind::Structural, start, false);
        let mut changed = false;
        for index in start.paragraph..=end.paragraph {
            let (from, to) = self.range_within(index, start, end);
            let Some(path) = position::paragraph_path(&self.tree.root, index) else { continue };
            let Some(paragraph) = edit::element_at_path_mut(&mut self.tree.root, &path) else {
                continue;
            };
            changed |= format::clear_run_properties(paragraph, from, to);
        }

        if changed {
            self.modified = true;
        }
        changed
    }

    /// Every place a string appears in the document, in reading order.
    ///
    /// Each is where the match starts and where it ends, which are not the
    /// start and the start plus the needle's length: a match found without
    /// minding capitals or how an accent was written may be a different length
    /// from what was typed into the search box.
    #[must_use]
    pub fn find_all(&self, needle: &str, how: search::Matching) -> Vec<(TextPosition, usize)> {
        let mut out = Vec::new();
        for index in 0..self.paragraph_count() {
            let Some(text) = self.paragraph_text(index) else { continue };
            for found in search::matches(&text, needle, how) {
                out.push((TextPosition::new(index, found.start), found.end));
            }
        }
        out
    }

    /// Finds the next occurrence of a string after a position, wrapping round.
    ///
    /// Wrapping because a search that stopped at the end of the document would
    /// miss what is behind the caret, which is where a person often is when
    /// they start looking.
    #[must_use]
    pub fn find_after(&self, needle: &str, after: TextPosition) -> Option<TextPosition> {
        if needle.is_empty() {
            return None;
        }
        let count = self.paragraph_count();

        // Everything from the caret onwards, then everything before it.
        for step in 0..=count {
            let index = (after.paragraph + step) % count.max(1);
            let text = self.paragraph_text(index)?;
            let from = if step == 0 { after.offset.min(text.len()) } else { 0 };
            let found = search::matches(&text, needle, search::Matching::default())
                .into_iter()
                .find(|found| found.start >= from);
            if let Some(found) = found {
                return Some(TextPosition::new(index, found.start));
            }
        }
        None
    }

    /// Replaces every match of a string, and says how many there were.
    ///
    /// Unlike [`Document::replace_text`], which matches the bytes exactly, this
    /// replaces what a search finds — the same matches, capitals and accents
    /// and all, that the person was shown before they pressed the button.
    ///
    /// The replacing runs backwards through the document so that each edit
    /// leaves the offsets of the matches before it alone, and the whole of it
    /// is one gesture, so one press of undo takes all of it back.
    pub fn replace_matching(
        &mut self,
        needle: &str,
        replacement: &str,
        how: search::Matching,
    ) -> usize {
        let found = self.find_all(needle, how);
        if found.is_empty() {
            return 0;
        }

        self.begin_gesture();
        let mut replaced = 0;
        for (at, end) in found.into_iter().rev() {
            if !self.delete_range(at.paragraph, at.offset, end) {
                continue;
            }
            if !replacement.is_empty() && !self.insert_text(at, replacement) {
                continue;
            }
            replaced += 1;
        }
        self.end_gesture();

        // The caret may have been sitting inside something that is no longer
        // there, so it is put somewhere that certainly exists.
        let caret = self.caret();
        let length = self.paragraph_text(caret.paragraph).unwrap_or_default().len();
        if caret.offset > length {
            self.set_caret(TextPosition::new(caret.paragraph, length));
        }
        replaced
    }

    /// Puts a table of empty cells after the paragraph the caret is in.
    ///
    /// A table goes between paragraphs, not inside one: it is a block, and the
    /// paragraph the caret sits in stays whole. A paragraph is added after it
    /// as well, because a document that ends in a table has nowhere to put the
    /// caret afterwards.
    pub fn insert_table(&mut self, rows: usize, columns: usize) -> bool {
        let rows = rows.clamp(1, 100);
        let columns = columns.clamp(1, 30);

        // The grid divides the text width evenly, in twentieths of a point.
        let text_width = 9360;
        let column_width = text_width / columns as i32;

        let table = Table::from_rows(
            (0..rows)
                .map(|_| TableRow::from_cells((0..columns).map(|_| TableCell::default()).collect()))
                .collect(),
        )
        .with_grid(vec![column_width; columns])
        .with_borders(model::TableBorders::grid());

        self.record(EditKind::Structural, self.caret, false);

        let prefix = self.prefix();
        let Some(path) = position::paragraph_path(&self.tree.root, self.caret.paragraph) else {
            return false;
        };
        let Some((position, parent_path)) = path.split_last() else { return false };
        let position = *position;
        let parent_path = parent_path.to_vec();
        let Some(parent) = edit::element_at_path_mut(&mut self.tree.root, &parent_path) else {
            return false;
        };

        parent.insert_element(
            position + 1,
            edit::paragraph_element(&Paragraph::default(), prefix.as_deref()),
        );
        parent.insert_element(position + 1, edit::table_element(&table, prefix.as_deref()));

        // The caret goes into the first cell, which is where a person expects
        // to start typing.
        self.caret = TextPosition::new(self.caret.paragraph + 1, 0);
        self.anchor = None;
        self.modified = true;
        true
    }

    /// Puts a picture at the caret.
    ///
    /// The bytes become a part of the package, a relationship points the
    /// document at them, and a drawing in the text points at the relationship.
    /// All three are needed: a picture is not something a paragraph contains,
    /// it is something a paragraph refers to.
    /// The size is given in English metric units, the unit DrawingML measures
    /// a picture in: there are [`EMU_PER_INCH`] of them to an inch. Deciding it
    /// belongs to whoever knows how big the picture is and how much room the
    /// text has, which is not this layer.
    pub fn insert_picture(
        &mut self,
        bytes: &[u8],
        extension: &str,
        width_emu: i64,
        height_emu: i64,
    ) -> Result<bool, Error> {
        let content_type = match extension.to_ascii_lowercase().as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "bmp" => "image/bmp",
            "tif" | "tiff" => "image/tiff",
            other => {
                let _ = other;
                "application/octet-stream"
            }
        };

        // A name nothing else in the package has.
        let mut index = 1usize;
        let name = loop {
            let candidate = format!("word/media/image{index}.{extension}");
            if self.package.part(&candidate).is_none() {
                break candidate;
            }
            index += 1;
        };

        self.record(EditKind::Structural, self.caret, false);
        self.package.add_part(&name, content_type, bytes.to_vec());

        let mut relationships = self
            .package
            .relationships(&self.main_part)
            .unwrap_or_else(|_| Relationships::new(&self.main_part));
        let target = name.strip_prefix("word/").unwrap_or(&name).to_owned();
        let id = relationships.add(IMAGE_RELATIONSHIP, &target, TargetMode::Internal).id.clone();
        self.package.set_relationships(&relationships)?;

        let prefix = self.prefix();
        let drawing = edit::drawing_element(&id, width_emu, height_emu, prefix.as_deref());
        let inserted = position::insert_element_at(
            &mut self.tree.root,
            self.caret,
            drawing,
            prefix.as_deref(),
        );

        if inserted {
            self.caret = TextPosition::new(self.caret.paragraph, self.caret.offset + 1);
            self.anchor = None;
            self.modified = true;
        }
        Ok(inserted)
    }

    /// Puts a chart part into the package, with the content type it needs.
    pub(crate) fn add_chart_part(&mut self, name: &str, xml: String) {
        self.package.add_part(name, crate::chart::CHART_CONTENT_TYPE, xml.into_bytes());
    }

    /// Points the document at a chart part, and gives back the relationship id.
    pub(crate) fn point_at_chart(&mut self, name: &str) -> Result<String, Error> {
        let mut relationships = self
            .package
            .relationships(&self.main_part)
            .unwrap_or_else(|_| Relationships::new(&self.main_part));
        let target = name.strip_prefix("word/").unwrap_or(name).to_owned();
        let id = relationships
            .add(crate::chart::CHART_RELATIONSHIP, &target, TargetMode::Internal)
            .id
            .clone();
        self.package.set_relationships(&relationships)?;
        Ok(id)
    }

    /// Where a relationship of the main document points, as a part name.
    #[must_use]
    pub fn relationship_target(&self, id: &str) -> Option<String> {
        let relationships = self.package.relationships(&self.main_part).ok()?;
        let target = relationships.by_id(id)?.target.clone();
        if target.starts_with("word/") || target.starts_with('/') {
            return Some(target.trim_start_matches('/').to_owned());
        }
        Some(format!("word/{target}"))
    }

    /// Puts a page break at the caret.
    pub fn insert_page_break(&mut self) -> bool {
        self.insert_break(BreakKind::Page)
    }

    /// Puts a break at the caret: of a line, of a page, or of a column.
    ///
    /// A line break ends the line without ending the paragraph, which is what
    /// Shift+Enter does; the other two end the page and the column.
    pub fn insert_break(&mut self, kind: BreakKind) -> bool {
        let at = self.caret;
        self.record(EditKind::Structural, at, false);
        let prefix = self.prefix();
        if position::insert_break(&mut self.tree.root, at, kind, prefix.as_deref()) {
            self.caret = TextPosition::new(at.paragraph, at.offset + 1);
            self.anchor = None;
            self.modified = true;
            return true;
        }
        false
    }

    /// How deep in the outline one paragraph is, if it is a heading at all.
    ///
    /// This is what makes a heading a heading — not its name. A document may
    /// call its headings anything; what says a paragraph belongs in the outline
    /// is `w:outlineLvl`, whether set on it or inherited from its style.
    #[must_use]
    pub fn outline_level(&self, index: usize) -> Option<u8> {
        let paragraph = self.paragraph_element(index)?;
        let direct = paragraph
            .child(Some(read::W), "pPr")
            .map(read::read_paragraph_properties)
            .unwrap_or_default();
        self.styles.resolve_paragraph(&direct).outline_level.filter(|level| *level < 9)
    }

    /// The style of one paragraph, if it names one.
    #[must_use]
    pub fn style_of(&self, index: usize) -> Option<String> {
        let paragraph = self.paragraph_element(index)?;
        paragraph
            .child(Some(read::W), "pPr")
            .and_then(|properties| properties.child(Some(read::W), "pStyle"))
            .and_then(read::value)
            .map(str::to_owned)
    }

    /// Where the tabs stop in the paragraph at the caret, after the style chain
    /// has had its say.
    ///
    /// Empty means the paragraph names none of its own and the document's
    /// default grid applies. See [`Document::default_tab_width`].
    #[must_use]
    pub fn tab_stops_here(&self) -> Vec<TabStop> {
        self.resolve_paragraph_here().tab_stops
    }

    /// Sets the tab stops of every paragraph the selection touches.
    pub fn set_tab_stops_here(&mut self, stops: &[TabStop]) -> bool {
        let stops = stops.to_vec();
        self.change_paragraphs(|paragraph, prefix| {
            format::set_paragraph_tab_stops(paragraph, &stops, prefix);
        })
    }

    /// Puts one stop in, replacing any that was already at that place.
    ///
    /// What a click on the ruler does.
    pub fn add_tab_stop_here(&mut self, stop: TabStop) -> bool {
        let mut stops = self.tab_stops_here();
        stops.retain(|found| found.position != stop.position);
        stops.push(stop);
        stops.sort_by_key(|found| found.position);
        self.set_tab_stops_here(&stops)
    }

    /// Takes the stop nearest a place out, if one is near enough.
    ///
    /// What dragging a marker off the ruler does. `slack` is how far away in
    /// twentieths of a point still counts as that stop, because a person aiming
    /// at a marker with a mouse does not hit it exactly.
    pub fn remove_tab_stop_here(&mut self, position: i32, slack: i32) -> bool {
        let mut stops = self.tab_stops_here();
        let Some(index) = nearest_stop(&stops, position, slack) else { return false };
        stops.remove(index);
        self.set_tab_stops_here(&stops)
    }

    /// Moves the stop nearest a place to another one.
    pub fn move_tab_stop_here(&mut self, from: i32, to: i32, slack: i32) -> bool {
        let mut stops = self.tab_stops_here();
        let Some(index) = nearest_stop(&stops, from, slack) else { return false };
        stops[index].position = to.max(0);
        stops.sort_by_key(|found| found.position);
        self.set_tab_stops_here(&stops)
    }

    /// How far apart the default stops are, in twentieths of a point.
    ///
    /// Every document says, in its settings; half an inch is what Word uses
    /// when it does not.
    #[must_use]
    pub fn default_tab_width(&self) -> i32 {
        self.settings_value("defaultTabStop")
            .and_then(|text| text.trim().parse::<i32>().ok())
            .filter(|width| *width > 0)
            .unwrap_or(720)
    }

    /// The indents of the paragraph at the caret, in twentieths of a point:
    /// from the left margin, of the first line, and from the right margin.
    #[must_use]
    pub fn indents_here(&self) -> (i32, i32, i32) {
        let resolved = self.resolve_paragraph_here();
        (resolved.indent_start, resolved.indent_first_line, resolved.indent_end)
    }

    /// The style of the paragraph the caret is in, if it has one.
    #[must_use]
    pub fn style_here(&self) -> Option<String> {
        let paragraph = self.paragraph_element(self.caret.paragraph)?;
        paragraph
            .child(Some(read::W), "pPr")
            .and_then(|properties| properties.child(Some(read::W), "pStyle"))
            .and_then(read::value)
            .map(str::to_owned)
    }

    /// The alignment of the paragraph the caret is in, after inheritance.
    #[must_use]
    pub fn alignment_here(&self) -> Alignment {
        let Some(paragraph) = self.paragraph_element(self.caret.paragraph) else {
            return Alignment::default();
        };
        let direct = paragraph
            .child(Some(read::W), "pPr")
            .map(read::read_paragraph_properties)
            .unwrap_or_default();
        self.styles.resolve_paragraph(&direct).alignment
    }

    /// Applies a change to every paragraph the selection touches, as one step.
    fn change_paragraphs(&mut self, change: impl Fn(&mut Element, Option<&str>)) -> bool {
        let (first, last) = match self.selection() {
            Some((start, end)) => (start.paragraph, end.paragraph),
            None => (self.caret.paragraph, self.caret.paragraph),
        };
        if self.paragraph_count() == 0 {
            return false;
        }

        self.record(EditKind::Structural, self.caret, false);
        let prefix = self.prefix();
        let mut changed = false;

        for index in first..=last {
            let Some(path) = position::paragraph_path(&self.tree.root, index) else { continue };
            let Some(paragraph) = edit::element_at_path_mut(&mut self.tree.root, &path) else {
                continue;
            };
            change(paragraph, prefix.as_deref());
            changed = true;
        }

        if changed {
            self.modified = true;
        }
        changed
    }

    /// Which stretch of one paragraph a selection covers.
    fn range_within(&self, index: usize, start: TextPosition, end: TextPosition) -> (usize, usize) {
        let length = self.paragraph_text(index).unwrap_or_default().len();
        let from = if index == start.paragraph { start.offset.min(length) } else { 0 };
        let to = if index == end.paragraph { end.offset.min(length) } else { length };
        (from, to)
    }

    /// Every paragraph of the document, in reading order.
    ///
    /// For anything that has to look at all of them: asking for them one at a
    /// time walks the element tree from the top each time, and a loop over the
    /// lot then costs the square of the document's length.
    pub(crate) fn paragraph_elements(&self) -> Vec<&Element> {
        position::paragraphs(&self.tree.root)
    }

    /// The nth paragraph element, if there is one.
    fn paragraph_element(&self, index: usize) -> Option<&Element> {
        let path = position::paragraph_path(&self.tree.root, index)?;
        let mut current = &self.tree.root;
        for step in &path {
            current = current.children.get(*step)?.as_element()?;
        }
        Some(current)
    }

    /// The formatting the character before the caret has.
    ///
    /// Before the caret rather than after it, so that typing at the end of a
    /// bold word continues in bold — which is also where `insert_text` puts the
    /// new text.
    fn resolved_at_caret(&self) -> ResolvedRunProperties {
        let Some(paragraph) = self.paragraph_element(self.caret.paragraph) else {
            return ResolvedRunProperties::default();
        };
        let text = self.paragraph_text(self.caret.paragraph).unwrap_or_default();

        let (start, end) = if self.caret.offset > 0 {
            (previous_boundary(&text, self.caret.offset), self.caret.offset)
        } else {
            (0, next_boundary(&text, 0))
        };

        format::resolved_in_range(paragraph, start, end, &self.styles)
            .into_iter()
            .next()
            // An empty paragraph still has formatting: a new heading is bold
            // before a single letter has been typed into it.
            .unwrap_or_else(|| format::resolved_for_paragraph(paragraph, &self.styles))
    }

    /// Appends a paragraph to the end of the document.
    pub fn append_paragraph(&mut self, paragraph: &Paragraph) -> bool {
        self.append_block(&Block::Paragraph(paragraph.clone()))
    }

    /// Appends a block to the end of the document, before the section properties.
    pub fn append_block(&mut self, block: &Block) -> bool {
        let prefix = self.prefix();
        let Some(body) = read::find_body_mut(&mut self.tree.root) else {
            return false;
        };
        edit::append_block(body, block, prefix.as_deref());
        self.modified = true;
        true
    }

    /// Sets the style of the paragraph at a given index, or clears it.
    pub fn set_paragraph_style(&mut self, index: usize, style: Option<&str>) -> bool {
        let prefix = self.prefix();
        let Some(body) = read::find_body_mut(&mut self.tree.root) else {
            return false;
        };
        let changed = edit::set_paragraph_style(body, index, style, prefix.as_deref());
        self.modified |= changed;
        changed
    }

    /// Sets the alignment of the paragraph at a given index.
    pub fn set_paragraph_alignment(&mut self, index: usize, alignment: Alignment) -> bool {
        let prefix = self.prefix();
        let Some(body) = read::find_body_mut(&mut self.tree.root) else {
            return false;
        };
        let changed = edit::set_paragraph_alignment(body, index, alignment, prefix.as_deref());
        self.modified |= changed;
        changed
    }

    /// Records that the bytes from [`Self::save`] have actually been stored.
    ///
    /// This commits the edited tree into the package and clears the modified
    /// flag. Clearing the flag alone would be a quiet corruption: the package
    /// would still hold the *old* main part, so the next save would write the
    /// document as it was before the edits.
    pub fn mark_saved(&mut self) -> Result<(), Error> {
        if !self.modified {
            return Ok(());
        }

        let xml = self
            .tree
            .to_xml()
            .map_err(|source| Error::Xml { part: self.main_part.clone(), source })?;
        self.package.set_part(&self.main_part, xml.into_bytes());
        self.modified = false;
        Ok(())
    }

    /// Writes the document back out.
    ///
    /// An unmodified document is written from its original bytes, so it comes
    /// out identical. A modified one has only its main part re-serialized;
    /// every other part is still written back exactly as it arrived.
    pub fn save(&self) -> Result<Vec<u8>, Error> {
        if !self.modified {
            return Ok(self.package.save()?);
        }

        let xml = self
            .tree
            .to_xml()
            .map_err(|source| Error::Xml { part: self.main_part.clone(), source })?;

        let mut package = self.package.clone();
        package.set_part(&self.main_part, xml.into_bytes());
        Ok(package.save()?)
    }
}

/// Reads the style definitions belonging to a document part.
///
/// The part is found by following the styles relationship rather than by
/// guessing at a filename, which is how a package is meant to be navigated. A
/// document with no styles part simply has none: that is unusual but valid, and
/// everything then falls back to the built-in defaults.
fn read_styles(package: &Package, main_part: &str, theme: crate::theme::Theme) -> Styles {
    match related_tree(package, main_part, STYLES_RELATIONSHIP, "word/styles.xml") {
        Some(tree) => Styles::parse(&tree.root).with_theme(theme),
        // A damaged styles part should not stop the document opening; the text
        // is still readable, it just renders with the defaults.
        None => Styles::default().with_theme(theme),
    }
}

fn read_numbering(package: &Package, main_part: &str) -> Numbering {
    match related_tree(package, main_part, NUMBERING_RELATIONSHIP, "word/numbering.xml") {
        Some(tree) => Numbering::parse(&tree.root),
        // Most documents have no lists and therefore no numbering part at all.
        None => Numbering::default(),
    }
}

/// Reads a part the main document points at, by relationship type.
///
/// The fallback name is what almost every document uses, and is worth trying:
/// a document whose relationships are damaged usually still has the part.
fn related_tree(
    package: &Package,
    main_part: &str,
    relationship: &str,
    fallback: &str,
) -> Option<XmlTree> {
    let target = package
        .relationships(main_part)
        .ok()
        .and_then(|relationships| {
            let found = relationships.single_by_type(relationship)?;
            found.resolved_target(main_part)?.ok()
        })
        .unwrap_or_else(|| fallback.to_owned());

    let Some(Ok(text)) = package.xml_part(&target) else {
        return None;
    };
    XmlTree::parse(&text).ok()
}

/// Builds the tree of a brand new `document.xml`.
fn build_document(body: &Body) -> XmlTree {
    let namespace = WORDPROCESSING_NAMESPACE;

    let mut root = Element::new("w:document", Some(namespace));
    root.declarations.push((Some("w".to_owned()), namespace.to_owned()));

    let mut body_element = Element::new("w:body", Some(namespace));
    for block in &body.blocks {
        edit::append_block(&mut body_element, block, Some("w"));
    }
    body_element.push_element(section_properties());
    root.push_element(body_element);

    XmlTree {
        standalone: Some(true),
        has_declaration: true,
        doctype: None,
        before_root: Vec::new(),
        root,
        after_root: Vec::new(),
    }
}

/// Page size and margins, which must be the last child of the body.
fn section_properties() -> Element {
    let namespace = WORDPROCESSING_NAMESPACE;
    let mut section = Element::new("w:sectPr", Some(namespace));

    let mut size = Element::new("w:pgSz", Some(namespace));
    size.set_namespaced_attribute("w:w", namespace, A4_WIDTH_TWIPS);
    size.set_namespaced_attribute("w:h", namespace, A4_HEIGHT_TWIPS);
    section.push_element(size);

    let mut margins = Element::new("w:pgMar", Some(namespace));
    for (name, value) in [
        ("w:top", MARGIN_TWIPS),
        ("w:right", MARGIN_TWIPS),
        ("w:bottom", MARGIN_TWIPS),
        ("w:left", MARGIN_TWIPS),
        ("w:header", "708"),
        ("w:footer", "708"),
        ("w:gutter", "0"),
    ] {
        margins.set_namespaced_attribute(name, namespace, value);
    }
    section.push_element(margins);

    section
}

/// Document settings.
///
/// The only thing in here is the compatibility mode, and it earns its place.
/// Without it Word assumes a document was written for Word 2007 and opens it in
/// compatibility mode: the title bar says so, newer features are disabled, and
/// the user is invited to convert a file that never needed converting. The
/// declaration is what says which version's rules the document was written to.
fn default_settings() -> String {
    let w = WORDPROCESSING_NAMESPACE;
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:settings xmlns:w="{w}">
<w:compat>
<w:compatSetting w:name="compatibilityMode" w:uri="http://schemas.microsoft.com/office/word" w:val="15"/>
</w:compat>
</w:settings>"#
    )
}

/// The list definitions a created document carries.
///
/// Two lists, the two a person reaches for: bullets and numbers, three levels
/// deep each. A document that carried none could not have a list added to it
/// later without one being written first, and Word's own blank template comes
/// with the same pair for the same reason.
///
/// The indents are Word's: half an inch per level, with the mark hanging a
/// quarter of an inch back into it.
fn default_numbering() -> String {
    let w = WORDPROCESSING_NAMESPACE;

    let mut levels = String::new();
    for level in 0..LIST_LEVELS {
        let indent = 720 * (level + 1);
        // Bullet, circle, square — the marks Word cycles through by depth.
        let mark = ['\u{2022}', '\u{25E6}', '\u{25AA}'][level as usize % 3];
        levels.push_str(&format!(
            r#"<w:lvl w:ilvl="{level}"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val="{mark}"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="{indent}" w:hanging="360"/></w:pPr></w:lvl>"#
        ));
    }

    let mut numbers = String::new();
    for level in 0..LIST_LEVELS {
        let indent = 720 * (level + 1);
        // Numbers, then letters, then roman: the sequence Word nests with.
        let format = ["decimal", "lowerLetter", "lowerRoman"][level as usize % 3];
        let template = format!("%{}.", level + 1);
        numbers.push_str(&format!(
            r#"<w:lvl w:ilvl="{level}"><w:start w:val="1"/><w:numFmt w:val="{format}"/><w:lvlText w:val="{template}"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="{indent}" w:hanging="360"/></w:pPr></w:lvl>"#
        ));
    }

    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:numbering xmlns:w="{w}">
<w:abstractNum w:abstractNumId="0"><w:multiLevelType w:val="hybridMultilevel"/>{levels}</w:abstractNum>
<w:abstractNum w:abstractNumId="1"><w:multiLevelType w:val="hybridMultilevel"/>{numbers}</w:abstractNum>
<w:num w:numId="{BULLET_LIST}"><w:abstractNumId w:val="0"/></w:num>
<w:num w:numId="{NUMBERED_LIST}"><w:abstractNumId w:val="1"/></w:num>
</w:numbering>"#
    )
}

/// A small stylesheet, so that documents created here have the styles their
/// paragraphs refer to.
///
/// Without it a `w:pStyle` naming `Heading1` would resolve to nothing and the
/// heading would render as body text.
///
/// The document defaults state the paragraph spacing explicitly. Leaving it
/// unstated does not mean zero: it means each program applies its own idea of a
/// default, and Word's is eight points after every paragraph. The same document
/// then came out one page here and two in Word. Saying it outright leaves
/// nothing to anyone's discretion.
fn default_styles() -> String {
    let w = WORDPROCESSING_NAMESPACE;
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="{w}">
<w:docDefaults>
<w:rPrDefault><w:rPr>
<w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:cs="Calibri" w:eastAsia="Calibri"/>
<w:sz w:val="22"/><w:szCs w:val="22"/>
</w:rPr></w:rPrDefault>
<w:pPrDefault><w:pPr>
<w:spacing w:after="160" w:line="259" w:lineRule="auto"/>
</w:pPr></w:pPrDefault>
</w:docDefaults>
<w:style w:type="paragraph" w:default="1" w:styleId="Normal">
<w:name w:val="Normal"/><w:qFormat/>
</w:style>
<w:style w:type="paragraph" w:styleId="Title">
<w:name w:val="Title"/><w:basedOn w:val="Normal"/><w:qFormat/>
<w:pPr><w:spacing w:before="240" w:after="240"/></w:pPr>
<w:rPr><w:b/><w:bCs/><w:sz w:val="56"/><w:szCs w:val="56"/></w:rPr>
</w:style>
<w:style w:type="paragraph" w:styleId="Heading1">
<w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:qFormat/>
<w:pPr><w:keepNext/><w:keepLines/><w:outlineLvl w:val="0"/><w:spacing w:before="240" w:after="120"/></w:pPr>
<w:rPr><w:b/><w:bCs/><w:sz w:val="32"/><w:szCs w:val="32"/></w:rPr>
</w:style>
<w:style w:type="paragraph" w:styleId="Heading2">
<w:name w:val="heading 2"/><w:basedOn w:val="Normal"/><w:qFormat/>
<w:pPr><w:keepNext/><w:keepLines/><w:outlineLvl w:val="1"/><w:spacing w:before="200" w:after="100"/></w:pPr>
<w:rPr><w:b/><w:bCs/><w:sz w:val="26"/><w:szCs w:val="26"/></w:rPr>
</w:style>
<w:style w:type="paragraph" w:styleId="Heading3">
<w:name w:val="heading 3"/><w:basedOn w:val="Normal"/><w:qFormat/>
<w:pPr><w:keepNext/><w:keepLines/><w:outlineLvl w:val="2"/><w:spacing w:before="180" w:after="80"/></w:pPr>
<w:rPr><w:b/><w:bCs/><w:sz w:val="24"/><w:szCs w:val="24"/></w:rPr>
</w:style>
<w:style w:type="paragraph" w:styleId="Heading4">
<w:name w:val="heading 4"/><w:basedOn w:val="Normal"/><w:qFormat/>
<w:pPr><w:keepNext/><w:keepLines/><w:outlineLvl w:val="3"/><w:spacing w:before="160" w:after="80"/></w:pPr>
<w:rPr><w:b/><w:bCs/><w:i/><w:iCs/><w:sz w:val="22"/><w:szCs w:val="22"/></w:rPr>
</w:style>
<w:style w:type="paragraph" w:styleId="Heading5">
<w:name w:val="heading 5"/><w:basedOn w:val="Normal"/><w:qFormat/>
<w:pPr><w:keepNext/><w:keepLines/><w:outlineLvl w:val="4"/><w:spacing w:before="140" w:after="60"/></w:pPr>
<w:rPr><w:b/><w:bCs/><w:sz w:val="22"/><w:szCs w:val="22"/></w:rPr>
</w:style>
<w:style w:type="paragraph" w:styleId="Heading6">
<w:name w:val="heading 6"/><w:basedOn w:val="Normal"/><w:qFormat/>
<w:pPr><w:keepNext/><w:keepLines/><w:outlineLvl w:val="5"/><w:spacing w:before="120" w:after="60"/></w:pPr>
<w:rPr><w:b/><w:bCs/><w:sz w:val="20"/><w:szCs w:val="20"/></w:rPr>
</w:style>
</w:styles>"#
    )
}

/// Where the character before an offset begins.
///
/// A character is what a reader counts, not what Rust calls a `char`: `é` may
/// be a letter and an accent drawn on it, an emoji may be seven code points
/// joined together, and Backspace takes the whole of either.
fn previous_boundary(text: &str, offset: usize) -> usize {
    wp_segment::previous_character(text, offset)
}

/// Where the character after an offset ends.
fn next_boundary(text: &str, offset: usize) -> usize {
    wp_segment::next_character(text, offset)
}

/// A stretch of a string, clamped to what is actually there.
fn slice(text: &str, start: usize, end: usize) -> &str {
    let start = start.min(text.len());
    let end = end.clamp(start, text.len());
    if text.is_char_boundary(start) && text.is_char_boundary(end) {
        &text[start..end]
    } else {
        ""
    }
}

/// Reads the theme belonging to a document part.
///
/// A document with no theme part gets the Office one, which is what Word shows
/// for such a document: the names still mean something, and what they mean is
/// the default.
fn read_theme(package: &Package, main_part: &str) -> crate::theme::Theme {
    const THEME_RELATIONSHIP: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme";
    match related_tree(package, main_part, THEME_RELATIONSHIP, "word/theme/theme1.xml") {
        Some(tree) => crate::theme::Theme::parse(&tree.root),
        None => crate::theme::Theme::default(),
    }
}

impl Document {
    /// Puts a field at the caret.
    ///
    /// `shown` is what it says now — the answer somebody worked out — which is
    /// what a reader sees until something works it out again. Every field in
    /// this format carries one, because a program that cannot answer a field
    /// still has to show something.
    pub fn insert_field(&mut self, instruction: &str, shown: &str) -> bool {
        let run = model::Run::field(instruction, shown);
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();

        let Some(path) = position::paragraph_path(&self.tree.root, caret.paragraph) else {
            return false;
        };
        let Some(paragraph) = edit::element_at_path_mut(&mut self.tree.root, &path) else {
            return false;
        };
        format::split_runs_at_offset(paragraph, caret.offset);
        let at = edit::child_position_at_offset(paragraph, caret.offset);

        let mut field = wp_xml::tree::Element::new(
            &edit::name_with(prefix.as_deref(), "fldSimple"),
            Some(read::W),
        );
        field.set_namespaced_attribute(
            &edit::name_with(prefix.as_deref(), "instr"),
            read::W,
            &format!(" {instruction} "),
        );
        field.push_element(edit::run_element(&run, prefix.as_deref()));
        paragraph.insert_element(at, field);

        self.set_caret(TextPosition::new(caret.paragraph, caret.offset + shown.len()));
        self.mark_modified();
        true
    }
}
