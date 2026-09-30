//! Mail merge: one letter written once and sent to everybody.
//!
//! # What the document holds and what it does not
//!
//! The letter holds `MERGEFIELD` fields — a name where a value goes — and, in
//! its settings, a note of where the values are to be read from. It does not
//! hold the values. That is the whole idea: the letter is one document however
//! many people it is for, and the list of people is a file beside it.
//!
//! # Why the list is a comma-separated file
//!
//! Word reads its recipients from Excel, from Access, from Outlook and from a
//! plain text file. The first three are file formats and a mail system, none of
//! which belongs inside a word processor. The fourth is a list of rows, which
//! is what a list of recipients is, and every one of the other three can write
//! one.

use wp_xml::tree::Element;

use crate::{edit, read, Document};

/// The people a letter is going to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Recipients {
    /// What each column is called, which is what a merge field names.
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl Recipients {
    /// How many people are on the list.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// One person's row, as a lookup from column name to value.
    ///
    /// A row with fewer values than there are columns reads as empty for the
    /// rest, which is what a list somebody left a gap in should do.
    #[must_use]
    pub fn record(&self, index: usize) -> Vec<(String, String)> {
        let Some(row) = self.rows.get(index) else { return Vec::new() };
        self.headers
            .iter()
            .enumerate()
            .map(|(at, header)| (header.clone(), row.get(at).cloned().unwrap_or_default()))
            .collect()
    }

    /// The value of one column for one person.
    ///
    /// A row with fewer values than there are columns reads as empty for the
    /// rest, which is the rule [`Recipients::record`] follows and has to be
    /// the same rule here: a list somebody added a column to has rows that
    /// stop short, and they are gaps rather than rows without the column.
    #[must_use]
    pub fn value(&self, index: usize, column: &str) -> Option<String> {
        let at = self.headers.iter().position(|header| header.eq_ignore_ascii_case(column))?;
        let row = self.rows.get(index)?;
        Some(row.get(at).cloned().unwrap_or_default())
    }

    /// The order the rows fall into when one column is sorted on.
    ///
    /// Given as the row numbers rather than as the rows themselves, so that
    /// whoever asked can decide whether to reorder the list or merely look at
    /// it in that order.
    ///
    /// Numbers sort as numbers where a whole column is numbers, and as words
    /// otherwise: a list of postcodes is words and a list of order numbers is
    /// not, and sorting the second as the first puts 10 before 9.
    #[must_use]
    pub fn sorted_by(&self, column: &str, ascending: bool) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.len()).collect();
        let numeric = (0..self.len()).all(|at| {
            self.value(at, column)
                .is_some_and(|value| value.trim().is_empty() || value.trim().parse::<f64>().is_ok())
        });

        order.sort_by(|left, right| {
            let (a, b) = (self.sorting_key(*left, column), self.sorting_key(*right, column));
            let ordering = if numeric {
                let (a, b) = (
                    a.trim().parse::<f64>().unwrap_or(0.0),
                    b.trim().parse::<f64>().unwrap_or(0.0),
                );
                a.partial_cmp(&b).unwrap_or(core::cmp::Ordering::Equal)
            } else {
                a.to_lowercase().cmp(&b.to_lowercase())
            };
            if ascending {
                ordering
            } else {
                ordering.reverse()
            }
        });
        order
    }

    /// What one row sorts by in one column.
    fn sorting_key(&self, index: usize, column: &str) -> String {
        self.value(index, column).unwrap_or_default()
    }

    /// The rows one column of which holds the words given.
    ///
    /// Compared without regard to case and by containing rather than by
    /// equalling, which is what somebody typing "london" into a filter box
    /// means. An empty column name looks in every column.
    #[must_use]
    pub fn matching(&self, column: &str, wanted: &str) -> Vec<usize> {
        let wanted = wanted.trim().to_lowercase();
        if wanted.is_empty() {
            return (0..self.len()).collect();
        }
        (0..self.len())
            .filter(|at| {
                if column.trim().is_empty() {
                    self.record(*at).iter().any(|(_, value)| value.to_lowercase().contains(&wanted))
                } else {
                    self.value(*at, column)
                        .is_some_and(|value| value.to_lowercase().contains(&wanted))
                }
            })
            .collect()
    }

    /// The rows that say the same thing as an earlier row.
    ///
    /// The first of each group is not in the answer: what a person wants from
    /// Find Duplicates is the copies, not the originals. Compared on every
    /// column, without regard to case or to the spaces round a value, because
    /// two lists merged by hand differ that way and mean the same person.
    #[must_use]
    pub fn duplicates(&self) -> Vec<usize> {
        let mut seen: Vec<String> = Vec::new();
        let mut out = Vec::new();
        for at in 0..self.len() {
            let key: String = self
                .record(at)
                .iter()
                .map(|(_, value)| value.trim().to_lowercase())
                .collect::<Vec<String>>()
                .join("\u{1}");
            if seen.contains(&key) {
                out.push(at);
            } else {
                seen.push(key);
            }
        }
        out
    }

    /// What is missing from a row for it to be an address.
    ///
    /// Word calls out to an add-in for this and says so plainly when there is
    /// none. What can be done here without one is the half that is arithmetic:
    /// a letter needs somebody to send it to and somewhere to send it, so a
    /// row with no name or no address is named as such. Whether the street
    /// exists is not a question a program with no map can answer, and this
    /// does not pretend to.
    #[must_use]
    pub fn missing_from(&self, index: usize) -> Vec<String> {
        /// What each part of an address may be called, in the order the
        /// answer names them.
        const WANTED: &[(&str, &[&str])] = &[
            ("a name", &["Last Name", "Surname", "Name", "First Name", "Company"]),
            (
                "an address",
                &[
                    "Address Line 1",
                    "Address",
                    "Street",
                    "City",
                    "Town",
                    "Postcode",
                    "Postal Code",
                    "ZIP",
                    "Email",
                    "Email Address",
                ],
            ),
        ];

        let record = self.record(index);
        let mut out = Vec::new();
        for (what, columns) in WANTED {
            let found = record.iter().any(|(header, value)| {
                !value.trim().is_empty()
                    && columns.iter().any(|name| header.eq_ignore_ascii_case(name))
            });
            if !found {
                out.push((*what).to_owned());
            }
        }
        out
    }

    /// Puts the rows in the order given, dropping any the order leaves out.
    pub fn reorder(&mut self, order: &[usize]) {
        let rows: Vec<Vec<String>> =
            order.iter().filter_map(|at| self.rows.get(*at).cloned()).collect();
        self.rows = rows;
    }

    /// Adds one person, by column name.
    ///
    /// A column the list has not got is added to it, because a list typed
    /// from nothing has whatever columns the first person needed.
    pub fn add(&mut self, values: &[(String, String)]) {
        for (header, _) in values {
            if !self.headers.iter().any(|found| found.eq_ignore_ascii_case(header)) {
                self.headers.push(header.clone());
            }
        }
        let row = self
            .headers
            .iter()
            .map(|header| {
                values
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(header))
                    .map(|(_, value)| value.clone())
                    .unwrap_or_default()
            })
            .collect();
        self.rows.push(row);
    }

    /// The whole list as the delimited file this program reads.
    ///
    /// Written so that a list typed from nothing is a file like any other:
    /// it can be opened again, edited elsewhere, and used by a program that
    /// never heard of this one.
    #[must_use]
    pub fn to_delimited(&self) -> String {
        let mut out = String::new();
        for row in core::iter::once(&self.headers).chain(self.rows.iter()) {
            let line: Vec<String> = row.iter().map(|value| quoted(value)).collect();
            out.push_str(&line.join(","));
            out.push('\n');
        }
        out
    }

    /// Reads a list out of the bytes of a comma-separated file.
    ///
    /// The first row is the names of the columns, which is the convention every
    /// program that writes one of these follows.
    #[must_use]
    pub fn parse(bytes: &[u8]) -> Self {
        let text = wp_xml::decode_to_utf8(bytes)
            .map(|text| text.into_owned())
            .unwrap_or_else(|_| bytes.iter().map(|byte| *byte as char).collect());

        let mut rows = parse_rows(&text);
        if rows.is_empty() {
            return Self::default();
        }
        let headers = rows.remove(0);
        // A row of nothing at the end is what a file ending in a line break
        // looks like, and it is not a person.
        rows.retain(|row| row.iter().any(|value| !value.trim().is_empty()));
        Self { headers, rows }
    }
}

