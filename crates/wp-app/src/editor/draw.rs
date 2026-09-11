//! Putting the window on screen: the pages, then the furniture around them.

use wp_layout::PageMetrics;
use wp_raster::Canvas;

use crate::chrome::rulers::{Indents, Measurements};
use crate::chrome::status::StatusState;
use crate::chrome::{rulers, status, ToolbarState};

use super::{Editor, CARET_WIDTH, DPI, POINTS_PER_INCH, TWIPS_PER_POINT};

impl Editor {
    /// What the ribbon needs to know about the document to draw itself.
    pub(super) fn toolbar_state(&self) -> ToolbarState {
        use wp_docx::CharacterFormat;
        let indents = self.document.indents_here();
        let room = self.document.paragraph_format_here();
        ToolbarState {
            bold: self.document.format_is_on(CharacterFormat::Bold),
            italic: self.document.format_is_on(CharacterFormat::Italic),
            underline: self.document.format_is_on(CharacterFormat::Underline),
            strike: self.document.format_is_on(CharacterFormat::Strikethrough),
            subscript: self.document.format_is_on(CharacterFormat::Subscript),
            superscript: self.document.format_is_on(CharacterFormat::Superscript),
            alignment: self.document.alignment_here(),
            style: self.document.style_here(),
            styles: self.style_gallery(),
            font: self.document.font_here(),
            size: self.document.size_here(),
            zoom: self.zoom,
            can_undo: self.document.can_undo(),
            can_redo: self.document.can_redo(),
            modified: self.document.is_modified(),
            has_selection: self.document.selection().is_some(),
            show_marks: self.show_marks,
            show_proofing: self.show_proofing,
            show_rulers: self.show_rulers,
            show_navigation: self.show_navigation,
            show_gridlines: self.show_gridlines,
            joined_pages: self.joined_pages,
            view: self.view.label(),
            dark_theme: self.theme.mode == crate::chrome::Mode::Dark,
            painting: self.painter.is_some(),
            tracking_changes: self.document.tracking_changes(),
            show_markup: self.show_markup,
            painting_borders: self.painting_borders(),
            show_comments: self.navigation.section == crate::chrome::navigation::Section::Comments,
            in_table: self.document.table_here().is_some(),
            table_look: self.document.table_look().unwrap_or_default(),
            show_table_gridlines: self.show_table_gridlines,
            repeat_header_row: self.document.table_header_row().unwrap_or(false),
            header_from_top: self.document.furniture_distances().0,
            footer_from_bottom: self.document.furniture_distances().1,
            row_height: self.document.table_row_height().unwrap_or(0),
            column_width: self.document.cell_width().unwrap_or(0),
            cell_alignment: self.cell_alignment_here().map(|at| at as u8),
            in_furniture: self.in_furniture(),
            text_color: self.chosen_text_color,
            highlight_color: self.chosen_highlight_color,
            indent_left: indents.0,
            indent_right: indents.2,
            space_before: room.space_before,
            space_after: room.space_after,
            unit: self.unit,
            typing: self.ribbon_box.map(|(command, _)| (command, self.box_text.clone())),
            open: self.popup.as_ref().map(|popup| popup.choice),
        }
    }

