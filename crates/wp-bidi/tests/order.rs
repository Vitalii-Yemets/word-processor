//! The bidirectional algorithm, checked against the examples the standard
//! itself gives and against sentences a reader would notice.
//!
//! The Hebrew is written here as it is stored — the order it is typed and read
//! aloud in — and the tests say what order it should be *drawn* in. Those are
//! two different things, and mixing them up is the whole difficulty of the
//! subject.

use wp_bidi::{character_levels, class_of, visual_order, Class, Direction};

/// The text drawn in the order the algorithm asks for.
fn drawn(text: &str, direction: Direction) -> String {
    let characters: Vec<char> = text.chars().collect();
    visual_order(text, direction).into_iter().map(|index| characters[index]).collect()
}

/// A Hebrew word, spelled here as separate letters so the test can be read.
const ALEF: char = 'א';
const BET: char = 'ב';
const GIMEL: char = 'ג';

#[test]
fn text_with_no_strong_characters_reads_left_to_right() {
    assert_eq!(Direction::from_text("123 456"), Direction::LeftToRight);
    assert_eq!(Direction::from_text("!?"), Direction::LeftToRight);
}

#[test]
fn the_first_strong_character_decides_the_direction() {
    assert_eq!(Direction::from_text("hello שלום"), Direction::LeftToRight);
    assert_eq!(Direction::from_text("שלום hello"), Direction::RightToLeft);
    // A number before the first letter decides nothing.
    assert_eq!(Direction::from_text("123 שלום"), Direction::RightToLeft);
}

#[test]
fn a_left_to_right_line_is_left_alone() {
    let text = "hello world";
    assert_eq!(drawn(text, Direction::LeftToRight), text);
}

#[test]
fn a_line_of_hebrew_is_turned_round() {
    let text: String = [ALEF, BET, GIMEL].iter().collect();
    let expected: String = [GIMEL, BET, ALEF].iter().collect();
    assert_eq!(drawn(&text, Direction::RightToLeft), expected);
}

#[test]
fn english_inside_hebrew_keeps_its_own_direction() {
    // Stored: alef bet, space, "car". Drawn from the right: the Hebrew word
    // first, then the English one, which is itself still left to right.
    let text = format!("{ALEF}{BET} car");
    let drawn = drawn(&text, Direction::RightToLeft);
    assert!(drawn.ends_with(&format!("{BET}{ALEF}")), "the Hebrew is not at the right: {drawn}");
    assert!(drawn.starts_with("car"), "the English word came out backwards: {drawn}");
}

#[test]
fn a_number_inside_hebrew_is_not_turned_round() {
    // The digits of a number always read left to right, whatever surrounds
    // them: a year is not written backwards in a Hebrew sentence.
    let text = format!("{ALEF}{BET} 1994");
    let drawn = drawn(&text, Direction::RightToLeft);
    assert!(drawn.starts_with("1994"), "the number came out backwards: {drawn}");
}

#[test]
fn hebrew_inside_english_is_turned_round_within_the_line() {
    let text = format!("The word {ALEF}{BET}{GIMEL} means something");
    let drawn = drawn(&text, Direction::LeftToRight);
    assert!(drawn.starts_with("The word "), "{drawn}");
    assert!(drawn.contains(&format!("{GIMEL}{BET}{ALEF}")), "the Hebrew was not turned: {drawn}");
    assert!(drawn.ends_with(" means something"), "{drawn}");
}

#[test]
fn a_full_stop_between_two_numbers_stays_with_them() {
    // W4: a single common separator between two European numbers joins them,
    // which is what keeps 3.14 in one piece.
    let text = format!("{ALEF} 3.14");
    let drawn = drawn(&text, Direction::RightToLeft);
    assert!(drawn.starts_with("3.14"), "the number was broken up: {drawn}");
}

#[test]
fn a_currency_sign_beside_a_number_goes_with_it() {
    // W5: terminators next to a European number join it.
    let text = format!("{ALEF} $50");
    let drawn = drawn(&text, Direction::RightToLeft);
    assert!(drawn.starts_with("$50"), "the sign was left behind: {drawn}");
}

#[test]
fn an_arabic_number_reads_its_own_way() {
    // W2: a European number after an Arabic letter becomes an Arabic number,
    // and an Arabic number sits at a different level from a European one.
    let text = "ب 12";
    let levels = character_levels(text, Direction::RightToLeft);
    let digit = text.chars().position(|character| character == '1').expect("a digit");
    assert_eq!(levels[digit] % 2, 0, "the digits do not read left to right");
    assert_eq!(class_of('١'), Class::AN);
}

#[test]
fn punctuation_between_two_hebrew_words_stays_between_them() {
    // N1: neutrals surrounded by one direction take it, so the comma does not
    // escape to the end of the line.
    let text = format!("{ALEF}, {BET}");
    let drawn = drawn(&text, Direction::RightToLeft);
    assert_eq!(drawn.chars().next(), Some(BET));
    assert_eq!(drawn.chars().last(), Some(ALEF));
    assert!(drawn.contains(','), "the comma went missing: {drawn}");
}

#[test]
fn punctuation_between_two_directions_takes_the_paragraphs() {
    // N2: neutrals between different directions take the paragraph's own.
    let text = format!("hello, {ALEF}{BET}");
    let drawn = drawn(&text, Direction::LeftToRight);
    assert!(drawn.starts_with("hello, "), "the comma moved: {drawn}");
}

#[test]
fn an_override_forces_the_direction_of_what_it_encloses() {
    // A right-to-left override, then Latin letters, then the pop: the letters
    // are drawn backwards because the document asked for it.
    let text = "\u{202E}abc\u{202C}";
    let drawn = drawn(text, Direction::LeftToRight);
    assert!(drawn.contains("cba"), "the override was ignored: {drawn}");
}

#[test]
fn an_isolate_keeps_what_it_encloses_out_of_the_way() {
    // What is inside an isolate does not change the direction of what is
    // outside it, which is the whole point of the isolates.
    let inside = format!("\u{2067}{ALEF}{BET}\u{2069}");
    assert_eq!(Direction::from_text(&format!("hello {inside} world")), Direction::LeftToRight);
}

#[test]
fn a_trailing_space_stays_at_the_end_of_the_line() {
    // L1: whitespace at the end of a line goes back to the paragraph's
    // direction, so it does not get dragged to the other end.
    let text = format!("{ALEF}{BET} ");
    let levels = character_levels(&text, Direction::RightToLeft);
    assert_eq!(levels.last().copied(), Some(1), "the trailing space kept a raised level");
}

#[test]
fn every_byte_of_a_character_carries_its_level() {
    // The levels are given per byte, because that is what the rest of the
    // program addresses text with.
    let text = format!("{ALEF}a");
    let levels = wp_bidi::levels(&text, Direction::RightToLeft);
    assert_eq!(levels.len(), text.len());
    assert_eq!(levels[0], levels[1], "the two bytes of the Hebrew letter disagree");
    assert_ne!(levels[0] % 2, levels[2] % 2, "the Latin letter took the Hebrew level");
}
