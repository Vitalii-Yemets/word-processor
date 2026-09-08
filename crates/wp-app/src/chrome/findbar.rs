//! The strip that finds and replaces, between the ribbon and the page.
//!
//! # Why a strip and not a dialog
//!
//! Word's Find and Replace is a floating dialog, and it is the one part of Word
//! that everybody moves out of the way before they can see what it found. A
//! strip across the top cannot cover the text it is talking about, which is the
//! whole point of the thing.
//!
//! It also keeps the promise the rest of this program makes: there are no
//! system controls in the window, so a dialog here would have to be drawn from
//! nothing anyway — and a strip is less to draw and more use.

use wp_layout::{LayoutEngine, Renderer};
use wp_raster::{Canvas, Color};

use super::icons::{self, Icon};
use super::theme::Theme;

/// How tall the strip is.
pub const HEIGHT: f32 = 34.0;

const FIELD_WIDTH: f32 = 210.0;
const FIELD_HEIGHT: f32 = 22.0;
const LABEL_GAP: f32 = 8.0;

/// What the strip is for.
///
/// The find strip and the comment strip are the same shape — a label, a field,
/// a couple of buttons — so they are the same strip with a different job.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Purpose {
    #[default]
    Find,
    Replace,
    /// Typing the text of a new comment.
    Comment,
    /// Typing the text of a new footnote or endnote.
    Note,
    /// Typing the words of a caption.
    Caption,
    /// Typing the name of a bookmark.
    Bookmark,
    /// Typing the words to list in the index.
    IndexEntry,
    /// Typing the details of a source, semicolon by semicolon.
    Source,
    /// Typing the address a link goes to.
    Link,
    /// Typing the word to print behind every page.
    Watermark,
    /// Typing one of the things the document says about itself.
    Property,
    /// Typing a citation to gather into the table of authorities.
    Authority,
    /// Typing the words for a text box.
    TextBox,
    /// Typing the words for a piece of WordArt.
    WordArt,
    /// Typing how tall a table row should be.
    RowHeight,
    /// Typing the address to print on an envelope.
    Envelope,
    /// Typing the numbers a chart is drawn from.
    Chart,
    /// Typing the condition of a merge rule.
    Rule,
    /// Typing an equation in the linear format.
    Equation,
    /// Typing the address of a video and what to call it.
    Video,
    /// Typing a name for a macro that has just been recorded.
    Macro,
    /// Typing what the boxes of a diagram say.
    Diagram,
    /// Typing who is to sign, and what they are called.
    Signature,
    /// Typing the words to print on every label of a sheet.
    Label,
}

/// Which part of the strip was pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    FindField,
    ReplaceField,
    FindNext,
    FindPrevious,
    Replace,
    ReplaceAll,
    MatchCase,
    Close,
    /// Finishes a comment.
    Add,
}

/// Which field has the keyboard.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Focus {
    #[default]
    Find,
    Replace,
}

/// The strip, and what has been typed into it.
#[derive(Clone, Debug, Default)]
pub struct FindBar {
    pub needle: String,
    pub replacement: String,
    pub focus: Focus,
    /// Whether the search minds about capital letters.
    pub match_case: bool,
    /// Whether the replace field is shown at all.
    ///
    /// Word has two commands and one dialog; this has two commands and one
    /// strip, which grows a second field when the command was Replace.
    pub replacing: bool,
    /// What the strip is being used for.
    pub purpose: Purpose,
    /// What the last search found, for the count shown on the right.
    pub found: usize,
    /// Where each part ended up when it was last drawn.
    placed: Vec<(Hit, f32, f32, f32, f32)>,
    hovered: Option<Hit>,
}

impl FindBar {
    #[must_use]
    pub fn new(replacing: bool) -> Self {
        let purpose = if replacing { Purpose::Replace } else { Purpose::Find };
        Self { replacing, purpose, ..Self::default() }
    }

    /// The strip that takes the text of a new comment.
    #[must_use]
    pub fn for_comment() -> Self {
        Self { purpose: Purpose::Comment, ..Self::default() }
    }

