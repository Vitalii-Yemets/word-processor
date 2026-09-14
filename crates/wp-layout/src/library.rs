//! Finding the fonts installed on the machine.
//!
//! No font is shipped with this program. Typefaces are data with their own
//! licences — Times New Roman and Calibri belong to Microsoft — so a document
//! asking for one is drawn with whatever the machine actually has, which is what
//! Word does too.
//!
//! Two kinds of substitution happen here, and they are different things:
//!
//! * **Family fallback.** The document asks for a family that is not installed,
//!   so a similar one is used instead.
//! * **Character fallback.** The chosen font simply has no glyph for a
//!   character — a Latin font asked to draw Chinese — so that one character is
//!   taken from another font. Without this, whole scripts come out as empty
//!   boxes even when the machine has a font that covers them.

use std::cell::OnceCell;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use wp_font::{CharacterMap, Font, GlyphId, TableRange};

/// One font face that can be drawn with.
#[derive(Debug)]
pub struct Face {
    /// The family the font declares, such as "DejaVu Sans".
    pub family: String,
    pub bold: bool,
    pub italic: bool,
    /// Where it came from, for diagnostics.
    pub path: PathBuf,
    /// Which font within the file, for collections.
    pub index: u32,
    /// The file, read the first time this face is actually drawn with.
    ///
    /// A machine can have hundreds of font files, and scanning them all is not
    /// a reason to keep them all. Loading eagerly cost six hundred megabytes to
    /// show one page; a document uses a handful of faces, and only those are
    /// held.
    data: OnceCell<Vec<u8>>,
    /// Where the `cmap` table sits, so coverage can be checked without reading
    /// the whole file.
    cmap: Option<TableRange>,
    /// Which characters this face can draw, read from that table alone.
    coverage: OnceCell<CharacterMap>,
    /// Where on its axes this face sits, for a variable font: a file that is a
    /// whole family appears here once per place the designer named.
    variations: Option<Vec<f32>>,
    /// Whether this face draws in colours of its own — an emoji font — which
    /// is what decides where an emoji is taken from.
    colour: bool,
}

impl Face {
    /// Parses the face so it can be measured and drawn.
    ///
    /// The file is read on first use and then kept, so the cost is paid once
    /// per face rather than once per glyph.
    #[must_use]
    pub fn font(&self) -> Option<Font<'_>> {
        let font = Font::parse_index(self.bytes()?, self.index).ok()?;
        Some(match &self.variations {
            Some(coordinates) => font.varied(coordinates),
            None => font,
        })
    }

    /// Which characters this face covers.
    ///
    /// Answered from the character mapping table alone. Deciding which font can
    /// draw a Chinese character means asking every face on the machine, and
    /// loading each whole file to ask is what made showing one page cost half a
    /// gigabyte.
    #[must_use]
    pub fn coverage(&self) -> Option<&CharacterMap> {
        if self.coverage.get().is_none() {
            let range = self.cmap?;
            let mut file = File::open(&self.path).ok()?;
            let table = read_at(&mut file, range.offset as u64, range.length)?;
            let map = wp_font::character_map_from_table(&table).ok()?;
            let _ = self.coverage.set(map);
        }
        self.coverage.get()
    }

    /// The file this face was read from, whole.
    ///
    /// For a program that has to hand the font on rather than draw with it: a
    /// PDF embeds the font itself, and a kind of font it cannot cut down it
    /// embeds entire.
    #[must_use]
    pub fn file(&self) -> Option<&[u8]> {
        self.bytes().map(Vec::as_slice)
    }

    /// The file contents, read if they are not already in memory.
    fn bytes(&self) -> Option<&Vec<u8>> {
        if self.data.get().is_none() {
            if let Ok(bytes) = std::fs::read(&self.path) {
                // Only fails if another call won the race, which cannot happen
                // here: a library belongs to one thread.
                let _ = self.data.set(bytes);
            }
        }
        self.data.get()
    }
}

/// Every usable font found on the machine.
#[derive(Debug, Default)]
pub struct FontLibrary {
    faces: Vec<Face>,
}

/// How large a font file may be before it is skipped.
///
/// A guard against a scan pulling something enormous into memory; the largest
/// real font is a small fraction of this.
const MAX_FONT_BYTES: u64 = 64 * 1024 * 1024;

