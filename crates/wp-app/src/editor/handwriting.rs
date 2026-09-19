//! Reading handwriting: block capitals and digits, one character at a time.
//!
//! # How a character is read
//!
//! By the point-cloud method: the strokes of a character are resampled to a
//! fixed number of points, scaled to fit a square and centred, and then
//! matched greedily against every template, point to nearest unmatched
//! point, with the earliest points weighing most. The template that is
//! nearest is the character. It does not care which stroke was drawn first,
//! or which way, which is what makes it fit for letters that everybody
//! writes in a different order.
//!
//! # What it can read, and what it cannot
//!
//! The twenty-six capitals and the ten digits, drawn the way they are printed
//! on a keyboard. Not cursive, not lower case, not punctuation. A stroke
//! that is nothing like any of them still comes out as the nearest, which is
//! what any reader of handwriting does with a scrawl; what is asked for is
//! the person's word, and the nearest guess at it is more use than nothing.
//!
//! # Which strokes make one character
//!
//! The ones that stand over one another: strokes whose reaches across the
//! page overlap are the same character, and a gap between two is a new one.
//! A gap wider than half a character's height is a space.

/// How many points a character is resampled to.
const POINTS: usize = 32;

/// A character's strokes: each a run of points.
pub(crate) type Strokes = Vec<Vec<(f32, f32)>>;

/// A point of a cloud, with which stroke it came from.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Dot {
    x: f32,
    y: f32,
    stroke: usize,
}

