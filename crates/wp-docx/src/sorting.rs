//! Putting the rows of a table, or a run of paragraphs, into order.
//!
//! # Why the elements are moved rather than the text
//!
//! Because a row is not its text. A row carries its cells, and a cell carries
//! its width, its shading, its borders, the formatting of every run in it and
//! whatever else the file said about it. Sorting by reading the text out,
//! ordering it and writing it back would order the words and throw all of that
//! away — which is what sorting a selection of paragraphs used to do here, and
//! it is the thing about it that was wrong.
//!
//! So nothing is rewritten. The `w:tr` elements are read to find out what they
//! say, and then the same elements are put back in another order.
//!
//! # The three keys
//!
//! Word sorts on up to three columns at once: by the first, and where two rows
//! agree by the second, and where they agree again by the third. Each key has
//! its own column, its own idea of what the text in it means — words, a number,
//! a date — and its own direction. That is one comparison made three times and
//! not three different sorts, which is why [`SortKey`] carries all three
//! decisions and [`order_of`] takes a list of them.
//!
//! # What a number is, and what a date is
//!
//! Word reads the number out of a cell that is mostly not a number: "£1,234.50
//! (est.)" sorts as 1234.5. So does this — the digits, the separators inside
//! them and one leading minus, with everything else stepped over.
//!
//! A date is the harder one, because "3/4/2024" is two different days depending
//! on where you are. This reads the day first, which is what the United Kingdom
//! and most of the world write and what this program's own language setting
//! says; `2024-03-04`, the one form nobody can misread, is read as itself.
//! Anything that is not a date at all sorts as text does, before everything
//! that is one.

use wp_xml::tree::{Element, Node};

use crate::history::EditKind;
use crate::{edit, position, read, Document};

/// What the text in a column is taken to mean.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SortKind {
    /// Words, compared as a person reading them would: case makes no
    /// difference, and where two differ only by case the order they came in
    /// stands.
    #[default]
    Text,
    /// A number, read out of whatever else is in the cell.
    Number,
    /// A day.
    Date,
}

impl SortKind {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Text => "Text",
            Self::Number => "Number",
            Self::Date => "Date",
        }
    }

    pub const ALL: &'static [Self] = &[Self::Text, Self::Number, Self::Date];
}

/// One of Word's three: which column, read how, which way round.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SortKey {
    /// Counted from zero. A key naming a column that is not there sorts
    /// everything equal, which leaves the rows as they were.
    pub column: usize,
    pub kind: SortKind,
    pub descending: bool,
}

impl SortKey {
    #[must_use]
    pub fn new(column: usize, kind: SortKind, descending: bool) -> Self {
        Self { column, kind, descending }
    }
}

/// What one cell says, in the form its key reads it.
///
/// Worked out once per cell rather than at every comparison: sorting a hundred
/// rows compares each of them several times, and parsing a date each time would
/// be parsing it a thousand times.
#[derive(Clone, Debug, PartialEq)]
enum Said {
    /// The text, lowercased, and the text as it came — the second only to break
    /// a tie between two that differ by case alone.
    Words(String, String),
    /// A number, or nothing where the cell holds none.
    Number(Option<f64>),
    /// A day, as a number that puts days in order, or nothing.
    Day(Option<i64>),
}

impl Said {
    /// Reads a cell the way a key asks for it.
    fn of(text: &str, kind: SortKind) -> Self {
        match kind {
            SortKind::Text => Said::Words(text.to_lowercase(), text.to_owned()),
            SortKind::Number => Said::Number(number_in(text)),
            SortKind::Date => Said::Day(day_in(text)),
        }
    }