    /// The strip that takes the text of a new note.
    #[must_use]
    pub fn for_note() -> Self {
        Self { purpose: Purpose::Note, ..Self::default() }
    }

    /// The strip that takes some other single line of text.
    #[must_use]
    pub fn for_purpose(purpose: Purpose) -> Self {
        Self { purpose, ..Self::default() }
    }

    /// Types one character into the focused field, or takes one away.
    ///
    /// Returns whether the needle changed, because that is what has to set the
    /// search running again.
    pub fn type_character(&mut self, character: char) -> bool {
        let searching = self.focus == Focus::Find;
        let field = match self.focus {
            Focus::Find => &mut self.needle,
            Focus::Replace => &mut self.replacement,
        };
        match character {
            '\u{8}' => {
                field.pop();
            }
            character if character.is_control() => return false,
            character => field.push(character),
        }
        searching
    }

    /// What a point in the strip stands for, if anything.
    #[must_use]
    pub fn hit(&self, x: i32, y: i32) -> Option<Hit> {
        let (x, y) = (x as f32, y as f32);
        self.placed
            .iter()
            .find(|(_, left, top, width, height)| {
                x >= *left && x < left + width && y >= *top && y < top + height
            })
            .map(|(hit, ..)| *hit)
    }

    /// Lights up whatever the pointer is over. Returns whether that changed.
    pub fn hover(&mut self, x: i32, y: i32) -> bool {
        let found = self.hit(x, y);
        let changed = found != self.hovered;
        self.hovered = found;
        changed
    }

    /// Draws the strip across the top of the page area.
    pub fn draw(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        top: f32,
        left_edge: f32,
        theme: &Theme,
    ) {
        self.placed.clear();

        let width = canvas.width() as f32;
        canvas.fill_rect(left_edge as i32, top as i32, width as i32, HEIGHT as i32, theme.ribbon);
        canvas.fill_rect(
            left_edge as i32,
            (top + HEIGHT - 1.0) as i32,
            width as i32,
            1,
            theme.ribbon_edge,
        );

        let middle = top + (HEIGHT - FIELD_HEIGHT) / 2.0;
        let baseline = middle + 15.0;
        let mut x = left_edge + 10.0;

        // The two fields, each with the word that names it.
        let first_label = match self.purpose {
            Purpose::Comment => "Comment",
            Purpose::Note => "Note",
            Purpose::Caption => "Caption",
            Purpose::Bookmark => "Name",
            Purpose::IndexEntry => "Index entry",
            Purpose::Source => "Source",
            Purpose::Link => "Address",
            Purpose::Watermark => "Watermark",
            Purpose::Property => "Value",
            Purpose::Authority => "Citation",
            Purpose::TextBox => "Text box",
            Purpose::WordArt => "WordArt",
            Purpose::RowHeight => "Height",
            Purpose::Chart => "Numbers",
            Purpose::Rule => "Rule",
            Purpose::Equation => "Equation",
            Purpose::Video => "Video",
            Purpose::Macro => "Name",
            Purpose::Diagram => "Boxes",
            Purpose::Signature => "Signer",
            Purpose::Envelope => "Address",
            Purpose::Label => "Label",
            Purpose::Find | Purpose::Replace => "Find",
        };
        x = self.draw_field(
            canvas,
            engine,
            renderer,
            first_label,
            &self.needle.clone(),
            Hit::FindField,
            self.focus == Focus::Find,
            x,
            middle,
            baseline,
            theme,
        );

        if self.replacing {
            x = self.draw_field(
                canvas,
                engine,
                renderer,
                "Replace",
                &self.replacement.clone(),
                Hit::ReplaceField,
                self.focus == Focus::Replace,
                x + 10.0,
                middle,
                baseline,
                theme,
            );
        }

        // The buttons, in the order they are used.
        x += 12.0;
        for (hit, icon, label) in self.buttons() {
            x = self.draw_button(
                canvas, engine, renderer, hit, icon, label, x, middle, baseline, theme,
            );
        }

        // How many the search found, which is the one number a person wants.
        // A comment strip counts nothing; a search strip says how many it found,
        // which is the one number a person wants.
        let count = if self.purpose != Purpose::Find && self.purpose != Purpose::Replace
            || self.needle.is_empty()
        {
            String::new()
        } else if self.found == 0 {
            "no matches".to_owned()
        } else if self.found == 1 {
            "1 match".to_owned()
        } else {
            format!("{} matches", self.found)
        };
        if !count.is_empty() {
            let line = engine.simple_line(&count, x + 10.0, baseline, 8.0, theme.dim_text);
            renderer.draw_onto(canvas, &line, 0.0, 0.0);
        }

        // The close button, at the far right where every strip keeps it.
        let close_left = width - 30.0;
        if self.hovered == Some(Hit::Close) {
            canvas.fill_rect(
                close_left as i32,
                middle as i32,
                22,
                FIELD_HEIGHT as i32,
                theme.hover,
            );
        }
        icons::draw_sized(canvas, Icon::Close, close_left + 3.0, middle + 3.0, 16.0, theme.text);
        self.placed.push((Hit::Close, close_left, middle, 22.0, FIELD_HEIGHT));
    }

