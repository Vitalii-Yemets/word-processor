//! What a Visual Basic value is, and what happens when two of them meet.
//!
//! # Most of the language is in here
//!
//! A macro rarely says what kind of thing a variable holds, so nearly every
//! value is a Variant: a thing that knows what it is at the moment and turns
//! into whatever the next operator needs. `"3" + 4` is seven; `"3" & 4` is
//! "34"; `#1/1/2000# + 1` is the second of January; `Empty` is nought to a
//! sum and "" to a join; and `Null` swallows whatever it touches. None of
//! that is obvious and all of it is what a macro depends on, so the rules are
//! written out here once, with the reason beside each, rather than scattered
//! through the interpreter.
//!
//! # Where the numbers differ from the obvious
//!
//! - `True` is minus one as a number, because the flags are all ones.
//! - `/` always gives a Double, even between two whole numbers; `\` throws
//!   the fraction away, and rounds its operands first.
//! - `Mod` rounds its operands too, which is why `7.6 Mod 3` is two and not
//!   one and a bit.
//! - Currency is a whole number of ten-thousandths, so money adds up exactly
//!   where a Double would drift.
//! - A date is a count of days from the thirtieth of December 1899, and the
//!   fraction is the time of day.

/// Something that went wrong while a macro ran.
///
/// The number is the one Visual Basic gives, because a macro can ask for it —
/// `Err.Number` — and act on it, so inventing numbers would break the macros
/// that do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fault {
    pub number: i32,
    pub description: String,
}

impl Fault {
    /// One of the errors the language itself raises.
    #[must_use]
    pub fn of(number: i32) -> Self {
        Self { number, description: described(number).to_owned() }
    }

    /// One a macro raised, or one with something to add.
    #[must_use]
    pub fn saying(number: i32, description: &str) -> Self {
        Self { number, description: description.to_owned() }
    }
}

impl core::fmt::Display for Fault {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "run-time error {}: {}", self.number, self.description)
    }
}

/// What Visual Basic says about each of the errors it raises.
#[must_use]
pub fn described(number: i32) -> &'static str {
    match number {
        5 => "Invalid procedure call or argument",
        6 => "Overflow",
        9 => "Subscript out of range",
        10 => "This array is fixed or temporarily locked",
        11 => "Division by zero",
        13 => "Type mismatch",
        52 => "Bad file name or number",
        53 => "File not found",
        62 => "Input past end of file",
        91 => "Object variable or With block variable not set",
        94 => "Invalid use of Null",
        424 => "Object required",
        450 => "Wrong number of arguments or invalid property assignment",
        _ => "Application-defined or object-defined error",
    }
}

/// A value, of whatever kind it is at the moment.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// A variable that has been declared and never given anything.
    Empty,
    /// A value that is known to be unknown, which swallows what it touches.
    Null,
    Boolean(bool),
    /// Every whole number: Integer, Long and LongLong are one thing here,
    /// and the range is checked where it matters rather than at every step.
    Long(i64),
    Double(f64),
    /// A whole number of ten-thousandths.
    Currency(i64),
    /// Days since the thirtieth of December 1899, the fraction being the
    /// time of day.
    Date(f64),
    Text(String),
    /// An object reference that points at nothing.
    Nothing,
    /// Something the program running the macro owns: a document, a range, a
    /// paragraph. What it can do is that program's business; all the language
    /// knows is that it is one and which one.
    Object(Handle),
    /// An array, with the bounds it was made with.
    Array(Box<Array>),
}

/// Which thing, of what kind.
///
/// The kind is what `TypeName` answers and what a message names when
/// something asks for a member nothing has; the number is whatever the
/// program that made it uses to find it again.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Handle {
    pub kind: String,
    pub id: u64,
}

impl Handle {
    /// One of a kind, by number.
    #[must_use]
    pub fn of(kind: &str, id: u64) -> Self {
        Self { kind: kind.to_owned(), id }
    }
}

/// An argument as the macro wrote it: `Replace:=True` carries its name.
///
/// Named arguments are how nearly every line of a real Word macro is
/// written, and a program that dropped the names would have to guess what
/// went where.
#[derive(Clone, Debug, PartialEq)]
pub struct Given {
    pub name: Option<String>,
    pub value: Value,
}

