//! The single-file web page: a page and its pictures in one file, as mail
//! carries them.
//!
//! `.mht` is MIME — a `multipart/related` message whose first part is the
//! page and whose other parts are the pictures it refers to, each named by
//! a `Content-Location` the page's `src` attributes match. The page is
//! usually written quoted-printable, the pictures base64. Word's "Single
//! File Web Page" is exactly this, and so is what a browser saves.

/// One part of the message: what it is named, what kind it is, and its
/// bytes decoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    pub location: Option<String>,
    pub content_type: String,
    /// The charset the part's header names, for a text part.
    pub charset: Option<String>,
    pub bytes: Vec<u8>,
}

/// Cuts a message into its parts.
#[must_use]
pub fn parts(bytes: &[u8]) -> Vec<Part> {
    let (headers, body) = split_headers(bytes);
    let content_type = header(&headers, "content-type").unwrap_or_default();
    let Some(boundary) = parameter(&content_type, "boundary") else {
        // Not multipart: the whole message is one part.
        let charset = parameter(&content_type, "charset");
        return vec![Part {
            location: header(&headers, "content-location"),
            content_type: content_type.split(';').next().unwrap_or("").trim().to_ascii_lowercase(),
            charset,
            bytes: decode_body(body, header(&headers, "content-transfer-encoding").as_deref()),
        }];
    };

    let mut out = Vec::new();
    let marker = format!("--{boundary}");
    for piece in split_on(body, marker.as_bytes()).into_iter().skip(1) {
        // The last piece follows the closing marker, `--boundary--`.
        if piece.starts_with(b"--") {
            break;
        }
        let piece = strip_line_end(piece);
        let (headers, body) = split_headers(piece);
        let content_type = header(&headers, "content-type").unwrap_or_default();
        let part = Part {
            location: header(&headers, "content-location"),
            charset: parameter(&content_type, "charset"),
            content_type: content_type.split(';').next().unwrap_or("").trim().to_ascii_lowercase(),
            bytes: decode_body(body, header(&headers, "content-transfer-encoding").as_deref()),
        };
        // A part that is itself multipart holds the real parts.
        if part.content_type.starts_with("multipart/") {
            out.extend(parts(piece));
        } else {
            out.push(part);
        }
    }
    out
}

/// The headers and the body, cut at the first empty line.
fn split_headers(bytes: &[u8]) -> (Vec<(String, String)>, &[u8]) {
    let end = find(bytes, b"\r\n\r\n")
        .map(|at| (at, 4))
        .or_else(|| find(bytes, b"\n\n").map(|at| (at, 2)));
    let (head, body) = match end {
        Some((at, gap)) => (&bytes[..at], &bytes[at + gap..]),
        None => (bytes, &bytes[bytes.len()..]),
    };
    let text = String::from_utf8_lossy(head);
    let mut headers: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        // A line beginning with whitespace continues the header before it.
        if line.starts_with([' ', '\t']) {
            if let Some((_, value)) = headers.last_mut() {
                value.push(' ');
                value.push_str(line.trim());
            }
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
        }
    }
    (headers, body)
}

fn header(headers: &[(String, String)], wanted: &str) -> Option<String> {
    headers.iter().find(|(name, _)| name == wanted).map(|(_, value)| value.clone())
}

/// A parameter of a header value: `charset` of `text/html; charset="utf-8"`.
fn parameter(value: &str, wanted: &str) -> Option<String> {
    value.split(';').skip(1).find_map(|piece| {
        let (name, value) = piece.split_once('=')?;
        (name.trim().eq_ignore_ascii_case(wanted))
            .then(|| value.trim().trim_matches('"').to_owned())
    })
}

fn decode_body(body: &[u8], encoding: Option<&str>) -> Vec<u8> {
    match encoding.map(str::to_ascii_lowercase).as_deref() {
        Some("base64") => decode_base64(body),
        Some("quoted-printable") => decode_quoted_printable(body),
        _ => strip_line_end(body).to_vec(),
    }
}

fn strip_line_end(bytes: &[u8]) -> &[u8] {
    // The line end before a boundary belongs to the boundary, not to the
    // part: one of them, whichever kind it is.
    match bytes.strip_suffix(b"\r\n") {
        Some(stripped) => stripped,
        None => bytes.strip_suffix(b"\n").unwrap_or(bytes),
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

fn split_on<'a>(bytes: &'a [u8], marker: &[u8]) -> Vec<&'a [u8]> {
    let mut pieces = Vec::new();
    let mut rest = bytes;
    while let Some(at) = find(rest, marker) {
        pieces.push(&rest[..at]);
        rest = &rest[at + marker.len()..];
    }
    pieces.push(rest);
    pieces
}

