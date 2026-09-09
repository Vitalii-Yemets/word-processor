//! Sources, citations and the bibliography.
//!
//! # Where a source lives
//!
//! Not in the document text. A document that cites something keeps the details
//! of what it cites in a part of its own, `word/bibliography.xml`, written in a
//! namespace that has nothing to do with WordprocessingML. The text holds only
//! a `CITATION` field naming a tag; everything a reader sees — the name in the
//! brackets, the line in the bibliography — is worked out from the source that
//! tag names.
//!
//! That is why deleting a citation leaves the source behind, and why a source
//! can be added long before anything cites it.
//!
//! # Why the bibliography is generated
//!
//! For the same reason the table of contents is: it is a `BIBLIOGRAPHY` field
//! wrapped round the lines somebody last generated. Writing only the lines
//! would give a list that looks right and is dead.

use wp_opc::TargetMode;
use wp_xml::tree::{Element, Node, XmlTree};

use crate::history::EditKind;
use crate::model::{Block, Run};
use crate::{edit, position, read, Document, Error, TextPosition};

/// The namespace sources are written in.
///
/// Deliberately not `w:` — a source is not part of the text.
pub const B: &str = "http://schemas.openxmlformats.org/officeDocument/2006/bibliography";

/// Content type of the part holding the sources.
const CONTENT_TYPE: &str = "application/xml";

/// Relationship type that points at it from the document.
const RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/bibliography";

/// The instruction that generates a bibliography.
pub const BIBLIOGRAPHY_INSTRUCTION: &str = "BIBLIOGRAPHY \\l 1033";

/// What kind of thing a source is.
///
/// The names are the ones the format uses, not translations of them: a source
/// type is written into the file and read back by Word, so it is spelt its way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SourceKind {
    #[default]
    Book,
    JournalArticle,
    InternetSite,
    Report,
}

impl SourceKind {
    /// The word written into the file.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Book => "Book",
            Self::JournalArticle => "JournalArticle",
            Self::InternetSite => "InternetSite",
            Self::Report => "Report",
        }
    }

    /// What a person is shown when picking one.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Book => "Book",
            Self::JournalArticle => "Journal Article",
            Self::InternetSite => "Web Site",
            Self::Report => "Report",
        }
    }

    /// Reads one back out of a file.
    #[must_use]
    pub fn from_word(word: &str) -> Self {
        match word {
            "JournalArticle" | "ArticleInAPeriodical" => Self::JournalArticle,
            "InternetSite" | "DocumentFromInternetSite" => Self::InternetSite,
            "Report" => Self::Report,
            _ => Self::Book,
        }
    }

    /// Every kind that can be picked.
    pub const ALL: &'static [Self] =
        &[Self::Book, Self::JournalArticle, Self::InternetSite, Self::Report];
}

/// One thing the document can cite.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Source {
    /// The short name a citation points at, unique within the document.
    pub tag: String,
    pub kind: SourceKind,
    /// Who wrote it, written out the way a person would say it.
    pub author: String,
    pub title: String,
    pub year: String,
    /// The publisher of a book, or the journal an article is in.
    pub publisher: String,
    /// Where it was published, or the address of a web site.
    pub city: String,
}

impl Source {
    /// The name in the brackets, which is what a citation shows.
    #[must_use]
    pub fn short(&self) -> String {
        let name = self.surname();
        match (name.is_empty(), self.year.is_empty()) {
            (true, true) => format!("({})", self.tag),
            (true, false) => format!("({})", self.year),
            (false, true) => format!("({name})"),
            (false, false) => format!("({name}, {})", self.year),
        }
    }

    /// The line a bibliography lists it on.
    ///
    /// Written the way a reference list is written — author, date, title, then
    /// where it came from — rather than in any one style's exact punctuation,
    /// which is a whole standard of its own.
    #[must_use]
    pub fn line(&self) -> String {
        let mut line = String::new();
        if !self.author.is_empty() {
            line.push_str(&self.listed_author());
        }
        if !self.year.is_empty() {
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(&format!("({}).", self.year));
        }
        if !self.title.is_empty() {
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(&self.title);
            if !self.title.ends_with('.') {
                line.push('.');
            }
        }
        let where_from = match (self.city.is_empty(), self.publisher.is_empty()) {
            (true, true) => String::new(),
            (true, false) => self.publisher.clone(),
            (false, true) => self.city.clone(),
            (false, false) => format!("{}: {}", self.city, self.publisher),
        };
        if !where_from.is_empty() {
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(&where_from);
            line.push('.');
        }
        if line.is_empty() {
            line.push_str(&self.tag);
        }
        line
    }