impl Given {
    /// One with no name on it.
    #[must_use]
    pub fn just(value: Value) -> Self {
        Self { name: None, value }
    }

    /// The value of the argument called this, or the one in this place.
    #[must_use]
    pub fn find<'a>(given: &'a [Self], name: &str, at: usize) -> Option<&'a Value> {
        given
            .iter()
            .find(|one| one.name.as_deref().is_some_and(|had| had.eq_ignore_ascii_case(name)))
            .map(|one| &one.value)
            .or_else(|| {
                // A place only counts where nothing before it was named,
                // which is the rule Visual Basic itself uses.
                given.get(at).filter(|one| one.name.is_none()).map(|one| &one.value)
            })
    }
}

/// An array and the bounds of each of its dimensions.
#[derive(Clone, Debug, PartialEq)]
pub struct Array {
    /// The lowest and highest subscript of each dimension.
    pub bounds: Vec<(i64, i64)>,
    pub values: Vec<Value>,
}

impl Array {
    /// An array of the bounds given, every cell empty.
    #[must_use]
    pub fn new(bounds: Vec<(i64, i64)>) -> Self {
        Self::filled(bounds, &Value::Empty)
    }

    /// And one whose cells start as whatever its kind starts as: a `Long`
    /// array holds noughts and not `Empty`, which is what `Erase` leaves
    /// behind and what a macro joining one into a sentence expects.
    #[must_use]
    pub fn filled(bounds: Vec<(i64, i64)>, value: &Value) -> Self {
        let count = bounds.iter().map(|(low, high)| (high - low + 1).max(0) as usize).product();
        Self { bounds, values: vec![value.clone(); count] }
    }

    /// Where a subscript lands, or why it cannot.
    pub fn at(&self, subscripts: &[i64]) -> Result<usize, Fault> {
        if subscripts.len() != self.bounds.len() {
            return Err(Fault::of(9));
        }
        let mut index = 0usize;
        for (subscript, (low, high)) in subscripts.iter().zip(&self.bounds) {
            if subscript < low || subscript > high {
                return Err(Fault::of(9));
            }
            let size = (high - low + 1).max(0) as usize;
            index = index * size + (subscript - low) as usize;
        }
        Ok(index)
    }
}

impl Value {
    /// A whole number, as a value.
    #[must_use]
    pub fn from_long(number: i64) -> Self {
        Self::Long(number)
    }

    /// Text, as a value.
    #[must_use]
    pub fn from_text(text: &str) -> Self {
        Self::Text(text.to_owned())
    }

