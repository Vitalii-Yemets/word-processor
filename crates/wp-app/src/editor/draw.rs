//! Putting the window on screen: the pages, then the furniture around them.

use wp_raster::Canvas;

use crate::chrome::rulers::{Indents, Measurements};
use crate::chrome::status::StatusState;
use crate::chrome::{rulers, status, ToolbarState};

use super::{Editor, DPI, POINTS_PER_INCH, TWIPS_PER_POINT};

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
            style_name: self.style_name_here(),
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
            drawing_table: self.holding_table_pen(super::borderpainter::TablePen::Draw),
            erasing: self.holding_table_pen(super::borderpainter::TablePen::Erase),
            show_comments: self.navigation.section == crate::chrome::navigation::Section::Comments,
            design_mode: self.design_mode,
            restricted: self.restriction_now(),
            limits: self.document.formatting_limits(),
            can_edit_here: !self.is_locked(),
            in_table: self.document.table_here().is_some(),
            in_diagram: self.chosen_diagram().is_some(),
            in_outline: self.view == super::views::View::Outline,
            outline_level: self.paragraph_level_name(),
            show_level: self.show_level_name(),
            outline_formatting: self.outlining.show_formatting,
            outline_first_line: self.outlining.first_line_only,
            text_pane_open: self.show_text_pane,
            pen_in_hand: self.ink_tool()
                == Some(super::inking::InkTool::Draw(super::inking::PenKind::Pen)),
            pencil_in_hand: self.ink_tool()
                == Some(super::inking::InkTool::Draw(super::inking::PenKind::Pencil)),
            highlighter_in_hand: self.ink_tool()
                == Some(super::inking::InkTool::Draw(super::inking::PenKind::Highlighter)),
            ink_eraser_in_hand: matches!(
                self.ink_tool(),
                Some(super::inking::InkTool::StrokeEraser | super::inking::InkTool::PointEraser)
            ),
            choosing_drawings: self.choosing_drawings(),
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
        // A block of cells is shown as the cells themselves rather than as the
        // text in them, which is how Word shows it — and the only way an empty
        // cell can be seen to be selected at all. So are the rows a selection
        // takes whole by running out of a table, and the text round them is
        // shown as text.
        let super::tablework::DrawnSelection { cells, text: selections } =
            self.selection_as_drawn();

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

            // The cells of a block, whole: the lines inside them are not drawn
            // over again, because the cell covers them.
            for (_, cell) in cells.iter().filter(|(page, _)| *page == index) {
                self.canvas.fill_rect(
                    (origin_x + cell.x) as i32,
                    (y + cell.y) as i32,
                    cell.width.ceil() as i32,
                    cell.height.ceil() as i32,
                    self.theme.selection,
                );
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

        // The outline's marks beside its paragraphs, which are not part of
        // the document either: they are what a heading is folded and moved
        // by.
        self.draw_outline();

        // A table's own two handles, over everything on the page: they are not
        // part of the document, they are what the pointer takes hold of.
        self.draw_table_handles();
        self.draw_insert_spot();
    }

    /// The circled plus that puts a row or a column in, beside the line it is
    /// about.
    ///
    /// Only while the pointer is near that line, which is when Word shows it:
    /// a button drawn beside every line of every table would be a row of
    /// buttons nobody asked for.
    fn draw_insert_spot(&mut self) {
        let Some(spot) = self.insert_spot() else { return };
        let size = super::tablespots::SPOT;
        let accent = self.theme.accent;
        let middle_x = spot.x + size / 2.0;
        let middle_y = spot.y + size / 2.0;

        self.draw_round_handle(middle_x, middle_y, size / 2.0, accent);
        // The plus inside it, drawn from the middle out so it stays centred
        // whatever the size comes to.
        let arm = (size / 2.0 - 3.0).max(2.0);
        self.canvas.fill_rect(
            (middle_x - arm) as i32,
            middle_y as i32,
            (arm * 2.0) as i32,
            1,
            accent,
        );
        self.canvas.fill_rect(
            middle_x as i32,
            (middle_y - arm) as i32,
            1,
            (arm * 2.0) as i32,
            accent,
        );
    }

    /// The square that moves a table and the one that resizes it.
    ///
    /// Word draws both just outside the corners of the table, so they are over
    /// the margin rather than over the text. While the move handle is being
    /// dragged, a line shows where the table would land: a table cannot be
    /// dragged about under the hand — it would take the text it passed through
    /// with it — so what moves during the drag is the mark, not the table.
    fn draw_table_handles(&mut self) {
        let handles = self.table_handles();
        if handles.is_empty() {
            return;
        }
        let size = super::tablehandles::HANDLE;
        let accent = self.theme.accent;
        let inside = self.theme.page;

        for (handle, x, y) in handles {
            let (left, top, span) = (x as i32, y as i32, size as i32);
            self.canvas.fill_rect(left, top, span, span, accent);
            self.canvas.fill_rect(left + 1, top + 1, span - 2, span - 2, inside);

            let middle = span / 2;
            match handle {
                // A cross, which is what a four-arrows pointer comes to at
                // eleven pixels across.
                super::tablehandles::TableHandle::Move => {
                    self.canvas.fill_rect(left + middle, top + 2, 1, span - 4, accent);
                    self.canvas.fill_rect(left + 2, top + middle, span - 4, 1, accent);
                }
                // And a corner, pointing the way the drag goes.
                super::tablehandles::TableHandle::Resize => {
                    self.canvas.fill_rect(left + 2, top + span - 3, span - 4, 1, accent);
                    self.canvas.fill_rect(left + span - 3, top + 2, 1, span - 4, accent);
                }
            }
        }

        self.draw_table_landing();
    }

    /// Where a table being dragged by its handle would land.
    fn draw_table_landing(&mut self) {
        let Some(drag) = self.handle_drag.clone() else { return };
        if drag.handle != super::tablehandles::TableHandle::Move || !drag.moved {
            return;
        }
        let (x, y) = (self.pointer_x as i32, self.pointer_y as i32);
        let Some(at) = self.position_at(x, y) else { return };
        let Some((page, line)) = self.line_of_paragraph(at.paragraph) else { return };

        let (origin_x, origin_y) = self.page_origin(page);
        let top = self.content_top() + origin_y - self.scroll_down();
        let found = &self.pages[page].lines[line];
        let (left, width) = (origin_x + found.left, self.pages[page].width * 0.6);
        let at_y = top + found.top();
        let colour = self.theme.accent;
        self.canvas.fill_rect(left as i32, at_y as i32, width as i32, 2, colour);
    }

    /// The first line of a paragraph, as a page and a line on it.
    fn line_of_paragraph(&self, paragraph: usize) -> Option<(usize, usize)> {
        self.pages.iter().enumerate().find_map(|(page_index, page)| {
            page.lines
                .iter()
                .position(|line| line.paragraph == paragraph)
                .map(|line| (page_index, line))
        })
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

        if let Some(mut pane) = self.recovery.take() {
            let top = self.ribbon_bottom();
            let bottom = self.window_bottom();
            pane.draw(
                &mut self.canvas,
                &mut self.chrome_engine,
                &mut self.renderer,
                (top, bottom),
                &theme,
            );
            self.recovery = Some(pane);
        } else if self.show_navigation {
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

        if let Some(mut bar) = self.info_bar.take() {
            let top = self.ribbon_bottom();
            let left = self.pane_width();
            let width = self.view_width as f32 - left;
            bar.draw(
                &mut self.canvas,
                &mut self.chrome_engine,
                &mut self.renderer,
                top,
                left,
                width,
                &theme,
            );
            self.info_bar = Some(bar);
        }
        // The second bar goes under the first, as Word stacks them.
        if let Some(mut bar) = self.signatures_bar.take() {
            let above = if self.info_bar.is_some() { crate::chrome::infobar::HEIGHT } else { 0.0 };
            let top = self.ribbon_bottom() + above;
            let left = self.pane_width();
            let width = self.view_width as f32 - left;
            bar.draw(
                &mut self.canvas,
                &mut self.chrome_engine,
                &mut self.renderer,
                top,
                left,
                width,
                &theme,
            );
            self.signatures_bar = Some(bar);
        }

        if let Some(mut bar) = self.find_bar.take() {
            let top = self.ribbon_bottom() + self.info_bar_height();
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
        // And the same for the stretches somebody may edit when the rest of
        // the document is shut.
        self.draw_marked_regions();
        // The tags at the ends of a content control, which go over the text
        // because they stand beside it rather than behind it — the faint
        // brackets, or in Design Mode the tags with the titles on them.
        if self.design_mode {
            self.draw_design_tags();
        } else {
            self.draw_control_edges();
        }
        // And the plus at the corner of each item of a repeating section.
        self.draw_repeat_buttons();

        // The wavy lines go over the text: they are about the words, and a word
        // drawn over its own mark would hide it.
        self.draw_proofing_marks();
        // The lines under text an input method is still composing, likewise.
        self.draw_composition_marks();

        // Over the page and under the chrome: the handles belong to the drawing
        // on the page, but nothing on the page may be drawn over them.
        self.draw_shape_handles();
        // And the stroke the pen is in the middle of, which is not on the
        // page yet.
        self.draw_stroke_in_hand();

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
        // The margins of the sheet the view lays the text out on, which in
        // print layout are the document's and in web layout the window's.
        let metrics = self.view_metrics();
        let pixels_per_inch = self.pixels_per_inch();
        let scale = pixels_per_inch / POINTS_PER_INCH;
        // The page being looked at, not the first one: the side ruler describes
        // the sheet in front of the reader, and on page three that is page
        // three.
        let index = self.visible_page();
        // The rulers are furniture: they are drawn turned about with
        // everything else, so they are given the page where it was laid
        // out rather than where it ended up on the screen.
        let (page_left, page_origin_y) = self.page_origin_as_laid_out(index);
        let page_width = self.pages.get(index).map_or(0.0, |page| page.width);
        let page_height = self.pages.get(index).map_or(0.0, |page| page.height);

        let indents = self.document.indents_here();
        let to_pixels = |twips: i32| twips as f32 / TWIPS_PER_POINT * scale;
        // The ruler counts in whatever measurements are shown in.
        let steps = if self.unit == crate::measure::Unit::Inches {
            rulers::Steps::inches(pixels_per_inch)
        } else {
            rulers::Steps::centimetres(pixels_per_inch)
        };
        let horizontal = Measurements {
            steps,
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
            steps,
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

        // A page on the web has no height to measure, and Word shows no side
        // ruler in web layout.
        if self.view != super::views::View::Web {
            rulers::draw_vertical(
                &mut self.canvas,
                &mut self.chrome_engine,
                &mut self.renderer,
                pane,
                vertical,
                &theme,
            );
        }
    }

    /// Draws the frame and the eight handles round the chosen drawing.
    ///
    /// Drawn over the page rather than on it, because they are not part of the
    /// document: they are what says the drawing can be taken hold of.
    pub(super) fn draw_shape_handles(&mut self) {
        // Every drawing chosen carries its own, because each of them can be
        // taken hold of and resized on its own even while all of them move
        // together.
        for chosen in self.chosen_drawing_boxes() {
            self.draw_handles_round(chosen);
        }
        self.draw_choosing_band();
    }

    /// The stroke being drawn, as a band along where the pointer has been, in
    /// the pen's colour and width: what it will be once the pen is lifted.
    fn draw_stroke_in_hand(&mut self) {
        let Some(stroke) = self.stroke_in_hand() else { return };
        let (points, colour, width) = (stroke.points, stroke.colour, stroke.width);
        if points.len() < 2 {
            return;
        }
        let mut path = wp_raster::Path::new();
        for (index, (x, y)) in points.iter().enumerate() {
            let point = wp_raster::Point::new(*x, *y);
            if index == 0 {
                path.move_to(point);
            } else {
                path.line_to(point);
            }
        }
        let band = wp_layout::geometry::band_along(&path, width);
        self.canvas.fill_path(&band, colour);
    }

    /// The band being swept round a handful of drawings.
    ///
    /// A dashed rectangle, which is what every program draws for one: a solid
    /// one would look like something that had been put on the page.
    fn draw_choosing_band(&mut self) {
        let Some((left, top, width, height)) = self.band_rect() else { return };
        let colour = self.theme.accent;
        let dash = 4i32;

        let mut x = left;
        while x < left + width {
            let run = dash.min(left + width - x);
            self.canvas.fill_rect(x, top, run, 1, colour);
            self.canvas.fill_rect(x, top + height, run, 1, colour);
            x += dash * 2;
        }
        let mut y = top;
        while y < top + height {
            let run = dash.min(top + height - y);
            self.canvas.fill_rect(left, y, 1, run, colour);
            self.canvas.fill_rect(left + width, y, 1, run, colour);
            y += dash * 2;
        }
    }

    /// The frame and the eight handles round one drawing.
    fn draw_handles_round(&mut self, chosen: super::handles::OnPage) {
        let (left, top, width, height) = (chosen.left, chosen.top, chosen.width, chosen.height);
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

        // And the round one that turns the drawing, standing clear above the
        // top edge on a short stalk that says what it belongs to. Round rather
        // than square because it does something the other eight do not.
        let (turn_x, turn_y) = super::handles::turn_handle(&chosen);
        self.canvas.fill_rect(turn_x as i32, turn_y as i32, 1, (top - turn_y) as i32, colour);
        self.draw_round_handle(turn_x, turn_y, handle / 2.0 + 1.0, colour);

        // The shape's own handles, which change the shape and not the box it
        // sits in. Yellow diamonds, as Word draws them: a different colour and
        // a different corner from the eight that change the size, because they
        // do a different thing and land inside the drawing where the eight
        // never do.
        for held in self.shape_handles(&chosen) {
            let at = held.at();
            self.draw_diamond_handle(at.x, at.y, handle / 2.0 + 1.0);
        }
    }

    /// A diamond standing on its point, drawn row by row.
    ///
    /// Yellow with a dark rim, which is the handle Word puts on a shape that
    /// can be changed. Row by row for the same reason the round one is: a path
    /// built and thrown away on every redraw to draw four short lines would be
    /// four short lines dearly bought.
    fn draw_diamond_handle(&mut self, middle_x: f32, middle_y: f32, reach: f32) {
        const FILL: wp_raster::Color = wp_raster::Color::rgb(0xFF, 0xC0, 0x00);
        const RIM: wp_raster::Color = wp_raster::Color::rgb(0x7F, 0x60, 0x00);

        let from = (middle_y - reach).floor() as i32;
        let to = (middle_y + reach).ceil() as i32;
        for y in from..=to {
            let down = (y as f32 + 0.5 - middle_y).abs();
            let half = reach - down;
            if half <= 0.0 {
                continue;
            }
            let left = (middle_x - half).round() as i32;
            let width = (half * 2.0).round().max(1.0) as i32;
            self.canvas.fill_rect(left, y, width, 1, RIM);
            // The same row a pixel narrower, which leaves the rim showing round
            // the yellow.
            if width > 2 {
                self.canvas.fill_rect(left + 1, y, width - 2, 1, FILL);
            }
        }
    }

    /// A disc with a rim, drawn row by row.
    ///
    /// There is no circle in the drawing stack below this — it fills paths, and
    /// a path for a handle would be four curves built and thrown away on every
    /// redraw — so the rows are worked out here from the circle itself.
    fn draw_round_handle(
        &mut self,
        middle_x: f32,
        middle_y: f32,
        radius: f32,
        rim: wp_raster::Color,
    ) {
        let inside = self.theme.page;
        let from = (middle_y - radius).floor() as i32;
        let to = (middle_y + radius).ceil() as i32;
        for y in from..=to {
            let down = y as f32 + 0.5 - middle_y;
            let half = (radius * radius - down * down).max(0.0).sqrt();
            if half <= 0.0 {
                continue;
            }
            let left = (middle_x - half).round() as i32;
            let width = (half * 2.0).round().max(1.0) as i32;
            self.canvas.fill_rect(left, y, width, 1, rim);

            // The same row of the disc one pixel smaller, which leaves the rim
            // showing round it.
            let inner = radius - 1.0;
            let half = (inner * inner - down * down).max(0.0).sqrt();
            if half > 0.0 {
                let left = (middle_x - half).round() as i32;
                let width = (half * 2.0).round().max(1.0) as i32;
                self.canvas.fill_rect(left, y, width, 1, inside);
            }
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
            language: crate::names::language(&self.document.language_here()),
            zoom: self.zoom,
            modified: self.document.is_modified(),
            selected_characters: selected.chars().count(),
            shows: self.status_shows,
        };
        self.reader_status = status::pieces(&state).into_iter().map(|(piece, _)| piece).collect();

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
        let Some((x, y, caret_width, caret_height)) = self.caret_rect() else {
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
        // Its width as well as its height, because a caret in a turned cell
        // lies the other way and putting back a tall thin rectangle over a
        // short wide one would leave a smear.
        let (x, y) = (x as i32, y as i32);
        let (width, height) = (caret_width.ceil().max(1.0) as i32, caret_height.ceil() as i32);
        // The input method's candidate list opens beside the caret, so it is
        // told where the caret is each time the caret is drawn; and a screen
        // reader is told when the caret has moved, so it reads the new line.
        wp_shell::place_composition(x, y, height);
        self.note_selection_for_reader();
        let under = self.canvas.copy_rect(x, y, width, height);
        if self.caret_on {
            self.canvas.fill_rect(x, y, width, height, self.theme.caret);
        }
        self.under_caret = Some((x, y, width, height, under));
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

        let Some((x, y, width, height, under)) = self.under_caret.take() else {
            // Nothing was kept, which means the caret was not drawn last time.
            // Then this is not a blink but a first drawing, and the caller
            // paints the window.
            return false;
        };
        self.canvas.paste_rect(x, y, width, height, &under);
        if self.caret_on {
            self.canvas.fill_rect(x, y, width, height, self.theme.caret);
        }
        // The pixels kept are the ones under the caret, not the caret itself,
        // so they serve both halves of every blink after this one.
        self.under_caret = Some((x, y, width, height, under));
        true
    }

    /// Draws the mark that shows where carried text would land.
    ///
    /// A caret of its own, drawn where the pointer is rather than where the
    /// document's caret is: the document's caret is still in the text being
    /// carried, and moving it would give up the selection before the person
    /// has decided anything.
    pub(super) fn draw_drop_mark(&mut self) {
        let Some(onto) = self.drop_target() else { return };
        let Some((x, y, width, height)) = self.caret_rect_at(onto) else { return };
        if y < self.content_top() || y + height > self.content_bottom() {
            return;
        }
        let colour = self.theme.accent;
        self.canvas.fill_rect(
            x as i32,
            y as i32,
            width.ceil().max(2.0) as i32,
            height.ceil() as i32,
            colour,
        );
    }
    /// Draws everything, in the order it has to go down in.
    pub(super) fn paint(&mut self, width: usize, height: usize) {
        // The window exists by the time anything is painted into it, which is
        // the earliest the desktop can be told anything about it.
        self.tell_desktop_the_theme();

        // The window's own pixels, of which this program's are a scale: the
        // canvas is as big as the window and draws everything that much
        // bigger, and the view is measured in this program's own.
        if self.canvas.pixel_width() != width || self.canvas.pixel_height() != height {
            self.canvas = Canvas::new(width, height);
            self.canvas.set_scale(self.scale);
            self.view_width = (width as f32 / self.scale).round().max(1.0) as usize;
            self.view_height = (height as f32 / self.scale).round().max(1.0) as usize;
            self.needs_redraw = true;
            self.under_caret = None;
        }

        // Everything the window is made of is drawn turned about where the
        // interface is read right to left. What is on the page is not: an
        // English document does not read backwards in an Arabic window. The
        // furniture is told as well as the canvas, because a button drawn on
        // one side has to be found again on that side — and this is said
        // before anything is drawn or asked about, including the blink
        // below, which draws nothing else.
        let about = self.mirrored().then_some(self.view_width as f32);
        if self.canvas.mirror() != about {
            // The language has been changed to one read the other way, so
            // everything is in the wrong place until it is drawn again.
            self.needs_redraw = true;
            self.under_caret = None;
        }
        self.canvas.set_mirror(about);
        crate::chrome::mirror::set(about);
        // And the interface's lines read its way, whatever they start with.
        self.chrome_engine.set_interface_direction(about.map(|_| wp_bidi::Direction::RightToLeft));

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

        // Word's Visual Basic Editor is a window of its own there and a page
        // of its own here, for the same reason the Print page is one: what is
        // being done has nothing to do with the document's own page.
        if self.editing_basic() {
            self.draw_title_bar();
            self.draw_basic_page();
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
        let held = self.canvas.suspend_mirror();
        self.draw_dimmed_document();
        self.canvas.set_mirror(held);

        // The pane being edited is drawn inside its own band, so that with the
        // window split it cannot draw over the other one.
        let (top, bottom) = self.pane_band(self.active_pane);
        let previous_clip = self.canvas.set_clip(
            0,
            top as i32,
            self.view_width as i32,
            (bottom - top).max(0.0) as i32,
        );
        let held = self.canvas.suspend_mirror();
        self.draw_pages();
        self.draw_caret();
        self.canvas.set_mirror(held);
        self.canvas.restore_clip(previous_clip);

        // And then the other view of the same document, if there is one.
        let held = self.canvas.suspend_mirror();
        self.draw_other_pane();
        self.canvas.set_mirror(held);

        // The furniture goes last, so a page scrolled under it is covered
        // rather than showing through.
        self.draw_chrome();
        self.draw_scrollbar();
        // The styles pane goes over the rulers rather than under them: it is a
        // pane of its own, and Word's rulers stop at its edge.
        self.draw_styles_pane();
        self.draw_restrict_pane();
        self.draw_signature_pane();
        self.draw_mapping_pane();
        self.draw_text_pane();
        self.draw_translator();
        self.draw_compare_pane();
        self.draw_drop_mark();

        // An open list goes over everything, which is what makes it a list
        // dropped in front of the window rather than part of it.
        let theme = self.theme;
        // The mark the middle-button scroll is measured from.
        self.draw_autoscroll_mark();

        // The little button at the end of a paste floats over the page, and
        // under whatever it drops open.
        self.draw_paste_badge();
        self.draw_correction_badge();

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

        // A form a macro has put up is over the document, and a dialog is
        // over everything: while one is up, it is the window.
        self.draw_form();
        self.draw_dialog();

        // And the tip over even that: it is the one thing that is always about
        // whatever the pointer is on this instant — unless something is open,
        // in which case it is forgotten rather than drawn over the menu it
        // would cover. A menu can be dropped open by the keyboard as well as by
        // the button, so the rule is kept here, where every road ends, and not
        // at each of the places that open one.
        if self.something_is_open() {
            self.tip = None;
        }
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
/// Asked of the same rules that decide what a double click selects, because
/// the two answers have to be the same one. Counting the gaps instead would
/// say that a page of Japanese — which has no gaps — is one word.
/// See [`wp_segment::count_words`].
pub(super) fn count_words(text: &str) -> usize {
    wp_segment::count_words(text)
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

#[cfg(test)]
mod scale_tests {
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Cursor, Event};

    use super::super::Editor;

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Hello")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        Editor::new(library(), document, None)
    }

    /// On a dense screen the window has twice the pixels, the view is the
    /// same size in the program's own, and what is at a point is what is
    /// at twice that point on the screen.
    #[test]
    fn a_denser_screen_draws_the_same_window_in_more_pixels() {
        let mut plain = editor();
        plain.handle(Event::Resized { width: 1400, height: 900 });
        plain.draw(1400, 900);
        let plain_pixel = plain.canvas().pixel(700, 60);

        let mut dense = editor();
        dense.handle(Event::ScaleChanged { scale: 2.0 });
        dense.handle(Event::Resized { width: 1400, height: 900 });
        let canvas = dense.draw(2800, 1800);
        assert_eq!((canvas.pixel_width(), canvas.pixel_height()), (2800, 1800));
        assert_eq!((canvas.width(), canvas.height()), (1400, 900));
        assert_eq!((dense.view_width, dense.view_height), (1400, 900));
        // The ribbon's own colour at a point, and the same point in the
        // program's pixels reads back the same through the scale.
        assert_eq!(dense.canvas().pixel(700, 60), plain_pixel);
        // Pressing where the Home tab is, in the program's pixels, still
        // finds it.
        let over_tab = dense.cursor(80, 48);
        assert_eq!(over_tab, Cursor::Hand);
        let elements = dense.accessible_elements();
        let home = elements.iter().find(|e| e.name == "Home").expect("the Home tab");
        assert!(home.rect.0 < 200, "the tab is placed in the program's pixels, not the screen's");
    }
}
