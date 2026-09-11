//! What every ribbon button and every shortcut actually does.
//!
//! One place, so a button and its shortcut cannot drift apart, and so that the
//! answer to "what does this program do" is a single list.

use wp_docx::model::Alignment;
use wp_docx::model::TableBorders;
use wp_docx::page::CaseChange;
use wp_docx::revisions::Decision;
use wp_docx::CharacterFormat;
use wp_shell::Response;

use crate::chrome::palette::Kind as PaletteKind;
use crate::chrome::{Choice, Command, TableBorderChoice};

use super::paste;
use super::{Editor, INDENT_STEP};

impl Editor {
    /// Carries out what a ribbon button or a shortcut asks for.
    #[allow(clippy::too_many_lines)]
    pub(super) fn run(&mut self, command: Command) -> Response {
        // A protected document is protected against the ribbon too, not only
        // against typing.
        self.record_command(command);
        if self.is_locked() && !command.is_allowed_when_locked() {
            return self.refuse_locked();
        }

        match command {
            // --- File ---------------------------------------------------------
            Command::New => self.new_document(),
            Command::Open => self.open_document(),
            Command::Save => {
                self.save_now();
                self.after_file_command()
            }
            Command::SaveAs => {
                self.save_as_now();
                self.after_file_command()
            }
            Command::Print => self.open_print(),
            Command::CloseDocument => {
                if self.may_discard() {
                    self.new_document()
                } else {
                    Response::Ignored
                }
            }
            Command::About => self.report(concat!(
                "A word processor written from nothing: no toolkit, no libraries, ",
                "no crates but the standard one."
            )),

            // --- Clipboard and history ---------------------------------------
            Command::Undo => self.undo(),
            Command::Redo => self.redo(),
            Command::Cut => self.cut(),
            Command::Copy => self.copy(),
            Command::Paste => self.paste(),
            Command::PasteKeepSource => self.paste_as(paste::PasteAs::KeepSource),
            Command::PasteMerge => self.paste_as(paste::PasteAs::Merge),
            Command::PasteAsPicture => self.paste_as(paste::PasteAs::Picture),
            Command::PasteTextOnly => self.paste_as(paste::PasteAs::TextOnly),
            Command::FormatPainter => self.toggle_format_painter(),

            // --- Font ---------------------------------------------------------
            Command::Format(format) => self.toggle(format, format_name(format)),
            Command::Subscript => {
                self.toggle(CharacterFormat::Subscript, format_name(CharacterFormat::Subscript))
            }
            Command::Superscript => {
                self.toggle(CharacterFormat::Superscript, format_name(CharacterFormat::Superscript))
            }
            Command::GrowFont => self.step_size(true),
            Command::ShrinkFont => self.step_size(false),
            // A pure dropdown on the ribbon, so the command means "ask which
            // case", both from a keystroke and from a recorded macro.
            Command::ChangeCase => self.open_list(Choice::LetterCase),
            Command::ClearFormatting => {
                let changed = self.document.clear_formatting();
                self.finish_character_change(changed, "Formatting cleared")
            }
            Command::TextColor => self.open_palette(PaletteKind::Text),
            Command::Highlight => self.open_palette(PaletteKind::Highlight),
            Command::FontDialog => self.open_font_dialog(),
            Command::ParagraphDialog => self.open_paragraph_dialog(),
            Command::StylesPane => self.toggle_styles_pane(),
            Command::Options => self.open_options(),
            Command::ChooseFont => self.open_list(Choice::Font),
            Command::ChooseSize => self.open_list(Choice::Size),
            Command::ChooseStyle => self.open_list(Choice::Style),
            Command::ChooseZoom => self.open_list(Choice::Zoom),

            // --- Paragraph ----------------------------------------------------
            Command::Align(alignment) => self.apply_alignment(alignment, alignment_name(alignment)),
            Command::Bullets => self.toggle_list(wp_docx::BULLET_LIST, "Bulleted list"),
            Command::Numbering => self.toggle_list(wp_docx::NUMBERED_LIST, "Numbered list"),
            Command::MultilevelList => self.open_list(Choice::MultilevelLibrary),
            Command::IndentMore => {
                let changed = self.document.adjust_indent_here(INDENT_STEP);
                self.edited(changed, "Indented")
            }
            Command::IndentLess => {
                let changed = self.document.adjust_indent_here(-INDENT_STEP);
                self.edited(changed, "Outdented")
            }
            Command::Sort => self.sort_selection(),
            Command::LineSpacing => self.open_list(Choice::LineSpacing),
            Command::DocumentSpacing => self.open_list(Choice::DocumentSpacing),
            Command::AlignmentTab => self.open_list(Choice::AlignmentTab),
            Command::ShowMarks => {
                self.show_marks = !self.show_marks;
                self.needs_redraw = true;
                self.report(if self.show_marks {
                    "Formatting marks shown"
                } else {
                    "Formatting marks hidden"
                })
            }
            Command::Shading => self.open_palette(PaletteKind::Shading),
            Command::Borders => self.open_borders(),
            Command::PageBorders => self.open_page_borders(),
            Command::Style(index) => {
                let style = self.style_gallery().get(index).and_then(|sample| sample.id.clone());
                self.apply_style(style.as_deref())
            }

            // --- Editing ------------------------------------------------------
            Command::Find => self.open_find(false),
            Command::Replace => self.open_find(true),
            Command::SelectAll => self.select_all(),
            Command::SelectSimilar => self.select_similar(),

            // --- Insert -------------------------------------------------------
            Command::PageBreak => {
                let changed = self.document.insert_page_break();
                self.edited(changed, "Page break")
            }
            Command::BlankPage => {
                // A blank page is two breaks: one to end this page and one to
                // end the blank one, leaving an empty page between them.
                let changed =
                    self.document.insert_page_break() && self.document.insert_page_break();
                self.edited(changed, "Blank page")
            }
            Command::InsertTable => self.open_table_grid(),
            Command::InsertPicture => self.insert_picture(),
            Command::InsertSymbol => self.open_symbol_dialog(),
            Command::Header => self.open_furniture(wp_docx::furniture::Furniture::Header),
            Command::GoToHeader => self.go_to_furniture(wp_docx::furniture::Furniture::Header),
            Command::GoToFooter => self.go_to_furniture(wp_docx::furniture::Furniture::Footer),
            Command::LinkToPrevious => self.toggle_link_to_previous(),
            Command::DifferentFirstPage => self.toggle_different_first_page(),
            Command::DifferentOddEven => self.toggle_different_odd_even(),
            Command::CloseFurniture => self.leave_furniture(),
            Command::FormatPageNumbers => self.open_page_numbering(),
            Command::Footer => self.open_furniture(wp_docx::furniture::Furniture::Footer),
            Command::PageNumber => self.open_furniture(wp_docx::furniture::Furniture::Footer),
            Command::InsertDate => {
                let today = super::files::today();
                let changed = self.document.type_text(&today);
                self.edited(changed, &today)
            }

            // --- Layout -------------------------------------------------------
            Command::Margins => self.open_margins(),
            Command::Orientation => self.open_orientation(),
            Command::PageSize => self.open_page_size(),
            Command::Columns => self.open_columns(),
            Command::Breaks => self.open_breaks(),
            // The four measurement boxes take the keyboard rather than doing
            // anything on their own. A command reaching here is one replayed
            // from a macro or a keystroke, and it means "put the caret in it".
            Command::IndentLeftBox
            | Command::IndentRightBox
            | Command::SpaceBeforeBox
            | Command::SpaceAfterBox
            | Command::RowHeightBox
            | Command::ColumnWidthBox
            | Command::HeaderFromTopBox
            | Command::FooterFromBottomBox => self.type_in_box(command),

            // --- Tables -------------------------------------------------------
            Command::InsertRowAbove => {
                let changed = self.document.insert_table_row(false);
                self.edited(changed, "Row added above")
            }
            Command::InsertRowBelow => {
                let changed = self.document.insert_table_row(true);
                self.edited(changed, "Row added below")
            }
            Command::InsertColumnLeft => {
                let changed = self.document.insert_table_column(false);
                self.edited(changed, "Column added to the left")
            }
            Command::InsertColumnRight => {
                let changed = self.document.insert_table_column(true);
                self.edited(changed, "Column added to the right")
            }
            Command::DeleteRow => {
                let changed = self.document.delete_table_row();
                self.edited(changed, "Row deleted")
            }
            Command::DeleteColumn => {
                let changed = self.document.delete_table_column();
                self.edited(changed, "Column deleted")
            }
            Command::DeleteTable => {
                let changed = self.document.delete_table();
                self.edited(changed, "Table deleted")
            }
            Command::TableBorders(choice) => {
                let borders = match choice {
                    TableBorderChoice::All => TableBorders::grid(),
                    TableBorderChoice::Outside => TableBorders {
                        inside_horizontal: None,
                        inside_vertical: None,
                        ..TableBorders::grid()
                    },
                    TableBorderChoice::None => TableBorders::default(),
                };
                let changed = self.document.set_table_borders(&borders);
                self.edited(changed, "Table borders")
            }
            // --- Review -------------------------------------------------------
            Command::NewComment => self.start_comment(),
            Command::DeleteComment => self.delete_comment_here(),
            Command::PreviousComment => self.step_comment(false),
            Command::NextComment => self.step_comment(true),
            Command::ShowComments | Command::ReviewingPane => self.show_comment_pane(),
            Command::TrackChanges => self.toggle_track_changes(),
            Command::ShowMarkup => self.open_markup_menu(),
            Command::AcceptChange => self.resolve_here(Decision::Accept),
            Command::RejectChange => self.resolve_here(Decision::Reject),
            Command::AcceptAll => self.resolve_all(Decision::Accept),
            Command::RejectAll => self.resolve_all(Decision::Reject),

            Command::MergeCells => {
                let changed = self.document.merge_cells();
                self.edited(changed, "Cells merged")
            }
            Command::SplitCells => {
                let changed = self.document.split_cell();
                self.edited(changed, "Cell split")
            }
            Command::DistributeColumns => {
                let changed = self.document.distribute_columns();
                self.edited(changed, "Columns distributed")
            }

            // --- References ---------------------------------------------------
            Command::InsertContents | Command::UpdateContents => self.write_contents(),
            Command::InsertFootnote => self.start_note(wp_docx::notes::Kind::Footnote),
            Command::InsertEndnote => self.start_note(wp_docx::notes::Kind::Endnote),
            Command::NextNote => self.step_note(),
            Command::InsertCaption => self.start_caption(),
            Command::TableOfFigures => self.write_figures(),
            Command::MarkIndexEntry => self.start_index_entry(),
            Command::InsertIndex => self.write_index(),
            Command::InsertCitation => self.open_citations(),
            Command::AddSource => self.start_source(),
            Command::ManageSources => self.open_sources(),
            Command::InsertBibliography => self.write_bibliography(),
            Command::InsertLink => self.start_link(),
            Command::RemoveLink => self.drop_link(),
            Command::PageColor => self.open_palette(PaletteKind::Page),
            Command::Watermark => self.open_watermarks(),
            Command::DocumentProperties => self.open_properties(),
            Command::CoverPage => self.open_cover_pages(),
            Command::MarkCitation => self.start_authority(),
            Command::TableOfAuthorities => self.open_authorities(),
            Command::Themes => self.open_themes(),
            Command::ThemeColors => self.open_theme_colors(),
            Command::ThemeFonts => self.open_theme_fonts(),
            Command::Language => self.open_languages(),
            Command::InsertShape => self.open_shapes(),
            Command::InsertTextBox => self.start_text_box(),
            Command::WrapText => self.open_wrapping(),
            Command::Position => self.open_position(),
            Command::QuickParts => self.open_quick_parts(),
            Command::WordArt => self.open_word_art(),
            Command::SelectionPane => self.open_selection_pane(),

            // --- Mailings -----------------------------------------------------
            Command::StartMailMerge => self.open_merge_kind(),
            Command::SelectRecipients => self.select_recipients(),
            Command::EditRecipientList => self.open_recipient_list(),
            Command::InsertMergeField => self.open_merge_fields(),
            Command::AddressBlock => self.insert_address_block(),
            Command::GreetingLine => self.insert_greeting_line(),
            Command::PreviewResults => self.toggle_preview(),
            Command::NextRecipient => self.step_recipient(true),
            Command::PreviousRecipient => self.step_recipient(false),
            Command::CheckMergeErrors => self.check_merge(),
            Command::FinishMerge => self.finish_merge(),

            // --- Proofing -----------------------------------------------------
            Command::Compare => self.compare_documents(),
            Command::Spelling => self.next_issue(),
            Command::ShowProofing => self.toggle_proofing(),
            Command::LoadDictionary => self.load_dictionary(),
            Command::CheckAccessibility => self.open_accessibility(),
            Command::HighlightMergeFields => self.toggle_field_highlight(),
            Command::TextEffects => self.open_text_effects(),
            Command::SetAsDefault => self.set_theme_as_default(),
            Command::MatchFields => self.open_match_fields(),
            Command::Split => self.toggle_split(),
            Command::SideToSide => self.toggle_movement(),
            Command::OutlineView => self.open_outline(),
            Command::Screenshot => self.open_screenshot(),
            Command::BlockAuthors => self.toggle_block_authors(),
            Command::Feedback => self.send_feedback(),
            Command::ShowTraining => self.show_training(),
            Command::WhatsNew => self.show_whats_new(),
            Command::SignatureLine => self.start_signature_line(),
            Command::TextFromFile => self.insert_text_from_file(),
            Command::SmartArt => self.open_diagram(),
            Command::Macros => self.open_macros(),
            Command::OnlineVideo => self.start_video(),
            Command::Equation => self.start_equation(),
            Command::Rules => self.open_rules(),
            Command::Chart => self.open_chart(),
            Command::Effects => self.open_theme_effects(),
            Command::NewWindow => self.new_window(),
            Command::ArrangeAll => self.arrange_all(),
            Command::Translate => self.translate(),
            Command::ExpandGroup(index) => self.open_group(index as usize),
            Command::Envelopes => self.open_envelopes(),
            Command::Labels => self.open_labels(),
            Command::TableProperties => self.open_table_properties(),
            Command::BringForward => self.move_shape_depth(true, false),
            Command::BringToFront => self.move_shape_depth(true, true),
            Command::BringInFrontOfText => self.set_shape_depth(false),
            Command::SendBackward => self.move_shape_depth(false, false),
            Command::SendToBack => self.move_shape_depth(false, true),
            Command::SendBehindText => self.set_shape_depth(true),
            Command::LineNumbers => self.open_line_numbers(),
            Command::Hyphenation => self.open_hyphenation(),
            Command::RestrictEditing => self.open_protection(),
            Command::AddBookmark => self.start_bookmark(),
            Command::CrossReference => self.open_references(wp_docx::captions::Reference::Text),
            Command::PageReference => self.open_references(wp_docx::captions::Reference::Page),
            Command::DeleteNote => self.delete_note_here(),
            Command::RemoveContents => {
                let changed = self.document.remove_contents();
                self.edited(changed, "Table of contents removed")
            }

            Command::TableStyles => self.open_table_styles(),
            Command::AlignCell(which) => self.align_cell(which as usize),
            Command::SelectTablePart => self.open_table_select(),
            Command::RepeatHeaderRow => self.toggle_repeat_header(),
            Command::ViewGridlines => self.toggle_table_gridlines(),
            Command::ConvertToText => self.convert_table_to_text(),
            Command::TableHeaderRow
            | Command::TableTotalRow
            | Command::TableFirstColumn
            | Command::TableLastColumn
            | Command::TableBandedRows
            | Command::TableBandedColumns => self.toggle_table_look(command),

            // --- Review -------------------------------------------------------
            Command::WordCount => self.open_word_count(),

            // --- View ---------------------------------------------------------
            Command::ToggleRulers => {
                self.show_rulers = !self.show_rulers;
                self.remembered_rulers = self.show_rulers;
                self.remember_window();
                self.clamp_scroll();
                self.needs_redraw = true;
                Response::Redraw
            }
            Command::ToggleNavigation => {
                self.show_navigation = !self.show_navigation;
                self.remembered_navigation = self.show_navigation;
                self.remember_window();
                self.relayout();
                Response::Redraw
            }
            Command::ToggleTheme => self.toggle_theme(),
            Command::Gridlines => {
                self.show_gridlines = !self.show_gridlines;
                self.needs_redraw = true;
                self.report(if self.show_gridlines {
                    "Gridlines shown"
                } else {
                    "Gridlines hidden"
                })
            }
            Command::JoinPages => self.toggle_joined_pages(),
            Command::PrintLayout => self.set_view(crate::editor::views::View::Print),
            Command::WebLayout => self.set_view(crate::editor::views::View::Web),
            Command::DraftView => self.set_view(crate::editor::views::View::Draft),
            Command::ReadMode => self.set_view(crate::editor::views::View::Reading),
            Command::ZoomIn => self.step_zoom(true),
            Command::ZoomOut => self.step_zoom(false),
            Command::ZoomHundred => self.set_zoom(100.0),
            Command::OnePage => self.zoom_to_fit(false),
            Command::PageWidth => self.zoom_to_fit(true),
        }
    }

