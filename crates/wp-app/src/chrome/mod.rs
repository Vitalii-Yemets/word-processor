//! The window's own furniture: the ribbon, the rulers, the navigation pane and
//! the status bar.
//!
//! Everything here is drawn by the same engine that draws the document — the
//! same fonts, the same rasterizer, the same canvas. There is no widget toolkit
//! underneath, and there is no second way to put a pixel on screen.
//!
//! # Why the buttons are drawn in what they do
//!
//! The bold button is a letter B set in bold, the italic button a letter I set
//! in italic, and a style in the gallery is shown in its own formatting. A
//! button that shows its own effect needs no label and no translation, which
//! matters for a program meant to be used in every language Word supports.

pub mod backstage;
pub mod basicpane;
pub mod customise;
pub mod dialog;
pub mod findbar;
pub mod grid;
mod icon_catalogue;
pub mod icons;
pub mod infobar;
pub mod keytips;
pub mod mappingpane;
pub mod minibar;
pub mod mirror;
pub mod navigation;
pub mod palette;
pub mod pane;
pub mod pastebadge;
pub mod popup;
pub mod printpane;
pub mod recoverypane;
pub mod restrictpane;
pub mod ribbon;
pub mod rulers;
pub mod scrollbar;
pub mod signaturepane;
pub mod status;
pub mod stylespane;
pub mod textpane;
pub mod theme;
pub mod tip;
pub mod titlebar;
pub mod userform;

use wp_docx::model::Alignment;
use wp_docx::CharacterFormat;
use wp_raster::Color;

pub use customise::Customisation;
pub use grid::TableGrid;
pub use minibar::MiniBar;
pub use navigation::Navigation;
pub use popup::{Choice, Popup, SIZES, ZOOMS};
pub use ribbon::{Ribbon, TOTAL_HEIGHT as RIBBON_TOTAL};
pub use rulers::{HORIZONTAL_HEIGHT, VERTICAL_WIDTH};
pub use scrollbar::{ScrollBar, THICKNESS as SCROLLBAR_THICKNESS};
pub use status::STATUS_HEIGHT;
pub use theme::{Mode, Theme};
pub use tip::Tip;
pub use titlebar::{TitleBar, WindowButton, HEIGHT as TITLE_HEIGHT};

