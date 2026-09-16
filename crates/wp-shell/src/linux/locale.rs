//! What the machine says about numbers, lengths, dates and paper.
//!
//! # Why the C library is asked
//!
//! Because it is where the answer is. A Linux machine's locale data — the
//! decimal point, the names of the months, the order of a short date,
//! whether lengths are metric — lives in compiled files that `glibc` reads
//! and that nothing else can read portably. `setlocale` and `nl_langinfo`
//! are the documented way to ask, and they are the operating system's own
//! interface, declared here as the Windows shell declares Win32's.
//!
//! # What is not asked
//!
//! The paper. The locale does carry a paper size, but the C library hands
//! it back in a form that has changed between versions and is not there at
//! all in the plainest locale, so it is read from `/etc/papersize`, which
//! is where Debian and everything descended from it keeps the answer, and
//! worked out from the measurement where there is no such file. A
//! programme that guessed A4 in Ohio would be a programme that printed
//! wrongly.

use std::ffi::{c_char, c_int, CStr};

use crate::locale::{DateOrder, Locale, Paper};

extern "C" {
    fn setlocale(category: c_int, name: *const c_char) -> *mut c_char;
    fn nl_langinfo(item: c_int) -> *mut c_char;
    fn localeconv() -> *mut Lconv;
}

/// The front of `struct lconv`, which is all that is read here. The two
/// strings are the first two fields of it in every version of the C
/// library there has been.
#[repr(C)]
struct Lconv {
    decimal_point: *const c_char,
    thousands_sep: *const c_char,
}

/// `LC_ALL`, which is what a program sets when it takes the machine's own
/// settings.
const LC_ALL: c_int = 6;

/// What `nl_langinfo` calls the things this asks for.
///
/// The number is the category in the top half of the word and the place in
/// the list in the bottom: `LC_TIME` is 2, so the months, which are the
/// twenty-seventh thing in it, are `(2 << 16) | 26`. The categories are
/// the C library's own, and the numbers are checked by the tests below
/// against what the library actually answers.
const LC_TIME: c_int = 2;
const LC_PAPER_MEASUREMENT: c_int = 11;
const DAY_1: c_int = (LC_TIME << 16) | 7;
const MON_1: c_int = (LC_TIME << 16) | 26;
const D_FMT: c_int = (LC_TIME << 16) | 41;
const T_FMT: c_int = (LC_TIME << 16) | 42;
/// One byte: 1 for metric, 2 for the measurements of the United States.
const MEASUREMENT: c_int = LC_PAPER_MEASUREMENT << 16;

/// What the machine says.
pub(crate) fn locale() -> Locale {
    // Nothing is read until the program says it wants the machine's own
    // settings rather than the bare ones every C program starts with.
    // SAFETY: an empty name is the documented way to say "whatever the
    // environment says", and the answer is not kept.
    unsafe { setlocale(LC_ALL, c"".as_ptr()) };

    let mut locale = Locale { name: name(), ..Locale::default() };
    if let Some(byte) = first_byte(MEASUREMENT) {
        locale.metric = byte != 2;
    }
    if let Some((decimal, thousands)) = separators() {
        locale.decimal = decimal;
        locale.thousands = thousands;
    }
    if let Some(format) = text(D_FMT) {
        if let Some((order, separator)) = date_shape(&format) {
            locale.date_order = order;
            locale.date_separator = separator;
        }
    }
    if let Some(format) = text(T_FMT) {
        locale.twenty_four_hour = !format.contains("%p") && !format.contains("%r");
    }
    let months: Vec<String> = (0..12).filter_map(|month| text(MON_1 + month)).collect();
    if months.len() == 12 {
        locale.months = months;
    }
    let days: Vec<String> = (0..7)
        // The C library starts its week on Sunday; this program, like the
        // rest of the program, starts it on Monday.
        .filter_map(|day| text(DAY_1 + (day + 1) % 7))
        .collect();
    if days.len() == 7 {
        locale.days = days;
    }
    locale.paper = paper(locale.metric);
    locale
}