/// One value as a delimited file writes it: in quotation marks where it holds
/// a comma, a quotation mark or a line break, and with its own quotation marks
/// doubled, which is the one convention every program that reads these agrees
/// on.
fn quoted(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        return format!("\"{}\"", value.replace('"', "\"\""));
    }
    value.to_owned()
}

/// Splits a comma-separated file into rows of values.
///
/// Quotes are honoured: a value in quotation marks may hold commas and line
/// breaks, and two quotation marks inside one stand for a single one. Without
/// that a list of addresses would fall apart at the first comma in a street.
#[must_use]
fn parse_rows(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut value = String::new();
    let mut quoted = false;
    let mut characters = text.chars().peekable();

    while let Some(character) = characters.next() {
        if quoted {
            if character == '"' {
                // Two in a row are one quotation mark; one on its own ends the
                // quoted value.
                if characters.peek() == Some(&'"') {
                    characters.next();
                    value.push('"');
                } else {
                    quoted = false;
                }
            } else {
                value.push(character);
            }
            continue;
        }

        match character {
            '"' if value.is_empty() => quoted = true,
            ',' | ';' | '\t' => {
                row.push(core::mem::take(&mut value));
            }
            '\r' => {
                // A carriage return before a line feed is part of the same
                // break, not a row of its own.
                if characters.peek() == Some(&'\n') {
                    characters.next();
                }
                row.push(core::mem::take(&mut value));
                rows.push(core::mem::take(&mut row));
            }
            '\n' => {
                row.push(core::mem::take(&mut value));
                rows.push(core::mem::take(&mut row));
            }
            _ => value.push(character),
        }
    }

    if !value.is_empty() || !row.is_empty() {
        row.push(value);
        rows.push(row);
    }
    rows
}