    /// Draws the pages, and everything on them.
    pub(super) fn draw_pages(&mut self) {
        let selections = self.document.selections();

        for index in 0..self.pages.len() {
            let (origin_x, origin_y) = self.page_origin(index);
            let y = self.content_top() + origin_y - self.scroll_down();
            let page_height = self.pages[index].height;

            // Anything scrolled off the screen costs nothing but this test.
            if y + page_height < 0.0 || y > self.view_height as f32 {
                continue;
            }

            // With the white space hidden the paper is only drawn where its text
            // is, so one page runs into the next with a line between them.
            let page_width = self.pages[index].width;
            let (trim_top, _) = self.page_trim();
            let paper_top = y + trim_top;
            let paper_height = self.visible_height(&self.pages[index]);
            // The edge round the sheet, which a view that shows no paper does
            // not draw — there is no sheet for it to be the edge of.
            if self.view.shows_paper() {
                self.canvas.fill_rect(
                    origin_x as i32 - 1,
                    paper_top as i32 - 1,
                    page_width as i32 + 2,
                    paper_height as i32 + 2,
                    self.theme.page_edge,
                );
            }
            self.canvas.fill_rect(
                origin_x as i32,
                paper_top as i32,
                page_width as i32,
                paper_height as i32,
                self.page_paint(),
            );
            // Behind everything else on the sheet, which is what makes it a
            // watermark rather than a heading.
            self.draw_watermark(origin_x, paper_top, page_width, paper_height);

            if self.show_gridlines {
                self.draw_gridlines(origin_x, paper_top, page_width, paper_height);
            }

            // The selection goes under the text, not over it, so the letters
            // stay the colour they were written in. Every stretch of it: a
            // person who held Ctrl and dragged out a second one has to see
            // both, or the next thing they press will surprise them.
            for (start, end) in &selections {
                for (rect_x, rect_y, rect_width, rect_height) in
                    self.pages[index].selection_rects(*start, *end)
                {
                    self.canvas.fill_rect(
                        (origin_x + rect_x) as i32,
                        (y + rect_y) as i32,
                        rect_width.ceil() as i32,
                        rect_height.ceil() as i32,
                        self.theme.selection,
                    );
                }
            }

            let page = &self.pages[index];
            self.renderer.draw_onto(&mut self.canvas, page, origin_x, y);

            if self.show_marks {
                self.draw_marks(index, origin_x, y);
            }
        }
    }

    /// Draws the marks that are normally invisible: a paragraph mark at the end
    /// of every paragraph.
    ///
    /// Word draws these for the same reason a proofreader wants them: an empty
    /// paragraph and a page break look identical until something says which is
    /// which.
    fn draw_marks(&mut self, index: usize, origin_x: f32, origin_y: f32) {
        let page = &self.pages[index];
        let mut marks = Vec::new();

        for (line_index, line) in page.lines.iter().enumerate() {
            let last_of_paragraph =
                page.lines.get(line_index + 1).is_none_or(|next| next.paragraph != line.paragraph);
            if last_of_paragraph {
                marks.push((origin_x + line.right + 2.0, origin_y + line.baseline, line.ascent));
            }
        }

        let marks_colour = self.theme.marks;
        for (x, baseline, ascent) in marks {
            let size = (ascent * 0.8).max(6.0);
            let line = self.engine.simple_line("¶", x, baseline, size * 0.72, marks_colour);
            self.renderer.draw_onto(&mut self.canvas, &line, 0.0, 0.0);
        }
    }

    /// Draws the bar along the top, which every view of the window has.
    pub(super) fn draw_title_bar(&mut self) {
        let state = self.toolbar_state();
        let caption = format!(
            "{}{} — Word Processor",
            if state.modified { "*" } else { "" },
            self.document_name()
        );
        let quick = self.ribbon.custom.quick.clone();
        let mut titlebar = std::mem::take(&mut self.titlebar);
        let theme = self.theme;
        titlebar.draw(
            &mut self.canvas,
            &mut self.chrome_engine,
            &mut self.renderer,
            &caption,
            &quick,
            &state,
            &theme,
        );
        self.titlebar = titlebar;
    }

    /// Draws whatever list is dropped open, over everything else.
    pub(super) fn draw_open_popup(&mut self) {
        let theme = self.theme;
        if let Some(popup) = self.popup.take() {
            let mut popup = popup;
            popup.keep_inside(
                self.view_width as f32,
                crate::chrome::TITLE_HEIGHT,
                self.view_height as f32,
            );
            popup.draw(&mut self.canvas, &mut self.chrome_engine, &mut self.renderer, &theme);
            self.popup = Some(popup);
        }
    }

