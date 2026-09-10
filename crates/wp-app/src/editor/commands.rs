//! The commands that needed more than a line each.
//!
//! The colour palettes, the format painter, the symbol list, sorting, list
//! levels, and the two view modes that change how the pages are stacked.

use wp_docx::furniture::{Furniture, Preset};
use wp_docx::model::{Alignment, BorderEdge, ParagraphBorders};
use wp_docx::TextPosition;
use wp_shell::Response;

use crate::chrome::palette::{Kind as PaletteKind, Palette};
use crate::chrome::{Choice, Command, Popup};

use super::Editor;

impl Editor {
    /// Drops open one of the three colour palettes under its button.
    pub(super) fn open_palette(&mut self, kind: PaletteKind) -> Response {
        if self.palette.is_some_and(|palette| palette.kind == kind) {
            self.palette = None;
            self.needs_redraw = true;
            return Response::Redraw;
        }
        let button = match kind {
            PaletteKind::Text => Command::TextColor,
            PaletteKind::Highlight => Command::Highlight,
            PaletteKind::Shading => Command::Shading,
            PaletteKind::Page => Command::PageColor,
        };
        // Under whatever asked for it: the ribbon button, or the mini
        // toolbar when the press came from there.
        let anchor = self.popup_anchor.or_else(|| self.ribbon.command_rect(button));
        let Some((left, top, _)) = anchor else {
            return Response::Ignored;
        };
        self.palette = Some(Palette::new(kind, left, top));
        self.popup = None;
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Applies whichever swatch of the open palette was pressed.
    pub(super) fn choose_color(&mut self, index: usize) -> Response {
        let Some(palette) = self.palette.take() else { return Response::Ignored };
        self.needs_redraw = true;
        let Some((name, value)) = palette.entries().get(index).copied() else {
            return Response::Redraw;
        };

        match palette.kind {
            PaletteKind::Highlight => {
                self.chosen_highlight_color = value
                    .and_then(crate::chrome::palette::highlight_color)
                    .unwrap_or(self.chosen_highlight_color);
                let changed = self.document.set_highlight(value);
                self.finish_character_change(changed, name)
            }
            PaletteKind::Text => {
                self.chosen_text_color =
                    value.and_then(wp_raster::Color::from_hex).unwrap_or(self.theme.page_text);
                let changed = self.document.set_color(value);
                self.finish_character_change(changed, name)
            }
            PaletteKind::Shading => {
                let changed = self.document.set_shading_here(value);
                self.edited(changed, &format!("Shading: {name}"))
            }
            PaletteKind::Page => {
                let changed = self.document.set_page_color(value);
                self.relayout();
                self.edited(changed, &format!("Page colour: {name}"))
            }
        }
    }

    /// Picks up the formatting at the caret, or puts down what it is holding.
    ///
    /// Word's format painter is a mode, not an action: one press arms it, the
    /// next selection it is used on receives the formatting, and it disarms.
    pub(super) fn toggle_format_painter(&mut self) -> Response {
        if self.painter.take().is_some() {
            self.needs_redraw = true;
            return self.report("Format painter put down");
        }
        self.painter = Some(self.document.run_formatting_here());
        self.needs_redraw = true;
        self.report("Format painter picked up — now select the text to paint")
    }

    /// Puts the painter's formatting onto whatever has just been selected.
    ///
    /// Called when a drag ends, which is the moment Word applies it.
    pub(super) fn apply_format_painter(&mut self) -> bool {
        let Some(formatting) = self.painter.take() else { return false };
        if self.document.selection().is_none() {
            self.painter = Some(formatting);
            return false;
        }
        let changed = self.document.apply_run_formatting(&formatting);
        if changed {
            self.relayout();
            self.status = "Formatting painted".to_owned();
        }
        self.needs_redraw = true;
        changed
    }

    /// Sorts the selected paragraphs into order.
    ///
    /// Word sorts by the text of each paragraph, ignoring case, which is what a
    /// person means by "sort this list".
    pub(super) fn sort_selection(&mut self) -> Response {
        let Some((start, end)) = self.document.selection() else {
            return self.report("Select the paragraphs to sort first");
        };
        if end.paragraph <= start.paragraph {
            return self.report("Select more than one paragraph to sort");
        }

        let mut lines: Vec<String> = (start.paragraph..=end.paragraph)
            .filter_map(|index| self.document.paragraph_text(index))
            .collect();
        if lines.len() < 2 {
            return Response::Ignored;
        }
        lines.sort_by_key(|line| line.to_lowercase());

        // Replacing the selection with the sorted text loses any formatting
        // that differed between the paragraphs, which is worth saying rather
        // than hiding.
        self.document.move_caret(TextPosition::new(start.paragraph, 0), false);
        let end_offset = self.document.paragraph_text(end.paragraph).map_or(0, |text| text.len());
        self.document.move_caret(TextPosition::new(end.paragraph, end_offset), true);

        let changed = self.document.paste(&lines.join("\n"));
        self.edited(changed, "Sorted")
    }

    /// Moves the paragraph one level deeper in its list, or back to the top.
    pub(super) fn step_list_level(&mut self, deeper: bool) -> Response {
        let Some(reference) = self.document.list_here() else {
            // Nothing is a list yet, so this starts one.
            return self.toggle_list(wp_docx::BULLET_LIST, "Bulleted list");
        };
        let levels = u8::try_from(wp_docx::LIST_LEVELS).unwrap_or(3);
        // Word stops at the ends rather than wrapping round: a list indented
        // once too often should stay where it is, not jump back to the margin.
        let next = if deeper {
            (reference.level + 1).min(levels - 1)
        } else {
            reference.level.saturating_sub(1)
        };
        let changed = self
            .document
            .set_list_here(Some(wp_docx::model::NumberingReference { level: next, ..reference }));
        self.edited(changed, &format!("List level {}", next + 1))
    }

    /// Collapses the space between one page and the next, or restores it.
    ///
    /// Word calls this "hide white space", and it is what makes a long document
    /// readable on a screen: the margins and the gap between pages are paper
    /// facts, and on a screen they are just distance between one line and the
    /// next.
    pub(super) fn toggle_joined_pages(&mut self) -> Response {
        self.joined_pages = !self.joined_pages;
        self.clamp_scroll();
        self.needs_redraw = true;
        self.report(if self.joined_pages { "White space hidden" } else { "White space shown" })
    }
}

impl Editor {
    /// Which join between two pages a point is on, if it is on one.
    ///
    /// Word puts a target here: double-clicking it hides the white space, and
    /// double-clicking again brings it back. The band is generous because it is
    /// a thin thing to hit and missing it does nothing visible.
    pub(super) fn between_pages(&self, x: i32, y: i32) -> Option<usize> {
        const REACH: f32 = 7.0;

        let (fx, fy) = (x as f32, y as f32);
        if fy < self.content_top() || fy >= self.content_bottom() {
            return None;
        }

        for index in 0..self.pages.len() {
            let (origin_x, origin_y) = self.page_origin(index);
            let width = self.pages[index].width;
            if fx < origin_x || fx > origin_x + width {
                continue;
            }
            let (trim_top, _) = self.page_trim();
            let top = self.content_top() + origin_y - self.scroll_down() + trim_top;
            let bottom = top + self.visible_height(&self.pages[index]);

            // The gap below this page, and the one above the first.
            if index == 0 && (fy - top).abs() <= REACH {
                return Some(index);
            }
            if (fy - bottom).abs() <= REACH {
                return Some(index);
            }
        }
        None
    }
}

impl Editor {
    /// Selects the word under a point, which is what a double click does.
    ///
    /// Which word is not quite "the one the offset falls in": a click on the
    /// right half of the last letter of `hello` puts the caret after it, and
    /// the word wanted is still `hello`, not the comma that follows.
    pub(super) fn select_word_at(&mut self, x: i32, y: i32) -> Response {
        let Some(position) = self.position_at(x, y) else { return Response::Ignored };
        let Some(text) = self.document.paragraph_text(position.paragraph) else {
            return Response::Ignored;
        };

        let offset = position.offset.min(text.len());
        let mut word = wp_segment::word_at(&text, offset);
        if word.start == offset && offset > 0 {
            let earlier = wp_segment::word_at(&text, offset - 1);
            if !is_gap(&text[earlier.clone()]) {
                word = earlier;
            }
        }

        // A double click in the white space between two words selects nothing
        // rather than selecting the gap.
        if word.is_empty() || is_gap(&text[word.clone()]) {
            return Response::Ignored;
        }
        self.document.move_caret(TextPosition::new(position.paragraph, word.start), false);
        self.document.move_caret(TextPosition::new(position.paragraph, word.end), true);
        self.needs_redraw = true;
        Response::Redraw
    }
}

/// What the borders menu offers, in Word's order.
///
/// Word's own list is longer — it has the diagonals and the table-only edges —
/// but these are the ones that apply to a paragraph, which is what the button
/// on the Home tab is about.
pub(super) const BORDER_CHOICES: &[(&str, BorderChoice)] = &[
    ("Bottom Border", BorderChoice::Edge(BorderEdge::Bottom)),
    ("Top Border", BorderChoice::Edge(BorderEdge::Top)),
    ("Left Border", BorderChoice::Edge(BorderEdge::Start)),
    ("Right Border", BorderChoice::Edge(BorderEdge::End)),
    ("No Border", BorderChoice::None),
    ("All Borders", BorderChoice::All),
    ("Outside Borders", BorderChoice::Outside),
];

/// One entry of that menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BorderChoice {
    Edge(BorderEdge),
    None,
    All,
    Outside,
}

