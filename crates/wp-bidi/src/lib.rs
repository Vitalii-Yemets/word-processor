//! The Unicode bidirectional algorithm: what order mixed text is drawn in.
//!
//! # What the problem is
//!
//! Hebrew and Arabic read right to left, and almost no text in them is purely
//! right to left. A Hebrew sentence quoting an English product name, or giving
//! a price, or citing a page number, holds runs of both directions at once —
//! and the order the characters are *stored* in is not the order they are
//! *drawn* in. The stored order is the order they are read aloud; the drawn
//! order is worked out from it.
//!
//! Getting this wrong is not a subtle typographic fault. A sentence comes out
//! with its clauses in the wrong places, brackets face the wrong way, and a
//! telephone number reads backwards. A reader of the language sees it at once.
//!
//! # What is here
//!
//! [UAX #9], the standard's own algorithm, in the order the standard sets it
//! out: the paragraph's direction (P2 and P3), the explicit embeddings and
//! isolates (X1 to X8), the weak types (W1 to W7), the paired brackets and the
//! neutrals (N0 to N2), the implicit levels (I1 and I2), the reordering of a
//! line (L1 and L2), and the characters that are drawn mirrored in it (L4).
//!
//! [UAX #9]: https://www.unicode.org/reports/tr9/
//!
//! # Example
//!
//! ```
//! // A Hebrew word, a space, then an English one: the English keeps its own
//! // direction inside a line that runs the other way.
//! let text = "שלום world";
//! let levels = wp_bidi::levels(text, wp_bidi::Direction::from_text(text));
//! assert_eq!(levels[0] % 2, 1, "the Hebrew is at an odd level");
//! assert_eq!(levels[text.len() - 1] % 2, 0, "the English is at an even one");
//! ```

#![forbid(unsafe_code)]

pub mod class;
pub mod mirror;

pub use class::{class_of, Class};
pub use mirror::mirrored;

/// The greatest depth the standard allows.
const MAX_DEPTH: u8 = 125;

/// Which way a paragraph runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Direction {
    #[default]
    LeftToRight,
    RightToLeft,
}

impl Direction {
    /// The direction a paragraph takes from its own text, by rules P2 and P3:
    /// the first strong character decides, and text with none reads left to
    /// right.
    #[must_use]
    pub fn from_text(text: &str) -> Self {
        let classes: Vec<Class> = text.chars().map(class_of).collect();
        match first_strong(&classes, 0..classes.len()) {
            Some(Class::R | Class::AL) => Self::RightToLeft,
            _ => Self::LeftToRight,
        }
    }

    #[must_use]
    fn level(self) -> u8 {
        match self {
            Self::LeftToRight => 0,
            Self::RightToLeft => 1,
        }
    }

    #[must_use]
    pub fn is_right_to_left(self) -> bool {
        self == Self::RightToLeft
    }
}

/// The embedding level of every character, one per byte of the text.
///
/// One per byte rather than per character so that a caller holding byte offsets
/// — which everything in this project does — can ask about any position without
/// counting characters first. Every byte of a character carries that
/// character's level.
#[must_use]
pub fn levels(text: &str, paragraph: Direction) -> Vec<u8> {
    let per_character = character_levels(text, paragraph);
    let mut out = Vec::with_capacity(text.len());
    for (level, character) in per_character.iter().zip(text.chars()) {
        for _ in 0..character.len_utf8() {
            out.push(*level);
        }
    }
    out
}

/// The order the characters of a line are drawn in, left to right.
///
/// Rules L1 and L2: the levels are worked out for the paragraph, trailing
/// whitespace is put back to the paragraph's own direction, and then each run
/// of characters at or above each level is reversed, from the highest level
/// down to the lowest odd one. What comes back is the index of each character
/// in the order it should be drawn.
#[must_use]
pub fn visual_order(text: &str, paragraph: Direction) -> Vec<usize> {
    let levels = character_levels(text, paragraph);
    reorder(&levels)
}

