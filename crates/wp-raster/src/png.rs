//! Writing PNG images.
//!
//! There is no window yet, so this is how rendered output can actually be looked
//! at: a page is drawn into a canvas and written out as an image. It is also
//! what makes rendering testable — a reference image can be compared against,
//! which is the only way to catch a layout change that a numeric assertion would
//! not notice.
//!
//! The format is a signature, then a chain of chunks each carrying a CRC-32, with
//! the image itself stored as a zlib stream. Both of those already exist in
//! `wp-deflate`.

use wp_deflate::{compress_zlib, crc32};

use crate::canvas::Canvas;

/// The eight bytes every PNG begins with.
///
/// Chosen by the format's designers to catch damage in transit: a byte with the
/// high bit set, letters, and both kinds of line ending, so a file mangled by a
/// text-mode transfer fails immediately instead of subtly.
const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// Encodes a canvas as a PNG image.
#[must_use]
pub fn encode(canvas: &Canvas) -> Vec<u8> {
    let width = canvas.width() as u32;
    let height = canvas.height() as u32;

    let mut out = Vec::with_capacity(canvas.pixels().len() / 2 + 1024);
    out.extend_from_slice(&SIGNATURE);

    // Image header: size, 8 bits per channel, colour type 6 (red, green, blue
    // and alpha), no compression or filter variations, not interlaced.
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    write_chunk(&mut out, b"IHDR", &header);

    write_chunk(&mut out, b"IDAT", &compress_zlib(&filtered_rows(canvas)));
    write_chunk(&mut out, b"IEND", &[]);

    out
}

/// Prepares the pixel data, one filter byte per row.
///
/// Each row may be encoded relative to the one above or to its own left
/// neighbour. "Up" is used here: a page is mostly flat colour, so a row is
/// usually identical to the one before it, and the difference is then a run of
/// zeros that compresses to almost nothing.
fn filtered_rows(canvas: &Canvas) -> Vec<u8> {
    let width = canvas.width();
    let height = canvas.height();
    let row_bytes = width * 4;

    let mut out = Vec::with_capacity(height * (row_bytes + 1));
    let pixels = canvas.pixels();

    for row in 0..height {
        let start = row * row_bytes;
        let current = &pixels[start..start + row_bytes];

        if row == 0 {
            // Nothing above the first row to subtract, so it is stored as-is.
            out.push(0);
            out.extend_from_slice(current);
        } else {
            out.push(2);
            let above = &pixels[start - row_bytes..start];
            for (value, previous) in current.iter().zip(above) {
                out.push(value.wrapping_sub(*previous));
            }
        }
    }

    out
}

/// Writes one chunk: length, type, payload, and a checksum over the last two.
fn write_chunk(out: &mut Vec<u8>, kind: &[u8; 4], payload: &[u8]) {
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());

    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(payload);

    let checksum = crc32(&out[start..]);
    out.extend_from_slice(&checksum.to_be_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::Color;

    #[test]
    fn produces_a_file_a_reader_would_recognize() {
        let image = encode(&Canvas::filled(4, 4, Color::WHITE));

        assert_eq!(&image[..8], &SIGNATURE);
        assert_eq!(&image[12..16], b"IHDR");
        assert!(image.windows(4).any(|window| window == b"IDAT"));
        assert_eq!(&image[image.len() - 8..image.len() - 4], b"IEND");
    }

    #[test]
    fn the_header_carries_the_size() {
        let image = encode(&Canvas::new(258, 3));
        assert_eq!(&image[16..20], &258u32.to_be_bytes());
        assert_eq!(&image[20..24], &3u32.to_be_bytes());
    }

    #[test]
    fn the_pixels_survive_the_round_trip() {
        // Decoded with our own inflate, which is the other half of the format.
        let mut canvas = Canvas::filled(8, 4, Color::WHITE);
        canvas.fill_rect(2, 1, 3, 2, Color::rgb(10, 20, 30));

        let image = encode(&canvas);
        let start = image
            .windows(4)
            .position(|window| window == b"IDAT")
            .expect("there should be an IDAT chunk");
        let length = u32::from_be_bytes([
            image[start - 4],
            image[start - 3],
            image[start - 2],
            image[start - 1],
        ]) as usize;
        let compressed = &image[start + 4..start + 4 + length];

        let raw = wp_deflate::inflate_zlib(compressed, 1 << 20).unwrap();

        // Undo the per-row filtering to get the pixels back.
        let row_bytes = canvas.width() * 4;
        let mut pixels = vec![0u8; canvas.height() * row_bytes];
        for row in 0..canvas.height() {
            let source = row * (row_bytes + 1);
            let filter = raw[source];
            for index in 0..row_bytes {
                let value = raw[source + 1 + index];
                pixels[row * row_bytes + index] = match filter {
                    0 => value,
                    2 => value.wrapping_add(pixels[(row - 1) * row_bytes + index]),
                    other => panic!("unexpected filter {other}"),
                };
            }
        }

        assert_eq!(pixels, canvas.pixels());
    }

    #[test]
    fn a_flat_image_compresses_well() {
        // The "up" filter turns identical rows into zeros, which is what makes a
        // page-sized image a sensible thing to write.
        let image = encode(&Canvas::filled(600, 800, Color::WHITE));
        let raw = 600 * 800 * 4;
        assert!(image.len() * 100 < raw, "expected far better than 100:1, got {}", image.len());
    }

    #[test]
    fn an_empty_canvas_still_produces_a_valid_file() {
        let image = encode(&Canvas::new(0, 0));
        assert_eq!(&image[..8], &SIGNATURE);
        assert_eq!(&image[image.len() - 8..image.len() - 4], b"IEND");
    }
}