    /// Whether this is `Null`, which most things have to ask before doing
    /// anything else.
    #[must_use]
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// What `TypeName` says about it.
    #[must_use]
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Empty => "Empty",
            Self::Null => "Null",
            Self::Boolean(_) => "Boolean",
            Self::Long(_) => "Long",
            Self::Double(_) => "Double",
            Self::Currency(_) => "Currency",
            Self::Date(_) => "Date",
            Self::Text(_) => "String",
            Self::Nothing => "Nothing",
            Self::Object(handle) => match handle.kind.as_str() {
                "" => "Object",
                _ => "Object",
            },
            Self::Array(_) => "Variant()",
        }
    }

    /// What `VarType` says about it, which is a number a macro may compare.
    #[must_use]
    pub fn var_type(&self) -> i64 {
        match self {
            Self::Empty => 0,
            Self::Null => 1,
            Self::Double(_) => 5,
            Self::Currency(_) => 6,
            Self::Date(_) => 7,
            Self::Text(_) => 8,
            Self::Nothing | Self::Object(_) => 9,
            Self::Boolean(_) => 11,
            Self::Long(_) => 3,
            Self::Array(_) => 8204,
        }
    }

    /// The value as a number, by the rules the language uses everywhere.
    ///
    /// `Empty` is nought, `True` is minus one, and text has to be a number
    /// all the way through — `"3 apples"` is a type mismatch and not three,
    /// which is where `Val` differs from arithmetic.
    pub fn number(&self) -> Result<f64, Fault> {
        match self {
            Self::Empty => Ok(0.0),
            Self::Null => Err(Fault::of(94)),
            Self::Boolean(on) => Ok(if *on { -1.0 } else { 0.0 }),
            #[allow(clippy::cast_precision_loss)]
            Self::Long(number) => Ok(*number as f64),
            Self::Double(number) | Self::Date(number) => Ok(*number),
            #[allow(clippy::cast_precision_loss)]
            Self::Currency(number) => Ok(*number as f64 / 10_000.0),
            Self::Text(text) => number_in(text).ok_or_else(|| Fault::of(13)),
            Self::Nothing | Self::Object(_) | Self::Array(_) => Err(Fault::of(13)),
        }
    }

    /// The value as a whole number, rounded the way Visual Basic rounds: to
    /// the nearest, and to the even one when it is exactly between.
    pub fn whole(&self) -> Result<i64, Fault> {
        if let Self::Long(number) = self {
            return Ok(*number);
        }
        let number = self.number()?;
        if !number.is_finite() {
            return Err(Fault::of(6));
        }
        let rounded = round_half_even(number);
        if rounded.abs() > 9.2e18 {
            return Err(Fault::of(6));
        }
        #[allow(clippy::cast_possible_truncation)]
        Ok(rounded as i64)
    }

    /// The value as text, the way `CStr` and `&` write one.
    pub fn text(&self) -> Result<String, Fault> {
        Ok(match self {
            Self::Empty => String::new(),
            Self::Null => return Err(Fault::of(94)),
            Self::Boolean(on) => (if *on { "True" } else { "False" }).to_owned(),
            Self::Long(number) => number.to_string(),
            Self::Double(number) => written_number(*number),
            #[allow(clippy::cast_precision_loss)]
            Self::Currency(number) => written_number(*number as f64 / 10_000.0),
            Self::Date(number) => crate::dates::written(*number),
            Self::Text(text) => text.clone(),
            Self::Nothing => "Nothing".to_owned(),
            // An object asked for its text is asked through the program that
            // owns it, which the interpreter does before it gets here.
            Self::Object(_) | Self::Array(_) => return Err(Fault::of(13)),
        })
    }

    /// Whether the value counts as true, which is "anything but nought".
    pub fn truth(&self) -> Result<bool, Fault> {
        match self {
            Self::Null => Err(Fault::of(94)),
            Self::Text(text) => {
                // A string in a condition is read as a number first, which is
                // how `If "1" Then` works and `If "yes" Then` does not.
                number_in(text).map(|number| number != 0.0).ok_or_else(|| Fault::of(13))
            }
            other => Ok(other.number()? != 0.0),
        }
    }
}

/// A number written the way Visual Basic writes one.
///
/// No trailing zeros, no decimal point when there is nothing after it, and
/// the exponent form for what is too big or too small to write out.
#[must_use]
pub fn written_number(number: f64) -> String {
    if number == 0.0 {
        return "0".to_owned();
    }
    if !number.is_finite() {
        return if number.is_nan() { "NaN".to_owned() } else { "Infinity".to_owned() };
    }
    let size = number.abs();
    if !(1e-4..1e16).contains(&size) {
        // 1E+20, as Visual Basic writes it.
        let written = format!("{number:E}");
        let (mantissa, exponent) = written.split_once('E').unwrap_or((written.as_str(), "0"));
        let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
        let sign = if exponent.starts_with('-') { "-" } else { "+" };
        let digits = exponent.trim_start_matches(['-', '+']);
        return format!("{mantissa}E{sign}{digits}");
    }

    let mut written = format!("{number:.15}");
    if written.contains('.') {
        written = written.trim_end_matches('0').trim_end_matches('.').to_owned();
    }
    // Fifteen digits is what a Double is worth; beyond that the zeros are
    // the arithmetic's noise and not the number's.
    let rounded = format!("{number}");
    if rounded.len() < written.len() {
        return rounded;
    }
    written
}