/// The visual order of a line whose levels are already known.
#[must_use]
pub fn reorder(levels: &[u8]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..levels.len()).collect();
    if levels.is_empty() {
        return order;
    }

    let highest = levels.iter().copied().max().unwrap_or(0);
    let lowest_odd = levels.iter().copied().filter(|level| level % 2 == 1).min().unwrap_or(0);
    if lowest_odd == 0 {
        return order;
    }

    // From the highest level down to the lowest odd one, every run at or above
    // that level is turned round. Doing it level by level is what nests a
    // left-to-right phrase inside a right-to-left one correctly.
    let mut level = highest;
    while level >= lowest_odd {
        let mut index = 0;
        while index < levels.len() {
            if levels[index] < level {
                index += 1;
                continue;
            }
            let start = index;
            while index < levels.len() && levels[index] >= level {
                index += 1;
            }
            order[start..index].reverse();
        }
        if level == 0 {
            break;
        }
        level -= 1;
    }
    order
}

/// The embedding level of every character.
#[must_use]
pub fn character_levels(text: &str, paragraph: Direction) -> Vec<u8> {
    let characters: Vec<char> = text.chars().collect();
    let original: Vec<Class> = characters.iter().copied().map(class_of).collect();
    let base = paragraph.level();
    let mut classes = original.clone();
    let mut levels = vec![base; classes.len()];

    explicit(&original, &mut classes, &mut levels, base);

    // The rules from here are applied to one isolating run sequence at a time.
    // A sequence is a run of characters at the same level, so the runs are
    // found first and each is resolved on its own.
    for run in level_runs(&levels) {
        let sos = boundary(&levels, &run, base, true);
        let eos = boundary(&levels, &run, base, false);
        weak(&original, &mut classes, &levels, &run, sos);
        brackets(&characters, &original, &mut classes, &levels, &run, sos);
        neutral(&mut classes, &levels, &run, sos, eos);
        implicit(&classes, &mut levels, &run);
    }

    trailing_whitespace(&original, &mut levels, base);
    levels
}

/// The first strong class in a stretch of text, if there is one.
fn first_strong(classes: &[Class], range: core::ops::Range<usize>) -> Option<Class> {
    let mut depth = 0usize;
    for index in range {
        let class = classes[index];
        // What an isolate encloses does not decide the direction outside it.
        if class.is_isolate_start() {
            depth += 1;
            continue;
        }
        if class == Class::PDI {
            depth = depth.saturating_sub(1);
            continue;
        }
        if depth == 0 && class.is_strong() {
            return Some(class);
        }
    }
    None
}

/// Rules X1 to X8: the explicit embeddings, overrides and isolates.
///
/// A stack of states, each remembering the level to use, whether characters are
/// being overridden to one direction, and whether the state was entered through
/// an isolate. Everything the rules call "removed by X9" — the codes themselves
/// and the boundary neutrals — is kept in place and marked, because taking
/// characters out of a string that the caller addresses by byte offset would
/// mean mapping every offset back afterwards.
fn explicit(original: &[Class], classes: &mut [Class], levels: &mut [u8], base: u8) {
    struct State {
        level: u8,
        override_to: Option<Class>,
        isolate: bool,
    }

    let mut stack = vec![State { level: base, override_to: None, isolate: false }];
    let mut overflow_isolates = 0usize;
    let mut overflow_embeddings = 0usize;
    let mut valid_isolates = 0usize;

    for index in 0..original.len() {
        let class = original[index];
        match class {
            Class::RLE | Class::LRE | Class::RLO | Class::LRO => {
                levels[index] = stack.last().map_or(base, |state| state.level);
                classes[index] = Class::BN;

                let last = stack.last().map_or(base, |state| state.level);
                let wanted = if matches!(class, Class::RLE | Class::RLO) {
                    (last + 1) | 1
                } else {
                    (last + 2) & !1
                };
                if wanted <= MAX_DEPTH && overflow_isolates == 0 && overflow_embeddings == 0 {
                    stack.push(State {
                        level: wanted,
                        override_to: match class {
                            Class::RLO => Some(Class::R),
                            Class::LRO => Some(Class::L),
                            _ => None,
                        },
                        isolate: false,
                    });
                } else if overflow_isolates == 0 {
                    overflow_embeddings += 1;
                }
            }
            Class::RLI | Class::LRI | Class::FSI => {
                // A first-strong isolate takes the direction of what it
                // encloses, which is what makes it useful for text whose
                // direction is not known when it is written.
                let treated = if class == Class::FSI {
                    let end = matching_isolate(original, index);
                    match first_strong(original, index + 1..end) {
                        Some(Class::R | Class::AL) => Class::RLI,
                        _ => Class::LRI,
                    }
                } else {
                    class
                };

                let last = stack.last().map_or(base, |state| state.level);
                levels[index] = last;
                if let Some(to) = stack.last().and_then(|state| state.override_to) {
                    classes[index] = to;
                }

                let wanted = if treated == Class::RLI { (last + 1) | 1 } else { (last + 2) & !1 };
                if wanted <= MAX_DEPTH && overflow_isolates == 0 && overflow_embeddings == 0 {
                    valid_isolates += 1;
                    stack.push(State { level: wanted, override_to: None, isolate: true });
                } else {
                    overflow_isolates += 1;
                }
            }
            Class::PDI => {
                if overflow_isolates > 0 {
                    overflow_isolates -= 1;
                } else if valid_isolates > 0 {
                    overflow_embeddings = 0;
                    while stack.last().is_some_and(|state| !state.isolate) {
                        stack.pop();
                    }
                    stack.pop();
                    valid_isolates -= 1;
                }
                let last = stack.last().map_or(base, |state| state.level);
                levels[index] = last;
                if let Some(to) = stack.last().and_then(|state| state.override_to) {
                    classes[index] = to;
                }
            }
            Class::PDF => {
                levels[index] = stack.last().map_or(base, |state| state.level);
                classes[index] = Class::BN;
                if overflow_isolates > 0 {
                } else if overflow_embeddings > 0 {
                    overflow_embeddings -= 1;
                } else if stack.last().is_some_and(|state| !state.isolate) && stack.len() > 1 {
                    stack.pop();
                }
            }
            Class::B => {
                // A paragraph separator ends everything.
                stack.truncate(1);
                overflow_isolates = 0;
                overflow_embeddings = 0;
                valid_isolates = 0;
                levels[index] = base;
            }
            _ => {
                let state = stack.last();
                levels[index] = state.map_or(base, |state| state.level);
                if let Some(to) = state.and_then(|state| state.override_to) {
                    classes[index] = to;
                }
            }
        }
    }
}

