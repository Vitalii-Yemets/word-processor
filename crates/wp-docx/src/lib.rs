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
pub mod art;
pub mod authorities;
pub mod bibliography;
pub mod blockcontrols;
pub mod blocks;
pub mod bookmarks;
pub mod captions;
pub mod casing;
pub mod cells;
pub mod chart;
pub mod clipboard;
pub mod colour;
pub mod combine;
pub mod comments;
pub mod compare;
pub mod contents;
pub mod controls;
pub mod cover;
pub mod customxml;
pub mod depth;
pub mod diagram;
pub mod eastasian;
pub mod edit;
pub mod effects;
pub mod embedded;
pub mod fields;
pub mod figures;
pub mod fills;
pub mod floating;
pub mod fonts;
mod format;
pub mod forms;
pub mod formula;
pub mod furniture;
pub mod gallery;
pub mod group;
mod history;
pub mod ink;
pub mod joins;
pub mod kinds;
pub mod languages;
pub mod lines;
pub mod links;
pub mod locking;
pub mod math;
pub mod merge;
pub mod model;
pub mod notes;
pub mod numberformat;
pub mod numbering;
pub mod page;
pub mod pageborders;
pub mod permissions;
pub mod position;
pub mod proofing;
pub mod properties;
pub mod protection;
mod read;
pub mod readonly;
pub mod revisions;
pub mod ruby;
pub mod rules;
pub mod sealing;
pub mod search;
pub mod sections;
pub mod settings;
pub mod shapeeffects;
pub mod shapes;
pub mod signature;
pub mod signing;
pub mod sorting;
pub mod stationery;
pub mod styles;
pub mod table_properties;
pub mod tables;
pub mod tablestyles;
pub mod theme;
pub mod translate;
pub mod typography;
pub mod video;
pub mod watermark;
pub mod words;
pub mod workbook;

use history::History;
use wp_opc::{Package, Relationship, Relationships, TargetMode};
use wp_xml::tree::{Element, XmlTree};

pub use format::CharacterFormat;
pub use history::EditKind;
pub use model::Body;
pub use numbering::{ListCounters, Numbering};
pub use position::TextPosition;
pub use read::W as WORDPROCESSING_NAMESPACE;
pub use styles::{Style, StyleDefinition, StyleKind, Styles};

use model::{
    nearest_stop, Alignment, Block, BreakKind, LineSpacing, NumberingReference, Paragraph,
    ParagraphBorders, ParagraphProperties, ResolvedParagraphProperties, ResolvedRunProperties, Run,
    RunProperties, TabStop, Table, TableCell, TableRow,
};

/// Content type of the styles part.
pub(crate) const STYLES_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml";

/// Relationship type of the styles part.
pub(crate) const STYLES_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles";

/// Content type of the settings part.
const SETTINGS_CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml";

/// Relationship type of the settings part.
const SETTINGS_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings";

/// Relationship type of the numbering part, which holds the list definitions.
pub(crate) const NUMBERING_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering";

/// Relationship type of an embedded picture.
const IMAGE_RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";

/// Content type of the numbering part.
pub(crate) const NUMBERING_CONTENT_TYPE: &str =
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
    /// The file is an encrypted document, which opens with a password and
    /// not without one. See [`Document::open_sealed`].
    Sealed,
    /// A password was given and it is not the password, or the file is
    /// encrypted a way this program does not read.
    Unsealing(wp_crypt::Error),
    /// A signature could not be put on.
    Signing(String),
    /// A part is not valid XML.
    Xml { part: String, source: wp_xml::Error },
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Package(error) => write!(f, "{error}"),
            Self::Sealed => write!(f, "the document is encrypted and needs its password"),
            Self::Unsealing(error) => write!(f, "{error}"),
            Self::Signing(what) => write!(f, "cannot sign: {what}"),
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

