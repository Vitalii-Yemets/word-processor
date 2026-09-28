//! Word's additional actions: what a stretch of text is recognised as, and
//! what can be done with it from the right-click menu.
//!
//! # Which of Word's
//!
//! Word's Actions tab lists what it recognises — a date, a measurement, a
//! telephone number, a stock symbol, a person's name — and each recognised
//! thing is offered to something else: the date to the calendar, the number
//! to the address book, the stock symbol to a web page of prices. Every one
//! of those but one leaves the document for another program or for the
//! internet, and this program talks to nobody.
//!
//! The one that does not is the Measurement Converter, which is here: `5
//! inches` is offered as `12.7 cm`, and the other way about, for lengths,
//! weights, volumes, speeds and temperatures. And a date is recognised too,
//! with what can be done to a date without a calendar: writing it another of
//! the ways the date can be written.
//!
//! The recognising is here, a question about text and nothing else; putting
//! the answer in is the editor's.

/// What a stretch of text was recognised as.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Recognised {
    /// A number with a unit after it, and the decimal mark it was written
    /// with, where it was written with one.
    Measurement { value: f64, unit: Unit, mark: Option<char> },
    /// A day.
    Date { year: i64, month: u32, day: u32 },
}

/// Something recognised, and where.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Found {
    /// Byte offsets into the text.
    pub start: usize,
    pub end: usize,
    pub what: Recognised,
}

/// One thing that can be done with it: what the menu says, and what goes in
/// its place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    pub label: String,
    pub putting: String,
}

/// Which of the recognisers are on: Word's Actions tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Recognisers {
    /// "Enable additional actions in the right-click menu": everything,
    /// which Word ships switched off.
    pub enabled: bool,
    pub dates: bool,
    pub measurements: bool,
}

impl Default for Recognisers {
    fn default() -> Self {
        Self { enabled: false, dates: true, measurements: true }
    }
}

/// What a unit measures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dimension {
    Length,
    Mass,
    Volume,
    Speed,
    Temperature,
}

/// A unit a measurement may be written in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Unit {
    /// How it is written in what is offered.
    pub symbol: &'static str,
    pub dimension: Dimension,
    /// How many of the dimension's base unit — the metre, the kilogram, the
    /// litre, the metre a second — one of it is. Unused for temperature.
    pub size: f64,
    /// Whether it is the metric system's rather than the American one.
    pub metric: bool,
}

const fn unit(symbol: &'static str, dimension: Dimension, size: f64, metric: bool) -> Unit {
    Unit { symbol, dimension, size, metric }
}

const MM: Unit = unit("mm", Dimension::Length, 0.001, true);
const CM: Unit = unit("cm", Dimension::Length, 0.01, true);
const M: Unit = unit("m", Dimension::Length, 1.0, true);
const KM: Unit = unit("km", Dimension::Length, 1000.0, true);
const IN: Unit = unit("in", Dimension::Length, 0.0254, false);
const FT: Unit = unit("ft", Dimension::Length, 0.3048, false);
const YD: Unit = unit("yd", Dimension::Length, 0.9144, false);
const MI: Unit = unit("mi", Dimension::Length, 1609.344, false);
const G: Unit = unit("g", Dimension::Mass, 0.001, true);
const KG: Unit = unit("kg", Dimension::Mass, 1.0, true);
const OZ: Unit = unit("oz", Dimension::Mass, 0.028_349_523_125, false);
const LB: Unit = unit("lb", Dimension::Mass, 0.453_592_37, false);
const ML: Unit = unit("ml", Dimension::Volume, 0.001, true);
const L: Unit = unit("l", Dimension::Volume, 1.0, true);
const FL_OZ: Unit = unit("fl oz", Dimension::Volume, 0.029_573_529_562_5, false);
const GAL: Unit = unit("gal", Dimension::Volume, 3.785_411_784, false);
const KMH: Unit = unit("km/h", Dimension::Speed, 1.0 / 3.6, true);
const MPH: Unit = unit("mph", Dimension::Speed, 0.447_04, false);
const CELSIUS: Unit = unit("\u{B0}C", Dimension::Temperature, 1.0, true);
const FAHRENHEIT: Unit = unit("\u{B0}F", Dimension::Temperature, 1.0, false);

/// Every unit, in the order conversions are offered in.
const UNITS: &[Unit] = &[
    MM, CM, M, KM, IN, FT, YD, MI, G, KG, OZ, LB, ML, L, FL_OZ, GAL, KMH, MPH, CELSIUS, FAHRENHEIT,
];