    /// How this one stands against another, before the direction is applied.
    fn against(&self, other: &Self) -> core::cmp::Ordering {
        use core::cmp::Ordering;
        match (self, other) {
            (Said::Words(one, first), Said::Words(other, second)) => {
                one.cmp(other).then_with(|| first.cmp(second))
            }
            // A cell with no number in it sorts before every cell that has one,
            // which is where Word puts it.
            (Said::Number(one), Said::Number(other)) => match (one, other) {
                (Some(one), Some(other)) => one.partial_cmp(other).unwrap_or(Ordering::Equal),
                (None, Some(_)) => Ordering::Less,
                (Some(_), None) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            },
            (Said::Day(one), Said::Day(other)) => one.cmp(other),
            // Two keys of different kinds never meet: a column is read one way
            // for the whole sort.
            _ => Ordering::Equal,
        }
    }
}

/// The order the rows should be in: for each place, which row goes there.
///
/// The rows are named by what is in them rather than by anything else, so this
/// is the whole of the sorting and every caller of it — a table, a run of
/// paragraphs — only has to say what its rows say.
///
/// Equal rows keep the order they came in, which is what makes sorting on three
/// keys mean anything: the third key is only asked where the first two agree,
/// and where all three agree the document's own order is the answer.
#[must_use]
pub fn order_of(rows: &[Vec<String>], keys: &[SortKey]) -> Vec<usize> {
    // Every key read once per row, before any comparing is done.
    let read: Vec<Vec<Said>> = rows
        .iter()
        .map(|cells| {
            keys.iter()
                .map(|key| Said::of(cells.get(key.column).map_or("", String::as_str), key.kind))
                .collect()
        })
        .collect();

    let mut order: Vec<usize> = (0..rows.len()).collect();
    order.sort_by(|one, other| {
        for (at, key) in keys.iter().enumerate() {
            let Some(first) = read[*one].get(at) else { continue };
            let Some(second) = read[*other].get(at) else { continue };
            let mut how = first.against(second);
            if key.descending {
                how = how.reverse();
            }
            if how != core::cmp::Ordering::Equal {
                return how;
            }
        }
        core::cmp::Ordering::Equal
    });
    order
}

/// The number a cell holds, whatever else is in it.
///
/// The first run of digits, with the separators inside it and a minus in front
/// of it: "£1,234.50 (est.)" is 1234.5, and "no idea" is nothing at all.
#[must_use]
pub fn number_in(text: &str) -> Option<f64> {
    let mut digits = String::new();
    let mut started = false;
    let mut characters = text.chars().peekable();

    while let Some(character) = characters.next() {
        match character {
            '-' | '\u{2212}' if !started && digits.is_empty() => digits.push('-'),
            '0'..='9' => {
                started = true;
                digits.push(character);
            }
            // A separator counts only between digits: the stop at the end of a
            // sentence is not part of the number before it.
            ',' | '\u{a0}' | ' ' if started => {
                if !characters.peek().is_some_and(char::is_ascii_digit) {
                    break;
                }
            }
            '.' if started => {
                if !characters.peek().is_some_and(char::is_ascii_digit) {
                    break;
                }
                digits.push('.');
            }
            _ if started => break,
            // Anything before the number is stepped over, and a minus that
            // turned out not to be in front of one is forgotten.
            _ => digits.clear(),
        }
    }

    if !started {
        return None;
    }
    digits.parse().ok()
}

/// The day a cell holds, as a number that puts days in order.
///
/// Not a calendar: nothing here needs to know what day of the week it was, only
/// which of two days came first. The number is the year, the month and the day
/// packed one after another, which orders them exactly and orders nothing else.
#[must_use]
pub fn day_in(text: &str) -> Option<i64> {
    let numbers: Vec<i64> = text
        .split(|character: char| !character.is_ascii_digit())
        .filter(|piece| !piece.is_empty())
        .take(4)
        .filter_map(|piece| piece.parse().ok())
        .collect();
    let [first, second, third, ..] = numbers[..] else { return None };

    // A four-figure number in front is a year, which is the one form nobody can
    // misread. Otherwise the day comes first, as it is written here.
    let (year, month, day) =
        if first > 31 { (first, second, third) } else { (third, second, first) };
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    // Two figures mean this century or the last one, the way every program that
    // has to guess guesses.
    let year = match year {
        0..=29 => year + 2000,
        30..=99 => year + 1900,
        other => other,
    };
    Some(year * 10_000 + month * 100 + day)
}

