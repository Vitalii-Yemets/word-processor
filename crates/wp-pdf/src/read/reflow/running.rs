//! Running headers and footers: the lines a page repeats at its top and
//! foot.
//!
//! A line in the top or the bottom eighth of the page that comes back on
//! at least half the pages — the same words, whatever its numbers say —
//! is not the text but the header or the footer. Its numbers that go up
//! with the page are the page number, and one that is the count of the
//! pages on every page is that; the rest are words. The header and the
//! footer are made from the first page that has them, and the lines are
//! taken off every page.

use std::collections::HashMap;

use wp_docx::model::{
    Alignment, Block, Body, Paragraph, ParagraphProperties, Run, RunContent, TabAlignment,
};

use super::super::content::Glyph;
use super::{decorate, lines_of, properties_of, Line, PageDrawn};

/// Where a running line stood on one page, for taking its glyphs off it.
#[derive(Clone, Copy, Debug)]
pub(super) struct Area {
    x0: f64,
    x1: f64,
    y: f64,
    size: f64,
}

impl Area {
    pub(super) fn holds(&self, glyph: &Glyph) -> bool {
        let x = (glyph.x + glyph.end_x) / 2.0;
        glyph.angle.abs() < 0.1
            && x >= self.x0 - 1.0
            && x <= self.x1 + 1.0
            && (glyph.y - self.y).abs() <= 0.5 * self.size.max(glyph.size)
    }
}

/// The header and footer the pages share, and where each page has them.
pub(super) struct Running {
    pub header: Option<Body>,
    pub footer: Option<Body>,
    pub areas: Vec<Vec<Area>>,
}

/// One page's line in its top or bottom band.
struct Candidate {
    page: usize,
    top: bool,
    line: Line,
    key: String,
}

pub(super) fn running_lines(pages: &[PageDrawn]) -> Running {
    let mut running = Running { header: None, footer: None, areas: vec![Vec::new(); pages.len()] };
    if pages.len() < 2 {
        return running;
    }
    let mut candidates = Vec::new();
    for (index, page) in pages.iter().enumerate() {
        let level: Vec<Glyph> =
            page.drawn.glyphs.iter().filter(|g| g.angle.abs() < 0.1).cloned().collect();
        let pieces = decorate(&level, &[], &[], 0);
        for line in lines_of(pieces, &mut [], &mut Vec::new()) {
            let top = line.y > page.height * 0.88;
            if top || line.y < page.height * 0.12 {
                let key = key_of(&line.text());
                candidates.push(Candidate { page: index, top, line, key });
            }
        }
    }
    let least = pages.len().div_ceil(2).max(2);
    for top in [true, false] {
        let mut pages_of: HashMap<&str, Vec<usize>> = HashMap::new();
        for candidate in candidates.iter().filter(|c| c.top == top) {
            let pages = pages_of.entry(candidate.key.as_str()).or_default();
            if !pages.contains(&candidate.page) {
                pages.push(candidate.page);
            }
        }
        let chosen: Vec<&Candidate> = candidates
            .iter()
            .filter(|c| {
                c.top == top
                    && !c.key.is_empty()
                    && pages_of.get(c.key.as_str()).is_some_and(|p| p.len() >= least)
            })
            .collect();
        if chosen.is_empty() {
            continue;
        }
        for candidate in &chosen {
            let line = &candidate.line;
            running.areas[candidate.page].push(Area {
                x0: line.x0,
                x1: line.x1,
                y: line.y,
                size: line.size,
            });
        }
        let first = chosen.iter().map(|c| c.page).min().unwrap_or(0);
        let body = furniture_of(&chosen, first, pages[first].width, pages.len());
        if top {
            running.header = Some(body);
        } else {
            running.footer = Some(body);
        }
    }
    running
}

