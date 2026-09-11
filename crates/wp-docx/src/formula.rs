//! Word's `=` field: arithmetic over the cells of a table.
//!
//! # What a formula is
//!
//! A field whose instruction begins with an equals sign. `=SUM(ABOVE)` adds up
//! the column it sits at the foot of; `=B2*1.2` multiplies one cell;
//! `=IF(A1>10,1,0)` chooses between two answers. It is written, read and updated
//! as every other field is — see [`crate::fields`] — and what is here is only
//! the arithmetic.
//!
//! Only the arithmetic: every answer is a number. Word lets `IF` choose between
//! two pieces of text as well, and a formula that asks for that is answered with
//! the number nothing rather than with the words.
//!
//! # Where the numbers come from
//!
//! From the cells around it, as text. A cell holds words, and what a formula
//! wants is a number, so the number is read out of whatever the cell says: "£1
//! 234.50" is 1234.5, and a cell with nothing countable in it counts as
//! nothing. That is [`crate::sorting::number_in`], the same reading a column
//! sorted as numbers gets, because a cell should not mean one thing to sorting
//! and another to adding up.
//!
//! # What ABOVE means
//!
//! The cells above this one in its own column, up to the first blank one. A
//! blank cell is where a column of figures starts, and Word stops there too —
//! otherwise a total at the foot of the second table in a cell would add up the
//! first one as well. `BELOW`, `LEFT` and `RIGHT` are the same rule in the
//! other three directions.
//!
//! # Why the answer is worked out at every layout and not kept
//!
//! Because a formula that answered with what it said last time would be wrong
//! the moment a figure above it changed, and a person who has just corrected a
//! number should not have to know that a field needs updating. The answer *is*
//! written into the file as the field's result, because a program that cannot
//! do arithmetic has to have something to show — but nothing here reads it
//! back.

use crate::sorting::number_in;

/// The cells a formula can see, and which of them it is in.
#[derive(Clone, Copy, Debug)]
pub struct Sheet<'a> {
    /// Every cell of the table, by row and then by column.
    pub rows: &'a [Vec<String>],
    /// Which cell the formula is in.
    pub row: usize,
    pub column: usize,
}

impl Sheet<'_> {
    /// What one cell holds, as a number.
    fn at(&self, row: usize, column: usize) -> Option<f64> {
        let text = self.rows.get(row)?.get(column)?;
        number_in(text)
    }

    /// Whether a cell is empty, which is where a run of figures stops.
    fn blank(&self, row: usize, column: usize) -> bool {
        self.rows.get(row).and_then(|row| row.get(column)).is_none_or(|text| text.trim().is_empty())
    }

    /// The numbers in one direction, up to the first blank cell.
    fn towards(&self, which: Direction) -> Vec<f64> {
        let mut out = Vec::new();
        let (mut row, mut column) = (self.row, self.column);
        loop {
            let stepped = match which {
                Direction::Above => row.checked_sub(1).map(|row| (row, column)),
                Direction::Below => Some((row + 1, column)),
                Direction::Left => column.checked_sub(1).map(|column| (row, column)),
                Direction::Right => Some((row, column + 1)),
            };
            let Some((next_row, next_column)) = stepped else { break };
            if next_row >= self.rows.len() || self.blank(next_row, next_column) {
                break;
            }
            if let Some(number) = self.at(next_row, next_column) {
                out.push(number);
            }
            row = next_row;
            column = next_column;
        }
        out
    }

    /// Every number in a rectangle of cells.
    fn rectangle(&self, from: (usize, usize), to: (usize, usize)) -> Vec<f64> {
        let rows = from.0.min(to.0)..=from.0.max(to.0);
        let columns = from.1.min(to.1)..=from.1.max(to.1);
        let mut out = Vec::new();
        for row in rows {
            for column in columns.clone() {
                if let Some(number) = self.at(row, column) {
                    out.push(number);
                }
            }
        }
        out
    }
}

