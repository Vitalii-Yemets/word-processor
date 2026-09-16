//! Numbers too big for a machine word.
//!
//! # Why this is here
//!
//! Because a signature is a number with six hundred digits in it raised to a
//! power and divided by another number with six hundred digits. There is no
//! way round that and no way to do it in a `u64`, so the arithmetic is
//! written out: a number is a run of thirty-two bit pieces, least significant
//! first, and long multiplication and long division are done on them the way
//! they are done on paper, with a `u64` to hold each pair of digits while
//! they are being multiplied.
//!
//! # What is not here
//!
//! Anything a general-purpose library of big numbers would have: negatives,
//! greatest common divisors, primality, square roots. What a signature needs
//! is comparison, subtraction, multiplication, remainder and raising to a
//! power modulo another number, and that is what is here.
//!
//! Nor is it fast. Raising a two-thousand-bit number to a two-thousand-bit
//! power takes a few thousand multiplications of that size, and this does
//! them by the schoolbook method. A signature takes a fraction of a second,
//! which is what it has to be: it happens once when a document is opened.

/// A number, as thirty-two bit pieces with the least significant first.
///
/// Thirty-two and not sixty-four because two pieces have to be multiplied
/// into one machine word, and sixty-four times sixty-four does not fit in a
/// `u64`. There is a wider type for that now, and this arithmetic is not the
/// part of opening a document that anybody waits for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Big {
    pieces: Vec<u32>,
}

impl Big {
    /// Nought.
    #[must_use]
    pub fn zero() -> Self {
        Self { pieces: Vec::new() }
    }

    /// A number written most significant byte first, which is how every
    /// format this program reads writes one.
    #[must_use]
    pub fn from_be_bytes(bytes: &[u8]) -> Self {
        let mut pieces = Vec::with_capacity(bytes.len().div_ceil(4));
        let mut at = bytes.len();
        while at > 0 {
            let from = at.saturating_sub(4);
            let mut piece = 0u32;
            for byte in &bytes[from..at] {
                piece = (piece << 8) | u32::from(*byte);
            }
            pieces.push(piece);
            at = from;
        }
        let mut out = Self { pieces };
        out.trim();
        out
    }

    /// The same, back again, in exactly `length` bytes.
    ///
    /// Padded with noughts at the front where the number is shorter, which is
    /// what a signature needs: a signature is as long as the key whatever the
    /// number happens to be.
    #[must_use]
    pub fn to_be_bytes(&self, length: usize) -> Vec<u8> {
        let mut out = vec![0u8; length];
        for (index, piece) in self.pieces.iter().enumerate() {
            for byte in 0..4 {
                let at = index * 4 + byte;
                if at < length {
                    out[length - 1 - at] = (piece >> (byte * 8)) as u8;
                }
            }
        }
        out
    }

