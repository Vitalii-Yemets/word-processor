//! Pasting, and the four answers Word gives to the question a paste asks.
//!
//! # The question
//!
//! Text copied from somewhere has formatting of its own, and the place it is
//! going has formatting of its own, and they disagree. Word does not ask before
//! pasting — a dialog in front of the commonest gesture in the program would be
//! unbearable — it pastes the likeliest way and then offers the others on a
//! little button at the end of what it put down. Choosing one takes the paste
//! back and puts it down again the other way, which is why every paste is one
//! thing to undo.
//!
//! # The four
//!
//! **Keep Source Formatting** — it arrives as it left. **Merge Formatting** —
//! the emphasis comes and the rest is the destination's. **Picture** — what was
//! copied, drawn, so it cannot reflow. **Keep Text Only** — the words alone.
//! The first two are [`wp_docx::clipboard::Formatting`]; the last is an
//! ordinary text paste; the picture is drawn here, because only this layer can
//! lay a document out and rasterize it.
//!
//! # What is offered and what is not
//!
//! All four when the clipboard still holds what this program copied, because
//! only then is there any formatting to keep or to draw. Text put there by
//! another program is words and nothing else, so there is one answer and Word
//! does not draw a menu of one either.

use wp_docx::clipboard::Formatting;
use wp_docx::model::{Block, Body};
use wp_docx::TextPosition;
use wp_layout::{Device, LayoutEngine, PageMetrics, Renderer};
use wp_raster::Canvas;
use wp_shell::Response;

use crate::chrome::icons::Icon;
use crate::chrome::pastebadge::{self, PasteBadge};
use crate::chrome::{Choice, Popup};

use super::Editor;

/// How the copied content goes in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PasteAs {
    KeepSource,
    Merge,
    Picture,
    TextOnly,
}

impl PasteAs {
    /// The four, in the order Word's button offers them.
    pub(super) const ALL: &'static [Self] =
        &[Self::KeepSource, Self::Merge, Self::Picture, Self::TextOnly];

    /// What Word calls it.
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::KeepSource => "Keep Source Formatting",
            Self::Merge => "Merge Formatting",
            Self::Picture => "Picture",
            Self::TextOnly => "Keep Text Only",
        }
    }

    /// The letter Word underlines, which is what picks it from the keyboard.
    pub(super) fn letter(self) -> char {
        match self {
            Self::KeepSource => 'k',
            Self::Merge => 'm',
            Self::Picture => 'u',
            Self::TextOnly => 't',
        }
    }

    /// The drawing on its row of the menu.
    pub(super) fn icon(self) -> Icon {
        match self {
            Self::KeepSource => Icon::Clipboard,
            // The brush is Word's own choice for merging formatting: it is the
            // same idea as the Format Painter, formatting taken from where the
            // text lands rather than from where it came from.
            Self::Merge => Icon::Brush,
            Self::Picture => Icon::Picture,
            Self::TextOnly => Icon::Letter,
        }
    }
}

/// What the last paste put down, so that another answer can replace it.
#[derive(Clone, Debug)]
pub(super) struct Pasted {
    /// Where the paste ended, so the button sits at the end of it.
    end: TextPosition,
    /// The formatted content, empty when the clipboard was another program's.
    blocks: Vec<Block>,
    /// The words, which is what a text-only paste puts down.
    text: String,
    /// Which answer is in force, so the menu can mark it.
    how: PasteAs,
    /// Whether the pointer is on the button.
    hot: bool,
}

impl Editor {
    /// Ctrl+V: pastes the likeliest way.
    ///
    /// The likeliest way is Word's: everything that was copied, when the
    /// clipboard still holds what this program put there, and the words alone
    /// when it does not.
    pub(super) fn paste(&mut self) -> Response {
        let how = if self.formatted_clipboard().is_empty() {
            PasteAs::TextOnly
        } else {
            PasteAs::KeepSource
        };
        self.paste_as(how)
    }

    /// Ctrl+Shift+V: the words with none of their formatting.
    pub(super) fn paste_plain(&mut self) -> Response {
        self.paste_as(PasteAs::TextOnly)
    }

    /// Pastes a chosen way.
    pub(super) fn paste_as(&mut self, how: PasteAs) -> Response {
        let Some(text) = wp_shell::clipboard::text() else {
            return self.report("The clipboard holds no text");
        };
        let blocks = self.formatted_clipboard();
        self.put_down(&text, &blocks, how)
    }