/// The instruction a merge field carries.
#[must_use]
pub fn merge_instruction(column: &str) -> String {
    format!("MERGEFIELD {column}")
}

/// The column a `MERGEFIELD` instruction names, if that is what it is.
#[must_use]
pub fn merge_column(instruction: &str) -> Option<String> {
    let rest = instruction.trim().strip_prefix("MERGEFIELD")?;
    let name = rest.split_whitespace().next()?;
    if name.starts_with('\\') {
        return None;
    }
    Some(name.trim_matches('"').to_owned())
}

/// What kind of thing a merge produces.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    #[default]
    Letters,
    Envelopes,
    Labels,
    Directory,
    Email,
}

impl Kind {
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Letters => "formLetters",
            Self::Envelopes => "envelopes",
            Self::Labels => "mailingLabels",
            Self::Directory => "catalog",
            Self::Email => "email",
        }
    }

    #[must_use]
    pub fn from_word(word: &str) -> Self {
        Self::ALL.iter().copied().find(|kind| kind.word() == word).unwrap_or(Self::Letters)
    }

    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Letters => "Letters",
            Self::Envelopes => "Envelopes",
            Self::Labels => "Labels",
            Self::Directory => "Directory",
            Self::Email => "Email Messages",
        }
    }

    pub const ALL: &'static [Self] =
        &[Self::Letters, Self::Envelopes, Self::Labels, Self::Directory, Self::Email];
}

impl Document {
    /// What the document is being merged into, and where its list is.
    ///
    /// `None` when it is an ordinary document rather than a letter waiting for
    /// a list.
    #[must_use]
    pub fn merge_source(&self) -> Option<(Kind, String)> {
        let root = self.settings_root()?;
        let merge = root.child(Some(read::W), "mailMerge")?;

        let kind = merge
            .child(Some(read::W), "mainDocumentType")
            .and_then(|element| element.attribute(Some(read::W), "val"))
            .map_or(Kind::Letters, Kind::from_word);
        let path = merge
            .child(Some(read::W), "dataSource")
            .and_then(|source| source.child(Some(read::W), "id"))
            .map(Element::text_content)
            .or_else(|| {
                merge
                    .child(Some(read::W), "dataSource")
                    .and_then(|source| source.attribute(Some(read::W), "id").map(str::to_owned))
            })
            .unwrap_or_default();
        Some((kind, path))
    }

