//! The functions a macro actually uses.
//!
//! Not all of Visual Basic's library — that is hundreds of names, most of
//! them for things this program has no business doing — but the ones that
//! turn up in macros people wrote: taking strings apart, counting, rounding,
//! dates, and the two that talk to whoever is sitting there.
//!
//! # Where the answers come from
//!
//! From what the language documents, and from the arithmetic in
//! [`crate::value`] underneath it. Each one that is easy to get subtly wrong
//! is tested against the answer Visual Basic gives, and the ones worth
//! knowing are written down beside the code: `Mid` counts from one, `InStr`
//! returns nought when it does not find anything rather than minus one,
//! `Round` goes to the even number when it is exactly between, and `Left` of
//! more letters than there are is the whole string rather than an error.

use crate::dates;
use crate::value::{self, Fault, Value};

/// Whatever the program running a macro is willing to do for it.
///
/// A macro that shows a message box is asking the program in front of the
/// person, and a test is asking nothing at all, so this is what tells the two
/// apart. Everything with a side effect that leaves this crate goes through
/// here.
pub trait Host {
    /// Shows a message and says which button was pressed. `vbOK` is one.
    fn message(&mut self, text: &str, buttons: i64, title: &str) -> i64 {
        let _ = (text, buttons, title);
        1
    }

    /// Asks for a line of text, or nothing if it was cancelled.
    fn ask(&mut self, prompt: &str, title: &str, default: &str) -> Option<String> {
        let _ = (prompt, title);
        (!default.is_empty()).then(|| default.to_owned())
    }

    /// `Debug.Print`.
    fn note(&mut self, text: &str) {
        let _ = text;
    }
}

/// A host that says nothing and remembers everything, which is what a test
/// wants and what a program that is not showing anything yet can use.
#[derive(Clone, Debug, Default)]
pub struct Quiet {
    /// What was shown, in order.
    pub messages: Vec<String>,
    /// And what was printed.
    pub notes: Vec<String>,
    /// What the next `InputBox` will be answered with.
    pub answers: Vec<String>,
}

impl Host for Quiet {
    fn message(&mut self, text: &str, _buttons: i64, _title: &str) -> i64 {
        self.messages.push(text.to_owned());
        1
    }

    fn ask(&mut self, _prompt: &str, _title: &str, default: &str) -> Option<String> {
        if self.answers.is_empty() {
            return (!default.is_empty()).then(|| default.to_owned());
        }
        Some(self.answers.remove(0))
    }

    fn note(&mut self, text: &str) {
        self.notes.push(text.to_owned());
    }
}

/// The constants the language itself defines.
#[must_use]
pub fn constant(name: &str) -> Option<Value> {
    let lowered = name.to_ascii_lowercase();
    Some(match lowered.as_str() {
        "true" => Value::Boolean(true),
        "false" => Value::Boolean(false),
        "empty" => Value::Empty,
        "null" => Value::Null,
        "nothing" => Value::Nothing,
        "vbcrlf" | "vbnewline" => Value::Text("\r\n".to_owned()),
        "vbcr" => Value::Text("\r".to_owned()),
        "vblf" => Value::Text("\n".to_owned()),
        "vbtab" => Value::Text("\t".to_owned()),
        "vbnullstring" => Value::Text(String::new()),
        "vbback" => Value::Text("\u{8}".to_owned()),
        "vbobjecterror" => Value::Long(-2_147_221_504),
        // The buttons a message box may show, and the answers it may give.
        "vbokonly" => Value::Long(0),
        "vbokcancel" => Value::Long(1),
        "vbabortretryignore" => Value::Long(2),
        "vbyesnocancel" => Value::Long(3),
        "vbyesno" => Value::Long(4),
        "vbretrycancel" => Value::Long(5),
        "vbcritical" => Value::Long(16),
        "vbquestion" => Value::Long(32),
        "vbexclamation" => Value::Long(48),
        "vbinformation" => Value::Long(64),
        "vbok" => Value::Long(1),
        "vbcancel" => Value::Long(2),
        "vbabort" => Value::Long(3),
        "vbretry" => Value::Long(4),
        "vbignore" => Value::Long(5),
        "vbyes" => Value::Long(6),
        "vbno" => Value::Long(7),
        // What a comparison is made by, and what a variable is.
        "vbbinarycompare" => Value::Long(0),
        "vbtextcompare" => Value::Long(1),
        "vbempty" => Value::Long(0),
        "vbnull" => Value::Long(1),
        "vbinteger" => Value::Long(2),
        "vblong" => Value::Long(3),
        "vbsingle" => Value::Long(4),
        "vbdouble" => Value::Long(5),
        "vbcurrency" => Value::Long(6),
        "vbdate" => Value::Long(7),
        "vbstring" => Value::Long(8),
        "vbobject" => Value::Long(9),
        "vberror" => Value::Long(10),
        "vbboolean" => Value::Long(11),
        "vbvariant" => Value::Long(12),
        "vbarray" => Value::Long(8192),
        // The days and the parts of a date, for `DateAdd` and `Weekday`.
        "vbsunday" => Value::Long(1),
        "vbmonday" => Value::Long(2),
        "vbtuesday" => Value::Long(3),
        "vbwednesday" => Value::Long(4),
        "vbthursday" => Value::Long(5),
        "vbfriday" => Value::Long(6),
        "vbsaturday" => Value::Long(7),
        _ => return None,
    })
}

