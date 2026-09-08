//! The table of authorities: the list of cases and statutes a legal document
//! cites, with the pages each is cited on.
//!
//! # Why it is not the index
//!
//! It looks like one and is built the same way — marks scattered through the
//! text, gathered into a field — but it differs in two things that matter.
//!
//! A mark carries *two* citations: the long one, written out in full the first
//! time a case is named, and the short one used everywhere after. The table
//! lists the long form and gathers the pages of both.
//!
//! And the marks are sorted into categories — cases in one list, statutes in
//! another — each with its own table. That is why the mark carries a number
//! saying which category it belongs to, and why the table's field names one.

use wp_xml::tree::{Element, Node};

use crate::history::EditKind;
use crate::model::{Block, Run};
use crate::{edit, figures, position, read, Document};

/// Which list a citation belongs in.
///
/// The seven Word gives numbers to, in its order — the number is written into
/// the file, so it is the number and not the name that has to be right.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Category {
    #[default]
    Cases,
    Statutes,
    OtherAuthorities,
    Rules,
    Treatises,
    Regulations,
    ConstitutionalProvisions,
}

impl Category {
    /// The number the format knows it by, counting from one.
    #[must_use]
    pub fn number(self) -> u8 {
        match self {
            Self::Cases => 1,
            Self::Statutes => 2,
            Self::OtherAuthorities => 3,
            Self::Rules => 4,
            Self::Treatises => 5,
            Self::Regulations => 6,
            Self::ConstitutionalProvisions => 7,
        }
    }

    /// Reads one back out of a file.
    #[must_use]
    pub fn from_number(number: u8) -> Option<Self> {
        Self::ALL.iter().copied().find(|category| category.number() == number)
    }

    /// The heading its table carries, which is what Word writes.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Cases => "Cases",
            Self::Statutes => "Statutes",
            Self::OtherAuthorities => "Other Authorities",
            Self::Rules => "Rules",
            Self::Treatises => "Treatises",
            Self::Regulations => "Regulations",
            Self::ConstitutionalProvisions => "Constitutional Provisions",
        }
    }

    /// Every one that can be picked, in Word's order.
    pub const ALL: &'static [Self] = &[
        Self::Cases,
        Self::Statutes,
        Self::OtherAuthorities,
        Self::Rules,
        Self::Treatises,
        Self::Regulations,
        Self::ConstitutionalProvisions,
    ];
}

/// One citation somebody marked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Authority {
    /// The citation written out in full, which is what the table lists.
    pub long: String,
    /// The short form used after the first mention, if there is one.
    pub short: String,
    pub category: Category,
    pub paragraph: usize,
    pub offset: usize,
    /// Which page it fell on, when that is known.
    pub page: usize,
}

/// The instruction that generates a table for one category.
#[must_use]
pub fn authorities_instruction(category: Category) -> String {
    format!("TOA \\h \\c \"{}\" \\p", category.number())
}

impl Document {
    /// Marks the citation at the caret for the table of authorities.
    pub fn mark_authority(&mut self, long: &str, short: &str, category: Category) -> bool {
        let long = long.trim();
        if long.is_empty() {
            return false;
        }
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

        // Like an index mark, this shows nothing and takes up nothing: a field
        // with an instruction and no result.
        let quoted = |text: &str| text.replace('"', "'");
        let mut instruction = format!(" TA \\l \"{}\"", quoted(long));
        if !short.trim().is_empty() {
            instruction.push_str(&format!(" \\s \"{}\"", quoted(short.trim())));
        }
        instruction.push_str(&format!(" \\c {} ", category.number()));

        let mut field =
            Element::new(&edit::name_with(prefix.as_deref(), "fldSimple"), Some(read::W));
        field.set_namespaced_attribute(
            &edit::name_with(prefix.as_deref(), "instr"),
            read::W,
            &instruction,
        );
        paragraph.insert_element(at, field);

        self.mark_modified();
        true
    }

    /// Every citation marked, in reading order.
    #[must_use]
    pub fn authorities(&self, pages: &[usize]) -> Vec<Authority> {
        let mut out = Vec::new();
        for index in 0..self.paragraph_count() {
            let Some(paragraph) = self.paragraph_element(index) else { continue };
            let mut offset = 0usize;
            walk_marks(paragraph, index, &mut offset, pages, &mut out);
        }
        out
    }

    /// Puts a table of authorities at the caret, replacing one that is there.
    ///
    /// Returns how many citations it gathered.
    pub fn insert_authorities(&mut self, category: Category, pages: &[usize]) -> usize {
        let mut gathered: Vec<(String, Vec<usize>)> = Vec::new();
        for authority in self.authorities(pages) {
            if authority.category != category {
                continue;
            }
            // The short form and the long one are the same authority, so their
            // pages go on one line — which is the whole point of marking both.
            match gathered.iter_mut().find(|(text, _)| *text == authority.long) {
                Some((_, seen)) => {
                    if authority.page > 0 && !seen.contains(&authority.page) {
                        seen.push(authority.page);
                    }
                }
                None => {
                    let first = if authority.page > 0 { vec![authority.page] } else { Vec::new() };
                    gathered.push((authority.long.clone(), first));
                }
            }
        }
        gathered.sort_by_key(|(text, _)| text.to_lowercase());

        let instruction = authorities_instruction(category);
        let caret = self.caret();
        self.record(EditKind::Structural, caret, false);
        let at = match self.field_table_range(&instruction) {
            Some((first, last)) => {
                self.remove_paragraphs(first, last);
                first
            }
            None => caret.paragraph,
        };

        let mut blocks = vec![Block::Paragraph(figures::field_paragraph(
            vec![figures::heading_run(category.label(), &instruction)],
            0,
        ))];
        if gathered.is_empty() {
            blocks.push(Block::Paragraph(figures::field_paragraph(
                vec![Run::field(
                    &instruction,
                    &format!("No {} are marked", category.label().to_lowercase()),
                )],
                0,
            )));
        }
        for (text, pages) in &gathered {
            let line = if pages.is_empty() {
                text.clone()
            } else {
                let numbers: Vec<String> = pages.iter().map(ToString::to_string).collect();
                format!("{text}\t{}", numbers.join(", "))
            };
            blocks.push(Block::Paragraph(figures::field_paragraph(
                vec![Run::field(&instruction, &line)],
                0,
            )));
        }

        self.write_generated(at, &blocks);
        gathered.len()
    }

