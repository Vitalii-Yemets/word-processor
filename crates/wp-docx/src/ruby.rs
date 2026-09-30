//! Ruby: the small reading printed over a word.
//!
//! # What it is
//!
//! Japanese and Chinese are written with characters a reader may not know how
//! to say, so the reading is printed over them in a smaller script: 漢字 with
//! かんじ above it. Word calls it the Phonetic Guide. The same thing is used in
//! any language for a gloss — a pronunciation over a name, a translation over
//! a foreign word — and nothing in the format ties it to Japanese.
//!
//! # Why it is two pieces of text and not one
//!
//! Because both are text: each has its own font, size and colour, each can be
//! searched, and the reading is not part of the sentence. So a ruby is a run
//! holding two lists of runs — the base, which is in the line, and the
//! annotation, which sits above it — and the characters of the base are the
//! characters of the document while the annotation's are not counted at all.
//! A caret walks through 漢字 in two steps, not five.
//!
//! # What the properties say
//!
//! How big the reading is set (`w:hps`, in half-points), how far above the
//! line it sits (`w:hpsRaise`), how big the text under it is (`w:hpsBaseText`)
//! and how the two are lined up when one is wider than the other
//! (`w:rubyAlign`). Word writes all four whenever it writes a ruby, and they
//! are kept as the file gives them: a reading set in the wrong size is a
//! reading a reader stumbles over.

use wp_xml::tree::Element;

use crate::model::Run;
use crate::read::W;

/// How the reading and the text under it are lined up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    /// The reading centred over the text, which is what a short reading over a
    /// long word wants.
    Center,
    /// Spread so that it starts and ends with the text, the gaps being put
    /// between the letters.
    DistributeLetter,
    /// The same, with a gap at each end as well. Word's own default.
    #[default]
    DistributeSpace,
    /// Against the left end of the text, and against the right.
    Left,
    Right,
}

impl Align {
    /// The name the format gives it.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Center => "center",
            Self::DistributeLetter => "distributeLetter",
            Self::DistributeSpace => "distributeSpace",
            Self::Left => "left",
            Self::Right => "right",
        }
    }

    /// And reading one back. Anything unknown is what Word writes when it has
    /// nothing to say.
    #[must_use]
    pub fn from_word(word: &str) -> Self {
        match word {
            "center" => Self::Center,
            "distributeLetter" => Self::DistributeLetter,
            "left" => Self::Left,
            "right" => Self::Right,
            _ => Self::DistributeSpace,
        }
    }
}

/// How a reading is set: its size, its height above the line, and its
/// alignment.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Properties {
    pub align: Align,
    /// The size of the reading, in half-points.
    pub size_half_points: Option<i32>,
    /// How far above the baseline it sits, in half-points.
    pub raise_half_points: Option<i32>,
    /// The size of the text under it, which Word writes so that a ruby keeps
    /// its proportions when the document's own size changes.
    pub base_size_half_points: Option<i32>,
    /// The language the reading is in, which is what a proofing tool needs to
    /// know to leave it alone.
    pub language: Option<String>,
}

/// A word and the reading printed over it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ruby {
    pub properties: Properties,
    /// The reading, which is not part of the sentence.
    pub annotation: Vec<Run>,
    /// The text it is printed over, which is.
    pub base: Vec<Run>,
}

impl Ruby {
    /// What the document says here, which is the text under the reading.
    ///
    /// The reading is not part of it: a search for the word finds the word,
    /// and a search for its reading finds the reading — which is the same
    /// answer Word gives.
    #[must_use]
    pub fn plain_text(&self) -> String {
        self.base.iter().map(Run::plain_text).collect()
    }

    /// And the reading on its own.
    #[must_use]
    pub fn reading(&self) -> String {
        self.annotation.iter().map(Run::plain_text).collect()
    }

    /// A ruby of plain text, set the way Word sets one.
    #[must_use]
    pub fn over(base: &str, reading: &str, base_size_half_points: i32) -> Self {
        // Word sets the reading at half the size of the text and lifts it by
        // rather more than that: those are the proportions of every ruby it
        // writes, and a reading set otherwise looks like a footnote.
        let size = (base_size_half_points / 2).max(2);
        Self {
            properties: Properties {
                align: Align::DistributeSpace,
                size_half_points: Some(size),
                raise_half_points: Some(base_size_half_points),
                base_size_half_points: Some(base_size_half_points),
                language: None,
            },
            annotation: vec![sized(reading, size)],
            base: vec![sized(base, base_size_half_points)],
        }
    }
}

