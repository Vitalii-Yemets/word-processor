//! Fields written the long way, as a run of markers rather than one element.
//!
//! # Why there are two ways of writing a field
//!
//! `w:fldSimple` is one element with the instruction in an attribute and the
//! last computed answer inside it. It is tidy, and it cannot hold a field
//! inside another field — which `IF`, `SKIPIF` and half of Word's own fields
//! need, because what they compare is itself a field.
//!
//! So the format has a second way: a run holding `w:fldChar` of type `begin`,
//! then runs of `w:instrText` spelling the instruction out, then a `separate`
//! marker, then the runs showing the answer, then `end`. Nothing is nested
//! inside anything, so nesting works: a field inside a field is simply another
//! begin and end inside the first one's instruction.
//!
//! Word writes almost everything this way — a page number, a table of
//! contents, a cross-reference. A reader that only understands `w:fldSimple`
//! sees the answer runs as ordinary text and does not know they are a field,
//! which is why this exists.
//!
//! # How a nested field is written down here
//!
//! Inside the instruction, in braces: `SKIPIF { MERGEFIELD City } = "Leeds"`.
//! That is how field codes are written everywhere they are written for people
//! to read, and it is what [`crate::merge`] reads back when it works out
//! whether to skip a record.

use wp_xml::tree::Element;

use crate::read::W;

/// Which marker a run carries, if it carries one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marker {
    /// The field starts here, and its instruction follows.
    Begin,
    /// The instruction ends here, and the answer follows.
    Separate,
    /// The field ends here.
    End,
}

impl Marker {
    #[must_use]
    pub fn from_word(word: &str) -> Option<Self> {
        match word {
            "begin" => Some(Self::Begin),
            "separate" => Some(Self::Separate),
            "end" => Some(Self::End),
            _ => None,
        }
    }
}

/// The marker a run carries, if it is a marker run rather than a run of text.
#[must_use]
pub fn marker_of(run: &Element) -> Option<Marker> {
    let character = run.child(Some(W), "fldChar")?;
    // A `begin` with no type attribute is what the schema says a missing one
    // means, and Word does write them.
    let word = character.attribute(Some(W), "fldCharType").unwrap_or("begin");
    Marker::from_word(word)
}

/// The instruction text a run spells out, if it spells any.
#[must_use]
pub fn instruction_of(run: &Element) -> Option<String> {
    let mut out = String::new();
    for child in run.child_elements() {
        if child.is(Some(W), "instrText") {
            out.push_str(&child.text_content());
        }
    }
    (!out.is_empty()).then_some(out)
}

/// One field being read, and how far through it the reader has got.
#[derive(Clone, Debug, Default)]
pub struct Frame {
    /// What the instruction says so far.
    pub instruction: String,
    /// Whether the answer has started, which is what `separate` says.
    pub in_result: bool,
}

/// The stack of fields the reader is inside.
///
/// A stack rather than one field because fields nest: the whole point of
/// writing a field this way is that another one can sit inside its
/// instruction.
#[derive(Clone, Debug, Default)]
pub struct Fields {
    frames: Vec<Frame>,
}

impl Fields {
    /// Whether anything is open.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// A field starts.
    pub fn begin(&mut self) {
        self.frames.push(Frame::default());
    }

    /// The instruction ends and the answer starts.
    pub fn separate(&mut self) {
        if let Some(frame) = self.frames.last_mut() {
            frame.in_result = true;
        }
    }

    /// A field ends. Gives back its instruction, tidied.
    ///
    /// A field that closes inside another one's instruction is written into
    /// that instruction in braces, which is how a nested field reads.
    pub fn end(&mut self) -> Option<String> {
        let frame = self.frames.pop()?;
        let instruction = frame.instruction.trim().to_owned();
        if let Some(outer) = self.frames.last_mut() {
            if !outer.in_result {
                outer.instruction.push_str(" { ");
                outer.instruction.push_str(&instruction);
                outer.instruction.push_str(" } ");
            }
        }
        Some(instruction)
    }

    /// Adds to the instruction of the field being read.
    pub fn add_instruction(&mut self, text: &str) {
        if let Some(frame) = self.frames.last_mut() {
            if !frame.in_result {
                frame.instruction.push_str(text);
            }
        }
    }

