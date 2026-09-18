//! Dates, counted the way Visual Basic counts them.
//!
//! A date is one number: the whole part is days since the thirtieth of
//! December 1899, and the fraction is the time of day. That is why adding one
//! to a date is tomorrow and why half past six in the morning is a quarter of
//! a day. The thirtieth of December is not a misprint: the count was made to
//! agree with a spreadsheet that believed 1900 was a leap year, and every
//! program that reads those files has counted from there ever since.
//!
//! Written out, a date takes the form the machine's own language uses. This
//! program says English as the United Kingdom writes it — see the status bar
//! — so `dd/mm/yyyy`, and a time of day as `hh:mm:ss`. A date with no time is
//! written without one, and a time with no date likewise, which is what
//! Visual Basic does and what a macro joining one into a sentence expects.

/// The day the count starts from, as days from the civil epoch.
const EPOCH: i64 = -25_569;

/// The year, month and day a serial number lands on.
#[must_use]
pub fn parts(serial: f64) -> (i64, u32, u32) {
    civil_from_days(serial.floor() as i64 + EPOCH)
}

/// The hour, minute and second.
#[must_use]
pub fn time_parts(serial: f64) -> (u32, u32, u32) {
    let fraction = serial - serial.floor();
    // To the nearest second: a date made from an hour and a minute is a
    // fraction that does not land exactly on a second, and nobody wants
    // half past six written as twenty-nine minutes and sixty seconds.
    let seconds = (fraction * 86_400.0).round().max(0.0) as i64 % 86_400;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    ((seconds / 3600) as u32, (seconds % 3600 / 60) as u32, (seconds % 60) as u32)
}

/// The serial number of a year, month and day.
///
/// A month outside one to twelve rolls into the next year, and a day outside
/// the month rolls into the next month, which is what `DateSerial` does and
/// what makes `DateSerial(y, m + 1, 0)` the last day of a month.
#[must_use]
pub fn serial(year: i64, month: i64, day: i64) -> f64 {
    let (year, month) = (year + (month - 1).div_euclid(12), (month - 1).rem_euclid(12) + 1);
    #[allow(clippy::cast_precision_loss)]
    let days = days_from_civil(year, month as u32, 1) + day - 1 - EPOCH;
    days as f64
}

/// The time of day as a fraction of one.
#[must_use]
pub fn time_serial(hour: i64, minute: i64, second: i64) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    let seconds = (hour * 3600 + minute * 60 + second) as f64;
    seconds / 86_400.0
}

/// A date written out, as `CStr` writes one.
#[must_use]
pub fn written(serial: f64) -> String {
    let day = serial.floor();
    let time = serial - day;
    let (year, month, date) = parts(serial);
    let (hour, minute, second) = time_parts(serial);

    if time == 0.0 {
        return format!("{date:02}/{month:02}/{year:04}");
    }
    if day == 0.0 {
        return format!("{hour:02}:{minute:02}:{second:02}");
    }
    format!("{date:02}/{month:02}/{year:04} {hour:02}:{minute:02}:{second:02}")
}

/// The date a string holds, if it holds one.
///
/// What `CDate` reads: a date the way it is written here, a date the way the
/// language writes one between hashes, and a time on its own. Month names are
/// read as well, because a macro written by somebody who typed `#1 Jan 2000#`
/// has them in its source.
#[must_use]
pub fn from_text(text: &str) -> Option<f64> {
    let trimmed = text.trim().trim_matches('#').trim();
    if trimmed.is_empty() {
        return None;
    }
    let (date_part, time_part) = match trimmed.split_once(' ') {
        Some((left, right)) if right.contains(':') => (left, Some(right)),
        _ if trimmed.contains(':') && !trimmed.contains('/') && !trimmed.contains('-') => {
            ("", Some(trimmed))
        }
        _ => (trimmed, None),
    };

    let mut serial = 0.0;
    if !date_part.is_empty() {
        serial += date_serial_of(date_part)?;
    }
    if let Some(time) = time_part {
        serial += time_serial_of(time)?;
    }
    Some(serial)
}

/// The date part of a string: `1/1/2000`, `2000-01-01`, `1 Jan 2000`.
fn date_serial_of(text: &str) -> Option<f64> {
    let pieces: Vec<&str> =
        text.split(['/', '-', ' ', '.']).filter(|piece| !piece.is_empty()).collect();
    if pieces.len() != 3 {
        return None;
    }
    // A four-digit first piece is a year, which is how a date written the
    // international way is told from one written the ordinary way.
    if pieces[0].len() == 4 {
        let year: i64 = pieces[0].parse().ok()?;
        let month = month_of(pieces[1])?;
        let day: i64 = pieces[2].parse().ok()?;
        return Some(serial(year, month, day));
    }
    let day: i64 = pieces[0].parse().ok()?;
    let month = month_of(pieces[1])?;
    let mut year: i64 = pieces[2].parse().ok()?;
    // Two digits mean this century up to twenty-nine and the last one after,
    // which is the rule Visual Basic uses.
    if pieces[2].len() <= 2 {
        year += if year < 30 { 2000 } else { 1900 };
    }
    Some(serial(year, month, day))
}