/// One of the four words a formula can use instead of naming cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Direction {
    Above,
    Below,
    Left,
    Right,
}

impl Direction {
    fn from_word(word: &str) -> Option<Self> {
        match word.to_ascii_uppercase().as_str() {
            "ABOVE" => Some(Self::Above),
            "BELOW" => Some(Self::Below),
            "LEFT" => Some(Self::Left),
            "RIGHT" => Some(Self::Right),
            _ => None,
        }
    }
}

/// What an expression comes to: one number, or a list of them where it named a
/// range of cells.
///
/// A list is only useful to a function that takes one — `SUM`, `AVERAGE` — and
/// anywhere else it is the first number in it, which is what Word does with a
/// range used where a number was wanted.
#[derive(Clone, Debug, PartialEq)]
enum Value {
    One(f64),
    Many(Vec<f64>),
}

impl Value {
    fn number(&self) -> f64 {
        match self {
            Value::One(number) => *number,
            Value::Many(numbers) => numbers.first().copied().unwrap_or(0.0),
        }
    }

    fn list(&self) -> Vec<f64> {
        match self {
            Value::One(number) => vec![*number],
            Value::Many(numbers) => numbers.clone(),
        }
    }
}

/// Works a formula out.
///
/// The instruction as the field holds it, with or without its leading equals
/// sign and with or without the switches after it: `=SUM(ABOVE) \# "#,##0.00"`
/// is answered the same as `SUM(ABOVE)`.
///
/// `None` where the formula cannot be read at all, which is what Word shows as
/// "!Syntax Error".
#[must_use]
pub fn evaluate(instruction: &str, sheet: &Sheet<'_>) -> Option<f64> {
    let mut parser = Parser { text: body_of(instruction).chars().collect(), at: 0, sheet };
    let value = parser.expression()?;
    parser.skip_space();
    // Anything left over means the formula was not understood, which is worth
    // saying rather than answering half of it.
    if parser.at < parser.text.len() {
        return None;
    }
    Some(value.number())
}

/// The arithmetic of an instruction, without the equals sign or the switches.
#[must_use]
pub fn body_of(instruction: &str) -> &str {
    let text = instruction.trim();
    let text = text.strip_prefix('=').unwrap_or(text);
    // A switch begins with a backslash: `\# "0.00"` says how to show the answer
    // and `\* MERGEFORMAT` says to keep the formatting.
    match text.find('\\') {
        Some(at) => text[..at].trim_end(),
        None => text.trim_end(),
    }
}

/// The number picture a formula asks for, if it asks for one.
///
/// `\# "#,##0.00"` — the switch Word's Formula dialog writes when a format is
/// chosen from its list.
#[must_use]
pub fn picture_of(instruction: &str) -> Option<String> {
    let at = instruction.find("\\#")?;
    let rest = instruction[at + 2..].trim_start();
    if let Some(quoted) = rest.strip_prefix('"') {
        let end = quoted.find('"')?;
        return Some(quoted[..end].to_owned());
    }
    // Unquoted, which Word allows for a picture with no spaces in it.
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    Some(rest[..end].to_owned()).filter(|picture| !picture.is_empty())
}

/// Whether an instruction is a formula at all.
#[must_use]
pub fn is_formula(instruction: &str) -> bool {
    instruction.trim_start().starts_with('=')
}

impl crate::Document {
    /// What a formula in one paragraph comes to, with the number picture it
    /// asks for applied.
    ///
    /// The paragraph says which cell the formula is in, and the table round it
    /// says what the numbers are. A formula outside a table has no cells to read
    /// and answers with whatever arithmetic it can do on its own — `=2*3` is six
    /// wherever it stands.
    ///
    /// `!Syntax Error` is what Word shows for a formula it cannot read, and
    /// showing the same thing is better than showing nothing: a field that
    /// silently disappeared would be a field nobody could find to correct.
    #[must_use]
    pub fn formula_answer(&self, instruction: &str, paragraph: usize) -> String {
        let rows = self.table_rows_text_at(paragraph);
        let (row, column) = match self.table_at(paragraph) {
            Some(place) => (place.row, place.column),
            None => (0, 0),
        };
        let sheet = Sheet { rows: &rows, row, column };

        match evaluate(instruction, &sheet) {
            Some(answer) => format(answer, picture_of(instruction).as_deref().unwrap_or_default()),
            None => "!Syntax Error".to_owned(),
        }
    }
}