    /// What was copied here, if the clipboard still holds it.
    ///
    /// Anything else on the clipboard means another program has copied since,
    /// and what that program put there is what the person last asked for.
    fn formatted_clipboard(&self) -> Vec<Block> {
        let Some(text) = wp_shell::clipboard::text() else { return Vec::new() };
        match &self.clipboard {
            Some((copied, blocks)) if *copied == text => blocks.clone(),
            _ => Vec::new(),
        }
    }

    /// Puts the content down, and leaves the button at the end of it.
    pub(super) fn put_down(&mut self, text: &str, blocks: &[Block], how: PasteAs) -> Response {
        // Nothing formatted to work with means there is only one thing that
        // can be done, whatever was asked for.
        let how = if blocks.is_empty() { PasteAs::TextOnly } else { how };
        let characters = text.chars().count();

        let changed = match how {
            PasteAs::KeepSource => self.document.paste_blocks_as(blocks, Formatting::Source),
            PasteAs::Merge => self.document.paste_blocks_as(blocks, Formatting::Merged),
            PasteAs::Picture => self.paste_as_picture(blocks),
            PasteAs::TextOnly => self.document.paste(text),
        };
        if !changed {
            return self.report("Nothing was pasted");
        }

        self.pasted = Some(Pasted {
            end: self.document.caret(),
            blocks: blocks.to_vec(),
            text: text.to_owned(),
            how,
            hot: false,
        });

        let note = match how {
            PasteAs::Picture => String::from("Pasted as a picture"),
            PasteAs::TextOnly if !blocks.is_empty() => {
                format!("Pasted {characters} characters, unformatted")
            }
            _ => format!("Pasted {characters} characters"),
        };
        self.edited(true, &note)
    }

    /// Takes the last paste back and puts it down another way.
    pub(super) fn paste_again(&mut self, how: PasteAs) -> Response {
        let Some(previous) = self.pasted.take() else { return Response::Ignored };
        self.popup = None;
        if previous.how == how {
            // Choosing what is already in force is not an edit, and undoing a
            // paste to redo the same paste would be one.
            self.pasted = Some(previous);
            self.needs_redraw = true;
            return Response::Redraw;
        }

        // A paste is one gesture, so one undo takes the whole of it back —
        // whichever of the four it was. See `Document::paste_blocks_as`.
        self.document.undo();
        self.put_down(&previous.text, &previous.blocks, how)
    }

    /// Which of the four the button offers.
    ///
    /// Decided by what was pasted and not by what is on the clipboard now. The
    /// button is about the text already on the page: another program may have
    /// copied something else since, and that has nothing to do with the paste
    /// this button is offering to redo.
    pub(super) fn paste_options(&self) -> Vec<PasteAs> {
        match &self.pasted {
            Some(pasted) if !pasted.blocks.is_empty() => PasteAs::ALL.to_vec(),
            _ => vec![PasteAs::TextOnly],
        }
    }

    /// Whether the clipboard holds formatting worth deciding about.
    ///
    /// The other question, and asked before anything has been pasted: it is
    /// what the right-click menu needs to know to decide whether to offer the
    /// four at all.
    pub(super) fn clipboard_has_formatting(&self) -> bool {
        !self.formatted_clipboard().is_empty()
    }

    /// Opens the little menu of the four.
    pub(super) fn open_paste_menu(&mut self) -> Response {
        let Some(pasted) = &self.pasted else { return Response::Ignored };
        let Some(badge) = self.paste_badge() else { return Response::Ignored };

        let options = self.paste_options();
        let current = options.iter().position(|option| *option == pasted.how);
        let items: Vec<String> = options
            .iter()
            .map(|option| format!("{} ({})", option.label(), option.letter().to_ascii_uppercase()))
            .collect();
        let rows = options
            .iter()
            .map(|option| {
                crate::chrome::popup::Row::new(crate::chrome::popup::Kind::Choice, option.icon())
            })
            .collect();

        self.popup = Some(
            Popup::new(
                Choice::PasteOption,
                items,
                current,
                badge.left,
                badge.top + pastebadge::HEIGHT,
                240.0,
            )
            .with_rows(rows),
        );
        self.needs_redraw = true;
        Response::Redraw
    }

    /// One of the menu's rows was picked.
    pub(super) fn choose_paste_option(&mut self, index: usize) -> Response {
        let Some(option) = self.paste_options().get(index).copied() else {
            return Response::Ignored;
        };
        self.paste_again(option)
    }