/// The number a string holds, if the whole of it is one.
///
/// Leading and trailing spaces are allowed, a leading sign is allowed, and
/// `&H` for hexadecimal, which a macro uses for colours.
#[must_use]
pub fn number_in(text: &str) -> Option<f64> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Some(0.0);
    }
    if let Some(digits) = trimmed.strip_prefix("&H").or_else(|| trimmed.strip_prefix("&h")) {
        #[allow(clippy::cast_precision_loss)]
        return i64::from_str_radix(digits.trim_end_matches(['&']), 16).ok().map(|n| n as f64);
    }
    if let Some(digits) = trimmed.strip_prefix("&O").or_else(|| trimmed.strip_prefix("&o")) {
        #[allow(clippy::cast_precision_loss)]
        return i64::from_str_radix(digits.trim_end_matches(['&']), 8).ok().map(|n| n as f64);
    }
    trimmed.parse::<f64>().ok()
}

/// To the nearest, and to the even one when exactly between: what Visual
/// Basic's own rounding does, and what surprises everybody who expects 2.5 to
/// go up.
#[must_use]
pub fn round_half_even(number: f64) -> f64 {
    let down = number.floor();
    let fraction = number - down;
    if (fraction - 0.5).abs() < f64::EPSILON {
        if (down / 2.0).fract() == 0.0 {
            down
        } else {
            down + 1.0
        }
    } else {
        number.round()
    }
}

/// Which of two values decides what kind the answer is.
fn wider(left: &Value, right: &Value) -> u8 {
    fn rank(value: &Value) -> u8 {
        match value {
            Value::Date(_) => 4,
            Value::Double(_) | Value::Text(_) => 3,
            Value::Currency(_) => 2,
            Value::Long(_) | Value::Boolean(_) | Value::Empty => 1,
            _ => 0,
        }
    }
    rank(left).max(rank(right))
}

/// Adding, which is also joining when both sides are text.
pub fn add(left: &Value, right: &Value) -> Result<Value, Fault> {
    if left.is_null() || right.is_null() {
        return Ok(Value::Null);
    }
    // Two strings are joined; a string and a number are added, and a string
    // that is not a number is a type mismatch. That is the rule that makes
    // `+` the wrong way to join two things.
    if let (Value::Text(one), Value::Text(other)) = (left, right) {
        return Ok(Value::Text(format!("{one}{other}")));
    }
    arithmetic(left, right, |a, b| Ok(a + b))
}

pub fn subtract(left: &Value, right: &Value) -> Result<Value, Fault> {
    if left.is_null() || right.is_null() {
        return Ok(Value::Null);
    }
    // A date minus a date is a number of days; a date minus a number is a
    // date.
    if matches!(left, Value::Date(_)) && matches!(right, Value::Date(_)) {
        return Ok(Value::Double(left.number()? - right.number()?));
    }
    arithmetic(left, right, |a, b| Ok(a - b))
}

pub fn multiply(left: &Value, right: &Value) -> Result<Value, Fault> {
    if left.is_null() || right.is_null() {
        return Ok(Value::Null);
    }
    arithmetic(left, right, |a, b| Ok(a * b))
}

/// Dividing, which always gives a Double however whole the two sides are.
pub fn divide(left: &Value, right: &Value) -> Result<Value, Fault> {
    if left.is_null() || right.is_null() {
        return Ok(Value::Null);
    }
    let divisor = right.number()?;
    if divisor == 0.0 {
        return Err(Fault::of(11));
    }
    Ok(Value::Double(left.number()? / divisor))
}

/// Dividing and throwing the fraction away, after rounding both sides.
pub fn divide_whole(left: &Value, right: &Value) -> Result<Value, Fault> {
    if left.is_null() || right.is_null() {
        return Ok(Value::Null);
    }
    let divisor = right.whole()?;
    if divisor == 0 {
        return Err(Fault::of(11));
    }
    Ok(Value::Long(left.whole()? / divisor))
}

/// The remainder, which rounds its two sides before dividing them.
pub fn remainder(left: &Value, right: &Value) -> Result<Value, Fault> {
    if left.is_null() || right.is_null() {
        return Ok(Value::Null);
    }
    let divisor = right.whole()?;
    if divisor == 0 {
        return Err(Fault::of(11));
    }
    Ok(Value::Long(left.whole()? % divisor))
}

