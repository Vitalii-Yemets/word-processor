//! The dialogs the program opens, and what it does with their answers.
//!
//! The machinery is [`crate::chrome::dialog`]; this is where the questions are
//! asked. A dialog is opened with the fields it needs, and when it is answered
//! the fields are read back and applied — which is why what each field means is
//! written down beside where it is read, and nowhere else.

use wp_shell::{Key, Response};

use crate::chrome::dialog::{Answer, Dialog, Field, Reaction};

use super::Editor;

/// Where the tick box sits in the Word Count dialog.
///
/// Named rather than written as a number twice: the row it is on is decided by
/// the list the dialog is built from, a few lines further down.
const EDGES: usize = 6;

/// Where each answer sits in the Page Setup dialog.
///
/// Named rather than written as a number twice, for the same reason the Font
/// dialog names its rows: the markers that arrange the fields take places in
/// the list too, so a row moved without moving these applies the wrong margin
/// to the wrong edge and never looks wrong doing it.
pub(super) const MARGIN_TOP: usize = 2;
const MARGIN_BOTTOM: usize = 3;
const MARGIN_LEFT: usize = 5;
const MARGIN_RIGHT: usize = 6;
const PAPER_SIZE: usize = 9;
const ORIENTATION: usize = 10;

/// Which question is being asked, so the answer can be acted on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Asking {
    /// A list of people being typed from nothing.
    NewList,
    /// What a content control is called, and what may be done to it.
    ControlProperties,
    /// The defaults a document starts from, and the styles carried between
    /// it and the template.
    ManageStyles,
    /// What two documents are to be compared by, and where the answer goes.
    Compare,
    /// Word's four hyphenation settings.
    HyphenationOptions,
    /// One word the automatic rule would break, offered to be broken by
    /// hand.
    ManualHyphenation,
    /// Word's two Asian Layout dialogs: a run set across a vertical line,
    /// and a run set as two lines in one.
    HorizontalInVertical,
    TwoLinesInOne,
    /// What the document is signed with, and what to sign it with.
    Signatures,
    /// Whether a document that asks to be opened read-only is to be, and the
    /// password that opens it for writing.
    OpenReadOnly,
    /// What a document should ask for the next time it is opened.
    ReadOnlySettings,
    /// How much there is of the document. Word's Word Count: a dialog that
    /// tells rather than asks.
    WordCount,
    /// What one of the document's own macros says. Another that tells: this
    /// program reads Visual Basic and does not run it.
    Macro,
    /// A `MsgBox` a macro put up, and an `InputBox`: answered to the macro.
    MacroMessage,
    MacroInput,
    /// The margins, the paper and the way round it goes.
    PageSetup,
    /// A name for this place in the document.
    Bookmark,
    /// Every character format there is: Word's Font dialog.
    Font,
    /// Where the lines of a paragraph sit, and where it may break.
    Paragraph,
    /// Where the tabs of a paragraph stop.
    TabStops,
    /// A style being made or changed.
    Style,
    /// What the selection is formatted with, and where it came from.
    Inspector,
    /// Which character to put in.
    Symbol,
    /// The table, the row, the cell and what a reader who cannot see it is
    /// told.
    Table,
    /// What the program does, rather than what the document says.
    Options,
    /// The border round the pages of a section.
    PageBorders,
    /// Which corrections are made as text is typed.
    AutoCorrect,
    /// Formatting the whole document at once: now, or with a review.
    AutoFormat,
    /// What to do with the changes AutoFormat marked for review.
    AutoFormatReview,
    /// Which language to translate into.
    Translator,
    /// Which kind of file to read a file as: Word's Convert File.
    Convert,
    /// The document's template, and whether its styles are the template's.
    Templates,
    /// Which encoding a text file is in, when its bytes do not say.
    TextOpen,
    /// Which encoding to write a text file in, and how to end its lines.
    TextSave,
    /// The words those corrections must leave alone, which is a dialog of its
    /// own behind the one above.
    Exceptions,
    /// Where a drawing sits, how the text keeps out of its way, and how big
    /// it is: Word's Layout, in three tabs.
    Layout,
    /// Which column to put the rows of a table in order by, and how.
    Sort,
    /// Arithmetic over the cells of a table.
    Formula,
    /// What a reader of this document may do to it, and the password behind
    /// the answer.
    FormattingLimits,
    Enforce,
    MoreUsers,
    /// And the password back again, before the restriction is lifted.
    Unprotect,
    /// Who a mail merge is for.
    Recipients,
    /// A name for the piece of the document being saved.
    NewBlock,
    /// Everything saved, and what to do with one of them.
    Organizer,
    /// The password of a file that cannot be read without one.
    Unseal,
    /// A password to make the document unreadable without, which is Word's
    /// Encrypt with Password.
    Encrypt,
}

