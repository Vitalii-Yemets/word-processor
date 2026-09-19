//! Ruby laid out: a word with its reading set over it.
//!
//! # What has to be decided
//!
//! Two pieces of text of different lengths, one above the other, and which of
//! them is wider is not known until both are measured. The reading is usually
//! the wider — three kana over two characters — so the word underneath has to
//! be given the reading's room, and the two lined up inside it.
//!
//! How they are lined up is the file's to say: centred, spread so that the two
//! ends meet, or against one end. The spreading is what Japanese typesetting
//! does by default and is why a ruby of one character over one character still
//! looks even.
//!
//! # Why the line has to be told
//!
//! Because the reading is above the line, not in it. A line of ordinary text
//! reaches as high as its letters; a line with a ruby in it reaches as high as
//! the reading, and a line that does not know that draws the reading over the
//! words of the line above.

use crate::layout::PositionedGlyph;

/// A ruby laid out: everything in it, placed against a baseline of its own.
#[derive(Clone, Debug, Default)]
pub struct RubyBox {
    pub width: f32,
    /// How far it reaches above the baseline, the reading included.
    pub ascent: f32,
    /// And below it, which is the word's own descent.
    pub descent: f32,
    pub glyphs: Vec<PositionedGlyph>,
}

impl RubyBox {
    /// The glyphs of it, moved to where the line puts them.
    #[must_use]
    pub fn placed(&self, x: f32, baseline: f32) -> Vec<PositionedGlyph> {
        self.glyphs
            .iter()
            .map(|glyph| PositionedGlyph {
                x: glyph.x + x,
                baseline: glyph.baseline + baseline,
                ..*glyph
            })
            .collect()
    }

    /// Nothing at all: a ruby whose word could not be laid out.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
    }
}

/// One half of a ruby, already shaped: its glyphs and how wide they are.
#[derive(Clone, Debug, Default)]
pub struct Half {
    pub glyphs: Vec<PositionedGlyph>,
    pub width: f32,
    pub ascent: f32,
    pub descent: f32,
}

/// Puts the reading over the word.
///
/// Both halves arrive shaped and measured, each starting at nothing; this
/// decides where each sits and how much room the pair takes.
#[must_use]
pub fn lay_out(word: Half, reading: Half, align: wp_docx::ruby::Align, raise: f32) -> RubyBox {
    let width = word.width.max(reading.width);
    let mut glyphs = Vec::with_capacity(word.glyphs.len() + reading.glyphs.len());

    // The word, in the room the pair takes.
    let (word_at, word_gap) = places(word.width, width, align, word.glyphs.len());
    glyphs.extend(spread(&word.glyphs, word_at, word_gap));

    // And the reading above it, lifted clear of the word's own letters.
    let (reading_at, reading_gap) = places(reading.width, width, align, reading.glyphs.len());
    for glyph in spread(&reading.glyphs, reading_at, reading_gap) {
        glyphs.push(PositionedGlyph { baseline: glyph.baseline - raise, ..glyph });
    }

    RubyBox {
        width,
        // The reading is lifted by the raise and reaches its own height above
        // that: a line with a ruby in it is as tall as the reading, not as
        // tall as the word.
        ascent: word.ascent.max(raise + reading.ascent),
        descent: word.descent,
        glyphs,
    }
}

/// Where one half starts, and how much room to put between its letters.
///
/// The room left over is what makes the difference between the alignments:
/// all of it before, all of it after, half at each end, or shared out between
/// the letters.
fn places(own: f32, room: f32, align: wp_docx::ruby::Align, count: usize) -> (f32, f32) {
    use wp_docx::ruby::Align;

    let spare = (room - own).max(0.0);
    if spare <= 0.0 || count == 0 {
        return (0.0, 0.0);
    }

    match align {
        Align::Left => (0.0, 0.0),
        Align::Right => (spare, 0.0),
        Align::Center => (spare / 2.0, 0.0),
        // Spread between the letters, the two ends meeting the ends of the
        // room: one letter has nothing to spread between and is centred.
        Align::DistributeLetter => {
            if count < 2 {
                (spare / 2.0, 0.0)
            } else {
                (0.0, spare / (count - 1) as f32)
            }
        }
        // The same with a gap at each end as well, which is Word's default and
        // what keeps a reading of one kana from sitting hard against the edge.
        Align::DistributeSpace => {
            let gap = spare / (count + 1) as f32;
            (gap, gap)
        }
    }
}

