//! Putting a tile-component back together from its bands.
//!
//! [T.800] Annex F. Each level of the wavelet split a resolution into a
//! lower one and three bands of detail; going back, the four are
//! interleaved — the lower resolution on the even rows and columns, the
//! details on the odd — and the lifting steps undone along each row and
//! then each column. The reversible 5-3 wavelet does it in integers, so a
//! lossless picture comes back exact; the 9-7 one in real numbers. An
//! edge is mirrored for the samples past it.

/// A rectangle of samples, where it lies on its grid.
#[derive(Clone, Debug, Default)]
pub struct Plane {
    pub x0: i64,
    pub y0: i64,
    pub x1: i64,
    pub y1: i64,
    pub data: Vec<f32>,
}

impl Plane {
    #[must_use]
    pub fn new(x0: i64, y0: i64, x1: i64, y1: i64) -> Self {
        let size = ((x1 - x0).max(0) * (y1 - y0).max(0)) as usize;
        Self { x0, y0, x1, y1, data: vec![0.0; size] }
    }

    #[must_use]
    pub fn width(&self) -> usize {
        (self.x1 - self.x0).max(0) as usize
    }

    #[must_use]
    pub fn height(&self) -> usize {
        (self.y1 - self.y0).max(0) as usize
    }

    fn at(&self, x: i64, y: i64) -> f32 {
        let (x, y) = (x - self.x0, y - self.y0);
        if x < 0 || y < 0 || x >= self.x1 - self.x0 || y >= self.y1 - self.y0 {
            return 0.0;
        }
        self.data[(y * (self.x1 - self.x0) + x) as usize]
    }
}

/// One level back: the lower resolution and the three bands make the
/// resolution `x0..x1` by `y0..y1`.
#[must_use]
pub fn compose(
    low: &Plane,
    [high_low, low_high, high_high]: [&Plane; 3],
    (x0, y0, x1, y1): (i64, i64, i64, i64),
    reversible: bool,
) -> Plane {
    let mut out = Plane::new(x0, y0, x1, y1);
    let width = out.width();
    let height = out.height();
    if width == 0 || height == 0 {
        return out;
    }
    for v in y0..y1 {
        for u in x0..x1 {
            let source = match (u.rem_euclid(2), v.rem_euclid(2)) {
                (0, 0) => low,
                (1, 0) => high_low,
                (0, _) => low_high,
                _ => high_high,
            };
            let value = source.at(u.div_euclid(2), v.div_euclid(2));
            out.data[((v - y0) as usize) * width + (u - x0) as usize] = value;
        }
    }
    let mut line = Vec::new();
    let mut buffer = Vec::new();
    for row in out.data.chunks_mut(width) {
        line.clear();
        line.extend_from_slice(row);
        one_dimension(&mut line, x0, reversible, &mut buffer);
        row.copy_from_slice(&line);
    }
    for x in 0..width {
        line.clear();
        line.extend((0..height).map(|y| out.data[y * width + x]));
        one_dimension(&mut line, y0, reversible, &mut buffer);
        for (y, value) in line.iter().enumerate() {
            out.data[y * width + x] = *value;
        }
    }
    out
}

const PAD: usize = 4;

/// The lifting steps undone along one line, whose first sample is at
/// `start`: even places hold the low-pass samples, odd the high-pass.
fn one_dimension(line: &mut [f32], start: i64, reversible: bool, buffer: &mut Vec<f32>) {
    let n = line.len();
    if n == 0 {
        return;
    }
    if n == 1 {
        if start.rem_euclid(2) == 1 {
            line[0] = if reversible { (line[0] / 2.0).trunc() } else { line[0] / 2.0 };
        }
        return;
    }
    let period = 2 * (n as i64 - 1);
    let mirror = |i: i64| -> usize {
        let m = i.rem_euclid(period);
        (if m < n as i64 { m } else { period - m }) as usize
    };
    buffer.clear();
    buffer.extend((-(PAD as i64)..(n + PAD) as i64).map(|i| line[mirror(i)]));
    let length = buffer.len();
    // Whether a place in the buffer holds a low-pass sample.
    let first_even = (start - PAD as i64).rem_euclid(2) == 0;
    let even = |j: usize| (j % 2 == 0) == first_even;
    if reversible {
        for j in 1..length - 1 {
            if even(j) {
                buffer[j] -= ((buffer[j - 1] + buffer[j + 1] + 2.0) / 4.0).floor();
            }
        }
        for j in 1..length - 1 {
            if !even(j) {
                buffer[j] += ((buffer[j - 1] + buffer[j + 1]) / 2.0).floor();
            }
        }
    } else {
        const ALPHA: f32 = -1.586_134_3;
        const BETA: f32 = -0.052_980_12;
        const GAMMA: f32 = 0.882_911_1;
        const DELTA: f32 = 0.443_506_87;
        const K: f32 = 1.230_174_1;
        for (j, value) in buffer.iter_mut().enumerate() {
            *value *= if even(j) { K } else { 1.0 / K };
        }
        for (factor, on_even) in [(DELTA, true), (GAMMA, false), (BETA, true), (ALPHA, false)] {
            for j in 1..length - 1 {
                if even(j) == on_even {
                    buffer[j] -= factor * (buffer[j - 1] + buffer[j + 1]);
                }
            }
        }
    }
    line.copy_from_slice(&buffer[PAD..PAD + n]);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reversible wavelet forward, to check the inverse against.
    fn forward(line: &[f32], start: i64) -> Vec<f32> {
        let n = line.len();
        let period = 2 * (n as i64 - 1);
        let mirror = |i: i64| -> usize {
            let m = i.rem_euclid(period);
            (if m < n as i64 { m } else { period - m }) as usize
        };
        let mut buffer: Vec<f32> = (-4..(n + 4) as i64).map(|i| line[mirror(i)]).collect();
        let first_even = (start - 4).rem_euclid(2) == 0;
        let even = |j: usize| (j % 2 == 0) == first_even;
        let length = buffer.len();
        for j in 1..length - 1 {
            if !even(j) {
                buffer[j] -= ((buffer[j - 1] + buffer[j + 1]) / 2.0).floor();
            }
        }
        for j in 1..length - 1 {
            if even(j) {
                buffer[j] += ((buffer[j - 1] + buffer[j + 1] + 2.0) / 4.0).floor();
            }
        }
        buffer[4..4 + n].to_vec()
    }

    #[test]
    fn the_reversible_wavelet_comes_back_exact() {
        let samples: Vec<f32> = [12, 200, 7, 7, 90, 91, 3, 255, 0, 17, 64].map(|v| v as f32).into();
        for start in [0, 1, 4, 7] {
            let mut line = forward(&samples, start);
            let mut buffer = Vec::new();
            one_dimension(&mut line, start, true, &mut buffer);
            assert_eq!(line, samples, "starting at {start}");
        }
    }
}