    /// Control on its own, which is Word's way to the options from the
    /// keyboard. It means nothing when there is nothing to offer.
    pub(super) fn control_pressed_alone(&mut self) -> Response {
        if self.pasted.is_none() {
            return Response::Ignored;
        }
        if self.popup.as_ref().is_some_and(|popup| popup.choice == Choice::PasteOption) {
            self.popup = None;
            self.needs_redraw = true;
            return Response::Redraw;
        }
        self.open_paste_menu()
    }

    /// Where the button is, if there is one to draw.
    ///
    /// Worked out afresh rather than remembered, because the page it points at
    /// moves: scrolling, a window resized, the text reflowed by an edit
    /// somewhere above it. A remembered place would be right until the first
    /// of those and wrong afterwards.
    pub(super) fn paste_badge(&self) -> Option<PasteBadge> {
        let pasted = self.pasted.as_ref()?;
        let (x, y, height) = self.caret_rect_at(pasted.end)?;
        let mut badge = PasteBadge::new(x, y + height + pastebadge::DROP);
        badge.hot = pasted.hot;

        // Not over the ribbon and not under the status strip: a button drawn
        // outside the page is a button pointing at nothing.
        if badge.top < self.content_top() || badge.top + pastebadge::HEIGHT > self.window_bottom() {
            return None;
        }
        Some(badge)
    }

    /// Draws it, when there is one.
    pub(super) fn draw_paste_badge(&mut self) {
        let Some(badge) = self.paste_badge() else { return };
        let theme = self.theme;
        badge.draw(&mut self.canvas, &mut self.chrome_engine, &mut self.renderer, &theme);
    }

    /// Whether a point is on the button.
    pub(super) fn over_paste_badge(&self, x: i32, y: i32) -> bool {
        self.paste_badge().is_some_and(|badge| badge.covers(x, y))
    }

    /// Lights the button up under the pointer. True when it changed.
    pub(super) fn follow_paste_badge(&mut self, x: i32, y: i32) -> bool {
        let over = self.over_paste_badge(x, y);
        let Some(pasted) = &mut self.pasted else { return false };
        if pasted.hot == over {
            return false;
        }
        pasted.hot = over;
        true
    }

    /// Puts the button away.
    ///
    /// Word's rule: it goes at the next thing you do — a key, a click
    /// somewhere else, Escape — because by then the question it was asking has
    /// been answered by going on without it.
    pub(super) fn forget_paste(&mut self) {
        if self.pasted.take().is_some() {
            self.needs_redraw = true;
        }
    }

    /// Whether the button is showing, which is what Escape has to know.
    pub(super) fn offering_paste_options(&self) -> bool {
        self.pasted.is_some()
    }

    /// Word's Picture: what was copied, drawn.
    ///
    /// The copied paragraphs are laid out on their own — against this document,
    /// so its styles and its theme decide how they look — and what comes out is
    /// rasterized and put in as a picture. Nothing about it can reflow
    /// afterwards, which is the whole point of the option: it is a photograph
    /// of the text, not the text.
    fn paste_as_picture(&mut self, blocks: &[Block]) -> bool {
        let Some((bytes, width, height)) = self.picture_of(blocks) else { return false };

        // Drawn at twice the screen's resolution and measured as though it were
        // at the screen's, so the picture is the size the text was and is not
        // soft when the document goes to a printer.
        let per_dot = wp_docx::EMU_PER_INCH / i64::from(PICTURE_DPI);
        let mut across = width as i64 * per_dot;
        let mut down = height as i64 * per_dot;
        let room = self.text_width_emu();
        if room > 0 && across > room {
            down = down * room / across;
            across = room;
        }

        self.document.begin_gesture();
        if self.document.selection().is_some() {
            self.document.delete_selection();
        }
        let inserted = self.document.insert_picture(&bytes, "png", across, down).unwrap_or(false);
        self.document.end_gesture();
        inserted
    }