/// One argument, or `Empty` where it was left out.
fn argument(arguments: &[Value], at: usize) -> Value {
    arguments.get(at).cloned().unwrap_or(Value::Empty)
}

/// The text of one argument.
fn text_of(arguments: &[Value], at: usize) -> Result<String, Fault> {
    argument(arguments, at).text()
}

/// The number of one argument.
fn number_of(arguments: &[Value], at: usize) -> Result<f64, Fault> {
    argument(arguments, at).number()
}

/// The whole number of one argument.
fn whole_of(arguments: &[Value], at: usize) -> Result<i64, Fault> {
    argument(arguments, at).whole()
}

/// Calls one of the library's own functions.
///
/// Nothing at all when the name is not one of them, which is how the caller
/// knows to look for a macro's own procedure of that name instead.
pub fn call(name: &str, arguments: &[Value], host: &mut dyn Host) -> Option<Result<Value, Fault>> {
    knows(name).then(|| called(&name.to_ascii_lowercase(), arguments, host))
}

/// One of them, by its name in lower case.
fn called(lowered: &str, arguments: &[Value], host: &mut dyn Host) -> Result<Value, Fault> {
    // A `Null` given to nearly anything makes the answer `Null`, and the few
    // that say something about it instead are asked first.
    if !matches!(
        lowered,
        "isnull" | "isempty" | "isarray" | "isobject" | "iserror" | "typename" | "vartype" | "iif"
    ) && arguments.iter().any(Value::is_null)
    {
        return Ok(Value::Null);
    }
    match lowered {
        "len" => len(arguments),
        "left" => left(arguments),
        "right" => right(arguments),
        "mid" => mid(arguments),
        "instr" => instr(arguments),
        "instrrev" => instr_rev(arguments),
        "replace" => replace(arguments),
        "trim" => text_of(arguments, 0).map(|text| Value::Text(text.trim().to_owned())),
        "ltrim" => text_of(arguments, 0).map(|text| Value::Text(text.trim_start().to_owned())),
        "rtrim" => text_of(arguments, 0).map(|text| Value::Text(text.trim_end().to_owned())),
        "ucase" => text_of(arguments, 0).map(|text| Value::Text(text.to_uppercase())),
        "lcase" => text_of(arguments, 0).map(|text| Value::Text(text.to_lowercase())),
        "strreverse" => text_of(arguments, 0).map(|text| Value::Text(text.chars().rev().collect())),
        "space" => whole_of(arguments, 0)
            .map(|count| Value::Text(" ".repeat(usize::try_from(count).unwrap_or_default()))),
        "string" => string(arguments),
        "split" => split(arguments),
        "join" => join(arguments),
        "strcomp" => compare_text(arguments),
        "chr" | "chrw" => chr(arguments),
        "asc" | "ascw" => asc(arguments),
        "val" => Ok(Value::Double(
            value::number_in(&leading_number(&text_of(arguments, 0)?)).unwrap_or(0.0),
        )),
        "cstr" => text_of(arguments, 0).map(Value::Text),
        "str" => number_of(arguments, 0).map(|number| {
            // `Str` leaves room for the sign, which is why a positive number
            // comes back with a space in front of it.
            let written = value::written_number(number);
            Value::Text(if number < 0.0 { written } else { format!(" {written}") })
        }),
        "format" => format(arguments),

        "abs" => number_of(arguments, 0).map(|number| Value::Double(number.abs())),
        "sgn" => number_of(arguments, 0)
            .map(|number| Value::Long(number.signum() as i64 * i64::from(number != 0.0))),
        "int" => number_of(arguments, 0).map(|number| Value::Double(number.floor())),
        "fix" => number_of(arguments, 0).map(|number| Value::Double(number.trunc())),
        "sqr" => square_root(arguments),
        "exp" => number_of(arguments, 0).map(|number| Value::Double(number.exp())),
        "log" => logarithm(arguments),
        "sin" => number_of(arguments, 0).map(|number| Value::Double(number.sin())),
        "cos" => number_of(arguments, 0).map(|number| Value::Double(number.cos())),
        "tan" => number_of(arguments, 0).map(|number| Value::Double(number.tan())),
        "atn" => number_of(arguments, 0).map(|number| Value::Double(number.atan())),
        "round" => round(arguments),
        "hex" => whole_of(arguments, 0).map(|number| Value::Text(format!("{number:X}"))),
        "oct" => whole_of(arguments, 0).map(|number| Value::Text(format!("{number:o}"))),

        "cint" | "clng" | "cbyte" => whole_of(arguments, 0).map(Value::Long),
        "cdbl" | "csng" => number_of(arguments, 0).map(Value::Double),
        "ccur" => number_of(arguments, 0)
            .map(|number| Value::Currency(value::round_half_even(number * 10_000.0) as i64)),
        "cbool" => argument(arguments, 0).truth().map(Value::Boolean),
        "cdate" | "datevalue" | "timevalue" => as_date(arguments),

        "isnumeric" => Ok(Value::Boolean(match argument(arguments, 0) {
            Value::Text(text) => value::number_in(&text).is_some(),
            Value::Empty | Value::Null | Value::Nothing | Value::Array(_) => false,
            _ => true,
        })),
        "isempty" => Ok(Value::Boolean(matches!(argument(arguments, 0), Value::Empty))),
        "isnull" => Ok(Value::Boolean(argument(arguments, 0).is_null())),
        "isdate" => Ok(Value::Boolean(match argument(arguments, 0) {
            Value::Date(_) => true,
            Value::Text(text) => dates::from_text(&text).is_some(),
            _ => false,
        })),
        "isarray" => Ok(Value::Boolean(matches!(argument(arguments, 0), Value::Array(_)))),
        "isobject" => Ok(Value::Boolean(matches!(argument(arguments, 0), Value::Nothing))),
        "iserror" => Ok(Value::Boolean(false)),
        "typename" => Ok(Value::Text(argument(arguments, 0).type_name().to_owned())),
        "vartype" => Ok(Value::Long(argument(arguments, 0).var_type())),

        "lbound" => bound(arguments, true),
        "ubound" => bound(arguments, false),
        "array" => Ok(Value::Array(Box::new(value::Array {
            bounds: vec![(0, arguments.len() as i64 - 1)],
            values: arguments.to_vec(),
        }))),

        "year" => date_part_of(arguments, |serial| dates::parts(serial).0),
        "month" => date_part_of(arguments, |serial| i64::from(dates::parts(serial).1)),
        "day" => date_part_of(arguments, |serial| i64::from(dates::parts(serial).2)),
        "hour" => date_part_of(arguments, |serial| i64::from(dates::time_parts(serial).0)),
        "minute" => date_part_of(arguments, |serial| i64::from(dates::time_parts(serial).1)),
        "second" => date_part_of(arguments, |serial| i64::from(dates::time_parts(serial).2)),
        "weekday" => date_part_of(arguments, dates::weekday),
        "dateserial" => Ok(Value::Date(dates::serial(
            whole_of(arguments, 0)?,
            whole_of(arguments, 1)?,
            whole_of(arguments, 2)?,
        ))),
        "timeserial" => Ok(Value::Date(dates::time_serial(
            whole_of(arguments, 0)?,
            whole_of(arguments, 1)?,
            whole_of(arguments, 2)?,
        ))),
        "dateadd" => date_add(arguments),
        "datediff" => date_difference(arguments),
        "datepart" => date_part(arguments),
        "monthname" => month_name(arguments),
        "weekdayname" => weekday_name(arguments),

        "iif" => Ok(if argument(arguments, 0).truth()? {
            argument(arguments, 1)
        } else {
            argument(arguments, 2)
        }),
        "choose" => {
            let which = whole_of(arguments, 0)?;
            Ok(usize::try_from(which)
                .ok()
                .and_then(|which| arguments.get(which))
                .cloned()
                .unwrap_or(Value::Null))
        }
        "switch" => {
            let mut answer = Value::Null;
            for pair in arguments.chunks(2) {
                if pair.len() == 2 && pair[0].truth()? {
                    answer = pair[1].clone();
                    break;
                }
            }
            Ok(answer)
        }

        "msgbox" => {
            let text = text_of(arguments, 0)?;
            let buttons = arguments.get(1).map_or(Ok(0), Value::whole)?;
            let title = arguments.get(2).map_or(Ok(String::new()), Value::text)?;
            Ok(Value::Long(host.message(&text, buttons, &title)))
        }
        "inputbox" => {
            let prompt = text_of(arguments, 0)?;
            let title = arguments.get(1).map_or(Ok(String::new()), Value::text)?;
            let default = arguments.get(2).map_or(Ok(String::new()), Value::text)?;
            Ok(Value::Text(host.ask(&prompt, &title, &default).unwrap_or_default()))
        }
        // Every name `knows` says is here is above. A test walks the list
        // against this arm, so a name added to one and not the other is
        // caught by the suite rather than by whoever runs the macro — and if
        // one ever does slip through, this is what they would be told.
        _ => Err(Fault::saying(5, NOT_WRITTEN)),
    }
}