/// What a running line is known by from page to page: its words, with a
/// number of any kind standing for every number.
fn key_of(text: &str) -> String {
    text.split_whitespace()
        .map(|word| {
            if is_roman(word) {
                return "#".to_owned();
            }
            let mut out = String::new();
            let mut in_number = false;
            for c in word.chars() {
                if c.is_ascii_digit() {
                    if !in_number {
                        out.push('#');
                    }
                    in_number = true;
                } else {
                    in_number = false;
                    out.push(c);
                }
            }
            out
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_roman(word: &str) -> bool {
    !word.is_empty()
        && word.len() <= 6
        && (word.chars().all(|c| "ivxlc".contains(c)) || word.chars().all(|c| "IVXLC".contains(c)))
}

/// The numbers in a line's text, in order.
fn numbers_of(text: &str) -> Vec<i64> {
    let mut out = Vec::new();
    let mut current: Option<i64> = None;
    for c in text.chars() {
        match (c.to_digit(10), current) {
            (Some(digit), Some(value)) => current = Some(value * 10 + i64::from(digit)),
            (Some(digit), None) => current = Some(i64::from(digit)),
            (None, Some(value)) => {
                out.push(value);
                current = None;
            }
            (None, None) => {}
        }
    }
    out.extend(current);
    out
}

/// What each number of a running line is: the page's number, the count of
/// pages, or just a number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Number {
    Page,
    Pages,
    Plain,
}

fn numbers_meaning(chosen: &[&Candidate], key: &str, pages: usize) -> Vec<Number> {
    let seen: Vec<(usize, Vec<i64>)> = chosen
        .iter()
        .filter(|c| c.key == key)
        .map(|c| (c.page, numbers_of(&c.line.text())))
        .collect();
    let count = seen.first().map_or(0, |(_, numbers)| numbers.len());
    (0..count)
        .map(|index| {
            let values: Vec<(i64, i64)> = seen
                .iter()
                .filter_map(|(page, numbers)| numbers.get(index).map(|&v| (*page as i64, v)))
                .collect();
            let offset = values.first().map(|(page, value)| value - page);
            if values.len() >= 2 && values.iter().all(|(page, value)| Some(value - page) == offset)
            {
                Number::Page
            } else if values.iter().all(|(_, value)| *value == pages as i64) {
                Number::Pages
            } else {
                Number::Plain
            }
        })
        .collect()
}

/// The header or footer as the first page that has it shows it: a
/// paragraph a baseline, parts of one baseline apart at the alignment
/// tabs, the page numbers fields.
fn furniture_of(chosen: &[&Candidate], first: usize, width: f64, pages: usize) -> Body {
    let mut lines: Vec<&Candidate> = chosen.iter().copied().filter(|c| c.page == first).collect();
    lines.sort_by(|a, b| {
        b.line
            .y
            .partial_cmp(&a.line.y)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.line.x0.partial_cmp(&b.line.x0).unwrap_or(std::cmp::Ordering::Equal))
    });
    let mut rows: Vec<Vec<&Candidate>> = Vec::new();
    for candidate in lines {
        match rows.last_mut() {
            Some(row) if (row[0].line.y - candidate.line.y).abs() < 1.0 => row.push(candidate),
            _ => rows.push(vec![candidate]),
        }
    }
    let mut body = Body::default();
    for row in rows {
        let mut properties = ParagraphProperties::default();
        let mut runs = Vec::new();
        if row.len() == 1 {
            let line = &row[0].line;
            let centre = (line.x0 + line.x1) / 2.0;
            if (centre - width / 2.0).abs() < width * 0.06 {
                properties.alignment = Some(Alignment::Center);
            } else if line.x0 > width / 2.0 {
                properties.alignment = Some(Alignment::End);
            }
        }
        for (index, candidate) in row.iter().enumerate() {
            if index > 0 {
                // The last part against the right margin, one between in
                // the middle.
                let alignment =
                    if index + 1 == row.len() { TabAlignment::End } else { TabAlignment::Center };
                runs.push(Run {
                    content: vec![RunContent::PositionTab(alignment)],
                    ..Run::text("")
                });
            }
            runs.extend(runs_of_line(candidate, chosen, pages));
        }
        body.blocks.push(Block::Paragraph(Paragraph { properties, runs }));
    }
    body
}

/// A running line's runs: its words as they are, its page numbers as
/// fields.
fn runs_of_line(candidate: &Candidate, chosen: &[&Candidate], pages: usize) -> Vec<Run> {
    let line = &candidate.line;
    let properties = line
        .pieces
        .iter()
        .find(|piece| !piece.is_space())
        .map(|piece| properties_of(&piece.format))
        .unwrap_or_default();
    let meanings = numbers_meaning(chosen, &candidate.key, pages);
    let text = line.text();
    let mut runs = Vec::new();
    let mut words = String::new();
    let mut number = String::new();
    let mut index = 0;
    let mut flush_number = |number: &mut String, runs: &mut Vec<Run>, words: &mut String| {
        if number.is_empty() {
            return;
        }
        let instruction = match meanings.get(index) {
            Some(Number::Page) => Some(" PAGE "),
            Some(Number::Pages) => Some(" NUMPAGES "),
            _ => None,
        };
        index += 1;
        match instruction {
            Some(instruction) => {
                if !words.is_empty() {
                    runs.push(Run { properties: properties.clone(), ..Run::text(words) });
                    words.clear();
                }
                runs.push(Run {
                    properties: properties.clone(),
                    ..Run::field(instruction, number)
                });
            }
            None => words.push_str(number),
        }
        number.clear();
    };
    for c in text.chars() {
        if c.is_ascii_digit() {
            number.push(c);
        } else {
            flush_number(&mut number, &mut runs, &mut words);
            words.push(c);
        }
    }
    flush_number(&mut number, &mut runs, &mut words);
    if !words.is_empty() {
        runs.push(Run { properties, ..Run::text(&words) });
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_running_line_is_known_whatever_its_numbers() {
        assert_eq!(key_of("Page 3 of 12"), "Page # of #");
        assert_eq!(key_of("Chapter iv"), "Chapter #");
        assert_eq!(key_of("  Annual   report  "), "Annual report");
        assert_eq!(numbers_of("Page 3 of 12"), vec![3, 12]);
    }
}