    /// Draws the ribbon, the rulers, the navigation pane and the status strip.
    pub(super) fn draw_chrome(&mut self) {
        let state = self.toolbar_state();
        let caption = format!(
            "{}{} — Word Processor",
            if state.modified { "*" } else { "" },
            self.document_name()
        );

        let quick = self.ribbon.custom.quick.clone();
        let mut titlebar = std::mem::take(&mut self.titlebar);
        let theme = self.theme;
        titlebar.draw(
            &mut self.canvas,
            &mut self.chrome_engine,
            &mut self.renderer,
            &caption,
            &quick,
            &state,
            &theme,
        );
        self.titlebar = titlebar;

        if self.view.shows_furniture() {
            let ribbon_top = crate::chrome::TITLE_HEIGHT;
            let mut ribbon = std::mem::take(&mut self.ribbon);
            ribbon.draw(
                &mut self.canvas,
                &mut self.chrome_engine,
                &mut self.renderer,
                ribbon_top,
                &state,
                self.hovered,
                &theme,
            );
            self.ribbon = ribbon;
        }

        if self.show_navigation {
            let contents = self.pane_contents();
            let current = match self.navigation.section {
                crate::chrome::navigation::Section::Headings => {
                    self.current_heading(&contents.headings)
                }
                _ => None,
            };
            let top = self.ribbon_bottom();
            let bottom = self.window_bottom();
            let mut navigation = std::mem::take(&mut self.navigation);
            navigation.draw(
                &mut self.canvas,
                &mut self.chrome_engine,
                &mut self.renderer,
                &contents,
                current,
                (top, bottom),
                &theme,
            );
            self.navigation = navigation;
        }

        if let Some(mut bar) = self.find_bar.take() {
            let top = self.ribbon_bottom();
            let left = self.pane_width();
            bar.draw(
                &mut self.canvas,
                &mut self.chrome_engine,
                &mut self.renderer,
                top,
                left,
                &theme,
            );
            self.find_bar = Some(bar);
        }

        // The shading behind the merge fields goes under everything: it is a
        // band on the page, not a mark on the text.
        self.draw_field_highlight();

        // The wavy lines go over the text: they are about the words, and a word
        // drawn over its own mark would hide it.
        self.draw_proofing_marks();

        // Over the page and under the chrome: the handles belong to the drawing
        // on the page, but nothing on the page may be drawn over them.
        self.draw_shape_handles();

        if self.show_rulers {
            self.draw_rulers();
        }
        self.draw_status();
    }

    /// Where the page and its margins sit on screen, for the rulers.
    ///
    /// Worked out in one place because the drawing and the hit-testing must
    /// agree exactly: a marker drawn a pixel from where it can be grabbed is a
    /// marker that cannot be dragged.
    pub(super) fn ruler_measurements(&self) -> (Measurements, rulers::Vertical) {
        let metrics = PageMetrics::from_document(&self.document);
        let pixels_per_inch = self.pixels_per_inch();
        let scale = pixels_per_inch / POINTS_PER_INCH;
        // The page being looked at, not the first one: the side ruler describes
        // the sheet in front of the reader, and on page three that is page
        // three.
        let index = self.visible_page();
        let (page_left, page_origin_y) = self.page_origin(index);
        let page_width = self.pages.get(index).map_or(0.0, |page| page.width);
        let page_height = self.pages.get(index).map_or(0.0, |page| page.height);

        let indents = self.document.indents_here();
        let to_pixels = |twips: i32| twips as f32 / TWIPS_PER_POINT * scale;
        let horizontal = Measurements {
            top: self.ruler_top(),
            left_edge: self.pane_width(),
            page_left,
            page_width,
            margin_left: metrics.margin_left * scale,
            margin_right: metrics.margin_right * scale,
            pixels_per_inch,
            indents: Indents {
                first_line: to_pixels(indents.0 + indents.1),
                start: to_pixels(indents.0),
                end: to_pixels(indents.2),
            },
        };

        let vertical = rulers::Vertical {
            top: self.whole_content_top(),
            bottom: self.window_bottom(),
            page_top: self.content_top() + page_origin_y - self.scroll_down(),
            page_height,
            margin_top: metrics.margin_top * scale,
            margin_bottom: metrics.margin_bottom * scale,
            pixels_per_inch,
        };
        (horizontal, vertical)
    }

