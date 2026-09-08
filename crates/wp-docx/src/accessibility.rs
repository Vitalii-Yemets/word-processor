//! What in a document would stop somebody reading it.
//!
//! # What is checked and why each one matters
//!
//! A picture with no description is silence to a screen reader: the reader
//! reaches it, has nothing to say, and moves on — so whatever the picture was
//! showing is simply missing from the document as that person receives it.
//!
//! A heading level skipped breaks the outline. Readers navigate long documents
//! by heading, and a document that goes from a first-level heading to a
//! third-level one reads as though a section is missing.
//!
//! Text too near the colour of the page behind it is unreadable to anybody
//! whose sight or screen is less than perfect. The measure is the contrast
//! ratio the Web Content Accessibility Guidelines define, and the thresholds
//! here are theirs.
//!
//! A table with no header row is a grid of values with nothing saying what the
//! columns are, which is what a header row is for.
//!
//! A link whose text is its own address is read out one character at a time.
//!
//! # What is not checked
//!
//! Whether a description is any good. "Image" is a description and passes; only
//! a person can say whether it describes anything.

use crate::model::{Block, Table};
use crate::Document;

/// How bad a problem is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Somebody will not be able to read this at all.
    Error,
    /// Somebody will find this harder than it need be.
    Warning,
    /// Worth looking at.
    Tip,
}

impl Severity {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Error => "Error",
            Self::Warning => "Warning",
            Self::Tip => "Tip",
        }
    }
}

/// One thing found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub severity: Severity,
    /// What is wrong.
    pub problem: String,
    /// Where it is, when it is somewhere in particular.
    pub paragraph: Option<usize>,
    /// What to do about it.
    pub advice: &'static str,
}

/// The contrast the guidelines ask of ordinary text.
const CONTRAST_WANTED: f32 = 4.5;
/// And of large text, which is easier to read at lower contrast.
const CONTRAST_WANTED_LARGE: f32 = 3.0;
/// The size at which text counts as large, in half-points — 18pt.
const LARGE_HALF_POINTS: u32 = 36;

impl Document {
    /// Everything about the document that would stop somebody reading it.
    #[must_use]
    pub fn accessibility_findings(&self) -> Vec<Finding> {
        let mut out = Vec::new();

        if self.properties().title.trim().is_empty() {
            out.push(Finding {
                severity: Severity::Warning,
                problem: "The document has no title".to_owned(),
                paragraph: None,
                advice: "File ▸ Properties ▸ Title",
            });
        }

        self.check_drawings(&mut out);
        self.check_headings(&mut out);
        self.check_contrast(&mut out);
        self.check_tables(&mut out);
        self.check_links(&mut out);

        out.sort_by_key(|finding| (finding.severity, finding.paragraph.unwrap_or(0)));
        out
    }

    /// Pictures and shapes with nothing said about them.
    fn check_drawings(&self, out: &mut Vec<Finding>) {
        for shape in self.shapes() {
            // A text box says what it says: its words are the description.
            if shape.has_text() {
                continue;
            }
            if shape.description.trim().is_empty() {
                out.push(Finding {
                    severity: Severity::Error,
                    problem: format!("{} has no description", shape.name),
                    paragraph: None,
                    advice: "Describe what it shows, so a screen reader can say it",
                });
            }
        }
    }

    /// Heading levels that skip.
    fn check_headings(&self, out: &mut Vec<Finding>) {
        let mut previous: Option<u8> = None;
        for index in 0..self.paragraph_count() {
            let Some(level) = self.outline_level(index) else { continue };
            if let Some(before) = previous {
                if level > before + 1 {
                    out.push(Finding {
                        severity: Severity::Warning,
                        problem: format!("Heading {} follows heading {}", level + 1, before + 1),
                        paragraph: Some(index),
                        advice: "Use the next level down, so the outline has no gaps",
                    });
                }
            }
            previous = Some(level);
        }
    }

    /// Text too near the colour of the page behind it.
    fn check_contrast(&self, out: &mut Vec<Finding>) {
        let page = self.page_color().unwrap_or_else(|| "FFFFFF".to_owned());
        let Some(background) = luminance(&page) else { return };

        for index in 0..self.paragraph_count() {
            let Some(text) = self.paragraph_text(index) else { continue };
            if text.trim().is_empty() {
                continue;
            }
            let Some(paragraph) = self.paragraph_element(index) else { continue };
            let resolved =
                crate::format::resolved_in_range(paragraph, 0, text.len(), self.styles());

            for run in resolved {
                let Some(colour) = run.color.as_deref() else { continue };
                let Some(foreground) = luminance(colour) else { continue };
                let ratio = contrast(foreground, background);
                let wanted = if run.size_half_points >= LARGE_HALF_POINTS {
                    CONTRAST_WANTED_LARGE
                } else {
                    CONTRAST_WANTED
                };
                if ratio < wanted {
                    out.push(Finding {
                        severity: Severity::Error,
                        problem: format!(
                            "Text at {ratio:.1} to 1 against the page, where {wanted} is wanted"
                        ),
                        paragraph: Some(index),
                        advice: "Darken the text or lighten the page",
                    });
                    break;
                }
            }
        }
    }