/// What pressing something on the ribbon does.
///
/// A variant per command, and every one of them does something: there is no
/// button on the ribbon that reports itself as unbuilt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    // File.
    New,
    Open,
    Save,
    SaveAs,
    Print,
    CloseDocument,
    About,

    // Clipboard and history.
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    /// The four answers Word's paste options offer. See
    /// [`crate::editor`]'s `paste` module for what each one means.
    PasteKeepSource,
    PasteMerge,
    PasteAsPicture,
    PasteTextOnly,
    FormatPainter,

    // Font.
    Format(CharacterFormat),
    GrowFont,
    ShrinkFont,
    ChangeCase,
    ClearFormatting,
    TextEffects,
    SetAsDefault,
    MatchFields,
    Split,
    SideToSide,
    OutlineView,
    Screenshot,
    BlockAuthors,
    /// The stretch of a protected document that stays editable.
    AllowEveryone,
    Feedback,
    ShowTraining,
    WhatsNew,
    SignatureLine,
    TextFromFile,
    SmartArt,
    /// The SmartArt Design tab: the words of a chosen diagram, and how it
    /// is drawn.
    DiagramAddShape,
    DiagramPromote,
    DiagramDemote,
    DiagramMoveUp,
    DiagramMoveDown,
    DiagramRightToLeft,
    DiagramTextPane,
    DiagramLayouts,
    DiagramColours,
    DiagramReset,
    /// And its Format tab: the frame grown or shrunk.
    DiagramLarger,
    DiagramSmaller,
    /// The Draw tab: Select, the two erasers, the three pens, and what the
    /// ink can be turned into.
    DrawSelect,
    DrawEraser,
    DrawPen,
    DrawPencil,
    DrawHighlighter,
    InkToShape,
    InkToText,
    Macros,
    OnlineVideo,
    Equation,
    Rules,
    Chart,
    Effects,
    NewWindow,
    ArrangeAll,
    Translate,
    /// One group of the ribbon, shown as a single button because the window is
    /// too narrow for all of them. The number is its place in the tab.
    ExpandGroup(u8),
    Subscript,
    Superscript,
    Highlight,
    TextColor,

    // Paragraph.
    Align(Alignment),
    Bullets,
    Numbering,
    MultilevelList,
    IndentMore,
    IndentLess,
    Sort,
    ShowMarks,
    LineSpacing,
    /// Word's Design tab: the spacing of the whole document, rather than of
    /// the paragraph the caret is in.
    DocumentSpacing,
    Shading,
    Borders,
    /// The border round the pages, which is not the border round a paragraph.
    PageBorders,

    /// Applies the style at this place in the gallery.
    ///
    /// An index rather than a name because the gallery is the document's own
    /// styles, read afresh every time it is drawn — a name here would have to
    /// outlive the document it came from.
    Style(usize),

    // Editing.
    Find,
    Replace,
    SelectAll,
    /// Every stretch of text set the way the one at the caret is set. Word's
    /// Select All Text With Similar Formatting, and the reason a selection has
    /// to be able to hold more than one stretch.
    SelectSimilar,
    /// Turns the pointer into one that chooses drawings rather than putting the
    /// caret in text. A mode, like the format painter: see
    /// [`crate::editor::handles`].
    SelectObjects,

    // Insert.
    PageBreak,
    BlankPage,
    InsertTable,
    InsertPicture,
    InsertSymbol,
    InsertDate,
    Header,
    Footer,
    PageNumber,

    // Layout.
    Margins,
    Orientation,
    PageSize,
    Columns,
    /// The page, column and section breaks, which drop open as a list.
    Breaks,
    /// The four measurement boxes on the Layout tab.
    IndentLeftBox,
    IndentRightBox,
    SpaceBeforeBox,
    SpaceAfterBox,

    // Tables, on the two contextual tabs.
    InsertRowAbove,
    InsertRowBelow,
    InsertColumnLeft,
    InsertColumnRight,
    DeleteRow,
    DeleteColumn,
    DeleteTable,
    TableBorders(TableBorderChoice),
    /// The gallery of table styles.
    TableStyles,
    /// The two measurement boxes of the Table Layout tab.
    RowHeightBox,
    /// And the two on the Header & Footer tab.
    HeaderFromTopBox,
    FooterFromBottomBox,
    /// Word's Insert Alignment Tab, which drops a tab that goes to the middle
    /// of the line or to its far end whatever the tab stops say.
    AlignmentTab,
    ColumnWidthBox,
    /// The nine alignments of a cell: across and down in one press.
    AlignCell(u8),
    /// Which part of a table to select.
    SelectTablePart,
    /// Whether the first row repeats at the top of every page.
    RepeatHeaderRow,
    /// Whether the boundaries of a borderless table are drawn.
    ViewGridlines,
    /// The table as ordinary paragraphs.
    ConvertToText,
    TableHeaderRow,
    TableBandedRows,
    TableTotalRow,
    TableFirstColumn,
    TableLastColumn,
    TableBandedColumns,
    MergeCells,
    SplitCells,
    DistributeColumns,
    /// Word's AutoFit menu: how a table decides how wide it is.
    AutoFit,
    /// Turns the text in the cell at the caret a right angle. Word's Text
    /// Direction, which is a button that cycles rather than a menu.
    TextDirection,
    /// Arithmetic over the cells of a table: Word's Formula.
    Formula,
    /// Lines the chosen drawings up with each other, the page or the margins.
    AlignObjects,
    /// Turns the chosen drawings, or mirrors them: Word's Rotate menu.
    RotateObjects,
    /// Makes one drawing of several, takes it apart, or puts it back together:
    /// Word's Group menu.
    GroupObjects,

    // References.
    InsertContents,
    UpdateContents,
    RemoveContents,
    InsertFootnote,
    InsertEndnote,
    NextNote,
    DeleteNote,
    InsertCaption,
    CrossReference,
    PageReference,
    AddBookmark,
    TableOfFigures,
    MarkIndexEntry,
    InsertIndex,
    InsertCitation,
    AddSource,
    ManageSources,
    InsertBibliography,
    InsertLink,
    RemoveLink,
    PageColor,
    Watermark,
    DocumentProperties,
    CoverPage,
    MarkCitation,
    TableOfAuthorities,
    Themes,
    ThemeColors,
    ThemeFonts,
    Language,
    ReadMode,
    DraftView,
    InsertShape,
    InsertTextBox,
    WrapText,
    Position,
    QuickParts,
    WordArt,
    SelectionPane,
    StartMailMerge,
    SelectRecipients,
    EditRecipientList,
    InsertMergeField,
    AddressBlock,
    GreetingLine,
    PreviewResults,
    NextRecipient,
    PreviousRecipient,
    CheckMergeErrors,
    FinishMerge,
    Compare,
    Spelling,
    /// One of the spellings offered for the word under the menu, by its place
    /// in the list.
    Correct(u8),
    /// Leave this word alone everywhere in the document.
    IgnoreAll,
    AddToDictionary,
    /// The words that mean what the word at the caret means.
    Thesaurus,
    /// One of them, by its place in the list.
    Synonym(u8),
    /// One of the additional actions the right-click menu offers for what
    /// was recognised under the pointer.
    Action(u8),
    /// What the words selected are in another language, which is what
    /// Word's Translate offers first and its right-click menu offers too.
    TranslateSelection,
    /// Word's AutoFormat: the dialog that asks whether to review.
    AutoFormat,
    /// And its AutoFormat Now, which does not ask.
    AutoFormatNow,
    ShowProofing,
    LoadDictionary,
    CheckAccessibility,
    HighlightMergeFields,
    Envelopes,
    Labels,
    TableProperties,
    /// Word's two table pens: the one that draws a line between cells and the
    /// one that rubs a line out. See [`crate::editor`]'s `borderpainter` module.
    DrawTable,
    Eraser,
    /// Word's Border Styles gallery and the pen that draws with what it
    /// offers. See [`crate::editor`]'s `borderpainter` module.
    BorderStyles,
    BorderPainter,
    /// The six of Word's two Arrange menus: one step through the pile, all the
    /// way to one end of it, or out of the pile altogether and in front of or
    /// behind the text.
    BringForward,
    BringToFront,
    BringInFrontOfText,
    SendBackward,
    SendToBack,
    SendBehindText,
    LineNumbers,
    Hyphenation,
    /// Which way the text of the section runs: across the page, or down it.
    TextDirectionSection,
    /// Word's Asian Layout menu: a run set across a vertical line, or as two
    /// lines in one.
    AsianLayout,

    // The Header & Footer tab, which is only there while one is being edited.
    GoToHeader,
    GoToFooter,
    LinkToPrevious,
    DifferentFirstPage,
    DifferentOddEven,
    CloseFurniture,
    /// How the section numbers its pages: the figures, and where it starts.
    FormatPageNumbers,
    RestrictEditing,

    // Review.
    WordCount,
    NewComment,
    DeleteComment,
    PreviousComment,
    NextComment,
    ShowComments,
    TrackChanges,
    ShowMarkup,
    ReviewingPane,
    AcceptChange,
    RejectChange,
    AcceptAll,
    RejectAll,

    // View.
    ToggleRulers,
    ToggleNavigation,
    ToggleTheme,
    Gridlines,
    PrintLayout,
    WebLayout,
    /// Collapses the space between one page and the next.
    JoinPages,
    ZoomIn,
    ZoomOut,
    ZoomHundred,
    OnePage,
    PageWidth,

    /// Word's Font dialog, behind the launcher in the corner of the Font group
    /// and behind Ctrl+D.
    FontDialog,
    /// Word's Paragraph dialog, behind the launcher in the corner of the
    /// Paragraph group.
    ParagraphDialog,
    /// The styles pane down the right-hand side, behind the launcher in the
    /// corner of the Styles group and behind Ctrl+Alt+Shift+S.
    StylesPane,
    /// What the program does rather than what the document says: Word's
    /// Options, under the File tab.
    Options,

    /// Opens one of the lists.
    ChooseFont,
    ChooseSize,
    ChooseStyle,
    ChooseZoom,
    /// One of the content controls, by its place in
    /// `wp_docx::controls::ControlKind::ALL`.
    Control(usize),
    /// The three fields a form was made of before content controls.
    LegacyFields,
    /// What a content control is called, and what may be done to it.
    ControlProperties,
    /// The section that repeats, put round the selection.
    RepeatingSection,
    /// A copy of the repeating item the caret is in, before it or after
    /// it, and the item taken away.
    RepeatItemBefore,
    RepeatItemAfter,
    DeleteRepeatItem,
    /// A control that offers a gallery of building blocks.
    GalleryControl,
    /// Word's Design Mode: the tags on every control, and placeholders
    /// written in place.
    DesignMode,
    /// Word's XML Mapping pane: the data a document carries, and binding a
    /// control to a node of it.
    XmlMapping,
    /// Which set of styles the whole document is formatted with.
    StyleSet,
}

