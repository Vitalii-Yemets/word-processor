//! The Visual Basic project a document carries, read.
//!
//! # What is inside `vbaProject.bin`
//!
//! A compound file — the same one [`wp_ole`] already reads — holding a `VBA`
//! storage, and inside that a `dir` stream and one stream per module. Every
//! one of them is compressed with the scheme in [`compress`], and the `dir`
//! stream is a list of records saying what the modules are called, which
//! stream each one lives in, and how far into that stream its text begins.
//! Before that offset is a cache Word keeps for its own editor; this program
//! reads past it, and leaves it alone.
//!
//! # What this does not do
//!
//! Run anything. This reads the project so that a person can see what a
//! document carries — which macros are in it, in which module, and what they
//! say — and nothing here executes a line of it. The bytes are kept exactly
//! as they came and written back untouched on save.
//!
//! Nor does it parse the language. A macro's name is found by looking at the
//! lines that declare one, the way a person scanning a listing would, which
//! is enough to list them and not enough to run them. The parser is a later
//! item and a much larger one.

#![forbid(unsafe_code)]

pub mod compress;

use wp_ole::CompoundFile;

/// Why a project could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The part is not a compound file at all.
    NotCompound(wp_ole::Error),
    /// It is one, and there is no Visual Basic project in it.
    NoProject,
    /// A stream would not decompress.
    Squeezed(compress::Error),
    /// The `dir` stream is not as the format says.
    Malformed(&'static str),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NotCompound(error) => write!(f, "{error}"),
            Self::NoProject => write!(f, "there is no Visual Basic project inside"),
            Self::Squeezed(error) => write!(f, "{error}"),
            Self::Malformed(what) => write!(f, "the project's {what} is not as the format says"),
        }
    }
}

impl std::error::Error for Error {}

/// What a module is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// An ordinary module of procedures: `Module1`.
    Standard,
    /// A class: `Class1`.
    Class,
    /// The one that belongs to the document itself: `ThisDocument`.
    Document,
}

impl Kind {
    /// The word for it, as Word's own editor writes it.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Standard => "Module",
            Self::Class => "Class",
            Self::Document => "Document",
        }
    }
}

/// What a declaration declares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sort {
    Sub,
    Function,
    Property,
}

/// One procedure a module declares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Procedure {
    pub name: String,
    pub sort: Sort,
    /// Which line of the module declares it, counting from one.
    pub line: usize,
    /// Whether anything outside the module may call it. A module says
    /// `Private Sub` for one that may not; anything else is public.
    pub public: bool,
    /// Whether it wants anything passed to it.
    pub takes_arguments: bool,
}

/// One module of a project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Module {
    /// What it is called in the editor.
    pub name: String,
    /// And what its stream is called inside the file, which is not always the
    /// same: a module renamed in Word keeps the stream it was born in.
    pub stream: String,
    pub kind: Kind,
    /// Its text, with the line endings it was written with left as they are
    /// found: a module is `\r\n` and a caller that splits on lines gets the
    /// same lines either way.
    pub source: String,
    pub read_only: bool,
    pub private: bool,
}

impl Module {
    /// Every procedure the module declares, in the order they are written.
    #[must_use]
    pub fn procedures(&self) -> Vec<Procedure> {
        self.source.lines().enumerate().filter_map(|(at, line)| declared(line, at + 1)).collect()
    }
}

/// One macro, as Word's Macros dialog lists one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Macro {
    pub module: String,
    pub name: String,
}

impl Macro {
    /// How the pair is written where both are wanted: `Module1.Hello`.
    #[must_use]
    pub fn qualified(&self) -> String {
        format!("{}.{}", self.module, self.name)
    }
}

/// A document's Visual Basic project.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    /// What the project is called — `VBAProject` unless somebody changed it.
    pub name: String,
    /// The code page its names and its text are written in.
    pub code_page: u16,
    pub modules: Vec<Module>,
}