/// The ways each is written after a number. Longer first, so that `inches`
/// is not read as `in` and a stray `ches`.
const WRITTEN: &[(&str, Unit)] = &[
    ("millimetres", MM),
    ("millimeters", MM),
    ("millimetre", MM),
    ("millimeter", MM),
    ("centimetres", CM),
    ("centimeters", CM),
    ("centimetre", CM),
    ("centimeter", CM),
    ("kilometres", KM),
    ("kilometers", KM),
    ("kilometre", KM),
    ("kilometer", KM),
    ("metres", M),
    ("meters", M),
    ("metre", M),
    ("meter", M),
    ("inches", IN),
    ("inch", IN),
    ("feet", FT),
    ("foot", FT),
    ("yards", YD),
    ("yard", YD),
    ("miles", MI),
    ("mile", MI),
    ("kilograms", KG),
    ("kilogram", KG),
    ("grams", G),
    ("gram", G),
    ("ounces", OZ),
    ("ounce", OZ),
    ("pounds", LB),
    ("pound", LB),
    ("millilitres", ML),
    ("milliliters", ML),
    ("litres", L),
    ("liters", L),
    ("litre", L),
    ("liter", L),
    ("gallons", GAL),
    ("gallon", GAL),
    ("fl oz", FL_OZ),
    ("km/h", KMH),
    ("kph", KMH),
    ("mph", MPH),
    ("\u{B0}C", CELSIUS),
    ("\u{B0} C", CELSIUS),
    ("\u{2103}", CELSIUS),
    ("\u{B0}F", FAHRENHEIT),
    ("\u{B0} F", FAHRENHEIT),
    ("\u{2109}", FAHRENHEIT),
    ("mm", MM),
    ("cm", CM),
    ("km", KM),
    ("kg", KG),
    ("lbs", LB),
    ("lb", LB),
    ("oz", OZ),
    ("ml", ML),
    ("mL", ML),
    ("gal", GAL),
    ("in", IN),
    ("ft", FT),
    ("yd", YD),
    ("mi", MI),
    ("m", M),
    ("g", G),
    ("l", L),
    ("L", L),
];

/// The months by name, in English, which a date may be written with
/// whatever the machine's own language.
const MONTHS: [&str; 12] = [
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

impl Recognisers {
    /// What is recognised at a place in a paragraph, where anything is and
    /// the tab says to look for it.
    #[must_use]
    pub fn at(&self, text: &str, offset: usize, months: &[String], day_first: bool) -> Vec<Found> {
        if !self.enabled {
            return Vec::new();
        }
        let mut out = Vec::new();
        if self.dates {
            out.extend(dates_in(text, months, day_first));
        }
        if self.measurements {
            out.extend(measurements_in(text));
        }
        out.retain(|found| found.start <= offset && offset <= found.end);
        out
    }
}

/// Every measurement in a text: a number, perhaps a space, and a unit that
/// ends where a word ends.
#[must_use]
pub fn measurements_in(text: &str) -> Vec<Found> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let starts_number = bytes[at].is_ascii_digit()
            && (at == 0
                || !(bytes[at - 1].is_ascii_alphanumeric()
                    || matches!(bytes[at - 1], b'.' | b',')));
        if !starts_number {
            at += 1;
            continue;
        }
        let start = at;
        while at < bytes.len() && bytes[at].is_ascii_digit() {
            at += 1;
        }
        let mut mark = None;
        if at + 1 < bytes.len()
            && matches!(bytes[at], b'.' | b',')
            && bytes[at + 1].is_ascii_digit()
        {
            mark = Some(char::from(bytes[at]));
            at += 1;
            while at < bytes.len() && bytes[at].is_ascii_digit() {
                at += 1;
            }
        }
        let number = &text[start..at];
        let Ok(value) = number.replace(',', ".").parse::<f64>() else { continue };

        // One space, or none, and then the unit — which has to end where a
        // word does, or "5 more" would be five metres and "ore".
        let after = &text[at..];
        let (gap, rest) = match after.strip_prefix([' ', '\u{A0}']) {
            Some(rest) => (after.len() - rest.len(), rest),
            None => (0, after),
        };
        let unit = WRITTEN.iter().find(|(written, unit)| {
            let tail = rest.get(written.len()..).unwrap_or_default();
            // "In" is inches only where no word follows it: "5 in." and "5
            // in," are a measurement, "3 in a row" is not.
            let a_word_follows = *unit == IN
                && *written == "in"
                && tail.trim_start().starts_with(|c: char| c.is_alphabetic());
            rest.starts_with(written)
                && !tail.starts_with(|c: char| c.is_alphanumeric())
                && !a_word_follows
        });
        if let Some((written, unit)) = unit {
            let end = at + gap + written.len();
            out.push(Found {
                start,
                end,
                what: Recognised::Measurement { value, unit: *unit, mark },
            });
            at = end;
        }
    }
    out
}