impl Document {
    /// Puts the rows of the table at the caret in order.
    ///
    /// `header` keeps the first row where it is, which is what it is for: the
    /// names of the columns are not one of the things being ordered.
    ///
    /// Returns whether anything moved.
    pub fn sort_table_rows(&mut self, keys: &[SortKey], header: bool) -> bool {
        let Some(place) = self.table_here() else { return false };
        if keys.is_empty() {
            return false;
        }
        let caret = self.caret();

        let Some(table) = edit::element_at_path(&self.tree().root, &place.table) else {
            return false;
        };
        // What each row says, cell by cell, and where each row sits among the
        // table's children.
        let rows: Vec<(usize, Vec<String>)> = table
            .children
            .iter()
            .enumerate()
            .filter_map(|(at, node)| {
                let element = node.as_element()?;
                element.is(Some(read::W), "tr").then(|| (at, cells_of(element)))
            })
            .collect();

        let first = usize::from(header);
        if rows.len() <= first + 1 {
            return false;
        }

        let said: Vec<Vec<String>> = rows[first..].iter().map(|(_, cells)| cells.clone()).collect();
        let order = order_of(&said, keys);
        if order.iter().enumerate().all(|(place, from)| place == *from) {
            // Already in that order, so nothing to record and nothing to undo.
            return false;
        }

        self.record(EditKind::Structural, caret, false);
        let Some(table) = edit::element_at_path_mut(&mut self.tree_mut().root, &place.table) else {
            return false;
        };

        // The rows are taken out and put back in the order asked for. Their
        // places among the table's other children — the properties, the grid —
        // are the places the rows go back into, so nothing else moves.
        let places: Vec<usize> = rows[first..].iter().map(|(at, _)| *at).collect();
        let taken: Vec<Node> =
            places.iter().filter_map(|at| table.children.get(*at).cloned()).collect();
        if taken.len() != places.len() {
            return false;
        }
        for (place, from) in places.iter().zip(&order) {
            if let (Some(slot), Some(row)) = (table.children.get_mut(*place), taken.get(*from)) {
                *slot = row.clone();
            }
        }

        // The caret was in a row that has moved, so it goes to the top of the
        // table rather than to wherever that row landed.
        if let Some((start, _)) = self.table_paragraphs() {
            self.set_caret(crate::TextPosition::new(start, 0));
        }
        self.mark_modified();
        true
    }

    /// And the same for a run of paragraphs, which is what Word's Sort does
    /// outside a table.
    ///
    /// Each paragraph is one row, and its "columns" are the pieces it is split
    /// into by tabs — Word's "separate fields at", of which this offers the one
    /// Word offers first. The paragraphs themselves are moved, so everything
    /// they were formatted with moves with them.
    pub fn sort_paragraphs(&mut self, from: usize, to: usize, keys: &[SortKey]) -> bool {
        if keys.is_empty() || to <= from {
            return false;
        }
        let caret = self.caret();

        // Every paragraph of the run has to be a child of the same parent, or
        // "putting them in another order" means nothing: a selection running
        // from a cell into the text after it is not a list to be sorted.
        let mut places = Vec::new();
        let mut parent: Option<Vec<usize>> = None;
        for index in from..=to {
            let Some(path) = position::paragraph_path(&self.tree().root, index) else {
                return false;
            };
            let Some((at, above)) = path.split_last() else { return false };
            match &parent {
                None => parent = Some(above.to_vec()),
                Some(known) if known == above => {}
                Some(_) => return false,
            }
            places.push(*at);
        }
        let Some(parent) = parent else { return false };

        // The whole line first and its tab-separated pieces after it, so that
        // Word's "Paragraphs" and its "Field 1" are the first two columns and a
        // key can name either.
        let said: Vec<Vec<String>> = (from..=to)
            .map(|index| fields_of(&self.paragraph_text(index).unwrap_or_default()))
            .collect();
        let order = order_of(&said, keys);
        if order.iter().enumerate().all(|(place, at)| place == *at) {
            return false;
        }

        self.record(EditKind::Structural, caret, false);
        let Some(holder) = edit::element_at_path_mut(&mut self.tree_mut().root, &parent) else {
            return false;
        };
        let taken: Vec<Node> =
            places.iter().filter_map(|at| holder.children.get(*at).cloned()).collect();
        if taken.len() != places.len() {
            return false;
        }
        for (place, at) in places.iter().zip(&order) {
            if let (Some(slot), Some(paragraph)) = (holder.children.get_mut(*place), taken.get(*at))
            {
                *slot = paragraph.clone();
            }
        }

        self.set_caret(crate::TextPosition::new(from, 0));
        self.clear_selection();
        self.mark_modified();
        true
    }