    /// The surname, which is what a citation and the sorting both go by.
    #[must_use]
    pub fn surname(&self) -> String {
        self.author.split_whitespace().next_back().unwrap_or_default().to_owned()
    }

    /// The author written surname-first, the way a reference list orders them.
    fn listed_author(&self) -> String {
        let mut words: Vec<&str> = self.author.split_whitespace().collect();
        let Some(last) = words.pop() else { return String::new() };
        if words.is_empty() {
            return format!("{last},");
        }
        let initials: String =
            words.iter().filter_map(|word| word.chars().next()).map(|c| format!("{c}. ")).collect();
        format!("{last}, {}", initials.trim_end())
    }
}

impl Document {
    /// Every source the document knows about, in the order they were added.
    #[must_use]
    pub fn sources(&self) -> Vec<Source> {
        let Some(part) = self.bibliography_part() else { return Vec::new() };
        let Some(Ok(text)) = self.package().xml_part(&part) else { return Vec::new() };
        let Ok(tree) = XmlTree::parse(&text) else { return Vec::new() };
        tree.root
            .child_elements()
            .filter(|child| child.local_name() == "Source")
            .map(read)
            .collect()
    }

    /// One source by its tag.
    #[must_use]
    pub fn source(&self, tag: &str) -> Option<Source> {
        self.sources().into_iter().find(|source| source.tag == tag)
    }

    /// Adds a source, or replaces the one already using its tag.
    ///
    /// Returns the tag it ended up with, which is the one given unless it was
    /// empty or already taken by something else.
    pub fn add_source(&mut self, source: &Source) -> Result<String, Error> {
        let mut source = source.clone();
        source.tag = self.settled_tag(&source);

        let mut root = self.sources_root();
        let wanted = source.tag.clone();
        root.children.retain(|node| match node {
            Node::Element(element) => {
                !(element.local_name() == "Source" && tag_of(element) == wanted)
            }
            _ => true,
        });
        root.push_element(write(&source));

        self.save_sources_root(root)?;
        self.mark_modified();
        Ok(source.tag)
    }

    /// Takes a source away. Citations pointing at it are left alone, the way
    /// Word leaves them: the field is still there, still says what it said.
    pub fn remove_source(&mut self, tag: &str) -> bool {
        let mut root = self.sources_root();
        let before = root.children.len();
        root.children.retain(|node| match node {
            Node::Element(element) => !(element.local_name() == "Source" && tag_of(element) == tag),
            _ => true,
        });
        if root.children.len() == before {
            return false;
        }
        if self.save_sources_root(root).is_err() {
            return false;
        }
        self.mark_modified();
        true
    }

    /// Puts a citation at the caret.
    pub fn insert_citation(&mut self, tag: &str) -> bool {
        let Some(source) = self.source(tag) else { return false };
        let shown = source.short();
        let instruction = citation_instruction(tag);
        let run = Run::field(&instruction, &shown);

        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let prefix = self.prefix();

        let Some(path) = position::paragraph_path(&self.tree().root, caret.paragraph) else {
            return false;
        };
        let Some(paragraph) = edit::element_at_path_mut(&mut self.tree_mut().root, &path) else {
            return false;
        };
        crate::format::split_runs_at_offset(paragraph, caret.offset);
        let at = edit::child_position_at_offset(paragraph, caret.offset);

        let mut field =
            Element::new(&edit::name_with(prefix.as_deref(), "fldSimple"), Some(read::W));
        field.set_namespaced_attribute(
            &edit::name_with(prefix.as_deref(), "instr"),
            read::W,
            &format!(" {instruction} "),
        );
        field.push_element(edit::run_element(&run, prefix.as_deref()));
        paragraph.insert_element(at, field);

        self.set_caret(TextPosition::new(caret.paragraph, caret.offset + shown.len()));
        self.mark_modified();
        true
    }

