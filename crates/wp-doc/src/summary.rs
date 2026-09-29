//! What the document says about itself: the two property streams.
//!
//! [MS-OLEPS]. A compound file keeps its title, its author and the rest in
//! two streams whose names begin with a control character — `\u{5}Summary
//! Information` and `\u{5}DocumentSummaryInformation` — each a set of
//! numbered properties, each property a type and a value. Every Office file
//! of the old kind has them, and so do many files that are not Office's; the
//! numbers are the same everywhere.
//!
//! Strings are in the code page the set names in its first property, or
//! two bytes a character where the set says so or the type does. Dates are
//! counts of a hundred nanoseconds since 1601.

use wp_docx::properties::Properties;
use wp_ole::CompoundFile;
use wp_text::Encoding;

/// The properties in a file's two streams, as far as the model keeps them.
pub(crate) fn properties_of(file: &CompoundFile) -> Properties {
    let mut properties = Properties::default();
    if let Some(stream) = file.stream("\u{5}SummaryInformation") {
        for (id, value) in property_set(&stream) {
            match (id, value) {
                (2, Value::Text(text)) => properties.title = text,
                (3, Value::Text(text)) => properties.subject = text,
                (4, Value::Text(text)) => properties.author = text,
                (5, Value::Text(text)) => properties.keywords = text,
                (6, Value::Text(text)) => properties.description = text,
                (8, Value::Text(text)) => properties.last_modified_by = text,
                (12, Value::Time(time)) => properties.created = time,
                (13, Value::Time(time)) => properties.modified = time,
                _ => {}
            }
        }
    }
    if let Some(stream) = file.stream("\u{5}DocumentSummaryInformation") {
        for (id, value) in property_set(&stream) {
            match (id, value) {
                (2, Value::Text(text)) => properties.category = text,
                (15, Value::Text(text)) => properties.company = text,
                _ => {}
            }
        }
    }
    properties
}

/// A property's value, of the kinds read here.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Value {
    Text(String),
    /// A date, written as the model writes one.
    Time(String),
    Other,
}

/// The first set in a stream: each property's number and value.
fn property_set(stream: &[u8]) -> Vec<(u32, Value)> {
    let u32_at = |bytes: &[u8], at: usize| {
        bytes.get(at..at + 4).map(|four| u32::from_le_bytes([four[0], four[1], four[2], four[3]]))
    };
    // The header: the byte order, the version, the system, a class, and how
    // many sets; then each set's identifier and where it is.
    if stream.get(0..2) != Some(&[0xFE, 0xFF]) {
        return Vec::new();
    }
    let Some(start) = u32_at(stream, 44).map(|at| at as usize) else { return Vec::new() };
    let Some(set) = stream.get(start..) else { return Vec::new() };
    let count = u32_at(set, 4).unwrap_or(0) as usize;
    let mut places = Vec::with_capacity(count.min(256));
    for index in 0..count.min(256) {
        let (Some(id), Some(at)) = (u32_at(set, 8 + index * 8), u32_at(set, 12 + index * 8)) else {
            break;
        };
        places.push((id, at as usize));
    }
    // Property one is the code page the strings are in.
    let page = places
        .iter()
        .find(|(id, _)| *id == 1)
        .and_then(|(_, at)| set.get(at + 4..at + 6))
        .map_or(1252, |two| u16::from_le_bytes([two[0], two[1]]));
    places.into_iter().map(|(id, at)| (id, value_at(set, at, page))).collect()
}

