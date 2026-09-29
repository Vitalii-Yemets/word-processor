//! The MQ arithmetic decoder, which JPEG 2000 and JBIG2 both code with.
//!
//! [ITU-T T.800] Annex C, and [T.88] Annex E, which are the same coder. A
//! bit is decoded in a *context* — a small state saying how likely a nought
//! is, learnt as the bits go — and the coder's interval is split by that
//! probability; what the code register falls in is the bit. The states and
//! how each moves on are a table of forty-seven.

/// One state: the probability of the less likely symbol, where the state
/// goes after the more likely one and after the less likely one, and
/// whether the less likely one swaps which symbol is more likely.
struct State {
    qe: u32,
    next_more: u8,
    next_less: u8,
    switch: bool,
}

const fn state(qe: u32, next_more: u8, next_less: u8, switch: bool) -> State {
    State { qe, next_more, next_less, switch }
}

const STATES: [State; 47] = [
    state(0x5601, 1, 1, true),
    state(0x3401, 2, 6, false),
    state(0x1801, 3, 9, false),
    state(0x0AC1, 4, 12, false),
    state(0x0521, 5, 29, false),
    state(0x0221, 38, 33, false),
    state(0x5601, 7, 6, true),
    state(0x5401, 8, 14, false),
    state(0x4801, 9, 14, false),
    state(0x3801, 10, 14, false),
    state(0x3001, 11, 17, false),
    state(0x2401, 12, 18, false),
    state(0x1C01, 13, 20, false),
    state(0x1601, 29, 21, false),
    state(0x5601, 15, 14, true),
    state(0x5401, 16, 14, false),
    state(0x5101, 17, 15, false),
    state(0x4801, 18, 16, false),
    state(0x3801, 19, 17, false),
    state(0x3401, 20, 18, false),
    state(0x3001, 21, 19, false),
    state(0x2801, 22, 19, false),
    state(0x2401, 23, 20, false),
    state(0x2201, 24, 21, false),
    state(0x1C01, 25, 22, false),
    state(0x1801, 26, 23, false),
    state(0x1601, 27, 24, false),
    state(0x1401, 28, 25, false),
    state(0x1201, 29, 26, false),
    state(0x1101, 30, 27, false),
    state(0x0AC1, 31, 28, false),
    state(0x09C1, 32, 29, false),
    state(0x08A1, 33, 30, false),
    state(0x0521, 34, 31, false),
    state(0x0441, 35, 32, false),
    state(0x02A1, 36, 33, false),
    state(0x0221, 37, 34, false),
    state(0x0141, 38, 35, false),
    state(0x0111, 39, 36, false),
    state(0x0085, 40, 37, false),
    state(0x0049, 41, 38, false),
    state(0x0025, 42, 39, false),
    state(0x0015, 43, 40, false),
    state(0x0009, 44, 41, false),
    state(0x0005, 45, 42, false),
    state(0x0001, 45, 43, false),
    state(0x5601, 46, 46, false),
];

/// A context: its state, and which symbol is the more likely.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Context {
    pub index: u8,
    pub more_likely: u8,
}

impl Context {
    #[must_use]
    pub const fn at(index: u8) -> Self {
        Self { index, more_likely: 0 }
    }
}

/// The decoder over some bytes.
pub struct Decoder<'a> {
    data: &'a [u8],
    at: usize,
    end: usize,
    high: u32,
    low: u32,
    count: u32,
    interval: u32,
}

impl<'a> Decoder<'a> {
    /// Begins decoding at `start`, reading no further than `end`: past it,
    /// the data is taken to be the ones a coder's flushing leaves.
    #[must_use]
    pub fn new(data: &'a [u8], start: usize, end: usize) -> Self {
        let end = end.min(data.len());
        let mut decoder = Self {
            data,
            at: start,
            end,
            high: u32::from(data.get(start).copied().filter(|_| start < end).unwrap_or(0xFF)),
            low: 0,
            count: 0,
            interval: 0,
        };
        decoder.byte_in();
        decoder.high = ((decoder.high << 7) & 0xFFFF) | ((decoder.low >> 9) & 0x7F);
        decoder.low = (decoder.low << 7) & 0xFFFF;
        decoder.count = decoder.count.saturating_sub(7);
        decoder.interval = 0x8000;
        decoder
    }