/// Quoted-printable back to bytes: `=3D` is a byte, `=` at the end of a
/// line is a soft break.
#[must_use]
pub fn decode_quoted_printable(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let byte = bytes[at];
        if byte == b'=' {
            if bytes.get(at + 1) == Some(&b'\r') && bytes.get(at + 2) == Some(&b'\n') {
                at += 3;
                continue;
            }
            if bytes.get(at + 1) == Some(&b'\n') {
                at += 2;
                continue;
            }
            let hex = |b: Option<&u8>| b.copied().and_then(|b| (b as char).to_digit(16));
            if let (Some(high), Some(low)) = (hex(bytes.get(at + 1)), hex(bytes.get(at + 2))) {
                out.push((high * 16 + low) as u8);
                at += 3;
                continue;
            }
        }
        out.push(byte);
        at += 1;
    }
    out
}

/// Bytes as quoted-printable, lines no longer than seventy-six.
#[must_use]
pub fn encode_quoted_printable(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 5 / 4);
    let mut line = 0;
    for (index, &byte) in bytes.iter().enumerate() {
        if byte == b'\n' {
            out.push_str("\r\n");
            line = 0;
            continue;
        }
        if byte == b'\r' {
            continue;
        }
        let at_line_end = matches!(bytes.get(index + 1), Some(b'\n') | None);
        let plain = ((33..=126).contains(&byte) && byte != b'=') || (byte == b' ' && !at_line_end);
        let piece = if plain { (byte as char).to_string() } else { format!("={byte:02X}") };
        if line + piece.len() > 75 {
            out.push_str("=\r\n");
            line = 0;
        }
        out.push_str(&piece);
        line += piece.len();
    }
    out
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

#[must_use]
pub fn encode_base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 4 / 3 + 4);
    let mut line = 0;
    for chunk in bytes.chunks(3) {
        let mut value: u32 = 0;
        for (index, byte) in chunk.iter().enumerate() {
            value |= u32::from(*byte) << (16 - 8 * index);
        }
        for index in 0..4 {
            if index <= chunk.len() {
                let digit = (value >> (18 - 6 * index)) & 63;
                out.push(BASE64[digit as usize] as char);
            } else {
                out.push('=');
            }
        }
        line += 4;
        if line >= 76 {
            out.push_str("\r\n");
            line = 0;
        }
    }
    if line > 0 {
        out.push_str("\r\n");
    }
    out
}

#[must_use]
pub fn decode_base64(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    let mut value: u32 = 0;
    let mut held = 0;
    for &byte in bytes {
        let digit = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => continue,
        };
        value = (value << 6) | u32::from(digit);
        held += 6;
        if held >= 8 {
            held -= 8;
            out.push((value >> held) as u8);
            value &= (1 << held) - 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_goes_both_ways() {
        for text in ["", "a", "ab", "abc", "abcd", "Hello, world!"] {
            let encoded = encode_base64(text.as_bytes());
            assert_eq!(decode_base64(encoded.as_bytes()), text.as_bytes(), "{text}");
        }
        assert_eq!(encode_base64(b"Man").trim(), "TWFu");
        assert_eq!(encode_base64(b"Ma").trim(), "TWE=");
        let long: Vec<u8> = (0..200).map(|i| i as u8).collect();
        let encoded = encode_base64(&long);
        assert!(encoded.lines().all(|line| line.len() <= 76));
        assert_eq!(decode_base64(encoded.as_bytes()), long);
    }

    #[test]
    fn quoted_printable_goes_both_ways() {
        let text = "caf\u{e9} = 3\nA long line that goes on and on and on and on and on and on and on and on and on and on\n";
        let encoded = encode_quoted_printable(text.as_bytes());
        assert!(encoded.contains("=C3=A9 =3D 3"), "{encoded}");
        assert!(encoded.lines().all(|line| line.len() <= 76), "{encoded}");
        assert_eq!(
            decode_quoted_printable(encoded.as_bytes()),
            text.replace('\n', "\r\n").as_bytes()
        );
    }

    #[test]
    fn a_message_is_cut_into_its_parts() {
        let message = b"MIME-Version: 1.0\r\nContent-Type: multipart/related;\r\n\tboundary=\"----=_NextPart_01\";\r\n\ttype=\"text/html\"\r\n\r\nThis is a multi-part message in MIME format.\r\n\r\n------=_NextPart_01\r\nContent-Location: file:///C:/doc.htm\r\nContent-Transfer-Encoding: quoted-printable\r\nContent-Type: text/html; charset=\"windows-1252\"\r\n\r\n<html><body>caf=E9</body></html>\r\n\r\n------=_NextPart_01\r\nContent-Location: file:///C:/doc_files/image001.png\r\nContent-Transfer-Encoding: base64\r\nContent-Type: image/png\r\n\r\nTWFu\r\n\r\n------=_NextPart_01--\r\n";
        let parts = parts(message);
        assert_eq!(parts.len(), 2, "{parts:?}");
        assert_eq!(parts[0].content_type, "text/html");
        assert_eq!(parts[0].charset.as_deref(), Some("windows-1252"));
        assert_eq!(parts[0].location.as_deref(), Some("file:///C:/doc.htm"));
        // The blank line before the boundary is the part's own last line end.
        assert_eq!(parts[0].bytes, b"<html><body>caf\xE9</body></html>\r\n");
        assert_eq!(parts[1].content_type, "image/png");
        assert_eq!(parts[1].bytes, b"Man");
    }
}