impl Command {
    /// Whether the command may still be used on a document that is protected.
    ///
    /// Named the safe way round on purpose: a command added later is refused
    /// until somebody has thought about it, rather than let through because
    /// nobody remembered to add it to a list.
    #[must_use]
    pub fn is_allowed_when_locked(self) -> bool {
        matches!(
            self,
            // Reading, moving about and looking at things.
            Self::Find
                | Self::Replace
                | Self::SelectAll
                | Self::Copy
                | Self::WordCount
                | Self::ShowMarks
                | Self::ShowMarkup
                | Self::ShowComments
                | Self::ReviewingPane
                | Self::PreviousComment
                | Self::NextComment
                | Self::NextNote
                | Self::ToggleRulers
                | Self::ToggleNavigation
                | Self::ToggleTheme
                | Self::Gridlines
                | Self::PrintLayout
                | Self::WebLayout
                | Self::JoinPages
                | Self::ZoomIn
                | Self::ZoomOut
                | Self::ZoomHundred
                | Self::OnePage
                | Self::PageWidth
                | Self::ChooseZoom
                // Getting the document out of the program, which changes
                // nothing in it.
                | Self::New
                | Self::Open
                | Self::Save
                | Self::SaveAs
                | Self::Print
                | Self::CloseDocument
                | Self::About
                // And the one that lifts the restriction, or there would be no
                // way back.
                | Self::RestrictEditing
        )
    }