/// The characters this can read, each as its strokes in a unit square with
/// the top left at the origin.
fn templates() -> Vec<(char, Strokes)> {
    // A run of points round an ellipse, from one angle to another, in
    // radians measured from three o'clock with noon at minus a quarter turn.
    fn arc(cx: f32, cy: f32, rx: f32, ry: f32, from: f32, to: f32) -> Vec<(f32, f32)> {
        (0..=16)
            .map(|step| {
                let angle = from + (to - from) * step as f32 / 16.0;
                (cx + rx * angle.cos(), cy + ry * angle.sin())
            })
            .collect()
    }
    use core::f32::consts::PI;
    let line = |points: &[(f32, f32)]| points.to_vec();
    vec![
        ('A', vec![line(&[(0.0, 1.0), (0.5, 0.0), (1.0, 1.0)]), line(&[(0.2, 0.6), (0.8, 0.6)])]),
        (
            'B',
            vec![
                line(&[(0.0, 0.0), (0.0, 1.0)]),
                line(&[(0.0, 0.0), (0.6, 0.0), (0.8, 0.1), (0.8, 0.4), (0.6, 0.5), (0.0, 0.5)]),
                line(&[(0.0, 0.5), (0.7, 0.5), (0.9, 0.65), (0.9, 0.9), (0.7, 1.0), (0.0, 1.0)]),
            ],
        ),
        ('C', vec![arc(0.5, 0.5, 0.5, 0.5, -PI / 4.0, -7.0 * PI / 4.0)]),
        (
            'D',
            vec![line(&[(0.0, 0.0), (0.0, 1.0)]), {
                let mut bowl = vec![(0.0, 0.0), (0.5, 0.0)];
                bowl.extend(arc(0.5, 0.5, 0.5, 0.5, -PI / 2.0, PI / 2.0));
                bowl.push((0.0, 1.0));
                bowl
            }],
        ),
        (
            'E',
            vec![
                line(&[(1.0, 0.0), (0.0, 0.0), (0.0, 1.0), (1.0, 1.0)]),
                line(&[(0.0, 0.5), (0.8, 0.5)]),
            ],
        ),
        ('F', vec![line(&[(1.0, 0.0), (0.0, 0.0), (0.0, 1.0)]), line(&[(0.0, 0.5), (0.8, 0.5)])]),
        (
            'G',
            vec![arc(0.5, 0.5, 0.5, 0.5, -PI / 4.0, -2.0 * PI), line(&[(1.0, 0.5), (0.55, 0.5)])],
        ),
        (
            'H',
            vec![
                line(&[(0.0, 0.0), (0.0, 1.0)]),
                line(&[(1.0, 0.0), (1.0, 1.0)]),
                line(&[(0.0, 0.5), (1.0, 0.5)]),
            ],
        ),
        ('I', vec![line(&[(0.5, 0.0), (0.5, 1.0)])]),
        (
            'I',
            vec![
                line(&[(0.5, 0.0), (0.5, 1.0)]),
                line(&[(0.2, 0.0), (0.8, 0.0)]),
                line(&[(0.2, 1.0), (0.8, 1.0)]),
            ],
        ),
        (
            'J',
            vec![line(&[
                (1.0, 0.0),
                (1.0, 0.7),
                (0.9, 0.9),
                (0.7, 1.0),
                (0.4, 1.0),
                (0.15, 0.85),
                (0.1, 0.7),
            ])],
        ),
        (
            'K',
            vec![
                line(&[(0.0, 0.0), (0.0, 1.0)]),
                line(&[(1.0, 0.0), (0.0, 0.55)]),
                line(&[(0.25, 0.4), (1.0, 1.0)]),
            ],
        ),
        ('L', vec![line(&[(0.0, 0.0), (0.0, 1.0), (1.0, 1.0)])]),
        ('M', vec![line(&[(0.0, 1.0), (0.0, 0.0), (0.5, 0.6), (1.0, 0.0), (1.0, 1.0)])]),
        ('N', vec![line(&[(0.0, 1.0), (0.0, 0.0), (1.0, 1.0), (1.0, 0.0)])]),
        ('O', vec![arc(0.5, 0.5, 0.5, 0.5, 0.0, 2.0 * PI)]),
        (
            'P',
            vec![line(&[
                (0.0, 1.0),
                (0.0, 0.0),
                (0.7, 0.0),
                (0.9, 0.15),
                (0.9, 0.4),
                (0.7, 0.55),
                (0.0, 0.55),
            ])],
        ),
        ('Q', vec![arc(0.5, 0.5, 0.5, 0.5, 0.0, 2.0 * PI), line(&[(0.6, 0.7), (1.0, 1.0)])]),
        (
            'R',
            vec![
                line(&[
                    (0.0, 1.0),
                    (0.0, 0.0),
                    (0.7, 0.0),
                    (0.9, 0.15),
                    (0.9, 0.4),
                    (0.7, 0.55),
                    (0.0, 0.55),
                ]),
                line(&[(0.5, 0.55), (1.0, 1.0)]),
            ],
        ),
        (
            'S',
            vec![line(&[
                (0.9, 0.15),
                (0.7, 0.0),
                (0.3, 0.0),
                (0.1, 0.15),
                (0.1, 0.35),
                (0.3, 0.5),
                (0.7, 0.5),
                (0.9, 0.65),
                (0.9, 0.85),
                (0.7, 1.0),
                (0.3, 1.0),
                (0.1, 0.85),
            ])],
        ),
        ('T', vec![line(&[(0.0, 0.0), (1.0, 0.0)]), line(&[(0.5, 0.0), (0.5, 1.0)])]),
        (
            'U',
            vec![line(&[
                (0.0, 0.0),
                (0.0, 0.7),
                (0.1, 0.9),
                (0.3, 1.0),
                (0.7, 1.0),
                (0.9, 0.9),
                (1.0, 0.7),
                (1.0, 0.0),
            ])],
        ),
        ('V', vec![line(&[(0.0, 0.0), (0.5, 1.0), (1.0, 0.0)])]),
        ('W', vec![line(&[(0.0, 0.0), (0.25, 1.0), (0.5, 0.3), (0.75, 1.0), (1.0, 0.0)])]),
        ('X', vec![line(&[(0.0, 0.0), (1.0, 1.0)]), line(&[(1.0, 0.0), (0.0, 1.0)])]),
        ('Y', vec![line(&[(0.0, 0.0), (0.5, 0.5), (1.0, 0.0)]), line(&[(0.5, 0.5), (0.5, 1.0)])]),
        ('Z', vec![line(&[(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)])]),
        ('0', vec![arc(0.5, 0.5, 0.35, 0.5, 0.0, 2.0 * PI)]),
        ('1', vec![line(&[(0.3, 0.2), (0.5, 0.0), (0.5, 1.0)])]),
        (
            '2',
            vec![line(&[
                (0.1, 0.2),
                (0.3, 0.0),
                (0.7, 0.0),
                (0.9, 0.2),
                (0.9, 0.4),
                (0.0, 1.0),
                (1.0, 1.0),
            ])],
        ),
        (
            '3',
            vec![line(&[
                (0.1, 0.1),
                (0.4, 0.0),
                (0.8, 0.0),
                (0.9, 0.2),
                (0.8, 0.45),
                (0.4, 0.5),
                (0.8, 0.55),
                (0.9, 0.8),
                (0.8, 1.0),
                (0.4, 1.0),
                (0.1, 0.9),
            ])],
        ),
        ('4', vec![line(&[(0.7, 0.0), (0.0, 0.7), (1.0, 0.7)]), line(&[(0.7, 0.0), (0.7, 1.0)])]),
        (
            '5',
            vec![line(&[
                (1.0, 0.0),
                (0.1, 0.0),
                (0.05, 0.45),
                (0.5, 0.4),
                (0.9, 0.55),
                (0.9, 0.85),
                (0.6, 1.0),
                (0.1, 0.9),
            ])],
        ),
        (
            '6',
            vec![line(&[
                (0.9, 0.05),
                (0.4, 0.05),
                (0.1, 0.4),
                (0.05, 0.75),
                (0.3, 1.0),
                (0.7, 1.0),
                (0.95, 0.75),
                (0.7, 0.5),
                (0.3, 0.5),
                (0.05, 0.75),
            ])],
        ),
        ('7', vec![line(&[(0.0, 0.0), (1.0, 0.0), (0.4, 1.0)])]),
        (
            '8',
            vec![line(&[
                (0.5, 0.5),
                (0.2, 0.35),
                (0.2, 0.1),
                (0.5, 0.0),
                (0.8, 0.1),
                (0.8, 0.35),
                (0.5, 0.5),
                (0.15, 0.65),
                (0.15, 0.9),
                (0.5, 1.0),
                (0.85, 0.9),
                (0.85, 0.65),
                (0.5, 0.5),
            ])],
        ),
        (
            '9',
            vec![line(&[
                (0.95, 0.25),
                (0.7, 0.0),
                (0.3, 0.0),
                (0.05, 0.25),
                (0.3, 0.5),
                (0.7, 0.5),
                (0.95, 0.25),
                (0.9, 0.6),
                (0.6, 0.95),
                (0.1, 0.95),
            ])],
        ),
    ]
}