    /// Zooms so that a page fits the window, either its width or the whole of it.
    fn zoom_to_fit(&mut self, width_only: bool) -> Response {
        let metrics = wp_layout::PageMetrics::from_document(&self.document);
        let room_across = self.view_width as f32 - self.content_left() - super::PAGE_GAP * 2.0;
        let room_down = self.content_bottom() - self.content_top() - super::PAGE_GAP;
        if room_across <= 0.0 || metrics.width <= 0.0 {
            return Response::Ignored;
        }

        // The page is measured in points, and a hundred per cent is the DPI the
        // document is laid out at, so the ratio of the two is the zoom.
        let across = room_across / (metrics.width / super::POINTS_PER_INCH * super::DPI) * 100.0;
        let down = room_down / (metrics.height / super::POINTS_PER_INCH * super::DPI) * 100.0;
        self.set_zoom(if width_only { across } else { across.min(down) })
    }
}

/// What the status strip calls a format.
pub(super) fn format_name(format: CharacterFormat) -> &'static str {
    match format {
        CharacterFormat::Bold => "Bold",
        CharacterFormat::Italic => "Italic",
        CharacterFormat::Underline => "Underline",
        CharacterFormat::Strikethrough => "Strikethrough",
        CharacterFormat::Superscript => "Superscript",
        CharacterFormat::Subscript => "Subscript",
    }
}

/// The same, for an alignment.
pub(super) fn alignment_name(alignment: Alignment) -> &'static str {
    match alignment {
        Alignment::Start => "Left",
        Alignment::Center => "Centred",
        Alignment::End => "Right",
        Alignment::Both => "Justified",
    }
}

/// Kept so the module can name the case cycle it steps through.
const _: CaseChange = CaseChange::Sentence;