    /// The buttons the strip carries, which depend on what it was opened for.
    fn buttons(&self) -> Vec<(Hit, Icon, &'static str)> {
        match self.purpose {
            Purpose::Comment => return vec![(Hit::Add, Icon::NewComment, "Add Comment")],
            Purpose::Note => return vec![(Hit::Add, Icon::Footnote, "Add Note")],
            Purpose::Caption => return vec![(Hit::Add, Icon::Caption, "Add Caption")],
            Purpose::Bookmark => return vec![(Hit::Add, Icon::Bookmark, "Add Bookmark")],
            Purpose::IndexEntry => return vec![(Hit::Add, Icon::MarkEntry, "Mark Entry")],
            Purpose::Source => return vec![(Hit::Add, Icon::ManageSources, "Add Source")],
            Purpose::Link => return vec![(Hit::Add, Icon::Link, "Add Link")],
            Purpose::Watermark => return vec![(Hit::Add, Icon::Watermark, "Add Watermark")],
            Purpose::Property => return vec![(Hit::Add, Icon::Save, "Set")],
            Purpose::Authority => return vec![(Hit::Add, Icon::MarkCitation, "Mark Citation")],
            Purpose::TextBox => return vec![(Hit::Add, Icon::TextBox, "Add Text Box")],
            Purpose::WordArt => return vec![(Hit::Add, Icon::WordArt, "Add WordArt")],
            Purpose::RowHeight => return vec![(Hit::Add, Icon::TableProperties, "Set Height")],
            Purpose::Chart => return vec![(Hit::Add, Icon::Chart, "Draw Chart")],
            Purpose::Rule => return vec![(Hit::Add, Icon::Rules, "Add Rule")],
            Purpose::Equation => return vec![(Hit::Add, Icon::Equation, "Add Equation")],
            Purpose::Video => return vec![(Hit::Add, Icon::Video, "Add Video")],
            Purpose::Macro => return vec![(Hit::Add, Icon::Macros, "Save Macro")],
            Purpose::Diagram => return vec![(Hit::Add, Icon::SmartArt, "Draw Diagram")],
            Purpose::Signature => return vec![(Hit::Add, Icon::Signature, "Add Signature Line")],
            Purpose::Envelope => return vec![(Hit::Add, Icon::Envelope, "Make Envelope")],
            Purpose::Label => return vec![(Hit::Add, Icon::Labels, "Make Labels")],
            Purpose::Find | Purpose::Replace => {}
        }
        let mut out = vec![
            (Hit::FindPrevious, Icon::Previous, "Previous"),
            (Hit::FindNext, Icon::Next, "Next"),
        ];
        if self.replacing {
            out.push((Hit::Replace, Icon::Replace, "Replace"));
            out.push((Hit::ReplaceAll, Icon::UpdateTable, "Replace All"));
        }
        out.push((Hit::MatchCase, Icon::ChangeCase, "Match Case"));
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_field(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        label: &str,
        text: &str,
        hit: Hit,
        focused: bool,
        left: f32,
        top: f32,
        baseline: f32,
        theme: &Theme,
    ) -> f32 {
        let line = engine.simple_line(label, left, baseline, 8.0, theme.dim_text);
        let label_width = line.width - left;
        renderer.draw_onto(canvas, &line, 0.0, 0.0);

        let box_left = left + label_width + LABEL_GAP;
        canvas.fill_rect(
            box_left as i32,
            top as i32,
            FIELD_WIDTH as i32,
            FIELD_HEIGHT as i32,
            theme.field,
        );
        outline(
            canvas,
            box_left,
            top,
            FIELD_WIDTH,
            FIELD_HEIGHT,
            if focused { theme.emphasis } else { theme.field_edge },
        );

        let line = engine.simple_line(text, box_left + 6.0, baseline, 8.5, theme.text);
        let measured = line.width - (box_left + 6.0);
        renderer.draw_onto(canvas, &line, 0.0, 0.0);

        // The caret, so a field with the keyboard looks like one.
        if focused {
            canvas.fill_rect(
                (box_left + 6.0 + measured) as i32,
                (top + 4.0) as i32,
                1,
                (FIELD_HEIGHT - 8.0) as i32,
                theme.text,
            );
        }

        self.placed.push((hit, box_left, top, FIELD_WIDTH, FIELD_HEIGHT));
        box_left + FIELD_WIDTH
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_button(
        &mut self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        hit: Hit,
        icon: Icon,
        label: &str,
        left: f32,
        top: f32,
        baseline: f32,
        theme: &Theme,
    ) -> f32 {
        let measured = engine.simple_line(label, 0.0, 0.0, 8.0, theme.text).width;
        let width = measured + 30.0;

        let on = hit == Hit::MatchCase && self.match_case;
        if on {
            canvas.fill_rect(
                left as i32,
                top as i32,
                width as i32,
                FIELD_HEIGHT as i32,
                theme.accent,
            );
        } else if self.hovered == Some(hit) {
            canvas.fill_rect(
                left as i32,
                top as i32,
                width as i32,
                FIELD_HEIGHT as i32,
                theme.hover,
            );
        }
        let ink = if on { theme.on_accent() } else { theme.text };

        icons::draw_sized(canvas, icon, left + 4.0, top + 3.0, 16.0, ink);
        let line = engine.simple_line(label, left + 24.0, baseline, 8.0, ink);
        renderer.draw_onto(canvas, &line, 0.0, 0.0);

        self.placed.push((hit, left, top, width, FIELD_HEIGHT));
        left + width + 4.0
    }
}

fn outline(canvas: &mut Canvas, x: f32, y: f32, width: f32, height: f32, colour: Color) {
    let (x, y, width, height) = (x as i32, y as i32, width as i32, height as i32);
    canvas.fill_rect(x, y, width, 1, colour);
    canvas.fill_rect(x, y + height - 1, width, 1, colour);
    canvas.fill_rect(x, y, 1, height, colour);
    canvas.fill_rect(x + width - 1, y, 1, height, colour);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_goes_into_whichever_field_has_the_keyboard() {
        let mut bar = FindBar::new(true);
        assert!(bar.type_character('a'), "typing in the find field restarts the search");
        bar.focus = Focus::Replace;
        assert!(!bar.type_character('b'), "typing a replacement does not");
        assert_eq!((bar.needle.as_str(), bar.replacement.as_str()), ("a", "b"));
    }

    #[test]
    fn backspace_takes_a_character_off_the_focused_field() {
        let mut bar = FindBar::new(false);
        bar.needle = "abc".to_owned();
        bar.type_character('\u{8}');
        assert_eq!(bar.needle, "ab");
    }

    #[test]
    fn a_control_character_is_not_typed_into_a_field() {
        let mut bar = FindBar::new(false);
        bar.type_character('\t');
        assert!(bar.needle.is_empty());
    }
}