    /// Says which list the letter is for.
    pub fn set_merge_source(&mut self, kind: Kind, path: &str) -> bool {
        let Some(mut root) = self.settings_root() else { return false };
        root.remove_children_named(Some(read::W), "mailMerge");

        let prefix = self.prefix();
        let name = |local: &str| edit::name_with(prefix.as_deref(), local);
        let mut merge = Element::new(&name("mailMerge"), Some(read::W));

        let mut kind_element = Element::new(&name("mainDocumentType"), Some(read::W));
        kind_element.set_namespaced_attribute(&name("val"), read::W, kind.word());
        merge.push_element(kind_element);

        // A plain text file, which is what a comma-separated list is to Word.
        let mut data_type = Element::new(&name("dataType"), Some(read::W));
        data_type.set_namespaced_attribute(&name("val"), read::W, "textFile");
        merge.push_element(data_type);

        let mut source = Element::new(&name("dataSource"), Some(read::W));
        let mut id = Element::new(&name("id"), Some(read::W));
        id.set_text(path);
        source.push_element(id);
        merge.push_element(source);

        crate::edit::insert_ordered(&mut root, merge, crate::settings::SETTINGS_ORDER);
        if !self.save_settings_root(root) {
            return false;
        }
        self.note_change();
        true
    }

    /// Stops the document being a merge letter.
    pub fn clear_merge_source(&mut self) -> bool {
        let Some(mut root) = self.settings_root() else { return false };
        if root.child(Some(read::W), "mailMerge").is_none() {
            return false;
        }
        root.remove_children_named(Some(read::W), "mailMerge");
        if !self.save_settings_root(root) {
            return false;
        }
        self.note_change();
        true
    }

    /// Every column the letter asks for, in the order it first asks.
    #[must_use]
    pub fn merge_fields(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for paragraph in self.paragraph_elements() {
            gather_merge_fields(paragraph, &mut out);
        }
        out
    }

    /// The columns the letter asks for that the list does not have.
    ///
    /// What Word's "Check for Errors" is for: a letter that greets somebody by
    /// a column nobody has is a letter that goes out saying nothing.
    #[must_use]
    pub fn missing_merge_fields(&self, recipients: &Recipients) -> Vec<String> {
        self.merge_fields()
            .into_iter()
            .filter(|column| {
                !recipients.headers.iter().any(|header| header.eq_ignore_ascii_case(column))
            })
            .collect()
    }
}

/// Finds every merge field in an element.
fn gather_merge_fields(element: &Element, out: &mut Vec<String>) {
    for child in element.child_elements() {
        if child.namespace.as_deref() == Some(read::W) && child.local_name() == "fldSimple" {
            let instruction = child.attribute(Some(read::W), "instr").unwrap_or_default();
            if let Some(column) = merge_column(instruction) {
                if !out.iter().any(|seen| seen.eq_ignore_ascii_case(&column)) {
                    out.push(column);
                }
            }
        }
        gather_merge_fields(child, out);
    }
}

impl Document {
    /// Replaces every merge field with one recipient's values.
    ///
    /// What Finish & Merge does: the letter stops being a letter for everybody
    /// and becomes a letter for one person. The fields are gone afterwards,
    /// which is right — the finished letter is not to be merged again.
    pub fn apply_merge_record(&mut self, record: &[(String, String)]) -> usize {
        let caret = self.caret();
        self.record(crate::history::EditKind::Structural, caret, false);
        let prefix = self.prefix();

        let mut replaced = 0usize;
        replace_merge_fields(
            &mut self.tree_to_edit().root,
            record,
            prefix.as_deref(),
            &mut replaced,
        );
        if replaced > 0 {
            self.clamp_caret();
            self.note_change();
        }
        replaced
    }
}