/// A run of text at a stated size.
///
/// Both halves carry their own: the properties above say what the sizes are
/// meant to be, and the runs are what actually gets drawn. A ruby whose runs
/// said nothing would be set at whatever size surrounded it, which for the
/// reading is the one size it must not be.
fn sized(text: &str, half_points: i32) -> Run {
    let mut run = Run::text(text);
    run.properties.size_half_points = Some(half_points.max(1) as u32);
    run
}

impl crate::Document {
    /// Puts a word with its reading over it at the caret.
    ///
    /// The word goes into the text and the reading does not: after this the
    /// caret stands past the word, as it would after typing it.
    pub fn insert_ruby(&mut self, ruby: &Ruby) -> bool {
        let word = ruby.plain_text();
        if word.is_empty() {
            return false;
        }

        let caret = self.caret();
        self.record(crate::history::EditKind::Structural, caret, false);
        let prefix = self.prefix();

        // The ruby itself and not a run round it: what goes in here lands
        // beside the words already in the run, and a run inside a run is not
        // something any reader of the format looks for.
        let inserted = crate::position::insert_element_at(
            &mut self.tree_to_edit().root,
            caret,
            ruby_element(ruby, prefix.as_deref()),
            prefix.as_deref(),
        );

        if inserted {
            self.set_caret(crate::TextPosition::new(caret.paragraph, caret.offset + word.len()));
            self.note_change();
        }
        inserted
    }
}

/// Reads a `w:ruby`.
#[must_use]
pub fn read_ruby(element: &Element) -> Option<Ruby> {
    let mut ruby = Ruby::default();

    if let Some(properties) = element.child(Some(W), "rubyPr") {
        let value = |local: &str| -> Option<&str> {
            properties.child(Some(W), local)?.attribute(Some(W), "val")
        };
        let number = |local: &str| -> Option<i32> { value(local)?.trim().parse().ok() };

        ruby.properties = Properties {
            align: value("rubyAlign").map(Align::from_word).unwrap_or_default(),
            size_half_points: number("hps"),
            raise_half_points: number("hpsRaise"),
            base_size_half_points: number("hpsBaseText"),
            language: value("lid").map(str::to_owned),
        };
    }

    if let Some(reading) = element.child(Some(W), "rt") {
        ruby.annotation = runs_in(reading);
    }
    if let Some(base) = element.child(Some(W), "rubyBase") {
        ruby.base = runs_in(base);
    }

    // A ruby with nothing under it is not a ruby: Word writes the base even
    // when the reading is empty, and a file that does not is one this cannot
    // draw.
    (!ruby.base.is_empty()).then_some(ruby)
}

/// The runs inside one half of a ruby.
fn runs_in(element: &Element) -> Vec<Run> {
    element
        .children_named(Some(W), "r")
        .map(crate::read::read_run)
        .filter(|run| !run.content.is_empty())
        .collect()
}