impl FontLibrary {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads every font in the places this operating system keeps them.
    #[must_use]
    pub fn scan_system() -> Self {
        let mut library = Self::new();
        for directory in system_font_directories() {
            library.scan_directory(&directory);
        }
        library
    }

    /// Loads every font under a directory, including subdirectories.
    pub fn scan_directory(&mut self, directory: &Path) {
        // Depth-first with an explicit stack: a font directory can contain
        // symbolic links back to itself, and recursion would not survive that.
        let mut pending = vec![directory.to_path_buf()];
        let mut visited = 0usize;

        while let Some(current) = pending.pop() {
            visited += 1;
            if visited > 4096 {
                break;
            }
            let Ok(entries) = std::fs::read_dir(&current) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                match entry.file_type() {
                    Ok(kind) if kind.is_dir() => pending.push(path),
                    Ok(_) => {
                        self.add_file(&path);
                    }
                    Err(_) => {}
                }
            }
        }
    }

    /// Loads one font file, which may contain several faces.
    ///
    /// Returns how many faces were added, which is zero for a file this program
    /// cannot read. That is not reported as an error: a font directory routinely
    /// holds formats it does not handle, and the user does not need to hear
    /// about every one of them.
    pub fn add_file(&mut self, path: &Path) -> usize {
        let extension =
            path.extension().and_then(|value| value.to_str()).map(str::to_ascii_lowercase);
        if !matches!(extension.as_deref(), Some("ttf" | "ttc" | "otf" | "otc")) {
            return 0;
        }

        let Ok(metadata) = std::fs::metadata(path) else {
            return 0;
        };
        if metadata.len() > MAX_FONT_BYTES {
            return 0;
        }

        let mut added = 0;
        for description in describe_faces(path) {
            self.faces.push(Face {
                family: description.family,
                bold: description.bold,
                italic: description.italic,
                path: path.to_path_buf(),
                index: description.index,
                // The bytes are deliberately not kept: this face may never be
                // drawn with, and the scan visits every font on the machine.
                data: OnceCell::new(),
                cmap: description.cmap,
                coverage: OnceCell::new(),
                variations: description.variations,
                colour: description.colour,
            });
            added += 1;
        }

        added
    }

    #[must_use]
    pub fn faces(&self) -> &[Face] {
        &self.faces
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }

    /// The families available, sorted and without duplicates.
    #[must_use]
    pub fn families(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.faces.iter().map(|face| face.family.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        names
    }

    /// Chooses a face for a request.
    ///
    /// The weight and slant are matched as well as the family, and a missing
    /// family falls back rather than failing: a document must still be readable
    /// on a machine that does not have the typeface it was written with.
    #[must_use]
    pub fn select(&self, family: Option<&str>, bold: bool, italic: bool) -> Option<usize> {
        if self.faces.is_empty() {
            return None;
        }

        let wanted = family.map(normalize);
        let matches_family = |face: &Face| match &wanted {
            Some(wanted) => normalize(&face.family) == *wanted,
            None => false,
        };

        // An exact match on family, weight and slant is the ideal.
        if let Some(index) = self
            .faces
            .iter()
            .position(|face| matches_family(face) && face.bold == bold && face.italic == italic)
        {
            return Some(index);
        }
        // The right family in the wrong weight still looks like the document.
        if let Some(index) = self.faces.iter().position(matches_family) {
            return Some(index);
        }

        self.default_face(bold, italic)
    }

    /// A reasonable face when the document's own choice is unavailable.
    #[must_use]
    pub fn default_face(&self, bold: bool, italic: bool) -> Option<usize> {
        // Families likely to be present and to cover a broad range of scripts.
        const PREFERRED: &[&str] = &[
            "dejavusans",
            "liberationsans",
            "notosans",
            "arial",
            "helvetica",
            "segoeui",
            "calibri",
            "freesans",
        ];

        for wanted in PREFERRED {
            if let Some(index) = self.faces.iter().position(|face| {
                normalize(&face.family) == *wanted && face.bold == bold && face.italic == italic
            }) {
                return Some(index);
            }
            if let Some(index) =
                self.faces.iter().position(|face| normalize(&face.family) == *wanted)
            {
                return Some(index);
            }
        }

        // Anything at all is better than drawing nothing.
        self.faces.iter().position(|face| face.bold == bold && face.italic == italic).or(Some(0))
    }

    /// Finds a face that can draw a character the chosen one cannot.
    ///
    /// Without this, a document mixing Latin and Chinese shows empty boxes for
    /// half of itself even when the machine has a font covering both.
    #[must_use]
    pub fn fallback_for(
        &self,
        character: char,
        bold: bool,
        italic: bool,
    ) -> Option<(usize, GlyphId)> {
        // Prefer a face matching the requested weight and slant, then any.
        for require_style in [true, false] {
            for (index, face) in self.faces.iter().enumerate() {
                if require_style && (face.bold != bold || face.italic != italic) {
                    continue;
                }
                let Some(coverage) = face.coverage() else {
                    continue;
                };
                if let Some(glyph) = coverage.glyph_for(character) {
                    return Some((index, glyph));
                }
            }
        }
        None
    }

    /// A face that draws this character in colours of its own, if the machine
    /// has one.
    ///
    /// What makes an emoji an emoji. A Latin font holds monochrome outlines for
    /// a good many of them, and a document that took those would show a rocket
    /// as a black shape — which is not what the writer meant and not what Word
    /// shows.
    #[must_use]
    pub fn colour_face_for(&self, character: char) -> Option<(usize, GlyphId)> {
        self.faces.iter().enumerate().filter(|(_, face)| face.colour).find_map(|(index, face)| {
            face.coverage()?.glyph_for(character).map(|glyph| (index, glyph))
        })
    }

    /// Whether anything on this machine can draw a character.
    ///
    /// What a grid of symbols has to know before it draws a cell: a cell that
    /// comes out as an empty box is worse than no cell.
    #[must_use]
    pub fn can_draw(&self, character: char) -> bool {
        self.faces
            .iter()
            .filter_map(Face::coverage)
            .any(|coverage| coverage.glyph_for(character).is_some())
    }

    /// The face at an index.
    #[must_use]
    pub fn face(&self, index: usize) -> Option<&Face> {
        self.faces.get(index)
    }
}