impl Project {
    /// Reads the project out of the bytes of `vbaProject.bin`.
    pub fn open(bytes: &[u8]) -> Result<Self, Error> {
        let file = CompoundFile::open(bytes.to_vec()).map_err(Error::NotCompound)?;
        let directory = file.walk(&["VBA", "dir"]).ok_or(Error::NoProject)?;
        let directory = compress::decompress(&directory).map_err(Error::Squeezed)?;

        let mut project = read_dir(&directory)?;
        let encoding = wp_text::Encoding::code_page(u32::from(project.code_page))
            .unwrap_or(wp_text::Encoding::CodePage(1252));

        // The names in the `dir` stream are written twice, once in the
        // project's code page and once in UTF-16. The second is taken where
        // it is there, because it cannot be wrong about a character the code
        // page has no room for.
        for module in &mut project.modules {
            if module.name.is_empty() {
                module.name = encoding.decode(module.raw_name.as_slice());
            }
            if module.stream.is_empty() {
                module.stream = encoding.decode(module.raw_stream.as_slice());
            }
        }

        let kinds = document_modules(&file, encoding);
        let mut modules = Vec::with_capacity(project.modules.len());
        for held in project.modules {
            let source = source_of(&file, &held, encoding)?;
            let kind = if kinds.iter().any(|name| name.eq_ignore_ascii_case(&held.name)) {
                Kind::Document
            } else {
                held.kind
            };
            modules.push(Module {
                name: held.name,
                stream: held.stream,
                kind,
                source,
                read_only: held.read_only,
                private: held.private,
            });
        }

        Ok(Self {
            name: encoding.decode(project.name.as_slice()),
            code_page: project.code_page,
            modules,
        })
    }

    /// The module of a name, if there is one.
    #[must_use]
    pub fn module(&self, name: &str) -> Option<&Module> {
        self.modules.iter().find(|module| module.name.eq_ignore_ascii_case(name))
    }

    /// The macros, as Word's Macros dialog lists them.
    ///
    /// Which is not every procedure: Word lists the ones a person could run
    /// from that dialog, and a function that wants an argument passed to it
    /// is not one of those. A `Private Sub` is not either — the module is
    /// saying it is for the module's own use.
    #[must_use]
    pub fn macros(&self) -> Vec<Macro> {
        let mut out = Vec::new();
        for module in &self.modules {
            for procedure in module.procedures() {
                if procedure.sort == Sort::Sub && procedure.public && !procedure.takes_arguments {
                    out.push(Macro { module: module.name.clone(), name: procedure.name });
                }
            }
        }
        out
    }
}

/// What the `dir` stream says about a module, before its text is read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Held {
    name: String,
    raw_name: Vec<u8>,
    stream: String,
    raw_stream: Vec<u8>,
    offset: usize,
    kind: Kind,
    read_only: bool,
    private: bool,
}

impl Default for Kind {
    fn default() -> Self {
        Self::Standard
    }
}

/// The `dir` stream, before the code page has been applied to it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Dir {
    name: Vec<u8>,
    code_page: u16,
    modules: Vec<Held>,
}

/// Reads the records of a decompressed `dir` stream.
///
/// Every record is an identifier, a length and that many bytes, with one
/// exception: the project's version has a length field that does not count
/// the version itself, and is walked past by hand. Everything else — the
/// references to other libraries especially — is skipped by its length
/// without being understood, which is what makes this survive a project full
/// of things this program has never seen.
fn read_dir(bytes: &[u8]) -> Result<Dir, Error> {
    let mut dir = Dir { code_page: 1252, ..Dir::default() };
    let mut module: Option<Held> = None;
    let mut at = 0usize;

    while at + 2 <= bytes.len() {
        let id = u16::from_le_bytes([bytes[at], bytes[at + 1]]);
        // The project's version: four bytes of reserved length, then six of
        // version, and no length field that covers them.
        if id == 0x0009 {
            at += 12;
            continue;
        }
        if at + 6 > bytes.len() {
            return Err(Error::Malformed("dir stream"));
        }
        let size = u32::from_le_bytes([bytes[at + 2], bytes[at + 3], bytes[at + 4], bytes[at + 5]])
            as usize;
        let from = at + 6;
        let to = from.checked_add(size).ok_or(Error::Malformed("dir stream"))?;
        if to > bytes.len() {
            return Err(Error::Malformed("dir stream"));
        }
        let data = &bytes[from..to];
        at = to;

        match id {
            0x0003 if size >= 2 => dir.code_page = u16::from_le_bytes([data[0], data[1]]),
            0x0004 => dir.name = data.to_vec(),
            // A module begins with its name and ends with a record of its
            // own, so everything between the two belongs to it.
            0x0019 => {
                if let Some(held) = module.take() {
                    dir.modules.push(held);
                }
                module = Some(Held { raw_name: data.to_vec(), ..Held::default() });
            }
            0x0047 => {
                if let Some(held) = &mut module {
                    held.name = utf16(data);
                }
            }
            0x001A => {
                if let Some(held) = &mut module {
                    held.raw_stream = data.to_vec();
                }
            }
            0x0032 => {
                if let Some(held) = &mut module {
                    held.stream = utf16(data);
                }
            }
            0x0031 if size >= 4 => {
                if let Some(held) = &mut module {
                    held.offset = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
                }
            }
            0x0021 => {
                if let Some(held) = &mut module {
                    held.kind = Kind::Standard;
                }
            }
            0x0022 => {
                if let Some(held) = &mut module {
                    held.kind = Kind::Class;
                }
            }
            0x0025 => {
                if let Some(held) = &mut module {
                    held.read_only = true;
                }
            }
            0x0028 => {
                if let Some(held) = &mut module {
                    held.private = true;
                }
            }
            0x002B => {
                if let Some(held) = module.take() {
                    dir.modules.push(held);
                }
            }
            _ => {}
        }
    }

    if let Some(held) = module.take() {
        dir.modules.push(held);
    }
    Ok(dir)
}