    /// Whether the command only makes a comment or takes one away.
    ///
    /// The one thing a document restricted to comments is for.
    #[must_use]
    pub fn is_a_comment(self) -> bool {
        matches!(self, Self::NewComment | Self::DeleteComment)
    }

    /// Whether it takes back what was just done, or puts it back.
    #[must_use]
    pub fn is_undoing(self) -> bool {
        matches!(self, Self::Undo | Self::Redo)
    }

    /// Whether it moves text through the clipboard.
    #[must_use]
    pub fn is_clipboard(self) -> bool {
        matches!(self, Self::Cut | Self::Paste | Self::PasteTextOnly)
    }

    /// Whether the command writes formatting straight on to the text.
    ///
    /// The Font group, the Paragraph group, and the two dialogs behind them:
    /// everything that makes a word look different without a style saying so.
    /// This is what "Limit formatting to a selection of styles" forbids, and
    /// the list is written out rather than worked out because a button added
    /// later must be thought about rather than let through.
    #[must_use]
    pub fn is_direct_formatting(self) -> bool {
        matches!(
            self,
            // The Font group.
            Self::Format(_)
                | Self::GrowFont
                | Self::ShrinkFont
                | Self::ChangeCase
                | Self::ClearFormatting
                | Self::TextEffects
                | Self::Subscript
                | Self::Superscript
                | Self::Highlight
                | Self::TextColor
                | Self::ChooseFont
                | Self::ChooseSize
                | Self::FontDialog
                | Self::SetAsDefault
                // The Paragraph group.
                | Self::Align(_)
                | Self::Bullets
                | Self::Numbering
                | Self::MultilevelList
                | Self::IndentMore
                | Self::IndentLess
                | Self::LineSpacing
                | Self::DocumentSpacing
                | Self::Shading
                | Self::Borders
                | Self::ParagraphDialog
                | Self::IndentLeftBox
                | Self::IndentRightBox
                | Self::SpaceBeforeBox
                | Self::SpaceAfterBox
                // Carrying formatting from one place to another is still
                // writing it.
                | Self::FormatPainter
                // And the same formatting applied to a table.
                | Self::TableStyles
                | Self::TableBorders(_)
                | Self::BorderStyles
                | Self::BorderPainter
                | Self::PageBorders
        )
    }

    /// Whether the command changes the theme, which is formatting written
    /// once for the whole document.
    #[must_use]
    pub fn is_theme_switching(self) -> bool {
        matches!(self, Self::Themes | Self::ThemeColors | Self::ThemeFonts | Self::PageColor)
    }

    /// Whether it changes the set of styles the whole document is formatted
    /// with, which is every heading in it at once.
    #[must_use]
    pub fn is_style_set_switching(self) -> bool {
        matches!(self, Self::StyleSet)
    }

