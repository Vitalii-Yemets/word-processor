//! Footnotes: small lines at the foot of a page, each starting with the
//! number of a raised reference in the text above.
//!
//! The lines in a smaller size than the text, under all of it and apart
//! from it, are a page's notes when the first starts with a number; each
//! line starting with a number starts a note, and the lines after it carry
//! it on. A note becomes a footnote when the text above has a raised
//! number the same, which becomes the note's reference; a note whose
//! reference cannot be found stays in the text, where it was.

use wp_docx::model::{Block, Body, Paragraph, ParagraphProperties, VerticalAlignment};

use super::super::content::Rectangle;
use super::{runs_of, Found, Line};

/// Takes a page's footnotes out of its lines and into `found`.
pub(super) fn footnotes_of(
    lines: &mut Vec<Line>,
    rectangles: &[Rectangle],
    body_size: f64,
    found: &mut Found,
) {
    lines.sort_by(|a, b| b.y.partial_cmp(&a.y).unwrap_or(std::cmp::Ordering::Equal));
    // The notes are under the short rule that separates them from the
    // text, when there is one: a third of the column or so, at its left.
    let text_left = lines.iter().map(|l| l.x0).fold(f64::INFINITY, f64::min);
    let separator = rectangles
        .iter()
        .filter(|r| {
            r.height() <= 2.0
                && r.width() >= 20.0
                && r.width() <= 250.0
                && (r.x0 - text_left).abs() < 12.0
        })
        .map(|r| (r.y0 + r.y1) / 2.0)
        .filter(|&y| lines.iter().any(|l| l.y < y) && lines.iter().any(|l| l.y > y + l.size))
        .fold(f64::NEG_INFINITY, f64::max);
    let mut start = lines.len();
    if separator.is_finite() {
        start = lines.iter().position(|l| l.y < separator).unwrap_or(lines.len());
    } else {
        // Or else they are the smaller lines at the foot: ten points to
        // the text's eleven, as Word sets them.
        while start > 0 && lines[start - 1].size < body_size * 0.95 {
            start -= 1;
        }
        // Or else the last lines at the foot, set apart from the text: a
        // footnote only if they start with a number the text has raised.
        if start == lines.len() && start > 0 {
            start -= 1;
            while start > 0 && lines[start - 1].y - lines[start].y <= 1.3 * lines[start].size {
                start -= 1;
            }
        }
    }
    if start == 0 || start == lines.len() {
        return;
    }
    // Apart from the text above: more than a line's pitch below it.
    if lines[start - 1].y - lines[start].y < 1.3 * lines[start - 1].size {
        return;
    }
    let mut notes: Vec<(u32, Vec<usize>, Vec<Line>)> = Vec::new();
    for (index, line) in lines.iter().enumerate().skip(start) {
        match leading_number(line) {
            Some((number, rest)) => notes.push((number, vec![index], vec![rest])),
            None => match notes.last_mut() {
                Some((_, indices, note)) => {
                    indices.push(index);
                    note.push(line.clone());
                }
                None => return,
            },
        }
    }
    let mut taken = Vec::new();
    for (number, indices, note_lines) in notes {
        let id = found.notes.len() as i32 + 1;
        if !mark_reference(&mut lines[..start], number, id) {
            continue;
        }
        let mut scratch = Found::default();
        let runs = runs_of(&note_lines, &mut scratch, 0);
        let paragraph = Paragraph { properties: ParagraphProperties::default(), runs };
        found.notes.push((id, Body { blocks: vec![Block::Paragraph(paragraph)] }));
        taken.extend(indices);
    }
    taken.sort_unstable();
    for index in taken.into_iter().rev() {
        lines.remove(index);
    }
}