/// Which characters look like another: a bar is an I or a one, a ring an O
/// or a nought. The choice between them is made from the company they keep.
fn twin(character: char) -> Option<(char, char)> {
    match character {
        'I' | '1' => Some(('I', '1')),
        'O' | '0' => Some(('O', '0')),
        _ => None,
    }
}

/// The strokes of some characters as a cloud each: the same number of
/// points for every character, spread evenly along the strokes, fitted to a
/// square and centred on the origin.
fn cloud(strokes: &[Vec<(f32, f32)>]) -> Vec<Dot> {
    let mut lengths = Vec::new();
    let mut total = 0.0f32;
    for stroke in strokes {
        let length: f32 = stroke
            .windows(2)
            .map(|pair| (pair[1].0 - pair[0].0).hypot(pair[1].1 - pair[0].1))
            .sum();
        lengths.push(length);
        total += length;
    }
    let mut dots = Vec::new();
    if total <= 0.0 {
        // A dot, or nothing: every point is the same place.
        for (index, stroke) in strokes.iter().enumerate() {
            if let Some((x, y)) = stroke.first() {
                dots.push(Dot { x: *x, y: *y, stroke: index });
            }
        }
    } else {
        let step = total / (POINTS - 1) as f32;
        let mut carried = 0.0f32;
        for (index, stroke) in strokes.iter().enumerate() {
            if stroke.is_empty() {
                continue;
            }
            dots.push(Dot { x: stroke[0].0, y: stroke[0].1, stroke: index });
            for pair in stroke.windows(2) {
                let (mut from, to) = (pair[0], pair[1]);
                let mut length = (to.0 - from.0).hypot(to.1 - from.1);
                while carried + length >= step && length > 0.0 {
                    let along = (step - carried) / length;
                    let at = (from.0 + (to.0 - from.0) * along, from.1 + (to.1 - from.1) * along);
                    dots.push(Dot { x: at.0, y: at.1, stroke: index });
                    from = at;
                    length = (to.0 - from.0).hypot(to.1 - from.1);
                    carried = 0.0;
                }
                carried += length;
            }
        }
    }
    dots.truncate(POINTS);

    // Fitted to a square, each way on its own, so a letter written tall and
    // narrow is compared with the template's shape and not its proportions
    // — except a bar, which has no width worth stretching and is fitted by
    // its height alone. Then centred, so where it was drawn does not count.
    let (mut left, mut top, mut right, mut bottom) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for dot in &dots {
        left = left.min(dot.x);
        top = top.min(dot.y);
        right = right.max(dot.x);
        bottom = bottom.max(dot.y);
    }
    let (width, height) = ((right - left).max(f32::EPSILON), (bottom - top).max(f32::EPSILON));
    let (sx, sy) = if width < height * 0.25 {
        (height, height)
    } else if height < width * 0.25 {
        (width, width)
    } else {
        (width, height)
    };
    let count = dots.len().max(1) as f32;
    let (cx, cy) = (
        dots.iter().map(|dot| dot.x).sum::<f32>() / count,
        dots.iter().map(|dot| dot.y).sum::<f32>() / count,
    );
    for dot in &mut dots {
        dot.x = (dot.x - cx) / sx;
        dot.y = (dot.y - cy) / sy;
    }
    dots
}