impl Editor {
    /// Whether a dialog is up. While one is, it has the window.
    #[must_use]
    pub(super) fn in_dialog(&self) -> bool {
        self.dialog.is_some()
    }

    /// Opens a dialog, and remembers what it is asking.
    pub(super) fn ask(&mut self, asking: Asking, dialog: Dialog) -> Response {
        self.dialog = Some(dialog);
        self.asking = Some(asking);
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Shuts the dialog, acting on the answer if it was accepted.
    ///
    /// Not every button shuts it. Word's Tabs dialog has three that change the
    /// list of stops and leave the dialog standing, and its Paragraph dialog
    /// has one that opens the Tabs dialog instead of answering. Those are dealt
    /// with first, and only what is left is an answer.
    pub(super) fn finish_dialog(&mut self, answer: Answer) -> Response {
        if let Answer::Named(button) = answer {
            if let Some(response) = self.pressed_without_answering(button) {
                return response;
            }
        }
        // The Font and Paragraph dialogs opened from inside the style dialog
        // go back there rather than to the page, taking what they put on the
        // paragraph into the style with them.
        if self.formatting_a_style && matches!(self.asking, Some(Asking::Font | Asking::Paragraph))
        {
            let applied = self.dialog.take().map(|dialog| match self.asking {
                Some(Asking::Font) => self.apply_font_dialog(&dialog, false),
                _ => self.apply_paragraph_dialog(&dialog, false),
            });
            let _ = applied;
            self.asking = None;
            if answer == Answer::Cancel {
                self.formatting_a_style = false;
            }
            return self.back_to_style_dialog();
        }
        let Some(dialog) = self.dialog.take() else { return Response::Ignored };
        let asking = self.asking.take();
        self.needs_redraw = true;

        // The Exceptions dialog goes back to the one it was opened from
        // whichever button shut it. Cancelling the exceptions is not
        // cancelling the dialog behind them, and Word does the same.
        if asking == Some(Asking::Exceptions) {
            let _ = dialog;
            return self.close_exceptions(answer != Answer::Cancel);
        }

        if answer == Answer::Cancel {
            match asking {
                Some(Asking::TextOpen) => self.cancel_text_open(),
                Some(Asking::Convert) => self.cancel_conversion(),
                Some(Asking::Unseal) => self.cancel_unseal(),
                Some(Asking::Compare) => self.cancel_comparison(),
                Some(Asking::ManualHyphenation) => self.cancel_manual_hyphenation(),
                Some(Asking::OpenReadOnly) => self.cancel_open_read_only(),
                Some(Asking::TextSave) => self.cancel_text_save(),
                Some(Asking::MacroMessage) => return self.answer_macro_message(None),
                Some(Asking::MacroInput) => return self.answer_macro_input(None),
                _ => {}
            }
            return Response::Redraw;
        }
        match asking {
            Some(Asking::MacroMessage) => {
                let button = match answer {
                    Answer::Named(label) => Some(label),
                    _ => None,
                };
                self.answer_macro_message(button)
            }
            Some(Asking::MacroInput) => self.answer_macro_input(Some(dialog.said(1))),
            // Nothing to take from a dialog that was telling rather than
            // asking.
            Some(Asking::PageSetup) => self.apply_page_setup(&dialog),
            Some(Asking::Bookmark) => self.apply_bookmark(&dialog),
            Some(Asking::Font) => self.apply_font_dialog(
                &dialog,
                answer == Answer::Named(super::fontdialog::SET_AS_DEFAULT),
            ),
            Some(Asking::Paragraph) => self.apply_paragraph_dialog(
                &dialog,
                answer == Answer::Named(super::paragraphdialog::SET_AS_DEFAULT),
            ),
            Some(Asking::TabStops) => self.apply_tabs_dialog(&dialog),
            Some(Asking::Style) => self.apply_style_dialog(&dialog),
            Some(Asking::Table) => self.apply_table_dialog(&dialog),
            Some(Asking::Layout) => self.apply_layout_dialog(&dialog),
            Some(Asking::Sort) => self.apply_sort(&dialog),
            Some(Asking::Formula) => self.apply_formula(&dialog),
            Some(Asking::FormattingLimits) => self.apply_formatting_limits(&dialog),
            Some(Asking::Enforce) => self.apply_enforcement(&dialog),
            Some(Asking::MoreUsers) => self.apply_more_users(&dialog),
            Some(Asking::ControlProperties) => self.apply_control_properties(&dialog),
            Some(Asking::ManageStyles) => self.apply_manage_styles(&dialog),
            Some(Asking::Compare) => self.apply_comparison(&dialog),
            Some(Asking::HyphenationOptions) => self.apply_hyphenation_options(
                &dialog,
                answer == Answer::Named(super::hyphenation::MANUAL),
            ),
            Some(Asking::ManualHyphenation) => {
                self.answer_manual_hyphenation(&dialog, answer == Answer::Accept)
            }
            Some(Asking::HorizontalInVertical) => self.apply_horizontal_in_vertical(
                &dialog,
                answer == Answer::Named(super::vertical::REMOVE),
            ),
            Some(Asking::TwoLinesInOne) => self
                .apply_two_lines_in_one(&dialog, answer == Answer::Named(super::vertical::REMOVE)),
            Some(Asking::Signatures) => self.apply_signature(&dialog),
            Some(Asking::OpenReadOnly) => self.apply_open_read_only(&dialog),
            Some(Asking::ReadOnlySettings) => self.apply_read_only_settings(&dialog),
            Some(Asking::Unprotect) => self.apply_unprotection(&dialog),
            Some(Asking::Recipients) => self.apply_recipient_list(&dialog),
            Some(Asking::NewList) => self.finish_new_list(&dialog),
            Some(Asking::NewBlock) => self.apply_new_block(&dialog),
            Some(Asking::Organizer) => {
                self.apply_organizer(&dialog, answer == Answer::Named(super::ownblocks::DELETE))
            }
            Some(Asking::Unseal) => self.apply_unseal(&dialog),
            Some(Asking::Encrypt) => self.apply_encryption(&dialog),
            Some(Asking::Options) => self.apply_options(&dialog),
            Some(Asking::AutoCorrect) => self.apply_autocorrect_dialog(&dialog),
            Some(Asking::AutoFormat) => self.apply_autoformat_dialog(&dialog),
            Some(Asking::AutoFormatReview) => {
                let _ = dialog;
                self.answer_autoformat_review(answer)
            }
            Some(Asking::Translator) => self.apply_translator_preferences(&dialog),
            Some(Asking::TextOpen) => self.apply_text_open(&dialog),
            Some(Asking::Convert) => self.apply_conversion(&dialog),
            Some(Asking::Templates) => self.apply_templates(&dialog),
            Some(Asking::TextSave) => self.apply_text_save(&dialog),
            Some(Asking::PageBorders) => self.apply_page_borders(&dialog),
            // Word's Symbol dialog is answered by its Insert button rather
            // than by OK, so there is nothing left to do when it shuts.
            Some(Asking::Symbol) | Some(Asking::Inspector) => {
                // Dialogs that tell, or that have already done their work.
                let _ = &dialog;
                Response::Redraw
            }
            // Answered on the way out, above.
            Some(Asking::Exceptions) => Response::Redraw,
            Some(Asking::WordCount) | Some(Asking::Macro) | None => {
                let _ = dialog;
                Response::Redraw
            }
        }
    }

    /// The buttons that do something other than answer the dialog.
    ///
    /// `None` when the button is not one of them, and the caller goes on to
    /// treat it as an answer.
    fn pressed_without_answering(&mut self, button: &str) -> Option<Response> {
        use super::autocorrectdialog::{ADD, DELETE, EXCEPTIONS};
        use super::tabsdialog::{CLEAR, CLEAR_ALL, SET};

        match (self.asking, button) {
            // Attach… chooses a template and leaves the dialog standing.
            (Some(Asking::Templates), super::templates::ATTACH) => {
                Some(self.attach_template_pressed())
            }
            // Add and Delete change a list and leave the dialog standing;
            // Exceptions hands over to a dialog of its own and comes back.
            (Some(Asking::AutoCorrect), ADD | DELETE | EXCEPTIONS) => {
                Some(self.autocorrect_dialog_button(button))
            }
            (Some(Asking::Exceptions), ADD | DELETE) => Some(self.exceptions_dialog_button(button)),
            // AutoFormat's Options… hands over to the tab its switches are on.
            (Some(Asking::AutoFormat), super::autoformat::OPTIONS) => {
                Some(self.autoformat_options())
            }
            // Word's four buttons under the list of recipients, each of
            // which changes the list and leaves the dialog standing.
            (Some(Asking::NewList), super::mailings::ADD_RECIPIENT) => {
                let dialog = self.dialog.clone()?;
                Some(self.add_typed_recipient(&dialog))
            }
            // Word's three buttons above the list of styles, each of which
            // ticks what it names and leaves the dialog standing.
            // The one button that runs a macro, which is the only way into
            // the interpreter this program has.
            (Some(Asking::Macro), button) if button == super::macros::RUN => {
                let wanted = self.showing_macro.clone()?;
                Some(self.run_macro(&wanted))
            }
            (Some(Asking::FormattingLimits), button)
                if matches!(
                    button,
                    super::protection::ALL_OF_THEM
                        | super::protection::RECOMMENDED
                        | super::protection::NONE_OF_THEM
                ) =>
            {
                let dialog = self.dialog.clone()?;
                Some(self.formatting_button(&dialog, button))
            }
            (Some(Asking::Organizer), super::ownblocks::MODIFY) => {
                let dialog = self.dialog.clone()?;
                Some(self.organizer_button(&dialog, super::ownblocks::MODIFY))
            }
            (Some(Asking::Recipients), button)
                if matches!(
                    button,
                    super::mailings::SORT
                        | super::mailings::FILTER
                        | super::mailings::DUPLICATES
                        | super::mailings::VALIDATE
                ) =>
            {
                let dialog = self.dialog.clone()?;
                Some(self.recipient_button(&dialog, button))
            }
            // The Organizer's three: each changes one of the two lists and
            // puts the dialog up again showing what is there now.
            (Some(Asking::ManageStyles), button)
                if matches!(
                    button,
                    super::managestyles::COPY
                        | super::managestyles::DELETE
                        | super::managestyles::RENAME
                        | super::managestyles::MODIFY
                ) =>
            {
                let dialog = self.dialog.clone()?;
                Some(self.manage_styles_button(&dialog, button))
            }
            // The two customising pages change a list and leave the dialog
            // standing, as the Tabs dialog's three do.
            (Some(Asking::Options), button)
                if matches!(
                    button,
                    super::ribbondialog::ADD
                        | super::ribbondialog::REMOVE
                        | super::ribbondialog::MOVE_UP
                        | super::ribbondialog::MOVE_DOWN
                        | super::ribbondialog::RESET
                ) =>
            {
                Some(self.customise_button(button))
            }
            // The file types are registered with the desktop there and then,
            // and the line above the button is rewritten to say what came of
            // it — a button that reported nothing would leave the person
            // pressing it twice.
            // The Trust Centre's two, which change a list and leave the
            // dialog standing.
            (Some(Asking::Options), super::optionsdialog::TRUST_FOLDER) => {
                let dialog = self.dialog.clone()?;
                Some(self.trust_this_folder(&dialog))
            }
            (Some(Asking::Options), super::optionsdialog::FORGET_PLACE) => {
                let dialog = self.dialog.clone()?;
                Some(self.forget_trusted_place(&dialog))
            }
            (Some(Asking::Options), super::optionsdialog::MAKE_DEFAULT) => {
                Some(self.make_default_program())
            }
            // Word's Proofing page hands over to the AutoCorrect dialog. What
            // Options said is applied on the way, so that nothing typed into it
            // is lost by going to look at the corrections.
            (Some(Asking::Options), super::optionsdialog::AUTOCORRECT_OPTIONS) => {
                let dialog = self.dialog.take()?;
                self.apply_options(&dialog);
                self.asking = None;
                Some(self.open_autocorrect())
            }
            // Word's Tabs button hands over to the Tabs dialog. What the
            // Paragraph dialog said is applied first, so that a stop set from
            // inside it lands on the paragraph the dialog was describing.
            (Some(Asking::Paragraph), super::paragraphdialog::TABS) => {
                let dialog = self.dialog.take()?;
                self.apply_paragraph_dialog(&dialog, false);
                Some(self.open_tabs_dialog())
            }
            // Word's Insert puts the character in and leaves the dialog up: a
            // person putting in three symbols should not open it three times.
            (Some(Asking::Symbol), super::symboldialog::INSERT) => {
                let dialog = self.dialog.clone()?;
                Some(self.insert_symbol_from_dialog(&dialog))
            }
            // And its AutoCorrect button hands the chosen character over to the
            // AutoCorrect dialog, already in the box that says what to put in.
            (Some(Asking::Symbol), super::symboldialog::AUTOCORRECT) => {
                let dialog = self.dialog.take()?;
                let character = self.symbol_dialog_says(&dialog)?;
                self.asking = None;
                Some(self.open_autocorrect_with(&character.to_string()))
            }
            // Set, Clear and Clear All change the list and leave the dialog up.
            (Some(Asking::TabStops), SET | CLEAR | CLEAR_ALL) => {
                let dialog = self.dialog.clone()?;
                Some(self.tabs_dialog_button(&dialog, button))
            }
            // Word's Borders button hands over to the borders menu, and what
            // the table dialog said is applied on the way so that a border
            // lands on the table the dialog was describing.
            (Some(Asking::Table), super::tabledialog::BORDERS) => {
                let dialog = self.dialog.take()?;
                self.apply_table_dialog(&dialog);
                self.asking = None;
                Some(self.run(crate::chrome::Command::Borders))
            }
            // The style dialog's Format menu hands over to the two dialogs that
            // hold every format there is, and comes back afterwards.
            (
                Some(Asking::Style),
                super::styledialog::FORMAT_FONT | super::styledialog::FORMAT_PARAGRAPH,
            ) => {
                let dialog = self.dialog.clone()?;
                let said = self.style_dialog_says(&dialog);
                self.editing_style = said;
                Some(self.style_dialog_format(button))
            }
            _ => None,
        }
    }

    /// A press while a dialog is up.
    pub(super) fn dialog_press(&mut self, x: i32, y: i32) -> Response {
        let Some(dialog) = &mut self.dialog else { return Response::Ignored };
        let reaction = dialog.press(x, y);
        self.reacted(reaction)
    }

    /// What to do about whatever the dialog made of the event.
    ///
    /// A dialog that shows what it is asking about — Word Count is the one —
    /// is built again when a field changes, because the numbers it is showing
    /// are the answer to that field.
    fn reacted(&mut self, reaction: Reaction) -> Response {
        match reaction {
            Reaction::Closed(answer) => self.finish_dialog(answer),
            Reaction::Changed => {
                match self.asking {
                    Some(Asking::WordCount) => {
                        self.count_the_edges =
                            self.dialog.as_ref().is_some_and(|dialog| dialog.ticked(EDGES));
                        self.dialog = Some(self.word_count_dialog(self.count_the_edges));
                    }
                    // The Font dialog's preview shows what its fields say, so
                    // the dialog is built again from what they now say — with
                    // the tab that was showing kept, or clicking a tab would
                    // put it straight back.
                    Some(Asking::Font) => {
                        if let Some(dialog) = self.dialog.take() {
                            let said = self.font_dialog_says(&dialog);
                            let mut built = self.font_dialog(&said);
                            built.carry_typing_from(&dialog);
                            self.dialog = Some(built);
                        }
                    }
                    // The subset changing is the grid changing.
                    Some(Asking::Symbol) => self.symbol_dialog_changed(),
                    // The encoding changing is the preview changing.
                    Some(Asking::TextOpen) => self.text_open_dialog_changed(),
                    Some(Asking::TextSave) => self.text_save_dialog_changed(),
                    // Its list of functions types into its formula rather than
                    // deciding anything of its own.
                    Some(Asking::Formula) => self.formula_dialog_changed(),
                    // Its Style list and its Art gallery both say what the
                    // border is, and its widths are in one unit or the other
                    // depending on which of them was picked.
                    Some(Asking::PageBorders) => self.page_borders_changed(),
                    // The same for the Paragraph dialog, whose preview is the
                    // shape its fields describe.
                    Some(Asking::Paragraph) => {
                        if let Some(dialog) = self.dialog.take() {
                            let said = self.paragraph_dialog_says(&dialog);
                            let mut built = self.paragraph_dialog(&said);
                            built.carry_typing_from(&dialog);
                            self.dialog = Some(built);
                        }
                    }
                    _ => {}
                }
                self.needs_redraw = true;
                Response::Redraw
            }
            Reaction::Ignored => Response::Ignored,
        }
    }

    /// The pointer moving while a dialog is up.
    pub(super) fn dialog_hover(&mut self, x: i32, y: i32) -> bool {
        self.dialog.as_mut().is_some_and(|dialog| dialog.hover(x, y))
    }

    /// A key while a dialog is up.
    pub(super) fn dialog_key(&mut self, key: Key, shift: bool, control: bool) -> Response {
        let Some(dialog) = &mut self.dialog else { return Response::Ignored };
        let reaction = dialog.key(key, shift, control);
        self.reacted(reaction)
    }

    /// A character typed while a dialog is up.
    pub(super) fn dialog_character(&mut self, character: char) -> Response {
        let Some(dialog) = &mut self.dialog else { return Response::Ignored };
        let reaction = dialog.character(character);
        self.reacted(reaction)
    }

    /// Draws whatever dialog is up, over everything else.
    pub(super) fn draw_dialog(&mut self) {
        let Some(mut dialog) = self.dialog.take() else { return };
        let theme = self.theme;
        dialog.draw(&mut self.canvas, &mut self.chrome_engine, &mut self.renderer, &theme);
        self.dialog = Some(dialog);
    }

    /// Word's Word Count: how much of the document there is.
    ///
    /// The tick box at the bottom is Word's, and it does what Word's does: the
    /// counts are of the body alone, or of the body together with everything
    /// written round the edges of it.
    pub(super) fn open_word_count(&mut self) -> Response {
        let dialog = self.word_count_dialog(self.count_the_edges);
        self.ask(Asking::WordCount, dialog)
    }

    /// The text the counts are made of.
    ///
    /// Notes and text boxes are counted separately because Word counts them
    /// separately: they are not in the body, and a person asking how long the
    /// document is usually means the body.
    pub(super) fn counted_text(&self, edges: bool) -> String {
        let mut text = self.document.plain_text();
        if !edges {
            return text;
        }
        for kind in [wp_docx::notes::Kind::Footnote, wp_docx::notes::Kind::Endnote] {
            for note in self.document.notes(kind) {
                text.push('\n');
                text.push_str(&note.text);
            }
        }
        for shape in self.document.shapes() {
            for paragraph in &shape.text {
                text.push('\n');
                text.push_str(&paragraph.plain_text());
            }
        }
        text
    }

    /// The dialog itself, built afresh whenever the tick box changes — which is
    /// how the numbers come to move when it is ticked.
    fn word_count_dialog(&self, edges: bool) -> Dialog {
        let text = self.counted_text(edges);
        let words = super::draw::count_words(&text);
        let characters = text.chars().count();
        let spaceless = text.chars().filter(|character| !character.is_whitespace()).count();
        let paragraphs = self.document.paragraph_count();
        let lines: usize = self.pages.iter().map(|page| page.lines.len()).sum();

        let said = |label: &str, value: String| Field::Said { label: label.to_owned(), value };
        Dialog::message(
            "Word Count",
            vec![
                said("Pages", self.pages.len().max(1).to_string()),
                said("Words", words.to_string()),
                said("Characters (no spaces)", spaceless.to_string()),
                said("Characters (with spaces)", characters.to_string()),
                said("Paragraphs", paragraphs.to_string()),
                said("Lines", lines.to_string()),
                Field::Check {
                    label: "Include textboxes, footnotes and endnotes".to_owned(),
                    on: edges,
                },
            ],
        )
    }

    /// Word's Page Setup: the margins typed rather than chosen, and the paper
    /// and the way round it goes.
    pub(super) fn open_page_setup(&mut self) -> Response {
        let (top, right, bottom, left) = self.document.page_margins();
        let unit = self.unit;
        let shown = |twips: i32| crate::measure::format(twips, unit);

        let papers: Vec<String> =
            wp_docx::page::PAGE_SIZES.iter().map(|(name, ..)| (*name).to_owned()).collect();
        let paper = self
            .document
            .page_size_name()
            .and_then(|name| papers.iter().position(|found| found == name))
            .unwrap_or(0);

        let dialog = Dialog::new(
            "Page Setup",
            vec![
                // Word's arrangement: the four margins two by two inside a box
                // of their own, and the paper under them in a box of its own.
                Field::Group("Margins".to_owned()),
                Field::Columns(2),
                Field::Number {
                    label: "Top".to_owned(),
                    value: shown(top),
                    unit: self.unit.mark(),
                },
                Field::Number {
                    label: "Bottom".to_owned(),
                    value: shown(bottom),
                    unit: self.unit.mark(),
                },
                Field::Columns(2),
                Field::Number {
                    label: "Left".to_owned(),
                    value: shown(left),
                    unit: self.unit.mark(),
                },
                Field::Number {
                    label: "Right".to_owned(),
                    value: shown(right),
                    unit: self.unit.mark(),
                },
                Field::Group("Paper".to_owned()),
                Field::Columns(2),
                Field::Choice { label: "Paper size".to_owned(), items: papers, current: paper },
                Field::Choice {
                    label: "Orientation".to_owned(),
                    items: vec!["Portrait".to_owned(), "Landscape".to_owned()],
                    current: usize::from(self.document.is_landscape()),
                },
            ],
        );
        self.ask(Asking::PageSetup, dialog)
    }

    /// Takes what the Page Setup dialog says and puts it on the document.
    fn apply_page_setup(&mut self, dialog: &Dialog) -> Response {
        // A margin cannot be negative and cannot be so large that there is no
        // page left, which is what the measuring does about it; an empty box
        // falls back to an inch rather than to nothing.
        let unit = self.unit;
        let twips = |said: String| crate::measure::parse(&said, unit).unwrap_or(1440).max(0);
        let top = twips(dialog.said(MARGIN_TOP));
        let bottom = twips(dialog.said(MARGIN_BOTTOM));
        let left = twips(dialog.said(MARGIN_LEFT));
        let right = twips(dialog.said(MARGIN_RIGHT));

        let mut changed = self.document.set_page_margins(top, right, bottom, left);
        if let Some((_, width, height)) = wp_docx::page::PAGE_SIZES.get(dialog.chose(PAPER_SIZE)) {
            changed |= self.document.set_page_size(*width, *height);
        }
        changed |= self.document.set_landscape(dialog.chose(ORIENTATION) == 1);

        self.relayout();
        self.edited(changed, "Page setup")
    }
    /// Word's Bookmark dialog: a name for this place, and the names already in
    /// the document.
    pub(super) fn start_bookmark(&mut self) -> Response {
        let existing: Vec<String> =
            self.document.bookmarks().into_iter().map(|mark| mark.name).collect();
        let mut fields =
            vec![Field::Text { label: "Bookmark name".to_owned(), value: String::new() }];
        if !existing.is_empty() {
            fields.push(Field::Heading("Already in this document".to_owned()));
            for name in existing.iter().take(6) {
                fields.push(Field::Said { label: String::new(), value: name.clone() });
            }
        }
        self.ask(Asking::Bookmark, Dialog::new("Bookmark", fields))
    }

    /// Names the selection, or the caret.
    fn apply_bookmark(&mut self, dialog: &Dialog) -> Response {
        let wanted = dialog.said(0);
        let wanted = wanted.trim();
        if wanted.is_empty() {
            return Response::Redraw;
        }
        let name = wp_docx::bookmarks::sanitise_name(wanted);
        let changed = self.document.add_bookmark(&name);
        self.needs_redraw = true;
        self.edited(changed, &format!("Bookmark: {name}"))
    }
}

#[cfg(test)]
mod tests {
    use super::Asking;
    use crate::chrome::dialog::{Answer, Dialog, Field};
    use crate::editor::Editor;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event, Key, Modifiers};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn editor() -> Editor {
        let mut body = Body::default();
        for index in 0..8 {
            body.blocks.push(Block::Paragraph(Paragraph::text(&format!("Paragraph {index}"))));
        }
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        // These tests are written in inches, so they say so: what a box
        // shows otherwise depends on the machine the test runs on.
        editor.unit = crate::measure::Unit::Inches;
        editor.handle(Event::Resized { width: 1200, height: 800 });
        editor
    }