/// What the catalogue needs to know about one face.
struct Description {
    index: u32,
    family: String,
    bold: bool,
    italic: bool,
    cmap: Option<TableRange>,
    /// Where on its axes this face sits, for a variable font. `None` for an
    /// ordinary one, which has nowhere to sit.
    variations: Option<Vec<f32>>,
    colour: bool,
}

/// The faces a variable font holds: one for each place on its axes the
/// designer named.
///
/// # Why a file becomes several faces
///
/// Because that is what a font menu shows and what a document names. A
/// variable font is one file holding every weight from Thin to Black, and a
/// document asking for "Inter SemiBold" is asking for a place on an axis, not
/// for a file. Word lists them the same way.
///
/// The four names that fit the ordinary styles — Regular, Bold, Italic, Bold
/// Italic — become the family itself with those flags set, because that is how
/// a document asks for them. Every other name becomes a family of its own,
/// which is how a font menu lists it.
fn instance_faces(
    family: &str,
    bold: bool,
    italic: bool,
    instances: &[wp_font::Instance],
) -> Vec<(String, bool, bool, Option<Vec<f32>>)> {
    let mut out = Vec::with_capacity(instances.len());
    for instance in instances {
        let Some(name) = instance.name.as_deref() else { continue };

        // The style is read off the name rather than off the file, because the
        // file says what its default instance is and this is not that. A font
        // with a slant axis is upright where it stands and holds an instance
        // called Italic.
        let mut words: Vec<&str> = name.split_whitespace().collect();
        let slanted = words.iter().any(|word| {
            word.eq_ignore_ascii_case("italic") || word.eq_ignore_ascii_case("oblique")
        });
        words.retain(|word| {
            !word.eq_ignore_ascii_case("italic")
                && !word.eq_ignore_ascii_case("oblique")
                && !word.eq_ignore_ascii_case("regular")
        });
        let heavy = words.len() == 1 && words[0].eq_ignore_ascii_case("bold");
        if heavy {
            words.clear();
        }

        let named = if words.is_empty() {
            family.to_owned()
        } else {
            format!("{family} {}", words.join(" "))
        };
        out.push((named, bold || heavy, italic || slanted, Some(instance.coordinates.clone())));
    }
    out
}