    fn draw_rulers(&mut self) {
        let theme = self.theme;
        // Everything is worked out first: the drawing calls hold the canvas,
        // and asking the editor a question while they do is a borrow the
        // compiler will not allow — rightly, since the answer could change.
        let (horizontal, vertical) = self.ruler_measurements();
        let pane = self.pane_width();

        let stops = self.ruler_stops();
        let tabs = rulers::Tabs { stops: &stops, chosen: self.tab_kind };
        rulers::draw_horizontal(
            &mut self.canvas,
            &mut self.chrome_engine,
            &mut self.renderer,
            horizontal,
            tabs,
            &theme,
        );

        rulers::draw_vertical(
            &mut self.canvas,
            &mut self.chrome_engine,
            &mut self.renderer,
            pane,
            vertical,
            &theme,
        );
    }

    /// Draws the frame and the eight handles round the drawing at the caret.
    ///
    /// Drawn over the page rather than on it, because they are not part of the
    /// document: they are what says the drawing can be taken hold of.
    pub(super) fn draw_shape_handles(&mut self) {
        let Some((left, top, width, height)) = self.selected_shape_box() else { return };
        let colour = self.theme.accent;
        let handle = super::handles::HANDLE;

        // A dashed frame round the drawing, which is what Word draws.
        let dash = 4i32;
        let mut x = left as i32;
        while x < (left + width) as i32 {
            let run = dash.min((left + width) as i32 - x);
            self.canvas.fill_rect(x, top as i32 - 1, run, 1, colour);
            self.canvas.fill_rect(x, (top + height) as i32, run, 1, colour);
            x += dash * 2;
        }
        let mut y = top as i32;
        while y < (top + height) as i32 {
            let run = dash.min((top + height) as i32 - y);
            self.canvas.fill_rect(left as i32 - 1, y, 1, run, colour);
            self.canvas.fill_rect((left + width) as i32, y, 1, run, colour);
            y += dash * 2;
        }

        for (fx, fy) in [
            (0.0, 0.0),
            (0.5, 0.0),
            (1.0, 0.0),
            (1.0, 0.5),
            (1.0, 1.0),
            (0.5, 1.0),
            (0.0, 1.0),
            (0.0, 0.5),
        ] {
            let cx = left + width * fx;
            let cy = top + height * fy;
            let size = handle as i32;
            self.canvas.fill_rect(
                (cx - handle / 2.0) as i32 - 1,
                (cy - handle / 2.0) as i32 - 1,
                size + 2,
                size + 2,
                colour,
            );
            self.canvas.fill_rect(
                (cx - handle / 2.0) as i32,
                (cy - handle / 2.0) as i32,
                size,
                size,
                self.theme.page,
            );
        }
    }

    fn draw_status(&mut self) {
        let selected = self.document.selected_text();
        // A link says where it goes whenever the caret is in one, unless there
        // is something more pressing to say.
        let note = if self.status.is_empty() {
            self.link_note().unwrap_or_default()
        } else {
            self.status.clone()
        };
        let text = self.document.plain_text();
        let state = StatusState {
            note,
            page: self.caret_page(),
            pages: self.pages.len().max(1),
            section: self.document.section_here() + 1,
            words: count_words(&text),
            characters: text.chars().filter(|letter| *letter != '\n').count(),
            // What the text at the caret is actually marked as, which is what
            // a spelling checker would go by.
            language: wp_docx::languages::name_of(&self.document.language_here()),
            zoom: self.zoom,
            modified: self.document.is_modified(),
            selected_characters: selected.chars().count(),
            shows: self.status_shows,
        };

        let theme = self.theme;
        let (slider, buttons) = status::draw(
            &mut self.canvas,
            &mut self.chrome_engine,
            &mut self.renderer,
            &state,
            &theme,
        );
        self.slider = slider;
        self.status_buttons = buttons.to_vec();
    }