    /// Whether a limit on the formatting lets this command run.
    #[must_use]
    pub fn is_allowed_by(self, limits: wp_docx::protection::Limits) -> bool {
        !(limits.formatting && self.is_direct_formatting())
            && !(limits.theme && self.is_theme_switching())
            && !(limits.style_set && self.is_style_set_switching())
    }

    /// Whether a restriction on the document lets this command run.
    ///
    /// `here` says whether the place the caret is in may be edited at all,
    /// which only a form protection makes a question.
    ///
    /// Written as an allow-list on purpose. A list of what is forbidden goes
    /// out of date the next time a button is added, and it goes out of date
    /// silently and in the dangerous direction.
    ///
    /// One answer, asked twice: the ribbon asks it to know whether to grey a
    /// button out, and the editor asks it to know whether to run one. A
    /// button that looks pressable and does nothing is the thing this is for.
    #[must_use]
    pub fn is_allowed_under(
        self,
        restriction: Option<wp_docx::protection::EditMode>,
        limits: wp_docx::protection::Limits,
        here: bool,
    ) -> bool {
        use wp_docx::protection::EditMode;
        // The two halves of Word's dialog are independent: a document may
        // limit its formatting and let anybody type, or the other way about.
        if !self.is_allowed_by(limits) {
            return false;
        }
        let Some(mode) = restriction else { return true };
        if self.is_allowed_when_locked() {
            return true;
        }
        match mode {
            EditMode::ReadOnly => false,
            // Undoing is allowed so that a comment can be taken back the way
            // everything else in this program is taken back.
            EditMode::Comments => self.is_a_comment() || self.is_undoing(),
            // Everything, because everything will be recorded.
            EditMode::TrackedChanges => true,
            // Filling in a form is typing, and typing is not a command. What
            // is let through is what somebody filling one in still needs, and
            // only where they are allowed to be.
            EditMode::Forms => (self.is_undoing() || self.is_clipboard()) && here,
        }
    }
}

/// Which lines a table draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableBorderChoice {
    All,
    Outside,
    None,
}

/// One style as the gallery shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct StyleSample {
    /// The identifier to apply, or `None` for the body style.
    pub id: Option<String>,
    pub name: String,
    pub bold: bool,
    pub italic: bool,
    /// The size the style asks for, in points, so the tile can hint at it.
    pub size: f32,
}

