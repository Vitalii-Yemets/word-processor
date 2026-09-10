//! The little label that appears when the pointer rests on a ribbon button.
//!
//! # Why it matters more than it looks
//!
//! Half the ribbon is buttons with a drawing and no words: the whole Font group
//! after the first two boxes, most of Paragraph, all of Arrange. Without a tip
//! the only way to find out what one of them does is to press it and see, which
//! is not a thing anybody should have to do in a program that edits their
//! documents.
//!
//! # What Word does
//!
//! Waits about half a second, then shows the command's name with its shortcut
//! after it, hanging under the button. It goes the moment the pointer leaves,
//! and pressing the button takes it away too.
//!
//! Word's screen tips also carry a sentence of explanation and sometimes a
//! picture. This shows the name and the shortcut, which is what tells somebody
//! which button they are on.

use wp_docx::model::Alignment;
use wp_docx::CharacterFormat;
use wp_layout::{LayoutEngine, Renderer};
use wp_raster::Canvas;

use super::theme::Theme;
use super::Command;

/// How long the pointer has to rest before the tip appears, in milliseconds.
///
/// Word's is about half a second: long enough that sweeping the pointer across
/// the ribbon shows nothing, short enough that stopping on a button answers.
pub const DELAY_MILLIS: u64 = 500;

const HEIGHT: f32 = 24.0;
const PADDING: f32 = 8.0;

/// A tip, and where it hangs.
#[derive(Clone, Debug)]
pub struct Tip {
    /// The button it belongs to, so it can be taken away when the pointer moves
    /// on to another one.
    pub command: Command,
    text: String,
    left: f32,
    top: f32,
    width: f32,
}

impl Tip {
    /// Makes one for a button, under it and inside the window.
    ///
    /// `button` is where the button ends: its left edge, its bottom, and how
    /// wide it is — which is what the ribbon can say about any of its items.
    #[must_use]
    pub fn new(
        command: Command,
        label: &str,
        button: (f32, f32, f32),
        engine: &mut LayoutEngine<'_>,
        window_width: f32,
    ) -> Self {
        let text = match shortcut_of(command) {
            Some(keys) => format!("{label}  ({keys})"),
            None => label.to_owned(),
        };
        let measured = engine.simple_line(&text, 0.0, 0.0, 8.5, wp_raster::Color::BLACK);
        let width = measured.width + PADDING * 2.0;

        let (button_left, button_bottom, _) = button;
        let left = button_left.min(window_width - width - 4.0).max(4.0);
        Self { command, text, left, top: button_bottom + 4.0, width }
    }

    pub fn draw(
        &self,
        canvas: &mut Canvas,
        engine: &mut LayoutEngine<'_>,
        renderer: &mut Renderer<'_>,
        theme: &Theme,
    ) {
        canvas.fill_rect(
            self.left as i32 - 1,
            self.top as i32 - 1,
            self.width as i32 + 2,
            HEIGHT as i32 + 2,
            theme.field_edge,
        );
        canvas.fill_rect(
            self.left as i32,
            self.top as i32,
            self.width as i32,
            HEIGHT as i32,
            theme.pane,
        );
        let line =
            engine.simple_line(&self.text, self.left + PADDING, self.top + 16.0, 8.5, theme.text);
        renderer.draw_onto(canvas, &line, 0.0, 0.0);
    }
}

/// What a button is called.
///
/// Most of the ribbon carries its name beside its drawing, and that name is
/// what the tip says. The buttons that are a drawing and nothing else have
/// their names here, because there is nowhere else in the program they are
/// written down — and those are exactly the ones somebody needs told.
#[must_use]
pub fn label_of(command: Command) -> Option<&'static str> {
    let named = match command {
        Command::Align(Alignment::Start) => "Align Left",
        Command::Align(Alignment::Center) => "Center",
        Command::Align(Alignment::End) => "Align Right",
        Command::Align(Alignment::Both) => "Justify",
        Command::Borders => "Borders",
        Command::Bullets => "Bullets",
        Command::ChangeCase => "Change Case",
        Command::ClearFormatting => "Clear All Formatting",
        Command::GrowFont => "Increase Font Size",
        Command::Highlight => "Text Highlight Color",
        Command::IndentLess => "Decrease Indent",
        Command::IndentMore => "Increase Indent",
        Command::LineSpacing => "Line and Paragraph Spacing",
        Command::DocumentSpacing => "Paragraph Spacing for the whole document",
        Command::MultilevelList => "Multilevel List",
        Command::Numbering => "Numbering",
        Command::Shading => "Shading",
        Command::ShowMarks => "Show/Hide Formatting Marks",
        Command::ShrinkFont => "Decrease Font Size",
        Command::Sort => "Sort",
        Command::Subscript => "Subscript",
        Command::Superscript => "Superscript",
        Command::TextColor => "Font Color",
        Command::TextEffects => "Text Effects and Typography",
        Command::Format(CharacterFormat::Bold) => "Bold",
        Command::Format(CharacterFormat::Italic) => "Italic",
        Command::Format(CharacterFormat::Underline) => "Underline",
        Command::Format(CharacterFormat::Strikethrough) => "Strikethrough",
        // Everything else says what it is on its own face.
        _ => return super::ribbon::name_of(command),
    };
    Some(named)
}

/// The keys that do the same thing as a button, where there are any.
///
/// Only the ones this program actually listens for: a tip that named a
/// shortcut nothing answered to would be a lie printed in a box.
#[must_use]
pub fn shortcut_of(command: Command) -> Option<&'static str> {
    Some(match command {
        Command::New => "Ctrl+N",
        Command::Open => "Ctrl+O",
        Command::Save => "Ctrl+S",
        Command::SaveAs => "Ctrl+Shift+S",
        Command::Print => "Ctrl+P",
        Command::Undo => "Ctrl+Z",
        Command::Redo => "Ctrl+Y",
        Command::Cut => "Ctrl+X",
        Command::Copy => "Ctrl+C",
        Command::Paste => "Ctrl+V",
        Command::SelectAll => "Ctrl+A",
        Command::Find => "Ctrl+F",
        Command::Replace => "Ctrl+H",
        Command::InsertLink => "Ctrl+K",
        Command::Format(CharacterFormat::Bold) => "Ctrl+B",
        Command::Format(CharacterFormat::Italic) => "Ctrl+I",
        Command::Format(CharacterFormat::Underline) => "Ctrl+U",
        Command::Align(Alignment::Start) => "Ctrl+L",
        Command::Align(Alignment::Center) => "Ctrl+E",
        Command::Align(Alignment::End) => "Ctrl+R",
        Command::Align(Alignment::Both) => "Ctrl+J",
        Command::IndentMore => "Ctrl+M",
        Command::IndentLess => "Ctrl+Shift+M",
        Command::Bullets => "Ctrl+Shift+8",
        Command::PageBreak => "Ctrl+Enter",
        Command::ZoomIn => "Ctrl+=",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shortcuts_named_are_the_ones_that_work() {
        assert_eq!(shortcut_of(Command::Save), Some("Ctrl+S"));
        assert_eq!(shortcut_of(Command::Format(CharacterFormat::Bold)), Some("Ctrl+B"));
    }

    #[test]
    fn a_button_with_no_shortcut_claims_none() {
        assert_eq!(shortcut_of(Command::Watermark), None);
    }
}