    /// What each cell of the table at the caret says, row by row.
    ///
    /// What a dialog needs to know how many columns there are, and what a test
    /// needs to see that they moved.
    #[must_use]
    pub fn table_rows_text(&self) -> Vec<Vec<String>> {
        self.table_rows_text_at(self.caret().paragraph)
    }

    /// And the same for the table round any paragraph, which is what a formula
    /// being worked out asks. See [`crate::formula`].
    #[must_use]
    pub fn table_rows_text_at(&self, paragraph: usize) -> Vec<Vec<String>> {
        let Some(place) = self.table_at(paragraph) else { return Vec::new() };
        let Some(table) = edit::element_at_path(&self.tree().root, &place.table) else {
            return Vec::new();
        };
        table.child_elements().filter(|child| child.is(Some(read::W), "tr")).map(cells_of).collect()
    }
}

/// What a paragraph offers a key: the whole line, then its pieces.
///
/// Word's Sort outside a table offers "Paragraphs" and then "Field 1", "Field
/// 2" and so on, where a field is what the tabs separate. They are the same
/// list of columns to everything else here, which is why the whole line is one
/// of them rather than a case of its own.
#[must_use]
pub fn fields_of(text: &str) -> Vec<String> {
    let mut out = vec![text.trim().to_owned()];
    out.extend(text.split('\t').map(|piece| piece.trim().to_owned()));
    out
}

/// How many things a run of paragraphs offers to sort on.
#[must_use]
pub fn field_count(lines: &[String]) -> usize {
    lines.iter().map(|line| fields_of(line).len()).max().unwrap_or(1)
}

/// What each cell of one `w:tr` says.
fn cells_of(row: &Element) -> Vec<String> {
    row.child_elements()
        .filter(|child| child.is(Some(read::W), "tc"))
        .map(|cell| {
            let mut text = String::new();
            gather_text(cell, &mut text);
            text.trim().to_owned()
        })
        .collect()
}