    /// Lays the copied content out and draws it, giving back a PNG and its size
    /// in dots.
    fn picture_of(&mut self, blocks: &[Block]) -> Option<(Vec<u8>, usize, usize)> {
        let body = Body { blocks: blocks.to_vec() };
        let device = Device::from_dots(f32::from(PICTURE_DPI), 0.0, 0.0, 0.0, 0.0);
        let mut engine = LayoutEngine::for_device(self.library, device);

        // A page as wide as the text is and as tall as it needs, with no
        // margins: what is being drawn is the words, not the paper round them.
        let metrics = PageMetrics {
            width: self.text_width_points(),
            height: TALL_ENOUGH,
            margin_top: 0.0,
            margin_right: 0.0,
            margin_bottom: 0.0,
            margin_left: 0.0,
            columns: 1,
            column_gap: 0.0,
        };
        let pages = engine.layout_body(&body, &self.document, metrics);
        let page = pages.first()?;

        let (width, height) = ink_of(page)?;
        let mut canvas = Canvas::new(width, height);
        let mut renderer = Renderer::new(self.library);
        renderer.draw_onto(&mut canvas, page, 0.0, 0.0);
        Some((wp_raster::encode_png(&canvas), width, height))
    }

    /// How wide the text is on this document's page, in points.
    fn text_width_points(&self) -> f32 {
        let metrics = PageMetrics::from_document(&self.document);
        (metrics.width - metrics.margin_left - metrics.margin_right).max(1.0)
    }
}

/// How many dots to the inch a pasted picture is drawn at.
const PICTURE_DPI: u16 = 192;

/// The height of the page the copied content is laid out on, in points.
///
/// Large enough that nothing anybody pastes falls onto a second page, where it
/// would be left out of the picture. Ten thousand points is eleven feet.
const TALL_ENOUGH: f32 = 10_000.0;

/// How much of a laid-out page has anything on it, in dots.
///
/// Everything drawn is measured, not only the letters: a rule under a heading,
/// the shading behind a paragraph and a picture inside the copied text all
/// count, and a picture cropped to the letters would cut them off.
fn ink_of(page: &wp_layout::Page) -> Option<(usize, usize)> {
    let mut right = 0.0f32;
    let mut bottom = 0.0f32;
    let mut anything = false;

    for glyph in &page.glyphs {
        anything = true;
        right = right.max(glyph.x + glyph.advance);
        bottom = bottom.max(glyph.baseline + glyph.size * DESCENT);
    }
    for image in &page.images {
        anything = true;
        right = right.max(image.x + image.width);
        bottom = bottom.max(image.y + image.height);
    }
    for decoration in &page.decorations {
        anything = true;
        right = right.max(decoration.x + decoration.width);
        bottom = bottom.max(decoration.y + decoration.height);
    }
    if !anything {
        return None;
    }

    // A little room round it, so an outline or an accent at the edge is not
    // shaved off by the rounding.
    let width = (right.ceil() as usize + 2).min(MOST_DOTS);
    let height = (bottom.ceil() as usize + 2).min(MOST_DOTS);
    if width == 0 || height == 0 {
        return None;
    }
    Some((width, height))
}

/// How far below the baseline a letter can reach, as a fraction of its size.
///
/// A descender is about a fifth of the em in every face that has one; a quarter
/// leaves room for the ones that go further without leaving a visible margin.
const DESCENT: f32 = 0.25;

