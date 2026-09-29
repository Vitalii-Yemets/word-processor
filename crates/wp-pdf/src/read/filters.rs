//! Undoing the filters a stream's bytes went through.
//!
//! Deflate, which nearly everything uses; LZW, which older files use; the
//! two ASCII armourings; and run lengths. The predictor a deflated stream
//! may have been passed through first is undone after. The picture
//! filters — JPEG, JPEG 2000, fax and JBIG2 — are not filters of bytes but
//! of pictures, and are left for the picture reader to know about.

use super::object::{Dictionary, Object};

/// The most a stream may inflate to: a page of text is kilobytes, a
/// picture megabytes, and a stream past this is not a document.
const MOST: usize = 256 * 1024 * 1024;

/// Whether a filter is one of the picture filters, which leave a picture
/// rather than bytes.
#[must_use]
pub fn is_picture_filter(name: &str) -> bool {
    matches!(name, "DCTDecode" | "DCT" | "JPXDecode" | "CCITTFaxDecode" | "CCF" | "JBIG2Decode")
}

/// A filter's full name, where an inline picture may give it short.
#[must_use]
pub fn full_name(name: &str) -> &str {
    match name {
        "Fl" => "FlateDecode",
        "LZW" => "LZWDecode",
        "AHx" => "ASCIIHexDecode",
        "A85" => "ASCII85Decode",
        "RL" => "RunLengthDecode",
        "DCT" => "DCTDecode",
        "CCF" => "CCITTFaxDecode",
        other => other,
    }
}

/// Applies one filter. Nothing if the filter is not known or the bytes are
/// not what it expects.
#[must_use]
pub fn apply(name: &str, data: &[u8], parameters: Option<&Dictionary>) -> Option<Vec<u8>> {
    let out = match name {
        "FlateDecode" | "Fl" => inflate(data)?,
        "LZWDecode" | "LZW" => {
            let early = parameters
                .and_then(|p| p.get("EarlyChange"))
                .and_then(Object::as_integer)
                .unwrap_or(1)
                != 0;
            lzw(data, early)
        }
        "ASCIIHexDecode" | "AHx" => ascii_hex(data),
        "ASCII85Decode" | "A85" => ascii85(data),
        "RunLengthDecode" | "RL" => run_length(data),
        "Crypt" => data.to_vec(),
        _ => return None,
    };
    Some(match parameters {
        Some(parameters) => predict(out, parameters),
        None => out,
    })
}

/// Inflates a zlib stream, or a bare deflate one when the header is not
/// there.
fn inflate(data: &[u8]) -> Option<Vec<u8>> {
    let start = data.iter().position(|b| !super::object::is_whitespace(*b)).unwrap_or(0);
    let data = &data[start..];
    if data.is_empty() {
        return Some(Vec::new());
    }
    if let Ok(out) = wp_deflate::inflate_zlib(data, MOST) {
        return Some(out);
    }
    if data.len() > 2 {
        if let Ok(out) = wp_deflate::inflate_limited(&data[2..], MOST) {
            return Some(out);
        }
    }
    wp_deflate::inflate_limited(data, MOST).ok()
}

/// LZW as the format uses it: codes of 9 to 12 bits, most significant bit
/// first, 256 clearing the table and 257 ending the data.
fn lzw(data: &[u8], early_change: bool) -> Vec<u8> {
    let mut out = Vec::new();
    let mut table: Vec<Vec<u8>> = (0..256).map(|b| vec![b as u8]).collect();
    table.push(Vec::new());
    table.push(Vec::new());
    let mut width = 9;
    let mut previous: Option<Vec<u8>> = None;
    let mut bits: u32 = 0;
    let mut held = 0;
    for &byte in data {
        bits = (bits << 8) | u32::from(byte);
        held += 8;
        while held >= width {
            let code = ((bits >> (held - width)) & ((1 << width) - 1)) as usize;
            held -= width;
            match code {
                256 => {
                    table.truncate(258);
                    width = 9;
                    previous = None;
                }
                257 => return out,
                _ => {
                    let entry = if code < table.len() {
                        table[code].clone()
                    } else if let Some(previous) = &previous {
                        let mut entry = previous.clone();
                        entry.push(previous[0]);
                        entry
                    } else {
                        return out;
                    };
                    out.extend_from_slice(&entry);
                    if let Some(previous) = previous {
                        let mut added = previous;
                        added.push(entry[0]);
                        table.push(added);
                    }
                    previous = Some(entry);
                    let limit = if early_change { 1 } else { 0 };
                    if table.len() + limit >= (1 << width) && width < 12 {
                        width += 1;
                    }
                }
            }
        }
    }
    out
}

fn ascii_hex(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 2);
    let mut high: Option<u8> = None;
    for &byte in data {
        if byte == b'>' {
            break;
        }
        let Some(digit) = (byte as char).to_digit(16) else { continue };
        match high.take() {
            Some(high) => out.push(high * 16 + digit as u8),
            None => high = Some(digit as u8),
        }
    }
    if let Some(high) = high {
        out.push(high * 16);
    }
    out
}

