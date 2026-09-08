//! The letters that appear over the ribbon when Alt is pressed.
//!
//! # What Word does
//!
//! Press and release Alt and a small badge appears over every tab and over the
//! buttons above the ribbon, each with a letter on it. Press that letter and
//! the tab opens with badges over everything in it; press one of those and the
//! command runs. Escape goes back a step, and Alt puts them all away.
//!
//! It is the whole ribbon reachable without a mouse, and it is the reason a
//! person who has learnt Word can drive it with their hands on the keys.
//!
//! # How the letters are picked
//!
//! Word's are fixed, chosen once for each command and printed in its
//! documentation. These are worked out from the labels instead: the first
//! letter that nothing else has taken, then a later letter of the same word,
//! then a digit. That gives every command a letter, keeps the obvious ones
//! obvious — F for Font, B for Bold — and needs no table to be kept in step
//! with the ribbon.

use wp_layout::{LayoutEngine, Renderer};
use wp_raster::Canvas;

use super::theme::Theme;

/// How big a badge is.
const SIZE: f32 = 16.0;

/// Picks a letter for each label, all of them different.
///
/// Upper case, because that is how a badge shows it and how somebody reading
/// one thinks of it. A label that can be given nothing at all — every letter
/// taken, every digit gone — comes back empty, and nothing is drawn for it.
#[must_use]
pub fn assign(labels: &[&str]) -> Vec<String> {
    let mut taken: Vec<char> = Vec::new();
    let mut out = Vec::with_capacity(labels.len());

    for label in labels {
        // The first letter of each word first, which is where a person looks:
        // "Format Painter" wants F, and failing that P.
        let first_letters = label
            .split_whitespace()
            .filter_map(|word| word.chars().next())
            .filter(|letter| letter.is_ascii_alphanumeric());
        let rest = label.chars().filter(|letter| letter.is_ascii_alphanumeric());
        let digits = "123456789".chars();

        let found = first_letters
            .chain(rest)
            .chain(digits)
            .map(|letter| letter.to_ascii_uppercase())
            .find(|letter| !taken.contains(letter));

        match found {
            Some(letter) => {
                taken.push(letter);
                out.push(letter.to_string());
            }
            None => out.push(String::new()),
        }
    }
    out
}

/// Draws one badge with its top-left corner at a point.
pub fn draw(
    canvas: &mut Canvas,
    engine: &mut LayoutEngine<'_>,
    renderer: &mut Renderer<'_>,
    letter: &str,
    x: f32,
    y: f32,
    theme: &Theme,
) {
    if letter.is_empty() {
        return;
    }
    let measured = engine.simple_line(letter, 0.0, 0.0, 8.0, theme.text);
    let width = (measured.width + 10.0).max(SIZE);

    canvas.fill_rect(
        x as i32 - 1,
        y as i32 - 1,
        width as i32 + 2,
        SIZE as i32 + 2,
        theme.field_edge,
    );
    canvas.fill_rect(x as i32, y as i32, width as i32, SIZE as i32, theme.pane);
    let line = engine.simple_line(
        letter,
        x + (width - measured.width) / 2.0,
        y + SIZE - 4.0,
        8.0,
        theme.text,
    );
    renderer.draw_onto(canvas, &line, 0.0, 0.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_label_gets_its_own_first_letter() {
        assert_eq!(assign(&["Home", "Insert", "View"]), ["H", "I", "V"]);
    }

    #[test]
    fn two_labels_that_start_alike_get_different_letters() {
        let given = assign(&["Font", "Format Painter"]);
        assert_eq!(given[0], "F");
        assert_ne!(given[1], "F", "two commands cannot share a letter");
        assert!(!given[1].is_empty());
    }

    #[test]
    fn the_second_word_is_tried_before_the_middle_of_the_first() {
        // "Format Painter" after "Font" and "Find" have taken F and I: P is the
        // letter somebody would guess, not O.
        assert_eq!(assign(&["Font", "Find", "Format Painter"])[2], "P");
    }

    #[test]
    fn a_long_list_of_different_labels_all_get_something() {
        let many = [
            "Bold",
            "Italic",
            "Underline",
            "Strikethrough",
            "Subscript",
            "Superscript",
            "Highlight",
            "Font Color",
            "Bullets",
            "Numbering",
            "Sort",
            "Borders",
            "Shading",
            "Align Left",
            "Center",
            "Justify",
        ];
        let given = assign(&many);
        assert!(given.iter().all(|letter| !letter.is_empty()), "{given:?}");
    }

    #[test]
    fn a_label_with_nothing_left_to_give_it_gets_nothing() {
        // Twenty copies of the same two characters: A, then the digits, and
        // then there is genuinely nothing left. Better an empty badge than a
        // letter that belongs to something else.
        let many: Vec<&str> = vec!["Aa"; 20];
        let given = assign(&many);
        assert_eq!(given[0], "A");
        assert!(given.last().expect("a last one").is_empty(), "{given:?}");
    }

    #[test]
    fn no_two_labels_share_a_letter() {
        let labels = ["Cut", "Copy", "Clipboard", "Cover", "Colour", "Chart"];
        let given = assign(&labels);
        let mut seen = given.clone();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), labels.len(), "{given:?}");
    }
}