impl From<wp_crypt::Error> for Error {
    fn from(error: wp_crypt::Error) -> Self {
        Self::Unsealing(error)
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
    /// The stretches selected besides the one the caret is in.
    ///
    /// Word lets a person hold Ctrl and drag out another stretch without losing
    /// the ones already chosen, which is how the same word is made bold in nine
    /// places at once. The caret and its anchor are the stretch being dragged
    /// now; these are the ones dragged before it.
    ///
    /// Not kept by undo. What is selected is where a person is looking, not
    /// what the document says, and an undo that put back a selection made three
    /// edits ago would be an undo that moved the view somewhere nobody asked
    /// for.
    extra: Vec<(TextPosition, TextPosition)>,
    /// Formatting chosen with nothing selected, waiting for the next thing
    /// typed.
    ///
    /// Pressing Ctrl+B before writing a word has to mean something, and the
    /// only thing it can mean is that the word about to be written is bold. It
    /// is dropped as soon as the caret is moved somewhere else, because by then
    /// it is about a place the user has left.
    pending: RunProperties,
    history: History,
    /// Whether the tree or the package has been changed since the history
    /// last gave the present state a number.
    ///
    /// Every edit sets it, and none of them has to know about numbers:
    /// recording a step, undoing, redoing and saving each give the present
    /// state its number first, and clear this. Whether the document has
    /// changed is then whether this is set or the present number is not the
    /// saved one — see [`history`] for why that is a number and not a flag.
    modified: bool,
    /// The package's generation when it last held exactly what is on disk.
    ///
    /// The package's own bytes stand for the document only while they are
    /// still that — the present state the saved one, and no part written since
    /// — which is what makes an untouched document come out identical. Coming
    /// back to the saved state by way of parts written on the way does not
    /// make the parts the saved ones again, and a save then has to write the
    /// tree.
    saved_generation: u64,
    /// The package's generation when the history last looked at what had been
    /// written to it.
    ///
    /// A step keeps the tree, and sometimes a few named parts beside it; a
    /// part written that no step keeps is a change no undo takes back. Every
    /// part written goes through the package, which says which parts were
    /// written since a generation, so the history can find such a change
    /// where it happened instead of every command having to own up to it.
    accounted_generation: u64,
    /// Whether the package's own bytes are the file, as it was opened.
    ///
    /// Then a document nobody has changed is saved as those bytes, identical.
    /// A save writes the package without the parts nothing reaches any more,
    /// and the package is kept with them — undo may want one back — so after
    /// a save the file is what the package is saved as, not its bytes. See
    /// [`Self::save`].
    package_is_the_file: bool,
    /// Where the caret was in each part when that part was last left.
    ///
    /// Undo that goes back into a header to take a change back leaves a way
    /// to redo it, and redo puts the caret back where it was in the header
    /// when undo went in — which is where the person left it, not where the
    /// caret is in the body.
    left_at: Vec<(String, TextPosition)>,
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
    /// The cells selected, when what is selected is a block of them, together
    /// with the selection it was made for.
    ///
    /// # Why it is kept and not worked out
    ///
    /// Because the cells of a new table hold nothing. A block of cells is
    /// ordinarily read back from the stretches of text in it — the first and
    /// the last say which rectangle was taken — but empty cells make empty
    /// stretches, and a selection of nine empty cells is indistinguishable
    /// from no selection at all. Merge Cells on a table somebody has only just
    /// inserted is the first thing anyone tries.
    ///
    /// # Why the selection is kept with it
    ///
    /// So that it goes stale by itself. The caret and its anchor are recorded
    /// as they were when the block was taken, and the block is only believed
    /// while they still are that: anything that moves the caret or selects
    /// anything else leaves them different, and the block is then ignored
    /// without anybody having to remember to say so.
    cell_block: Option<(cells::CellRange, Option<TextPosition>, TextPosition)>,
    gesture_depth: usize,
    gesture_noted: bool,
    /// Where the caret was when the gesture began, which is where undoing the
    /// gesture puts it back: a gesture moves the caret about to do its work,
    /// and the person was not where it went.
    gesture_caret: TextPosition,
    /// The password the document was opened with, and is to be written back
    /// under.
    ///
    /// Kept so that saving an encrypted document leaves it encrypted. Not
    /// kept in the file, obviously, and not written anywhere: it lives as
    /// long as the document is open and no longer.
    password: Option<String>,
}

impl Document {
    /// Opens a `.docx` from its bytes.
    ///
    /// An encrypted document is refused with [`Error::Sealed`] rather than
    /// with the package layer's complaint that the bytes are not a zip: what
    /// a program has to do about one is ask for the password, and it can only
    /// do that if it is told which kind of failure this was.
    pub fn open(bytes: &[u8]) -> Result<Self, Error> {
        if wp_crypt::is_encrypted(bytes) {
            return Err(Error::Sealed);
        }
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

        let saved_generation = package.generation();
        let mut document = Self {
            package,
            password: None,
            document_part: main_part.clone(),
            main_part,
            tree,
            styles,
            numbering,
            caret: TextPosition::default(),
            anchor: None,
            extra: Vec::new(),
            cell_block: None,
            pending: RunProperties::default(),
            history: History::default(),
            modified: false,
            saved_generation,
            accounted_generation: saved_generation,
            package_is_the_file: true,
            left_at: Vec::new(),
            tracking: false,
            reviser: revisions::Reviser::default(),
            gesture_depth: 0,
            gesture_noted: false,
            gesture_caret: TextPosition::new(0, 0),
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
        let saved_generation = package.generation();

        Ok(Self {
            package,
            password: None,
            main_part: "word/document.xml".to_owned(),
            document_part: "word/document.xml".to_owned(),
            tree,
            styles,
            numbering,
            caret: TextPosition::default(),
            anchor: None,
            extra: Vec::new(),
            cell_block: None,
            pending: RunProperties::default(),
            history: History::default(),
            modified: false,
            saved_generation,
            accounted_generation: saved_generation,
            package_is_the_file: true,
            left_at: Vec::new(),
            tracking: false,
            reviser: revisions::Reviser::default(),
            gesture_depth: 0,
            gesture_noted: false,
            gesture_caret: TextPosition::new(0, 0),
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
    /// afterwards whether it was. What is done through it is no step of the
    /// history's — nothing here knows what it was, so nothing can take it back —
    /// so no undo may come back to the state on disk after it: see
    /// [`history::History::lose_saved`].
    pub fn tree_mut(&mut self) -> &mut XmlTree {
        self.changed_off_the_record();
        &mut self.tree
    }

    /// The element tree, for an edit of this crate's own.
    ///
    /// Marks the document as changed, as [`Self::tree_mut`] does, and nothing
    /// more: the edits that take it record a step first, so the history knows
    /// how to take them back.
    pub(crate) fn tree_to_edit(&mut self) -> &mut XmlTree {
        self.modified = true;
        &mut self.tree
    }

    /// Says an edit of this crate's own has changed the document.
    ///
    /// The step that takes it back was recorded before it, or the part it
    /// wrote is found by the history when it next looks: see
    /// [`Self::account_for_package_writes`].
    pub(crate) fn note_change(&mut self) {
        self.modified = true;
    }

    /// Says the document has changed in a way no step records and nothing
    /// in the package shows.
    ///
    /// The password the file is to be sealed with is one: it is kept beside
    /// the package, not in it. So is whatever a caller does to the tree with
    /// [`Self::tree_mut`]. No undo takes such a change back, so no undo may
    /// come back to the state on disk after one.
    pub(crate) fn changed_off_the_record(&mut self) {
        self.modified = true;
        self.history.lose_saved();
    }

    /// Whether the document differs from what was last saved, or from what
    /// was opened if it has not been saved.
    ///
    /// Undo and redo count: taking back what was typed after a save goes back
    /// to the saved document, and taking back more than that leaves one that
    /// is not on disk.
    #[must_use]
    pub fn is_modified(&self) -> bool {
        self.modified || !self.history.is_at_saved()
    }

    /// Gives the present state its number, if it has changed since it was last
    /// given one.
    ///
    /// Called before anything that remembers where the document stands — a
    /// step recorded, undo, redo, a save — so that what it remembers is the
    /// present state and no other.
    fn number_present(&mut self) {
        self.account_for_package_writes();
        if core::mem::take(&mut self.modified) {
            self.history.advance();
        }
    }

    /// Looks at what has been written to the package since it was last looked
    /// at, and puts the saved state out of reach if any of it is something
    /// no step can put back.
    ///
    /// A step keeps the tree, and a step made with
    /// [`Self::record_with_parts`] the parts it names as well. A part written
    /// outside those — the settings, the properties, the notes, a picture —
    /// stays written whatever undo does, so after it no undo can come back to
    /// the file. Found here, where every change to the package passes, rather
    /// than asked of every command that writes one; and the history's own
    /// writes — the tree written back as a part is left, the parts a step puts
    /// back, a save — are looked past, because they are what the history
    /// keeps.
    fn account_for_package_writes(&mut self) {
        let generation = self.package.generation();
        if generation == self.accounted_generation {
            return;
        }
        let kept = self.history.parts_kept_by_last_step();
        let unkept = self
            .package
            .written_since(self.accounted_generation)
            .any(|written| !kept.iter().any(|name| name.eq_ignore_ascii_case(written)));
        if unkept {
            self.history.lose_saved();
        }
        self.accounted_generation = generation;
    }

    /// Whether the package, with the tree beside it, is exactly what is on
    /// disk.
    ///
    /// Only then may the package's own bytes stand for the document, or the
    /// tree be left unwritten when another part is entered: the present state
    /// has to be the saved one, and no part may have been written since the
    /// save. A document that undo has taken back past the save, or brought
    /// back to it through a header written on the way, is not.
    fn is_as_saved(&self) -> bool {
        !self.modified
            && self.history.is_at_saved()
            && self.package.generation() == self.saved_generation
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

    /// A document made of lines of plain text, one paragraph each, which is
    /// what opening a text file gives: Word's Normal style and nothing
    /// else, because a text file says nothing else.
    pub fn from_text(lines: &[String]) -> Result<Self, Error> {
        let mut body = Body::default();
        for line in lines {
            body.blocks.push(Block::Paragraph(Paragraph::text(line)));
        }
        if body.blocks.is_empty() {
            body.blocks.push(Block::Paragraph(Paragraph::default()));
        }
        Self::create(&body)
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

    /// Says the document holds changes that are not on disk.
    ///
    /// For a document that did not come off disk at all — one recovered from
    /// a copy after a crash is exactly that, and a program that did not say
    /// so would let the person close it without being asked.
    ///
    /// A change until the next save, whatever undo does: undo takes back what
    /// the history recorded, and this it did not, so the saved state is put
    /// out of reach. The edits of this crate say the same thing with
    /// [`Self::note_change`], which leaves the way back to the saved state to
    /// the steps they record.
    pub fn mark_modified(&mut self) {
        self.changed_off_the_record();
    }

    /// Counts the present state as the one on disk, without writing anything.
    ///
    /// For what is done to a document that is not a person's edit and is not
    /// to be offered for saving: bringing bound controls up to date as it is
    /// opened, making a document from a template. The tree is not written
    /// into the package: what was brought up to date as the file was opened
    /// is brought up to date again the next time it is, and a file nobody
    /// has changed is written back byte for byte.
    pub(crate) fn count_as_saved(&mut self) {
        self.number_present();
        self.history.mark_saved();
        self.saved_generation = self.package.generation();
        self.accounted_generation = self.saved_generation;
    }

    pub(crate) fn prefix(&self) -> Option<String> {
        edit::prefix_for(&self.tree.root, WORDPROCESSING_NAMESPACE)
    }

    /// Replaces every occurrence of a string, returning how many were changed.
    ///
    /// The search works across run boundaries, which it has to: Word splits a
    /// paragraph's text between runs wherever formatting changes, so a word can
    /// easily be stored in two pieces.
    ///
    /// Word's Replace All, and one step to take back, as it is there.
    pub fn replace_text(&mut self, needle: &str, replacement: &str) -> usize {
        let mut replaced = 0;
        self.edit_tree_as_one_step(|root, _| {
            replaced = edit::replace_text(root, needle, replacement);
            replaced > 0
        });
        replaced
    }

    /// Makes a change to the tree that is one step to take back, and a step
    /// only if it changed anything.
    ///
    /// The change is made on a copy first, because whether it changes anything
    /// is only known once it is made, and a step recorded for nothing is a
    /// press of undo that does nothing. The copy is what a step keeps anyway.
    fn edit_tree_as_one_step(
        &mut self,
        change: impl FnOnce(&mut Element, Option<&str>) -> bool,
    ) -> bool {
        let prefix = self.prefix();
        let mut root = self.tree.root.clone();
        if !change(&mut root, prefix.as_deref()) {
            return false;
        }
        self.record(EditKind::Structural, self.caret, false);
        self.tree.root = root;
        self.modified = true;
        true
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
        // Undoing a gesture puts the caret where it was before the gesture,
        // not where the gesture had moved it to by its first change.
        let caret = if in_gesture { self.gesture_caret } else { self.caret };
        self.number_present();
        self.history.record(kept, &self.main_part, caret, kind, ends_at, mergeable);
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
            history::Kept::WithParts { parts, .. } => {
                let names: Vec<&str> = parts.iter().map(|(name, _)| name.as_str()).collect();
                self.kept_with_parts(&names)
            }
        }
    }

    /// The whole tree and some parts of the package as they are now.
    fn kept_with_parts(&self, names: &[&str]) -> history::Kept {
        let parts = names
            .iter()
            .filter_map(|name| Some(((*name).to_owned(), self.package.part(name)?.to_vec())))
            .collect();
        history::Kept::WithParts { tree: self.tree.clone(), parts }
    }

    /// Records the current state, tree and named parts of the package, so a
    /// change to those parts can be taken back.
    pub(crate) fn record_with_parts(&mut self, ends_at: TextPosition, names: &[&str]) {
        if self.gesture_depth > 0 {
            if self.gesture_noted {
                return;
            }
            self.gesture_noted = true;
        }
        let kept = self.kept_with_parts(names);
        let caret = if self.gesture_depth > 0 { self.gesture_caret } else { self.caret };
        self.number_present();
        self.history.record(kept, &self.main_part, caret, EditKind::Structural, ends_at, false);
    }

    /// Puts a kept state back where it came from.
    fn put_back(&mut self, kept: history::Kept) {
        match kept {
            history::Kept::Whole(tree) => self.tree = tree,
            history::Kept::WithParts { tree, parts } => {
                self.tree = tree;
                for (name, bytes) in parts {
                    self.package.set_part(&name, bytes);
                }
                // What is read from those parts once and kept — the styles and
                // the theme they resolve against, the lists, whether changes
                // are tracked — is read again, or the parts would be back and
                // the document would go on as if they were not.
                self.reread_what_parts_say();
            }
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
            self.gesture_caret = self.caret;
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
        self.extra.clear();
        self.pending = RunProperties::default();
        self.history.break_merge();
    }

    /// Keeps whatever is selected and begins another stretch.
    ///
    /// Word's Ctrl and a drag. The stretch being dragged now is put away with
    /// the others, and a new one starts where the pointer went down. An empty
    /// one is not put away: a Ctrl click that selects nothing has nothing to
    /// keep.
    pub fn add_selection_at(&mut self, position: TextPosition) {
        if let Some(stretch) = self.selection() {
            self.extra.push(stretch);
        }
        self.caret = self.clamp(position);
        self.anchor = None;
        self.pending = RunProperties::default();
        self.history.break_merge();
    }

    /// Replaces the selection with a list of stretches.
    ///
    /// What a column selection is made of, and why it is a setter rather than
    /// a series of adds: the shape of a rectangle changes everywhere at once as
    /// it is dragged, so every stretch is worked out again on every movement.
    pub fn set_selections(&mut self, stretches: &[(TextPosition, TextPosition)]) {
        self.anchor = None;
        self.extra.clear();
        self.pending = RunProperties::default();
        self.history.break_merge();

        let Some((last_start, last_end)) = stretches.last().copied() else { return };
        for (start, end) in &stretches[..stretches.len() - 1] {
            self.add_selection(*start, *end);
        }
        // The last one is the live stretch, so the caret is in it and a
        // keystroke goes where a person is looking.
        self.anchor = Some(self.clamp(last_start));
        self.caret = self.clamp(last_end);
    }

    /// Adds a stretch outright, without moving the caret into it.
    ///
    /// What Select All Text With Similar Formatting and a search for every
    /// occurrence of a word are made of: many stretches at once, none of them
    /// dragged out by hand.
    pub fn add_selection(&mut self, start: TextPosition, end: TextPosition) {
        let (start, end) = (self.clamp(start), self.clamp(end));
        if start == end {
            return;
        }
        self.extra.push(if start <= end { (start, end) } else { (end, start) });
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
        self.extra.clear();
    }

    /// The selected range, in document order, if anything is selected.
    ///
    /// The stretch the caret is in, or — when the caret is in none — the first
    /// of the others. What a command that can work on only one stretch should
    /// use: a comment is about one piece of text, and so is a hyperlink.
    /// Everything that can work on all of them uses [`Self::selections`].
    #[must_use]
    pub fn selection(&self) -> Option<(TextPosition, TextPosition)> {
        match self.anchor {
            Some(anchor) if anchor != self.caret => {
                Some(if anchor <= self.caret { (anchor, self.caret) } else { (self.caret, anchor) })
            }
            _ => self.selections().into_iter().next(),
        }
    }

    /// Every selected stretch, in document order, with none of them touching.
    ///
    /// Two stretches that overlap are one stretch: a person who dragged over
    /// the same words twice selected them once, and a command that ran over
    /// them twice would bold what was already bold and delete what was already
    /// gone.
    #[must_use]
    pub fn selections(&self) -> Vec<(TextPosition, TextPosition)> {
        let mut out = self.extra.clone();
        if let Some(anchor) = self.anchor {
            if anchor != self.caret {
                out.push(if anchor <= self.caret {
                    (anchor, self.caret)
                } else {
                    (self.caret, anchor)
                });
            }
        }
        if out.len() < 2 {
            return out;
        }

        out.sort();
        let mut merged: Vec<(TextPosition, TextPosition)> = Vec::with_capacity(out.len());
        for (start, end) in out {
            match merged.last_mut() {
                Some((_, last_end)) if start <= *last_end => *last_end = (*last_end).max(end),
                _ => merged.push((start, end)),
            }
        }
        merged
    }

    /// Where the stretch being dragged began, which is not where it starts.
    ///
    /// A drag that went backwards has its anchor at the end. What the code that
    /// grows a drag to whole words needs, and the one thing
    /// [`Self::selection`] cannot say because it puts the two in order.
    #[must_use]
    pub fn selection_anchor(&self) -> Option<TextPosition> {
        self.anchor
    }

    /// The selected text, with a line break between paragraphs.
    ///
    /// A selection of several stretches gives them all, one after another with
    /// a line break between: Word joins them the same way, because what a
    /// person pastes has to be one piece of text whatever it was taken from.
    #[must_use]
    pub fn selected_text(&self) -> String {
        let stretches = self.selections();
        if stretches.len() > 1 {
            let pieces: Vec<String> =
                stretches.iter().map(|(start, end)| self.text_between(*start, *end)).collect();
            return pieces.join("\n");
        }
        match stretches.first() {
            Some((start, end)) => self.text_between(*start, *end),
            None => String::new(),
        }
    }

    /// The text between two positions.
    #[must_use]
    fn text_between(&self, start: TextPosition, end: TextPosition) -> String {
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
        let stretches = self.selections();
        let Some((first, _)) = stretches.first().copied() else {
            return false;
        };
        self.record(EditKind::Structural, first, false);

        // Last first. Removing a stretch shortens the text after it and moves
        // every position past it; taking the last one out first means the ones
        // still to go are where they were when they were found.
        let mut removed = false;
        for (start, end) in stretches.into_iter().rev() {
            removed |= self.remove_range(start, end);
        }
        if removed {
            self.caret = first;
            self.anchor = None;
            self.extra.clear();
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

        // Typing over a selection made of several stretches — a block of the
        // cells of a table, or pieces picked out with Ctrl held — empties all
        // of them and types where the first of them was. Word does the same:
        // what was selected is what is replaced, not the last piece of it.
        if self.selections().len() > 1 {
            self.begin_gesture();
            let emptied = self.delete_selection();
            let at = self.caret;
            let typed = emptied && self.write_text(at, text);
            if typed {
                self.caret = TextPosition::new(at.paragraph, at.offset + text.len());
                self.apply_pending(at, self.caret);
                self.modified = true;
            }
            self.end_gesture();
            return emptied || typed;
        }

        // Typing over a selection is one change, not two: taking it back has to
        // bring the replaced text straight back, the way it does in every other
        // editor. So the removal and the insertion share a single step.
        if let Some((start, end)) = self.selection() {
            // A selection that is the whole of what a content control holds
            // is answered rather than removed: the words go inside the
            // control, where its boundary would otherwise be an edge the
            // new text fell off. See [`controls`].
            if let Some(control) = self.control_at(start) {
                if control.start == start
                    && control.end == end
                    && control.start != control.end
                    && !matches!(
                        control.kind,
                        controls::ControlKind::CheckBox | controls::ControlKind::Picture
                    )
                {
                    self.record(EditKind::Structural, start, false);
                    self.anchor = None;
                    if self.set_control_text(start, text) {
                        self.caret = TextPosition::new(start.paragraph, start.offset + text.len());
                        self.apply_pending(start, self.caret);
                    } else {
                        self.caret = start;
                    }
                    self.modified = true;
                    self.history.break_merge();
                    return true;
                }
            }
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
        let recording = self.recording_formatting();

        let Some(path) = position::paragraph_path(&self.tree.root, start.paragraph) else {
            return;
        };
        let Some(paragraph) = edit::element_at_path_mut(&mut self.tree.root, &path) else {
            return;
        };
        format::apply_to_range(
            paragraph,
            start.offset,
            end.offset,
            &change,
            prefix.as_deref(),
            recording.as_ref().map(|(reviser, id)| (reviser, *id)),
        );
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

    /// Forgets every step there is to undo or redo.
    ///
    /// For a document that was built by a reader of another format, whose
    /// pictures, notes and sections went in one edit at a time: none of that is
    /// anything a person did, and Undo straight after opening must not take a
    /// picture out of the file.
    pub fn forget_history(&mut self) {
        self.history.forget();
    }

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
        let left = (core::mem::replace(&mut self.main_part, part.to_owned()), self.caret);
        match self.left_at.iter_mut().find(|(name, _)| *name == left.0) {
            Some(entry) => *entry = left,
            None => self.left_at.push(left),
        }
        self.tree = tree;
        self.caret = TextPosition::new(0, 0);
        self.anchor = None;
        // The stretches picked out with Ctrl held were places in the part
        // being left, as the caret and the anchor were.
        self.extra.clear();
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
    /// "Changed" is against the file, not against the last step: a tree that
    /// undo has put back is a change to the part the package holds, whatever
    /// the history says about the document as a whole.
    fn flush_part(&mut self) {
        if self.is_as_saved() {
            return;
        }
        // Anything written before this is looked at first: the tree's own
        // part is the history's to write, and whatever else was written
        // since the history last looked must not be taken for it.
        self.account_for_package_writes();
        if let Ok(xml) = self.tree.to_xml() {
            self.package.set_part(&self.main_part, xml.into_bytes());
        }
        self.accounted_generation = self.package.generation();
    }

    /// Takes back the last change.
    ///
    /// A step that belongs to another part of the package — a header, say —
    /// brings that part back with it, because undoing an edit means being where
    /// the edit was.
    pub fn undo(&mut self) -> bool {
        let Some((_, part)) = self.history.next_undo() else { return false };
        let part = part.to_owned();
        if !self.go_to_part_of_step(&part) {
            return false;
        }
        let Some((shape, _)) = self.history.next_undo() else { return false };
        let now = self.kept_like(shape);
        self.number_present();
        let Some((kept, caret)) = self.history.undo(now, &self.main_part, self.caret) else {
            return false;
        };
        self.restore(kept, caret);
        true
    }

    /// Puts back a change that was taken back, in the part it was taken back
    /// in, for the same reason.
    pub fn redo(&mut self) -> bool {
        let Some((_, part)) = self.history.next_redo() else { return false };
        let part = part.to_owned();
        if !self.go_to_part_of_step(&part) {
            return false;
        }
        let Some((shape, _)) = self.history.next_redo() else { return false };
        let now = self.kept_like(shape);
        self.number_present();
        let Some((kept, caret)) = self.history.redo(now, &self.main_part, self.caret) else {
            return false;
        };
        self.restore(kept, caret);
        true
    }

    /// Makes the part a step was recorded in the one being edited.
    ///
    /// A step keeps a state of its own part — a paragraph of a header is
    /// found by where it stands in the header — so it can only be put back
    /// into that part's tree, and what is kept of the present for the way back
    /// has to be taken from that tree too. Both happen after this. The part is
    /// loaded the way [`Self::enter_part`] loads it, since that is what Word
    /// does: undo of a change in a header opens the header. Going there is not
    /// itself a change, so nothing is recorded.
    ///
    /// The caret goes where it was when that part was last left, so that what
    /// is kept for redo remembers where the person was in it.
    fn go_to_part_of_step(&mut self, part: &str) -> bool {
        if part == self.main_part {
            return true;
        }
        if !self.enter_part(part) {
            return false;
        }
        if let Some((_, caret)) = self.left_at.iter().find(|(name, _)| name == part) {
            self.caret = self.clamp(*caret);
        }
        true
    }

    /// Puts a remembered state back into the part being edited, which is the
    /// one it was taken from.
    ///
    /// The document is then exactly the state the history has just named, so
    /// nothing has changed since it was numbered.
    fn restore(&mut self, kept: history::Kept, caret: TextPosition) {
        self.put_back(kept);
        // The parts a step puts back are the history's own writes.
        self.accounted_generation = self.package.generation();
        self.caret = self.clamp(caret);
        self.anchor = None;
        self.pending = RunProperties::default();
        self.modified = false;
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

        // Throughout every stretch, not only the one the caret is in: a button
        // that lit up because one of nine selected words was bold would be a
        // button that lied about the other eight.
        let mut seen = false;
        for (start, end) in self.selections() {
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
        }
        if seen {
            return true;
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

        if self.selection().is_none() {
            if format.is_on(&self.resolved_at_caret()) == on {
                // Asking for what the text here already is: nothing needs
                // remembering, and the next thing typed simply inherits it.
                format.clear(&mut self.pending);
            } else {
                self.pending = self.pending.overlaid_with(&change);
            }
            return true;
        }
        self.format_selection(&change)
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
            effects::declare_namespace(&mut self.tree_to_edit().root);
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
    /// Applies a whole set of character formatting at once.
    ///
    /// The Font dialog is answered all together rather than a property at a
    /// time, and a change made in one go is one undo step and one pass over the
    /// runs — which is what Word does, and what stops a dialog with a dozen
    /// fields in it from filling the undo list with a dozen entries.
    pub fn set_character_format(&mut self, change: &RunProperties) -> bool {
        // The namespace has to be declared before anything in it is written,
        // and there is nothing to declare for a change that asks the font for
        // nothing.
        if change.open_type.as_ref().is_some_and(|wanted| !wanted.is_empty()) {
            typography::declare_namespace(&mut self.tree_to_edit().root);
        }
        self.apply_character_change(change)
    }

    /// Everything the text under the caret or across the selection is formatted
    /// with, which is what the Font dialog opens showing.
    #[must_use]
    pub fn character_format_here(&self) -> ResolvedRunProperties {
        self.resolved_over_selection()
    }

    /// How the text at a place is formatted, and the stretch of its paragraph
    /// formatted the same way — the run it is in, as byte offsets into the
    /// paragraph's text. What a screen reader asks when it reads how a word
    /// is set: at a run's end, the next run's; in an empty paragraph, what
    /// the paragraph would give typing.
    #[must_use]
    pub fn formatting_at(&self, at: TextPosition) -> (ResolvedRunProperties, usize, usize) {
        let Some(paragraph) = self.paragraph_element(at.paragraph) else {
            return (ResolvedRunProperties::default(), at.offset, at.offset);
        };
        let length = self.paragraph_text(at.paragraph).map_or(0, |text| text.len());
        let runs = format::runs_in_range(paragraph, 0, length, &self.styles);
        let found = runs
            .iter()
            .find(|(from, (to, _))| at.offset >= *from && at.offset < *to)
            .or_else(|| runs.last().filter(|(_, (to, _))| at.offset >= *to));
        match found {
            Some((from, (to, properties))) => (properties.clone(), *from, *to),
            None => (format::resolved_for_paragraph(paragraph, &self.styles), 0, length),
        }
    }

    /// Word's Set As Default: makes this the formatting everything inherits.
    ///
    /// It is written into `w:docDefaults`, which is the bottom of the
    /// inheritance chain — under every style and under every run. So it reaches
    /// every paragraph that never said otherwise, and nothing that did.
    ///
    /// Word offers to write it into the template as well, so that new documents
    /// start with it. There is no template here yet, so it reaches this
    /// document and no other; **H4** in the roadmap is where a template would
    /// come from.
    pub fn set_default_character_format(&mut self, change: &RunProperties) -> bool {
        let Some(mut tree) = self.styles_tree() else { return false };

        if change.open_type.as_ref().is_some_and(|wanted| !wanted.is_empty()) {
            typography::declare_namespace(&mut tree.root);
        }

        // The chain is docDefaults > rPrDefault > rPr, and every link of it is
        // made if it is not there: a styles part with no defaults at all is
        // unusual but perfectly valid.
        let prefix = edit::prefix_for(&tree.root, WORDPROCESSING_NAMESPACE);
        let defaults = child_or_new(&mut tree.root, "docDefaults", prefix.as_deref());
        let run_defaults = child_or_new(defaults, "rPrDefault", prefix.as_deref());
        let properties = child_or_new(run_defaults, "rPr", prefix.as_deref());

        let before = properties.clone();
        format::write_run_properties(properties, change, prefix.as_deref());
        if *properties == before {
            return false;
        }

        // What the styles say has changed, so what every run resolves to has
        // changed with it.
        self.record_styles_change();
        self.styles = Styles::parse(&tree.root).with_theme(self.styles.theme().clone());
        self.save_styles_tree(&tree);
        self.note_change();
        true
    }

    /// Writes a style definition, making it if the document has none by that
    /// identifier.
    ///
    /// What Word's New Style and Modify Style do. Only what is named is
    /// written: a style keeps everything this program does not model, because
    /// its element is edited rather than replaced. A document is full of style
    /// properties nobody here has heard of, and rewriting a style whole would
    /// throw them away.
    pub fn set_style(&mut self, wanted: &StyleDefinition) -> bool {
        self.set_style_of_kind(wanted, StyleKind::Paragraph)
    }

    /// The same for a style of any kind: a new one is made of that kind, and
    /// one already there keeps the kind it has.
    ///
    /// What a file from another program needs, which brings its character
    /// styles and its table styles with it as well as its paragraph styles.
    /// A character style has no paragraph formatting to write, and none is.
    pub fn set_style_of_kind(&mut self, wanted: &StyleDefinition, kind: StyleKind) -> bool {
        let id = wanted.id.trim();
        if id.is_empty() {
            return false;
        }
        let Some(mut tree) = self.styles_tree() else { return false };
        let prefix = edit::prefix_for(&tree.root, WORDPROCESSING_NAMESPACE);
        let before = tree.root.clone();

        // The one whose identifier matches, or a new one at the end.
        if !tree
            .root
            .children_named(Some(read::W), "style")
            .any(|style| style.attribute(Some(read::W), "styleId").is_some_and(|found| found == id))
        {
            let mut element =
                Element::new(&edit::name_with(prefix.as_deref(), "style"), Some(read::W));
            element.set_namespaced_attribute(
                &edit::name_with(prefix.as_deref(), "type"),
                read::W,
                match kind {
                    StyleKind::Character => "character",
                    StyleKind::Table => "table",
                    StyleKind::Numbering => "numbering",
                    StyleKind::Paragraph | StyleKind::Other => "paragraph",
                },
            );
            element.set_namespaced_attribute(
                &edit::name_with(prefix.as_deref(), "styleId"),
                read::W,
                id,
            );
            tree.root.push_element(element);
        }

        let Some(element) = tree.root.child_elements_mut().find(|style| {
            style.is(Some(read::W), "style")
                && style.attribute(Some(read::W), "styleId").is_some_and(|found| found == id)
        }) else {
            return false;
        };

        let named = |local: &str, value: &str| {
            let mut child = Element::new(&edit::name_with(prefix.as_deref(), local), Some(read::W));
            child.set_namespaced_attribute(
                &edit::name_with(prefix.as_deref(), "val"),
                read::W,
                value,
            );
            child
        };
        // The three names go at the front, in the order the schema wants them.
        for (local, value) in [
            ("next", wanted.next.as_deref()),
            ("basedOn", wanted.based_on.as_deref()),
            ("name", Some(wanted.name.as_str())),
        ] {
            element.remove_children_named(Some(read::W), local);
            if let Some(value) = value.filter(|text| !text.trim().is_empty()) {
                element.insert_element(0, named(local, value.trim()));
            }
        }

        // And the formatting, into the style's own `pPr` and `rPr`.
        if kind != StyleKind::Character {
            let paragraph = child_or_new(element, "pPr", prefix.as_deref());
            format::write_paragraph_properties(paragraph, &wanted.paragraph, prefix.as_deref());
            // The borders and the shading are written apart from the rest,
            // because a paragraph's are set from their own dialog and a style's
            // come with the definition: the one writer that does both takes the
            // properties rather than the paragraph round them.
            format::write_borders_into(paragraph, &wanted.paragraph.borders, prefix.as_deref());
            format::write_shading_into(
                paragraph,
                wanted.paragraph.shading.as_deref(),
                prefix.as_deref(),
            );
        }
        let run = child_or_new(element, "rPr", prefix.as_deref());
        format::write_run_properties(run, &wanted.run, prefix.as_deref());

        if tree.root == before {
            return false;
        }
        self.record_styles_change();
        self.styles = Styles::parse(&tree.root).with_theme(self.styles.theme().clone());
        self.save_styles_tree(&tree);
        self.note_change();
        true
    }

    /// Copies a style from another document, exactly as it was written.
    ///
    /// Word's Organizer, which is how a style gets from a document into a
    /// template or the other way about. The element is lifted whole rather
    /// than read and written again: a style is full of properties this
    /// program does not model, and a copy that kept only what it understood
    /// would be a different style wearing the same name.
    ///
    /// A style of that identifier already here is replaced, which is what
    /// copying onto one means and what Word asks about first.
    pub fn copy_style_from(&mut self, other: &Document, id: &str) -> bool {
        let Some(source) = other.styles_tree() else { return false };
        let Some(wanted) = source
            .root
            .children_named(Some(read::W), "style")
            .find(|style| {
                style
                    .attribute(Some(read::W), "styleId")
                    .is_some_and(|found| found.eq_ignore_ascii_case(id))
            })
            .cloned()
        else {
            return false;
        };

        let Some(mut tree) = self.styles_tree() else { return false };
        let before = tree.root.clone();
        remove_style(&mut tree.root, id);
        tree.root.push_element(wanted);
        if tree.root == before {
            return false;
        }
        self.styles = Styles::parse(&tree.root).with_theme(self.styles.theme().clone());
        self.save_styles_tree(&tree);
        self.note_change();
        true
    }

    /// Takes a style out of the document.
    ///
    /// The paragraphs that used it are not touched: they name a style that is
    /// no longer there, and the format's answer to that is the document
    /// default, which is what Word leaves them looking like.
    pub fn delete_style(&mut self, id: &str) -> bool {
        let Some(mut tree) = self.styles_tree() else { return false };
        if !remove_style(&mut tree.root, id) {
            return false;
        }
        self.styles = Styles::parse(&tree.root).with_theme(self.styles.theme().clone());
        self.save_styles_tree(&tree);
        self.note_change();
        true
    }

    /// Gives a style another name to be shown under.
    ///
    /// The identifier stays what it was, because everything that uses the
    /// style refers to it by that: renaming the identifier would be renaming
    /// every paragraph's reference to it as well, and Word's Organizer
    /// renames what a person reads.
    pub fn rename_style(&mut self, id: &str, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        let Some(mut tree) = self.styles_tree() else { return false };
        let prefix = edit::prefix_for(&tree.root, WORDPROCESSING_NAMESPACE);
        let Some(style) = tree.root.child_elements_mut().find(|style| {
            style.is(Some(read::W), "style")
                && style
                    .attribute(Some(read::W), "styleId")
                    .is_some_and(|found| found.eq_ignore_ascii_case(id))
        }) else {
            return false;
        };

        style.remove_children_named(Some(read::W), "name");
        let mut element = Element::new(&edit::name_with(prefix.as_deref(), "name"), Some(read::W));
        element.set_namespaced_attribute(&edit::name_with(prefix.as_deref(), "val"), read::W, name);
        // The name goes first, which is where the schema puts it.
        style.insert_element(0, element);

        self.styles = Styles::parse(&tree.root).with_theme(self.styles.theme().clone());
        self.save_styles_tree(&tree);
        self.note_change();
        true
    }

    /// Which styles the document actually uses, by identifier.
    ///
    /// What Word's Styles pane shows when it is set to "In current document":
    /// a document made from a template carries a hundred styles and uses six,
    /// and a list of the hundred is a list nobody reads.
    #[must_use]
    pub fn styles_in_use(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for index in 0..self.paragraph_count() {
            if let Some(id) = self.style_of(index) {
                if !out.iter().any(|found| found.eq_ignore_ascii_case(&id)) {
                    out.push(id);
                }
            }
        }
        out
    }

    /// The styles part as a tree, however the document points at it.
    fn styles_tree(&self) -> Option<XmlTree> {
        related_tree(self.package(), self.main_part(), STYLES_RELATIONSHIP, "word/styles.xml")
            .or_else(|| XmlTree::parse(&default_styles()).ok())
    }

    /// Writes the styles part back where it came from.
    fn save_styles_tree(&mut self, tree: &XmlTree) {
        let Ok(xml) = tree.to_xml() else { return };
        let target = self.styles_part();
        self.package_mut().add_part(&target, STYLES_CONTENT_TYPE, xml.into_bytes());
    }

    /// The name of the part the styles live in, or the one they would go in.
    fn styles_part(&self) -> String {
        let main_part = self.main_part();
        self.package()
            .relationships(main_part)
            .ok()
            .and_then(|relationships| {
                let found = relationships.single_by_type(STYLES_RELATIONSHIP)?;
                found.resolved_target(main_part)?.ok()
            })
            .unwrap_or_else(|| "word/styles.xml".to_owned())
    }

    /// Records a step that keeps the styles part, before a change to it.
    ///
    /// Word takes back a style modified, a default set and a set of styles
    /// chosen, and a step that kept only the tree would take back none of
    /// them: the change is in the part.
    fn record_styles_change(&mut self) {
        let part = self.styles_part();
        self.record_with_parts(self.caret, &[&part]);
    }

    /// Records a step that keeps the settings part, before a change to it:
    /// hyphenation and the default tab stops are Word's to take back, and
    /// they live there.
    pub(crate) fn record_settings_change(&mut self) {
        let Some(part) = self.settings_part() else { return };
        self.record_with_parts(self.caret, &[&part]);
    }

    /// Reads again what is kept from the parts beside the document's own: the
    /// theme, the styles resolved against it, and the lists.
    ///
    /// Read once when the document is opened, and so again whenever a step
    /// puts those parts back.
    fn reread_what_parts_say(&mut self) {
        let theme = read_theme(&self.package, &self.document_part);
        self.styles = read_styles(&self.package, &self.document_part, theme);
        self.numbering = read_numbering(&self.package, &self.document_part);
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
            // Everything the Font dialog sets is part of the look as well, and
            // is carried the same way: written out rather than left unsaid, so
            // that painting plain text over expanded small capitals puts them
            // back to normal instead of leaving them as they were.
            double_strike: Some(resolved.double_strike),
            caps: Some(resolved.caps),
            small_caps: Some(resolved.small_caps),
            hidden: Some(resolved.hidden),
            no_proof: Some(resolved.no_proof),
            underline_color: resolved.underline_color,
            scale: Some(resolved.scale),
            spacing_twentieths: Some(resolved.spacing_twentieths),
            position_half_points: Some(resolved.position_half_points),
            kerning_half_points: resolved.kerning_half_points,
            open_type: Some(resolved.open_type),
            east_asian_layout: Some(resolved.east_asian_layout),
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
        if self.selection().is_none() {
            self.pending = self.pending.overlaid_with(change);
            return true;
        }
        self.format_selection(change)
    }

    /// Writes a character change over every selected stretch, as one step.
    ///
    /// Every stretch, and not only the one the caret is in: that is the whole
    /// point of being able to select more than one. One undo takes the lot
    /// back, because one press of the button put it there.
    fn format_selection(&mut self, change: &RunProperties) -> bool {
        let stretches = self.selections();
        let Some((first, _)) = stretches.first().copied() else { return false };

        // An undo step is only worth recording when there is something to take
        // back. Bolding text that is already bold changes nothing, and should
        // leave nothing behind either.
        let anything_to_do = stretches.iter().any(|(start, end)| {
            (start.paragraph..=end.paragraph).any(|index| {
                let (from, to) = self.range_within(index, *start, *end);
                self.paragraph_element(index).is_some_and(|paragraph| {
                    format::range_needs_change(paragraph, from, to, change)
                })
            })
        });
        if !anything_to_do {
            return false;
        }

        self.record(EditKind::Structural, first, false);
        let prefix = self.prefix();
        // One number for all of them: one press of Bold is one change to
        // review, however many stretches it landed on.
        let recording = self.recording_formatting();
        let mut changed = false;

        for (start, end) in stretches {
            for index in start.paragraph..=end.paragraph {
                let (from, to) = self.range_within(index, start, end);
                let Some(path) = position::paragraph_path(&self.tree.root, index) else { continue };
                let Some(paragraph) = edit::element_at_path_mut(&mut self.tree.root, &path) else {
                    continue;
                };
                changed |= format::apply_to_range(
                    paragraph,
                    from,
                    to,
                    change,
                    prefix.as_deref(),
                    recording.as_ref().map(|(reviser, id)| (reviser, *id)),
                );
            }
        }

        if changed {
            self.modified = true;
        }
        changed
    }

    /// Sets the style of every paragraph the selection touches.
    ///
    /// `None` removes the style, leaving the paragraph on the document default.
    ///
    /// A style the document has locked is refused while the restriction
    /// stands, the way a document restricted to tracked changes refuses to
    /// stop recording them: see [`crate::locking`].
    pub fn set_paragraph_style_here(&mut self, style: Option<&str>) -> bool {
        if !self.style_may_be_applied(style) {
            return false;
        }
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

    /// Everything the paragraph at the caret is formatted with, which is what
    /// the Paragraph dialog opens showing.
    #[must_use]
    pub fn paragraph_format_here(&self) -> ResolvedParagraphProperties {
        self.resolve_paragraph_here()
    }

    /// Applies a whole set of paragraph formatting at once.
    ///
    /// The Paragraph dialog is answered all together rather than a property at
    /// a time, and a change made in one go is one undo step and one pass over
    /// the paragraphs — which is what Word does, and what stops a dialog with
    /// twenty fields in it from filling the undo list with twenty entries.
    pub fn set_paragraph_format(&mut self, change: &ParagraphProperties) -> bool {
        let change = change.clone();
        self.change_paragraphs(move |paragraph, prefix| {
            format::apply_paragraph_properties(paragraph, &change, prefix);
        })
    }

    /// Word's Set As Default for a paragraph: makes this the formatting every
    /// paragraph that never said otherwise inherits.
    ///
    /// Written into `w:docDefaults`, under every style, exactly as
    /// [`Self::set_default_character_format`] writes the character half.
    pub fn set_default_paragraph_format(&mut self, change: &ParagraphProperties) -> bool {
        let Some(mut tree) = self.styles_tree() else { return false };

        let prefix = edit::prefix_for(&tree.root, WORDPROCESSING_NAMESPACE);
        let defaults = child_or_new(&mut tree.root, "docDefaults", prefix.as_deref());
        let paragraph_defaults = child_or_new(defaults, "pPrDefault", prefix.as_deref());
        let properties = child_or_new(paragraph_defaults, "pPr", prefix.as_deref());

        let before = properties.clone();
        format::write_paragraph_properties(properties, change, prefix.as_deref());
        if *properties == before {
            return false;
        }

        self.record_styles_change();
        self.styles = Styles::parse(&tree.root).with_theme(self.styles.theme().clone());
        self.save_styles_tree(&tree);
        self.note_change();
        true
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
        let stretches = self.selections();
        let Some((first, _)) = stretches.first().copied() else {
            self.pending = RunProperties::default();
            return true;
        };

        self.record(EditKind::Structural, first, false);
        let mut changed = false;
        for (start, end) in stretches {
            for index in start.paragraph..=end.paragraph {
                let (from, to) = self.range_within(index, start, end);
                let Some(path) = position::paragraph_path(&self.tree.root, index) else { continue };
                let Some(paragraph) = edit::element_at_path_mut(&mut self.tree.root, &path) else {
                    continue;
                };
                changed |= format::clear_run_properties(paragraph, from, to);
            }
        }

        if changed {
            self.modified = true;
        }
        changed
    }

    /// What two pieces of text have to share to count as set the same way.
    ///
    /// Kept apart from [`ResolvedRunProperties`] because that holds everything
    /// the format can say and this holds what a person can see. Two runs that
    /// differ only in which dictionary proofs them look identical, and Word's
    /// Select All Text With Similar Formatting picks up both.
    fn look_of(resolved: &ResolvedRunProperties) -> Look {
        Look {
            font: resolved.font.clone(),
            size_half_points: resolved.size_half_points,
            color: resolved.color.clone(),
            bold: resolved.bold,
            italic: resolved.italic,
            underlined: resolved.underline.is_visible(),
        }
    }

    /// Selects every stretch of text set the way the one at the caret is set.
    ///
    /// Word's Select All Text With Similar Formatting. "Similar" is Word's own
    /// word and its own vagueness; what is compared here is what a person can
    /// see and would call the same look — the typeface, the size, the colour,
    /// and whether it is bold, italic or underlined. Not the language, not the
    /// kerning, not which style it came from: two pieces of text that look
    /// identical are similar, whatever the file says about them.
    ///
    /// Returns how many stretches were found, the one at the caret included.
    pub fn select_similar(&mut self) -> usize {
        let wanted = Self::look_of(&self.resolved_over_selection());

        let mut found: Vec<(TextPosition, TextPosition)> = Vec::new();
        for index in 0..self.paragraph_count() {
            let Some(paragraph) = self.paragraph_element(index) else { continue };
            let length = self.paragraph_text(index).unwrap_or_default().len();
            for (from, to) in format::runs_in_range(paragraph, 0, length, &self.styles) {
                if Self::look_of(&to.1) != wanted {
                    continue;
                }
                let (start, end) = (TextPosition::new(index, from), TextPosition::new(index, to.0));
                // Runs that are next to each other and look the same are one
                // stretch: the file's run boundaries are not something anybody
                // put there on purpose.
                match found.last_mut() {
                    Some((_, last_end)) if *last_end == start => *last_end = end,
                    _ => found.push((start, end)),
                }
            }
        }

        if found.is_empty() {
            return 0;
        }
        let (first_start, first_end) = found[0];
        self.caret = first_end;
        self.anchor = Some(first_start);
        self.extra = found[1..].to_vec();
        self.pending = RunProperties::default();
        self.history.break_merge();
        found.len()
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
        self.put_table(&empty_table(rows, &vec![column_width; columns]), false)
    }

    /// Puts a table of one row in place of the paragraph the caret is in,
    /// with columns of the widths given, in twentieths of a point.
    ///
    /// What AutoFormat makes of `+---+---+` and Enter: the line of plus signs
    /// is not a paragraph any more but the table it drew, and the paragraph
    /// after it is where Enter would have gone.
    pub fn replace_paragraph_with_table(&mut self, widths: &[i32]) -> bool {
        if widths.is_empty() || widths.len() > 63 {
            return false;
        }
        let widths: Vec<i32> = widths.iter().map(|width| (*width).max(1)).collect();
        self.put_table(&empty_table(1, &widths), true)
    }

    /// Puts a table after the paragraph the caret is in, or in its place,
    /// with a paragraph after it — because a document that ends in a table
    /// has nowhere to put the caret afterwards — and the caret in the first
    /// cell, which is where a person expects to start typing.
    fn put_table(&mut self, table: &Table, in_place: bool) -> bool {
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

        let at = if in_place {
            parent.children.remove(position);
            position
        } else {
            position + 1
        };
        parent
            .insert_element(at, edit::paragraph_element(&Paragraph::default(), prefix.as_deref()));
        parent.insert_element(at, edit::table_element(table, prefix.as_deref()));

        let first_cell = if in_place { self.caret.paragraph } else { self.caret.paragraph + 1 };
        self.caret = TextPosition::new(first_cell, 0);
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
        self.record(EditKind::Structural, self.caret, false);
        let id = self.adopt_picture(bytes, extension)?;

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

    /// Takes a picture into the package without putting it anywhere in the
    /// text: the bytes become a part and a relationship points at them,
    /// and the relationship's id is given back for a drawing to refer to.
    ///
    /// What a paste from another program needs: the pasted paragraphs refer
    /// to pictures by relationship, so the pictures have to be here before
    /// the paragraphs are.
    pub fn adopt_picture(&mut self, bytes: &[u8], extension: &str) -> Result<String, Error> {
        let content_type = match extension.to_ascii_lowercase().as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "bmp" => "image/bmp",
            "tif" | "tiff" => "image/tiff",
            // The metafiles, which a document carries as often as any of the
            // others and which Word writes under these names.
            "emf" => "image/x-emf",
            "wmf" => "image/x-wmf",
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

        self.package.add_part(&name, content_type, bytes.to_vec());

        let mut relationships = self
            .package
            .relationships(&self.main_part)
            .unwrap_or_else(|_| Relationships::new(&self.main_part));
        let target = name.strip_prefix("word/").unwrap_or(&name).to_owned();
        let id = relationships.add(IMAGE_RELATIONSHIP, &target, TargetMode::Internal).id.clone();
        self.package.set_relationships(&relationships)?;
        Ok(id)
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

    /// Puts a part into the package with the content type it needs.
    pub(crate) fn add_package_part(&mut self, name: &str, content_type: &str, xml: String) {
        self.package.add_part(name, content_type, xml.into_bytes());
    }

    /// Points the document at a part of the package.
    pub(crate) fn point_at_part(
        &mut self,
        name: &str,
        relationship: &str,
    ) -> Result<String, Error> {
        let mut relationships = self
            .package
            .relationships(&self.main_part)
            .unwrap_or_else(|_| Relationships::new(&self.main_part));
        let target = name.strip_prefix("word/").unwrap_or(name).to_owned();
        let id = relationships.add(relationship, &target, TargetMode::Internal).id.clone();
        self.package.set_relationships(&relationships)?;
        Ok(id)
    }

    /// Points one part of the package at another.
    ///
    /// Every part may have relationships of its own, and some must: the
    /// drawing a diagram was laid out into is reached from the diagram's data
    /// model and from nowhere else.
    pub(crate) fn point_part_at(
        &mut self,
        source: &str,
        target: &str,
        relationship: &str,
    ) -> Result<String, Error> {
        let mut relationships =
            self.package.relationships(source).unwrap_or_else(|_| Relationships::new(source));
        let id = relationships.add(relationship, target, TargetMode::Internal).id.clone();
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

    /// The style of every paragraph, in one walk of the document.
    ///
    /// For whoever asks about every paragraph's neighbours in turn: asking
    /// [`Self::style_of`] eleven thousand times walks the tree eleven thousand
    /// times, and that is a keystroke that costs seconds.
    #[must_use]
    pub fn paragraph_styles(&self) -> Vec<Option<String>> {
        self.paragraph_elements()
            .into_iter()
            .map(|paragraph| {
                paragraph
                    .child(Some(read::W), "pPr")
                    .and_then(|properties| properties.child(Some(read::W), "pStyle"))
                    .and_then(read::value)
                    .map(str::to_owned)
            })
            .collect()
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

    /// Sets how far apart the default stops are.
    ///
    /// A document-wide setting rather than a paragraph one, which is why
    /// Word's Tabs dialog puts it beside the stops of this paragraph and not
    /// among them.
    pub fn set_default_tab_width(&mut self, twips: i32) -> bool {
        let twips = twips.clamp(1, 31_680);
        if self.default_tab_width() == twips {
            return false;
        }
        self.set_setting_value_as_step("defaultTabStop", Some(&twips.to_string()))
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
        // Every paragraph any stretch touches, each of them once: two stretches
        // in the same paragraph are one paragraph, and indenting it twice would
        // indent it twice as far.
        let mut wanted: Vec<usize> = self
            .selections()
            .into_iter()
            .flat_map(|(start, end)| start.paragraph..=end.paragraph)
            .collect();
        if wanted.is_empty() {
            wanted.push(self.caret.paragraph);
        }
        wanted.sort_unstable();
        wanted.dedup();

        if self.paragraph_count() == 0 {
            return false;
        }

        self.record(EditKind::Structural, self.caret, false);
        let prefix = self.prefix();
        // While changes are tracked, a paragraph's formatting is one of them:
        // what its properties said before is kept in a `w:pPrChange`, which is
        // what rejecting it puts back. One number for the lot, as one press of
        // a style is one change to review.
        let recording = self.recording_formatting();
        let mut changed = false;

        for index in wanted {
            let Some(path) = position::paragraph_path(&self.tree.root, index) else { continue };
            let Some(paragraph) = edit::element_at_path_mut(&mut self.tree.root, &path) else {
                continue;
            };
            let before = paragraph.child(Some(read::W), "pPr").cloned();
            change(paragraph, prefix.as_deref());
            if let Some((reviser, id)) = &recording {
                // The old properties back in place, and the new ones written
                // over them by what keeps a record of the old.
                let after = paragraph.child(Some(read::W), "pPr").cloned();
                if let Some(at) = paragraph.position_of(Some(read::W), "pPr") {
                    match before {
                        Some(before) => {
                            paragraph.children[at] = wp_xml::tree::Node::Element(before)
                        }
                        None => {
                            paragraph.children.remove(at);
                        }
                    }
                }
                format::note_properties_change(
                    paragraph,
                    "pPr",
                    after.as_ref(),
                    reviser,
                    *id,
                    prefix.as_deref(),
                );
            }
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
    ///
    /// One step to take back, like any other edit: a macro's `Paragraphs.Add`
    /// comes here, and Word takes back what a macro did.
    pub fn append_block(&mut self, block: &Block) -> bool {
        self.edit_tree_as_one_step(|root, prefix| {
            let Some(body) = read::find_body_mut(root) else { return false };
            edit::append_block(body, block, prefix);
            true
        })
    }

    /// Whether a style may be applied at all.
    ///
    /// Clearing the style - `None` - is putting the paragraph back on the
    /// document's own default, which is a style nobody can lock and the only
    /// way out of a locked one.
    fn style_may_be_applied(&self, style: Option<&str>) -> bool {
        style.is_none_or(|id| self.style_is_available(id))
    }

    /// Sets the style of the paragraph at a given index, or clears it.
    pub fn set_paragraph_style(&mut self, index: usize, style: Option<&str>) -> bool {
        if !self.style_may_be_applied(style) {
            return false;
        }
        self.edit_tree_as_one_step(|root, prefix| {
            read::find_body_mut(root)
                .is_some_and(|body| edit::set_paragraph_style(body, index, style, prefix))
        })
    }

    /// Sets the alignment of the paragraph at a given index.
    pub fn set_paragraph_alignment(&mut self, index: usize, alignment: Alignment) -> bool {
        self.edit_tree_as_one_step(|root, prefix| {
            read::find_body_mut(root)
                .is_some_and(|body| edit::set_paragraph_alignment(body, index, alignment, prefix))
        })
    }

    /// Records that the bytes from [`Self::save`] have actually been stored.
    ///
    /// This makes the package what [`Self::save`] wrote — the edited tree in
    /// its part, and no signatures if the document is no longer what was
    /// signed — and makes the present state of the history the saved one.
    /// Moving the saved state alone would be a quiet corruption: the package
    /// would still hold the *old* main part, so the next save would write the
    /// document as it was before the edits; and a package that kept its
    /// signatures would have the next save, finding nothing changed, write
    /// back a signature over a document it no longer describes.
    ///
    /// It also ends the step being typed. The saved state is a place undo has
    /// to be able to come back to, and it could not if the next word typed
    /// were folded into the step before the save.
    pub fn mark_saved(&mut self) -> Result<(), Error> {
        if self.is_as_saved() {
            return Ok(());
        }

        self.number_present();
        Self::write_into(&self.tree, &self.main_part, &mut self.package)?;
        self.history.mark_saved();
        self.history.break_merge();
        self.saved_generation = self.package.generation();
        self.accounted_generation = self.saved_generation;
        self.package_is_the_file = false;
        Ok(())
    }

    /// Writes the document back out.
    ///
    /// A document that is exactly what was opened is written from the
    /// package's own bytes, so it comes out identical. Any other has only its
    /// main part re-serialized, and every other part is written back exactly
    /// as the package holds it — except a part nothing reaches any more, which
    /// is left out, as Word leaves it out. See [`without_what_nothing_reaches`].
    pub fn save(&self) -> Result<Vec<u8>, Error> {
        if self.is_as_saved() && self.package_is_the_file {
            return Ok(self.package.save()?);
        }
        let mut package = self.package.clone();
        if !self.is_as_saved() {
            Self::write_into(&self.tree, &self.main_part, &mut package)?;
        }
        without_what_nothing_reaches(&mut package);
        Ok(package.save()?)
    }

    /// Makes a package what a document that is not the one on disk is saved
    /// as: the tree in its part, and no signatures.
    ///
    /// One place for both ways a save goes — the bytes written and the
    /// package kept afterwards — so that the two cannot come apart.
    ///
    /// # Why the signatures go
    ///
    /// A signature says the document is what it was when it was signed, and
    /// after an edit that is not true. Word takes the signatures off an
    /// edited document — it asks before the first edit, since editing is what
    /// takes them off — and leaving one would leave a claim in the file that
    /// the file disproves.
    fn write_into(tree: &XmlTree, part: &str, package: &mut Package) -> Result<(), Error> {
        let xml = tree.to_xml().map_err(|source| Error::Xml { part: part.to_owned(), source })?;
        package.set_part(part, xml.into_bytes());
        wp_sign::unsign(package);
        Ok(())
    }
}

/// The kinds of relationship a part names by identifier, from inside itself:
/// a picture's `r:embed`, a chart's `r:id`, an embedded object's.
///
/// A relationship of any other kind — the styles, the settings, a note part, a
/// header's own theme — is found by its kind and counts for as long as it is
/// there; one of these counts only while its part still names it. Ink is not
/// here, though its part names it too: see [`named_by_identifier`].
const NAMED_BY_IDENTIFIER: &[&str] = &[
    IMAGE_RELATIONSHIP,
    chart::CHART_RELATIONSHIP,
    diagram::DATA_RELATIONSHIP,
    diagram::LAYOUT_RELATIONSHIP,
    diagram::STYLE_RELATIONSHIP,
    diagram::COLORS_RELATIONSHIP,
    workbook::PACKAGE_RELATIONSHIP,
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/oleObject",
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/video",
    "http://schemas.microsoft.com/office/2007/relationships/media",
];

/// Whether a relationship is one its part names by identifier, and so counts
/// only while the part still names it.
///
/// # Why the kind alone does not say, for ink
///
/// Word reaches an ink part by the relationship it writes for a content part,
/// which is the very kind that reaches a custom XML data part — and nothing
/// names the data part by identifier: Word finds it by its kind, and a control
/// bound to it finds it by the GUID of its datastore item. Asking the document
/// whether it names such a relationship would take every custom XML part out
/// of the file, and its datastore item with it. So for that kind the part
/// reached decides, by what the package says it is: ink is named, and anything
/// else is kept for as long as its relationship is there, which is the side a
/// doubt has to fall on.
fn named_by_identifier(package: &Package, source: &str, relationship: &Relationship) -> bool {
    if NAMED_BY_IDENTIFIER.contains(&relationship.kind.as_str()) {
        return true;
    }
    relationship.kind == ink::INK_RELATIONSHIP
        && matches!(
            relationship.resolved_target(source),
            Some(Ok(target)) if package.content_type(&target) == Some(ink::INK_CONTENT_TYPE)
        )
}

/// Leaves out of a package being written every part that nothing reaches.
///
/// # Why a part is left behind, and why it goes
///
/// A part is added when a picture is inserted or pasted, and nothing takes it
/// out again: a picture cut, a paste undone, an old chart replaced by the one
/// pasted in its place all leave the part — and the relationship to it —
/// where they were, because undo may want them back while the document is
/// open. Word writes a file without them, and so does this: what is written
/// is what the package's relationships reach, walked from the package's own
/// (see [`wp_opc::Package::prune_unreachable`]), with one thing more — a
/// relationship of a kind a part names by its identifier counts only while
/// the part still names it, because a cut picture leaves its relationship
/// behind as well as its part. A diagram's drawing is not of those kinds:
/// Word names it from the diagram's data model rather than from the part the
/// relationship belongs to, so it is kept for as long as its relationship is.
fn without_what_nothing_reaches(package: &mut Package) {
    // The text of the part last looked at: a part's relationships are asked
    // about one after another, and a part is read once for all of them.
    let read: std::cell::RefCell<Option<(String, Option<String>)>> = std::cell::RefCell::new(None);
    package.prune_unreachable(|package, source, relationship| {
        if !named_by_identifier(package, source, relationship) {
            return true;
        }
        let mut read = read.borrow_mut();
        if read.as_ref().is_none_or(|(held, _)| held != source) {
            // Only a part the package says is XML is read for the names in
            // it: the bytes of a picture or a macro project can come out as
            // text by chance, and text that is not XML names nothing.
            let is_xml = package.content_type(source).is_some_and(|kind| kind.ends_with("xml"));
            let text = if is_xml { package.xml_part(source).and_then(Result::ok) } else { None };
            *read = Some((source.to_owned(), text));
        }
        // A part that cannot be read as text cannot be asked, and whatever
        // it points at stays.
        let Some((_, Some(text))) = read.as_ref() else { return true };
        let id = &relationship.id;
        text.contains(&format!("\"{id}\"")) || text.contains(&format!("'{id}'"))
    });
}

/// A table of empty cells with columns of the widths given.
///
/// Every cell states the width of its column, which is what Word writes and
/// what keeps a new table the width it was asked for. A cell's stated width is
/// a preference rather than a measurement, and it is what AutoFit Contents
/// clears to make the table hug what is in it: a table that stated nothing
/// would collapse to its contents the moment it was made, which is not what
/// asking for a three by three table means. See [`model::TableFit`].
fn empty_table(rows: usize, widths: &[i32]) -> Table {
    Table::from_rows(
        (0..rows)
            .map(|_| {
                TableRow::from_cells(
                    widths
                        .iter()
                        .map(|width| TableCell { width: Some(*width), ..TableCell::default() })
                        .collect(),
                )
            })
            .collect(),
    )
    .with_grid(widths.to_vec())
    .with_borders(model::TableBorders::grid())
}

/// The named child of an element, made if it is not there.
///
/// Used where a chain of elements has to exist before something can be written
/// at the bottom of it — `docDefaults`, then `rPrDefault`, then `rPr` — and
/// where any of them may be missing in a document that never needed it.
fn child_or_new<'a>(parent: &'a mut Element, local: &str, prefix: Option<&str>) -> &'a mut Element {
    if parent.child(Some(WORDPROCESSING_NAMESPACE), local).is_none() {
        parent.push_element(Element::new(
            &edit::name_with(prefix, local),
            Some(WORDPROCESSING_NAMESPACE),
        ));
    }
    parent.child_mut(Some(WORDPROCESSING_NAMESPACE), local).expect("just ensured")
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
pub(crate) fn related_tree(
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

    // The text effects and the OpenType features are in a namespace of their
    // own, and a prefix used but not declared is not XML at all. Declared only
    // when something in the body actually uses one, so a plain document carries
    // nothing it does not need. See [`effects`] and [`typography`].
    if uses_extensions(&root) {
        effects::declare_namespace(&mut root);
    }

    XmlTree {
        standalone: Some(true),
        has_declaration: true,
        doctype: None,
        before_root: Vec::new(),
        root,
        after_root: Vec::new(),
    }
}

/// Whether anything under this element is in Microsoft's extension namespace.
///
/// Asked of the finished tree rather than of the model, because the model can
/// carry a property that writes no element — "no effect", an empty set of
/// features — and a declaration for a namespace nothing uses is clutter.
fn uses_extensions(element: &Element) -> bool {
    element.namespace.as_deref() == Some(effects::W14)
        || element.child_elements().any(uses_extensions)
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
pub(crate) fn default_numbering() -> String {
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

/// Takes the style of an identifier out of a styles part, if it is there.
fn remove_style(root: &mut Element, id: &str) -> bool {
    let before = root.children.len();
    root.children.retain(|node| {
        node.as_element().is_none_or(|style| {
            !(style.is(Some(read::W), "style")
                && style
                    .attribute(Some(read::W), "styleId")
                    .is_some_and(|found| found.eq_ignore_ascii_case(id)))
        })
    });
    root.children.len() != before
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
        for element in edit::run_elements(&run, prefix.as_deref()) {
            field.push_element(element);
        }
        paragraph.insert_element(at, field);

        self.set_caret(TextPosition::new(caret.paragraph, caret.offset + shown.len()));
        self.note_change();
        true
    }
}

/// What a person sees when they look at a piece of text.
///
/// The comparison behind Select All Text With Similar Formatting. See
/// [`Document::look_of`] for why it is not simply the resolved properties.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Look {
    font: Option<String>,
    size_half_points: u32,
    color: Option<String>,
    bold: bool,
    italic: bool,
    underlined: bool,
}