    /// The instruction of the innermost field whose answer is being read.
    ///
    /// Nothing when no field is open, and nothing while an instruction is
    /// still being spelled out — the runs there are the instruction, not text
    /// the document says.
    #[must_use]
    pub fn current(&self) -> Option<String> {
        let frame = self.frames.last()?;
        frame.in_result.then(|| frame.instruction.trim().to_owned())
    }

    /// Whether what is being read now is part of an instruction rather than
    /// part of the document.
    #[must_use]
    pub fn in_instruction(&self) -> bool {
        self.frames.last().is_some_and(|frame| !frame.in_result)
    }
}

/// Builds the runs that write one field the long way.
///
/// The instruction may name nested fields in braces; each becomes a field of
/// its own inside this one's instruction, which is the only way the format has
/// of saying so.
#[must_use]
pub fn field_runs(instruction: &str, result: &str, prefix: Option<&str>) -> Vec<Element> {
    let mut out = vec![marker_run(prefix, "begin")];
    for piece in split_nested(instruction) {
        match piece {
            Piece::Text(text) => out.push(instruction_run(prefix, &text)),
            Piece::Nested(inner) => {
                // A nested field is written whole, inside the instruction of
                // the one that holds it — begin, instruction, separate, end.
                out.extend(field_runs(&inner, "", prefix));
            }
        }
    }
    out.push(marker_run(prefix, "separate"));
    if !result.is_empty() {
        out.push(crate::edit::run_element(&crate::model::Run::text(result), prefix));
    }
    out.push(marker_run(prefix, "end"));
    out
}

/// A piece of an instruction: plain text, or a field written in braces.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Piece {
    Text(String),
    Nested(String),
}

/// Splits an instruction into its plain parts and its nested fields.
fn split_nested(instruction: &str) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut depth = 0usize;
    let mut nested = String::new();

    for character in instruction.chars() {
        match character {
            '{' => {
                if depth == 0 {
                    if !current.trim().is_empty() {
                        out.push(Piece::Text(core::mem::take(&mut current)));
                    }
                    current.clear();
                } else {
                    nested.push(character);
                }
                depth += 1;
            }
            '}' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    out.push(Piece::Nested(nested.trim().to_owned()));
                    nested.clear();
                } else {
                    nested.push(character);
                }
            }
            _ if depth > 0 => nested.push(character),
            _ => current.push(character),
        }
    }

    // A brace nobody closed is text, not a field: the instruction is what
    // somebody wrote, and refusing it over one key would lose the lot.
    if depth > 0 {
        current.push('{');
        current.push_str(&nested);
    }
    if !current.trim().is_empty() {
        out.push(Piece::Text(current));
    }
    out
}

/// A run holding one marker.
fn marker_run(prefix: Option<&str>, kind: &str) -> Element {
    let mut run = Element::new(&crate::edit::name_with(prefix, "r"), Some(W));
    let mut character = Element::new(&crate::edit::name_with(prefix, "fldChar"), Some(W));
    character.set_namespaced_attribute(&crate::edit::name_with(prefix, "fldCharType"), W, kind);
    run.push_element(character);
    run
}

