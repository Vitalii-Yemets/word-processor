//! Numbers coded with the MQ coder.
//!
//! [T.88] Annex A. An integer is a sign and then a few bits saying which
//! range it is in and a few more saying where in it, each bit in a context
//! made of the bits before it; a symbol's number is so many bits, each in
//! the context of the ones before.

use super::super::mq::{Context, Decoder};

/// One of the integer decoders: each kind of number a region codes has
/// its own contexts.
pub struct Integers {
    contexts: Vec<Context>,
}

impl Default for Integers {
    fn default() -> Self {
        Self { contexts: vec![Context::default(); 512] }
    }
}

impl Integers {
    /// A number, or nothing for the out-of-band value that ends a run.
    pub fn decode(&mut self, decoder: &mut Decoder<'_>) -> Option<i64> {
        let mut previous = 1usize;
        let mut bit = |decoder: &mut Decoder<'_>, previous: &mut usize| -> u32 {
            let value = decoder.bit(&mut self.contexts[*previous]);
            *previous = if *previous < 256 {
                (*previous << 1) | usize::from(value)
            } else {
                (((*previous << 1) | usize::from(value)) & 511) | 256
            };
            u32::from(value)
        };
        let sign = bit(decoder, &mut previous);
        let (bits, offset) = if bit(decoder, &mut previous) == 0 {
            (2, 0)
        } else if bit(decoder, &mut previous) == 0 {
            (4, 4)
        } else if bit(decoder, &mut previous) == 0 {
            (6, 20)
        } else if bit(decoder, &mut previous) == 0 {
            (8, 84)
        } else if bit(decoder, &mut previous) == 0 {
            (12, 340)
        } else {
            (32, 4436)
        };
        let mut value: i64 = 0;
        for _ in 0..bits {
            value = (value << 1) | i64::from(bit(decoder, &mut previous));
        }
        let value = value + offset;
        match (sign, value) {
            (1, 0) => None,
            (1, _) => Some(-value),
            _ => Some(value),
        }
    }
}

/// The decoder of symbol numbers, `length` bits each.
pub struct SymbolIds {
    length: u32,
    contexts: Vec<Context>,
}

impl SymbolIds {
    #[must_use]
    pub fn new(length: u32) -> Self {
        let length = length.min(24);
        Self { length, contexts: vec![Context::default(); 1 << (length + 1)] }
    }

    pub fn decode(&mut self, decoder: &mut Decoder<'_>) -> usize {
        let mut previous = 1usize;
        for _ in 0..self.length {
            let bit = decoder.bit(&mut self.contexts[previous]);
            previous = (previous << 1) | usize::from(bit);
        }
        previous - (1 << self.length)
    }
}

/// The bits it takes to number `count` things.
#[must_use]
pub fn bits_for(count: usize) -> u32 {
    let mut bits = 0;
    while (1usize << bits) < count {
        bits += 1;
    }
    bits
}