/// Turns every merge field under an element into the words it stands for.
fn replace_merge_fields(
    element: &mut Element,
    record: &[(String, String)],
    prefix: Option<&str>,
    replaced: &mut usize,
) {
    let mut children = Vec::with_capacity(element.children.len());
    for node in core::mem::take(&mut element.children) {
        let Some(child) = node.as_element() else {
            children.push(node);
            continue;
        };

        if child.namespace.as_deref() == Some(read::W) && child.local_name() == "fldSimple" {
            let instruction = child.attribute(Some(read::W), "instr").unwrap_or_default();
            if let Some(column) = merge_column(instruction) {
                let value = record
                    .iter()
                    .find(|(header, _)| header.eq_ignore_ascii_case(&column))
                    .map(|(_, value)| value.clone())
                    .unwrap_or_default();

                // The runs inside the field are kept and their text replaced,
                // so the value comes out in whatever the field was formatted
                // in — which is what a letter written in a particular hand
                // expects.
                let mut run = crate::model::Run::text(&value);
                if let Some(first) = child
                    .child_elements()
                    .find(|inner| inner.local_name() == "r")
                    .and_then(|inner| inner.child(Some(read::W), "rPr"))
                {
                    run.properties = read::read_run_properties(first);
                }
                children.extend(
                    edit::run_elements(&run, prefix).into_iter().map(wp_xml::tree::Node::Element),
                );
                *replaced += 1;
                continue;
            }
        }

        let mut child = child.clone();
        replace_merge_fields(&mut child, record, prefix, replaced);
        children.push(wp_xml::tree::Node::Element(child));
    }
    element.children = children;
}

impl Document {
    /// Where every merge field sits in the text.
    ///
    /// A field covers the words it last worked out, which is what a reader sees
    /// and therefore what a shading has to cover.
    #[must_use]
    pub fn merge_field_ranges(&self) -> Vec<(crate::TextPosition, crate::TextPosition)> {
        let mut out = Vec::new();
        for (index, paragraph) in self.paragraph_elements().into_iter().enumerate() {
            let mut offset = 0usize;
            walk_merge_ranges(paragraph, index, &mut offset, &mut out);
        }
        out
    }
}

/// Finds every merge field in a paragraph, with the stretch it covers.
fn walk_merge_ranges(
    element: &Element,
    paragraph: usize,
    offset: &mut usize,
    out: &mut Vec<(crate::TextPosition, crate::TextPosition)>,
) {
    for node in &element.children {
        let Some(child) = node.as_element() else { continue };
        if child.namespace.as_deref() != Some(read::W) {
            continue;
        }

        if child.local_name() == "fldSimple" {
            let instruction = child.attribute(Some(read::W), "instr").unwrap_or_default();
            let start = *offset;
            let mut inner = *offset;
            walk_merge_ranges(child, paragraph, &mut inner, out);
            // Whatever the field's runs took up, counted the way the caret
            // counts it.
            inner = start + edit::measured_length(child);
            if merge_column(instruction).is_some() {
                out.push((
                    crate::TextPosition::new(paragraph, start),
                    crate::TextPosition::new(paragraph, inner),
                ));
            }
            *offset = inner;
            continue;
        }
        if child.local_name() == "r" {
            *offset += edit::measured_length(child);
            continue;
        }
        walk_merge_ranges(child, paragraph, offset, out);
    }
}

impl Document {
    /// Puts a merge rule at the caret.
    ///
    /// The rule is written as a field the long way round, because the condition
    /// inside it is itself a field — see [`crate::fields`].
    pub fn insert_rule(&mut self, instruction: &str) -> bool {
        if instruction.trim().is_empty() {
            return false;
        }
        let caret = self.caret();
        self.record(crate::history::EditKind::Structural, caret, false);

        let prefix = self.prefix();
        let runs = crate::fields::field_runs(instruction, "", prefix.as_deref());
        let Some(path) = crate::position::paragraph_path(&self.tree().root, caret.paragraph) else {
            return false;
        };
        let Some(paragraph) =
            crate::edit::element_at_path_mut(&mut self.tree_to_edit().root, &path)
        else {
            return false;
        };

        crate::format::split_runs_at_offset(paragraph, caret.offset);
        let position = crate::edit::child_position_at_offset(paragraph, caret.offset);
        for (offset, run) in runs.into_iter().enumerate() {
            paragraph.insert_element(position + offset, run);
        }

        self.note_change();
        true
    }

    /// Whether a rule in the document says to leave this recipient out.
    #[must_use]
    pub fn record_is_skipped(&self, record: &[(String, String)]) -> bool {
        for paragraph in self.paragraph_elements() {
            if paragraph_skips(paragraph, record) {
                return true;
            }
        }
        false
    }