/// Every date in a text: `2026-09-28`, `28 September 2026`, `September 28,
/// 2026`, `28/09/2026` and `28.09.2026` — the last two read day first or
/// month first as the machine writes its dates.
#[must_use]
pub fn dates_in(text: &str, months: &[String], day_first: bool) -> Vec<Found> {
    let mut out = Vec::new();
    // The words of the text with where each is, a word being letters or
    // figures, and the punctuation between them kept apart.
    let mut words: Vec<(usize, usize)> = Vec::new();
    let mut start: Option<usize> = None;
    for (at, c) in text.char_indices().chain([(text.len(), ' ')]) {
        match (c.is_alphanumeric(), start) {
            (true, None) => start = Some(at),
            (false, Some(from)) => {
                words.push((from, at));
                start = None;
            }
            _ => {}
        }
    }
    let month_named = |word: &str| -> Option<u32> {
        let lower = word.to_lowercase();
        let english = MONTHS.iter().position(|name| {
            *name == lower || (lower.len() >= 3 && name.starts_with(&lower) && lower.len() <= 4)
        });
        let own = months.iter().position(|name| name.to_lowercase() == lower);
        english.or(own).map(|index| index as u32 + 1)
    };
    let number = |word: &str| -> Option<i64> {
        (word.len() <= 4 && word.chars().all(|c| c.is_ascii_digit())).then(|| word.parse().ok())?
    };
    // Only one sort of mark joins the parts of a date in figures.
    let joined_by = |from: usize, to: usize, marks: &[char]| -> Option<char> {
        let between = &text[from..to];
        let mark = between.chars().next()?;
        (between.chars().count() == 1 && marks.contains(&mark)).then_some(mark)
    };

    for index in 0..words.len() {
        let (a_start, a_end) = words[index];
        let a = &text[a_start..a_end];
        let (Some(&(b_start, b_end)), Some(&(c_start, c_end))) =
            (words.get(index + 1), words.get(index + 2))
        else {
            continue;
        };
        let (b, c) = (&text[b_start..b_end], &text[c_start..c_end]);

        let mut found = None;
        // In figures: 2026-09-28, or 28/09/2026 and 28.09.2026.
        if let (Some(x), Some(y), Some(z)) = (number(a), number(b), number(c)) {
            let first = joined_by(a_end, b_start, &['-', '/', '.']);
            let second = joined_by(b_end, c_start, &['-', '/', '.']);
            if first.is_some() && first == second {
                found = if a.len() == 4 && first == Some('-') {
                    Some((x, y, z))
                } else if c.len() == 4 && first != Some('-') {
                    Some(if day_first { (z, y, x) } else { (z, x, y) })
                } else {
                    None
                };
            }
        }
        // By name: 28 September 2026, 28. September 2026, September 28, 2026.
        if found.is_none() {
            let gap =
                |from: usize, to: usize| text[from..to].trim_matches([' ', '.', ',']).is_empty();
            if let (Some(day), Some(month), Some(year)) = (number(a), month_named(b), number(c)) {
                if gap(a_end, b_start) && gap(b_end, c_start) && c.len() == 4 {
                    found = Some((year, i64::from(month), day));
                }
            } else if let (Some(month), Some(day), Some(year)) =
                (month_named(a), number(b), number(c))
            {
                if gap(a_end, b_start) && gap(b_end, c_start) && c.len() == 4 {
                    found = Some((year, i64::from(month), day));
                }
            }
        }
        let Some((year, month, day)) = found else { continue };
        if !(1..=12).contains(&month) || day < 1 || day > days_in(year, month as u32) as i64 {
            continue;
        }
        out.push(Found {
            start: a_start,
            end: c_end,
            what: Recognised::Date { year, month: month as u32, day: day as u32 },
        });
    }
    out
}

/// How many days a month has.
#[must_use]
pub fn days_in(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        _ => 28,
    }
}