/// What the place is called, as the environment says it: `de_DE.UTF-8`
/// becomes `de-DE`.
fn name() -> String {
    for variable in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        let Ok(value) = std::env::var(variable) else { continue };
        let value = value.split('.').next().unwrap_or_default();
        if value.is_empty() || value == "C" || value == "POSIX" {
            continue;
        }
        return value.replace('_', "-");
    }
    String::new()
}

/// The paper this machine prints on.
fn paper(metric: bool) -> Paper {
    if let Ok(said) = std::fs::read_to_string("/etc/papersize") {
        for line in said.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            return if line.eq_ignore_ascii_case("letter") { Paper::Letter } else { Paper::A4 };
        }
    }
    if metric {
        Paper::A4
    } else {
        Paper::Letter
    }
}

/// The two marks a number is written with.
fn separators() -> Option<(char, Option<char>)> {
    // SAFETY: the C library owns the structure and keeps it until the
    // locale is changed, which this program does not do again.
    let conv = unsafe { localeconv() };
    if conv.is_null() {
        return None;
    }
    // SAFETY: the pointer came from the call above and is not null.
    let conv = unsafe { &*conv };
    let decimal = one_character(conv.decimal_point)?;
    let thousands = one_character(conv.thousands_sep);
    Some((decimal, thousands))
}

/// The first character of one of the C library's strings.
fn one_character(text: *const c_char) -> Option<char> {
    if text.is_null() {
        return None;
    }
    // SAFETY: the C library's strings are null-terminated and live as long
    // as the locale does.
    let text = unsafe { CStr::from_ptr(text) };
    text.to_str().ok()?.chars().next()
}

/// One of the things `nl_langinfo` knows, as text.
fn text(item: c_int) -> Option<String> {
    // SAFETY: the item is one of the documented ones and the answer is a
    // null-terminated string the C library owns.
    let answer = unsafe { nl_langinfo(item) };
    if answer.is_null() {
        return None;
    }
    // SAFETY: as above.
    let text = unsafe { CStr::from_ptr(answer) }.to_str().ok()?;
    (!text.is_empty()).then(|| text.to_owned())
}

/// The same, for the things the library answers with a single byte rather
/// than with a string.
fn first_byte(item: c_int) -> Option<u8> {
    // SAFETY: as above.
    let answer = unsafe { nl_langinfo(item) };
    if answer.is_null() {
        return None;
    }
    // SAFETY: the answer is at least one byte long, which is what this
    // kind of item is.
    Some(unsafe { *answer } as u8)
}