/// Raising to a power, which always gives a Double.
pub fn power(left: &Value, right: &Value) -> Result<Value, Fault> {
    if left.is_null() || right.is_null() {
        return Ok(Value::Null);
    }
    Ok(Value::Double(left.number()?.powf(right.number()?)))
}

/// Joining, which turns both sides into text whatever they are.
///
/// `Null` is the one exception, and even it is only half an exception: `Null
/// & "a"` is "a", because joining nothing to something is that something.
pub fn join(left: &Value, right: &Value) -> Result<Value, Fault> {
    if left.is_null() && right.is_null() {
        return Ok(Value::Null);
    }
    let one = if left.is_null() { String::new() } else { left.text()? };
    let other = if right.is_null() { String::new() } else { right.text()? };
    Ok(Value::Text(format!("{one}{other}")))
}

/// What two values do when an operator wants numbers of them.
fn arithmetic(
    left: &Value,
    right: &Value,
    what: impl Fn(f64, f64) -> Result<f64, Fault>,
) -> Result<Value, Fault> {
    let answer = what(left.number()?, right.number()?)?;
    if !answer.is_finite() {
        return Err(Fault::of(6));
    }
    Ok(match wider(left, right) {
        // A date on either side keeps the answer a date.
        4 => Value::Date(answer),
        3 => Value::Double(answer),
        #[allow(clippy::cast_possible_truncation)]
        2 => Value::Currency(round_half_even(answer * 10_000.0) as i64),
        // Two whole numbers stay whole, unless the answer is not one.
        _ if answer.fract() == 0.0 && answer.abs() < 9.2e18 =>
        {
            #[allow(clippy::cast_possible_truncation)]
            Value::Long(answer as i64)
        }
        _ => Value::Double(answer),
    })
}