    /// Draws the caret, if it is inside the pane being drawn.
    pub(super) fn draw_caret(&mut self) {
        let Some((x, y, caret_height)) = self.caret_rect() else {
            self.under_caret = None;
            return;
        };
        // Only inside the page area: a caret scrolled up behind the ribbon
        // must not be drawn on top of it.
        if y < self.content_top() || y + caret_height > self.content_bottom() {
            self.under_caret = None;
            return;
        }

        // What is under it is kept whether it is drawn or not, so that either
        // half of the next blink can be done without drawing the window again.
        let (x, y, height) = (x as i32, y as i32, caret_height.ceil() as i32);
        let under = self.canvas.copy_rect(x, y, CARET_WIDTH, height);
        if self.caret_on {
            self.canvas.fill_rect(x, y, CARET_WIDTH, height, self.theme.caret);
        }
        self.under_caret = Some((x, y, under));
    }

    /// Draws the caret's half of a blink, and nothing else.
    ///
    /// A caret blinks twice a second for as long as the window is open. Drawing
    /// the whole window each time — every glyph of every page on screen,
    /// rasterized again — is work nobody asked for and a fan nobody wants. What
    /// was under the caret was kept when it was drawn, so putting it back is a
    /// few hundred bytes copied and no drawing at all.
    ///
    /// True when the window was changed and has to be shown again.
    pub(super) fn blink_caret(&mut self) -> bool {
        // Anything floating over the page may have been drawn on top of the
        // caret after it was drawn, and putting back what was under the caret
        // would put it back over that. So while something floats, a blink is
        // an ordinary repaint.
        if self.popup.is_some()
            || self.mini_bar.is_some()
            || self.palette.is_some()
            || self.table_grid.is_some()
            || self.tip.is_some()
            || self.showing_key_tips()
        {
            return false;
        }

        let Some((x, y, under)) = self.under_caret.take() else {
            // Nothing was kept, which means the caret was not drawn last time.
            // Then this is not a blink but a first drawing, and the caller
            // paints the window.
            return false;
        };
        let height = (under.len() / (CARET_WIDTH.max(1) as usize * 4)) as i32;
        self.canvas.paste_rect(x, y, CARET_WIDTH, height, &under);
        if self.caret_on {
            self.canvas.fill_rect(x, y, CARET_WIDTH, height, self.theme.caret);
        }
        // The pixels kept are the ones under the caret, not the caret itself,
        // so they serve both halves of every blink after this one.
        self.under_caret = Some((x, y, under));
        true
    }