/// The number a note's line starts with, and the line without it: one to
/// three digits, raised, or followed by a space, a full stop or a bracket.
fn leading_number(line: &Line) -> Option<(u32, Line)> {
    let pieces = &line.pieces;
    let mut at = 0;
    let mut digits = String::new();
    while let Some(piece) = pieces.get(at) {
        if piece.text.is_empty() || !piece.text.chars().all(|c| c.is_ascii_digit()) {
            break;
        }
        digits.push_str(&piece.text);
        at += 1;
    }
    if digits.is_empty() || digits.len() > 3 {
        return None;
    }
    let raised = pieces[0].format.vertical == VerticalAlignment::Superscript;
    let separated =
        |piece: &super::Piece| piece.is_space() || piece.text == "." || piece.text == ")";
    if !raised && !pieces.get(at).is_some_and(separated) {
        return None;
    }
    while pieces.get(at).is_some_and(separated) {
        at += 1;
    }
    let rest = pieces.get(at..).filter(|rest| !rest.is_empty())?.to_vec();
    let x0 = rest[0].x0;
    Some((digits.parse().ok()?, Line { x0, pieces: rest, ..line.clone() }))
}

/// Turns the first raised `number` after a word in the lines into the
/// reference of note `id`.
fn mark_reference(lines: &mut [Line], number: u32, id: i32) -> bool {
    let wanted = number.to_string();
    for line in lines.iter_mut() {
        let mut index = 1;
        while index < line.pieces.len() {
            let raised = |piece: &super::Piece| {
                piece.format.vertical == VerticalAlignment::Superscript && piece.note.is_none()
            };
            if !raised(&line.pieces[index]) || raised(&line.pieces[index - 1]) {
                index += 1;
                continue;
            }
            let mut end = index;
            let mut text = String::new();
            while end < line.pieces.len() && raised(&line.pieces[end]) {
                text.push_str(&line.pieces[end].text);
                end += 1;
            }
            if text.trim() == wanted && !line.pieces[index - 1].is_space() {
                line.pieces[index].text.clear();
                line.pieces[index].note = Some(id);
                line.pieces.drain(index + 1..end);
                return true;
            }
            index = end;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::super::{Format, Piece};
    use super::*;

    /// A line of text, a piece a character, those at `raised` raised.
    fn line(text: &str, y: f64, size: f64, raised: &[usize]) -> Line {
        let mut pieces = Vec::new();
        let mut x = 72.0;
        for (index, c) in text.chars().enumerate() {
            let vertical = if raised.contains(&index) {
                VerticalAlignment::Superscript
            } else {
                VerticalAlignment::Baseline
            };
            pieces.push(Piece {
                x0: x,
                x1: x + size * 0.5,
                y,
                size,
                text: c.to_string(),
                format: Format {
                    family: "Arial".into(),
                    bold: false,
                    italic: false,
                    half_points: (size * 2.0) as u32,
                    colour: [0, 0, 0],
                    underline: false,
                    strike: false,
                    vertical,
                },
                link: None,
                picture: None,
                note: None,
            });
            x += size * 0.5;
        }
        Line { y, x0: 72.0, x1: x, size, pieces, space_widths: Vec::new() }
    }

    #[test]
    fn a_smaller_numbered_line_at_the_foot_is_the_note_a_raised_number_refers_to() {
        let mut lines = vec![
            line("Text with a mark2 in it", 700.0, 11.0, &[16]),
            line("and more text after it.", 686.0, 11.0, &[]),
            line("2 The note itself,", 90.0, 9.0, &[]),
            line("carried on.", 80.0, 9.0, &[]),
        ];
        let mut found = Found::default();
        footnotes_of(&mut lines, &[], 11.0, &mut found);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text(), "Text with a mark in it");
        assert!(lines[0].pieces.iter().any(|piece| piece.note == Some(1)));
        assert_eq!(found.notes.len(), 1);
        let text: String = found.notes[0].1.blocks.iter().map(Block::plain_text).collect();
        assert_eq!(text, "The note itself, carried on.");
    }

    #[test]
    fn a_numbered_line_with_nothing_raised_to_match_stays_in_the_text() {
        let mut lines = vec![
            line("Text with nothing raised", 700.0, 11.0, &[]),
            line("3 A line that starts with three.", 90.0, 9.0, &[]),
        ];
        let mut found = Found::default();
        footnotes_of(&mut lines, &[], 11.0, &mut found);
        assert_eq!(lines.len(), 2);
        assert!(found.notes.is_empty());
    }
}