    /// The tags the document cites, in reading order and each named once.
    #[must_use]
    pub fn citations(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for paragraph in self.paragraph_elements() {
            gather_citations(paragraph, &mut out);
        }
        out
    }

    /// Puts a bibliography at the caret, replacing one that is there.
    ///
    /// Lists only what the document actually cites, which is what a reference
    /// list is; a source nobody cites stays in the part, unlisted.
    pub fn insert_bibliography(&mut self) -> usize {
        let cited = self.citations();
        let mut sources: Vec<Source> =
            cited.iter().filter_map(|tag| self.source(tag)).collect::<Vec<_>>();
        sources.sort_by(|left, right| {
            left.surname()
                .to_lowercase()
                .cmp(&right.surname().to_lowercase())
                .then(left.year.cmp(&right.year))
        });

        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let at = match self.field_table_range(BIBLIOGRAPHY_INSTRUCTION) {
            Some((first, last)) => {
                self.remove_paragraphs(first, last);
                first
            }
            None => caret.paragraph,
        };

        let mut blocks = vec![Block::Paragraph(crate::figures::field_paragraph(
            vec![crate::figures::heading_run("Bibliography", BIBLIOGRAPHY_INSTRUCTION)],
            0,
            None,
        ))];
        if sources.is_empty() {
            blocks.push(Block::Paragraph(crate::figures::field_paragraph(
                vec![Run::field(BIBLIOGRAPHY_INSTRUCTION, "Nothing in this document is cited")],
                0,
                None,
            )));
        }
        for source in &sources {
            blocks.push(Block::Paragraph(crate::figures::field_paragraph(
                vec![Run::field(BIBLIOGRAPHY_INSTRUCTION, &source.line())],
                0,
                // A bibliography is prose, not a column of numbers: nothing to
                // put against the right-hand edge.
                None,
            )));
        }

        self.write_generated(at, &blocks);
        sources.len()
    }

    /// Whether the document already has a bibliography.
    #[must_use]
    pub fn has_bibliography(&self) -> bool {
        self.field_table_range(BIBLIOGRAPHY_INSTRUCTION).is_some()
    }

    /// The name of the part holding the sources, if there is one.
    #[must_use]
    pub fn bibliography_part(&self) -> Option<String> {
        let relationships = self.package().relationships(self.main_part()).ok()?;
        let relationship = relationships.single_by_type(RELATIONSHIP)?;
        relationship.resolved_target(self.main_part())?.ok()
    }

    /// A tag nothing else is using.
    fn settled_tag(&self, source: &Source) -> String {
        let wanted = sanitise_tag(&source.tag);
        if !wanted.is_empty() {
            return wanted;
        }
        // Word's own scheme when a person does not name one: surname, then the
        // year, then a letter if that is taken too.
        let stem = {
            let surname = sanitise_tag(&source.surname());
            let stem = format!("{surname}{}", sanitise_tag(&source.year));
            if stem.is_empty() {
                "Source".to_owned()
            } else {
                stem
            }
        };
        let taken: Vec<String> = self.sources().into_iter().map(|source| source.tag).collect();
        if !taken.contains(&stem) {
            return stem;
        }
        for letter in 'a'..='z' {
            let candidate = format!("{stem}{letter}");
            if !taken.contains(&candidate) {
                return candidate;
            }
        }
        stem
    }

    /// The `b:Sources` element, read from the part or made afresh.
    fn sources_root(&self) -> Element {
        if let Some(part) = self.bibliography_part() {
            if let Some(Ok(text)) = self.package().xml_part(&part) {
                if let Ok(tree) = XmlTree::parse(&text) {
                    return tree.root;
                }
            }
        }

        let mut root = Element::new("b:Sources", Some(B));
        root.declarations.push((Some("b".to_owned()), B.to_owned()));
        // The style a reader's Word would format the citations in, were it
        // doing the formatting. This one does its own.
        root.set_attribute("SelectedStyle", "\\APA.XSL");
        root.set_attribute("StyleName", "APA");
        root
    }