/// Every piece of text under an element, in the order it is written.
fn gather_text(element: &Element, out: &mut String) {
    for child in element.child_elements() {
        if child.namespace.as_deref() != Some(read::W) {
            continue;
        }
        match child.local_name() {
            "t" => out.push_str(&child.text_content()),
            "tab" => out.push('\t'),
            // Text somebody deleted is not what the row says.
            "delText" | "del" => {}
            "p" => {
                if !out.is_empty() && !out.ends_with(' ') {
                    out.push(' ');
                }
                gather_text(child, out);
            }
            _ => gather_text(child, out),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(cells: &[&[&str]]) -> Vec<Vec<String>> {
        cells.iter().map(|row| row.iter().map(|text| (*text).to_owned()).collect()).collect()
    }

    #[test]
    fn words_are_ordered_as_a_person_reads_them() {
        let rows = rows(&[&["pear"], &["Apple"], &["banana"]]);
        let order = order_of(&rows, &[SortKey::new(0, SortKind::Text, false)]);
        assert_eq!(order, vec![1, 2, 0], "capitals were not ignored");
    }

    #[test]
    fn the_other_way_round_is_the_other_way_round() {
        let rows = rows(&[&["pear"], &["Apple"], &["banana"]]);
        let order = order_of(&rows, &[SortKey::new(0, SortKind::Text, true)]);
        assert_eq!(order, vec![0, 2, 1]);
    }

    #[test]
    fn numbers_are_ordered_as_numbers_and_not_as_words() {
        let rows = rows(&[&["9"], &["10"], &["100"]]);
        let as_words = order_of(&rows, &[SortKey::new(0, SortKind::Text, false)]);
        assert_eq!(as_words, vec![1, 2, 0], "as words, ten comes before nine");

        let as_numbers = order_of(&rows, &[SortKey::new(0, SortKind::Number, false)]);
        assert_eq!(as_numbers, vec![0, 1, 2]);
    }

    #[test]
    fn a_number_is_read_out_of_whatever_else_is_in_the_cell() {
        assert_eq!(number_in("£1,234.50 (est.)"), Some(1234.5));
        assert_eq!(number_in("-7 degrees"), Some(-7.0));
        assert_eq!(number_in("about 3.5 metres"), Some(3.5));
        assert_eq!(number_in("Total: 1 000"), Some(1000.0));
        assert_eq!(number_in("nothing here"), None);
        assert_eq!(number_in(""), None);
        // A stop that ends a sentence is not part of the number before it.
        assert_eq!(number_in("It cost 12."), Some(12.0));
    }

    #[test]
    fn a_cell_with_no_number_sorts_before_the_ones_that_have_one() {
        let rows = rows(&[&["7"], &["nothing"], &["3"]]);
        let order = order_of(&rows, &[SortKey::new(0, SortKind::Number, false)]);
        assert_eq!(order, vec![1, 2, 0]);
    }

    #[test]
    fn days_are_ordered_by_when_they_were() {
        let rows = rows(&[&["3/4/2024"], &["2024-01-05"], &["31.12.2023"]]);
        let order = order_of(&rows, &[SortKey::new(0, SortKind::Date, false)]);
        assert_eq!(order, vec![2, 1, 0]);
    }

    #[test]
    fn a_day_is_read_with_the_day_first_unless_the_year_is() {
        // The third of April, which is what this is everywhere but America.
        assert_eq!(day_in("3/4/2024"), Some(20_240_403));
        // And the one form nobody can misread.
        assert_eq!(day_in("2024-04-03"), Some(20_240_403));
        assert_eq!(day_in("not a date"), None);
        assert_eq!(day_in("32/1/2024"), None, "there is no thirty-second");
        assert_eq!(day_in("1/13/2024"), None, "there is no thirteenth month");
    }

    #[test]
    fn the_second_key_is_asked_where_the_first_agrees() {
        let rows = rows(&[&["b", "2"], &["a", "9"], &["a", "1"]]);
        let order = order_of(
            &rows,
            &[SortKey::new(0, SortKind::Text, false), SortKey::new(1, SortKind::Number, false)],
        );
        assert_eq!(order, vec![2, 1, 0]);
    }

    #[test]
    fn and_the_third_where_the_first_two_do() {
        let rows = rows(&[&["a", "1", "z"], &["a", "1", "m"], &["a", "0", "q"]]);
        let order = order_of(
            &rows,
            &[
                SortKey::new(0, SortKind::Text, false),
                SortKey::new(1, SortKind::Number, false),
                SortKey::new(2, SortKind::Text, false),
            ],
        );
        assert_eq!(order, vec![2, 1, 0]);
    }

    #[test]
    fn rows_that_are_equal_stay_where_they_were() {
        let rows = rows(&[&["same"], &["same"], &["same"]]);
        let order = order_of(&rows, &[SortKey::new(0, SortKind::Text, false)]);
        assert_eq!(order, vec![0, 1, 2]);
    }

    #[test]
    fn a_key_naming_a_column_that_is_not_there_orders_nothing() {
        let rows = rows(&[&["b"], &["a"]]);
        let order = order_of(&rows, &[SortKey::new(4, SortKind::Text, false)]);
        assert_eq!(order, vec![0, 1]);
    }
}