/// One value: its type in two bytes, two of padding, and what the type says.
fn value_at(set: &[u8], at: usize, page: u16) -> Value {
    let Some(kind) = set.get(at..at + 2).map(|two| u16::from_le_bytes([two[0], two[1]])) else {
        return Value::Other;
    };
    let body = set.get(at + 4..).unwrap_or(&[]);
    let length = || {
        body.get(0..4).map_or(0, |four| u32::from_le_bytes([four[0], four[1], four[2], four[3]]))
            as usize
    };
    match kind {
        // A string in the set's code page, counted in bytes with its ending.
        0x1E => {
            let bytes = body.get(4..4 + length()).unwrap_or(&[]);
            if page == 1200 {
                return Value::Text(wide(bytes));
            }
            let bytes: Vec<u8> = bytes.iter().copied().take_while(|byte| *byte != 0).collect();
            let encoding = Encoding::code_page(u32::from(page)).unwrap_or(Encoding::CodePage(1252));
            Value::Text(encoding.decode(&bytes))
        }
        // Two bytes a character, counted in characters.
        0x1F => Value::Text(wide(body.get(4..4 + length() * 2).unwrap_or(&[]))),
        0x40 => {
            let Some(eight) = body.get(0..8) else { return Value::Other };
            let ticks = u64::from_le_bytes(eight.try_into().unwrap_or([0; 8]));
            file_time(ticks).map_or(Value::Other, Value::Time)
        }
        _ => Value::Other,
    }
}

/// Two bytes a character, up to the first nought.
fn wide(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .take_while(|unit| *unit != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

/// A count of a hundred nanoseconds since the first of January 1601, as the
/// model writes a date; nothing for nought, which is a date never set.
fn file_time(ticks: u64) -> Option<String> {
    if ticks == 0 {
        return None;
    }
    let seconds = ticks / 10_000_000;
    // From 1601 to 1970.
    let unix = i64::try_from(seconds).ok()? - 11_644_473_600;
    let days = unix.div_euclid(86_400);
    let of_day = unix.rem_euclid(86_400);
    let (year, month, day) = civil(days);
    if !(1900..=9999).contains(&year) {
        return None;
    }
    Some(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        of_day / 3600,
        of_day % 3600 / 60,
        of_day % 60
    ))
}

/// The year, month and day a count of days since 1970 falls on.
fn civil(days: i64) -> (i64, i64, i64) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let of_era = shifted.rem_euclid(146_097);
    let year_of_era = (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * of_year + 2) / 153;
    let day = of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_time_is_a_date() {
        // 2024-03-05 10:30:00 UTC.
        let ticks = (1_709_634_600u64 + 11_644_473_600) * 10_000_000;
        assert_eq!(file_time(ticks).as_deref(), Some("2024-03-05T10:30:00Z"));
        assert_eq!(file_time(0), None);
    }

    #[test]
    fn a_set_is_read_in_its_code_page() {
        // One set holding the code page and a title.
        let mut set = Vec::new();
        let title = b"Report\0\0";
        let values: Vec<(u32, Vec<u8>)> = vec![
            (1, [2u16.to_le_bytes(), [0, 0], 1252u16.to_le_bytes(), [0, 0]].concat()),
            (2, [&0x1Eu32.to_le_bytes()[..], &(title.len() as u32).to_le_bytes(), title].concat()),
        ];
        let mut offsets = Vec::new();
        let mut body = Vec::new();
        let table_length = 8 + values.len() * 8;
        for (id, bytes) in &values {
            offsets.push((*id, table_length + body.len()));
            body.extend_from_slice(bytes);
        }
        set.extend_from_slice(&((table_length + body.len()) as u32).to_le_bytes());
        set.extend_from_slice(&(values.len() as u32).to_le_bytes());
        for (id, at) in offsets {
            set.extend_from_slice(&id.to_le_bytes());
            set.extend_from_slice(&(at as u32).to_le_bytes());
        }
        set.extend_from_slice(&body);
        let mut stream = vec![0xFE, 0xFF, 0, 0];
        stream.extend_from_slice(&[0; 20]);
        stream.extend_from_slice(&1u32.to_le_bytes());
        stream.extend_from_slice(&[0; 16]);
        stream.extend_from_slice(&48u32.to_le_bytes());
        stream.extend_from_slice(&set);
        let read = property_set(&stream);
        assert_eq!(read[1], (2, Value::Text("Report".to_owned())));
    }
}