    #[test]
    fn a_dialog_takes_the_keyboard_away_from_the_document() {
        let mut editor = editor();
        let before = editor.document.plain_text();
        editor.ask(
            Asking::Bookmark,
            Dialog::new(
                "Bookmark",
                vec![Field::Text { label: "Bookmark name".to_owned(), value: String::new() }],
            ),
        );

        // Typing goes into the dialog, not into the page behind it.
        for character in "Start".chars() {
            editor.handle(Event::Char(character));
        }
        assert_eq!(editor.document.plain_text(), before, "the document took the typing");
        assert_eq!(editor.dialog.as_ref().map(|dialog| dialog.said(0)), Some("Start".to_owned()));
    }

    #[test]
    fn a_click_on_the_page_behind_does_nothing_while_a_dialog_is_up() {
        let mut editor = editor();
        editor.open_word_count();
        let caret = editor.document.caret();

        // Well away from the panel, which is drawn in the middle.
        editor.handle(Event::MouseDown { x: 20, y: 700, modifiers: Modifiers::default() });
        assert!(editor.in_dialog(), "the dialog went away");
        assert_eq!(editor.document.caret(), caret, "the caret moved behind the dialog");
    }

    #[test]
    fn escape_puts_a_dialog_away_and_changes_nothing() {
        let mut editor = editor();
        let margins = editor.document.page_margins();
        editor.open_page_setup();
        assert!(editor.in_dialog());

        editor.handle(Event::KeyDown { key: Key::Escape, modifiers: Modifiers::default() });
        assert!(!editor.in_dialog(), "the dialog stayed up");
        assert_eq!(editor.document.page_margins(), margins, "cancelling changed the page");
    }