/// What a measurement is in the other system's units: the one or two that
/// give a number a person would write — neither a fraction of a thousandth
/// nor a number in the tens of thousands — nearest to one first.
#[must_use]
pub fn conversions(value: f64, unit: Unit, comma: bool) -> Vec<Offer> {
    if unit.dimension == Dimension::Temperature {
        let (converted, into) = if unit.metric {
            (value * 9.0 / 5.0 + 32.0, FAHRENHEIT)
        } else {
            ((value - 32.0) * 5.0 / 9.0, CELSIUS)
        };
        return vec![offer(converted, into, comma, 3)];
    }
    let base = value * unit.size;
    let mut candidates: Vec<(f64, Unit)> = UNITS
        .iter()
        .filter(|other| other.dimension == unit.dimension && other.metric != unit.metric)
        .map(|other| (base / other.size, *other))
        .filter(|(amount, _)| (0.01..100_000.0).contains(amount))
        .collect();
    candidates.sort_by(|(one, _), (two, _)| {
        let distance = |amount: f64| (amount.log10() - 1.0).abs();
        distance(*one).total_cmp(&distance(*two))
    });
    candidates.into_iter().take(2).map(|(amount, into)| offer(amount, into, comma, 3)).collect()
}

/// A converted amount, as the menu offers it.
fn offer(amount: f64, into: Unit, comma: bool, figures: usize) -> Offer {
    let written = format!("{} {}", tidy(amount, figures, comma), into.symbol);
    Offer { label: crate::messages::with("Convert to {0}", &[&written]), putting: written }
}

/// An amount to as many significant figures as asked, with no zeros left
/// dangling after the point, and the decimal mark the writer used.
#[must_use]
pub fn tidy(amount: f64, figures: usize, comma: bool) -> String {
    let magnitude = if amount == 0.0 { 0 } else { amount.abs().log10().floor() as i32 };
    let places = (figures as i32 - 1 - magnitude).clamp(0, 6) as usize;
    let mut out = format!("{amount:.places$}");
    if out.contains('.') {
        out = out.trim_end_matches('0').trim_end_matches('.').to_owned();
    }
    if out == "-0" {
        out = "0".to_owned();
    }
    if comma {
        out = out.replace('.', ",");
    }
    out
}

/// The other ways a date can be written, each as a line of the menu: the
/// machine's long form and its short one, and the international one.
#[must_use]
pub fn date_forms(year: i64, month: u32, day: u32, written: &str) -> Vec<Offer> {
    let forms = [
        crate::locale::long_date(year, month, day),
        crate::locale::short_date(year, month, day),
        format!("{year:04}-{month:02}-{day:02}"),
    ];
    let mut out: Vec<Offer> = Vec::new();
    for form in forms {
        if form != written && !out.iter().any(|offer| offer.putting == form) {
            out.push(Offer {
                label: crate::messages::with("Change to {0}", &[&form]),
                putting: form,
            });
        }
    }
    out
}