/// What a name on the list with nothing behind it would say.
const NOT_WRITTEN: &str = "This function is named in the library and is not written";

/// Every name the library answers to, in lower case.
pub const NAMES: [&str; 81] = [
    "len",
    "left",
    "right",
    "mid",
    "instr",
    "instrrev",
    "replace",
    "trim",
    "ltrim",
    "rtrim",
    "ucase",
    "lcase",
    "strreverse",
    "space",
    "string",
    "split",
    "join",
    "strcomp",
    "chr",
    "chrw",
    "asc",
    "ascw",
    "val",
    "cstr",
    "str",
    "format",
    "abs",
    "sgn",
    "int",
    "fix",
    "sqr",
    "exp",
    "log",
    "sin",
    "cos",
    "tan",
    "atn",
    "round",
    "hex",
    "oct",
    "cint",
    "clng",
    "cbyte",
    "cdbl",
    "csng",
    "ccur",
    "cbool",
    "cdate",
    "datevalue",
    "timevalue",
    "isnumeric",
    "isempty",
    "isnull",
    "isdate",
    "isarray",
    "isobject",
    "iserror",
    "typename",
    "vartype",
    "lbound",
    "ubound",
    "array",
    "year",
    "month",
    "day",
    "hour",
    "minute",
    "second",
    "weekday",
    "dateserial",
    "timeserial",
    "dateadd",
    "datediff",
    "datepart",
    "monthname",
    "weekdayname",
    "iif",
    "choose",
    "switch",
    "msgbox",
    "inputbox",
];

/// Whether a name is one of the library's, without calling it.
#[must_use]
pub fn knows(name: &str) -> bool {
    NAMES.contains(&name.to_ascii_lowercase().as_str())
}

// --- Strings ---------------------------------------------------------------

fn len(arguments: &[Value]) -> Result<Value, Fault> {
    Ok(Value::Long(text_of(arguments, 0)?.chars().count() as i64))
}

