//! The shape of a PDF file: numbered objects, and a table saying where each one
//! begins.
//!
//! A PDF is not a stream to be read from the front. It is a heap of numbered
//! objects with an index at the end, and a reader starts at the end, reads the
//! index, and jumps to whatever it needs. That is why a PDF can be opened at
//! page nine hundred without reading the first eight hundred and ninety-nine,
//! and it is the whole of the file format's structure: everything else is what
//! the objects say.
//!
//! This writes them: hand it objects, it hands back the bytes.

/// A file being written.
#[derive(Debug, Default)]
pub(crate) struct Writer {
    bytes: Vec<u8>,
    /// Where each object begins, by its number. The zeroth is the free-list
    /// head the format insists on and nothing else.
    offsets: Vec<usize>,
}

/// A number given out for an object that has not been written yet.
///
/// A PDF is full of forward references — a page names its contents, which are
/// written afterwards — so the number has to be handed out before the object
/// exists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Id(pub u32);

impl Id {
    /// How the object is referred to from inside another one.
    pub(crate) fn reference(self) -> String {
        format!("{} 0 R", self.0)
    }
}

impl Writer {
    pub(crate) fn new() -> Self {
        let mut writer = Self { bytes: Vec::new(), offsets: vec![0] };
        // Every PDF says which version it is, and then a line of high bytes so
        // that anything moving the file about treats it as binary.
        writer.bytes.extend_from_slice(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n");
        writer
    }

    /// Takes the next object number without writing anything at it yet.
    pub(crate) fn reserve(&mut self) -> Id {
        self.offsets.push(0);
        Id(self.offsets.len() as u32 - 1)
    }

    /// Writes an object at a number already taken.
    pub(crate) fn put(&mut self, id: Id, body: &str) {
        self.begin(id);
        self.bytes.extend_from_slice(body.as_bytes());
        self.end();
    }

    /// Writes an object and hands back its number.
    pub(crate) fn add(&mut self, body: &str) -> Id {
        let id = self.reserve();
        self.put(id, body);
        id
    }

    /// Writes a stream: a dictionary, then the bytes it describes.
    ///
    /// The data is deflated, which every PDF reader can undo and which turns a
    /// page of text from tens of kilobytes into a few.
    pub(crate) fn put_stream(&mut self, id: Id, dictionary: &str, data: &[u8]) {
        let packed = wp_deflate::compress_zlib(data);
        self.begin(id);
        self.bytes.extend_from_slice(b"<< ");
        self.bytes.extend_from_slice(dictionary.as_bytes());
        self.bytes.extend_from_slice(
            format!(" /Filter /FlateDecode /Length {} >>\nstream\n", packed.len()).as_bytes(),
        );
        self.bytes.extend_from_slice(&packed);
        self.bytes.extend_from_slice(b"\nendstream");
        self.end();
    }

    /// The same, returning a fresh number.
    pub(crate) fn add_stream(&mut self, dictionary: &str, data: &[u8]) -> Id {
        let id = self.reserve();
        self.put_stream(id, dictionary, data);
        id
    }

    /// Finishes the file: the table of where everything is, and the trailer
    /// that points at the table.
    pub(crate) fn finish(mut self, catalogue: Id, information: Id) -> Vec<u8> {
        let start = self.bytes.len();
        let count = self.offsets.len();

        self.bytes.extend_from_slice(format!("xref\n0 {count}\n").as_bytes());
        // The zeroth entry is the head of the list of free objects, and in a
        // file written in one go there are none.
        self.bytes.extend_from_slice(b"0000000000 65535 f \n");
        for offset in self.offsets.iter().skip(1) {
            self.bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }

        self.bytes.extend_from_slice(
            format!(
                "trailer\n<< /Size {count} /Root {} /Info {} >>\nstartxref\n{start}\n%%EOF\n",
                catalogue.reference(),
                information.reference()
            )
            .as_bytes(),
        );
        self.bytes
    }

    fn begin(&mut self, id: Id) {
        self.offsets[id.0 as usize] = self.bytes.len();
        self.bytes.extend_from_slice(format!("{} 0 obj\n", id.0).as_bytes());
    }

    fn end(&mut self) {
        self.bytes.extend_from_slice(b"\nendobj\n");
    }
}

/// A string as a PDF holds one: in brackets, with the three characters that
/// mean something to the format escaped.
pub(crate) fn text_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('(');
    for character in text.chars() {
        match character {
            '(' | ')' | '\\' => {
                out.push('\\');
                out.push(character);
            }
            // Anything outside Latin-1 cannot be written this way at all; the
            // few places this is used are titles and dates, and a reader shows
            // a question mark rather than refusing the file.
            character if (character as u32) < 256 => out.push(character),
            _ => out.push('?'),
        }
    }
    out.push(')');
    out
}