/// Moves a half's glyphs to where they belong, opening the gaps between them.
fn spread(glyphs: &[PositionedGlyph], at: f32, gap: f32) -> Vec<PositionedGlyph> {
    let mut out = Vec::with_capacity(glyphs.len());
    let mut extra = 0.0f32;
    for (index, glyph) in glyphs.iter().enumerate() {
        out.push(PositionedGlyph { x: glyph.x + at + extra, ..*glyph });
        if index + 1 < glyphs.len() {
            extra += gap;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::ruby::Align;
    use wp_docx::TextPosition;
    use wp_font::GlyphId;
    use wp_raster::Color;

    /// A half of so many glyphs, each this wide.
    fn half(count: usize, advance: f32) -> Half {
        let glyphs = (0..count)
            .map(|index| PositionedGlyph {
                face: 0,
                glyph: GlyphId(1),
                x: index as f32 * advance,
                baseline: 0.0,
                advance,
                size: 12.0,
                stretch: 1.0,
                color: Color::BLACK,
                effect: None,
                source: TextPosition::new(0, 0),
                source_length: 1,
                invisible: false,
                shift_x: 0.0,
                shift_y: 0.0,
            })
            .collect();
        Half { glyphs, width: count as f32 * advance, ascent: 9.0, descent: 3.0 }
    }

    /// Where each glyph of a laid-out ruby sits, in order.
    fn places_of(laid: &RubyBox) -> Vec<f32> {
        laid.glyphs.iter().map(|glyph| glyph.x).collect()
    }

    #[test]
    fn the_pair_is_as_wide_as_the_wider_of_them() {
        // The reading is usually the wider: three kana over two characters.
        let laid = lay_out(half(2, 10.0), half(3, 6.0), Align::Center, 8.0);
        assert!((laid.width - 20.0).abs() < 0.01);

        let other = lay_out(half(2, 6.0), half(3, 10.0), Align::Center, 8.0);
        assert!((other.width - 30.0).abs() < 0.01);
    }

    #[test]
    fn the_reading_is_lifted_clear_of_the_word() {
        let laid = lay_out(half(2, 10.0), half(2, 5.0), Align::Center, 8.0);
        let baselines: Vec<f32> = laid.glyphs.iter().map(|glyph| glyph.baseline).collect();
        assert!(baselines.iter().any(|baseline| *baseline < -7.9), "{baselines:?}");
        assert!(baselines.iter().any(|baseline| baseline.abs() < 0.01), "the word moved");
    }

    #[test]
    fn a_line_with_a_ruby_in_it_is_as_tall_as_the_reading() {
        // Which is the whole reason the line is told about it: a line as tall
        // as its letters draws the reading over the line above.
        let laid = lay_out(half(2, 10.0), half(2, 5.0), Align::Center, 8.0);
        assert!(laid.ascent > 9.0, "the ruby claimed to be no taller than the word");
        assert!((laid.descent - 3.0).abs() < 0.01, "the reading reached below the line");
    }

    #[test]
    fn the_narrower_half_is_centred_when_that_is_what_the_file_says() {
        let laid = lay_out(half(2, 10.0), half(1, 6.0), Align::Center, 8.0);
        // The word fills the room; the one-letter reading sits in the middle.
        let reading = places_of(&laid)[2];
        assert!((reading - 7.0).abs() < 0.01, "{reading}");
    }

    #[test]
    fn against_one_end_is_against_that_end() {
        let left = lay_out(half(2, 10.0), half(1, 6.0), Align::Left, 8.0);
        assert!((places_of(&left)[2] - 0.0).abs() < 0.01);

        let right = lay_out(half(2, 10.0), half(1, 6.0), Align::Right, 8.0);
        assert!((places_of(&right)[2] - 14.0).abs() < 0.01);
    }

    #[test]
    fn spreading_puts_the_room_between_the_letters() {
        // Two letters in twenty points of room, each six wide: eight to share,
        // and by letter it all goes in the middle.
        let laid = lay_out(half(2, 10.0), half(2, 6.0), Align::DistributeLetter, 8.0);
        let reading = &places_of(&laid)[2..];
        assert!((reading[0] - 0.0).abs() < 0.01, "{reading:?}");
        assert!((reading[1] - 14.0).abs() < 0.01, "{reading:?}");
    }

    #[test]
    fn spreading_with_spaces_leaves_a_gap_at_each_end() {
        // The same, Word's way: the room is shared three ways rather than one,
        // so the reading does not touch either end.
        let laid = lay_out(half(2, 10.0), half(2, 6.0), Align::DistributeSpace, 8.0);
        let reading = &places_of(&laid)[2..];
        let gap = 8.0 / 3.0;
        assert!((reading[0] - gap).abs() < 0.01, "{reading:?}");
        assert!((reading[1] - (6.0 + 2.0 * gap)).abs() < 0.01, "{reading:?}");
    }

    #[test]
    fn a_reading_wider_than_its_word_spreads_the_word_instead() {
        // It works both ways round: whichever is narrower is the one spread.
        let laid = lay_out(half(2, 5.0), half(3, 10.0), Align::DistributeSpace, 8.0);
        let word = &places_of(&laid)[..2];
        assert!(word[0] > 0.01, "the word was left against the edge: {word:?}");
    }
}