/// Which way round a short date goes, and what it is written with.
///
/// `%m/%d/%y` is the American order, `%d.%m.%Y` the German one, `%Y-%m-%d`
/// the one the rest of the world agreed on in a standard.
pub(crate) fn date_shape(format: &str) -> Option<(DateOrder, char)> {
    let mut parts = Vec::new();
    let mut separator = None;
    let mut characters = format.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '%' {
            match characters.next() {
                Some('d') | Some('e') => parts.push('d'),
                Some('m') => parts.push('m'),
                Some('y') | Some('Y') => parts.push('y'),
                _ => {}
            }
            continue;
        }
        if separator.is_none() && !character.is_whitespace() && !parts.is_empty() {
            separator = Some(character);
        }
    }
    let order = match parts.as_slice() {
        ['d', 'm', ..] => DateOrder::DayMonthYear,
        ['m', 'd', ..] => DateOrder::MonthDayYear,
        ['y', ..] => DateOrder::YearMonthDay,
        _ => return None,
    };
    Some((order, separator.unwrap_or('/')))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_date_says_which_way_round_it_goes() {
        assert_eq!(date_shape("%m/%d/%y"), Some((DateOrder::MonthDayYear, '/')));
        assert_eq!(date_shape("%d.%m.%Y"), Some((DateOrder::DayMonthYear, '.')));
        assert_eq!(date_shape("%Y-%m-%d"), Some((DateOrder::YearMonthDay, '-')));
        assert_eq!(date_shape("%d/%m/%y"), Some((DateOrder::DayMonthYear, '/')));
        assert_eq!(date_shape("nothing of the kind"), None);
    }

    /// Runs the work as though on a machine set to that place, and puts
    /// the machine back afterwards.
    ///
    /// Nothing at all where the machine has not got that place's files:
    /// the C library would answer with the plain locale, which is not what
    /// is being asked about. The environment and the C library's own idea
    /// of where it is both belong to the whole program, so these go one at
    /// a time.
    fn in_place<R>(place: &str, work: impl FnOnce(&Locale) -> R) -> Option<R> {
        static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _held = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let was = std::env::var("LC_ALL").ok();
        std::env::set_var("LC_ALL", place);
        let read = locale();
        let wanted = place.split('.').next().unwrap_or_default().replace('_', "-");
        let out = (read.name == wanted).then(|| work(&read));
        match was {
            Some(value) => std::env::set_var("LC_ALL", value),
            None => std::env::remove_var("LC_ALL"),
        }
        // And the C library is told to read the environment again, or the
        // next test would find this one's country.
        // SAFETY: as in `locale`.
        unsafe { setlocale(LC_ALL, c"".as_ptr()) };
        out
    }

    #[test]
    fn a_german_machine_says_centimetres_a_comma_and_the_day_first() {
        let read = in_place("de_DE.UTF-8", |german| {
            assert!(german.metric, "lengths are metric");
            assert_eq!(german.decimal, ',', "and a comma stands before the fraction");
            assert_eq!(german.date_order, DateOrder::DayMonthYear);
            assert_eq!(german.date_separator, '.');
            assert_eq!(german.months[0], "Januar");
            assert_eq!(german.days[0], "Montag", "the week starts on Monday here");
            assert!(german.twenty_four_hour, "and the clock has twenty-four hours");
            assert_eq!(german.paper, Paper::A4);
        });
        if read.is_none() {
            eprintln!("skipped: this machine has no German locale");
        }
    }

    #[test]
    fn an_american_machine_says_inches_a_point_and_the_month_first() {
        let read = in_place("en_US.UTF-8", |american| {
            assert!(!american.metric, "lengths are not metric");
            assert_eq!(american.decimal, '.');
            assert_eq!(american.date_order, DateOrder::MonthDayYear);
            assert_eq!(american.months[0], "January");
            assert_eq!(american.days[0], "Monday");
            assert!(!american.twenty_four_hour, "and the clock has twelve hours on it");
            assert_eq!(american.paper, Paper::Letter);
            // The item numbers are worked out rather than included from a
            // header, so this holds them to what the library actually
            // answers, in a place whose answers are known.
            assert_eq!(text(MON_1).as_deref(), Some("January"));
            assert_eq!(text(DAY_1 + 1).as_deref(), Some("Monday"), "the day after Sunday");
            assert!(text(D_FMT).is_some_and(|format| format.contains('%')));
        });
        if read.is_none() {
            eprintln!("skipped: this machine has no American locale");
        }
    }

    #[test]
    fn the_machine_answers_about_itself() {
        // Whatever this machine is set to, the answers have to be answers:
        // twelve months, seven days, a decimal point that is a character.
        let locale = locale();
        assert_eq!(locale.months.len(), 12);
        assert_eq!(locale.days.len(), 7);
        assert!(!locale.months[0].is_empty(), "January is called something");
        assert!(!locale.days[0].is_empty(), "and so is Monday");
        assert!(
            locale.decimal == '.' || locale.decimal == ',',
            "a decimal point is one or the other: {:?}",
            locale.decimal
        );
    }
}