/// How two values compare, or nothing at all when one of them is `Null`.
///
/// Two strings are compared as text; anything else is compared as numbers,
/// which is why `"10" < "9"` is true and `10 < 9` is not.
pub fn compare(left: &Value, right: &Value) -> Result<Option<core::cmp::Ordering>, Fault> {
    if left.is_null() || right.is_null() {
        return Ok(None);
    }
    if let (Value::Text(one), Value::Text(other)) = (left, right) {
        return Ok(Some(one.cmp(other)));
    }
    // Empty against a string is an empty string, which is how `If s = "" `
    // works on a variable nobody has filled in.
    if matches!(left, Value::Empty) && matches!(right, Value::Text(_)) {
        return Ok(Some(String::new().cmp(&right.text()?)));
    }
    if matches!(right, Value::Empty) && matches!(left, Value::Text(_)) {
        return Ok(Some(left.text()?.cmp(&String::new())));
    }
    let (one, other) = (left.number()?, right.number()?);
    Ok(one.partial_cmp(&other))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn long(number: i64) -> Value {
        Value::Long(number)
    }

    fn text(what: &str) -> Value {
        Value::Text(what.to_owned())
    }

    #[test]
    fn a_string_and_a_number_add_up_and_two_strings_join() {
        // The rule everybody trips over: `+` on two strings joins them, so a
        // macro that means to add has to be sure what it has.
        // Seven, as a Double: a string converted for arithmetic converts to
        // a Double, whatever it looks like.
        assert_eq!(add(&text("3"), &long(4)).expect("adding"), Value::Double(7.0));
        assert_eq!(add(&text("3"), &text("4")).expect("joining"), text("34"));
        assert_eq!(join(&text("3"), &long(4)).expect("joining"), text("34"));
        // And a string that is not a number is a type mismatch, not a nought.
        assert_eq!(add(&text("3 apples"), &long(1)).unwrap_err().number, 13);
    }

    #[test]
    fn empty_is_nought_to_a_sum_and_nothing_to_a_join() {
        assert_eq!(add(&Value::Empty, &long(4)).expect("adding"), long(4));
        assert_eq!(join(&Value::Empty, &text("a")).expect("joining"), text("a"));
        assert_eq!(
            compare(&Value::Empty, &text("")).expect("comparing"),
            Some(core::cmp::Ordering::Equal)
        );
    }

    #[test]
    fn null_swallows_what_it_touches_except_a_join() {
        assert!(add(&Value::Null, &long(1)).expect("adding").is_null());
        assert!(multiply(&Value::Null, &long(0)).expect("multiplying").is_null());
        assert_eq!(join(&Value::Null, &text("a")).expect("joining"), text("a"));
        assert!(join(&Value::Null, &Value::Null).expect("joining").is_null());
        assert!(compare(&Value::Null, &long(1)).expect("comparing").is_none());
    }

    #[test]
    fn true_is_minus_one_as_a_number() {
        assert_eq!(Value::Boolean(true).number().expect("a number"), -1.0);
        assert_eq!(add(&Value::Boolean(true), &long(1)).expect("adding"), long(0));
        assert_eq!(Value::Boolean(false).text().expect("text"), "False");
    }

    #[test]
    fn dividing_gives_a_double_and_the_backslash_throws_the_fraction_away() {
        assert_eq!(divide(&long(7), &long(2)).expect("dividing"), Value::Double(3.5));
        assert_eq!(divide_whole(&long(7), &long(2)).expect("dividing"), long(3));
        // And both sides are rounded first, which is why this is two.
        assert_eq!(remainder(&Value::Double(7.6), &long(3)).expect("a remainder"), long(2));
        assert_eq!(divide(&long(1), &long(0)).unwrap_err().number, 11);
    }

    #[test]
    fn money_adds_up_exactly_where_a_double_would_drift() {
        // A tenth cannot be written in binary, so a hundred of them is not
        // ten. Currency counts ten-thousandths, so it is.
        let tenth = Value::Currency(1_000);
        let mut total = Value::Currency(0);
        for _ in 0..100 {
            total = add(&total, &tenth).expect("adding");
        }
        assert_eq!(total, Value::Currency(100_000));
        assert_eq!(total.text().expect("text"), "10");
    }

    #[test]
    fn a_date_and_a_number_make_a_date_and_two_dates_make_a_number() {
        let day = Value::Date(36_526.0);
        assert_eq!(add(&day, &long(1)).expect("adding"), Value::Date(36_527.0));
        assert_eq!(
            subtract(&Value::Date(36_527.0), &day).expect("subtracting"),
            Value::Double(1.0)
        );
    }

    #[test]
    fn numbers_are_written_the_way_visual_basic_writes_them() {
        assert_eq!(Value::Double(1.5).text().expect("text"), "1.5");
        assert_eq!(Value::Double(3.0).text().expect("text"), "3");
        assert_eq!(Value::Double(0.0001).text().expect("text"), "0.0001");
        assert_eq!(Value::Double(1e20).text().expect("text"), "1E+20");
        assert_eq!(Value::Long(-7).text().expect("text"), "-7");
    }

    #[test]
    fn rounding_goes_to_the_even_one_when_it_is_exactly_between() {
        assert_eq!(round_half_even(2.5), 2.0);
        assert_eq!(round_half_even(3.5), 4.0);
        assert_eq!(round_half_even(-2.5), -2.0);
        assert_eq!(round_half_even(2.4), 2.0);
    }

    #[test]
    fn two_strings_compare_as_text_and_two_numbers_as_numbers() {
        assert_eq!(
            compare(&text("10"), &text("9")).expect("comparing"),
            Some(core::cmp::Ordering::Less)
        );
        assert_eq!(
            compare(&long(10), &long(9)).expect("comparing"),
            Some(core::cmp::Ordering::Greater)
        );
    }

    #[test]
    fn an_array_knows_its_bounds_and_says_when_a_subscript_is_outside_them() {
        let array = Array::new(vec![(1, 3), (0, 1)]);
        assert_eq!(array.values.len(), 6);
        assert_eq!(array.at(&[1, 0]).expect("the first"), 0);
        assert_eq!(array.at(&[3, 1]).expect("the last"), 5);
        assert_eq!(array.at(&[4, 0]).unwrap_err().number, 9);
        assert_eq!(array.at(&[1]).unwrap_err().number, 9);
    }
}