/// Text written as two bytes a character, which is how the `dir` stream
/// writes a name a code page could not hold.
fn utf16(bytes: &[u8]) -> String {
    let units: Vec<u16> =
        bytes.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
    String::from_utf16_lossy(&units)
}

/// A module's text, out of its own stream.
fn source_of(
    file: &CompoundFile,
    held: &Held,
    encoding: wp_text::Encoding,
) -> Result<String, Error> {
    let stream = file
        .walk(&["VBA", &held.stream])
        .or_else(|| file.walk(&[&held.stream]))
        .ok_or(Error::NoProject)?;
    if held.offset > stream.len() {
        return Err(Error::Malformed("module offset"));
    }
    // Before the offset is the cache Word's own editor keeps. It is not text
    // and it is none of this program's business.
    let text = compress::decompress(&stream[held.offset..]).map_err(Error::Squeezed)?;
    Ok(encoding.decode(&text))
}

/// The modules the `PROJECT` stream calls documents.
///
/// The `dir` stream says only whether a module is procedural or not, which
/// puts a class and the document's own module together. The `PROJECT` stream
/// — a page of `name=value` lines beside the storage — is where the two are
/// told apart, and a project without one loses nothing but the word.
fn document_modules(file: &CompoundFile, encoding: wp_text::Encoding) -> Vec<String> {
    let Some(bytes) = file.walk(&["PROJECT"]) else {
        return Vec::new();
    };
    encoding
        .decode(&bytes)
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            if !key.trim().eq_ignore_ascii_case("Document") {
                return None;
            }
            // `Document=ThisDocument/&H00000000`, and the tail is a cookie.
            Some(value.trim().split('/').next().unwrap_or_default().trim().to_owned())
        })
        .filter(|name| !name.is_empty())
        .collect()
}

/// The procedure a line declares, if it declares one.
///
/// Read the way somebody scanning a listing reads it: the modifiers a
/// declaration may carry, then the word that says what it is, then the name.
/// A line inside a string or a comment is not a declaration, and a comment is
/// the only one of those two that begins a line.
fn declared(line: &str, number: usize) -> Option<Procedure> {
    let mut words = line.trim().split_whitespace().peekable();
    let mut public = true;

    loop {
        let word = *words.peek()?;
        match word.to_ascii_lowercase().as_str() {
            "'" => return None,
            "public" | "friend" | "static" => public = true,
            "private" => public = false,
            _ => break,
        }
        words.next();
    }
    if line.trim_start().starts_with('\'') {
        return None;
    }

    let word = words.next()?;
    let sort = match word.to_ascii_lowercase().as_str() {
        "sub" => Sort::Sub,
        "function" => Sort::Function,
        "property" => {
            // `Property Get`, `Property Let`, `Property Set`: the second word
            // says which, and the name is the third.
            let which = words.next()?.to_ascii_lowercase();
            if !matches!(which.as_str(), "get" | "let" | "set") {
                return None;
            }
            Sort::Property
        }
        _ => return None,
    };

    let rest: String = words.collect::<Vec<_>>().join(" ");
    let (name, arguments) = match rest.split_once('(') {
        Some((name, arguments)) => (name.trim(), arguments),
        None => (rest.trim(), ""),
    };
    if name.is_empty() || !name.chars().all(|letter| letter.is_alphanumeric() || letter == '_') {
        return None;
    }

    let takes_arguments =
        !arguments.trim_start().starts_with(')') && arguments.trim_end_matches(')').trim() != "";
    Some(Procedure { name: name.to_owned(), sort, line: number, public, takes_arguments })
}