fn left(arguments: &[Value]) -> Result<Value, Fault> {
    let text = text_of(arguments, 0)?;
    let count = whole_of(arguments, 1)?;
    if count < 0 {
        return Err(Fault::of(5));
    }
    Ok(Value::Text(text.chars().take(usize::try_from(count).unwrap_or_default()).collect()))
}

fn right(arguments: &[Value]) -> Result<Value, Fault> {
    let text = text_of(arguments, 0)?;
    let count = whole_of(arguments, 1)?;
    if count < 0 {
        return Err(Fault::of(5));
    }
    let letters: Vec<char> = text.chars().collect();
    let from = letters.len().saturating_sub(usize::try_from(count).unwrap_or_default());
    Ok(Value::Text(letters[from..].iter().collect()))
}

/// `Mid`, which counts from one, not from nought.
fn mid(arguments: &[Value]) -> Result<Value, Fault> {
    let text = text_of(arguments, 0)?;
    let start = whole_of(arguments, 1)?;
    if start < 1 {
        return Err(Fault::of(5));
    }
    let letters: Vec<char> = text.chars().collect();
    let from = (start - 1) as usize;
    if from >= letters.len() {
        return Ok(Value::Text(String::new()));
    }
    let count = match arguments.get(2) {
        None | Some(Value::Empty) => letters.len() - from,
        Some(value) => usize::try_from(value.whole()?).map_err(|_| Fault::of(5))?,
    };
    Ok(Value::Text(letters[from..(from + count).min(letters.len())].iter().collect()))
}

/// `InStr`, which counts from one and answers nought when it finds nothing.
fn instr(arguments: &[Value]) -> Result<Value, Fault> {
    // `InStr` takes its start first when it is given one, which is the only
    // function in the library that moves its arguments about.
    let (start, haystack, needle) = if arguments.len() >= 3 {
        (whole_of(arguments, 0)?, text_of(arguments, 1)?, text_of(arguments, 2)?)
    } else {
        (1, text_of(arguments, 0)?, text_of(arguments, 1)?)
    };
    if start < 1 {
        return Err(Fault::of(5));
    }
    let letters: Vec<char> = haystack.chars().collect();
    let from = (start - 1) as usize;
    if from >= letters.len() {
        return Ok(Value::Long(0));
    }
    let rest: String = letters[from..].iter().collect();
    if needle.is_empty() {
        return Ok(Value::Long(start));
    }
    Ok(Value::Long(match rest.find(&needle) {
        Some(at) => (rest[..at].chars().count() + from + 1) as i64,
        None => 0,
    }))
}

fn instr_rev(arguments: &[Value]) -> Result<Value, Fault> {
    let haystack = text_of(arguments, 0)?;
    let needle = text_of(arguments, 1)?;
    if needle.is_empty() {
        return Ok(Value::Long(0));
    }
    Ok(Value::Long(match haystack.rfind(&needle) {
        Some(at) => (haystack[..at].chars().count() + 1) as i64,
        None => 0,
    }))
}

fn replace(arguments: &[Value]) -> Result<Value, Fault> {
    let text = text_of(arguments, 0)?;
    let from = text_of(arguments, 1)?;
    let to = text_of(arguments, 2)?;
    if from.is_empty() {
        return Ok(Value::Text(text));
    }
    Ok(Value::Text(text.replace(&from, &to)))
}

fn string(arguments: &[Value]) -> Result<Value, Fault> {
    let count = usize::try_from(whole_of(arguments, 0)?).map_err(|_| Fault::of(5))?;
    let letter = match argument(arguments, 1) {
        Value::Text(text) => text.chars().next().unwrap_or(' '),
        other => char::from_u32(u32::try_from(other.whole()?).map_err(|_| Fault::of(5))?)
            .ok_or_else(|| Fault::of(5))?,
    };
    Ok(Value::Text(letter.to_string().repeat(count)))
}

fn split(arguments: &[Value]) -> Result<Value, Fault> {
    let text = text_of(arguments, 0)?;
    let between = match arguments.get(1) {
        None | Some(Value::Empty) => " ".to_owned(),
        Some(value) => value.text()?,
    };
    let pieces: Vec<Value> = if between.is_empty() {
        vec![Value::Text(text)]
    } else {
        text.split(&between).map(|piece| Value::Text(piece.to_owned())).collect()
    };
    Ok(Value::Array(Box::new(value::Array {
        bounds: vec![(0, pieces.len() as i64 - 1)],
        values: pieces,
    })))
}

fn join(arguments: &[Value]) -> Result<Value, Fault> {
    let Value::Array(array) = argument(arguments, 0) else {
        return Err(Fault::of(13));
    };
    let between = match arguments.get(1) {
        None | Some(Value::Empty) => " ".to_owned(),
        Some(value) => value.text()?,
    };
    let mut pieces = Vec::with_capacity(array.values.len());
    for value in &array.values {
        pieces.push(value.text()?);
    }
    Ok(Value::Text(pieces.join(&between)))
}

fn compare_text(arguments: &[Value]) -> Result<Value, Fault> {
    let one = text_of(arguments, 0)?;
    let other = text_of(arguments, 1)?;
    // The third argument says whether case matters: one means it does not.
    let (one, other) = if whole_of(arguments, 2).unwrap_or(0) == 1 {
        (one.to_lowercase(), other.to_lowercase())
    } else {
        (one, other)
    };
    Ok(Value::Long(match one.cmp(&other) {
        core::cmp::Ordering::Less => -1,
        core::cmp::Ordering::Equal => 0,
        core::cmp::Ordering::Greater => 1,
    }))
}