/// Writes a ruby back out.
#[must_use]
pub fn ruby_element(ruby: &Ruby, prefix: Option<&str>) -> Element {
    let named = |local: &str| crate::edit::name_with(prefix, local);
    let mut element = Element::new(&named("ruby"), Some(W));

    let mut properties = Element::new(&named("rubyPr"), Some(W));
    properties.push_element(crate::edit::valued(prefix, "rubyAlign", ruby.properties.align.word()));
    for (local, value) in [
        ("hps", ruby.properties.size_half_points),
        ("hpsRaise", ruby.properties.raise_half_points),
        ("hpsBaseText", ruby.properties.base_size_half_points),
    ] {
        if let Some(value) = value {
            properties.push_element(crate::edit::valued(prefix, local, &value.to_string()));
        }
    }
    if let Some(language) = &ruby.properties.language {
        properties.push_element(crate::edit::valued(prefix, "lid", language));
    }
    element.push_element(properties);

    // The reading first and the text after it, which is the order the schema
    // asks for and the order Word writes.
    let mut reading = Element::new(&named("rt"), Some(W));
    for run in &ruby.annotation {
        for child in crate::edit::run_elements(run, prefix) {
            reading.push_element(child);
        }
    }
    element.push_element(reading);

    let mut base = Element::new(&named("rubyBase"), Some(W));
    for run in &ruby.base {
        for child in crate::edit::run_elements(run, prefix) {
            base.push_element(child);
        }
    }
    element.push_element(base);

    element
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_xml::tree::XmlTree;

    fn parsed(inside: &str) -> Element {
        let text = format!("<w:ruby xmlns:w=\"{W}\">{inside}</w:ruby>");
        XmlTree::parse(&text).expect("a ruby").root
    }

    /// A ruby written the way Word writes one.
    const WORD: &str = "<w:rubyPr><w:rubyAlign w:val=\"distributeSpace\"/><w:hps w:val=\"10\"/>\
         <w:hpsRaise w:val=\"22\"/><w:hpsBaseText w:val=\"21\"/><w:lid w:val=\"ja-JP\"/>\
         </w:rubyPr><w:rt><w:r><w:t>\u{304B}\u{3093}\u{3058}</w:t></w:r></w:rt>\
         <w:rubyBase><w:r><w:t>\u{6F22}\u{5B57}</w:t></w:r></w:rubyBase>";

    #[test]
    fn a_ruby_word_wrote_is_read_whole() {
        let ruby = read_ruby(&parsed(WORD)).expect("a ruby");
        assert_eq!(ruby.plain_text(), "\u{6F22}\u{5B57}");
        assert_eq!(ruby.reading(), "\u{304B}\u{3093}\u{3058}");
        assert_eq!(ruby.properties.align, Align::DistributeSpace);
        assert_eq!(ruby.properties.size_half_points, Some(10));
        assert_eq!(ruby.properties.raise_half_points, Some(22));
        assert_eq!(ruby.properties.base_size_half_points, Some(21));
        assert_eq!(ruby.properties.language.as_deref(), Some("ja-JP"));
    }

    #[test]
    fn what_the_document_says_is_the_word_and_not_the_reading() {
        // Which is what makes a search for the word find it, and a word count
        // count it once.
        let ruby = read_ruby(&parsed(WORD)).expect("a ruby");
        assert_eq!(ruby.plain_text().chars().count(), 2);
    }

    #[test]
    fn a_ruby_survives_being_written_and_read_back() {
        let ruby = read_ruby(&parsed(WORD)).expect("a ruby");
        let written = ruby_element(&ruby, None);
        let read = read_ruby(&written).expect("a ruby");
        assert_eq!(read, ruby);
    }

    #[test]
    fn every_alignment_is_written_and_read_back() {
        for align in [
            Align::Center,
            Align::DistributeLetter,
            Align::DistributeSpace,
            Align::Left,
            Align::Right,
        ] {
            let mut ruby = Ruby::over("word", "reading", 20);
            ruby.properties.align = align;
            let read = read_ruby(&ruby_element(&ruby, None)).expect("a ruby");
            assert_eq!(read.properties.align, align, "{}", align.word());
        }
    }

    #[test]
    fn a_ruby_with_nothing_under_it_is_not_a_ruby() {
        let only_reading = "<w:rt><w:r><w:t>reading</w:t></w:r></w:rt>";
        assert!(read_ruby(&parsed(only_reading)).is_none());
    }

    #[test]
    fn one_made_here_is_set_the_way_word_sets_one() {
        // Half the size of the text, lifted by about the height of it.
        let ruby = Ruby::over("\u{6F22}\u{5B57}", "\u{304B}\u{3093}\u{3058}", 24);
        assert_eq!(ruby.properties.size_half_points, Some(12));
        assert_eq!(ruby.properties.base_size_half_points, Some(24));
        assert!(ruby.properties.raise_half_points.unwrap_or(0) > 0);
    }
    #[test]
    fn one_put_into_a_document_is_there_afterwards() {
        use crate::model::{Block, Body, Paragraph};

        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("before after")));
        let bytes = crate::Document::create(&body).expect("a document").save().expect("saving");
        let mut document = crate::Document::open(&bytes).expect("reopening");

        document.set_caret(crate::TextPosition::new(0, 7));
        let ruby = Ruby::over("\u{6F22}\u{5B57}", "\u{304B}\u{3093}\u{3058}", 22);
        assert!(document.insert_ruby(&ruby));

        // The word is in the text; the reading is not.
        let text = document.paragraph_text(0).unwrap_or_default();
        assert_eq!(text, "before \u{6F22}\u{5B57}after");
        // And the caret stands past the word, as after typing it.
        assert_eq!(document.caret().offset, 7 + "\u{6F22}\u{5B57}".len());
    }

    #[test]
    fn a_ruby_over_nothing_goes_nowhere() {
        use crate::model::{Block, Body, Paragraph};

        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("text")));
        let bytes = crate::Document::create(&body).expect("a document").save().expect("saving");
        let mut document = crate::Document::open(&bytes).expect("reopening");

        let empty = Ruby { base: Vec::new(), ..Ruby::over("x", "y", 20) };
        assert!(!document.insert_ruby(&empty));
    }
}