/// A record of the `dir` stream: an identifier, a length, and the bytes.
fn record(id: u16, data: &[u8]) -> Vec<u8> {
    let mut out = id.to_le_bytes().to_vec();
    #[allow(clippy::cast_possible_truncation)]
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
    out
}

/// Text as two bytes a character, as the `dir` stream writes a name.
fn wide(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// A project of the modules given, built the way Word builds one.
///
/// Here rather than in this crate's tests because two of them need it: this
/// crate, to show that what it reads is what was written, and the program
/// itself, to show that a document carrying one lists what is in it. There is
/// no Word file in the repository to be given instead — the corpus is every
/// machine's own — so a project built here is the only one either of them can
/// have.
///
/// A module called `ThisDocument` comes back as the document's own, because
/// the `PROJECT` stream written beside it says so, which is where Word says
/// it too.
#[must_use]
pub fn example(modules: &[(&str, &str)]) -> Vec<u8> {
    let mut dir = Vec::new();
    dir.extend_from_slice(&record(0x0003, &1252u16.to_le_bytes()));
    dir.extend_from_slice(&record(0x0004, b"VBAProject"));
    // The version, which is the one record with no length of its own.
    dir.extend_from_slice(&0x0009u16.to_le_bytes());
    dir.extend_from_slice(&4u32.to_le_bytes());
    dir.extend_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    dir.extend_from_slice(&0u16.to_le_bytes());
    #[allow(clippy::cast_possible_truncation)]
    dir.extend_from_slice(&record(0x000F, &(modules.len() as u16).to_le_bytes()));

    let mut streams = vec![("dir".to_owned(), Vec::new())];
    for (name, source) in modules {
        dir.extend_from_slice(&record(0x0019, name.as_bytes()));
        dir.extend_from_slice(&record(0x0047, &wide(name)));
        dir.extend_from_slice(&record(0x001A, name.as_bytes()));
        dir.extend_from_slice(&record(0x0032, &wide(name)));
        dir.extend_from_slice(&record(0x0021, &[]));
        // A cache Word keeps for its own editor, which this program reads
        // past and leaves alone.
        let cache = b"not text, and none of this program's business".to_vec();
        #[allow(clippy::cast_possible_truncation)]
        dir.extend_from_slice(&record(0x0031, &(cache.len() as u32).to_le_bytes()));
        dir.extend_from_slice(&record(0x002B, &[]));

        let mut stream = cache;
        stream.extend_from_slice(&compress::compress(source.as_bytes()));
        streams.push(((*name).to_owned(), stream));
    }
    dir.extend_from_slice(&record(0x0010, &[]));
    streams[0].1 = compress::compress(&dir);

    let mut builder = wp_ole::Builder::new();
    builder.item(wp_ole::Item::storage(
        "VBA",
        streams.into_iter().map(|(name, bytes)| wp_ole::Item::stream(&name, bytes)).collect(),
    ));
    builder.stream(
        "PROJECT",
        b"ID=\"{00000000-0000-0000-0000-000000000000}\"\r\nDocument=ThisDocument/&H00000000\r\n"
            .to_vec(),
    );
    builder.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELLO: &str = "Attribute VB_Name = \"Module1\"\r\n\
         Option Explicit\r\n\
         \r\n\
         ' The one everybody writes first.\r\n\
         Public Sub Hello()\r\n    \
             MsgBox \"Hello\"\r\n\
         End Sub\r\n\
         \r\n\
         Private Sub Quietly()\r\n\
         End Sub\r\n\
         \r\n\
         Function Twice(ByVal n As Long) As Long\r\n    \
             Twice = n * 2\r\n\
         End Function\r\n";

    #[test]
    fn a_project_is_read_out_of_the_bytes_of_the_part() {
        let bytes = example(&[
            ("Module1", HELLO),
            ("ThisDocument", "Attribute VB_Name = \"ThisDocument\"\r\n"),
        ]);
        let project = Project::open(&bytes).expect("a project");

        assert_eq!(project.name, "VBAProject");
        assert_eq!(project.code_page, 1252);
        let names: Vec<&str> = project.modules.iter().map(|module| module.name.as_str()).collect();
        assert_eq!(names, vec!["Module1", "ThisDocument"]);
    }

    #[test]
    fn a_modules_text_is_read_past_the_cache_word_keeps_in_front_of_it() {
        let bytes = example(&[("Module1", HELLO)]);
        let project = Project::open(&bytes).expect("a project");
        let module = project.module("module1").expect("the module, whatever its case");

        assert!(module.source.starts_with("Attribute VB_Name"), "{}", module.source);
        assert!(module.source.contains("MsgBox"), "{}", module.source);
        assert!(
            !module.source.contains("none of this program's business"),
            "the cache was read as text"
        );
    }

    #[test]
    fn the_procedures_of_a_module_are_found_where_they_are_declared() {
        let bytes = example(&[("Module1", HELLO)]);
        let project = Project::open(&bytes).expect("a project");
        let found = project.module("Module1").expect("the module").procedures();

        let named: Vec<(&str, Sort, bool, bool)> = found
            .iter()
            .map(|one| (one.name.as_str(), one.sort, one.public, one.takes_arguments))
            .collect();
        assert_eq!(
            named,
            vec![
                ("Hello", Sort::Sub, true, false),
                ("Quietly", Sort::Sub, false, false),
                ("Twice", Sort::Function, true, true),
            ]
        );
        assert_eq!(found[0].line, 5, "the line a macro is declared on is what an editor jumps to");
    }

    #[test]
    fn the_macro_list_is_what_word_would_offer_to_run() {
        // Not every procedure: a private one is the module's own business,
        // and a function wanting an argument cannot be run from a list.
        let bytes = example(&[("Module1", HELLO)]);
        let project = Project::open(&bytes).expect("a project");
        let macros = project.macros();

        assert_eq!(macros.len(), 1, "{macros:?}");
        assert_eq!(macros[0].qualified(), "Module1.Hello");
    }

    #[test]
    fn a_comment_that_looks_like_a_declaration_is_not_one() {
        assert!(declared("' Public Sub Hello()", 1).is_none());
        assert!(declared("    ' Sub Hello()", 1).is_none());
        assert!(declared("Dim Sub As Long", 1).is_none());
        assert!(declared("End Sub", 1).is_none());
    }

    #[test]
    fn a_property_is_declared_by_three_words_and_not_two() {
        let got = declared("Public Property Get Width() As Long", 3).expect("a property");
        assert_eq!((got.name.as_str(), got.sort, got.line), ("Width", Sort::Property, 3));
        assert!(declared("Property Something Width()", 1).is_none());
    }

    #[test]
    fn the_module_that_belongs_to_the_document_is_told_apart_from_a_class() {
        let bytes = example(&[
            ("Module1", HELLO),
            ("ThisDocument", "Attribute VB_Name = \"ThisDocument\"\r\n"),
        ]);
        let project = Project::open(&bytes).expect("a project");

        assert_eq!(project.module("Module1").expect("it").kind, Kind::Standard);
        assert_eq!(
            project.module("ThisDocument").expect("it").kind,
            Kind::Document,
            "the PROJECT stream says which module is the document's own"
        );
    }

    #[test]
    fn a_part_that_is_not_a_project_says_so_rather_than_guessing() {
        assert!(matches!(Project::open(b"not a compound file"), Err(Error::NotCompound(_))));

        let mut builder = wp_ole::Builder::new();
        builder.stream("WordDocument", vec![1, 2, 3, 4]);
        assert_eq!(Project::open(&builder.build()), Err(Error::NoProject));
    }
}
