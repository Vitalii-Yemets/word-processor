//! Writing a number, a length, a date and a sheet of paper the way this
//! machine's owner writes them.
//!
//! # Why not in the catalogue with the rest
//!
//! Because these are not messages. "1,5 cm" is not a translation of
//! "1.5 cm": it is the same sentence written the way the person's machine
//! says numbers are written, and the machine is where the answer is. A
//! person may well want a program in English on a machine set to German —
//! Word lets them, and so does this — and then the interface is English
//! and the numbers are still German. See [`crate::messages`] for the other
//! half.
//!
//! # What follows from it
//!
//! The unit lengths are shown in unless the person has said otherwise, the
//! paper a new document is made on, the mark between a number and its
//! fraction and between its thousands, and how a date is written.

use crate::measure::Unit;

/// What the machine says, asked once.
#[must_use]
pub fn current() -> &'static wp_shell::locale::Locale {
    wp_shell::locale::current()
}

/// The unit lengths are shown in where nobody has chosen one.
///
/// Word's Options starts on the machine's own measurement and lets it be
/// changed; what is changed is remembered, and this is only the starting
/// point.
#[must_use]
pub fn unit() -> Unit {
    if current().metric {
        Unit::Centimetres
    } else {
        Unit::Inches
    }
}

/// The paper a new document is made on, in twentieths of a point.
///
/// A4 everywhere but North America, which is what the machine says and
/// what Word does with it.
#[must_use]
pub fn paper() -> (i32, i32) {
    match current().paper {
        // 210 by 297 millimetres.
        wp_shell::locale::Paper::A4 => (11906, 16838),
        // Eight and a half by eleven inches.
        wp_shell::locale::Paper::Letter => (12240, 15840),
    }
}

/// A number with as many figures after the point as asked for, written the
/// way this machine writes one.
#[must_use]
pub fn number(value: f64, places: usize) -> String {
    let written = format!("{value:.places$}");
    let decimal = current().decimal;
    if decimal == '.' {
        return written;
    }
    written.replace('.', &decimal.to_string())
}

/// A count — of words, of characters, of pages — with its thousands
/// marked off, which is how every one of them is shown in Word's strip
/// along the bottom.
#[must_use]
pub fn count(value: usize) -> String {
    let digits = value.to_string();
    let Some(mark) = current().thousands else { return digits };
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            out.push(mark);
        }
        out.push(digit);
    }
    out
}

/// Today, written out: the month by name, in the order this machine writes
/// a date in.
///
/// What Word's Date and Time puts in a document when the long form is
/// chosen, which is the form it offers first.
#[must_use]
pub fn today() -> String {
    let (year, month, day) = civil_now();
    long_date(year, month, day)
}

/// A date written out, by this machine's rules.
#[must_use]
pub fn long_date(year: i64, month: u32, day: u32) -> String {
    let locale = current();
    let name = locale
        .months
        .get((month as usize).saturating_sub(1))
        .cloned()
        .unwrap_or_else(|| month.to_string());
    match locale.date_order {
        wp_shell::locale::DateOrder::MonthDayYear => format!("{name} {day}, {year}"),
        wp_shell::locale::DateOrder::DayMonthYear => format!("{day} {name} {year}"),
        wp_shell::locale::DateOrder::YearMonthDay => format!("{year} {name} {day}"),
    }
}

/// A date in figures, in this machine's order and with its own separator:
/// `16/09/2026`, `9/16/2026`, `2026-09-16`.
#[must_use]
pub fn short_date(year: i64, month: u32, day: u32) -> String {
    let locale = current();
    let mark = locale.date_separator;
    match locale.date_order {
        wp_shell::locale::DateOrder::MonthDayYear => {
            format!("{month:02}{mark}{day:02}{mark}{year:04}")
        }
        wp_shell::locale::DateOrder::DayMonthYear => {
            format!("{day:02}{mark}{month:02}{mark}{year:04}")
        }
        wp_shell::locale::DateOrder::YearMonthDay => {
            format!("{year:04}{mark}{month:02}{mark}{day:02}")
        }
    }
}