    /// Works the rules out for one recipient, replacing each with what it says.
    ///
    /// `number` is which recipient this is, counting from one, which is what
    /// `MERGEREC` and `MERGESEQ` answer with.
    pub fn apply_merge_rules(&mut self, record: &[(String, String)], number: usize) -> usize {
        let caret = self.caret();
        self.record(crate::history::EditKind::Structural, caret, false);
        let prefix = self.prefix();

        let mut replaced = 0usize;
        rewrite_rules(
            &mut self.tree_to_edit().root,
            record,
            number,
            prefix.as_deref(),
            &mut replaced,
        );
        if replaced > 0 {
            self.clamp_caret();
            self.note_change();
        }
        replaced
    }
}

/// Whether any rule in one paragraph skips the record.
fn paragraph_skips(element: &Element, record: &[(String, String)]) -> bool {
    let mut open = crate::fields::Fields::default();
    for child in element.child_elements() {
        match crate::fields::marker_of(child) {
            Some(crate::fields::Marker::Begin) => open.begin(),
            Some(crate::fields::Marker::Separate) => open.separate(),
            Some(crate::fields::Marker::End) => {
                let instruction = open.end().unwrap_or_default();
                if open.is_empty() {
                    if let Some(crate::rules::Instruction::Skip(condition)) =
                        crate::rules::read_instruction(&instruction)
                    {
                        if condition.holds(record) {
                            return true;
                        }
                    }
                }
            }
            None => {
                if let Some(text) = crate::fields::instruction_of(child) {
                    open.add_instruction(&text);
                } else if open.is_empty() && paragraph_skips(child, record) {
                    return true;
                }
            }
        }
    }
    false
}

/// Replaces every rule under an element with what it works out to.
fn rewrite_rules(
    element: &mut Element,
    record: &[(String, String)],
    number: usize,
    prefix: Option<&str>,
    replaced: &mut usize,
) {
    let children = core::mem::take(&mut element.children);
    let mut out: Vec<wp_xml::tree::Node> = Vec::with_capacity(children.len());

    // The field being collected, and how deep inside one we are: a rule holds
    // a merge field inside its condition, so fields nest.
    let mut open = crate::fields::Fields::default();
    let mut buffer: Vec<wp_xml::tree::Node> = Vec::new();
    let mut depth = 0usize;

    for node in children {
        let marker = node.as_element().and_then(crate::fields::marker_of);

        if marker == Some(crate::fields::Marker::Begin) {
            open.begin();
            depth += 1;
            buffer.push(node);
            continue;
        }

        if depth > 0 {
            match marker {
                Some(crate::fields::Marker::Separate) => open.separate(),
                Some(crate::fields::Marker::End) => {
                    let instruction = open.end().unwrap_or_default();
                    depth -= 1;
                    buffer.push(node);
                    if depth == 0 {
                        match crate::rules::read_instruction(&instruction) {
                            Some(answer) => {
                                if let Some(text) = answered(&answer, record, number) {
                                    let run = crate::model::Run::text(&text);
                                    out.extend(
                                        crate::edit::run_elements(&run, prefix)
                                            .into_iter()
                                            .map(wp_xml::tree::Node::Element),
                                    );
                                }
                                *replaced += 1;
                            }
                            // Not a rule: a page number, a reference, anything
                            // else. Left exactly as it was.
                            None => out.append(&mut buffer),
                        }
                        buffer.clear();
                    }
                    continue;
                }
                None => {
                    if let Some(text) = crate::fields::instruction_of(
                        node.as_element().expect("checked by the marker read"),
                    ) {
                        open.add_instruction(&text);
                    }
                }
                Some(crate::fields::Marker::Begin) => {}
            }
            buffer.push(node);
            continue;
        }

        let mut node = node;
        if let Some(child) = node.as_element_mut() {
            rewrite_rules(child, record, number, prefix, replaced);
        }
        out.push(node);
    }

    // A field nobody closed: kept rather than thrown away.
    out.append(&mut buffer);
    element.children = out;
}

