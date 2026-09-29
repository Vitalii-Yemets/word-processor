//! A bitmap, black or not a pixel at a time, and putting one onto another.

/// How one bitmap goes onto another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Combine {
    Or,
    And,
    Xor,
    Xnor,
    Replace,
}

impl Combine {
    #[must_use]
    pub fn of(code: u8) -> Self {
        match code {
            1 => Self::And,
            2 => Self::Xor,
            3 => Self::Xnor,
            4 => Self::Replace,
            _ => Self::Or,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Bitmap {
    pub width: usize,
    pub height: usize,
    /// A byte a pixel, one for black, row after row.
    pub pixels: Vec<u8>,
}

impl Bitmap {
    #[must_use]
    pub fn new(width: usize, height: usize) -> Self {
        Self::filled(width, height, 0)
    }

    #[must_use]
    pub fn filled(width: usize, height: usize, value: u8) -> Self {
        Self { width, height, pixels: vec![value; width * height] }
    }

    /// A pixel, or white outside.
    #[must_use]
    pub fn get(&self, x: i64, y: i64) -> u8 {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            0
        } else {
            self.pixels[y as usize * self.width + x as usize]
        }
    }

    pub fn set(&mut self, x: usize, y: usize, value: u8) {
        if x < self.width && y < self.height {
            self.pixels[y * self.width + x] = value;
        }
    }

    /// The rows of bits, packed a byte to eight pixels, as a fax coding
    /// or an uncoded bitmap gives them.
    #[must_use]
    pub fn from_packed(data: &[u8], width: usize, height: usize) -> Self {
        let row_bytes = width.div_ceil(8);
        let mut out = Self::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let byte = data.get(y * row_bytes + x / 8).copied().unwrap_or(0);
                out.pixels[y * width + x] = (byte >> (7 - x % 8)) & 1;
            }
        }
        out
    }

    #[must_use]
    pub fn packed(&self) -> Vec<u8> {
        let row_bytes = self.width.div_ceil(8);
        let mut out = vec![0u8; row_bytes * self.height];
        for y in 0..self.height {
            for x in 0..self.width {
                if self.pixels[y * self.width + x] != 0 {
                    out[y * row_bytes + x / 8] |= 0x80 >> (x % 8);
                }
            }
        }
        out
    }

    /// A rectangle of this one.
    #[must_use]
    pub fn part(&self, x: i64, y: i64, width: usize, height: usize) -> Self {
        let mut out = Self::new(width, height);
        for row in 0..height {
            for column in 0..width {
                out.pixels[row * width + column] = self.get(x + column as i64, y + row as i64);
            }
        }
        out
    }

    /// Another bitmap put onto this one with its top left at `x`, `y`.
    pub fn compose(&mut self, other: &Self, x: i64, y: i64, how: Combine) {
        for row in 0..other.height {
            let ty = y + row as i64;
            if ty < 0 || ty >= self.height as i64 {
                continue;
            }
            for column in 0..other.width {
                let tx = x + column as i64;
                if tx < 0 || tx >= self.width as i64 {
                    continue;
                }
                let at = ty as usize * self.width + tx as usize;
                let (old, new) = (self.pixels[at], other.pixels[row * other.width + column]);
                self.pixels[at] = match how {
                    Combine::Or => old | new,
                    Combine::And => old & new,
                    Combine::Xor => old ^ new,
                    Combine::Xnor => 1 - (old ^ new),
                    Combine::Replace => new,
                };
            }
        }
    }

    /// Taller, the new rows in `value`.
    pub fn grow_to(&mut self, height: usize, value: u8) {
        if height > self.height {
            self.pixels.resize(self.width * height, value);
            self.height = height;
        }
    }
}