/// What the ribbon needs to know about the document to draw itself.
#[derive(Clone, Debug)]
pub struct ToolbarState {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub subscript: bool,
    pub superscript: bool,
    pub alignment: Alignment,
    /// The style of the paragraph the caret is in, if it has one.
    pub style: Option<String>,
    /// The styles the gallery offers, which are the document's own.
    pub styles: Vec<StyleSample>,
    /// The font in use where the caret is, and its size in points.
    pub font: Option<String>,
    pub size: f32,
    pub zoom: f32,
    pub can_undo: bool,
    pub can_redo: bool,
    pub modified: bool,
    pub has_selection: bool,
    pub show_marks: bool,
    /// Whether the writing is being checked.
    pub show_proofing: bool,
    pub show_rulers: bool,
    pub show_navigation: bool,
    pub show_gridlines: bool,
    pub joined_pages: bool,
    /// What the current view mode is called, so the button matching it can be
    /// shown as pressed.
    pub view: &'static str,
    /// Whether the dark theme is the one in use.
    pub dark_theme: bool,
    /// Whether the format painter is holding formatting to apply.
    pub painting: bool,
    /// Whether every edit is being recorded as a tracked change.
    pub tracking_changes: bool,
    /// Whether tracked changes are drawn as changes.
    pub show_markup: bool,
    /// Whether the border painter is in hand, which lights its button the way
    /// the format painter's is lit.
    pub painting_borders: bool,
    /// And which of the two table pens is, which lights one of their buttons.
    pub drawing_table: bool,
    pub erasing: bool,
    /// Whether the comments are listed in the pane.
    pub show_comments: bool,
    /// Whether Design Mode is on, which lights its button.
    pub design_mode: bool,
    /// What the document's restriction lets be pressed, if it has one.
    pub restricted: Option<wp_docx::protection::EditMode>,
    /// And what it forbids the formatting to be changed with.
    pub limits: wp_docx::protection::Limits,
    /// Whether the place the caret is in may be edited. Only a form
    /// protection makes that a question: everything else is the same answer
    /// everywhere in the document.
    pub can_edit_here: bool,
    /// The colours the two coloured buttons would apply.
    pub text_color: Color,
    pub highlight_color: Color,
    /// Which parts of the table at the caret its style may treat specially,
    /// so the six switches on the Table Design tab can show as pressed.
    pub table_look: wp_docx::model::TableLook,
    /// Whether the boundaries of a borderless table are drawn.
    pub show_table_gridlines: bool,
    /// Whether the first row repeats on every page.
    pub repeat_header_row: bool,
    /// How tall the row at the caret is and how wide its cell, in twentieths
    /// of a point. Zero where the table has not been told.
    /// How far the header sits from the top of the paper and the footer from
    /// the bottom, in twentieths of a point.
    pub header_from_top: i32,
    pub footer_from_bottom: i32,
    pub row_height: i32,
    pub column_width: i32,
    /// Which of the nine cell alignments is in force, if one of them is.
    pub cell_alignment: Option<u8>,
    /// The indents of the paragraph at the caret, in twentieths of a point.
    pub indent_left: i32,
    pub indent_right: i32,
    /// And the room above and below it, in the same unit.
    pub space_before: i32,
    pub space_after: i32,
    /// What unit a measurement is shown in. See [`crate::measure`].
    pub unit: crate::measure::Unit,
    /// The box on the ribbon that has the keyboard, and what has been typed
    /// into it so far.
    ///
    /// Kept here rather than in the ribbon because the ribbon is drawn afresh
    /// from this every time: what is being typed is a fact about the editor,
    /// like the caret in the document.
    pub typing: Option<(Command, String)>,
    /// Whether the caret is in a table, which is what shows the two
    /// contextual tabs.
    pub in_table: bool,
    /// Whether a header or a footer is being edited, which is what puts the
    /// Header & Footer tab on the ribbon.
    pub in_furniture: bool,
    /// Whether a diagram is chosen, which is when its two tabs show.
    pub in_diagram: bool,
    /// Whether the diagram's Text Pane is open, for its button to show
    /// pressed.
    pub text_pane_open: bool,
    /// Which of the Draw tab's tools is in hand, for its button to show
    /// pressed: the pen, the pencil, the highlighter, an eraser — and
    /// Select Objects, which the tab's Select is.
    pub pen_in_hand: bool,
    pub pencil_in_hand: bool,
    pub highlighter_in_hand: bool,
    pub ink_eraser_in_hand: bool,
    pub choosing_drawings: bool,
    /// Which list is dropped open, so its field stays lit while it is.
    pub open: Option<Choice>,
}

impl ToolbarState {
    /// What one of the measurement boxes on the Layout tab reads.
    ///
    /// In centimetres, as Word shows them in a metric locale, because the
    /// underlying unit — twentieths of a point — is meaningless to anyone.
    #[must_use]
    pub fn measure(&self, command: Command) -> String {
        // What is being typed wins over what the document says: while a box has
        // the keyboard it shows the keystrokes, not the paragraph.
        if let Some((box_command, typed)) = &self.typing {
            if *box_command == command {
                return typed.clone();
            }
        }

        match command {
            // An indent is a length, and Word shows a length in whatever unit
            // its Options were set to.
            Command::IndentLeftBox => self.length(self.indent_left),
            Command::IndentRightBox => self.length(self.indent_right),
            // The room above and below a paragraph is in points whatever that
            // setting says, because that is what Word does with it — a person
            // who asked for centimetres did not ask for six-hundredths of one.
            Command::SpaceBeforeBox => points(self.space_before),
            // A row and a column are lengths like an indent, so they follow
            // the same unit.
            Command::RowHeightBox => self.length(self.row_height),
            Command::ColumnWidthBox => self.length(self.column_width),
            Command::HeaderFromTopBox => self.length(self.header_from_top),
            Command::FooterFromBottomBox => self.length(self.footer_from_bottom),
            Command::SpaceAfterBox => points(self.space_after),
            _ => String::new(),
        }
    }

    /// A length as its box shows it, with the mark for the unit.
    fn length(&self, twips: i32) -> String {
        format!("{}{}", crate::measure::format(twips, self.unit), self.unit.mark())
    }
}