fn chr(arguments: &[Value]) -> Result<Value, Fault> {
    let number = whole_of(arguments, 0)?;
    let letter = u32::try_from(number).ok().and_then(char::from_u32).ok_or_else(|| Fault::of(5))?;
    Ok(Value::Text(letter.to_string()))
}

fn asc(arguments: &[Value]) -> Result<Value, Fault> {
    let text = text_of(arguments, 0)?;
    let letter = text.chars().next().ok_or_else(|| Fault::of(5))?;
    Ok(Value::Long(i64::from(u32::from(letter))))
}

/// The number at the front of a string, which is what `Val` answers with.
fn leading_number(text: &str) -> String {
    let mut out = String::new();
    let mut seen_point = false;
    for letter in text.chars() {
        match letter {
            ' ' | '\t' if out.is_empty() => {}
            '-' | '+' if out.is_empty() => out.push(letter),
            '.' if !seen_point => {
                seen_point = true;
                out.push(letter);
            }
            digit if digit.is_ascii_digit() => out.push(digit),
            _ => break,
        }
    }
    out
}

// --- Numbers ---------------------------------------------------------------

fn square_root(arguments: &[Value]) -> Result<Value, Fault> {
    let number = number_of(arguments, 0)?;
    if number < 0.0 {
        return Err(Fault::of(5));
    }
    Ok(Value::Double(number.sqrt()))
}

fn logarithm(arguments: &[Value]) -> Result<Value, Fault> {
    let number = number_of(arguments, 0)?;
    if number <= 0.0 {
        return Err(Fault::of(5));
    }
    Ok(Value::Double(number.ln()))
}

/// `Round`, which goes to the even one when it is exactly between — so two
/// and a half rounds to two, and everybody is surprised once.
fn round(arguments: &[Value]) -> Result<Value, Fault> {
    let number = number_of(arguments, 0)?;
    let places = arguments.get(1).map_or(Ok(0), Value::whole)?;
    if places < 0 {
        return Err(Fault::of(5));
    }
    let scale = 10f64.powi(i32::try_from(places).map_err(|_| Fault::of(5))?);
    Ok(Value::Double(value::round_half_even(number * scale) / scale))
}

fn bound(arguments: &[Value], low: bool) -> Result<Value, Fault> {
    let Value::Array(array) = argument(arguments, 0) else {
        return Err(Fault::of(13));
    };
    let which = arguments.get(1).map_or(Ok(1), Value::whole)?;
    let at = usize::try_from(which - 1).map_err(|_| Fault::of(9))?;
    let (lowest, highest) = *array.bounds.get(at).ok_or_else(|| Fault::of(9))?;
    Ok(Value::Long(if low { lowest } else { highest }))
}

// --- Dates -----------------------------------------------------------------

fn as_date(arguments: &[Value]) -> Result<Value, Fault> {
    Ok(match argument(arguments, 0) {
        Value::Date(serial) => Value::Date(serial),
        Value::Text(text) => Value::Date(dates::from_text(&text).ok_or_else(|| Fault::of(13))?),
        other => Value::Date(other.number()?),
    })
}

fn serial_of(arguments: &[Value], at: usize) -> Result<f64, Fault> {
    match argument(arguments, at) {
        Value::Text(text) => dates::from_text(&text).ok_or_else(|| Fault::of(13)),
        other => other.number(),
    }
}

fn date_part_of(arguments: &[Value], what: impl Fn(f64) -> i64) -> Result<Value, Fault> {
    Ok(Value::Long(what(serial_of(arguments, 0)?)))
}

/// `DateAdd("m", 1, d)`: the units are the language's own two-letter names.
fn date_add(arguments: &[Value]) -> Result<Value, Fault> {
    let unit = text_of(arguments, 0)?.to_lowercase();
    let count = whole_of(arguments, 1)?;
    let serial = serial_of(arguments, 2)?;
    let (year, month, day) = dates::parts(serial);
    let time = serial - serial.floor();

    #[allow(clippy::cast_precision_loss)]
    Ok(Value::Date(match unit.as_str() {
        "yyyy" => dates::serial(year + count, i64::from(month), i64::from(day)) + time,
        "q" => dates::serial(year, i64::from(month) + count * 3, i64::from(day)) + time,
        "m" => dates::serial(year, i64::from(month) + count, i64::from(day)) + time,
        "y" | "d" | "w" => serial + count as f64,
        "ww" => serial + (count * 7) as f64,
        "h" => serial + count as f64 / 24.0,
        "n" => serial + count as f64 / 1440.0,
        "s" => serial + count as f64 / 86_400.0,
        _ => return Err(Fault::of(5)),
    }))
}

fn date_difference(arguments: &[Value]) -> Result<Value, Fault> {
    let unit = text_of(arguments, 0)?.to_lowercase();
    let from = serial_of(arguments, 1)?;
    let to = serial_of(arguments, 2)?;
    let (from_year, from_month, _) = dates::parts(from);
    let (to_year, to_month, _) = dates::parts(to);

    #[allow(clippy::cast_possible_truncation)]
    Ok(Value::Long(match unit.as_str() {
        "yyyy" => to_year - from_year,
        "q" => {
            (to_year - from_year) * 4 + (i64::from(to_month) - 1) / 3
                - (i64::from(from_month) - 1) / 3
        }
        "m" => (to_year - from_year) * 12 + i64::from(to_month) - i64::from(from_month),
        "y" | "d" => (to.floor() - from.floor()) as i64,
        "w" | "ww" => ((to.floor() - from.floor()) / 7.0).trunc() as i64,
        "h" => ((to - from) * 24.0).trunc() as i64,
        "n" => ((to - from) * 1440.0).trunc() as i64,
        "s" => ((to - from) * 86_400.0).round() as i64,
        _ => return Err(Fault::of(5)),
    }))
}