    /// Writes the part back, adding the relationship the first time.
    fn save_sources_root(&mut self, root: Element) -> Result<(), Error> {
        let tree = XmlTree {
            standalone: Some(true),
            has_declaration: true,
            doctype: None,
            before_root: Vec::new(),
            root,
            after_root: Vec::new(),
        };
        let xml = tree
            .to_xml()
            .map_err(|source| Error::Xml { part: "word/bibliography.xml".to_owned(), source })?;

        let part = self.bibliography_part().unwrap_or_else(|| "word/bibliography.xml".to_owned());
        self.package_mut().add_part(&part, CONTENT_TYPE, xml.into_bytes());

        let main_part = self.main_part().to_owned();
        let mut relationships = self
            .package()
            .relationships(&main_part)
            .unwrap_or_else(|_| wp_opc::Relationships::new(&main_part));
        if relationships.single_by_type(RELATIONSHIP).is_none() {
            let target = part.strip_prefix("word/").unwrap_or(&part).to_owned();
            relationships.add(RELATIONSHIP, &target, TargetMode::Internal);
            self.package_mut().set_relationships(&relationships)?;
        }
        Ok(())
    }
}

/// The instruction a citation carries.
#[must_use]
pub fn citation_instruction(tag: &str) -> String {
    format!("CITATION {tag} \\l 1033")
}

/// A tag with the characters a tag cannot hold taken out.
#[must_use]
pub fn sanitise_tag(text: &str) -> String {
    text.chars().filter(|c| c.is_alphanumeric()).collect()
}

/// The tag of a `b:Source` element.
fn tag_of(element: &Element) -> String {
    element
        .child_elements()
        .find(|child| child.local_name() == "Tag")
        .map(Element::text_content)
        .unwrap_or_default()
}

/// Reads one source out of its element.
fn read(element: &Element) -> Source {
    let field = |name: &str| {
        element
            .child_elements()
            .find(|child| child.local_name() == name)
            .map(Element::text_content)
            .unwrap_or_default()
    };

    Source {
        tag: field("Tag"),
        kind: SourceKind::from_word(&field("SourceType")),
        author: read_author(element),
        title: field("Title"),
        year: field("Year"),
        // A journal article names its journal where a book names its
        // publisher, and a web site names neither.
        publisher: {
            let publisher = field("Publisher");
            if publisher.is_empty() {
                field("JournalName")
            } else {
                publisher
            }
        },
        city: {
            let city = field("City");
            if city.is_empty() {
                field("URL")
            } else {
                city
            }
        },
    }
}