/// The most dots a pasted picture can be across or down.
///
/// A guard rather than a limit anybody will meet: at 192 dots to the inch this
/// is fifty inches, and the copied content would have to be enormous. Without
/// it a bad measurement would ask for a canvas that cannot be allocated.
const MOST_DOTS: usize = 9_600;

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Something to copy")));
        body.blocks.push(Block::Paragraph(Paragraph::text("Somewhere to put it")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    #[test]
    fn every_option_word_offers_is_here() {
        assert_eq!(PasteAs::ALL.len(), 4);
        // Word's letters, which is what picks one from the keyboard.
        let letters: Vec<char> = PasteAs::ALL.iter().map(|option| option.letter()).collect();
        assert_eq!(letters, vec!['k', 'm', 'u', 't']);
    }

    #[test]
    fn nothing_formatted_means_one_answer() {
        // Nothing has been pasted, so there is nothing to offer another way of
        // pasting, and a menu of one is not a menu.
        let editor = editor();
        assert_eq!(editor.paste_options(), vec![PasteAs::TextOnly]);
    }

    #[test]
    fn what_the_button_offers_is_about_what_was_pasted() {
        // Not about the clipboard: the machine running the tests has nothing on
        // its clipboard at all, and the four are still the right answer,
        // because four things could be done with what is on the page.
        let mut editor = editor();
        editor.document.set_caret(TextPosition::new(1, 0));
        let blocks = vec![Block::Paragraph(Paragraph::text("pasted"))];
        editor.put_down("pasted", &blocks, PasteAs::KeepSource);

        assert_eq!(editor.paste_options(), PasteAs::ALL.to_vec());
        assert!(!editor.clipboard_has_formatting(), "the clipboard held something after all");
    }

    #[test]
    fn there_is_no_button_until_something_is_pasted() {
        let editor = editor();
        assert!(!editor.offering_paste_options());
        assert!(editor.paste_badge().is_none());
    }

    #[test]
    fn the_button_appears_where_the_paste_ended() {
        let mut editor = editor();
        editor.document.set_caret(TextPosition::new(1, 0));
        let blocks = vec![Block::Paragraph(Paragraph::text("pasted"))];
        editor.put_down("pasted", &blocks, PasteAs::KeepSource);

        assert!(editor.offering_paste_options());
        let badge = editor.paste_badge().expect("a button");
        let (_, y, height) = editor
            .caret_rect_at(TextPosition::new(1, "pasted".len()))
            .expect("the end of the paste");
        assert!(
            (badge.top - (y + height + pastebadge::DROP)).abs() < 0.5,
            "the button is not under the pasted text"
        );
    }

    #[test]
    fn choosing_another_option_replaces_the_paste_rather_than_adding_to_it() {
        let mut editor = editor();
        let before = editor.document.plain_text();
        editor.document.set_caret(TextPosition::new(1, 0));

        let blocks = vec![Block::Paragraph(Paragraph::text("pasted"))];
        editor.put_down("pasted", &blocks, PasteAs::KeepSource);
        let once = editor.document.plain_text();
        assert_ne!(once, before);

        editor.paste_again(PasteAs::Merge);
        assert_eq!(
            editor.document.plain_text(),
            once,
            "the text was pasted twice instead of once, another way"
        );
    }

    #[test]
    fn choosing_what_is_already_in_force_changes_nothing() {
        let mut editor = editor();
        editor.document.set_caret(TextPosition::new(1, 0));
        let blocks = vec![Block::Paragraph(Paragraph::text("pasted"))];
        editor.put_down("pasted", &blocks, PasteAs::KeepSource);
        let once = editor.document.plain_text();

        editor.paste_again(PasteAs::KeepSource);
        assert_eq!(editor.document.plain_text(), once);
        assert!(editor.offering_paste_options(), "the button went away");
    }

    #[test]
    fn the_button_goes_at_the_next_thing_done() {
        let mut editor = editor();
        editor.document.set_caret(TextPosition::new(1, 0));
        let blocks = vec![Block::Paragraph(Paragraph::text("pasted"))];
        editor.put_down("pasted", &blocks, PasteAs::KeepSource);
        assert!(editor.offering_paste_options());

        editor.handle(Event::Char('x'));
        assert!(!editor.offering_paste_options(), "the button stayed after typing");
    }

    #[test]
    fn control_on_its_own_means_nothing_when_there_is_nothing_to_offer() {
        let mut editor = editor();
        assert_eq!(editor.handle(Event::ControlKey), Response::Ignored);
    }

    #[test]
    fn a_picture_of_the_copied_text_is_a_picture_of_it() {
        let mut editor = editor();
        let blocks = vec![Block::Paragraph(Paragraph::text("Drawn, not typed"))];
        let (bytes, width, height) = editor.picture_of(&blocks).expect("a picture");

        assert!(width > 0 && height > 0, "an empty picture");
        assert!(bytes.starts_with(&[0x89, b'P', b'N', b'G']), "not a PNG");
        // Wider than it is tall: it is one line of text.
        assert!(width > height, "one line came out {width} by {height}");
    }

    #[test]
    fn pasting_as_a_picture_puts_a_picture_in_the_document() {
        let mut editor = editor();
        editor.document.set_caret(TextPosition::new(1, 0));
        let blocks = vec![Block::Paragraph(Paragraph::text("Drawn, not typed"))];
        editor.put_down("Drawn, not typed", &blocks, PasteAs::Picture);

        let saved = editor.document.save().expect("saving");
        let reopened = Document::open(&saved).expect("reopening");
        let pictures = reopened
            .body()
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::Paragraph(paragraph) => Some(paragraph),
                Block::Table(_) => None,
            })
            .flat_map(|paragraph| paragraph.runs.iter())
            .flat_map(|run| run.content.iter())
            .filter(|piece| matches!(piece, wp_docx::model::RunContent::Picture(_)))
            .count();
        assert_eq!(pictures, 1, "the picture did not go in");

        // And the words did not go in as words: a picture of text is not text.
        assert!(
            !reopened.plain_text().contains("Drawn, not typed"),
            "the text went in as well as the picture"
        );
    }
}