/// What a rule works out to for one recipient, or nothing when it produces no
/// text at all.
fn answered(
    instruction: &crate::rules::Instruction,
    record: &[(String, String)],
    number: usize,
) -> Option<String> {
    match instruction {
        crate::rules::Instruction::Number => Some(number.to_string()),
        crate::rules::Instruction::Choose(condition, then, otherwise) => {
            let chosen = if condition.holds(record) { then } else { otherwise };
            (!chosen.is_empty()).then(|| chosen.clone())
        }
        // A skipped record is left out whole, by whoever is doing the merging;
        // the rule itself says nothing on the page.
        crate::rules::Instruction::Skip(_) => None,
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_list_is_read_into_columns_and_rows() {
        let list = Recipients::parse(b"Name,Town\nAnn,Leeds\nJohn,Hull\n");
        assert_eq!(list.headers, vec!["Name".to_owned(), "Town".to_owned()]);
        assert_eq!(list.len(), 2);
        assert_eq!(list.value(0, "Name").as_deref(), Some("Ann"));
        assert_eq!(list.value(1, "Town").as_deref(), Some("Hull"));
    }

    #[test]
    fn a_column_is_found_whatever_case_it_is_asked_for_in() {
        let list = Recipients::parse(b"Name\nAnn\n");
        assert_eq!(list.value(0, "name").as_deref(), Some("Ann"));
        assert_eq!(list.value(0, "NAME").as_deref(), Some("Ann"));
    }

    #[test]
    fn a_value_in_quotes_may_hold_a_comma() {
        let list = Recipients::parse(b"Name,Address\nAnn,\"12 High Street, Leeds\"\n");
        assert_eq!(list.value(0, "Address").as_deref(), Some("12 High Street, Leeds"));
    }

    #[test]
    fn two_quotation_marks_inside_a_value_stand_for_one() {
        let list = Recipients::parse(b"Name\n\"Ann \"\"Annie\"\" Roe\"\n");
        assert_eq!(list.value(0, "Name").as_deref(), Some("Ann \"Annie\" Roe"));
    }

    #[test]
    fn a_value_in_quotes_may_hold_a_line_break() {
        let list = Recipients::parse(b"Name,Address\nAnn,\"12 High Street\nLeeds\"\n");
        assert_eq!(list.len(), 1, "the break inside the quotes is not a new row");
        assert_eq!(list.value(0, "Address").as_deref(), Some("12 High Street\nLeeds"));
    }

    #[test]
    fn a_file_written_on_windows_reads_the_same_as_one_written_anywhere_else() {
        let windows = Recipients::parse(b"Name,Town\r\nAnn,Leeds\r\n");
        let other = Recipients::parse(b"Name,Town\nAnn,Leeds\n");
        assert_eq!(windows, other);
    }

    #[test]
    fn a_list_separated_by_semicolons_reads_too() {
        // What a spreadsheet writes where the comma is the decimal point.
        let list = Recipients::parse("Name;Town\nAnn;Leeds\n".as_bytes());
        assert_eq!(list.headers.len(), 2);
        assert_eq!(list.value(0, "Town").as_deref(), Some("Leeds"));
    }

    #[test]
    fn a_row_with_a_gap_reads_as_empty_rather_than_as_missing() {
        let list = Recipients::parse(b"Name,Town\nAnn\n");
        let record = list.record(0);
        assert_eq!(record.len(), 2);
        assert_eq!(record[1], ("Town".to_owned(), String::new()));
    }

    #[test]
    fn an_empty_file_is_a_list_of_nobody() {
        assert!(Recipients::parse(b"").is_empty());
        assert!(Recipients::parse(b"Name,Town\n").is_empty());
    }

    #[test]
    fn a_list_written_in_utf_eight_with_a_mark_reads_correctly() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("Имя\nАнна\n".as_bytes());
        let list = Recipients::parse(&bytes);
        assert_eq!(list.headers, vec!["Имя".to_owned()]);
        assert_eq!(list.value(0, "Имя").as_deref(), Some("Анна"));
    }

    #[test]
    fn an_instruction_names_its_column() {
        assert_eq!(merge_column("MERGEFIELD Name").as_deref(), Some("Name"));
        assert_eq!(merge_column(" MERGEFIELD Town \\* MERGEFORMAT ").as_deref(), Some("Town"));
        assert_eq!(merge_column("PAGE"), None);
        assert_eq!(merge_column("MERGEFIELD"), None);
    }

    #[test]
    fn every_kind_survives_being_written_and_read_back() {
        for kind in Kind::ALL {
            assert_eq!(Kind::from_word(kind.word()), *kind);
        }
        assert_eq!(Kind::from_word("something else"), Kind::Letters);
    }
}