/// Where the isolate begun at `start` is closed, or the end of the text.
fn matching_isolate(classes: &[Class], start: usize) -> usize {
    let mut depth = 1usize;
    for (index, class) in classes.iter().enumerate().skip(start + 1) {
        if class.is_isolate_start() {
            depth += 1;
        } else if *class == Class::PDI {
            depth -= 1;
            if depth == 0 {
                return index;
            }
        }
    }
    classes.len()
}

/// The stretches of text that share one level.
fn level_runs(levels: &[u8]) -> Vec<core::ops::Range<usize>> {
    let mut runs = Vec::new();
    let mut index = 0;
    while index < levels.len() {
        let start = index;
        let level = levels[index];
        while index < levels.len() && levels[index] == level {
            index += 1;
        }
        runs.push(start..index);
    }
    runs
}

/// The direction to assume just before or just after a run, by rule X10.
fn boundary(levels: &[u8], run: &core::ops::Range<usize>, base: u8, before: bool) -> Class {
    let inside = levels[run.start];
    let outside = if before {
        run.start.checked_sub(1).map_or(base, |index| levels[index])
    } else {
        levels.get(run.end).copied().unwrap_or(base)
    };
    if inside.max(outside) % 2 == 0 {
        Class::L
    } else {
        Class::R
    }
}