/// A run holding a piece of instruction text.
fn instruction_run(prefix: Option<&str>, text: &str) -> Element {
    let mut run = Element::new(&crate::edit::name_with(prefix, "r"), Some(W));
    let mut body = Element::new(&crate::edit::name_with(prefix, "instrText"), Some(W));
    body.set_text(text);
    // The spaces round a field code are part of it: `MERGEFIELDCity` is not a
    // field, and a reader that trims them makes one.
    body.set_namespaced_attribute("xml:space", crate::edit::XML_NAMESPACE, "preserve");
    run.push_element(body);
    run
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_marker_is_read_by_the_name_the_format_gives_it() {
        assert_eq!(Marker::from_word("begin"), Some(Marker::Begin));
        assert_eq!(Marker::from_word("separate"), Some(Marker::Separate));
        assert_eq!(Marker::from_word("end"), Some(Marker::End));
        assert_eq!(Marker::from_word("something else"), None);
    }

    #[test]
    fn a_field_gives_back_its_instruction_when_it_ends() {
        let mut fields = Fields::default();
        fields.begin();
        fields.add_instruction(" PAGE ");
        fields.separate();
        assert_eq!(fields.current().as_deref(), Some("PAGE"));
        assert_eq!(fields.end().as_deref(), Some("PAGE"));
        assert!(fields.is_empty());
    }

    #[test]
    fn text_in_the_instruction_is_not_text_of_the_document() {
        let mut fields = Fields::default();
        fields.begin();
        assert!(fields.in_instruction());
        assert_eq!(fields.current(), None);
        fields.separate();
        assert!(!fields.in_instruction());
    }

    #[test]
    fn a_field_inside_another_is_written_into_its_instruction() {
        let mut fields = Fields::default();
        fields.begin();
        fields.add_instruction(" SKIPIF ");
        fields.begin();
        fields.add_instruction(" MERGEFIELD City ");
        fields.end();
        fields.add_instruction(" = \"Leeds\"");
        fields.separate();
        assert_eq!(fields.current().as_deref(), Some("SKIPIF  { MERGEFIELD City }  = \"Leeds\""));
    }

    #[test]
    fn nothing_open_is_nothing_to_report() {
        let fields = Fields::default();
        assert!(fields.is_empty());
        assert_eq!(fields.current(), None);
        assert!(!fields.in_instruction());
    }

    #[test]
    fn an_instruction_splits_into_its_plain_parts_and_its_fields() {
        assert_eq!(
            split_nested("SKIPIF { MERGEFIELD City } = \"Leeds\""),
            vec![
                Piece::Text("SKIPIF ".to_owned()),
                Piece::Nested("MERGEFIELD City".to_owned()),
                Piece::Text(" = \"Leeds\"".to_owned()),
            ]
        );
    }

    #[test]
    fn an_instruction_with_no_fields_in_it_is_one_piece() {
        assert_eq!(split_nested("PAGE"), vec![Piece::Text("PAGE".to_owned())]);
    }

    #[test]
    fn a_brace_nobody_closed_is_text() {
        let split = split_nested("IF { MERGEFIELD City");
        assert!(
            split.iter().all(|piece| matches!(piece, Piece::Text(_))),
            "an unclosed brace became a field: {split:?}"
        );
        let joined: String = split
            .iter()
            .map(|piece| match piece {
                Piece::Text(text) => text.clone(),
                Piece::Nested(_) => String::new(),
            })
            .collect();
        assert_eq!(joined, "IF { MERGEFIELD City");
    }

    #[test]
    fn a_field_is_written_as_begin_instruction_separate_end() {
        let runs = field_runs("PAGE", "", None);
        assert_eq!(runs.len(), 4);
        assert_eq!(marker_of(&runs[0]), Some(Marker::Begin));
        assert_eq!(instruction_of(&runs[1]).as_deref(), Some("PAGE"));
        assert_eq!(marker_of(&runs[2]), Some(Marker::Separate));
        assert_eq!(marker_of(&runs[3]), Some(Marker::End));
    }

    #[test]
    fn a_field_with_an_answer_carries_the_answer_between_the_markers() {
        let runs = field_runs("PAGE", "7", None);
        assert_eq!(runs.len(), 5);
        assert_eq!(crate::read::read_run(&runs[3]).plain_text(), "7");
    }

    #[test]
    fn a_nested_field_is_written_inside_the_instruction() {
        let runs = field_runs("SKIPIF { MERGEFIELD City } = \"Leeds\"", "", None);
        // begin, "SKIPIF ", the whole nested field, " = ...", separate, end.
        let markers: Vec<Option<Marker>> = runs.iter().map(marker_of).collect();
        assert_eq!(markers[0], Some(Marker::Begin));
        assert_eq!(markers.iter().filter(|m| **m == Some(Marker::Begin)).count(), 2);
        assert_eq!(markers.iter().filter(|m| **m == Some(Marker::End)).count(), 2);
    }

    #[test]
    fn what_is_written_reads_back_as_the_instruction_it_was_written_from() {
        let runs = field_runs("SKIPIF { MERGEFIELD City } = \"Leeds\"", "", None);
        let mut fields = Fields::default();
        let mut outermost = None;
        for run in &runs {
            match marker_of(run) {
                Some(Marker::Begin) => fields.begin(),
                Some(Marker::Separate) => fields.separate(),
                Some(Marker::End) => {
                    let instruction = fields.end();
                    if fields.is_empty() {
                        outermost = instruction;
                    }
                }
                None => {
                    if let Some(text) = instruction_of(run) {
                        fields.add_instruction(&text);
                    }
                }
            }
        }
        let read = outermost.expect("an instruction");
        assert!(read.starts_with("SKIPIF"), "{read}");
        assert!(read.contains("MERGEFIELD City"), "{read}");
        assert!(read.contains("Leeds"), "{read}");
    }
}
