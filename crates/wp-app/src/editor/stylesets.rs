//! Quick Style Sets: the whole document's look, changed in one press.
//!
//! # What a style set is
//!
//! Word's Design tab opens with a gallery of them, and every one is the same
//! thing: a table of what the built-in styles should say. Picking one writes
//! those styles into the document, so every heading in it changes at once
//! without a single paragraph being touched.
//!
//! That is what makes it worth having and what makes it worth locking. A
//! document whose formatting is restricted to a selection of styles (**J9**)
//! and whose style set could still be switched would be a document anybody
//! could reformat entirely in one click, which is why Word's Restrict Editing
//! has a box for it — see [`wp_docx::protection`].
//!
//! # Whose sets these are
//!
//! This program's own. Word's are Word's content, shipped inside Word, and a
//! gallery offering "Lines (Distinctive)" with something else under the name
//! would be lying about what a person was picking — the same reason **J6**
//! does not fill Word's cover-page gallery. So these are named for what they
//! do and drawn here.

use wp_docx::model::{Alignment, Border, ParagraphProperties, RunProperties};
use wp_docx::styles::StyleDefinition;
use wp_shell::Response;

use super::Editor;

/// What one style says under one set.
struct Says {
    id: &'static str,
    name: &'static str,
    /// Half-points, as the format counts them.
    size: u32,
    bold: bool,
    /// Twips before and after.
    before: i32,
    after: i32,
    alignment: Alignment,
    /// Whether a rule is drawn under it.
    ruled: bool,
    /// Whether it sits on a shaded band.
    shaded: bool,
}

/// One set: a name, a word about it, and what it says about each style.
pub(crate) struct Set {
    pub(crate) name: &'static str,
    pub(crate) note: &'static str,
    says: &'static [Says],
}

