//! Word's own clipboard format: the copy as a Word document of its own.
//!
//! # What Word puts on the clipboard beside the rest
//!
//! Besides the text, Rich Text and HTML, Word offers what was copied as an
//! object — "Embed Source" — which is what its Paste Special calls a
//! Microsoft Word Document Object, together with an "Object Descriptor"
//! that says whose object it is and what to call it. The object is OLE's:
//! a compound file whose root names the class of object it holds, with the
//! two little streams every embedded object carries — `\x01CompObj`, which
//! names it in words, and `\x01Ole` — and, since Word 2007, the document
//! itself as a `.docx` package in a stream called `Package`. So the copied
//! stretch goes as a whole document, with its styles, numbering and
//! everything else the package holds, which is more than Rich Text or HTML
//! can carry.
//!
//! # What is done with it here
//!
//! The copy goes out that way too, from the package the copied stretch is
//! already made into for the other formats, so Word's Paste Special
//! offers it as Word's own. And a paste takes the package out of one Word
//! put there, which is the fullest account of what was copied that is on
//! the clipboard at all.
//!
//! The layouts are [MS-CFB] for the file, [MS-OLEDS] for the two streams,
//! and `OBJECTDESCRIPTOR` in `oleidl.h` for the descriptor.

/// `Word.Document.12`, the class of a Word document since Word 2007:
/// `{F4754C9B-64F5-4B40-8AF4-679732AC0607}`, in the order its bytes are
/// kept — the first three parts turned little-endian.
pub(crate) const WORD_DOCUMENT: [u8; 16] = [
    0x9B, 0x4C, 0x75, 0xF4, 0xF5, 0x64, 0x40, 0x4B, 0x8A, 0xF4, 0x67, 0x97, 0x32, 0xAC, 0x06, 0x07,
];

/// What the object is called, in words.
const USER_TYPE: &str = "Microsoft Word Document";
/// And by the name the registry knows it.
const PROGRAM_ID: &str = "Word.Document.12";
/// The clipboard format Word names for its own documents.
const CLIPBOARD_NAME: &str = "MSWordDocx";

/// `DVASPECT_CONTENT`: the object as its content rather than an icon.
const ASPECT_CONTENT: u32 = 1;

/// The copy as "Embed Source": a compound file holding the package, said to
/// be a Word document.
pub(crate) fn embed_source(package: &[u8]) -> Vec<u8> {
    let mut builder = wp_ole::Builder::new();
    builder
        .class(WORD_DOCUMENT)
        .stream("\u{1}CompObj", comp_obj())
        .stream("\u{1}Ole", ole_stream())
        .stream("Package", package.to_vec());
    builder.build()
}

/// The `\x01CompObj` stream: who the object is, in words, by clipboard
/// format and by program.
fn comp_obj() -> Vec<u8> {
    let mut out = Vec::new();
    // The header: a mark, the version, and — where the format asks for
    // nothing in particular — what Word writes, the class again.
    out.extend_from_slice(&0xFFFE_0001u32.to_le_bytes());
    out.extend_from_slice(&0x0000_0A03u32.to_le_bytes());
    out.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    out.extend_from_slice(&WORD_DOCUMENT);
    for text in [USER_TYPE, CLIPBOARD_NAME, PROGRAM_ID] {
        ansi_string(&mut out, text);
    }
    // The mark that says the Unicode half follows, and that half empty,
    // which a reader takes as "as above".
    out.extend_from_slice(&0x71B2_39F4u32.to_le_bytes());
    out.extend_from_slice(&[0; 12]);
    out
}

/// A string as [MS-OLEDS] writes an ANSI one: its length with the nought,
/// then it and the nought.
fn ansi_string(out: &mut Vec<u8>, text: &str) {
    out.extend_from_slice(&(text.len() as u32 + 1).to_le_bytes());
    out.extend_from_slice(text.as_bytes());
    out.push(0);
}

/// The `\x01Ole` stream of an object embedded rather than linked: the
/// version, and noughts for everything a link would say.
fn ole_stream() -> Vec<u8> {
    let mut out = 0x0200_0001u32.to_le_bytes().to_vec();
    out.extend_from_slice(&[0; 16]);
    out
}