/// Reads just enough of a font file to catalogue the faces in it.
///
/// Reading each file whole for one string cost hundreds of megabytes across a
/// machine's font directory. Only the table directory and the two tables that
/// carry the name and the style are read, which is a few kilobytes per file.
fn describe_faces(path: &Path) -> Vec<Description> {
    let Ok(mut file) = File::open(path) else {
        return Vec::new();
    };

    // Enough for the collection header and a large table directory.
    let Some(header) = read_at(&mut file, 0, 4096) else {
        return Vec::new();
    };
    let Ok(offsets) = wp_font::collection_offsets(&header) else {
        return Vec::new();
    };

    let mut descriptions = Vec::new();
    for (index, start) in offsets.iter().enumerate().take(64) {
        let Some(directory_header) = read_at(&mut file, *start as u64, 4096) else {
            continue;
        };
        let Ok(tables) = wp_font::table_directory(&directory_header) else {
            continue;
        };

        let find = |wanted: &[u8; 4]| {
            tables.iter().find(|(tag, _)| tag == wanted).map(|(_, range)| *range)
        };

        // A face nothing can be drawn from is no use. Outlines live in one of
        // two tables — quadratic curves in `glyf`, PostScript ones in `CFF` —
        // and a font has one or the other. A colour font may have neither and
        // still draw: an emoji font of pictures is all picture.
        let quadratic = find(b"glyf").is_some() && find(b"loca").is_some();
        let postscript = find(b"CFF ").is_some() || find(b"CFF2").is_some();
        let pictures = find(b"CBDT").is_some() && find(b"CBLC").is_some();
        if !quadratic && !postscript && !pictures {
            continue;
        }

        let Some(name_range) = find(b"name") else {
            continue;
        };
        let Some(name_table) = read_at(&mut file, name_range.offset as u64, name_range.length)
        else {
            continue;
        };
        let Some(family) = wp_font::family_from_name_table(&name_table) else {
            continue;
        };

        // The style flags sit at a fixed place in OS/2; two bytes are enough.
        let (mut bold, mut italic) = (false, false);
        if let Some(os2) = find(b"OS/2") {
            if let Some(bytes) = read_at(&mut file, os2.offset as u64 + 62, 2) {
                if bytes.len() == 2 {
                    let selection = u16::from_be_bytes([bytes[0], bytes[1]]);
                    italic = selection & 0x0001 != 0;
                    bold = selection & 0x0020 != 0;
                }
            }
            if let Some(bytes) = read_at(&mut file, os2.offset as u64 + 4, 2) {
                if bytes.len() == 2 {
                    bold = bold || u16::from_be_bytes([bytes[0], bytes[1]]) >= 600;
                }
            }
        }

        // A variable font is a family in one file: every place on its axes
        // the designer named becomes a face here, because that is what a
        // document asks for and what a font menu lists.
        let cmap = find(b"cmap");
        let colour =
            find(b"COLR").is_some() || (find(b"CBDT").is_some() && find(b"CBLC").is_some());
        let instances = find(b"fvar")
            .and_then(|range| read_at(&mut file, range.offset as u64, range.length))
            .map(|fvar| wp_font::variations_from_tables(&fvar, &name_table).1)
            .unwrap_or_default();

        if instances.is_empty() {
            descriptions.push(Description {
                index: index as u32,
                family,
                bold,
                italic,
                cmap,
                variations: None,
                colour,
            });
            continue;
        }
        for (family, bold, italic, variations) in instance_faces(&family, bold, italic, &instances)
        {
            descriptions.push(Description {
                index: index as u32,
                family,
                bold,
                italic,
                cmap,
                variations,
                colour,
            });
        }
    }

    descriptions
}

/// Reads a stretch of a file, returning as much of it as exists.
fn read_at(file: &mut File, offset: u64, length: usize) -> Option<Vec<u8>> {
    // A malformed font can claim a table larger than any real one.
    const MAX_READ: usize = 1 << 20;
    if length == 0 || length > MAX_READ {
        return None;
    }

    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut buffer = vec![0u8; length];
    let mut filled = 0usize;
    while filled < length {
        match file.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(count) => filled += count,
            Err(_) => return None,
        }
    }
    buffer.truncate(filled);
    if buffer.is_empty() {
        None
    } else {
        Some(buffer)
    }
}