fn date_part(arguments: &[Value]) -> Result<Value, Fault> {
    let unit = text_of(arguments, 0)?.to_lowercase();
    let serial = serial_of(arguments, 1)?;
    let (year, month, day) = dates::parts(serial);
    let (hour, minute, second) = dates::time_parts(serial);
    Ok(Value::Long(match unit.as_str() {
        "yyyy" => year,
        "q" => (i64::from(month) - 1) / 3 + 1,
        "m" => i64::from(month),
        "d" => i64::from(day),
        "w" => dates::weekday(serial),
        "h" => i64::from(hour),
        "n" => i64::from(minute),
        "s" => i64::from(second),
        "y" => (serial.floor() - dates::serial(year, 1, 1)) as i64 + 1,
        "ww" => ((serial.floor() - dates::serial(year, 1, 1)) / 7.0).floor() as i64 + 1,
        _ => return Err(Fault::of(5)),
    }))
}

/// The names of the months, which `Format` and `MonthName` both want.
pub(crate) const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// And of the days, from Sunday, because that is day one.
pub(crate) const DAYS: [&str; 7] =
    ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

fn month_name(arguments: &[Value]) -> Result<Value, Fault> {
    let which = whole_of(arguments, 0)?;
    let short = arguments.get(1).map_or(Ok(false), Value::truth)?;
    let name = MONTHS
        .get(usize::try_from(which - 1).map_err(|_| Fault::of(5))?)
        .ok_or_else(|| Fault::of(5))?;
    Ok(Value::Text(if short { name[..3].to_owned() } else { (*name).to_owned() }))
}

fn weekday_name(arguments: &[Value]) -> Result<Value, Fault> {
    let which = whole_of(arguments, 0)?;
    let short = arguments.get(1).map_or(Ok(false), Value::truth)?;
    let name = DAYS
        .get(usize::try_from(which - 1).map_err(|_| Fault::of(5))?)
        .ok_or_else(|| Fault::of(5))?;
    Ok(Value::Text(if short { name[..3].to_owned() } else { (*name).to_owned() }))
}

// --- Format ----------------------------------------------------------------

/// `Format`, with the pictures a macro actually writes.
///
/// The named ones, the number pictures made of `0`, `#`, a point and commas,
/// and the date pictures made of `y`, `m`, `d`, `h`, `n` and `s`. Which of
/// the two a picture is depends on what is in it, which is how Visual Basic
/// decides as well: a picture with a `d` or a `y` in it is about a date, one
/// with a `0` or a `#` is about a number.
fn format(arguments: &[Value]) -> Result<Value, Fault> {
    let value = argument(arguments, 0);
    let picture = match arguments.get(1) {
        None | Some(Value::Empty) => return value.text().map(Value::Text),
        Some(value) => value.text()?,
    };

    if let Some(answer) = named_format(&value, &picture)? {
        return Ok(Value::Text(answer));
    }
    if is_a_date_picture(&picture) {
        let serial = match &value {
            Value::Text(text) => dates::from_text(text).ok_or_else(|| Fault::of(13))?,
            other => other.number()?,
        };
        return Ok(Value::Text(date_picture(serial, &picture)));
    }
    Ok(Value::Text(number_picture(value.number()?, &picture)))
}

fn named_format(value: &Value, picture: &str) -> Result<Option<String>, Fault> {
    let lowered = picture.to_lowercase();
    Ok(Some(match lowered.as_str() {
        "general number" => value::written_number(value.number()?),
        "fixed" => format!("{:.2}", value.number()?),
        "standard" => with_thousands(&format!("{:.2}", value.number()?)),
        "currency" => format!("£{}", with_thousands(&format!("{:.2}", value.number()?.abs()))),
        "percent" => format!("{:.2}%", value.number()? * 100.0),
        "scientific" => format!("{:E}", value.number()?),
        "yes/no" => (if value.truth()? { "Yes" } else { "No" }).to_owned(),
        "true/false" => (if value.truth()? { "True" } else { "False" }).to_owned(),
        "on/off" => (if value.truth()? { "On" } else { "Off" }).to_owned(),
        "long date" => date_picture(value.number()?, "dddd, d mmmm yyyy"),
        "medium date" => date_picture(value.number()?, "dd-mmm-yy"),
        "short date" => date_picture(value.number()?, "dd/mm/yyyy"),
        "long time" => date_picture(value.number()?, "hh:nn:ss"),
        "medium time" => date_picture(value.number()?, "hh:nn AM/PM"),
        "short time" => date_picture(value.number()?, "hh:nn"),
        _ => return Ok(None),
    }))
}

fn is_a_date_picture(picture: &str) -> bool {
    let lowered = picture.to_lowercase();
    lowered.contains('y')
        || lowered.contains('d')
        || lowered.contains('h')
        || lowered.contains(':')
        || lowered.contains("mmm")
}