/// A measurement in points, as the boxes that are always in points show it.
///
/// Written without a fraction where there is none, because Word writes "6 pt"
/// and not "6.0 pt", and a box of round numbers reads as round numbers.
#[must_use]
fn points(twips: i32) -> String {
    let points = f64::from(twips) / 20.0;
    if (points.fract()).abs() < 0.005 {
        format!("{points:.0} pt")
    } else {
        format!("{points:.1} pt")
    }
}

/// Whether a command is currently in effect, so its button shows as on.
#[must_use]
pub fn is_active(command: Command, state: &ToolbarState) -> bool {
    match command {
        Command::Format(CharacterFormat::Bold) => state.bold,
        Command::Format(CharacterFormat::Italic) => state.italic,
        Command::Format(CharacterFormat::Underline) => state.underline,
        Command::Format(CharacterFormat::Strikethrough) => state.strike,
        Command::Subscript => state.subscript,
        Command::Superscript => state.superscript,
        Command::Align(alignment) => state.alignment == alignment,
        Command::Style(index) => {
            state.styles.get(index).is_some_and(|sample| sample.id == state.style)
        }
        Command::FormatPainter => state.painting,
        Command::ShowMarks => state.show_marks,
        Command::ToggleRulers => state.show_rulers,
        Command::ToggleNavigation => state.show_navigation,
        Command::ToggleTheme => state.dark_theme,
        Command::Gridlines => state.show_gridlines,
        Command::JoinPages => state.joined_pages,
        Command::TrackChanges => state.tracking_changes,
        // The six Table Style Options show as pressed while they are on.
        Command::TableHeaderRow => state.table_look.first_row,
        Command::TableTotalRow => state.table_look.last_row,
        Command::TableFirstColumn => state.table_look.first_column,
        Command::TableLastColumn => state.table_look.last_column,
        Command::TableBandedRows => state.table_look.banded_rows,
        Command::TableBandedColumns => state.table_look.banded_columns,
        Command::ViewGridlines => state.show_table_gridlines,
        Command::RepeatHeaderRow => state.repeat_header_row,
        Command::AlignCell(which) => state.cell_alignment == Some(which),
        Command::ShowMarkup => state.show_markup,
        Command::BorderPainter => state.painting_borders,
        Command::DiagramTextPane => state.text_pane_open,
        Command::DrawPen => state.pen_in_hand,
        Command::DrawPencil => state.pencil_in_hand,
        Command::DrawHighlighter => state.highlighter_in_hand,
        Command::DrawEraser => state.ink_eraser_in_hand,
        Command::DrawSelect | Command::SelectObjects => state.choosing_drawings,
        Command::DrawTable => state.drawing_table,
        Command::Eraser => state.erasing,
        Command::ShowProofing => state.show_proofing,
        Command::ReviewingPane | Command::ShowComments => state.show_comments,
        Command::DesignMode => state.design_mode,
        Command::WebLayout => state.view_is("Web layout"),
        Command::PrintLayout => state.view_is("Print layout"),
        Command::DraftView => state.view_is("Draft"),
        Command::ReadMode => state.view_is("Read mode"),
        Command::ChooseFont => state.open == Some(Choice::Font),
        Command::ChooseSize => state.open == Some(Choice::Size),
        Command::ChooseStyle => state.open == Some(Choice::Style),
        Command::ChooseZoom => state.open == Some(Choice::Zoom),
        _ => false,
    }
}

/// Whether a command can be used at all right now.
#[must_use]
pub fn is_enabled(command: Command, state: &ToolbarState) -> bool {
    if !command.is_allowed_under(state.restricted, state.limits, state.can_edit_here) {
        return false;
    }
    match command {
        Command::Undo => state.can_undo,
        Command::Redo => state.can_redo,
        Command::Save => state.modified,
        Command::Cut | Command::Copy => state.has_selection,
        _ => true,
    }
}

/// Writes a size the way a person does: without a decimal point unless there is
/// half a point in it.
#[must_use]
pub fn format_size(points: f32) -> String {
    if (points - points.round()).abs() < 0.01 {
        format!("{}", points.round() as i32)
    } else {
        format!("{points:.1}")
    }
}

impl ToolbarState {
    /// Whether the view mode is the one named.
    ///
    /// Compared by name rather than by type because the ribbon knows what the
    /// buttons are called and nothing else about how a document is shown.
    #[must_use]
    pub fn view_is(&self, name: &str) -> bool {
        self.view == name
    }
}
