//! One code-block's coefficients out of its coded passes.
//!
//! [T.800] Annex D. A block's coefficients are coded a bit-plane at a time,
//! most significant first, and each plane in up to three passes: the
//! coefficients next to ones already significant, then the bits of those
//! already significant, then the rest. Every bit is coded in a context
//! made of which of its eight neighbours are significant, so the coder
//! learns how likely a coefficient is to matter from what is around it.
//! The block's style may code some passes raw, reset the contexts after
//! each pass, end the coding after each pass, keep the context from
//! looking below a stripe, or add a check symbol after each clean-up.

use super::super::mq::{Context, Decoder};

pub const BYPASS: u8 = 0x01;
pub const RESET: u8 = 0x02;
pub const TERMINATE_EACH: u8 = 0x04;
pub const VERTICALLY_CAUSAL: u8 = 0x08;
pub const SEGMENTATION: u8 = 0x20;

/// Which band a block is in, which decides how its neighbours count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    LowLow,
    HighLow,
    LowHigh,
    HighHigh,
}

/// A run of passes coded together, and their bytes.
#[derive(Clone, Debug, Default)]
pub struct Segment {
    pub data: Vec<u8>,
    pub passes: usize,
    /// The most passes the segment may hold, by where it starts.
    pub most: usize,
}

/// The most passes a segment starting at pass `first` may hold.
#[must_use]
pub fn segment_room(style: u8, first: usize) -> usize {
    if style & TERMINATE_EACH != 0 {
        1
    } else if style & BYPASS != 0 {
        if first < 10 {
            10 - first
        } else if (first - 10) % 3 == 0 {
            // A significance pass and the refinement after it, both raw.
            2
        } else {
            1
        }
    } else {
        usize::MAX
    }
}

const SIGNIFICANT: u8 = 1;
const VISITED: u8 = 2;
const REFINED: u8 = 4;
const NEGATIVE: u8 = 8;

const RUN: usize = 17;
const UNIFORM: usize = 18;

fn fresh_contexts() -> [Context; 19] {
    let mut contexts = [Context::default(); 19];
    contexts[0] = Context::at(4);
    contexts[RUN] = Context::at(3);
    contexts[UNIFORM] = Context::at(46);
    contexts
}

/// Bits coded raw, for the passes the bypass style leaves uncoded: most
/// significant first, a nought stuffed after every 0xFF.
struct Raw<'a> {
    data: &'a [u8],
    at: usize,
    byte: u8,
    left: u32,
}

impl Raw<'_> {
    fn bit(&mut self) -> u8 {
        if self.left == 0 {
            let previous = self.byte;
            self.byte = self.data.get(self.at).copied().unwrap_or(0xFF);
            self.at += 1;
            self.left = if previous == 0xFF { 7 } else { 8 };
        }
        self.left -= 1;
        (self.byte >> self.left) & 1
    }
}

enum Coder<'a> {
    Arithmetic(Decoder<'a>),
    Raw(Raw<'a>),
}

/// The block being decoded: its coefficients' states with a border of one
/// all round, so every coefficient has eight neighbours to look at.
struct Block {
    width: usize,
    height: usize,
    stride: usize,
    flags: Vec<u8>,
    magnitudes: Vec<u32>,
    /// The plane each coefficient was last coded in.
    coded: Vec<u8>,
    orientation: Orientation,
    causal: bool,
}

impl Block {
    fn index(&self, x: usize, y: usize) -> usize {
        (y + 1) * self.stride + x + 1
    }

    fn significant(&self, index: usize) -> u32 {
        u32::from(self.flags[index] & SIGNIFICANT)
    }

    /// How many neighbours are significant, across, up and down, and on
    /// the diagonals; below a stripe's last row counts for nothing when
    /// the style keeps to the stripe.
    fn neighbours(&self, index: usize, y: usize) -> (u32, u32, u32) {
        let below = !(self.causal && y % 4 == 3);
        let s = self.stride;
        let across = self.significant(index - 1) + self.significant(index + 1);
        let mut up_down = self.significant(index - s);
        let mut diagonal = self.significant(index - s - 1) + self.significant(index - s + 1);
        if below {
            up_down += self.significant(index + s);
            diagonal += self.significant(index + s - 1) + self.significant(index + s + 1);
        }
        (across, up_down, diagonal)
    }

    /// The context a still-insignificant coefficient is coded in.
    fn zero_context(&self, index: usize, y: usize) -> usize {
        let (h, v, d) = self.neighbours(index, y);
        match self.orientation {
            Orientation::LowLow | Orientation::LowHigh => primary(h, v, d),
            Orientation::HighLow => primary(v, h, d),
            Orientation::HighHigh => {
                let hv = h + v;
                match d {
                    0 => hv.min(2) as usize,
                    1 => 3 + hv.min(2) as usize,
                    2 => {
                        if hv == 0 {
                            6
                        } else {
                            7
                        }
                    }
                    _ => 8,
                }
            }
        }
    }

    /// The context a sign is coded in, and whether the bit is flipped.
    fn sign_context(&self, index: usize, y: usize) -> (usize, u8) {
        let below = !(self.causal && y % 4 == 3);
        let s = self.stride;
        let part = |at: usize| -> i32 {
            let flags = self.flags[at];
            if flags & SIGNIFICANT == 0 {
                0
            } else if flags & NEGATIVE != 0 {
                -1
            } else {
                1
            }
        };
        let h = (part(index - 1) + part(index + 1)).clamp(-1, 1);
        let v = (part(index - s) + if below { part(index + s) } else { 0 }).clamp(-1, 1);
        match (h, v) {
            (1, 1) => (13, 0),
            (1, 0) => (12, 0),
            (1, -1) => (11, 0),
            (0, 1) => (10, 0),
            (0, 0) => (9, 0),
            (0, -1) => (10, 1),
            (-1, 1) => (11, 1),
            (-1, 0) => (12, 1),
            _ => (13, 1),
        }
    }
}

/// The zero-coding label where `h` is the neighbours that count most.
fn primary(h: u32, v: u32, d: u32) -> usize {
    match h {
        0 => match v {
            0 => d.min(2) as usize,
            1 => 3,
            _ => 4,
        },
        1 => {
            if v > 0 {
                7
            } else if d > 0 {
                6
            } else {
                5
            }
        }
        _ => 8,
    }
}

/// The pass kinds, in the order a plane's passes come.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pass {
    Significance,
    Refinement,
    Cleanup,
}