/// The "Object Descriptor" that goes beside it: whose object it is, what
/// to call it, and where it came from.
pub(crate) fn object_descriptor(source: &str) -> Vec<u8> {
    const FIXED: usize = 52;
    let name: Vec<u8> = wide_with_nought(USER_TYPE);
    let from: Vec<u8> = wide_with_nought(source);
    let size = (FIXED + name.len() + from.len()) as u32;
    let mut out = Vec::with_capacity(size as usize);
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&WORD_DOCUMENT);
    out.extend_from_slice(&ASPECT_CONTENT.to_le_bytes());
    // Its size and the point it was taken at, which a document of text
    // has neither of.
    out.extend_from_slice(&[0; 16]);
    // Nothing unusual about how it behaves.
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&(FIXED as u32).to_le_bytes());
    out.extend_from_slice(&((FIXED + name.len()) as u32).to_le_bytes());
    out.extend_from_slice(&name);
    out.extend_from_slice(&from);
    out
}

fn wide_with_nought(text: &str) -> Vec<u8> {
    text.encode_utf16().chain([0]).flat_map(u16::to_le_bytes).collect()
}

/// The Word document in an "Embed Source", if it holds one: a compound
/// file said to be a Word document, or named one in its `\x01CompObj`,
/// with a `.docx` package in its `Package` stream. Nothing for any other
/// program's object.
pub(crate) fn package_of(embed_source: &[u8]) -> Option<Vec<u8>> {
    let file = wp_ole::CompoundFile::open(embed_source.to_vec()).ok()?;
    let root = file.entries().first()?;
    let named_word = file.stream("\u{1}CompObj").is_some_and(|stream| {
        stream.windows(b"Word.Document.".len()).any(|window| window == b"Word.Document.")
    });
    if root.class != WORD_DOCUMENT && !named_word {
        return None;
    }
    let package = file.stream("Package")?;
    package.starts_with(b"PK\x03\x04").then_some(package)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_copy_is_a_word_document_object_holding_the_package() {
        let package = b"PK\x03\x04 the rest of a package".to_vec();
        let bytes = embed_source(&package);
        let file = wp_ole::CompoundFile::open(bytes.clone()).expect("a compound file");
        assert_eq!(file.entries()[0].class, WORD_DOCUMENT);
        assert_eq!(file.walk(&["Package"]), Some(package.clone()));
        let comp_obj = file.walk(&["\u{1}CompObj"]).expect("the object's names");
        assert_eq!(&comp_obj[..4], &[0x01, 0x00, 0xFE, 0xFF]);
        assert_eq!(&comp_obj[12..28], &WORD_DOCUMENT);
        let words = String::from_utf8_lossy(&comp_obj);
        assert!(words.contains("Microsoft Word Document") && words.contains("Word.Document.12"));
        assert_eq!(file.walk(&["\u{1}Ole"]).map(|stream| stream.len()), Some(20));
        // And a paste takes it out again.
        assert_eq!(package_of(&bytes), Some(package));
    }

    #[test]
    fn another_program_s_object_is_not_taken_for_a_document() {
        let mut builder = wp_ole::Builder::new();
        builder.class([1; 16]).stream("Package", b"PK\x03\x04 a spreadsheet".to_vec());
        assert_eq!(package_of(&builder.build()), None);
        assert_eq!(package_of(b"not a compound file"), None);
        // A Word object whose package is not one is not either.
        let mut builder = wp_ole::Builder::new();
        builder.class(WORD_DOCUMENT).stream("Package", b"garbage".to_vec());
        assert_eq!(package_of(&builder.build()), None);
    }

    #[test]
    fn the_descriptor_is_laid_out_as_the_structure_is() {
        let bytes = object_descriptor("Word Processor");
        let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().expect("four"));
        assert_eq!(u32_at(0) as usize, bytes.len(), "its size is its length");
        assert_eq!(&bytes[4..20], &WORD_DOCUMENT);
        assert_eq!(u32_at(20), ASPECT_CONTENT);
        let text_at = |at: usize| {
            let units: Vec<u16> = bytes[at..]
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .take_while(|unit| *unit != 0)
                .collect();
            String::from_utf16_lossy(&units)
        };
        assert_eq!(text_at(u32_at(44) as usize), "Microsoft Word Document");
        assert_eq!(text_at(u32_at(48) as usize), "Word Processor");
    }
}