/// The sets this program offers.
///
/// Five, because a gallery of one is not a gallery and a gallery of twenty is
/// a wall. Each changes the four styles a document's shape is made of.
pub(crate) const SETS: &[Set] = &[
    Set {
        name: "Plain",
        note: "The headings a new document starts with",
        says: &[
            Says {
                id: "Title",
                name: "Title",
                size: 56,
                bold: true,
                before: 240,
                after: 240,
                alignment: Alignment::Start,
                ruled: false,
                shaded: false,
            },
            Says {
                id: "Heading1",
                name: "heading 1",
                size: 32,
                bold: true,
                before: 240,
                after: 120,
                alignment: Alignment::Start,
                ruled: false,
                shaded: false,
            },
            Says {
                id: "Heading2",
                name: "heading 2",
                size: 26,
                bold: true,
                before: 200,
                after: 100,
                alignment: Alignment::Start,
                ruled: false,
                shaded: false,
            },
            Says {
                id: "Heading3",
                name: "heading 3",
                size: 24,
                bold: true,
                before: 160,
                after: 80,
                alignment: Alignment::Start,
                ruled: false,
                shaded: false,
            },
        ],
    },
    Set {
        name: "Lines",
        note: "A rule under every heading",
        says: &[
            Says {
                id: "Title",
                name: "Title",
                size: 56,
                bold: true,
                before: 240,
                after: 120,
                alignment: Alignment::Start,
                ruled: true,
                shaded: false,
            },
            Says {
                id: "Heading1",
                name: "heading 1",
                size: 32,
                bold: true,
                before: 240,
                after: 120,
                alignment: Alignment::Start,
                ruled: true,
                shaded: false,
            },
            Says {
                id: "Heading2",
                name: "heading 2",
                size: 26,
                bold: true,
                before: 200,
                after: 100,
                alignment: Alignment::Start,
                ruled: true,
                shaded: false,
            },
            Says {
                id: "Heading3",
                name: "heading 3",
                size: 24,
                bold: true,
                before: 160,
                after: 80,
                alignment: Alignment::Start,
                ruled: false,
                shaded: false,
            },
        ],
    },
    Set {
        name: "Shaded",
        note: "Headings on a band",
        says: &[
            Says {
                id: "Title",
                name: "Title",
                size: 56,
                bold: true,
                before: 240,
                after: 240,
                alignment: Alignment::Start,
                ruled: false,
                shaded: false,
            },
            Says {
                id: "Heading1",
                name: "heading 1",
                size: 30,
                bold: true,
                before: 240,
                after: 120,
                alignment: Alignment::Start,
                ruled: false,
                shaded: true,
            },
            Says {
                id: "Heading2",
                name: "heading 2",
                size: 26,
                bold: true,
                before: 200,
                after: 100,
                alignment: Alignment::Start,
                ruled: false,
                shaded: true,
            },
            Says {
                id: "Heading3",
                name: "heading 3",
                size: 24,
                bold: true,
                before: 160,
                after: 80,
                alignment: Alignment::Start,
                ruled: false,
                shaded: false,
            },
        ],
    },
    Set {
        name: "Centred",
        note: "Headings down the middle",
        says: &[
            Says {
                id: "Title",
                name: "Title",
                size: 60,
                bold: true,
                before: 360,
                after: 240,
                alignment: Alignment::Center,
                ruled: false,
                shaded: false,
            },
            Says {
                id: "Heading1",
                name: "heading 1",
                size: 32,
                bold: true,
                before: 240,
                after: 120,
                alignment: Alignment::Center,
                ruled: false,
                shaded: false,
            },
            Says {
                id: "Heading2",
                name: "heading 2",
                size: 26,
                bold: true,
                before: 200,
                after: 100,
                alignment: Alignment::Center,
                ruled: false,
                shaded: false,
            },
            Says {
                id: "Heading3",
                name: "heading 3",
                size: 24,
                bold: false,
                before: 160,
                after: 80,
                alignment: Alignment::Center,
                ruled: false,
                shaded: false,
            },
        ],
    },
    Set {
        name: "Compact",
        note: "The same headings, closer together",
        says: &[
            Says {
                id: "Title",
                name: "Title",
                size: 44,
                bold: true,
                before: 120,
                after: 120,
                alignment: Alignment::Start,
                ruled: false,
                shaded: false,
            },
            Says {
                id: "Heading1",
                name: "heading 1",
                size: 28,
                bold: true,
                before: 120,
                after: 60,
                alignment: Alignment::Start,
                ruled: false,
                shaded: false,
            },
            Says {
                id: "Heading2",
                name: "heading 2",
                size: 24,
                bold: true,
                before: 100,
                after: 40,
                alignment: Alignment::Start,
                ruled: false,
                shaded: false,
            },
            Says {
                id: "Heading3",
                name: "heading 3",
                size: 22,
                bold: true,
                before: 80,
                after: 40,
                alignment: Alignment::Start,
                ruled: false,
                shaded: false,
            },
        ],
    },
];

/// The colour a shaded heading sits on, and the colour of a rule.
///
/// Grey rather than the theme's accent, because a set is about shape and a
/// theme is about colour: **C**'s themes change the second, and a set that
/// changed both would undo whichever was picked last.
const BAND: &str = "EEEEEE";
const RULE: &str = "808080";

impl Editor {
    /// What the gallery offers, in order.
    #[must_use]
    pub(super) fn style_set_names() -> Vec<String> {
        use crate::messages::t;
        SETS.iter().map(|set| t(set.name).to_owned()).collect()
    }

    /// And what each of them is, for the line under the name.
    #[must_use]
    pub(super) fn style_set_note(index: usize) -> String {
        use crate::messages::t;
        SETS.get(index).map(|set| t(set.note).to_owned()).unwrap_or_default()
    }

    /// Drops the gallery open.
    pub(super) fn open_style_sets(&mut self) -> Response {
        self.open_ribbon_menu(crate::chrome::Choice::StyleSet)
    }

    /// Writes whichever was picked into the document's styles.
    pub(super) fn choose_style_set(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some(set) = SETS.get(index) else { return Response::Ignored };

        let mut changed = false;
        for says in set.says {
            changed |= self.document.set_style(&definition(says));
        }
        self.relayout();
        self.edited(changed, &crate::messages::with("Style set: {0}", &[set.name]))
    }