/// Rules W1 to W7: the weak types.
fn weak(
    original: &[Class],
    classes: &mut [Class],
    levels: &[u8],
    run: &core::ops::Range<usize>,
    sos: Class,
) {
    let _ = levels;

    // W1: a mark takes the type of what it follows.
    let mut previous = sos;
    for index in run.clone() {
        if classes[index] == Class::NSM {
            classes[index] = if previous.is_explicit() { Class::ON } else { previous };
        }
        if classes[index] != Class::BN {
            previous = classes[index];
        }
    }

    // W2: a European number after an Arabic letter is an Arabic number.
    let mut strong = sos;
    for index in run.clone() {
        match classes[index] {
            Class::L | Class::R | Class::AL => strong = classes[index],
            Class::EN if strong == Class::AL => classes[index] = Class::AN,
            _ => {}
        }
    }

    // W3: an Arabic letter is simply right-to-left from here on.
    for index in run.clone() {
        if classes[index] == Class::AL {
            classes[index] = Class::R;
        }
    }

    // W4: a single separator between two numbers of the same kind joins them.
    for index in run.clone() {
        let (Some(before), Some(after)) =
            (previous_class(classes, run, index), next_class(classes, run, index))
        else {
            continue;
        };
        match classes[index] {
            Class::ES if before == Class::EN && after == Class::EN => classes[index] = Class::EN,
            Class::CS if before == Class::EN && after == Class::EN => classes[index] = Class::EN,
            Class::CS if before == Class::AN && after == Class::AN => classes[index] = Class::AN,
            _ => {}
        }
    }

    // W5: a run of terminators beside a European number joins it.
    let indices: Vec<usize> = run.clone().filter(|index| classes[*index] != Class::BN).collect();
    let mut position = 0;
    while position < indices.len() {
        if classes[indices[position]] != Class::ET {
            position += 1;
            continue;
        }
        let start = position;
        while position < indices.len() && classes[indices[position]] == Class::ET {
            position += 1;
        }
        let before = start.checked_sub(1).map(|at| classes[indices[at]]);
        let after = indices.get(position).map(|at| classes[*at]);
        if before == Some(Class::EN) || after == Some(Class::EN) {
            for at in &indices[start..position] {
                classes[*at] = Class::EN;
            }
        }
    }

    // W6: whatever is left of the separators and terminators is neutral.
    for index in run.clone() {
        if matches!(classes[index], Class::ES | Class::ET | Class::CS) {
            classes[index] = Class::ON;
        }
    }

    // W7: a European number after a left-to-right strong type is left-to-right.
    let mut strong = sos;
    for index in run.clone() {
        match classes[index] {
            Class::L | Class::R => strong = classes[index],
            Class::EN if strong == Class::L => classes[index] = Class::L,
            _ => {}
        }
    }

    let _ = original;
}

/// The class of the last character before `index` that counts.
fn previous_class(classes: &[Class], run: &core::ops::Range<usize>, index: usize) -> Option<Class> {
    (run.start..index).rev().map(|at| classes[at]).find(|class| *class != Class::BN)
}

/// And of the next one.
fn next_class(classes: &[Class], run: &core::ops::Range<usize>, index: usize) -> Option<Class> {
    (index + 1..run.end).map(|at| classes[at]).find(|class| *class != Class::BN)
}

/// Rule N0: a bracketed phrase takes the direction of what is inside it.
///
/// The rule exists because the brackets themselves say nothing about
/// direction, and taking it from their surroundings puts them the wrong way
/// round: a Latin phrase in brackets inside a Hebrew sentence should have its
/// brackets read as the Latin does, not as the Hebrew around them does.
///
/// The pairs are found first, as BD16 says: a stack of the brackets that are
/// open, and a closing one matched against the nearest opening one it fits. An
/// unmatched bracket is left to the rules that follow.
fn brackets(
    characters: &[char],
    original: &[Class],
    classes: &mut [Class],
    levels: &[u8],
    run: &core::ops::Range<usize>,
    sos: Class,
) {
    let embedding = if levels[run.start] % 2 == 0 { Class::L } else { Class::R };
    let opposite = if embedding == Class::L { Class::R } else { Class::L };

    for (opening, closing) in pairs(characters, classes, run) {
        // The strong directions inside the pair, brackets not counted.
        let inside = (opening + 1..closing).filter_map(|at| strong(classes[at]));
        let mut found_embedding = false;
        let mut found_opposite = false;
        for class in inside {
            if class == embedding {
                found_embedding = true;
                break;
            }
            found_opposite = true;
        }

        let wanted = if found_embedding {
            // N0 b: what is inside reads the way the text around it does, so
            // the brackets do too.
            embedding
        } else if found_opposite {
            // N0 c: what is inside reads the other way. The brackets follow it
            // only if the text before them was already going that way.
            let before = (run.start..opening)
                .rev()
                .find_map(|at| strong(classes[at]))
                .unwrap_or(if sos == Class::R { Class::R } else { Class::L });
            if before == opposite {
                opposite
            } else {
                embedding
            }
        } else {
            // N0 d: nothing strong inside, so the brackets are left to N1 and
            // N2 like any other neutral.
            continue;
        };

        for at in [opening, closing] {
            classes[at] = wanted;
            // The marks hanging from a bracket go with it: they are drawn on
            // it, so they cannot read the other way.
            for following in at + 1..run.end {
                if original[following] != Class::NSM {
                    break;
                }
                classes[following] = wanted;
            }
        }
    }
}