/// Reads an expression out of the text of a formula.
struct Parser<'a> {
    text: Vec<char>,
    at: usize,
    sheet: &'a Sheet<'a>,
}

impl Parser<'_> {
    fn skip_space(&mut self) {
        while self.text.get(self.at).is_some_and(|character| character.is_whitespace()) {
            self.at += 1;
        }
    }

    fn peek(&mut self) -> Option<char> {
        self.skip_space();
        self.text.get(self.at).copied()
    }

    /// Takes a run of characters if it is next, whatever case it is in.
    fn take(&mut self, word: &str) -> bool {
        self.skip_space();
        let wanted: Vec<char> = word.chars().collect();
        if self.text.len() < self.at + wanted.len() {
            return false;
        }
        let found = self.text[self.at..self.at + wanted.len()]
            .iter()
            .zip(&wanted)
            .all(|(one, other)| one.eq_ignore_ascii_case(other));
        if found {
            self.at += wanted.len();
        }
        found
    }

    /// The whole of an expression, comparisons and all.
    fn expression(&mut self) -> Option<Value> {
        let left = self.sum()?;
        // Word's comparisons answer one or nought, which is what its `IF` reads.
        for (operator, ordering) in [
            ("<>", [true, false, true]),
            ("<=", [true, true, false]),
            (">=", [false, true, true]),
            ("=", [false, true, false]),
            ("<", [true, false, false]),
            (">", [false, false, true]),
        ] {
            if self.take(operator) {
                let right = self.sum()?;
                let (one, other) = (left.number(), right.number());
                let how = one.partial_cmp(&other)?;
                let answer = match how {
                    core::cmp::Ordering::Less => ordering[0],
                    core::cmp::Ordering::Equal => ordering[1],
                    core::cmp::Ordering::Greater => ordering[2],
                };
                return Some(Value::One(if answer { 1.0 } else { 0.0 }));
            }
        }
        Some(left)
    }

    fn sum(&mut self) -> Option<Value> {
        // The value is kept whole until something is done to it: a range of
        // cells with no arithmetic round it is still a range, which is what
        // SUM and AVERAGE are given.
        let mut total = self.product()?;
        loop {
            if self.take("+") {
                total = Value::One(total.number() + self.product()?.number());
            } else if self.take("-") {
                total = Value::One(total.number() - self.product()?.number());
            } else {
                return Some(total);
            }
        }
    }

    fn product(&mut self) -> Option<Value> {
        let mut total = self.unary()?;
        loop {
            if self.take("*") {
                total = Value::One(total.number() * self.unary()?.number());
            } else if self.take("/") {
                let by = self.unary()?.number();
                // Word answers a division by nothing with an error, and an
                // error here is nothing rather than an infinity in the page.
                if by == 0.0 {
                    return None;
                }
                total = Value::One(total.number() / by);
            } else {
                return Some(total);
            }
        }
    }

    fn unary(&mut self) -> Option<Value> {
        if self.take("-") {
            return Some(Value::One(-self.unary()?.number()));
        }
        if self.take("+") {
            return self.unary();
        }
        let value = self.atom()?;
        // A percentage sign after a number divides it by a hundred, which is
        // what Word does with `50%`.
        if self.take("%") {
            return Some(Value::One(value.number() / 100.0));
        }
        Some(value)
    }

    fn atom(&mut self) -> Option<Value> {
        self.skip_space();
        if self.take("(") {
            let inside = self.expression()?;
            if !self.take(")") {
                return None;
            }
            return Some(inside);
        }

        let next = self.peek()?;
        if next.is_ascii_digit() || next == '.' {
            return Some(Value::One(self.number()?));
        }
        if next.is_ascii_alphabetic() {
            return self.word();
        }
        None
    }

    fn number(&mut self) -> Option<f64> {
        let start = self.at;
        while self
            .text
            .get(self.at)
            .is_some_and(|character| character.is_ascii_digit() || *character == '.')
        {
            self.at += 1;
        }
        let text: String = self.text[start..self.at].iter().collect();
        text.parse().ok()
    }

    /// A name: a function, a direction, or a cell.
    fn word(&mut self) -> Option<Value> {
        let start = self.at;
        while self.text.get(self.at).is_some_and(|character| character.is_ascii_alphanumeric()) {
            self.at += 1;
        }
        let word: String = self.text[start..self.at].iter().collect();

        // A function is a name with a bracket after it.
        if self.peek() == Some('(') {
            self.take("(");
            let mut arguments = Vec::new();
            if !self.take(")") {
                loop {
                    arguments.push(self.expression()?);
                    if self.take(",") {
                        continue;
                    }
                    if self.take(")") {
                        break;
                    }
                    return None;
                }
            }
            return apply(&word, &arguments);
        }

        if let Some(direction) = Direction::from_word(&word) {
            return Some(Value::Many(self.sheet.towards(direction)));
        }

        // A cell, or a rectangle of them: `B2` or `A1:C3`.
        let from = cell_named(&word)?;
        if self.peek() == Some(':') {
            self.take(":");
            let start = self.at;
            while self.text.get(self.at).is_some_and(|character| character.is_ascii_alphanumeric())
            {
                self.at += 1;
            }
            let second: String = self.text[start..self.at].iter().collect();
            let to = cell_named(&second)?;
            return Some(Value::Many(self.sheet.rectangle(from, to)));
        }
        Some(Value::One(self.sheet.at(from.0, from.1).unwrap_or(0.0)))
    }
}