    #[test]
    fn page_setup_puts_what_was_typed_onto_the_document() {
        let mut editor = editor();
        editor.open_page_setup();

        // The top margin, typed afresh: Word takes inches, and stores twips.
        if let Some(dialog) = &mut editor.dialog {
            dialog.fields[super::MARGIN_TOP] =
                Field::Number { label: "Top".to_owned(), value: "2".to_owned(), unit: "\"" };
        }
        editor.handle(Event::KeyDown { key: Key::Enter, modifiers: Modifiers::default() });

        assert!(!editor.in_dialog());
        assert_eq!(editor.document.page_margins().0, 2880, "two inches is 2880 twips");
    }

    #[test]
    fn the_bookmark_dialog_names_the_place_the_caret_is_in() {
        let mut editor = editor();
        editor.start_bookmark();
        if let Some(dialog) = &mut editor.dialog {
            dialog.fields[0] =
                Field::Text { label: "Bookmark name".to_owned(), value: "Chapter One".to_owned() };
        }
        editor.handle(Event::KeyDown { key: Key::Enter, modifiers: Modifiers::default() });

        let names: Vec<String> =
            editor.document.bookmarks().into_iter().map(|mark| mark.name).collect();
        // Word puts an underscore where a space was, because a name with a
        // space in it is not one a field can refer to.
        assert_eq!(names, vec!["Chapter_One".to_owned()]);
    }

    #[test]
    fn word_count_tells_and_does_not_ask() {
        let mut editor = editor();
        editor.open_word_count();
        let dialog = editor.dialog.as_ref().expect("a dialog");
        assert_eq!(dialog.buttons.len(), 1, "a dialog that tells has one button");

        // Every count Word shows, in the order Word shows them.
        let labels: Vec<&str> = dialog.fields.iter().filter_map(Field::label).collect();
        assert_eq!(
            labels,
            vec![
                "Pages",
                "Words",
                "Characters (no spaces)",
                "Characters (with spaces)",
                "Paragraphs",
                "Lines",
            ]
        );
        assert_eq!(editor.finish_dialog(Answer::Accept), wp_shell::Response::Redraw);
        assert!(!editor.in_dialog());
    }
}