/// A date written to a picture.
fn date_picture(serial: f64, picture: &str) -> String {
    let (year, month, day) = dates::parts(serial);
    let (hour, minute, second) = dates::time_parts(serial);
    let twelve_hour = picture.to_lowercase().contains("am/pm") || picture.contains("AMPM");
    let shown_hour = if twelve_hour {
        match hour % 12 {
            0 => 12,
            other => other,
        }
    } else {
        hour
    };

    let letters: Vec<char> = picture.chars().collect();
    let mut out = String::new();
    let mut at = 0usize;
    while at < letters.len() {
        // How many of the same letter in a row, which is what says how wide
        // the piece is: `d`, `dd`, `ddd`, `dddd`.
        let letter = letters[at];
        let mut run = 1usize;
        while at + run < letters.len() && letters[at + run] == letter {
            run += 1;
        }
        let rest: String = letters[at..].iter().collect();
        let lowered_rest = rest.to_lowercase();

        if lowered_rest.starts_with("am/pm") {
            out.push_str(if hour < 12 { "AM" } else { "PM" });
            at += 5;
            continue;
        }
        match letter.to_ascii_lowercase() {
            'y' if run >= 4 => out.push_str(&format!("{year:04}")),
            'y' if run >= 2 => out.push_str(&format!("{:02}", year % 100)),
            'y' => out.push_str(&(year % 100).to_string()),
            'm' if run >= 4 => out.push_str(MONTHS[(month as usize - 1) % 12]),
            'm' if run == 3 => out.push_str(&MONTHS[(month as usize - 1) % 12][..3]),
            'm' if run == 2 => out.push_str(&format!("{month:02}")),
            'm' => out.push_str(&month.to_string()),
            'd' if run >= 4 => out.push_str(DAYS[(dates::weekday(serial) as usize - 1) % 7]),
            'd' if run == 3 => {
                out.push_str(&DAYS[(dates::weekday(serial) as usize - 1) % 7][..3]);
            }
            'd' if run == 2 => out.push_str(&format!("{day:02}")),
            'd' => out.push_str(&day.to_string()),
            'h' if run >= 2 => out.push_str(&format!("{shown_hour:02}")),
            'h' => out.push_str(&shown_hour.to_string()),
            'n' if run >= 2 => out.push_str(&format!("{minute:02}")),
            'n' => out.push_str(&minute.to_string()),
            's' if run >= 2 => out.push_str(&format!("{second:02}")),
            's' => out.push_str(&second.to_string()),
            _ => {
                out.extend(letters[at..at + run].iter());
            }
        }
        at += run;
    }
    out
}

/// A number written to a picture of `0`, `#`, a point and commas.
fn number_picture(number: f64, picture: &str) -> String {
    let percent = picture.contains('%');
    let number = if percent { number * 100.0 } else { number };
    let thousands = picture.contains(',');

    let (before, after) = match picture.split_once('.') {
        Some((before, after)) => (before, after),
        None => (picture, ""),
    };
    let places = after.chars().filter(|letter| *letter == '0' || *letter == '#').count();
    let least = before.chars().filter(|letter| *letter == '0').count().max(1);

    let rounded = format!("{:.*}", places, number.abs());
    let (whole, fraction) = match rounded.split_once('.') {
        Some((whole, fraction)) => (whole.to_owned(), fraction.to_owned()),
        None => (rounded.clone(), String::new()),
    };
    let mut whole = whole;
    while whole.len() < least {
        whole.insert(0, '0');
    }
    if thousands {
        whole = with_thousands(&whole);
    }

    // A `#` after the point shows a digit only when there is one to show.
    let mut fraction = fraction;
    if after.chars().rev().take_while(|letter| *letter == '#').count() > 0 {
        while fraction.ends_with('0') {
            fraction.pop();
        }
    }

    let mut out = String::new();
    if number < 0.0 {
        out.push('-');
    }
    out.push_str(&whole);
    if !fraction.is_empty() {
        out.push('.');
        out.push_str(&fraction);
    }
    if percent {
        out.push('%');
    }
    out
}