/// How unlike two clouds are: the greedy match, tried from every few
/// starting points and both ways round, the least of them.
fn unlikeness(one: &[Dot], other: &[Dot]) -> f32 {
    if one.is_empty() || other.is_empty() {
        return f32::MAX;
    }
    let count = one.len().min(other.len());
    let stride = ((count as f32).sqrt() as usize).max(1);
    let mut least = f32::MAX;
    let mut start = 0;
    while start < count {
        least = least.min(greedy(one, other, start)).min(greedy(other, one, start));
        start += stride;
    }
    least
}

fn greedy(from: &[Dot], to: &[Dot], start: usize) -> f32 {
    let count = from.len().min(to.len());
    let mut matched = vec![false; to.len()];
    let mut sum = 0.0f32;
    for step in 0..count {
        let index = (start + step) % count;
        let dot = from[index];
        let mut nearest = (f32::MAX, 0usize);
        for (candidate, other) in to.iter().enumerate() {
            if matched[candidate] {
                continue;
            }
            let far = (dot.x - other.x).hypot(dot.y - other.y);
            if far < nearest.0 {
                nearest = (far, candidate);
            }
        }
        matched[nearest.1] = true;
        let weight = 1.0 - step as f32 / count as f32;
        sum += weight * nearest.0;
    }
    sum
}

/// The character some strokes are most like.
#[must_use]
pub(crate) fn recognise(strokes: &[Vec<(f32, f32)>]) -> Option<char> {
    let drawn = cloud(strokes);
    if drawn.is_empty() {
        return None;
    }
    templates()
        .iter()
        .map(|(character, template)| (*character, unlikeness(&drawn, &cloud(template))))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(character, _)| character)
}

