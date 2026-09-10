//! The Styles pane, and what pressing things in it does.
//!
//! The pane itself is [`crate::chrome::stylespane`]; this is where its list is
//! filled in and where its buttons lead. The list is gathered from the document
//! every time it is drawn rather than kept alongside it: a list that is rebuilt
//! cannot go stale, and the document is the only thing that knows what styles
//! it has.

use wp_layout::TextStyle;
use wp_shell::Response;

use crate::chrome::stylespane::{Entry, Hit, Showing, WIDTH};

use super::Editor;

impl Editor {
    /// How much room the styles pane takes, which is none when it is shut.
    pub(super) fn styles_pane_width(&self) -> f32 {
        if self.show_styles {
            WIDTH
        } else {
            0.0
        }
    }

    /// Opens or shuts the pane.
    pub(super) fn toggle_styles_pane(&mut self) -> Response {
        self.show_styles = !self.show_styles;
        self.clamp_scroll();
        self.needs_redraw = true;
        Response::Redraw
    }

    /// The styles the pane lists, in the order it lists them.
    ///
    /// Word's order: the body style first, then the rest by name. What is in
    /// use is marked rather than moved, so a style does not jump about the list
    /// as the caret moves.
    pub(super) fn styles_pane_entries(&self) -> Vec<Entry> {
        let styles = self.document.styles();
        let here = self.document.style_here();
        let used = self.document.styles_in_use();
        let is_used = |id: &str| used.iter().any(|found| found.eq_ignore_ascii_case(id));

        let body = styles.resolve_run(None, &Default::default());
        let mut out = vec![Entry {
            id: None,
            name: "Normal".to_owned(),
            style: TextStyle {
                bold: body.bold,
                italic: body.italic,
                underline: body.underline.is_visible(),
                strike: body.strike,
            },
            size: body.size_half_points as f32 / 2.0,
            current: here.is_none(),
            in_use: true,
        }];

        for style in styles.all() {
            if style.kind != wp_docx::StyleKind::Paragraph || style.is_default {
                continue;
            }
            if self.styles_pane.showing == Showing::InUse && !is_used(&style.id) {
                continue;
            }
            let resolved = styles.resolve_run(Some(&style.id), &Default::default());
            out.push(Entry {
                id: Some(style.id.clone()),
                name: style.name.clone().unwrap_or_else(|| style.id.clone()),
                style: TextStyle {
                    bold: resolved.bold,
                    italic: resolved.italic,
                    underline: resolved.underline.is_visible(),
                    strike: resolved.strike,
                },
                size: resolved.size_half_points as f32 / 2.0,
                current: here.as_deref().is_some_and(|id| id.eq_ignore_ascii_case(&style.id)),
                in_use: is_used(&style.id),
            });
        }
        out
    }

    /// A press inside the pane.
    pub(super) fn styles_pane_press(&mut self, x: i32, y: i32) -> Response {
        let Some(hit) = self.styles_pane.at(x, y) else { return Response::Ignored };
        match hit {
            Hit::Close => self.toggle_styles_pane(),
            Hit::ShowPreview => {
                self.styles_pane.preview = !self.styles_pane.preview;
                self.needs_redraw = true;
                Response::Redraw
            }
            Hit::Options => {
                self.styles_pane.showing = self.styles_pane.showing.other();
                self.needs_redraw = true;
                Response::Redraw
            }
            Hit::Style(index) => {
                let Some(entry) = self.styles_pane_entries().get(index).cloned() else {
                    return Response::Ignored;
                };
                self.apply_style(entry.id.as_deref())
            }
            Hit::NewStyle => self.open_new_style(),
            Hit::Inspector => self.open_style_inspector(),
            Hit::Manage => self.open_modify_style(),
        }
    }

    /// The pointer moving over the pane.
    pub(super) fn styles_pane_hover(&mut self, x: i32, y: i32) -> bool {
        self.styles_pane.hover(x, y)
    }

    /// The wheel over the pane.
    pub(super) fn styles_pane_scroll(&mut self, rows: i32) -> bool {
        let total = self.styles_pane_entries().len();
        self.styles_pane.scroll_by(rows, total)
    }

    /// Whether a point is inside the pane at all.
    pub(super) fn over_styles_pane(&self, x: i32) -> bool {
        self.show_styles && (x as f32) >= self.styles_pane_left()
    }

    /// Where the pane's left edge is.
    pub(super) fn styles_pane_left(&self) -> f32 {
        self.view_width as f32 - WIDTH
    }

    /// Draws the pane, if it is showing.
    pub(super) fn draw_styles_pane(&mut self) {
        if !self.show_styles {
            return;
        }
        let entries = self.styles_pane_entries();
        let left = self.styles_pane_left();
        let top = self.ribbon_bottom();
        let bottom = self.window_bottom();
        let theme = self.theme;

        let mut pane = core::mem::take(&mut self.styles_pane);
        pane.draw(
            &mut self.canvas,
            &mut self.chrome_engine,
            &mut self.renderer,
            &entries,
            left,
            top,
            bottom,
            &theme,
        );
        self.styles_pane = pane;
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
        body.blocks.push(Block::Paragraph(Paragraph::text("A heading").with_style("Heading1")));
        body.blocks.push(Block::Paragraph(Paragraph::text("Body text")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor
    }

    #[test]
    fn the_pane_lists_the_body_style_first() {
        // Every document has it, and it is the one a person goes back to.
        let editor = editor();
        let entries = editor.styles_pane_entries();
        assert_eq!(entries.first().map(|entry| entry.name.clone()), Some("Normal".to_owned()));
        assert_eq!(entries[0].id, None);
    }

    #[test]
    fn each_style_carries_what_it_looks_like() {
        let editor = editor();
        let entries = editor.styles_pane_entries();
        let heading = entries
            .iter()
            .find(|entry| entry.id.as_deref() == Some("Heading1"))
            .expect("the document has a heading style");
        // A heading is bigger than the body text, which is what the preview is
        // for showing.
        assert!(heading.size > entries[0].size, "the heading is not larger");
    }

    #[test]
    fn the_style_the_caret_is_in_is_the_one_marked() {
        let mut editor = editor();
        editor.document.set_caret(wp_docx::TextPosition::new(0, 0));
        let entries = editor.styles_pane_entries();
        let current: Vec<&str> =
            entries.iter().filter(|entry| entry.current).map(|entry| entry.name.as_str()).collect();
        assert_eq!(current.len(), 1, "exactly one style is in force: {current:?}");
    }

    #[test]
    fn in_use_lists_only_what_the_document_has() {
        let mut editor = editor();
        let all = editor.styles_pane_entries().len();

        editor.styles_pane.showing = Showing::InUse;
        let used = editor.styles_pane_entries();
        assert!(used.len() < all, "the shorter list is not shorter");
        // And every one of them is in use, which is the whole promise.
        assert!(used.iter().all(|entry| entry.in_use));
    }

    #[test]
    fn the_pane_takes_room_only_when_it_is_open() {
        let mut editor = editor();
        assert_eq!(editor.styles_pane_width(), 0.0);
        editor.toggle_styles_pane();
        assert_eq!(editor.styles_pane_width(), WIDTH);
    }
}