impl BorderChoice {
    /// The borders this entry stands for.
    fn borders(self) -> ParagraphBorders {
        match self {
            Self::Edge(edge) => ParagraphBorders::only(edge),
            Self::None => ParagraphBorders::default(),
            Self::All => ParagraphBorders::box_all(),
            // The same as all borders, minus the one drawn between two
            // neighbouring paragraphs.
            Self::Outside => ParagraphBorders { between: None, ..ParagraphBorders::box_all() },
        }
    }
}

impl Editor {
    /// Drops open the borders menu.
    pub(super) fn open_borders(&mut self) -> Response {
        if self.popup.as_ref().is_some_and(|popup| popup.choice == Choice::Border) {
            self.popup = None;
            self.needs_redraw = true;
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::Borders) else {
            return Response::Ignored;
        };
        let items = BORDER_CHOICES.iter().map(|(label, _)| (*label).to_owned()).collect();
        self.popup = Some(Popup::new(Choice::Border, items, None, left, top, 150.0));
        self.palette = None;
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Puts the chosen borders round the selected paragraphs.
    pub(super) fn choose_border(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some((label, choice)) = BORDER_CHOICES.get(index).copied() else {
            return Response::Ignored;
        };
        let changed = self.document.set_borders_here(&choice.borders());
        self.edited(changed, label)
    }
}

/// What the header, footer and page-number menus offer.
///
/// Word calls these a gallery and fills it with designs; without a designer the
/// useful ones are the four that say something and the one that takes the whole
/// thing away again.
pub(super) const FURNITURE_CHOICES: &[(&str, Preset, Alignment)] = &[
    ("Page number, centred", Preset::PageNumber, Alignment::Center),
    ("Page number, right", Preset::PageNumber, Alignment::End),
    ("Page X of Y, centred", Preset::PageOfTotal, Alignment::Center),
    ("Document name, centred", Preset::Text, Alignment::Center),
    ("Document name, left", Preset::Text, Alignment::Start),
    ("Empty", Preset::Blank, Alignment::Start),
    ("Remove", Preset::None, Alignment::Start),
];

impl Editor {
    /// Drops open the menu of ready-made headers or footers.
    pub(super) fn open_furniture(&mut self, which: Furniture) -> Response {
        if self.popup.as_ref().is_some_and(|popup| popup.choice == Choice::Furniture) {
            self.popup = None;
            self.needs_redraw = true;
            return Response::Redraw;
        }
        let button = match which {
            Furniture::Header => Command::Header,
            Furniture::Footer => Command::Footer,
        };
        let Some((left, top, _)) = self.ribbon.command_rect(button) else {
            return Response::Ignored;
        };

        self.choosing_furniture = which;
        let items = FURNITURE_CHOICES.iter().map(|(label, ..)| (*label).to_owned()).collect();
        self.popup = Some(Popup::new(Choice::Furniture, items, None, left, top, 190.0));
        self.palette = None;
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Puts the chosen header or footer on the document.
    pub(super) fn choose_furniture(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some((label, preset, alignment)) = FURNITURE_CHOICES.get(index).copied() else {
            return Response::Ignored;
        };
        let which = self.choosing_furniture;
        let caption = self.document_name();

        match self.document.set_furniture(which, preset, alignment, &caption) {
            Ok(changed) => {
                let name = match which {
                    Furniture::Header => "Header",
                    Furniture::Footer => "Footer",
                };
                self.edited(changed, &format!("{name}: {label}"))
            }
            Err(error) => {
                wp_shell::dialog::show_error(&format!("Cannot set that: {error}"));
                Response::Ignored
            }
        }
    }
}

/// Whether a piece of text is white space rather than a word.
fn is_gap(piece: &str) -> bool {
    piece.chars().all(char::is_whitespace)
}