/// The direction a class counts as for the bracket rule.
///
/// A number counts as right-to-left here, exactly as it does for N1: digits in
/// a Hebrew sentence are part of the Hebrew, however they are drawn.
fn strong(class: Class) -> Option<Class> {
    match class {
        Class::L => Some(Class::L),
        Class::R | Class::EN | Class::AN => Some(Class::R),
        _ => None,
    }
}

/// The bracket pairs inside one sequence, in the order they open.
///
/// BD16: a stack of what is open, and a closing bracket matched against the
/// nearest opening one that fits. The standard stops looking after sixty-three
/// open brackets rather than growing the stack without limit, and so does this.
fn pairs(
    characters: &[char],
    classes: &[Class],
    run: &core::ops::Range<usize>,
) -> Vec<(usize, usize)> {
    const STACK_LIMIT: usize = 63;

    let mut open: Vec<(char, usize)> = Vec::new();
    let mut found: Vec<(usize, usize)> = Vec::new();

    for index in run.clone() {
        // Only a bracket that is still a neutral counts: one the weak rules
        // have already turned into something else is no longer a bracket.
        if classes[index] != Class::ON {
            continue;
        }
        let Some(character) = characters.get(index).copied() else { continue };
        let Some((side, closing)) = mirror::bracket(character) else { continue };

        match side {
            mirror::Side::Opening => {
                if open.len() == STACK_LIMIT {
                    break;
                }
                open.push((closing, index));
            }
            mirror::Side::Closing => {
                if let Some(depth) =
                    open.iter().rposition(|(wanted, _)| mirror::same_bracket(*wanted, closing))
                {
                    found.push((open[depth].1, index));
                    open.truncate(depth);
                }
            }
        }
    }

    found.sort_unstable();
    found
}

/// Rules N1 and N2: the neutrals take the direction around them.
fn neutral(
    classes: &mut [Class],
    levels: &[u8],
    run: &core::ops::Range<usize>,
    sos: Class,
    eos: Class,
) {
    let embedding = if levels[run.start] % 2 == 0 { Class::L } else { Class::R };
    let indices: Vec<usize> = run.clone().filter(|index| classes[*index] != Class::BN).collect();

    let mut position = 0;
    while position < indices.len() {
        if !is_neutral(classes[indices[position]]) {
            position += 1;
            continue;
        }
        let start = position;
        while position < indices.len() && is_neutral(classes[indices[position]]) {
            position += 1;
        }

        let before =
            start.checked_sub(1).and_then(|at| strong(classes[indices[at]])).unwrap_or(sos);
        let after = indices.get(position).and_then(|at| strong(classes[*at])).unwrap_or(eos);

        // N1: neutrals between two of the same direction take it. N2: the rest
        // take the direction of the paragraph they sit in.
        let wanted = if before == after { before } else { embedding };
        for at in &indices[start..position] {
            classes[*at] = wanted;
        }
    }
}

fn is_neutral(class: Class) -> bool {
    matches!(class, Class::B | Class::S | Class::WS | Class::ON) || class.is_explicit()
}

/// Rules I1 and I2: the levels the resolved types imply.
fn implicit(classes: &[Class], levels: &mut [u8], run: &core::ops::Range<usize>) {
    for index in run.clone() {
        let level = levels[index];
        let raise = if level % 2 == 0 {
            match classes[index] {
                Class::R => 1,
                Class::AN | Class::EN => 2,
                _ => 0,
            }
        } else {
            match classes[index] {
                Class::L | Class::EN | Class::AN => 1,
                _ => 0,
            }
        };
        levels[index] = level.saturating_add(raise);
    }
}

/// Rule L1: whitespace at the end of a line, and the separators, go back to the
/// paragraph's own direction.
///
/// This is what keeps the ragged edge of a right-to-left paragraph on the left
/// and stops a trailing space from being dragged to the wrong end of the line.
fn trailing_whitespace(original: &[Class], levels: &mut [u8], base: u8) {
    let mut reset_from = original.len();
    for index in (0..original.len()).rev() {
        match original[index] {
            Class::WS
            | Class::BN
            | Class::LRE
            | Class::RLE
            | Class::LRO
            | Class::RLO
            | Class::PDF
            | Class::LRI
            | Class::RLI
            | Class::FSI
            | Class::PDI => {
                reset_from = index;
            }
            Class::B | Class::S => {
                levels[index] = base;
                reset_from = index;
            }
            _ => break,
        }
    }
    for level in levels.iter_mut().skip(reset_from) {
        *level = base;
    }
}