/// What can be done with something recognised. `comma` is whether the
/// machine writes a decimal comma, which is what a converted amount gets
/// where the amount it came from was written with no decimal mark to follow.
#[must_use]
pub fn offers(found: &Found, written: &str, comma: bool) -> Vec<Offer> {
    match found.what {
        Recognised::Measurement { value, unit, mark } => {
            conversions(value, unit, mark.map_or(comma, |mark| mark == ','))
        }
        Recognised::Date { year, month, day } => date_forms(year, month, day, written),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measured(text: &str) -> Vec<(String, String)> {
        measurements_in(text)
            .into_iter()
            .map(|found| {
                let Recognised::Measurement { unit, .. } = found.what else { panic!() };
                (text[found.start..found.end].to_owned(), unit.symbol.to_owned())
            })
            .collect()
    }

    #[test]
    fn a_number_with_a_unit_after_it_is_a_measurement() {
        assert_eq!(
            measured("a 5 inch pipe, 12.5 cm long, 3kg and 72\u{B0}F"),
            vec![
                ("5 inch".to_owned(), "in".to_owned()),
                ("12.5 cm".to_owned(), "cm".to_owned()),
                ("3kg".to_owned(), "kg".to_owned()),
                ("72\u{B0}F".to_owned(), "\u{B0}F".to_owned()),
            ]
        );
    }

    #[test]
    fn a_unit_has_to_end_where_a_word_does() {
        // "5 more" is not five metres of "ore", "10 items" not ten inches,
        // and a number inside a word is no measurement.
        assert_eq!(measured("5 more and 10 items, A4 lg, x2m"), vec![]);
        assert_eq!(measured("7 miles"), vec![("7 miles".to_owned(), "mi".to_owned())]);
        // And "in" with a word after it is where something is.
        assert_eq!(measured("3 in a row"), vec![]);
        assert_eq!(measured("it is 3 in."), vec![("3 in".to_owned(), "in".to_owned())]);
    }

    fn converted(text: &str) -> Vec<String> {
        let found = measurements_in(text)[0];
        offers(&found, text, false).into_iter().map(|offer| offer.putting).collect()
    }

    #[test]
    fn a_whole_number_takes_the_machines_decimal_mark() {
        // "5 inches" has no mark to follow; a machine that writes 12,7 gets
        // 12,7 — and a number written with a point keeps its point.
        let found = measurements_in("5 inches")[0];
        let offered = offers(&found, "5 inches", true);
        assert_eq!(offered[0].putting, "12,7 cm");
        let found = measurements_in("1.5 in")[0];
        assert_eq!(offers(&found, "1.5 in", true)[0].putting, "3.81 cm");
    }

    #[test]
    fn a_measurement_is_offered_in_the_other_system() {
        assert_eq!(converted("5 inches"), vec!["12.7 cm", "127 mm"]);
        assert_eq!(converted("100 km"), vec!["62.1 mi"]);
        assert_eq!(converted("1 mi"), vec!["1.61 km", "1609 m"]);
        assert_eq!(converted("2 lb"), vec!["0.907 kg", "907 g"]);
        assert_eq!(converted("60 mph"), vec!["96.6 km/h"]);
        assert_eq!(converted("20\u{B0}C"), vec!["68 \u{B0}F"]);
        assert_eq!(converted("72 \u{B0}F"), vec!["22.2 \u{B0}C"]);
        // With the decimal mark the writer used.
        assert_eq!(converted("12,5 cm"), vec!["4,92 in", "0,41 ft"]);
    }

    fn dated(text: &str, day_first: bool) -> Vec<(String, (i64, u32, u32))> {
        dates_in(text, &[], day_first)
            .into_iter()
            .map(|found| {
                let Recognised::Date { year, month, day } = found.what else { panic!() };
                (text[found.start..found.end].to_owned(), (year, month, day))
            })
            .collect()
    }

    #[test]
    fn a_date_is_recognised_however_it_is_written() {
        for (text, wanted) in [
            ("on 2026-09-28 at", "2026-09-28"),
            ("on 28 September 2026 at", "28 September 2026"),
            ("on 28. September 2026 at", "28. September 2026"),
            ("on September 28, 2026 at", "September 28, 2026"),
            ("on Sept 28, 2026 at", "Sept 28, 2026"),
            ("on 28/09/2026 at", "28/09/2026"),
            ("on 28.09.2026 at", "28.09.2026"),
        ] {
            assert_eq!(dated(text, true), vec![(wanted.to_owned(), (2026, 9, 28))], "{text}");
        }
        // Day first or month first, as the machine writes dates.
        assert_eq!(dated("09/10/2026", true)[0].1, (2026, 10, 9));
        assert_eq!(dated("09/10/2026", false)[0].1, (2026, 9, 10));
        // And the machine's own month names.
        let german = [
            "Januar",
            "Februar",
            "März",
            "April",
            "Mai",
            "Juni",
            "Juli",
            "August",
            "September",
            "Oktober",
            "November",
            "Dezember",
        ]
        .map(str::to_owned);
        assert_eq!(dates_in("am 3. März 2026", &german, true).len(), 1);
    }

    #[test]
    fn what_is_not_a_date_is_not_one() {
        for text in
            ["31/02/2026", "13/13/2026", "2026-13-01", "28-09/2026", "5 May", "page 3 of 12"]
        {
            assert_eq!(dated(text, true), vec![], "{text}");
        }
    }

    #[test]
    fn nothing_is_recognised_until_the_tab_says_so() {
        let off = Recognisers::default();
        assert!(off.at("5 inches", 2, &[], true).is_empty(), "Word ships it off");
        let on = Recognisers { enabled: true, ..Recognisers::default() };
        assert_eq!(on.at("a 5 inch pipe", 3, &[], true).len(), 1);
        assert!(on.at("a 5 inch pipe", 10, &[], true).is_empty(), "the pointer is past it");
        let no_measurements = Recognisers { measurements: false, ..on };
        assert!(no_measurements.at("a 5 inch pipe", 3, &[], true).is_empty());
    }

    #[test]
    fn an_amount_is_written_as_a_person_would() {
        assert_eq!(tidy(12.7, 3, false), "12.7");
        assert_eq!(tidy(127.0, 3, false), "127");
        assert_eq!(tidy(0.90718474, 3, false), "0.907");
        assert_eq!(tidy(1609.344, 3, false), "1609");
        assert_eq!(tidy(68.0, 3, false), "68");
        assert_eq!(tidy(22.222, 3, false), "22.2");
        assert_eq!(tidy(4.921, 3, true), "4,92");
    }
}