    /// Whether the document already has a table for a category.
    #[must_use]
    pub fn has_authorities(&self, category: Category) -> bool {
        self.field_table_range(&authorities_instruction(category)).is_some()
    }
}

/// Finds every citation mark in an element, with where it sits.
fn walk_marks(
    element: &Element,
    paragraph: usize,
    offset: &mut usize,
    pages: &[usize],
    out: &mut Vec<Authority>,
) {
    for node in &element.children {
        let Some(child) = node.as_element() else { continue };
        if child.namespace.as_deref() == Some(read::W) && child.local_name() == "fldSimple" {
            let instruction = child.attribute(Some(read::W), "instr").unwrap_or_default();
            if let Some(mut authority) = read_mark(instruction) {
                authority.paragraph = paragraph;
                authority.offset = *offset;
                authority.page = pages.get(paragraph).copied().unwrap_or(0);
                out.push(authority);
                // A mark takes up no room in the text, so the offset does not
                // move past it.
                continue;
            }
        }
        if child.namespace.as_deref() == Some(read::W) && child.local_name() == "r" {
            *offset += edit::measured_length(child);
            continue;
        }
        walk_marks(child, paragraph, offset, pages, out);
    }
}

/// Reads a `TA` instruction, if that is what it is.
fn read_mark(instruction: &str) -> Option<Authority> {
    let instruction = instruction.trim();
    let rest = instruction.strip_prefix("TA ").or_else(|| instruction.strip_prefix("TA\t"))?;

    let long = switch_value(rest, 'l')?;
    let short = switch_value(rest, 's').unwrap_or_default();
    let category =
        switch_number(rest, 'c').and_then(Category::from_number).unwrap_or(Category::Cases);

    Some(Authority { long, short, category, paragraph: 0, offset: 0, page: 0 })
}

/// The quoted value of a switch, such as the `\l "…"` of a `TA` field.
fn switch_value(instruction: &str, switch: char) -> Option<String> {
    let marker = format!("\\{switch} ");
    let at = instruction.find(&marker)? + marker.len();
    let rest = instruction.get(at..)?.trim_start();
    let quoted = rest.strip_prefix('"')?;
    let end = quoted.find('"')?;
    Some(quoted[..end].to_owned())
}

/// The unquoted number of a switch, such as the `\c 1` of a `TA` field.
fn switch_number(instruction: &str, switch: char) -> Option<u8> {
    let marker = format!("\\{switch} ");
    let at = instruction.find(&marker)? + marker.len();
    let rest = instruction.get(at..)?.trim_start();
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// Kept so this module names what it walks past.
const _: fn(&Node) -> Option<&Element> = Node::as_element;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_category_survives_being_written_and_read_back() {
        for category in Category::ALL {
            assert_eq!(Category::from_number(category.number()), Some(*category));
        }
    }

    #[test]
    fn a_number_no_category_uses_is_no_category() {
        assert_eq!(Category::from_number(0), None);
        assert_eq!(Category::from_number(8), None);
    }

    #[test]
    fn the_instruction_names_its_category() {
        assert_eq!(authorities_instruction(Category::Statutes), "TOA \\h \\c \"2\" \\p");
    }

    #[test]
    fn a_mark_is_read_back_from_its_instruction() {
        let mark = read_mark(r#" TA \l "Smith v Jones" \s "Smith" \c 1 "#).expect("a mark");
        assert_eq!(mark.long, "Smith v Jones");
        assert_eq!(mark.short, "Smith");
        assert_eq!(mark.category, Category::Cases);
    }

    #[test]
    fn a_mark_without_a_short_form_reads_back_without_one() {
        let mark = read_mark(r#" TA \l "Smith v Jones" \c 2 "#).expect("a mark");
        assert!(mark.short.is_empty());
        assert_eq!(mark.category, Category::Statutes);
    }

    #[test]
    fn anything_that_is_not_a_mark_is_not_read_as_one() {
        assert!(read_mark(" PAGE ").is_none());
        assert!(read_mark(r#" XE "apples" "#).is_none());
        // A `TA` with no citation is not a citation.
        assert!(read_mark(r#" TA \c 1 "#).is_none());
    }

    #[test]
    fn a_switch_is_read_out_of_the_middle_of_an_instruction() {
        assert_eq!(switch_value(r#"\l "one" \s "two""#, 's').as_deref(), Some("two"));
        assert_eq!(switch_number(r#"\l "one" \c 4"#, 'c'), Some(4));
        assert_eq!(switch_value(r#"\l "one""#, 's'), None);
    }
}
