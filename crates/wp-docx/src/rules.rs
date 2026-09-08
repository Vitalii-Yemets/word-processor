//! The rules a mail merge can carry: skip this record, say one thing or
//! another, number the letters.
//!
//! # Which of Word's rules are here
//!
//! Word's Rules menu has nine. Four are here, and they are the four that mean
//! the same thing in a merge that makes one letter per recipient:
//!
//! - **If…Then…Else…** — one wording for some recipients and another for the
//!   rest.
//! - **Skip Record If…** — leave a recipient out.
//! - **Merge Record #** — which recipient this is, counting from one.
//! - **Merge Sequence #** — how many have been merged so far.
//!
//! The five that are not here — Next Record, Next Record If, Ask, Fill-in and
//! Set Bookmark — all need something this merge does not have. The Next family
//! puts several recipients into one document, which is what a sheet of labels
//! is and what a letter is not; Ask and Fill-in stop the merge to ask a
//! question. Writing the field codes without doing what they say would produce
//! documents that behave one way here and another way in Word, which is worse
//! than not offering them.
//!
//! # Why the condition is a field inside a field
//!
//! `SKIPIF` compares something with something, and the something is a merge
//! field — so the instruction has a field inside it. That cannot be written as
//! `w:fldSimple`, which is why [`crate::fields`] exists.

use crate::merge::merge_instruction;

/// One of the rules a merge can carry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    If,
    SkipIf,
    RecordNumber,
    SequenceNumber,
}

impl Rule {
    pub const ALL: &'static [Self] =
        &[Self::If, Self::SkipIf, Self::RecordNumber, Self::SequenceNumber];

    /// What the menu calls it, which is what Word calls it.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::If => "If…Then…Else…",
            Self::SkipIf => "Skip Record If…",
            Self::RecordNumber => "Merge Record #",
            Self::SequenceNumber => "Merge Sequence #",
        }
    }

    /// What to ask for, or nothing when the rule needs no answer.
    #[must_use]
    pub fn prompt(self) -> Option<&'static str> {
        match self {
            Self::If => Some("column; value; what to say; what to say otherwise"),
            Self::SkipIf => Some("column; value"),
            Self::RecordNumber | Self::SequenceNumber => None,
        }
    }

    /// The instruction this rule writes, from what was typed for it.
    ///
    /// Nothing when what was typed does not name a column, because a condition
    /// comparing nothing with something is not a condition.
    #[must_use]
    pub fn instruction(self, typed: &str) -> Option<String> {
        let parts: Vec<&str> = typed.split(';').map(str::trim).collect();
        match self {
            Self::RecordNumber => Some("MERGEREC".to_owned()),
            Self::SequenceNumber => Some("MERGESEQ".to_owned()),
            Self::SkipIf => {
                let column = parts.first().filter(|part| !part.is_empty())?;
                let value = parts.get(1).copied().unwrap_or_default();
                Some(format!("SKIPIF {{ {} }} = \"{value}\"", merge_instruction(column)))
            }
            Self::If => {
                let column = parts.first().filter(|part| !part.is_empty())?;
                let value = parts.get(1).copied().unwrap_or_default();
                let then = parts.get(2).copied().unwrap_or_default();
                let otherwise = parts.get(3).copied().unwrap_or_default();
                Some(format!(
                    "IF {{ {} }} = \"{value}\" \"{then}\" \"{otherwise}\"",
                    merge_instruction(column)
                ))
            }
        }
    }
}

/// A condition read back out of an instruction: which column, and what it is
/// compared with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Condition {
    pub column: String,
    /// `=` or `<>`, which are the two Word's own dialog offers.
    pub equal: bool,
    pub value: String,
}

impl Condition {
    /// Whether a record satisfies it.
    #[must_use]
    pub fn holds(&self, record: &[(String, String)]) -> bool {
        let found = record
            .iter()
            .find(|(header, _)| header.eq_ignore_ascii_case(&self.column))
            .map(|(_, value)| value.as_str())
            .unwrap_or_default();
        // Compared the way a person would: the case of a town is not part of
        // which town it is.
        let same = found.trim().eq_ignore_ascii_case(self.value.trim());
        same == self.equal
    }
}

/// What an instruction turns out to be, once it has been read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Instruction {
    /// `SKIPIF`, with the condition it skips on.
    Skip(Condition),
    /// `IF`, with the condition and the two things it says.
    Choose(Condition, String, String),
    /// `MERGEREC` or `MERGESEQ`.
    Number,
}

/// Reads one of the rules out of a field instruction.
///
/// Nothing for an instruction that is not one of them, which is most of them:
/// a page number and a table of contents both arrive here too.
#[must_use]
pub fn read_instruction(instruction: &str) -> Option<Instruction> {
    let trimmed = instruction.trim();
    let (word, rest) = split_word(trimmed);

    match word.to_ascii_uppercase().as_str() {
        "MERGEREC" | "MERGESEQ" => Some(Instruction::Number),
        "SKIPIF" => read_condition(rest).map(|(condition, _)| Instruction::Skip(condition)),
        "IF" => {
            let (condition, tail) = read_condition(rest)?;
            let strings = quoted_strings(tail);
            let then = strings.first().cloned().unwrap_or_default();
            let otherwise = strings.get(1).cloned().unwrap_or_default();
            Some(Instruction::Choose(condition, then, otherwise))
        }
        _ => None,
    }
}

/// The first word of an instruction, and what follows it.
fn split_word(instruction: &str) -> (&str, &str) {
    match instruction.find(char::is_whitespace) {
        Some(at) => (&instruction[..at], instruction[at..].trim_start()),
        None => (instruction, ""),
    }
}