/// A month, as a number or as a name.
fn month_of(text: &str) -> Option<i64> {
    if let Ok(number) = text.parse::<i64>() {
        return (1..=12).contains(&number).then_some(number);
    }
    const NAMES: [&str; 12] = [
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
    ];
    let lowered = text.to_lowercase();
    #[allow(clippy::cast_possible_wrap)]
    NAMES
        .iter()
        .position(|name| *name == lowered || name.starts_with(&lowered) && lowered.len() >= 3)
        .map(|at| at as i64 + 1)
}

/// The time part of a string: `18:30`, `18:30:05`, `6:30:05 PM`.
fn time_serial_of(text: &str) -> Option<f64> {
    let mut rest = text.trim();
    let mut after_noon = None;
    for (suffix, afternoon) in [("PM", true), ("AM", false), ("pm", true), ("am", false)] {
        if let Some(head) = rest.strip_suffix(suffix) {
            rest = head.trim();
            after_noon = Some(afternoon);
            break;
        }
    }

    let pieces: Vec<&str> = rest.split(':').collect();
    if pieces.is_empty() || pieces.len() > 3 {
        return None;
    }
    let mut hour: i64 = pieces[0].trim().parse().ok()?;
    let minute: i64 = pieces.get(1).map_or(Ok(0), |piece| piece.trim().parse()).ok()?;
    let second: i64 = pieces.get(2).map_or(Ok(0), |piece| piece.trim().parse()).ok()?;
    match after_noon {
        Some(true) if hour < 12 => hour += 12,
        Some(false) if hour == 12 => hour = 0,
        _ => {}
    }
    if !(0..24).contains(&hour) || !(0..60).contains(&minute) || !(0..60).contains(&second) {
        return None;
    }
    Some(time_serial(hour, minute, second))
}

/// Which day of the week a date is, Sunday being one, as `Weekday` says.
#[must_use]
pub fn weekday(serial: f64) -> i64 {
    let days = serial.floor() as i64 + EPOCH;
    // The civil epoch was a Thursday.
    (days + 4).rem_euclid(7) + 1
}

/// The calendar arithmetic, which is the same everywhere and is written out
/// rather than asked of a library this program does not have.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let of_era = shifted.rem_euclid(146_097);
    let year_of_era = (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * of_year + 2) / 153;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let day = (of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let month = if month_prime < 10 { month_prime + 3 } else { month_prime - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// And the other way about.
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let of_year =
        (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + i64::from(day) - 1;
    let of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + of_year;
    era * 146_097 + of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_count_starts_where_the_spreadsheet_said_it_did() {
        // Serial one is the last day of 1899, because of a leap year that
        // never happened. Every program that reads those files counts from
        // there, and a macro comparing dates with one that does not would be
        // a day out for ever.
        assert_eq!(parts(1.0), (1899, 12, 31));
        assert_eq!(parts(2.0), (1900, 1, 1));
        assert_eq!(serial(1900, 1, 1), 2.0);
        assert_eq!(serial(2000, 1, 1), 36_526.0);
        assert_eq!(parts(36_526.0), (2000, 1, 1));
    }

    #[test]
    fn a_month_outside_the_year_rolls_over_and_day_nought_is_the_month_before() {
        assert_eq!(serial(2000, 13, 1), serial(2001, 1, 1));
        assert_eq!(serial(2000, 0, 1), serial(1999, 12, 1));
        // Which is how a macro asks for the last day of a month.
        assert_eq!(parts(serial(2000, 3, 0)), (2000, 2, 29));
        assert_eq!(parts(serial(1900, 3, 0)), (1900, 2, 28), "1900 was not a leap year");
    }

    #[test]
    fn the_fraction_is_the_time_of_day() {
        assert_eq!(time_serial(6, 0, 0), 0.25);
        assert_eq!(time_parts(0.25), (6, 0, 0));
        assert_eq!(time_parts(0.5), (12, 0, 0));
        // And a time that does not land on a whole second is written as the
        // second it is nearest.
        assert_eq!(time_parts(time_serial(18, 30, 5)), (18, 30, 5));
    }

    #[test]
    fn a_date_is_written_the_way_this_machine_writes_one() {
        assert_eq!(written(36_526.0), "01/01/2000");
        assert_eq!(written(0.25), "06:00:00");
        assert_eq!(written(36_526.25), "01/01/2000 06:00:00");
    }

    #[test]
    fn a_date_is_read_however_it_was_written() {
        assert_eq!(from_text("1/1/2000"), Some(36_526.0));
        assert_eq!(from_text("#1/1/2000#"), Some(36_526.0));
        assert_eq!(from_text("2000-01-01"), Some(36_526.0));
        assert_eq!(from_text("1 Jan 2000"), Some(36_526.0));
        assert_eq!(from_text("1/1/2000 06:00"), Some(36_526.25));
        assert_eq!(from_text("06:00"), Some(0.25));
        assert_eq!(from_text("6:00 PM"), Some(0.75));
        assert_eq!(from_text("not a date"), None);
    }

    #[test]
    fn two_digit_years_belong_to_this_century_up_to_twenty_nine() {
        assert_eq!(parts(from_text("1/1/29").expect("a date")), (2029, 1, 1));
        assert_eq!(parts(from_text("1/1/30").expect("a date")), (1930, 1, 1));
    }

    #[test]
    fn the_day_of_the_week_counts_from_sunday() {
        // The first of January 2000 was a Saturday, which is the seventh.
        assert_eq!(weekday(36_526.0), 7);
        assert_eq!(weekday(36_527.0), 1);
    }
}