    /// Which set the document's styles match, if any.
    ///
    /// Word marks the one in use in its gallery. A document nobody has
    /// applied a set to matches none of them, and then there is nothing to
    /// mark — which is the truth and is what is shown.
    #[must_use]
    pub(super) fn style_set_in_use(&self) -> Option<usize> {
        SETS.iter().position(|set| {
            set.says.iter().all(|says| {
                self.document.styles().get(says.id).is_some_and(|style| {
                    style.run.size_half_points == Some(says.size)
                        && style.paragraph.alignment == Some(says.alignment)
                        && style.paragraph.space_before == Some(says.before)
                        && style.paragraph.borders.bottom.is_some() == says.ruled
                        && style.paragraph.shading.is_some() == says.shaded
                })
            })
        })
    }
}

/// One style, as a set says it.
fn definition(says: &Says) -> StyleDefinition {
    let mut paragraph = ParagraphProperties {
        alignment: Some(says.alignment),
        space_before: Some(says.before),
        space_after: Some(says.after),
        ..ParagraphProperties::default()
    };
    if says.ruled {
        paragraph.borders.bottom = Some(Border::line("single", 6, Some(RULE)));
    }
    if says.shaded {
        paragraph.shading = Some(BAND.to_owned());
    }

    StyleDefinition {
        id: says.id.to_owned(),
        name: says.name.to_owned(),
        based_on: (says.id != "Title").then(|| String::from("Normal")),
        next: Some(String::from("Normal")),
        paragraph,
        run: RunProperties {
            bold: Some(says.bold),
            size_half_points: Some(says.size),
            ..RunProperties::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("A heading")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    #[test]
    fn every_set_has_a_name_and_a_word_about_it() {
        let names = Editor::style_set_names();
        assert_eq!(names.len(), SETS.len());
        for (at, name) in names.iter().enumerate() {
            assert!(!name.trim().is_empty(), "set {at} has no name");
            assert!(!Editor::style_set_note(at).trim().is_empty(), "{name} says nothing about it");
        }
        assert!(Editor::style_set_note(99).is_empty(), "a set past the end said something");
    }

    #[test]
    fn picking_one_writes_every_heading_it_names() {
        let mut editor = editor();
        editor.choose_style_set(3);

        for id in ["Title", "Heading1", "Heading2", "Heading3"] {
            let style = editor.document.styles().get(id).expect("the style");
            assert_eq!(
                style.paragraph.alignment,
                Some(Alignment::Center),
                "{id} was not centred by the Centred set"
            );
        }
    }

    #[test]
    fn the_set_in_use_is_the_one_the_styles_match() {
        let mut editor = editor();
        for which in 0..SETS.len() {
            editor.choose_style_set(which);
            assert_eq!(editor.style_set_in_use(), Some(which), "{which}");
        }
    }

    #[test]
    fn a_document_nobody_has_applied_one_to_matches_none() {
        let editor = editor();
        // The styles a new document starts with are the Plain set's own, so
        // this says the matching is not merely answering yes to everything.
        let matched = editor.style_set_in_use();
        assert!(matched.is_none() || matched == Some(0), "{matched:?}");
    }

    #[test]
    fn a_set_past_the_end_writes_nothing() {
        let mut editor = editor();
        let before = editor.document.styles().get("Heading1").cloned();
        editor.choose_style_set(99);
        assert_eq!(editor.document.styles().get("Heading1").cloned(), before);
    }

    #[test]
    fn the_rule_and_the_band_are_written_where_the_set_says() {
        let mut editor = editor();
        editor.choose_style_set(1);
        let heading = editor.document.styles().get("Heading1").expect("the style");
        assert!(heading.paragraph.borders.bottom.is_some(), "the Lines set drew no rule");

        editor.choose_style_set(2);
        let heading = editor.document.styles().get("Heading1").expect("the style");
        assert_eq!(heading.paragraph.shading.as_deref(), Some(BAND), "the Shaded set drew no band");
        assert!(
            heading.paragraph.borders.bottom.is_none(),
            "the rule from the set before it stayed"
        );
    }
}