/// Reads `{ MERGEFIELD City } = "Leeds"`, and says what is left after it.
fn read_condition(text: &str) -> Option<(Condition, &str)> {
    let open = text.find('{')?;
    let close = text[open..].find('}')? + open;
    let inner = text[open + 1..close].trim();
    let column = crate::merge::merge_column(inner)?;

    let rest = text[close + 1..].trim_start();
    // `<>` is Word's "is not", and the only other comparison its dialog offers
    // for a merge field.
    let (equal, rest) = if let Some(tail) = rest.strip_prefix("<>") {
        (false, tail)
    } else if let Some(tail) = rest.strip_prefix('=') {
        (true, tail)
    } else {
        (true, rest)
    };

    let strings = quoted_strings(rest);
    let value = strings.first().cloned().unwrap_or_default();
    // Whatever is after the value belongs to whoever asked.
    let after = match rest
        .find('"')
        .and_then(|first| rest[first + 1..].find('"').map(|second| first + 1 + second + 1))
    {
        Some(at) => &rest[at..],
        None => "",
    };
    Some((Condition { column, equal, value }, after))
}

/// Every double-quoted string in a piece of text, in order.
fn quoted_strings(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut inside: Option<String> = None;
    for character in text.chars() {
        match (character, &mut inside) {
            ('"', None) => inside = Some(String::new()),
            ('"', Some(_)) => {
                if let Some(finished) = inside.take() {
                    out.push(finished);
                }
            }
            (_, Some(current)) => current.push(character),
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> Vec<(String, String)> {
        vec![("Name".to_owned(), "Ann Roe".to_owned()), ("City".to_owned(), "Leeds".to_owned())]
    }

    #[test]
    fn every_rule_says_what_it_is_called() {
        for rule in Rule::ALL {
            assert!(!rule.label().is_empty());
        }
    }

    #[test]
    fn the_numbering_rules_need_nothing_typed() {
        assert_eq!(Rule::RecordNumber.prompt(), None);
        assert_eq!(Rule::SequenceNumber.prompt(), None);
        assert_eq!(Rule::RecordNumber.instruction("").as_deref(), Some("MERGEREC"));
    }

    #[test]
    fn a_condition_needs_a_column() {
        assert_eq!(Rule::SkipIf.instruction(""), None);
        assert_eq!(Rule::If.instruction("  ; Leeds"), None);
    }

    #[test]
    fn a_skip_rule_writes_the_field_inside_the_field() {
        let instruction = Rule::SkipIf.instruction("City; Leeds").expect("an instruction");
        assert_eq!(instruction, "SKIPIF { MERGEFIELD City } = \"Leeds\"");
    }

    #[test]
    fn a_choice_writes_both_things_it_might_say() {
        let instruction =
            Rule::If.instruction("City; Leeds; near you; far away").expect("an instruction");
        assert!(instruction.contains("\"near you\""), "{instruction}");
        assert!(instruction.contains("\"far away\""), "{instruction}");
    }

    #[test]
    fn every_rule_reads_back_as_what_it_was_written_as() {
        for (rule, typed) in [
            (Rule::SkipIf, "City; Leeds"),
            (Rule::If, "City; Leeds; yes; no"),
            (Rule::RecordNumber, ""),
            (Rule::SequenceNumber, ""),
        ] {
            let instruction = rule.instruction(typed).expect("an instruction");
            assert!(read_instruction(&instruction).is_some(), "{instruction}");
        }
    }

    #[test]
    fn a_skip_rule_reads_back_its_condition() {
        let read = read_instruction("SKIPIF { MERGEFIELD City } = \"Leeds\"");
        let Some(Instruction::Skip(condition)) = read else { panic!("a skip, got {read:?}") };
        assert_eq!(condition.column, "City");
        assert!(condition.equal);
        assert_eq!(condition.value, "Leeds");
    }

    #[test]
    fn a_choice_reads_back_both_of_its_answers() {
        let read = read_instruction("IF { MERGEFIELD City } = \"Leeds\" \"yes\" \"no\"");
        let Some(Instruction::Choose(condition, then, otherwise)) = read else {
            panic!("a choice, got {read:?}")
        };
        assert_eq!(condition.column, "City");
        assert_eq!(then, "yes");
        assert_eq!(otherwise, "no");
    }

    #[test]
    fn a_condition_can_be_the_other_way_round() {
        let read = read_instruction("SKIPIF { MERGEFIELD City } <> \"Leeds\"");
        let Some(Instruction::Skip(condition)) = read else { panic!("a skip") };
        assert!(!condition.equal);
    }

    #[test]
    fn a_condition_is_true_when_the_record_says_so() {
        let condition =
            Condition { column: "City".to_owned(), equal: true, value: "Leeds".to_owned() };
        assert!(condition.holds(&record()));

        let elsewhere =
            Condition { column: "City".to_owned(), equal: true, value: "York".to_owned() };
        assert!(!elsewhere.holds(&record()));
    }

    #[test]
    fn the_case_of_a_town_is_not_part_of_which_town_it_is() {
        let condition =
            Condition { column: "City".to_owned(), equal: true, value: "  leeds ".to_owned() };
        assert!(condition.holds(&record()));
    }

    #[test]
    fn a_column_the_record_does_not_have_is_empty_rather_than_missing() {
        let condition =
            Condition { column: "County".to_owned(), equal: true, value: String::new() };
        assert!(condition.holds(&record()), "an absent column is not the value asked for");
    }

    #[test]
    fn an_instruction_that_is_not_a_rule_is_not_read_as_one() {
        assert_eq!(read_instruction("PAGE"), None);
        assert_eq!(read_instruction("MERGEFIELD Name"), None);
        assert_eq!(read_instruction(""), None);
    }
}