/// Which cell a name like `B2` means, as a row and a column counted from zero.
fn cell_named(word: &str) -> Option<(usize, usize)> {
    let letters: String =
        word.chars().take_while(char::is_ascii_alphabetic).collect::<String>().to_uppercase();
    let digits: String = word.chars().skip(letters.len()).collect();
    if letters.is_empty() || digits.is_empty() {
        return None;
    }

    // The columns are lettered the way a spreadsheet letters them: A to Z, then
    // AA to AZ, and so on.
    let mut column = 0usize;
    for letter in letters.chars() {
        if !letter.is_ascii_uppercase() {
            return None;
        }
        column = column * 26 + (letter as usize - 'A' as usize + 1);
    }
    let row: usize = digits.parse().ok()?;
    Some((row.checked_sub(1)?, column.checked_sub(1)?))
}

/// Works one of Word's functions out.
fn apply(name: &str, arguments: &[Value]) -> Option<Value> {
    let all: Vec<f64> = arguments.iter().flat_map(Value::list).collect();
    let first = || arguments.first().map(Value::number).unwrap_or_default();
    let truth = |value: f64| value != 0.0;

    let answer = match name.to_ascii_uppercase().as_str() {
        "SUM" => all.iter().sum(),
        "PRODUCT" => all.iter().product(),
        "COUNT" => all.len() as f64,
        "AVERAGE" => {
            if all.is_empty() {
                return None;
            }
            all.iter().sum::<f64>() / all.len() as f64
        }
        "MIN" => all.iter().copied().fold(f64::INFINITY, f64::min),
        "MAX" => all.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        "ABS" => first().abs(),
        "INT" => first().trunc(),
        "SIGN" => {
            let value = first();
            if value > 0.0 {
                1.0
            } else if value < 0.0 {
                -1.0
            } else {
                0.0
            }
        }
        "ROUND" => {
            let places = arguments.get(1).map(Value::number).unwrap_or_default();
            let factor = 10f64.powi(places as i32);
            (first() * factor).round() / factor
        }
        "MOD" => {
            let by = arguments.get(1).map(Value::number).unwrap_or_default();
            if by == 0.0 {
                return None;
            }
            first() % by
        }
        "IF" => {
            let chosen = if truth(first()) { arguments.get(1) } else { arguments.get(2) };
            chosen.map(Value::number).unwrap_or_default()
        }
        "AND" => f64::from(all.iter().copied().all(truth)),
        "OR" => f64::from(all.iter().copied().any(truth)),
        "NOT" => f64::from(!truth(first())),
        "TRUE" => 1.0,
        "FALSE" => 0.0,
        _ => return None,
    };

    // A MIN or a MAX of nothing at all is nothing rather than an infinity.
    if !answer.is_finite() {
        return None;
    }
    Some(Value::One(answer))
}