/// The strokes gathered into the characters they make, left to right, with
/// `None` between two characters far enough apart to be a space.
fn characters(strokes: &[Vec<(f32, f32)>]) -> Vec<Option<Strokes>> {
    let mut reaches: Vec<(f32, f32, f32, f32, usize)> = strokes
        .iter()
        .enumerate()
        .filter(|(_, stroke)| !stroke.is_empty())
        .map(|(index, stroke)| {
            let (mut left, mut top, mut right, mut bottom) =
                (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for (x, y) in stroke {
                left = left.min(*x);
                top = top.min(*y);
                right = right.max(*x);
                bottom = bottom.max(*y);
            }
            (left, top, right, bottom, index)
        })
        .collect();
    reaches.sort_by(|a, b| a.0.total_cmp(&b.0));

    // Strokes whose reaches across overlap are one character.
    let mut groups: Vec<(f32, f32, f32, f32, Vec<usize>)> = Vec::new();
    for (left, top, right, bottom, index) in reaches {
        match groups.last_mut() {
            Some(group) if left <= group.2 => {
                group.2 = group.2.max(right);
                group.1 = group.1.min(top);
                group.3 = group.3.max(bottom);
                group.4.push(index);
            }
            _ => groups.push((left, top, right, bottom, vec![index])),
        }
    }
    let height = groups.iter().map(|group| group.3 - group.1).fold(0.0f32, f32::max).max(1.0);

    let mut out = Vec::new();
    let mut last_right: Option<f32> = None;
    for group in groups {
        if let Some(right) = last_right {
            if group.0 - right > height * 0.5 {
                out.push(None);
            }
        }
        out.push(Some(group.4.iter().map(|index| strokes[*index].clone()).collect()));
        last_right = Some(group.2);
    }
    out
}

/// Reads what some strokes spell: the characters they make, left to right,
/// with a space where a gap is wide enough for one.
#[must_use]
pub(crate) fn read(strokes: &[Vec<(f32, f32)>]) -> String {
    let read: Vec<Option<char>> = characters(strokes)
        .iter()
        .map(|character| character.as_ref().and_then(|strokes| recognise(strokes)))
        .collect();

    // A bar is an I among letters and a one among digits; a ring an O or a
    // nought. Each takes the company of its word.
    let mut out = String::new();
    for word in read.split(|character| character.is_none()) {
        let digits =
            word.iter().flatten().filter(|c| c.is_ascii_digit() && twin(**c).is_none()).count();
        let letters = word
            .iter()
            .flatten()
            .filter(|c| c.is_ascii_alphabetic() && twin(**c).is_none())
            .count();
        if !out.is_empty() {
            out.push(' ');
        }
        for character in word.iter().flatten() {
            out.push(match twin(*character) {
                Some((letter, digit)) => {
                    if digits > letters {
                        digit
                    } else {
                        letter
                    }
                }
                None => *character,
            });
        }
    }
    out.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A template drawn at some size and place, a little unevenly.
    fn drawn(character: char, size: f32, x: f32, y: f32) -> Strokes {
        let (_, strokes) = templates().into_iter().find(|(c, _)| *c == character).expect("known");
        strokes
            .iter()
            .map(|stroke| {
                stroke
                    .iter()
                    .enumerate()
                    .map(|(index, (sx, sy))| {
                        let wobble = if index % 2 == 0 { 0.02 } else { -0.02 };
                        (x + (sx + wobble) * size, y + (sy - wobble) * size * 1.3)
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn every_character_is_read_back_from_its_own_shape_drawn_larger_and_elsewhere() {
        for (character, _) in templates() {
            let strokes = drawn(character, 40.0, 100.0, 200.0);
            let read = recognise(&strokes).expect("something");
            let same = read == character
                || twin(read).is_some_and(|(a, b)| a == character || b == character);
            assert!(same, "{character} was read as {read}");
        }
    }

    #[test]
    fn strokes_that_stand_over_one_another_make_one_character_and_gaps_make_words() {
        let mut strokes = drawn('H', 40.0, 0.0, 0.0);
        strokes.extend(drawn('I', 40.0, 40.0, 0.0));
        strokes.extend(drawn('A', 40.0, 160.0, 0.0));
        strokes.extend(drawn('T', 40.0, 220.0, 0.0));
        assert_eq!(read(&strokes), "HI AT");
        // Among digits a bar is a one.
        let mut digits = drawn('4', 40.0, 0.0, 0.0);
        digits.extend(drawn('1', 40.0, 40.0, 0.0));
        digits.extend(drawn('2', 40.0, 75.0, 0.0));
        assert_eq!(read(&digits), "412");
    }

    #[test]
    fn nothing_reads_as_nothing() {
        assert_eq!(read(&[]), "");
        assert_eq!(recognise(&[Vec::new()]), None);
    }
}