fn pass_kind(index: usize) -> Pass {
    if index == 0 {
        Pass::Cleanup
    } else {
        [Pass::Significance, Pass::Refinement, Pass::Cleanup][(index - 1) % 3]
    }
}

/// Decodes a block of `width` by `height` whose top `planes` bit-planes
/// are coded in `segments`. Gives the coefficients, sign and magnitude,
/// with bit nought the least bit a plane could give, and for each the
/// lowest plane its value was coded in.
#[must_use]
pub fn decode(
    width: usize,
    height: usize,
    orientation: Orientation,
    planes: u32,
    segments: &[Segment],
    style: u8,
) -> (Vec<i64>, Vec<u8>) {
    let stride = width + 2;
    let mut block = Block {
        width,
        height,
        stride,
        flags: vec![0; stride * (height + 2)],
        magnitudes: vec![0; stride * (height + 2)],
        coded: vec![0; stride * (height + 2)],
        orientation,
        causal: style & VERTICALLY_CAUSAL != 0,
    };
    let mut contexts = fresh_contexts();
    let mut pass = 0usize;
    'segments: for segment in segments {
        let mut coder: Option<Coder<'_>> = None;
        for _ in 0..segment.passes {
            let plane = pass.div_ceil(3);
            if plane as u32 >= planes || planes > 31 {
                break 'segments;
            }
            let bit_position = planes - 1 - plane as u32;
            let kind = pass_kind(pass);
            let raw = style & BYPASS != 0 && pass >= 10 && kind != Pass::Cleanup;
            let coder = coder.get_or_insert_with(|| {
                if raw {
                    Coder::Raw(Raw { data: &segment.data, at: 0, byte: 0, left: 0 })
                } else {
                    Coder::Arithmetic(Decoder::new(&segment.data, 0, segment.data.len()))
                }
            });
            let bit = 1u32 << bit_position;
            match kind {
                Pass::Significance => significance(&mut block, coder, &mut contexts, bit),
                Pass::Refinement => refinement(&mut block, coder, &mut contexts, bit),
                Pass::Cleanup => {
                    cleanup(&mut block, coder, &mut contexts, bit);
                    if style & SEGMENTATION != 0 {
                        if let Coder::Arithmetic(decoder) = coder {
                            for _ in 0..4 {
                                decoder.bit(&mut contexts[UNIFORM]);
                            }
                        }
                    }
                    for flags in &mut block.flags {
                        *flags &= !VISITED;
                    }
                }
            }
            if style & RESET != 0 {
                contexts = fresh_contexts();
            }
            pass += 1;
        }
    }
    let mut out = Vec::with_capacity(width * height);
    let mut coded = Vec::with_capacity(width * height);
    for y in 0..height {
        for x in 0..width {
            let index = block.index(x, y);
            let magnitude = i64::from(block.magnitudes[index]);
            out.push(if block.flags[index] & NEGATIVE != 0 { -magnitude } else { magnitude });
            coded.push(block.coded[index]);
        }
    }
    (out, coded)
}

fn decide(coder: &mut Coder<'_>, contexts: &mut [Context; 19], context: usize) -> u8 {
    match coder {
        Coder::Arithmetic(decoder) => decoder.bit(&mut contexts[context]),
        Coder::Raw(raw) => raw.bit(),
    }
}