/// A whole number with a comma every three digits.
fn with_thousands(number: &str) -> String {
    let (whole, rest) = match number.split_once('.') {
        Some((whole, rest)) => (whole, Some(rest)),
        None => (number, None),
    };
    let (sign, digits) = match whole.strip_prefix('-') {
        Some(digits) => ("-", digits),
        None => ("", whole),
    };
    let mut out = String::new();
    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    match rest {
        Some(rest) => format!("{sign}{out}.{rest}"),
        None => format!("{sign}{out}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn called(name: &str, arguments: &[Value]) -> Value {
        let mut host = Quiet::default();
        call(name, arguments, &mut host).expect("a known name").expect("an answer")
    }

    fn text(what: &str) -> Value {
        Value::Text(what.to_owned())
    }

    #[test]
    fn strings_are_counted_from_one() {
        // The mistake everybody makes once: `Mid` and `InStr` count from one,
        // and `InStr` answers nought when it finds nothing.
        assert_eq!(called("Mid", &[text("Hello"), Value::Long(2), Value::Long(3)]), text("ell"));
        assert_eq!(called("Mid", &[text("Hello"), Value::Long(4)]), text("lo"));
        assert_eq!(called("InStr", &[text("Hello"), text("l")]), Value::Long(3));
        assert_eq!(called("InStr", &[Value::Long(4), text("Hello"), text("l")]), Value::Long(4));
        assert_eq!(called("InStr", &[text("Hello"), text("z")]), Value::Long(0));
        assert_eq!(called("Left", &[text("Hi"), Value::Long(10)]), text("Hi"));
        assert_eq!(called("Right", &[text("Hello"), Value::Long(2)]), text("lo"));
        assert_eq!(called("Len", &[text("Hello")]), Value::Long(5));
    }

    #[test]
    fn splitting_and_joining_are_the_two_ways_round() {
        let pieces = called("Split", &[text("a,b,c"), text(",")]);
        let Value::Array(array) = &pieces else { panic!("not an array: {pieces:?}") };
        assert_eq!(array.bounds, vec![(0, 2)]);
        assert_eq!(called("UBound", &[pieces.clone()]), Value::Long(2));
        assert_eq!(called("Join", &[pieces, text("-")]), text("a-b-c"));
    }

    #[test]
    fn the_numbers_that_surprise_people() {
        // Round goes to the even one; Int goes down and Fix goes towards
        // nought, which is the same for a positive number and not for a
        // negative one.
        assert_eq!(called("Round", &[Value::Double(2.5)]), Value::Double(2.0));
        assert_eq!(called("Round", &[Value::Double(1.5)]), Value::Double(2.0));
        // Written in binary, 2.125 is exactly what it says; 2.345 is not,
        // and asking what a number that cannot be written in binary rounds
        // to is asking about the arithmetic and not about the rounding.
        assert_eq!(called("Round", &[Value::Double(2.125), Value::Long(2)]), Value::Double(2.12));
        assert_eq!(called("Int", &[Value::Double(-2.5)]), Value::Double(-3.0));
        assert_eq!(called("Fix", &[Value::Double(-2.5)]), Value::Double(-2.0));
        assert_eq!(called("Val", &[text("3 apples")]), Value::Double(3.0));
        assert_eq!(called("Sgn", &[Value::Double(-7.0)]), Value::Long(-1));
    }

    #[test]
    fn null_goes_through_nearly_everything_and_is_asked_about_by_a_few() {
        assert!(called("Left", &[Value::Null, Value::Long(2)]).is_null());
        assert!(called("Round", &[Value::Null]).is_null());
        assert_eq!(called("IsNull", &[Value::Null]), Value::Boolean(true));
        assert_eq!(called("TypeName", &[Value::Null]), text("Null"));
    }

    #[test]
    fn dates_are_taken_apart_and_put_together() {
        let day = Value::Date(dates::serial(2000, 1, 1));
        assert_eq!(called("Year", &[day.clone()]), Value::Long(2000));
        assert_eq!(called("Month", &[day.clone()]), Value::Long(1));
        assert_eq!(called("Day", &[day.clone()]), Value::Long(1));
        assert_eq!(called("DateSerial", &[Value::Long(2000), Value::Long(1), Value::Long(1)]), day);
        assert_eq!(
            called("DateAdd", &[text("m"), Value::Long(1), day.clone()]),
            Value::Date(dates::serial(2000, 2, 1))
        );
        assert_eq!(
            called("DateDiff", &[text("d"), day, Value::Date(dates::serial(2000, 1, 11))]),
            Value::Long(10)
        );
    }

    #[test]
    fn format_writes_a_number_or_a_date_by_what_the_picture_says() {
        assert_eq!(called("Format", &[Value::Double(3.14159), text("0.00")]), text("3.14"));
        assert_eq!(called("Format", &[Value::Double(1234.5), text("#,##0.00")]), text("1,234.50"));
        assert_eq!(called("Format", &[Value::Double(0.256), text("0%")]), text("26%"));
        let day = Value::Date(dates::serial(2000, 1, 2) + dates::time_serial(18, 5, 0));
        assert_eq!(called("Format", &[day.clone(), text("yyyy-mm-dd")]), text("2000-01-02"));
        assert_eq!(called("Format", &[day.clone(), text("d mmmm yyyy")]), text("2 January 2000"));
        assert_eq!(called("Format", &[day.clone(), text("hh:nn")]), text("18:05"));
        assert_eq!(called("Format", &[day, text("h:nn AM/PM")]), text("6:05 PM"));
    }

    #[test]
    fn a_message_box_asks_the_program_and_not_the_language() {
        // Which is what lets a test run a macro that shows one, and what
        // will let the editor show a real one later.
        let mut host = Quiet::default();
        let answer = call("MsgBox", &[text("Saved"), Value::Long(0)], &mut host)
            .expect("a known name")
            .expect("an answer");
        assert_eq!(answer, Value::Long(1));
        assert_eq!(host.messages, vec!["Saved".to_owned()]);
    }

    #[test]
    fn a_name_that_is_not_the_librarys_is_left_for_the_macro() {
        let mut host = Quiet::default();
        assert!(call("MyOwnFunction", &[], &mut host).is_none());
        assert!(!knows("MyOwnFunction"));
        assert!(knows("left") && knows("Left") && knows("FORMAT"));
    }

    #[test]
    fn every_name_the_library_claims_is_one_it_answers_to() {
        // The list and the functions are two places saying the same thing,
        // and two places drift. This walks one against the other: a name on
        // the list that nothing implements would come back as "invalid
        // procedure call" from the arm that catches what is left.
        let mut host = Quiet::default();
        for name in NAMES {
            let answer = call(name, &[Value::Long(1), Value::Long(1), Value::Long(1)], &mut host)
                .unwrap_or_else(|| panic!("{name} is on the list and is not answered"));
            if let Err(fault) = answer {
                assert_ne!(fault.description, NOT_WRITTEN, "{name}");
            }
        }
    }
}