    fn byte(&self, at: usize) -> u32 {
        if at < self.end {
            u32::from(self.data[at])
        } else {
            0xFF
        }
    }

    fn byte_in(&mut self) {
        if self.byte(self.at) == 0xFF {
            if self.byte(self.at + 1) > 0x8F {
                self.low += 0xFF00;
                self.count = 8;
            } else {
                self.at += 1;
                self.low += self.byte(self.at) << 9;
                self.count = 7;
            }
        } else {
            self.at += 1;
            self.low += if self.at < self.end { self.byte(self.at) << 8 } else { 0xFF00 };
            self.count = 8;
        }
        if self.low > 0xFFFF {
            self.high += self.low >> 16;
            self.low &= 0xFFFF;
        }
    }

    /// One bit, in a context, which it teaches.
    pub fn bit(&mut self, context: &mut Context) -> u8 {
        let state = &STATES[usize::from(context.index)];
        let qe = state.qe;
        let mut more = context.more_likely;
        let index;
        let decoded;
        let mut interval = self.interval - qe;
        if self.high < qe {
            // The less likely half, or an exchange.
            if interval < qe {
                interval = qe;
                decoded = more;
                index = state.next_more;
            } else {
                interval = qe;
                decoded = 1 ^ more;
                if state.switch {
                    more = decoded;
                }
                index = state.next_less;
            }
        } else {
            self.high -= qe;
            if interval & 0x8000 != 0 {
                self.interval = interval;
                return more;
            }
            if interval < qe {
                decoded = 1 ^ more;
                if state.switch {
                    more = decoded;
                }
                index = state.next_less;
            } else {
                decoded = more;
                index = state.next_more;
            }
        }
        // Renormalise.
        loop {
            if self.count == 0 {
                self.byte_in();
            }
            interval <<= 1;
            self.high = ((self.high << 1) & 0xFFFF) | ((self.low >> 15) & 1);
            self.low = (self.low << 1) & 0xFFFF;
            self.count -= 1;
            if interval & 0x8000 != 0 {
                break;
            }
        }
        self.interval = interval;
        context.index = index;
        context.more_likely = more;
        decoded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_standards_test_sequence_decodes() {
        // T.88 Annex H.2: thirty bytes of coded data that decode, all in
        // one context, to the 256 bits below.
        let coded = [
            0x84, 0xC7, 0x3B, 0xFC, 0xE1, 0xA1, 0x43, 0x04, 0x02, 0x20, 0x00, 0x00, 0x41, 0x0D,
            0xBB, 0x86, 0xF4, 0x31, 0x7F, 0xFF, 0x88, 0xFF, 0x37, 0x47, 0x1A, 0xDB, 0x6A, 0xDF,
            0xFF, 0xAC,
        ];
        let expected = [
            0x00, 0x02, 0x00, 0x51, 0x00, 0x00, 0x00, 0xC0, 0x03, 0x52, 0x87, 0x2A, 0xAA, 0xAA,
            0xAA, 0xAA, 0x82, 0xC0, 0x20, 0x00, 0xFC, 0xD7, 0x9E, 0xF6, 0xBF, 0x7F, 0xED, 0x90,
            0x4F, 0x46, 0xA3, 0xBF,
        ];
        let mut decoder = Decoder::new(&coded, 0, coded.len());
        let mut context = Context::default();
        let mut out = Vec::new();
        for _ in 0..expected.len() {
            let mut value = 0u8;
            for _ in 0..8 {
                value = (value << 1) | decoder.bit(&mut context);
            }
            out.push(value);
        }
        assert_eq!(out, expected);
    }
}