/// A time of day, on the clock this machine has: `14:22`, or `2:22 PM`.
#[must_use]
pub fn time_of_day(hour: u32, minute: u32) -> String {
    if current().twenty_four_hour {
        return format!("{hour:02}:{minute:02}");
    }
    let (shown, half) = match hour {
        0 => (12, "AM"),
        1..=11 => (hour, "AM"),
        12 => (12, "PM"),
        _ => (hour - 12, "PM"),
    };
    format!("{shown}:{minute:02} {half}")
}

/// A moment this program wrote down — the date and the time, as
/// `2026-09-16T11:22:33Z` — written the way this machine writes them.
///
/// What the Document Recovery pane shows under a file's name. The moment
/// is kept in the one form everything agrees on and shown in the
/// person's; the other way round would be a program whose own files could
/// not be read on a machine set differently.
#[must_use]
pub fn moment(stamp: &str) -> String {
    let Some((date, rest)) = stamp.split_once('T') else { return stamp.to_owned() };
    let numbers: Vec<i64> = date.split('-').filter_map(|part| part.parse().ok()).collect();
    let [year, month, day] = numbers.as_slice() else { return stamp.to_owned() };
    let clock: Vec<u32> =
        rest.trim_end_matches('Z').split(':').filter_map(|part| part.parse().ok()).collect();
    let (hour, minute) = match clock.as_slice() {
        [hour, minute, ..] => (*hour, *minute),
        _ => (0, 0),
    };
    format!("{} {}", short_date(*year, *month as u32, *day as u32), time_of_day(hour, minute))
}

/// Today's date, from the clock.
fn civil_now() -> (i64, u32, u32) {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    civil_from_days((seconds / 86_400) as i64)
}

/// The date a count of days since 1970 lands on.
///
/// The standard arithmetic: the era is shifted to start in March so that
/// the leap day is the last day of the year and no month needs a special
/// case.
#[must_use]
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 { shifted } else { shifted - 146_096 } / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * shifted_month + 2) / 5 + 1) as u32;
    let month = if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_count_has_its_thousands_marked_off() {
        // Whatever this machine's mark is, a count is grouped by threes.
        let counted = count(1_234_567);
        let figures: String = counted.chars().filter(char::is_ascii_digit).collect();
        assert_eq!(figures, "1234567");
        if current().thousands.is_some() {
            assert_eq!(counted.chars().filter(|c| !c.is_ascii_digit()).count(), 2);
        }
        assert_eq!(count(999), "999", "and a small one has none");
    }

    #[test]
    fn a_number_is_written_with_this_machines_own_point() {
        let written = number(1.5, 2);
        assert!(written.starts_with('1') && written.ends_with("50"));
        assert_eq!(written.len(), 4, "one figure, a mark, two figures: {written}");
    }

    #[test]
    fn a_date_comes_out_in_the_order_this_machine_writes_one() {
        let short = short_date(2026, 9, 16);
        assert!(short.contains("2026") && short.contains("09") && short.contains("16"));
        let long = long_date(2026, 9, 16);
        assert!(long.contains("2026") && long.contains("16"));
        assert!(long.contains(&current().months[8]), "the ninth month by name: {long}");
    }

    #[test]
    fn a_time_is_on_the_clock_this_machine_has() {
        let afternoon = time_of_day(14, 5);
        if current().twenty_four_hour {
            assert_eq!(afternoon, "14:05");
        } else {
            assert_eq!(afternoon, "2:05 PM");
        }
    }

    #[test]
    fn a_moment_written_down_is_shown_the_way_this_machine_shows_one() {
        let shown = moment("2026-09-16T11:22:33Z");
        assert!(shown.contains("16") && shown.contains("2026"), "the date: {shown}");
        assert!(shown.contains("11:22") || shown.contains("11:22 AM"), "and the time: {shown}");
        assert_eq!(moment("not a moment"), "not a moment");
    }

    #[test]
    fn the_days_arithmetic_lands_on_the_right_date() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_782), (2024, 2, 29), "a leap day");
        assert_eq!(civil_from_days(20_346), (2025, 9, 15));
    }
}