/// Reduces a family name to something comparable: lower case, no spaces or
/// punctuation, so "Times New Roman" and "TimesNewRoman" are the same request.
fn normalize(name: &str) -> String {
    name.chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Where this operating system keeps its fonts.
fn system_font_directories() -> Vec<PathBuf> {
    let mut directories = Vec::new();

    if cfg!(windows) {
        if let Ok(windows) = std::env::var("SystemRoot") {
            directories.push(PathBuf::from(windows).join("Fonts"));
        } else {
            directories.push(PathBuf::from("C:\\Windows\\Fonts"));
        }
        // Fonts a user installed without administrator rights live separately.
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            directories.push(PathBuf::from(local).join("Microsoft").join("Windows").join("Fonts"));
        }
    } else {
        directories.push(PathBuf::from("/usr/share/fonts"));
        directories.push(PathBuf::from("/usr/local/share/fonts"));
        if let Ok(home) = std::env::var("HOME") {
            directories.push(PathBuf::from(&home).join(".fonts"));
            directories.push(PathBuf::from(&home).join(".local/share/fonts"));
        }
        // macOS, in case this ever runs there.
        directories.push(PathBuf::from("/System/Library/Fonts"));
        directories.push(PathBuf::from("/Library/Fonts"));
    }

    directories.into_iter().filter(|directory| directory.is_dir()).collect()
}

/// The fonts of this machine, offered to a metafile being played back.
///
/// A metafile names a face the way the program that recorded it knew it, and
/// what is actually here is this library's business. The nearest face is what
/// [`FontLibrary::select`] already answers for a document's own text, so a
/// metafile gets the same answer a paragraph would.
impl wp_image::metafile::Faces for FontLibrary {
    fn face(&self, family: &str, bold: bool, italic: bool) -> Option<wp_font::Font<'_>> {
        let wanted = (!family.is_empty()).then_some(family);
        let index =
            self.select(wanted, bold, italic).or_else(|| self.default_face(bold, italic))?;
        self.face(index)?.font()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn family_names_compare_without_spaces_or_case() {
        assert_eq!(normalize("Times New Roman"), normalize("timesnewroman"));
        assert_eq!(normalize("DejaVu Sans"), normalize("dejavusans"));
        assert_ne!(normalize("Arial"), normalize("Arial Black"));
    }

    #[test]
    fn an_empty_library_selects_nothing() {
        let library = FontLibrary::new();
        assert_eq!(library.select(Some("Arial"), false, false), None);
        assert!(library.families().is_empty());
    }

    #[test]
    fn a_file_that_is_not_a_font_is_skipped() {
        let mut library = FontLibrary::new();
        assert_eq!(library.add_file(Path::new("Cargo.toml")), 0);
        assert_eq!(library.add_file(Path::new("does-not-exist.ttf")), 0);
        assert!(library.is_empty());
    }

    /// Every weight of a variable font is a family a document can ask for.
    ///
    /// Which is the point of cataloguing them separately: a document written in
    /// Inter SemiBold names a place on an axis, and without this it would get
    /// whichever weight the designer happened to draw first.
    #[test]
    fn the_weights_of_a_variable_font_are_families_of_their_own() {
        let mut library = FontLibrary::new();
        let added =
            library.add_file(Path::new("/usr/share/fonts/truetype/inter-vf/Inter-roman.var.ttf"));
        if added == 0 {
            // The build image should have it; a machine without it has nothing
            // to say here rather than a failure to report.
            return;
        }

        let families = library.families();
        assert!(families.contains(&"Inter"), "{families:?}");
        assert!(families.contains(&"Inter Thin"), "{families:?}");
        assert!(families.contains(&"Inter Black"), "{families:?}");

        // And each of them draws a different letter, which is the only thing
        // that makes the list worth having.
        let ink = |family: &str| -> f32 {
            let index = library.select(Some(family), false, false).expect("a face");
            let face = library.face(index).expect("a face");
            let font = face.font().expect("a font");
            let glyph = font.glyph_for('n').expect("a letter");
            let outline = font.outline(glyph).expect("an outline").expect("something drawn");
            f32::from(outline.bounds.max_x - outline.bounds.min_x)
                * f32::from(outline.bounds.max_y - outline.bounds.min_y)
        };
        let thin = ink("Inter Thin");
        let black = ink("Inter Black");
        assert!(black > thin, "Black covers {black} and Thin {thin}");
    }
}