/// A number as a PDF wants it: no exponent, and no more figures than matter.
pub(crate) fn number(value: f32) -> String {
    if value == value.trunc() && value.abs() < 1e9 {
        return format!("{}", value as i64);
    }
    let mut text = format!("{value:.3}");
    while text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::{number, text_string, Writer};

    #[test]
    fn a_file_begins_with_the_version_and_ends_with_the_marker() {
        let mut writer = Writer::new();
        let catalogue = writer.add("<< /Type /Catalog >>");
        let information = writer.add("<< /Producer (Word Processor) >>");
        let bytes = writer.finish(catalogue, information);

        assert!(bytes.starts_with(b"%PDF-1.7"));
        assert!(bytes.ends_with(b"%%EOF\n"));
    }

    /// Where a run of bytes begins, if it is there at all.
    fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack.windows(needle.len()).position(|window| window == needle)
    }

    #[test]
    fn the_table_says_where_every_object_really_is() {
        let mut writer = Writer::new();
        let first = writer.add("<< /A 1 >>");
        let second = writer.add("<< /B 2 >>");
        let bytes = writer.finish(first, second);

        // Read as bytes throughout: the file begins with a line of high bytes
        // that is not text at all, and reading it as text would move every
        // offset after it.
        let table = find(&bytes, b"xref\n").expect("a table") + "xref\n0 3\n".len();
        for (number, marker) in [(0usize, "1 0 obj"), (1, "2 0 obj")] {
            // Each entry is exactly twenty bytes: ten of offset, five of
            // generation, and the rest is punctuation.
            let entry = table + 20 + number * 20;
            let offset: usize =
                String::from_utf8_lossy(&bytes[entry..entry + 10]).parse().expect("an offset");
            assert!(bytes[offset..].starts_with(marker.as_bytes()), "not at {offset}");
        }
    }

    #[test]
    fn a_stream_says_how_long_it_is() {
        let mut writer = Writer::new();
        let stream = writer.add_stream("/Type /Test", b"some content");
        let catalogue = writer.add("<< /Type /Catalog >>");
        let bytes = writer.finish(catalogue, stream);

        let at = find(&bytes, b"/Length ").expect("a length") + "/Length ".len();
        let digits: String = String::from_utf8_lossy(&bytes[at..at + 10])
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        let length: usize = digits.parse().expect("a number");
        let start = find(&bytes, b"stream\n").expect("the stream") + "stream\n".len();
        assert_eq!(&bytes[start + length..start + length + 10], b"\nendstream");
    }

    #[test]
    fn a_stream_can_be_read_back_out_again() {
        let mut writer = Writer::new();
        writer.add_stream("/Type /Test", b"the quick brown fox");
        let bytes = writer.finish(super::Id(1), super::Id(1));

        let start = find(&bytes, b"stream\n").expect("the stream") + "stream\n".len();
        let end = find(&bytes, b"\nendstream").expect("the end");
        let unpacked = wp_deflate::inflate_zlib(&bytes[start..end], 4096).expect("readable");
        assert_eq!(unpacked, b"the quick brown fox");
    }

    #[test]
    fn the_characters_a_pdf_string_cannot_hold_are_escaped() {
        assert_eq!(text_string("plain"), "(plain)");
        assert_eq!(text_string("a (b) c"), "(a \\(b\\) c)");
        assert_eq!(text_string("back\\slash"), "(back\\\\slash)");
    }

    #[test]
    fn numbers_are_written_without_an_exponent_or_a_tail_of_zeroes() {
        assert_eq!(number(1.0), "1");
        assert_eq!(number(-2.5), "-2.5");
        assert_eq!(number(0.000_001), "0");
        assert_eq!(number(1_234.567_9), "1234.568");
    }
}