/// Digs the author's name out of the four elements it is buried under.
fn read_author(element: &Element) -> String {
    let Some(outer) = element.child_elements().find(|child| child.local_name() == "Author") else {
        return String::new();
    };
    let Some(inner) = outer.child_elements().find(|child| child.local_name() == "Author") else {
        return String::new();
    };

    // A corporate author is written straight out rather than as a person.
    if let Some(corporate) = inner.child_elements().find(|child| child.local_name() == "Corporate")
    {
        return corporate.text_content();
    }

    let Some(list) = inner.child_elements().find(|child| child.local_name() == "NameList") else {
        return String::new();
    };
    let names: Vec<String> = list
        .child_elements()
        .filter(|child| child.local_name() == "Person")
        .map(|person| {
            let part = |name: &str| {
                person
                    .child_elements()
                    .find(|child| child.local_name() == name)
                    .map(Element::text_content)
                    .unwrap_or_default()
            };
            let (first, middle, last) = (part("First"), part("Middle"), part("Last"));
            [first, middle, last]
                .into_iter()
                .filter(|piece| !piece.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|name| !name.is_empty())
        .collect();
    names.join(", ")
}

/// Writes one source as its element.
fn write(source: &Source) -> Element {
    let mut element = Element::new("b:Source", Some(B));
    let mut put = |name: &str, value: &str| {
        if value.is_empty() {
            return;
        }
        let mut child = Element::new(&format!("b:{name}"), Some(B));
        child.set_text(value);
        element.push_element(child);
    };

    put("Tag", &source.tag);
    put("SourceType", source.kind.word());
    put("Title", &source.title);
    put("Year", &source.year);
    match source.kind {
        SourceKind::JournalArticle => put("JournalName", &source.publisher),
        _ => put("Publisher", &source.publisher),
    }
    match source.kind {
        SourceKind::InternetSite => put("URL", &source.city),
        _ => put("City", &source.city),
    }

    if !source.author.is_empty() {
        element.push_element(author_element(&source.author));
    }
    element
}

/// The four nested elements an author's name is written in.
fn author_element(author: &str) -> Element {
    let mut list = Element::new("b:NameList", Some(B));
    for name in author.split(',').map(str::trim).filter(|name| !name.is_empty()) {
        let mut words: Vec<&str> = name.split_whitespace().collect();
        let Some(last) = words.pop() else { continue };

        let mut person = Element::new("b:Person", Some(B));
        let mut last_element = Element::new("b:Last", Some(B));
        last_element.set_text(last);
        person.push_element(last_element);
        if let Some(first) = words.first() {
            let mut first_element = Element::new("b:First", Some(B));
            first_element.set_text(first);
            person.push_element(first_element);
        }
        if words.len() > 1 {
            let mut middle = Element::new("b:Middle", Some(B));
            middle.set_text(&words[1..].join(" "));
            person.push_element(middle);
        }
        list.push_element(person);
    }

    let mut inner = Element::new("b:Author", Some(B));
    inner.push_element(list);
    let mut outer = Element::new("b:Author", Some(B));
    outer.push_element(inner);
    outer
}

/// Finds every citation in an element, adding each tag once.
fn gather_citations(element: &Element, out: &mut Vec<String>) {
    for child in element.child_elements() {
        if child.namespace.as_deref() == Some(read::W) && child.local_name() == "fldSimple" {
            let instruction = child.attribute(Some(read::W), "instr").unwrap_or_default();
            if let Some(tag) = tag_in(instruction) {
                if !out.contains(&tag) {
                    out.push(tag);
                }
            }
        }
        gather_citations(child, out);
    }
}

/// The tag a `CITATION` instruction names, if it is one.
fn tag_in(instruction: &str) -> Option<String> {
    let rest = instruction.trim().strip_prefix("CITATION")?;
    let tag = rest.split_whitespace().next()?;
    if tag.starts_with('\\') {
        return None;
    }
    Some(tag.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book() -> Source {
        Source {
            tag: "Doe01".to_owned(),
            kind: SourceKind::Book,
            author: "John Doe".to_owned(),
            title: "A Book".to_owned(),
            year: "2001".to_owned(),
            publisher: "A Press".to_owned(),
            city: "London".to_owned(),
        }
    }

    #[test]
    fn a_citation_shows_the_surname_and_the_year() {
        assert_eq!(book().short(), "(Doe, 2001)");
    }

    #[test]
    fn a_citation_falls_back_to_the_tag_when_nothing_else_is_known() {
        let source = Source { tag: "Anon".to_owned(), ..Source::default() };
        assert_eq!(source.short(), "(Anon)");
    }

    #[test]
    fn a_listed_author_is_written_surname_first() {
        assert_eq!(book().line(), "Doe, J. (2001). A Book. London: A Press.");
    }

    #[test]
    fn a_source_survives_being_written_and_read_back() {
        let read_back = read(&write(&book()));
        assert_eq!(read_back, book());
    }

    #[test]
    fn a_journal_article_names_a_journal_rather_than_a_publisher() {
        let source = Source { kind: SourceKind::JournalArticle, ..book() };
        let element = write(&source);
        assert!(element.child_elements().any(|child| child.local_name() == "JournalName"));
        assert_eq!(read(&element).publisher, "A Press");
    }

    #[test]
    fn an_instruction_names_its_tag() {
        assert_eq!(tag_in("CITATION Doe01 \\l 1033").as_deref(), Some("Doe01"));
        assert_eq!(tag_in("PAGE"), None);
        assert_eq!(tag_in("CITATION"), None);
    }

    #[test]
    fn a_tag_keeps_only_the_characters_a_tag_can_hold() {
        assert_eq!(sanitise_tag("Doe, J. 01"), "DoeJ01");
    }
}