/// Shows a number the way a picture asks for.
///
/// Word's numeric picture switch, of which its Formula dialog offers seven.
/// `0` is a figure always shown, `#` one shown only where it counts, a comma
/// groups the thousands, a full stop is the decimal point, a per cent sign
/// multiplies by a hundred, and everything else is written out as it stands. A
/// picture in two halves — `positive;negative` — says how to show a number
/// below nothing, which is how the accountant's form with brackets is written.
#[must_use]
pub fn format(value: f64, picture: &str) -> String {
    if picture.trim().is_empty() {
        return trimmed(value);
    }

    let halves: Vec<&str> = picture.split(';').collect();
    let below_nothing = value < 0.0;
    let half = if below_nothing && halves.len() > 1 { halves[1] } else { halves[0] };

    let mut value = value.abs();
    if half.contains('%') {
        value *= 100.0;
    }

    // What the picture asks of the number itself.
    let numbers: String = half.chars().filter(|character| "#0,.".contains(*character)).collect();
    let (whole, fraction) = match numbers.split_once('.') {
        Some((whole, fraction)) => (whole, fraction),
        None => (numbers.as_str(), ""),
    };
    let places = fraction.chars().filter(|character| "#0".contains(*character)).count();
    let grouped = whole.contains(',');
    let least = whole.chars().filter(|character| *character == '0').count().max(1);

    // Half is rounded away from nothing, as Word and as anybody totting up a
    // column does — the other way is what a formatter does by itself, and it
    // shows 1234.5 as 1,234.
    let factor = 10f64.powi(places as i32);
    let value = (value * factor).round() / factor;
    let rounded = format!("{value:.places$}");
    let (digits, decimals) = match rounded.split_once('.') {
        Some((digits, decimals)) => (digits.to_owned(), decimals.to_owned()),
        None => (rounded.clone(), String::new()),
    };
    let mut digits = digits.trim_start_matches('0').to_owned();
    while digits.len() < least {
        digits.insert(0, '0');
    }
    if grouped {
        digits = in_thousands(&digits);
    }

    let mut shown = digits;
    if !decimals.is_empty() {
        shown.push('.');
        shown.push_str(&decimals);
    }

    // And the rest of the picture: the currency in front, the per cent after,
    // the brackets round the whole of it.
    let mut out = String::new();
    let mut written = false;
    for character in half.chars() {
        match character {
            '#' | '0' | ',' | '.' => {
                if !written {
                    out.push_str(&shown);
                    written = true;
                }
            }
            other => out.push(other),
        }
    }
    if !written {
        out.push_str(&shown);
    }
    // A number below nothing keeps its minus sign unless the picture said what
    // to do with one, which is what its second half is for.
    if below_nothing && halves.len() == 1 {
        out.insert(0, '-');
    }
    out
}