    /// Tables whose first row is not marked as headings.
    fn check_tables(&self, out: &mut Vec<Finding>) {
        let mut number = 0usize;
        for block in &self.body().blocks {
            let Block::Table(table) = block else { continue };
            number += 1;
            if !has_header_row(table) {
                out.push(Finding {
                    severity: Severity::Warning,
                    problem: format!("Table {number} has no header row"),
                    paragraph: None,
                    advice: "Mark the first row as headings, so the columns have names",
                });
            }
        }
    }

    /// Links whose text is the address itself.
    fn check_links(&self, out: &mut Vec<Finding>) {
        for link in self.hyperlinks() {
            let text = link.text.trim();
            if text.is_empty() {
                continue;
            }
            let looks_like_an_address =
                text.starts_with("http://") || text.starts_with("https://") || text.contains("://");
            if looks_like_an_address {
                out.push(Finding {
                    severity: Severity::Tip,
                    problem: format!("A link reads as its own address: {text}"),
                    paragraph: Some(link.paragraph),
                    advice: "Say where it goes instead, so it is not read out letter by letter",
                });
            }
        }
    }
}

/// Whether a table's first row is marked as a header.
fn has_header_row(table: &Table) -> bool {
    table.rows.first().is_some_and(|row| row.is_header)
}

/// How bright a colour is, by the measure the guidelines use.
///
/// Not the average of the three: the eye is far more sensitive to green than to
/// blue, and a measure that ignores that calls yellow and blue equally bright.
#[must_use]
pub fn luminance(colour: &str) -> Option<f32> {
    let colour = colour.trim_start_matches('#');
    if colour.len() != 6 {
        return None;
    }
    let channel = |at: usize| -> Option<f32> {
        let value = u8::from_str_radix(&colour[at..at + 2], 16).ok()?;
        let value = f32::from(value) / 255.0;
        // The curve the guidelines define, which undoes the encoding a screen
        // applies before showing a colour.
        Some(if value <= 0.039_28 { value / 12.92 } else { ((value + 0.055) / 1.055).powf(2.4) })
    };
    Some(0.2126 * channel(0)? + 0.7152 * channel(2)? + 0.0722 * channel(4)?)
}

/// How far apart two brightnesses are, as the guidelines count it.
#[must_use]
pub fn contrast(first: f32, second: f32) -> f32 {
    let (lighter, darker) = if first >= second { (first, second) } else { (second, first) };
    (lighter + 0.05) / (darker + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn black_on_white_is_as_far_apart_as_two_colours_get() {
        let black = luminance("000000").expect("black");
        let white = luminance("FFFFFF").expect("white");
        assert!((contrast(black, white) - 21.0).abs() < 0.1, "got {}", contrast(black, white));
    }

    #[test]
    fn a_colour_against_itself_has_no_contrast_at_all() {
        let grey = luminance("808080").expect("grey");
        assert!((contrast(grey, grey) - 1.0).abs() < 0.001);
    }

    #[test]
    fn contrast_does_not_depend_on_which_way_round_it_is_asked() {
        let (black, white) = (luminance("000000").unwrap(), luminance("FFFFFF").unwrap());
        assert!((contrast(black, white) - contrast(white, black)).abs() < 0.001);
    }

    #[test]
    fn green_counts_for_more_than_blue() {
        let green = luminance("00FF00").expect("green");
        let blue = luminance("0000FF").expect("blue");
        assert!(green > blue * 5.0, "green {green}, blue {blue}");
    }

    #[test]
    fn light_grey_on_white_fails_the_measure_the_guidelines_set() {
        let grey = luminance("AAAAAA").expect("grey");
        let white = luminance("FFFFFF").expect("white");
        assert!(contrast(grey, white) < CONTRAST_WANTED);
    }

    #[test]
    fn a_dark_blue_on_white_passes_it() {
        let blue = luminance("1F3864").expect("blue");
        let white = luminance("FFFFFF").expect("white");
        assert!(contrast(blue, white) > CONTRAST_WANTED);
    }

    #[test]
    fn a_colour_that_is_not_six_digits_is_not_a_colour() {
        assert_eq!(luminance("nonsense"), None);
        assert_eq!(luminance("FFF"), None);
    }

    #[test]
    fn a_colour_may_be_written_with_a_hash_in_front_of_it() {
        assert_eq!(luminance("#FFFFFF"), luminance("FFFFFF"));
    }

    #[test]
    fn the_worst_problems_are_listed_first() {
        let mut findings = [
            Finding {
                severity: Severity::Tip,
                problem: "a tip".to_owned(),
                paragraph: None,
                advice: "",
            },
            Finding {
                severity: Severity::Error,
                problem: "an error".to_owned(),
                paragraph: None,
                advice: "",
            },
        ];
        findings.sort_by_key(|finding| (finding.severity, finding.paragraph.unwrap_or(0)));
        assert_eq!(findings[0].severity, Severity::Error);
    }
}