fn ascii85(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() * 4 / 5);
    let mut group = [0u32; 5];
    let mut count = 0;
    let mut data = data;
    if data.starts_with(b"<~") {
        data = &data[2..];
    }
    for &byte in data {
        if super::object::is_whitespace(byte) {
            continue;
        }
        if byte == b'~' {
            break;
        }
        if byte == b'z' && count == 0 {
            out.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        if !(b'!'..=b'u').contains(&byte) {
            continue;
        }
        group[count] = u32::from(byte - b'!');
        count += 1;
        if count == 5 {
            let value =
                group.iter().fold(0u32, |acc, &digit| acc.wrapping_mul(85).wrapping_add(digit));
            out.extend_from_slice(&value.to_be_bytes());
            count = 0;
        }
    }
    if count > 0 {
        for slot in group.iter_mut().skip(count) {
            *slot = 84;
        }
        let value = group.iter().fold(0u32, |acc, &digit| acc.wrapping_mul(85).wrapping_add(digit));
        out.extend_from_slice(&value.to_be_bytes()[..count - 1]);
    }
    out
}

fn run_length(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < data.len() {
        let length = data[at];
        at += 1;
        match length {
            128 => break,
            0..=127 => {
                let end = (at + usize::from(length) + 1).min(data.len());
                out.extend_from_slice(&data[at..end]);
                at = end;
            }
            _ => {
                if let Some(&byte) = data.get(at) {
                    out.extend(std::iter::repeat_n(byte, 257 - usize::from(length)));
                }
                at += 1;
            }
        }
    }
    out
}

/// Undoes a predictor: the TIFF one, which stores each sample as the
/// difference from the one to its left, or the PNG ones, which store each
/// row with a byte in front saying how it was predicted.
fn predict(data: Vec<u8>, parameters: &Dictionary) -> Vec<u8> {
    let number = |key: &str, default: i64| {
        parameters.get(key).and_then(Object::as_integer).unwrap_or(default)
    };
    let predictor = number("Predictor", 1);
    if predictor <= 1 {
        return data;
    }
    let colors = number("Colors", 1).clamp(1, 64) as usize;
    let bits = number("BitsPerComponent", 8).clamp(1, 16) as usize;
    let columns = number("Columns", 1).max(1) as usize;
    let bytes_per_pixel = (colors * bits).div_ceil(8).max(1);
    let row_length = (columns * colors * bits).div_ceil(8);
    if predictor == 2 {
        if bits != 8 {
            return data;
        }
        let mut data = data;
        for row in data.chunks_mut(row_length) {
            for index in bytes_per_pixel..row.len() {
                row[index] = row[index].wrapping_add(row[index - bytes_per_pixel]);
            }
        }
        return data;
    }
    let mut out = Vec::with_capacity(data.len());
    let mut previous = vec![0u8; row_length];
    for row in data.chunks(row_length + 1) {
        let Some((&kind, row)) = row.split_first() else { break };
        let mut current = row.to_vec();
        current.resize(row_length, 0);
        for index in 0..row_length {
            let left = if index >= bytes_per_pixel { current[index - bytes_per_pixel] } else { 0 };
            let up = previous[index];
            let up_left =
                if index >= bytes_per_pixel { previous[index - bytes_per_pixel] } else { 0 };
            let predicted = match kind {
                0 => 0,
                1 => left,
                2 => up,
                3 => ((u16::from(left) + u16::from(up)) / 2) as u8,
                4 => paeth(left, up, up_left),
                _ => 0,
            };
            current[index] = current[index].wrapping_add(predicted);
        }
        out.extend_from_slice(&current);
        previous = current;
    }
    out
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = i16::from(a) + i16::from(b) - i16::from(c);
    let pa = (p - i16::from(a)).abs();
    let pb = (p - i16::from(b)).abs();
    let pc = (p - i16::from(c)).abs();
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ascii_armourings_come_off() {
        assert_eq!(ascii_hex(b"48656C6C 6F>"), b"Hello");
        assert_eq!(ascii_hex(b"4"), [0x40]);
        assert_eq!(ascii85(b"87cURD]i,\"Ebo80~>"), b"Hello World!");
        assert_eq!(ascii85(b"<~z~>"), [0, 0, 0, 0]);
    }

    #[test]
    fn run_lengths_expand() {
        assert_eq!(run_length(&[2, b'a', b'b', b'c', 254, b'x', 128]), b"abcxxx");
    }

    #[test]
    fn lzw_decodes_the_specifications_example() {
        let data = [0x80, 0x0B, 0x60, 0x50, 0x22, 0x0C, 0x0C, 0x85, 0x01];
        assert_eq!(lzw(&data, true), [45, 45, 45, 45, 45, 65, 45, 45, 45, 66]);
    }

    #[test]
    fn a_png_predictor_is_undone() {
        // Two rows of three bytes, the second predicted "up" from the first.
        let mut parameters = Dictionary::new();
        parameters.insert("Predictor".into(), Object::Number(12.0));
        parameters.insert("Columns".into(), Object::Number(3.0));
        let data = vec![0, 1, 2, 3, 2, 1, 1, 1];
        assert_eq!(predict(data, &parameters), [1, 2, 3, 2, 3, 4]);
    }

    #[test]
    fn deflate_inflates() {
        let packed = wp_deflate::compress_zlib(b"a page of text");
        assert_eq!(apply("FlateDecode", &packed, None).unwrap(), b"a page of text");
    }
}