    /// Draws the mark that shows where carried text would land.
    ///
    /// A caret of its own, drawn where the pointer is rather than where the
    /// document's caret is: the document's caret is still in the text being
    /// carried, and moving it would give up the selection before the person
    /// has decided anything.
    pub(super) fn draw_drop_mark(&mut self) {
        let Some(onto) = self.text_drop_target() else { return };
        let Some((x, y, height)) = self.caret_rect_at(onto) else { return };
        if y < self.content_top() || y + height > self.content_bottom() {
            return;
        }
        let colour = self.theme.accent;
        self.canvas.fill_rect(x as i32, y as i32, 2, height.ceil() as i32, colour);
    }
    /// Draws everything, in the order it has to go down in.
    pub(super) fn paint(&mut self, width: usize, height: usize) {
        // The window exists by the time anything is painted into it, which is
        // the earliest the desktop can be told anything about it.
        self.tell_desktop_the_theme();

        if self.canvas.width() != width || self.canvas.height() != height {
            self.canvas = Canvas::new(width, height);
            self.view_width = width;
            self.view_height = height;
            self.needs_redraw = true;
            self.under_caret = None;
        }

        // A blink and nothing else: put back what the caret was drawn over and
        // draw it again, rather than drawing the window.
        if !self.needs_redraw && self.caret_only {
            self.caret_only = false;
            if self.blink_caret() {
                return;
            }
            self.needs_redraw = true;
        }
        self.caret_only = false;

        if !self.needs_redraw {
            return;
        }

        self.canvas.clear(self.theme.desk);

        // The File tab is a window of its own, as Word's is, and nothing of the
        // document shows behind it.
        if self.in_backstage() {
            self.draw_title_bar();
            self.draw_backstage();
            self.draw_open_popup();
            return;
        }

        // The Print page is not a thing over the document: it is what the
        // window shows instead of it, as Word's is.
        if self.printing() {
            self.draw_title_bar();
            self.draw_print_pane();
            self.draw_open_popup();
            return;
        }

        // The document behind whatever is being edited in front of it.
        self.draw_dimmed_document();

        // The pane being edited is drawn inside its own band, so that with the
        // window split it cannot draw over the other one.
        let (top, bottom) = self.pane_band(self.active_pane);
        let previous_clip = self.canvas.set_clip(
            0,
            top as i32,
            self.view_width as i32,
            (bottom - top).max(0.0) as i32,
        );
        self.draw_pages();
        self.draw_caret();
        self.canvas.restore_clip(previous_clip);

        // And then the other view of the same document, if there is one.
        self.draw_other_pane();

        // The furniture goes last, so a page scrolled under it is covered
        // rather than showing through.
        self.draw_chrome();
        self.draw_scrollbar();
        // The styles pane goes over the rulers rather than under them: it is a
        // pane of its own, and Word's rulers stop at its edge.
        self.draw_styles_pane();
        self.draw_drop_mark();

        // An open list goes over everything, which is what makes it a list
        // dropped in front of the window rather than part of it.
        let theme = self.theme;
        // The mark the middle-button scroll is measured from.
        self.draw_autoscroll_mark();

        // The little button at the end of a paste floats over the page, and
        // under whatever it drops open.
        self.draw_paste_badge();

        // The letters over the ribbon, while Alt has put them there.
        self.draw_key_tips();

        // The mini toolbar goes under them: a list dropped from one of its
        // boxes has to be in front of the bar it came from.
        if let Some(bar) = self.mini_bar.take() {
            let state = self.toolbar_state();
            bar.draw(&mut self.canvas, &mut self.chrome_engine, &mut self.renderer, &state, &theme);
            self.mini_bar = Some(bar);
        }
        if let Some(grid) = self.table_grid {
            grid.draw(&mut self.canvas, &mut self.chrome_engine, &mut self.renderer, &theme);
        }
        if let Some(palette) = self.palette {
            palette.draw(&mut self.canvas, &mut self.chrome_engine, &mut self.renderer, &theme);
        }
        if let Some(popup) = self.popup.take() {
            let mut popup = popup;
            popup.keep_inside(self.view_width as f32, self.content_top(), self.window_bottom());
            popup.draw(&mut self.canvas, &mut self.chrome_engine, &mut self.renderer, &theme);
            self.popup = Some(popup);
        }

        // A dialog is over everything: while one is up, it is the window.
        self.draw_dialog();

        // And the tip over even that: it is the one thing that is always about
        // whatever the pointer is on this instant.
        if let Some(tip) = self.tip.take() {
            tip.draw(&mut self.canvas, &mut self.chrome_engine, &mut self.renderer, &theme);
            self.tip = Some(tip);
        }

        self.needs_redraw = false;
    }

    pub(super) fn canvas(&self) -> &Canvas {
        &self.canvas
    }
}

/// How many words a piece of text holds.
///
/// Counted the way a person would: runs of anything that is not a space. Word
/// counts the same way, which is what makes the two numbers agree.
pub(super) fn count_words(text: &str) -> usize {
    text.split_whitespace().filter(|word| word.chars().any(char::is_alphanumeric)).count()
}

/// Kept so the module can name the constant it needs.
const _: f32 = DPI;

impl Editor {
    /// The grid Word can draw over the page, for lining things up by eye.
    ///
    /// Every quarter inch, which is what Word uses, and drawn under the text
    /// rather than over it so that it never makes a word harder to read.
    fn draw_gridlines(&mut self, left: f32, top: f32, width: f32, height: f32) {
        let step = self.pixels_per_inch() / 4.0;
        if step < 3.0 {
            return;
        }
        let colour = self.theme.gridline;

        let mut x = left;
        while x < left + width {
            self.canvas.fill_rect(x as i32, top as i32, 1, height as i32, colour);
            x += step;
        }
        let mut y = top;
        while y < top + height {
            self.canvas.fill_rect(left as i32, y as i32, width as i32, 1, colour);
            y += step;
        }
    }
}