/// A coefficient found significant: its sign follows.
fn become_significant(
    block: &mut Block,
    coder: &mut Coder<'_>,
    contexts: &mut [Context; 19],
    index: usize,
    y: usize,
    bit: u32,
) {
    let negative = match coder {
        Coder::Arithmetic(decoder) => {
            let (context, flip) = block.sign_context(index, y);
            decoder.bit(&mut contexts[context]) ^ flip
        }
        Coder::Raw(raw) => raw.bit(),
    };
    block.flags[index] |= SIGNIFICANT | if negative == 1 { NEGATIVE } else { 0 };
    block.magnitudes[index] |= bit;
    block.coded[index] = bit.trailing_zeros() as u8;
}

fn significance(block: &mut Block, coder: &mut Coder<'_>, contexts: &mut [Context; 19], bit: u32) {
    for stripe in (0..block.height).step_by(4) {
        for x in 0..block.width {
            for y in stripe..(stripe + 4).min(block.height) {
                let index = block.index(x, y);
                if block.flags[index] & SIGNIFICANT != 0 {
                    continue;
                }
                let (h, v, d) = block.neighbours(index, y);
                if h + v + d == 0 {
                    continue;
                }
                let context = block.zero_context(index, y);
                if decide(coder, contexts, context) == 1 {
                    become_significant(block, coder, contexts, index, y, bit);
                }
                block.flags[index] |= VISITED;
            }
        }
    }
}

fn refinement(block: &mut Block, coder: &mut Coder<'_>, contexts: &mut [Context; 19], bit: u32) {
    for stripe in (0..block.height).step_by(4) {
        for x in 0..block.width {
            for y in stripe..(stripe + 4).min(block.height) {
                let index = block.index(x, y);
                let flags = block.flags[index];
                if flags & SIGNIFICANT == 0 || flags & VISITED != 0 {
                    continue;
                }
                let context = if flags & REFINED != 0 {
                    16
                } else {
                    let (h, v, d) = block.neighbours(index, y);
                    if h + v + d == 0 {
                        14
                    } else {
                        15
                    }
                };
                if decide(coder, contexts, context) == 1 {
                    block.magnitudes[index] |= bit;
                }
                block.coded[index] = bit.trailing_zeros() as u8;
                block.flags[index] |= REFINED;
            }
        }
    }
}

fn cleanup(block: &mut Block, coder: &mut Coder<'_>, contexts: &mut [Context; 19], bit: u32) {
    for stripe in (0..block.height).step_by(4) {
        let full = stripe + 4 <= block.height;
        for x in 0..block.width {
            let mut y = stripe;
            // A whole column of four with nothing around: one bit says
            // whether any of them is significant, and two more which.
            let quiet = full
                && (stripe..stripe + 4).all(|row| {
                    let index = block.index(x, row);
                    let (h, v, d) = block.neighbours(index, row);
                    block.flags[index] & (SIGNIFICANT | VISITED) == 0 && h + v + d == 0
                });
            if quiet {
                if decide(coder, contexts, RUN) == 0 {
                    continue;
                }
                let first = decide(coder, contexts, UNIFORM);
                let second = decide(coder, contexts, UNIFORM);
                y = stripe + usize::from(first * 2 + second);
                let index = block.index(x, y);
                become_significant(block, coder, contexts, index, y, bit);
                y += 1;
            }
            for row in y..(stripe + 4).min(block.height) {
                let index = block.index(x, row);
                if block.flags[index] & (SIGNIFICANT | VISITED) != 0 {
                    continue;
                }
                let context = block.zero_context(index, row);
                if decide(coder, contexts, context) == 1 {
                    become_significant(block, coder, contexts, index, row, bit);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_break_where_the_style_ends_the_coding() {
        assert_eq!(segment_room(0, 0), usize::MAX);
        assert_eq!(segment_room(TERMINATE_EACH, 7), 1);
        assert_eq!(segment_room(BYPASS, 0), 10);
        assert_eq!(segment_room(BYPASS, 10), 2);
        assert_eq!(segment_room(BYPASS, 12), 1);
        assert_eq!(segment_room(BYPASS, 13), 2);
    }

    #[test]
    fn the_zero_contexts_follow_the_table() {
        assert_eq!(primary(2, 0, 0), 8);
        assert_eq!(primary(1, 1, 0), 7);
        assert_eq!(primary(1, 0, 3), 6);
        assert_eq!(primary(1, 0, 0), 5);
        assert_eq!(primary(0, 2, 0), 4);
        assert_eq!(primary(0, 1, 4), 3);
        assert_eq!(primary(0, 0, 3), 2);
        assert_eq!(primary(0, 0, 1), 1);
        assert_eq!(primary(0, 0, 0), 0);
    }
}