/// A number with no picture at all: as many places as it needs and no more.
fn trimmed(value: f64) -> String {
    if value == value.trunc() && value.abs() < 1e15 {
        return format!("{value:.0}");
    }
    let mut text = format!("{value:.6}");
    while text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

/// Commas every three figures, counting from the right.
fn in_thousands(digits: &str) -> String {
    let mut out = String::new();
    let figures: Vec<char> = digits.chars().collect();
    for (at, figure) in figures.iter().enumerate() {
        if at > 0 && (figures.len() - at) % 3 == 0 {
            out.push(',');
        }
        out.push(*figure);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(cells: &[&[&str]]) -> Vec<Vec<String>> {
        cells.iter().map(|row| row.iter().map(|text| (*text).to_owned()).collect()).collect()
    }

    /// A table of figures, with the formula in the cell asked for.
    fn sheet<'a>(rows: &'a [Vec<String>], row: usize, column: usize) -> Sheet<'a> {
        Sheet { rows, row, column }
    }

    #[test]
    fn a_column_is_added_up_from_the_foot_of_it() {
        let rows = rows(&[&["Item", "Cost"], &["Pens", "3"], &["Paper", "4.5"], &["Total", ""]]);
        let answer = evaluate("=SUM(ABOVE)", &sheet(&rows, 3, 1));
        assert_eq!(answer, Some(7.5));
    }

    #[test]
    fn a_word_in_the_way_counts_as_nothing_and_a_blank_stops_the_run() {
        // The heading is not a number, so it adds nothing; a blank cell is
        // where the column of figures starts.
        let rows = rows(&[&["Cost"], &["3"], &[""], &["99"], &["4"], &[""]]);
        // From the bottom: 4, then 99, then a blank — so the three above it are
        // not counted.
        assert_eq!(evaluate("=SUM(ABOVE)", &sheet(&rows, 5, 0)), Some(103.0));
    }

    #[test]
    fn the_other_three_directions_work_the_same_way() {
        let rows = rows(&[&["1", "2", "3", ""], &["4", "5", "6", ""]]);
        assert_eq!(evaluate("=SUM(LEFT)", &sheet(&rows, 0, 3)), Some(6.0));
        assert_eq!(evaluate("=SUM(RIGHT)", &sheet(&rows, 1, 0)), Some(11.0));
        assert_eq!(evaluate("=SUM(BELOW)", &sheet(&rows, 0, 1)), Some(5.0));
    }

    #[test]
    fn a_cell_can_be_named_the_way_a_spreadsheet_names_one() {
        let rows = rows(&[&["1", "2"], &["3", "4"]]);
        assert_eq!(evaluate("=A1", &sheet(&rows, 1, 1)), Some(1.0));
        assert_eq!(evaluate("=B2", &sheet(&rows, 0, 0)), Some(4.0));
        assert_eq!(evaluate("=SUM(A1:B2)", &sheet(&rows, 0, 0)), Some(10.0));
    }

    #[test]
    fn the_arithmetic_is_arithmetic() {
        let rows = rows(&[&["2"]]);
        let sheet = sheet(&rows, 0, 0);
        assert_eq!(evaluate("=1+2*3", &sheet), Some(7.0));
        assert_eq!(evaluate("=(1+2)*3", &sheet), Some(9.0));
        assert_eq!(evaluate("=10/4", &sheet), Some(2.5));
        assert_eq!(evaluate("=-3+1", &sheet), Some(-2.0));
        assert_eq!(evaluate("=50%", &sheet), Some(0.5));
        assert_eq!(evaluate("=1/0", &sheet), None, "a division by nothing is an error");
    }

    #[test]
    fn words_functions_answer_as_words_do() {
        let rows = rows(&[&["1", "2", "3", "4"]]);
        let sheet = sheet(&rows, 0, 0);
        assert_eq!(evaluate("=AVERAGE(1,2,3)", &sheet), Some(2.0));
        assert_eq!(evaluate("=COUNT(A1:D1)", &sheet), Some(4.0));
        assert_eq!(evaluate("=MIN(5,2,8)", &sheet), Some(2.0));
        assert_eq!(evaluate("=MAX(5,2,8)", &sheet), Some(8.0));
        assert_eq!(evaluate("=PRODUCT(2,3,4)", &sheet), Some(24.0));
        assert_eq!(evaluate("=ABS(-4)", &sheet), Some(4.0));
        assert_eq!(evaluate("=INT(4.9)", &sheet), Some(4.0));
        assert_eq!(evaluate("=ROUND(4.567,2)", &sheet), Some(4.57));
        assert_eq!(evaluate("=MOD(7,3)", &sheet), Some(1.0));
        assert_eq!(evaluate("=SIGN(-9)", &sheet), Some(-1.0));
    }

    #[test]
    fn a_comparison_answers_one_or_nothing() {
        let rows = rows(&[&["5"]]);
        let sheet = sheet(&rows, 0, 0);
        assert_eq!(evaluate("=1<2", &sheet), Some(1.0));
        assert_eq!(evaluate("=1>2", &sheet), Some(0.0));
        assert_eq!(evaluate("=2=2", &sheet), Some(1.0));
        assert_eq!(evaluate("=2<>2", &sheet), Some(0.0));
        assert_eq!(evaluate("=IF(1>0,10,20)", &sheet), Some(10.0));
        assert_eq!(evaluate("=IF(1<0,10,20)", &sheet), Some(20.0));
        assert_eq!(evaluate("=AND(1,1)", &sheet), Some(1.0));
        assert_eq!(evaluate("=OR(0,1)", &sheet), Some(1.0));
        assert_eq!(evaluate("=NOT(0)", &sheet), Some(1.0));
    }

    #[test]
    fn the_switches_after_a_formula_are_not_part_of_it() {
        let rows = rows(&[&["2"], &["3"]]);
        let answer = evaluate("=SUM(ABOVE) \\# \"#,##0.00\"", &sheet(&rows, 1, 0));
        assert_eq!(answer, Some(2.0));
        assert_eq!(body_of("=SUM(ABOVE) \\# \"0.00\""), "SUM(ABOVE)");
        assert_eq!(picture_of("=SUM(ABOVE) \\# \"#,##0.00\"").as_deref(), Some("#,##0.00"));
        assert_eq!(picture_of("=SUM(ABOVE)"), None);
    }

    #[test]
    fn something_that_is_not_a_formula_is_answered_with_nothing() {
        let rows = rows(&[&["1"]]);
        let sheet = sheet(&rows, 0, 0);
        assert_eq!(evaluate("=SUM(", &sheet), None);
        assert_eq!(evaluate("=2 +", &sheet), None);
        assert_eq!(evaluate("=NONSENSE(1)", &sheet), None);
        assert_eq!(evaluate("=1 2", &sheet), None);
    }

    #[test]
    fn a_picture_shows_the_number_the_way_it_asks() {
        assert_eq!(format(1234.5, "#,##0.00"), "1,234.50");
        assert_eq!(format(1234.5, "#,##0"), "1,235");
        assert_eq!(format(0.25, "0%"), "25%");
        assert_eq!(format(7.0, "0.00"), "7.00");
        assert_eq!(format(7.0, "0"), "7");
        assert_eq!(format(1234.5, "$#,##0.00"), "$1,234.50");
    }

    #[test]
    fn a_picture_in_two_halves_says_how_to_show_a_number_below_nothing() {
        assert_eq!(format(-1234.5, "$#,##0.00;($#,##0.00)"), "($1,234.50)");
        assert_eq!(format(1234.5, "$#,##0.00;($#,##0.00)"), "$1,234.50");
    }

    #[test]
    fn no_picture_at_all_shows_as_many_figures_as_it_needs() {
        assert_eq!(format(7.0, ""), "7");
        assert_eq!(format(7.5, ""), "7.5");
        assert_eq!(format(1.0 / 3.0, ""), "0.333333");
    }

    #[test]
    fn which_cell_a_name_means() {
        assert_eq!(cell_named("A1"), Some((0, 0)));
        assert_eq!(cell_named("B2"), Some((1, 1)));
        assert_eq!(cell_named("AA10"), Some((9, 26)));
        assert_eq!(cell_named("A"), None);
        assert_eq!(cell_named("1"), None);
    }
}
