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

pub mod dialog;
pub mod findbar;
pub mod grid;
mod icon_catalogue;
pub mod icons;
pub mod keytips;
pub mod minibar;
pub mod navigation;
pub mod palette;
pub mod popup;
pub mod printpane;
pub mod ribbon;
pub mod rulers;
pub mod scrollbar;
pub mod status;
pub mod theme;
pub mod tip;
pub mod titlebar;

use wp_docx::model::Alignment;
use wp_docx::CharacterFormat;
use wp_raster::Color;

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
    Feedback,
    ShowTraining,
    WhatsNew,
    SignatureLine,
    TextFromFile,
    SmartArt,
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
    Shading,
    Borders,

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
    /// The two indent boxes on the Layout tab.
    IndentLeftBox,
    IndentRightBox,

    // Tables, on the two contextual tabs.
    InsertRowAbove,
    InsertRowBelow,
    InsertColumnLeft,
    InsertColumnRight,
    DeleteRow,
    DeleteColumn,
    DeleteTable,
    TableBorders(TableBorderChoice),
    TableHeaderRow,
    TableBandedRows,
    MergeCells,
    SplitCells,
    DistributeColumns,

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
    ShowProofing,
    LoadDictionary,
    CheckAccessibility,
    HighlightMergeFields,
    Envelopes,
    Labels,
    TableProperties,
    BringForward,
    SendBackward,
    LineNumbers,
    Hyphenation,

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

    /// Opens one of the lists.
    ChooseFont,
    ChooseSize,
    ChooseStyle,
    ChooseZoom,
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
    /// Whether the comments are listed in the pane.
    pub show_comments: bool,
    /// The colours the two coloured buttons would apply.
    pub text_color: Color,
    pub highlight_color: Color,
    /// The indents of the paragraph at the caret, in twentieths of a point.
    pub indent_left: i32,
    pub indent_right: i32,
    /// Whether the caret is in a table, which is what shows the two
    /// contextual tabs.
    pub in_table: bool,
    /// Whether a header or a footer is being edited, which is what puts the
    /// Header & Footer tab on the ribbon.
    pub in_furniture: bool,
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
        let twips = match command {
            Command::IndentLeftBox => self.indent_left,
            Command::IndentRightBox => self.indent_right,
            _ => return String::new(),
        };
        format!("{:.2} cm", f64::from(twips) / 1440.0 * 2.54)
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
        Command::ShowMarkup => state.show_markup,
        Command::ShowProofing => state.show_proofing,
        Command::ReviewingPane | Command::ShowComments => state.show_comments,
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