    /// How many bytes it takes to write.
    #[must_use]
    pub fn byte_length(&self) -> usize {
        match self.pieces.last() {
            None => 0,
            Some(top) => {
                (self.pieces.len() - 1) * 4 + (32 - top.leading_zeros() as usize).div_ceil(8)
            }
        }
    }

    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.pieces.is_empty()
    }

    /// Takes the noughts off the top, so that two equal numbers are equal.
    fn trim(&mut self) {
        while self.pieces.last() == Some(&0) {
            self.pieces.pop();
        }
    }

    /// Which is the bigger.
    #[must_use]
    fn compare(&self, other: &Self) -> core::cmp::Ordering {
        if self.pieces.len() != other.pieces.len() {
            return self.pieces.len().cmp(&other.pieces.len());
        }
        for (mine, theirs) in self.pieces.iter().rev().zip(other.pieces.iter().rev()) {
            let order = mine.cmp(theirs);
            if order != core::cmp::Ordering::Equal {
                return order;
            }
        }
        core::cmp::Ordering::Equal
    }

    /// One number times another, by long multiplication.
    #[must_use]
    fn times(&self, other: &Self) -> Self {
        if self.is_zero() || other.is_zero() {
            return Self::zero();
        }
        let mut pieces = vec![0u32; self.pieces.len() + other.pieces.len()];
        for (index, mine) in self.pieces.iter().enumerate() {
            let mut carry = 0u64;
            for (step, theirs) in other.pieces.iter().enumerate() {
                let at = index + step;
                let sum = u64::from(pieces[at]) + u64::from(*mine) * u64::from(*theirs) + carry;
                pieces[at] = sum as u32;
                carry = sum >> 32;
            }
            let mut at = index + other.pieces.len();
            while carry > 0 {
                let sum = u64::from(pieces[at]) + carry;
                pieces[at] = sum as u32;
                carry = sum >> 32;
                at += 1;
            }
        }
        let mut out = Self { pieces };
        out.trim();
        out
    }

    /// Takes one number away from another, which must not be the larger.
    fn take_away(&mut self, other: &Self) {
        let mut borrow = 0i64;
        for (index, theirs) in other.pieces.iter().enumerate() {
            let difference = i64::from(self.pieces[index]) - i64::from(*theirs) - borrow;
            if difference < 0 {
                self.pieces[index] = (difference + (1i64 << 32)) as u32;
                borrow = 1;
            } else {
                self.pieces[index] = difference as u32;
                borrow = 0;
            }
        }
        let mut index = other.pieces.len();
        while borrow > 0 {
            let difference = i64::from(self.pieces[index]) - borrow;
            if difference < 0 {
                self.pieces[index] = (difference + (1i64 << 32)) as u32;
                borrow = 1;
            } else {
                self.pieces[index] = difference as u32;
                borrow = 0;
            }
            index += 1;
        }
        self.trim();
    }

    /// Doubles it.
    fn double(&mut self) {
        let mut carry = 0u32;
        for piece in &mut self.pieces {
            let doubled = (u64::from(*piece) << 1) | u64::from(carry);
            *piece = doubled as u32;
            carry = (doubled >> 32) as u32;
        }
        if carry > 0 {
            self.pieces.push(carry);
        }
    }

    /// Adds one to it.
    fn add_one(&mut self) {
        for piece in &mut self.pieces {
            let (sum, carried) = piece.overflowing_add(1);
            *piece = sum;
            if !carried {
                return;
            }
        }
        self.pieces.push(1);
    }

    /// What is left of this number after taking the other away as many times
    /// as it will go.
    ///
    /// Long division, one bit at a time, which is the plainest way of writing
    /// it and fast enough for a number a signature's size. The quotient is
    /// not kept: nothing here needs it.
    #[must_use]
    pub fn remainder(&self, divisor: &Self) -> Self {
        if divisor.is_zero() || self.compare(divisor) == core::cmp::Ordering::Less {
            return self.clone();
        }
        let mut left = Self::zero();
        for bit in (0..self.bit_length()).rev() {
            left.double();
            if self.bit(bit) {
                left.add_one();
            }
            if left.compare(divisor) != core::cmp::Ordering::Less {
                left.take_away(divisor);
            }
        }
        left
    }

    /// How many bits it takes to write.
    #[must_use]
    fn bit_length(&self) -> usize {
        match self.pieces.last() {
            None => 0,
            Some(top) => self.pieces.len() * 32 - top.leading_zeros() as usize,
        }
    }

    /// Whether a given bit is set, counted from the least significant.
    #[must_use]
    fn bit(&self, at: usize) -> bool {
        self.pieces.get(at / 32).is_some_and(|piece| piece >> (at % 32) & 1 == 1)
    }

    /// This number raised to a power, modulo another: the one operation the
    /// whole of RSA is.
    ///
    /// By squaring: the exponent's bits are walked from the top, the answer
    /// squared at each step and multiplied by the base where the bit is set.
    /// Two thousand bits of exponent is four thousand multiplications rather
    /// than the unimaginable number of them that multiplying one at a time
    /// would be.
    ///
    /// Not constant-time. Which bits of the exponent are set can be told from
    /// how long this takes, and where the exponent is a private key that
    /// matters very much — see the crate's own documentation for what is
    /// being defended here and what is not.
    #[must_use]
    pub fn power_modulo(&self, exponent: &Self, modulus: &Self) -> Self {
        if modulus.is_zero() {
            return Self::zero();
        }
        let base = self.remainder(modulus);
        let mut answer = Self::from_be_bytes(&[1]);
        for bit in (0..exponent.bit_length()).rev() {
            answer = answer.times(&answer).remainder(modulus);
            if exponent.bit(bit) {
                answer = answer.times(&base).remainder(modulus);
            }
        }
        answer
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn of(value: u64) -> Big {
        Big::from_be_bytes(&value.to_be_bytes())
    }

    fn as_u64(value: &Big) -> u64 {
        let bytes = value.to_be_bytes(8);
        u64::from_be_bytes(bytes.try_into().expect("eight bytes"))
    }

    #[test]
    fn a_number_written_and_read_back_is_the_same_number() {
        for value in [0u64, 1, 255, 256, 65_535, 1 << 31, u64::MAX, 1_234_567_890_123] {
            assert_eq!(as_u64(&of(value)), value);
        }
        // The noughts at the front of a written number are not part of it.
        assert_eq!(Big::from_be_bytes(&[0, 0, 5]), Big::from_be_bytes(&[5]));
        assert!(Big::from_be_bytes(&[0, 0, 0]).is_zero());
    }

    #[test]
    fn multiplying_and_taking_the_remainder_agree_with_the_machine() {
        let cases = [
            (0u64, 1u64),
            (1, 1),
            (7, 5),
            (65_537, 65_535),
            (1 << 31, 3),
            (4_294_967_295, 4_294_967_295),
            (1_000_000_007, 998_244_353),
        ];
        for (left, right) in cases {
            let product = of(left).times(&of(right));
            assert_eq!(as_u64(&product), left * right, "{left} times {right}");
            if right != 0 {
                assert_eq!(as_u64(&of(left).remainder(&of(right))), left % right);
            }
        }
    }

    #[test]
    fn raising_to_a_power_agrees_with_the_machine() {
        for (base, exponent, modulus) in [
            (2u64, 10u64, 1000u64),
            (3, 0, 7),
            (5, 117, 19),
            (65_537, 31, 1_000_003),
            (123_456, 65_537, 4_294_967_291),
        ] {
            let mut wanted = 1u128;
            for _ in 0..exponent {
                wanted = wanted * u128::from(base) % u128::from(modulus);
            }
            assert_eq!(
                as_u64(&of(base).power_modulo(&of(exponent), &of(modulus))),
                wanted as u64,
                "{base} to the {exponent} modulo {modulus}"
            );
        }
    }

    #[test]
    fn a_number_of_the_size_a_signature_uses() {
        // Two thousand and forty-eight bits, and the smallest exponent that
        // is ever used with one.
        let modulus = Big::from_be_bytes(&[0xC5; 256]);
        let base = Big::from_be_bytes(&[0x7B; 255]);
        let exponent = Big::from_be_bytes(&[1, 0, 1]);
        let answer = base.power_modulo(&exponent, &modulus);

        // Raising to the power three by hand, the same way, must agree.
        let cubed = {
            let squared = base.times(&base).remainder(&modulus);
            let mut out = Big::from_be_bytes(&[1]);
            for _ in 0..3 {
                out = out.times(&base).remainder(&modulus);
            }
            let _ = squared;
            out
        };
        assert_eq!(base.power_modulo(&Big::from_be_bytes(&[3]), &modulus), cubed);
        assert!(!answer.is_zero());
        assert!(answer.byte_length() <= 256);
    }

    #[test]
    fn how_long_a_number_is_to_write() {
        assert_eq!(Big::zero().byte_length(), 0);
        assert_eq!(Big::from_be_bytes(&[1]).byte_length(), 1);
        assert_eq!(Big::from_be_bytes(&[255]).byte_length(), 1);
        assert_eq!(Big::from_be_bytes(&[1, 0]).byte_length(), 2);
        assert_eq!(Big::from_be_bytes(&[0xFF; 9]).byte_length(), 9);
    }
}
