//! What the window does when something happens to it.

use std::time::{Duration, Instant};

use wp_docx::model::Alignment;
use wp_docx::{CharacterFormat, TextPosition};
use wp_raster::Canvas;
use wp_shell::{App, Cursor, Event, Key, Modifiers, Response};

use crate::chrome::rulers;
use crate::chrome::{self, Choice, Command, MiniBar, Popup, Tip};

use super::{Editor, SCROLL_PER_NOTCH};

impl App for Editor {
    fn is_caption(&mut self, x: i32, y: i32) -> bool {
        self.titlebar.is_caption(x, y)
    }

    /// Which pointer belongs over a point.
    ///
    /// The rule is Word's, and it is the only one that reads right: an I-beam
    /// where text can be put, an arrow over everything that is furniture, and a
    /// bar where an edge can be dragged. A window with one cursor from edge to
    /// edge tells the user the ribbon is text.
    fn cursor(&mut self, x: i32, y: i32) -> Cursor {
        let (fx, fy) = (x as f32, y as f32);

        // Anything dropped open takes the pointer first, and a row of it is a
        // thing to press.
        if let Some(grid) = self.table_grid {
            if grid.covers(x, y) {
                return if grid.hit(x, y).is_some() { Cursor::Hand } else { Cursor::Arrow };
            }
        }
        if let Some(palette) = self.palette {
            if palette.covers(x, y) {
                return if palette.hit(x, y).is_some() { Cursor::Hand } else { Cursor::Arrow };
            }
        }
        if let Some(popup) = &self.popup {
            if popup.covers(x, y) {
                return if popup.hit(x, y).is_some() { Cursor::Hand } else { Cursor::Arrow };
            }
        }
        if let Some(bar) = &self.mini_bar {
            if bar.covers(x, y) {
                return if bar.hit(x, y).is_some() { Cursor::Hand } else { Cursor::Arrow };
            }
        }

        // The little button at the end of a paste is a button.
        if self.over_paste_badge(x, y) {
            return Cursor::Hand;
        }
        if self.over_correction_badge(x, y) {
            return Cursor::Hand;
        }

        // The File tab covers everything under the caption bar, so what is
        // under the pointer there is a line of it or nothing — never the
        // document, and never an I-beam.
        if self.in_backstage() && fy >= crate::chrome::TITLE_HEIGHT {
            let over = self.backstage.as_ref().and_then(|open| open.at(x, y)).is_some();
            return if over { Cursor::Hand } else { Cursor::Arrow };
        }

        // The title bar: its buttons are buttons, the rest of it drags the
        // window, which the system already shows a cursor for.
        if fy < crate::chrome::TITLE_HEIGHT {
            let over_button = self.titlebar.window_button_at(x, y).is_some()
                || self.titlebar.quick_at(x, y).is_some();
            return if over_button { Cursor::Hand } else { Cursor::Arrow };
        }

        // The ribbon: a tab, a button and the search box are all pressable.
        if fy < self.ribbon_bottom() {
            if self.ribbon.search_at(x, y) {
                return Cursor::Text;
            }
            let over = self.ribbon.tab_at(x, y).is_some() || self.ribbon.command_at(x, y).is_some();
            return if over { Cursor::Hand } else { Cursor::Arrow };
        }

        // The status strip: the zoom slider is dragged, its buttons pressed.
        if fy >= self.window_bottom() {
            if self.slider.is_some_and(|slider| slider.covers(x, y)) {
                return Cursor::ResizeHorizontal;
            }
            let over = self.status_buttons.iter().any(|(_, left, top)| {
                fx >= *left
                    && fx < left + 16.0
                    && fy >= *top
                    && fy < top + crate::chrome::STATUS_HEIGHT
            });
            return if over { Cursor::Hand } else { Cursor::Arrow };
        }

        // The margin down the left of the page is the selection bar: an arrow
        // there, not an I-beam, because a click takes a whole line rather than
        // putting the caret in one.
        if self.in_selection_bar(x, y).is_some() {
            return Cursor::Arrow;
        }

        // The bar down the right is furniture: an arrow over it, not an I-beam.
        if self.on_scrollbar(x, y) {
            return Cursor::Arrow;
        }

        // The bar between two views of the document.
        if self.split_dragging || self.on_split_bar(fy) {
            return Cursor::ResizeVertical;
        }

        if self.show_navigation {
            let (top, bottom) = (self.ribbon_bottom(), self.window_bottom());
            if self.resizing_pane || self.navigation.on_splitter(x, y, top, bottom) {
                return Cursor::ResizeHorizontal;
            }
            if crate::chrome::mirror::flip_f(fx) < self.navigation.width() {
                return match self.navigation.hit(x, y, top, bottom) {
                    Some(crate::chrome::navigation::Hit::SearchBox) => Cursor::Text,
                    Some(_) => Cursor::Hand,
                    None => Cursor::Arrow,
                };
            }
        }

        if self.show_rulers {
            let left = self.pane_width();
            // Only the markers and the margin boundaries are draggable, so only
            // over those does the pointer say so — a strip that claims to be
            // draggable all the way across is a strip that lies.
            let (horizontal, vertical) = self.ruler_measurements();
            if fy < self.content_top() {
                if fy < self.ribbon_bottom() {
                    return Cursor::Arrow;
                }
                let stops = self.ruler_stops();
                let hit = rulers::hit_horizontal(horizontal, &stops, x, y);
                return match hit {
                    // The box and the face are pressed, not dragged, so the
                    // pointer over them says so.
                    Some(rulers::Hit::StopSelector | rulers::Hit::Face) => Cursor::Hand,
                    Some(_) => Cursor::ResizeHorizontal,
                    None => Cursor::Arrow,
                };
            }
            if fx < left + crate::chrome::VERTICAL_WIDTH {
                return if rulers::hit_vertical(left, vertical, x, y).is_some() {
                    Cursor::ResizeVertical
                } else {
                    Cursor::Arrow
                };
            }
        }

        // Over a drawing that is selected, the pointer says what dragging will
        // do to it.
        if let Some(cursor) = self.shape_cursor(x, y) {
            return cursor;
        }

        // The find strip: its fields take text, its buttons are pressed.
        if self.find_bar.is_some() && fy < self.content_top() {
            return match self.find_bar.as_ref().and_then(|bar| bar.hit(x, y)) {
                Some(crate::chrome::findbar::Hit::FindField)
                | Some(crate::chrome::findbar::Hit::ReplaceField) => Cursor::Text,
                Some(_) => Cursor::Hand,
                None => Cursor::Arrow,
            };
        }

        // The join between two pages, which is double-clicked to hide the white
        // space or bring it back.
        if self.between_pages(x, y).is_some() {
            return Cursor::ResizeVertical;
        }

        // The button beside a line, and a table's handles: all pressed.
        if self.spot_at(x, y).is_some() {
            return Cursor::Hand;
        }

        // A table's handles are pressed, not typed in.
        if self.table_handle_at(x, y).is_some() {
            return Cursor::Hand;
        }

        // The band above a table is not text: a press there takes a column.
        if self.column_bar_at(x, y).is_some() {
            return Cursor::Arrow;
        }

        // On one of a table's own lines, the pointer says which way it moves.
        if let Some(edge) = self.table_edge_at(x, y) {
            return if edge.across { Cursor::ResizeHorizontal } else { Cursor::ResizeVertical };
        }

        // Over the page area: an I-beam where there is a place for the caret,
        // and an arrow over the desk beside and between the pages.
        if self.position_at(x, y).is_some() {
            Cursor::Text
        } else {
            Cursor::Arrow
        }
    }

    fn handle(&mut self, event: Event) -> Response {
        // Whether this is the kind of event that may have changed the
        // document, which is what a binding follows: see [`super::mapping`].
        let may_have_edited = !matches!(event, Event::Tick | Event::MouseMove { .. });
        let response = self.handle_event(event);
        if may_have_edited {
            self.keep_placeholders();
            self.keep_bindings();
        }
        // Wherever the caret went, the document may have something to say
        // about it: see [`super::autoevents`].
        self.notice_control_change();
        response
    }

    fn switch_window(&mut self, index: usize) {
        self.use_window(index);
    }

    fn draw(&mut self, width: usize, height: usize) -> &Canvas {
        self.paint(width, height);
        self.canvas()
    }

    // What a screen reader is told. See [`super::accessible`].
    fn accessible_elements(&mut self) -> Vec<wp_shell::accessibility::Element> {
        Editor::accessible_elements(self)
    }

    fn accessible_invoke(&mut self, id: u64) -> Response {
        Editor::accessible_invoke(self, id)
    }

    fn accessible_text(&mut self) -> Option<wp_shell::accessibility::TextState> {
        Some(Editor::accessible_text(self))
    }

    fn accessible_select(&mut self, start: usize, end: usize) -> Response {
        Editor::accessible_select(self, start, end)
    }

    fn accessible_rects(&mut self, start: usize, end: usize) -> Vec<(i32, i32, i32, i32)> {
        Editor::accessible_rects(self, start, end)
    }
}

impl Editor {
    /// One event, as it comes.
    #[allow(clippy::too_many_lines)]
    fn handle_event(&mut self, event: Event) -> Response {
        // Word's Visual Basic editor is a window of its own there and has the
        // window here, the way the Print page does: while it is up, the
        // document behind it is not being typed into.
        if self.editing_basic() {
            match event {
                Event::KeyDown { key, modifiers } => {
                    // Alt+F11 shuts it again, as it opens it.
                    if key == Key::Function(11) && modifiers.alt {
                        return self.close_basic();
                    }
                    return self.basic_key(key, modifiers.shift, modifiers.control);
                }
                Event::Char(character) => return self.basic_character(character),
                Event::Commit(text) => {
                    let mut response = Response::Ignored;
                    for character in text.chars() {
                        response = self.basic_character(character);
                    }
                    return response;
                }
                Event::MouseDown { x, y, .. } | Event::DoubleClick { x, y } => {
                    let (width, height) = (self.view_width, self.view_height);
                    return self.basic_press(x, y, width, height);
                }
                Event::Scroll { lines, .. } => return self.basic_scroll(lines),
                // A macro that is running is answered here, and a macro that
                // is stopped is why the window is still drawing.
                Event::Tick => {
                    return if self.pump_macro() { Response::Redraw } else { Response::Ignored };
                }
                // The window changing size, and everything else about the
                // window rather than about what is on it, goes on as usual.
                Event::Resized { .. } | Event::ScaleChanged { .. } | Event::Closing => {}
                _ => return Response::Ignored,
            }
        }

        // A macro run from anywhere else is answered on the tick as well.
        if let Event::Tick = event {
            if self.debugger.is_some() && !self.in_form() {
                let pumped = self.pump_macro();
                if pumped && self.debugger.is_none() {
                    return Response::Redraw;
                }
            }
        }

        // Alt+F11 opens it from anywhere, which is how everybody who uses it
        // opens it.
        if let Event::KeyDown { key, modifiers } = event {
            if key == Key::Function(11) && modifiers.alt && !self.editing_basic() {
                return self.open_basic();
            }
        }

        // A dialog is modal, as every dialog in Word is: while one is up it has
        // the mouse and the keyboard, and the document behind it neither
        // scrolls nor takes a click. Everything else — the window changing size,
        // the caret blinking — goes on behind it as it does in Word.
        if self.in_dialog() {
            match event {
                Event::MouseDown { x, y, .. } | Event::DoubleClick { x, y } => {
                    return self.dialog_press(x, y)
                }
                Event::MouseMove { x, y, .. } => {
                    return if self.dialog_hover(x, y) {
                        self.needs_redraw = true;
                        Response::Redraw
                    } else {
                        Response::Ignored
                    }
                }
                Event::KeyDown { key, modifiers } => {
                    return self.dialog_key(key, modifiers.shift, modifiers.control)
                }
                Event::Char(character) => return self.dialog_character(character),
                // A dialog's box takes composed text as it is committed; the
                // composition itself is not shown in it.
                Event::Commit(text) => {
                    let mut response = Response::Ignored;
                    for character in text.chars() {
                        response = self.dialog_character(character);
                    }
                    return response;
                }
                Event::Compose { .. } | Event::ComposeEnd => return Response::Ignored,
                // A modal dialog takes no drops either.
                Event::FilesDropped { .. }
                | Event::DataDragOver { .. }
                | Event::DataDragLeft
                | Event::DataDropped { .. } => return Response::Ignored,
                // Swallowed rather than passed through: the window behind a
                // modal dialog does not answer these.
                Event::Scroll { .. }
                | Event::MouseUp { .. }
                | Event::RightClick { .. }
                | Event::MiddleClick { .. }
                | Event::MenuKey
                | Event::ControlKey => return Response::Ignored,
                _ => {}
            }
        }

        // A form a macro has put up is modal in the same way, under a
        // dialog the macro's code may put up over it.
        if self.in_form() {
            match event {
                Event::MouseDown { x, y, .. } | Event::DoubleClick { x, y } => {
                    return self.form_press(x, y)
                }
                Event::MouseMove { x, y, .. } => {
                    return if self.form_hover(x, y) {
                        self.needs_redraw = true;
                        Response::Redraw
                    } else {
                        Response::Ignored
                    }
                }
                Event::KeyDown { key, modifiers } => return self.form_key(key, modifiers.shift),
                Event::Char(character) => return self.form_character(character),
                Event::Commit(text) => {
                    let mut response = Response::Ignored;
                    for character in text.chars() {
                        response = self.form_character(character);
                    }
                    return response;
                }
                Event::Scroll { .. }
                | Event::MouseUp { .. }
                | Event::RightClick { .. }
                | Event::MiddleClick { .. }
                | Event::MenuKey
                | Event::ControlKey
                | Event::Compose { .. }
                | Event::ComposeEnd
                | Event::FilesDropped { .. }
                | Event::DataDragOver { .. }
                | Event::DataDragLeft
                | Event::DataDropped { .. } => return Response::Ignored,
                _ => {}
            }
        }

        match event {
            // The window came up on, or moved to, a screen of another density:
            // the same window, drawn again with everything that many times
            // bigger in the screen's pixels.
            Event::ScaleChanged { scale } => {
                if (scale - self.scale).abs() > f32::EPSILON {
                    self.scale = scale;
                    self.canvas = Canvas::new(1, 1);
                    self.under_caret = None;
                    self.needs_redraw = true;
                }
                Response::Redraw
            }
            Event::Resized { width, height } => {
                self.view_width = width as usize;
                self.view_height = height as usize;
                self.clamp_scroll();
                // The first size arrives once the window exists, which is the
                // first moment its caption can be set.
                self.update_title();
                self.needs_redraw = true;
                Response::Redraw
            }

            // The wheel scrolls whatever is under it.
            Event::Scroll { lines, modifiers } => {
                // Ctrl and the wheel is zoom, in this program as in every other
                // one. Word steps by ten per cent a notch and stops at the same
                // ends the slider does.
                if modifiers.control {
                    return self.set_zoom(self.zoom + lines * 10.0);
                }
                // Shift and the wheel moves the view sideways, which is the
                // only thing to do with a wheel when the page is too wide.
                if modifiers.shift {
                    return self.scroll_across_by(-lines * SCROLL_PER_NOTCH);
                }
                if let Some(popup) = &mut self.popup {
                    return if popup.scroll_by(-lines.round() as i32 * 3) {
                        self.needs_redraw = true;
                        Response::Redraw
                    } else {
                        Response::Ignored
                    };
                }
                // Over the File tab the wheel winds its list, and nothing of
                // the document behind it moves.
                if self.in_backstage() {
                    return if self.backstage_scroll(-lines) {
                        self.needs_redraw = true;
                        Response::Redraw
                    } else {
                        Response::Ignored
                    };
                }
                // The wheel over one of the panes down the side scrolls that
                // pane rather than the document, which is what a person means
                // by turning it while the pointer is over one.
                if self.over_restrict_pane(self.pointer_x as i32) {
                    return if self.restrict_pane.scroll_by(-lines * super::PANE_STEP) {
                        self.needs_redraw = true;
                        Response::Redraw
                    } else {
                        Response::Ignored
                    };
                }
                if self.over_signature_pane(self.pointer_x as i32) {
                    return if self.signature_pane.scroll_by(-lines * super::PANE_STEP) {
                        self.needs_redraw = true;
                        Response::Redraw
                    } else {
                        Response::Ignored
                    };
                }
                if self.over_mapping_pane(self.pointer_x as i32) {
                    return if self.mapping_pane.scroll_by(-lines * super::PANE_STEP) {
                        self.needs_redraw = true;
                        Response::Redraw
                    } else {
                        Response::Ignored
                    };
                }
                if self.over_text_pane(self.pointer_x as i32) {
                    return if self.text_pane.scroll_by(-lines * super::PANE_STEP) {
                        self.needs_redraw = true;
                        Response::Redraw
                    } else {
                        Response::Ignored
                    };
                }
                // Over the navigation pane the wheel scrolls the outline.
                // The wheel over the styles pane scrolls its list.
                if self.over_styles_pane(self.pointer_x as i32) {
                    return if self.styles_pane_scroll(-lines.round() as i32 * 3) {
                        self.needs_redraw = true;
                        Response::Redraw
                    } else {
                        Response::Ignored
                    };
                }
                // And over the recovery pane, its list of recovered files.
                if crate::chrome::mirror::flip_f(self.pointer_x) < self.pane_width() {
                    if let Some(pane) = &mut self.recovery {
                        return if pane.scroll_by(-lines.round() as i32 * 3) {
                            self.needs_redraw = true;
                            Response::Redraw
                        } else {
                            Response::Ignored
                        };
                    }
                }
                if self.show_navigation
                    && !self.recovering()
                    && crate::chrome::mirror::flip_f(self.pointer_x) < self.navigation.width()
                {
                    let total = self.headings().len();
                    return if self.navigation.scroll_by(-lines.round() as i32 * 3, total) {
                        self.needs_redraw = true;
                        Response::Redraw
                    } else {
                        Response::Ignored
                    };
                }
                // The page moving out from under the bar leaves it pointing at
                // nothing, so it goes.
                self.hide_mini_bar();
                self.scroll_by(-lines * SCROLL_PER_NOTCH)
            }

            Event::MouseDown { x, y, modifiers } => self.pressed(x, y, modifiers),
            Event::MouseMove { x, y, held, modifiers } => self.moved_pointer(x, y, held, modifiers),
            Event::MouseUp { x, y } => {
                if self.ink_release(x, y) {
                    return Response::Redraw;
                }
                self.release_shape();
                if self.pending_text_drag.is_some() || self.dragging_text() {
                    return self.drop_text(x, y, false);
                }
                self.split_dragging = false;
                self.release_ruler();
                let was_dragging = self.dragging;
                self.dragging = false;
                self.column_drag = None;
                self.sliding = false;
                self.resizing_pane = false;

                // A table dragged by its handle is put down where it was let go.
                if self.release_table_handle(x, y) {
                    return Response::Redraw;
                }

                // A line of the table that was being dragged is left where it is.
                if self.release_table_edge() {
                    return Response::Redraw;
                }

                // The table pen draws its line when the drag that made it ends.
                if self.table_pen_release(x, y) {
                    return Response::Redraw;
                }

                // Ctrl and a drag adds a stretch to the selection; Ctrl and a
                // click takes the sentence. Which of the two it was is only
                // known when the button comes up: a drag that never moved never
                // set an anchor, and that is the one that meant the sentence.
                if std::mem::take(&mut self.adding_selection)
                    && self.document.selection_anchor().is_none()
                {
                    return self.select_sentence_at(x, y);
                }
                // The format painter puts its formatting down when the drag
                // that chose the text ends, which is when Word applies it.
                if was_dragging && self.apply_format_painter() {
                    return Response::Redraw;
                }
                // A selection made with the mouse brings up the mini toolbar,
                // which is what Word does the moment the button comes up.
                if was_dragging && self.document.selection().is_some() {
                    self.show_mini_bar(x, y);
                    return Response::Redraw;
                }
                Response::Ignored
            }
            Event::RightClick { x, y, .. } => {
                // The strip along the bottom answers the right button with a
                // menu of its own: what it should be showing.
                if self.on_status_bar(y) {
                    return self.open_status_menu(x, y);
                }
                // The bar comes up with the menu, solid rather than faint: the
                // right button is somebody asking for both.
                let response = self.open_context_menu(x, y);
                if response != Response::Ignored {
                    self.show_mini_bar_awake(x, y);
                }
                response
            }

            Event::DoubleClick { x, y } => {
                // Two clicks on a signature line ask to sign it, which is
                // Word's own shortcut and the thing a person tries first.
                if let Some(response) = self.sign_line_at(x, y) {
                    return response;
                }
                // On the line at the right of a column, two clicks fit that
                // column to what is in it, which is Word's quickest way to
                // tidy a table up.
                if self.fit_column_at(x, y) == Response::Redraw {
                    return Response::Redraw;
                }
                // Double-clicking the join between two pages hides the white
                // space between them, exactly as it does in Word.
                if self.between_pages(x, y).is_some() {
                    return self.toggle_joined_pages();
                }
                // A double click on a tab stop opens what can be changed about
                // it: the dialog Word opens from the same place, as a menu.
                if self.show_rulers {
                    let (horizontal, _) = self.ruler_measurements();
                    let stops = self.ruler_stops();
                    if let Some(rulers::Hit::TabStop(index)) =
                        rulers::hit_horizontal(horizontal, &stops, x, y)
                    {
                        return self.open_tab_stop_menu(index, x, y);
                    }
                }
                // And double-clicking a ruler opens the page setup, which is
                // what a ruler is a picture of.
                if self.show_rulers && self.on_a_ruler(x, y) {
                    return self.run(Command::Margins);
                }
                // Double-clicking the top or the foot of a page opens the
                // header or the footer, and double-clicking the document comes
                // back out — which is what Word does and what a person tries.
                if self.in_furniture() {
                    return self.leave_furniture();
                }
                if let Some(which) = self.furniture_at(x, y) {
                    return self.edit_furniture(which);
                }
                if let Some((page, _)) = self.in_selection_bar(x, y) {
                    // Two clicks in the selection bar take the paragraph, as
                    // three in the text do.
                    let _ = page;
                    self.note_double_click(x, y);
                    return self.select_paragraph_at(x, y);
                }
                self.note_double_click(x, y);
                self.drag_by = super::selecting::Granularity::Word;
                let response = self.select_word_at(x, y);
                // A word taken with two clicks brings the bar up as well: it
                // is still a selection made with the mouse.
                self.show_mini_bar(x, y);
                if self.mini_bar.is_some() {
                    Response::Redraw
                } else {
                    response
                }
            }

            Event::Char(character) => {
                // A measurement box on the ribbon takes the keyboard while it
                // has it, the way any box with a caret in it does.
                if self.typing_in_box() {
                    return self.type_into_box(character);
                }
                // The pane's search box takes the keyboard while it has it,
                // the way any box with a caret in it does.
                if self.find_has_keyboard() {
                    return self.type_into_find(character);
                }
                if self.show_navigation && self.navigation.searching {
                    return self.type_into_search(character);
                }
                // The Text Pane takes typing for the chosen diagram's words.
                if self.text_pane_has_keyboard() {
                    return self.text_pane_character(character);
                }
                if self.is_locked() {
                    return self.refuse_locked();
                }
                // A character typed past a composition the input method did
                // not close ends it: what was not committed was not wanted.
                if self.is_composing() {
                    self.end_composition();
                }
                // Typing is somebody saying they were not after the bar — nor
                // after the other ways of pasting what was just pasted, nor
                // after the drawing that was chosen: the letter goes in the
                // text, so the text is what is being worked on.
                self.hide_mini_bar();
                self.forget_paste();
                self.drop_chosen_drawing();
                self.type_character(character)
            }

            // An input method's composition goes into the document; a box on
            // the ribbon or in a pane takes only what is committed, a
            // character at a time, the way it takes typing.
            Event::Compose { text, caret, attributes } => {
                if self.typing_in_box()
                    || self.find_has_keyboard()
                    || (self.show_navigation && self.navigation.searching)
                {
                    return Response::Ignored;
                }
                self.compose(text, caret, attributes)
            }
            Event::Commit(text) => {
                if self.typing_in_box()
                    || self.find_has_keyboard()
                    || (self.show_navigation && self.navigation.searching)
                {
                    let mut response = Response::Ignored;
                    for character in text.chars() {
                        response = self.handle(Event::Char(character));
                    }
                    return response;
                }
                self.commit_composition(text)
            }
            Event::ComposeEnd => self.end_composition(),

            // What the desktop drags in: files, or another program's text.
            // See [`super::dropping`].
            Event::FilesDropped { paths, x, y } => self.drop_files(paths, x, y),
            Event::DataDragOver { x, y } => self.foreign_drag_over(x, y),
            Event::DataDragLeft => self.foreign_drag_left(),
            Event::DataDropped { contents, x, y, .. } => self.drop_data(contents, x, y),

            // Nothing in the window is under the pointer any more, so nothing
            // in it should look as though it is.
            Event::PointerLeft => {
                let lit = self.hovered.take().is_some();
                let tipped = self.tip.take().is_some();
                let barred = self.hide_mini_bar();
                if lit || tipped || barred {
                    self.needs_redraw = true;
                    return Response::Redraw;
                }
                Response::Ignored
            }

            // Alt on its own puts the letters over the ribbon, and takes them
            // away again.
            Event::MenuKey => self.toggle_key_tips(),

            // Control on its own opens the paste options, which is the one
            // thing Word gives that key by itself.
            Event::ControlKey => self.control_pressed_alone(),

            // The wheel pressed starts the scroll that follows the pointer, and
            // pressed again stops it.
            Event::MiddleClick { x, y } => self.toggle_autoscroll(x, y),

            Event::Tick => self.ticked(),

            Event::KeyDown { key, modifiers } => {
                self.hide_mini_bar();
                self.key(key, modifiers)
            }

            // Unsaved work is not thrown away without asking — but closing one
            // of several windows is closing a view, not the document, and a
            // view has nothing to lose.
            Event::Closing => {
                if self.is_last_window() && !self.may_discard() {
                    return Response::Refuse;
                }
                // A run that ends properly takes its copy with it, which is
                // how the next start knows that a copy still lying there
                // means a run that did not.
                if self.is_last_window() {
                    self.finish_autorecover();
                }
                Response::Ignored
            }
        }
    }
}

impl Editor {
    /// A press in the caption bar, for the pages that fill the window.
    ///
    /// The Print page and the File tab cover everything under the caption bar,
    /// but the three window buttons in it must still work: a window nobody can
    /// close is not a window.
    fn pressed_in_title_bar(&mut self, x: i32, y: i32) -> Response {
        if let Some(button) = self.titlebar.window_button_at(x, y) {
            wp_shell::window_command(match button {
                chrome::WindowButton::Minimise => wp_shell::WindowCommand::Minimise,
                chrome::WindowButton::Maximise => wp_shell::WindowCommand::ToggleMaximise,
                chrome::WindowButton::Close => wp_shell::WindowCommand::Close,
            });
        }
        Response::Ignored
    }

    /// Reacts to a press, wherever in the window it landed.
    fn pressed(&mut self, x: i32, y: i32, modifiers: Modifiers) -> Response {
        // The mini toolbar takes a press before anything else, because it is
        // floating over whatever is under it.
        if let Some(response) = self.mini_bar_press(x, y) {
            return response;
        }
        // A number typed into a box on the ribbon and then left is a number
        // meant, so a press anywhere below the ribbon applies it. A press on
        // the ribbon itself is left to the box, which can tell whether it
        // landed on the same one.
        if self.typing_in_box() && (y as f32) >= self.ribbon_bottom() {
            self.finish_box();
        }

        // A press anywhere else puts it away, and then goes on to mean whatever
        // it would have meant. A tip goes with it: the button has been found.
        self.hide_mini_bar();
        self.hide_key_tips();
        self.stop_autoscroll();
        self.tip = None;

        // The File tab is what the window is showing, so a press belongs to it
        // and to nothing behind it. The caption bar is still the caption bar.
        if self.in_backstage() {
            if (y as f32) < chrome::TITLE_HEIGHT {
                return self.pressed_in_title_bar(x, y);
            }
            return self.backstage_press(x, y);
        }

        // The Print page is what the window is showing, so a press belongs to
        // it and to nothing behind it — except a list it has dropped open,
        // which is in front of it.
        if self.printing() {
            if let Some(popup) = &self.popup {
                if let Some(index) = popup.hit(x, y) {
                    return self.choose(index);
                }
                self.popup = None;
                self.needs_redraw = true;
                return Response::Redraw;
            }
            if (y as f32) < chrome::TITLE_HEIGHT {
                return self.pressed_in_title_bar(x, y);
            }
            return self.print_pane_press(x, y);
        }

        // A palette takes a press before anything else while it is open.
        if let Some(palette) = self.palette {
            if let Some(index) = palette.hit(x, y) {
                return self.choose_color(index);
            }
            if palette.covers(x, y) {
                return Response::Ignored;
            }
            self.palette = None;
            self.needs_redraw = true;
            return Response::Redraw;
        }

        // The table grid takes a press before anything else while it is open.
        if let Some(grid) = self.table_grid {
            if let Some((rows, columns)) = grid.hit(x, y) {
                return self.choose_table(rows, columns);
            }
            if grid.covers(x, y) {
                return Response::Ignored;
            }
            self.table_grid = None;
            self.needs_redraw = true;
            return Response::Redraw;
        }

        // An open list takes the press before anything else, and a press
        // anywhere else closes it without doing anything.
        if let Some(popup) = &self.popup {
            if let Some(index) = popup.hit(x, y) {
                return self.choose(index);
            }
            if popup.covers(x, y) {
                return Response::Ignored;
            }
            self.popup = None;
            self.needs_redraw = true;
            return Response::Redraw;
        }

        // The little button at the end of a paste floats over the page, so it
        // takes a press before the page does — but after a list, because the
        // list it drops open is in front of it. A press anywhere else puts it
        // away: going on without it is an answer to what it was asking.
        if self.over_paste_badge(x, y) {
            return self.open_paste_menu();
        }
        self.forget_paste();
        if self.over_correction_badge(x, y) {
            return self.open_correction_options();
        }

        // The title bar: the window's own buttons and the quick access ones.
        if (y as f32) < chrome::TITLE_HEIGHT {
            if let Some(button) = self.titlebar.window_button_at(x, y) {
                wp_shell::window_command(match button {
                    chrome::WindowButton::Minimise => wp_shell::WindowCommand::Minimise,
                    chrome::WindowButton::Maximise => wp_shell::WindowCommand::ToggleMaximise,
                    chrome::WindowButton::Close => wp_shell::WindowCommand::Close,
                });
                return Response::Ignored;
            }
            if let Some(command) = self.titlebar.quick_at(x, y) {
                return self.run(command);
            }
            return Response::Ignored;
        }

        // The bar that says something about the document sits directly under
        // the ribbon, above everything else that hangs there.
        if self.over_info_bar(y) {
            return self.press_info_bar(x, y);
        }

        // The find strip sits between that and the page.
        if self.find_bar.is_some()
            && (y as f32) >= self.ribbon_bottom() + self.info_bar_height()
            && (y as f32) < self.ribbon_bottom() + self.info_bar_height() + self.find_bar_height()
        {
            return self.pressed_in_find(x, y);
        }

        // Reading mode draws no ribbon, so nothing up there is pressable — and
        // the places its buttons were last drawn are no longer where they are.
        if (y as f32) < self.ribbon_bottom() && self.view.shows_furniture() {
            if let Some(tab) = self.ribbon.tab_at(x, y) {
                return self.choose_tab(tab);
            }
            // A button with an arrow means two things depending on which half
            // of it was pressed, and the ribbon is what knows where the line
            // between them was drawn.
            return match self.ribbon.press_at(x, y) {
                Some(chrome::ribbon::Press::Run(command)) => self.run(command),
                Some(chrome::ribbon::Press::Drop(_, choice)) => self.open_list(choice),
                Some(chrome::ribbon::Press::Type(command)) => self.type_in_box(command),
                Some(chrome::ribbon::Press::Step(command, up)) => self.step_box(command, up),
                None => Response::Ignored,
            };
        }

        // The status strip: the zoom slider and the two buttons beside it.
        if (y as f32) >= self.window_bottom() {
            if let Some(slider) = self.slider {
                if slider.covers(x, y) {
                    self.sliding = true;
                    return self.set_zoom(slider.zoom_at(x));
                }
            }
            for (command, left, top) in self.status_buttons.clone() {
                if (x as f32) >= left
                    && (x as f32) < left + 16.0
                    && (y as f32) >= top
                    && (y as f32) < top + chrome::STATUS_HEIGHT
                {
                    return self.run(command);
                }
            }
            return Response::Ignored;
        }

        // The styles pane down the right, which is in front of the page and of
        // the bar beside it.
        if self.over_signature_pane(x) && (y as f32) > self.ribbon_bottom() {
            return self.signature_pane_press(x, y);
        }
        if self.over_mapping_pane(x) && (y as f32) > self.ribbon_bottom() {
            return self.mapping_pane_press(x, y);
        }
        if self.over_text_pane(x) && (y as f32) > self.ribbon_bottom() {
            return self.text_pane_press(x, y);
        }
        if self.over_restrict_pane(x) && (y as f32) > self.ribbon_bottom() {
            return self.restrict_pane_press(x, y);
        }
        if self.over_styles_pane(x) && (y as f32) > self.ribbon_bottom() {
            return self.styles_pane_press(x, y);
        }

        if self.recovering()
            && crate::chrome::mirror::flip_f(x as f32) < self.pane_width()
            && (y as f32) > self.ribbon_bottom()
        {
            return self.pressed_in_recovery(x, y);
        }

        if self.show_navigation && !self.recovering() {
            let ribbon_bottom = self.ribbon_bottom();
            let content_bottom = self.window_bottom();

            // The handle down the pane's edge, which is outside the pane by
            // half its width and so has to be tested before it.
            if self.navigation.on_splitter(x, y, ribbon_bottom, content_bottom) {
                self.resizing_pane = true;
                return Response::Ignored;
            }
            if crate::chrome::mirror::flip_f(x as f32) < self.navigation.width() {
                return self.pressed_in_pane(x, y, ribbon_bottom, content_bottom);
            }
        }

        // The bar down the right of the document, which is furniture rather
        // than part of the page.
        if self.on_scrollbar(x, y) {
            return self.press_on_scrollbar(x, y);
        }

        // The bar between two views of the document is dragged, and a press in
        // either view makes that view the one being edited — which has to
        // happen before the press is turned into a position, because a position
        // is measured from the view it is in.
        if self.on_split_bar(y as f32) {
            return self.start_split_drag();
        }
        self.activate_pane(self.pane_at(y as f32));

        // A pen or an eraser from the Draw tab takes a press on a page before
        // anything on the page does: while one is in hand, a press draws.
        if self.ink_press(x, y) {
            return Response::Redraw;
        }

        // A drawing is taken hold of before the page underneath it is asked
        // about the press: a press on a picture is about the picture.
        if self.press_on_shape(x, y, modifiers.shift) {
            return Response::Redraw;
        }

        // The rulers sit between the ribbon and the page. Their markers are
        // taken hold of before the page is asked about the press, but after the
        // pane's edge — that edge runs down through the side ruler, and
        // resizing the pane is the larger gesture of the two.
        if self.press_on_ruler(x, y) {
            self.needs_redraw = true;
            return Response::Redraw;
        }

        // Three clicks take the paragraph, and a click in the margin down the
        // left of the page takes the line. Both are asked before the page is,
        // because both are presses on the page.
        if self.is_third_click(x, y) {
            self.last_double_click = None;
            return self.select_paragraph_at(x, y);
        }
        if let Some((page, line)) = self.in_selection_bar(x, y) {
            return self.select_line(page, line);
        }

        // A press inside the selection decides nothing yet: it becomes a drag
        // if the pointer moves, and a click that gives up the selection if it
        // does not. See [`dragtext`].
        if self.press_may_drag_text(x, y, modifiers.shift, modifiers.control) {
            self.wait_for_text_drag(x, y);
            return Response::Ignored;
        }

        let Some(position) = self.position_at(x, y) else {
            return Response::Ignored;
        };

        // Ctrl and a click follows a link, which is Word's arrangement: a link
        // in a document being written is text first and a link second.
        if modifiers.control {
            // The stretch being dragged is put away before anything else,
            // because everything below moves the caret and moving the caret is
            // what drops a selection.
            self.document.add_selection_at(position);
            if self.document.hyperlink_here().is_some()
                || self.document.drawing_link_here().is_some()
            {
                self.document.clear_selection();
                return self.follow_link();
            }

            // Where there is no link, Ctrl and a drag begins another stretch of
            // the selection without losing the ones already made, and Ctrl and
            // a click takes the sentence. The two are told apart when the
            // button comes up, above: until then this is a drag that has not
            // moved yet.
            self.adding_selection = true;
            self.drag_by = super::selecting::Granularity::Character;
            self.dragging = true;
            self.status.clear();
            self.needs_redraw = true;
            return Response::Redraw;
        }

        // A table pen in hand takes the press before anything else: while one
        // is out, a press in a table is drawing rather than typing.
        if self.table_pen_press(x, y) {
            return Response::Redraw;
        }

        // The border painter, while it is in hand, takes a press that lands on
        // the edge of a cell. One that does not is an ordinary press: a pen out
        // must not swallow every click in the document.
        if self.paint_border_at(x, y) {
            return Response::Redraw;
        }

        // The button that puts a row or a column in, which sits beside the
        // line it is about — outside the table, where nothing else is asking
        // for the press.
        if self.press_insert_spot(x, y) {
            return Response::Redraw;
        }

        // A table's own handles come first of all: they are drawn outside the
        // table, over the margin or the desk beside it, where nothing else is
        // asking for the press.
        if self.press_table_handle(x, y) {
            self.status.clear();
            self.needs_redraw = true;
            return Response::Redraw;
        }

        // Just above a table, a press takes the column under the pointer.
        // Before the lines, because the band above the table is not a line and
        // after the caret would have been moved into a cell it is too late.
        if self.press_column_bar(x, y) {
            self.status.clear();
            self.needs_redraw = true;
            return Response::Redraw;
        }

        // A press on one of a table's own lines drags that line, which is how a
        // column is given a width in Word. Before the caret is moved, because
        // the line is not a place for the caret to go.
        if self.press_table_edge(x, y) {
            self.status.clear();
            self.needs_redraw = true;
            return Response::Redraw;
        }

        // The plus at the corner of a repeating section's item, which is
        // drawn beside the text and copies the item.
        if self.press_repeat_button(x, y) {
            self.needs_redraw = true;
            return Response::Redraw;
        }

        // A tick box ticks when it is clicked and a drop-down drops open,
        // which is the whole of what makes a form a form rather than a
        // picture of one. Before the caret is moved, because what was
        // clicked is the control and not the place between two letters.
        if !modifiers.alt && !modifiers.shift {
            if let Some(response) = self.used_a_control(position) {
                return response;
            }
        }

        // Alt and a drag takes a rectangle of text rather than a stretch of it.
        if modifiers.alt {
            self.document.set_caret(position);
            self.column_drag = Some((x, y));
            self.dragging = true;
            self.status.clear();
            self.needs_redraw = true;
            return Response::Redraw;
        }

        // Shift+click reaches from where the caret already is, which is how a
        // selection is made without dragging — and in a table it reaches by
        // whole cells, as a drag does.
        if modifiers.shift && self.document.table_here().is_some() {
            self.cell_anchor = self.document.table_here().map(|place| (place.row, place.column));
            if self.extend_cell_drag(x, y) {
                self.drag_by = super::selecting::Granularity::Character;
                self.dragging = true;
                self.status.clear();
                return Response::Redraw;
            }
        }

        self.document.move_caret(position, modifiers.shift);
        // Where the caret has landed is where a drag across cells reaches
        // from. Outside a table there is no such cell, and this is None.
        self.cell_anchor = self.document.table_here().map(|place| (place.row, place.column));
        self.drag_by = super::selecting::Granularity::Character;
        self.dragging = true;
        self.status.clear();
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Adds one character to the pane's search box, or takes one away.
    fn type_into_search(&mut self, character: char) -> Response {
        match character {
            '\u{8}' => {
                if self.navigation.search.pop().is_none() {
                    return Response::Ignored;
                }
            }
            '\r' | '\n' => {
                // Enter jumps to the first thing found, which is what a search
                // box is for.
                self.navigation.section = crate::chrome::navigation::Section::Results;
                let first = self.pane_contents().found.first().cloned();
                if let Some(found) = first {
                    self.document.set_caret(TextPosition::new(found.paragraph, found.offset));
                    self.reveal_caret();
                }
                self.needs_redraw = true;
                return Response::Redraw;
            }
            '\u{1b}' => {
                self.navigation.search.clear();
                self.navigation.searching = false;
            }
            character if character.is_control() => return Response::Ignored,
            character => self.navigation.search.push(character),
        }
        // Typing in the box is about the pane, so the results tab is what
        // should be showing.
        self.navigation.section = crate::chrome::navigation::Section::Results;
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Reacts to a press inside the navigation pane.
    fn pressed_in_pane(&mut self, x: i32, y: i32, top: f32, bottom: f32) -> Response {
        use crate::chrome::navigation::{Hit, Section};

        match self.navigation.hit(x, y, top, bottom) {
            Some(Hit::Close) => self.run(Command::ToggleNavigation),
            Some(Hit::Tab(section)) => {
                self.navigation.section = section;
                self.navigation.searching = section == Section::Results;
                self.needs_redraw = true;
                Response::Redraw
            }
            Some(Hit::SearchBox) => {
                self.navigation.searching = true;
                self.needs_redraw = true;
                Response::Redraw
            }
            Some(Hit::Row(index)) => {
                let contents = self.pane_contents();
                let position = match self.navigation.section {
                    Section::Headings => contents
                        .headings
                        .get(index)
                        .map(|heading| TextPosition::new(heading.paragraph, 0)),
                    Section::Results => contents
                        .found
                        .get(index)
                        .map(|found| TextPosition::new(found.paragraph, found.offset)),
                    Section::Comments => contents
                        .notes
                        .get(index)
                        .map(|note| TextPosition::new(note.paragraph, note.offset)),
                    // A page is not a place in the text, it is a place in the
                    // view, so pressing one scrolls rather than moving the caret.
                    Section::Pages => return self.scroll_to_page(index),
                };
                let Some(position) = position else { return Response::Ignored };
                self.document.set_caret(position);
                self.reveal_caret();
                self.needs_redraw = true;
                Response::Redraw
            }
            None => Response::Ignored,
        }
    }

    /// Puts the tip up for whatever the pointer is resting on.
    ///
    /// The ribbon or the mini toolbar: both are rows of buttons, and a button
    /// that only shows a drawing needs telling on whichever of them it sits.
    pub(super) fn show_tip(&mut self) -> Option<Response> {
        if self.something_is_open() {
            return None;
        }
        let (command, button) = match self.hovered {
            Some(command) => (command, self.ribbon.command_rect(command)?),
            None => {
                let bar = self.mini_bar.as_ref()?;
                let command = bar.hovered()?;
                (command, bar.anchor(command)?)
            }
        };
        let label = crate::chrome::tip::label_of(command)?;
        let width = self.view_width as f32;
        self.tip = Some(Tip::new(command, label, button, &mut self.chrome_engine, width));
        self.needs_redraw = true;
        Some(Response::Redraw)
    }

    /// Whether something is dropped open over the ribbon.
    ///
    /// # Why a tip must not appear over it
    ///
    /// Because the button that opened a menu is still under the pointer, and a
    /// tip for it would hang exactly where the first row of the menu is — over
    /// the one thing the person is looking for. Word does not show one: while a
    /// menu is open the tips stop, and they start again when it closes.
    ///
    /// The list of things counted here is the list of things drawn over the
    /// ribbon. The mini toolbar is not one of them: it floats over the text, it
    /// is a row of buttons with no words, and it has tips of its own.
    pub(super) fn something_is_open(&self) -> bool {
        self.popup.is_some()
            || self.palette.is_some()
            || self.table_grid.is_some()
            || self.dialog.is_some()
            || self.backstage.is_some()
            || self.showing_key_tips()
    }

    /// The clock ticked.
    ///
    /// The caret blinks on it, at the rate the system was asked for. Nothing
    /// else happens on most ticks, and a tick that changes nothing costs one
    /// comparison and no drawing — which is what lets the window ask for a
    /// heartbeat at all.
    fn ticked(&mut self) -> Response {
        // The copy that survives the program not closing. First, because a
        // tick spent deciding whether a tip is due is a tick in which the
        // work is still only in memory.
        self.autorecover_tick();

        // The document creeping along under the pointer, while the middle
        // button has it doing that.
        if self.autoscrolling() && self.autoscroll_tick() == Response::Redraw {
            return Response::Redraw;
        }

        // A button the pointer has rested on says what it is — on the ribbon,
        // or on the mini toolbar floating over the text.
        let resting = self.hovered.or_else(|| self.mini_bar.as_ref().and_then(MiniBar::hovered));
        let tip_due = resting.is_some()
            && !self.something_is_open()
            && self.tip.as_ref().map(|tip| tip.command) != resting
            && self.hovered_since.elapsed()
                >= Duration::from_millis(crate::chrome::tip::DELAY_MILLIS);
        if tip_due {
            if let Some(response) = self.show_tip() {
                return response;
            }
        }

        let Some(blink) = self.caret_blink else { return Response::Ignored };
        if self.caret_flipped.elapsed() < blink {
            return Response::Ignored;
        }
        // A caret that is not on the screen is not worth a repaint: scrolled
        // away, or in a part of the window the pages do not reach.
        let Some((_, y, _, height)) = self.caret_rect() else { return Response::Ignored };
        if y < self.content_top() || y + height > self.content_bottom() {
            return Response::Ignored;
        }

        self.caret_flipped = Instant::now();
        self.caret_on = !self.caret_on;
        // Only the caret changed, so only the caret is drawn again.
        self.caret_only = true;
        Response::Redraw
    }

    /// Reacts to the pointer moving.
    fn moved_pointer(&mut self, x: i32, y: i32, held: bool, modifiers: Modifiers) -> Response {
        self.pointer_x = x as f32;
        self.pointer_y = y as f32;

        // The mini toolbar wakes as the pointer comes to it and goes when the
        // pointer leaves — except while the menu it came up with is open,
        // because the pointer is on its way there.
        if !held && self.mini_bar.is_some() {
            let over_menu = self.popup.as_ref().is_some_and(|popup| popup.covers(x, y));
            if !over_menu && self.mini_bar_moved(x, y) {
                return Response::Redraw;
            }
        }

        // Text taken hold of inside the selection is carried until it is let
        // go. The press itself decided nothing; this is where it becomes a
        // drag. See [`dragtext`].
        if self.pending_text_drag.is_some() || self.dragging_text() {
            if !held {
                return self.drop_text(x, y, modifiers.control);
            }
            // Carried out of the window, the text goes to the desktop as a
            // drag for whatever program takes it. See [`super::dropping`].
            if self.dragging_text() && self.is_outside_window(x, y) {
                return self.drag_text_out();
            }
            return self.drag_text(x, y);
        }

        // A stroke being drawn, or an eraser being dragged.
        if let Some(response) = self.ink_move(x, y, held) {
            return response;
        }

        // The band being swept round a handful of drawings, while Select
        // Objects is in hand.
        if self.dragging_band() {
            if !held {
                self.release_band();
                return Response::Redraw;
            }
            return self.drag_band(x, y);
        }

        if self.dragging_shape() {
            if !held {
                self.release_shape();
                if self.pending_text_drag.is_some() || self.dragging_text() {
                    return self.drop_text(x, y, false);
                }
                return Response::Ignored;
            }
            return self.drag_shape(x, y);
        }

        if self.dragging_scrollbar() {
            if !held {
                self.release_scrollbar();
                return Response::Ignored;
            }
            return self.drag_scrollbar(x, y);
        }

        if self.split_dragging {
            if !held {
                self.split_dragging = false;
                return Response::Ignored;
            }
            return self.drag_split_to(y as f32);
        }

        if self.dragging_ruler() {
            if !held {
                self.release_ruler();
                return Response::Ignored;
            }
            return self.drag_ruler(x, y, modifiers);
        }

        if self.resizing_pane {
            if !held {
                self.resizing_pane = false;
            } else if self.navigation.resize_to(x) {
                self.relayout();
                return Response::Redraw;
            } else {
                return Response::Ignored;
            }
        }

        if let Some(palette) = &mut self.palette {
            return if palette.hover(x, y) {
                self.needs_redraw = true;
                Response::Redraw
            } else {
                Response::Ignored
            };
        }

        if let Some(grid) = &mut self.table_grid {
            return if grid.hover(x, y) {
                self.needs_redraw = true;
                Response::Redraw
            } else {
                Response::Ignored
            };
        }

        if let Some(popup) = &mut self.popup {
            return if popup.hover(x, y) {
                self.needs_redraw = true;
                Response::Redraw
            } else {
                Response::Ignored
            };
        }

        // The little button at the end of a paste lights up under the pointer,
        // and takes it from the page behind it.
        if self.follow_paste_badge(x, y) {
            self.needs_redraw = true;
            return Response::Redraw;
        }
        if self.over_paste_badge(x, y) {
            return Response::Ignored;
        }
        // And so does the box under a word AutoCorrect changed, which appears
        // when the pointer rests on the word.
        if self.follow_correction_badge(x, y) {
            self.needs_redraw = true;
            return Response::Redraw;
        }
        if self.over_correction_badge(x, y) {
            return Response::Ignored;
        }

        // Over the File tab, only the File tab lights up.
        if self.in_backstage() {
            let changed = self.backstage_hover(x, y);
            self.needs_redraw |= changed;
            return if changed { Response::Redraw } else { Response::Ignored };
        }

        // Over the Print page, only the Print page lights up.
        if self.printing() {
            let changed = self.print_pane_hover(x, y);
            self.needs_redraw |= changed;
            return if changed { Response::Redraw } else { Response::Ignored };
        }

        // And over the styles pane, only the styles pane.
        if self.over_signature_pane(x) && (y as f32) > self.ribbon_bottom() {
            let changed = self.signature_pane_hover(x, y);
            if changed {
                self.needs_redraw = true;
            }
            return Response::Redraw;
        }
        if self.over_mapping_pane(x) && (y as f32) > self.ribbon_bottom() {
            let changed = self.mapping_pane_hover(x, y);
            if changed {
                self.needs_redraw = true;
            }
            return Response::Redraw;
        }
        if self.over_text_pane(x) && (y as f32) > self.ribbon_bottom() {
            let changed = self.text_pane_hover(x, y);
            if changed {
                self.needs_redraw = true;
            }
            return Response::Redraw;
        }
        if self.over_restrict_pane(x) && (y as f32) > self.ribbon_bottom() {
            let changed = self.restrict_pane_hover(x, y);
            if changed {
                self.needs_redraw = true;
            }
            return Response::Redraw;
        }
        if self.over_styles_pane(x) && (y as f32) > self.ribbon_bottom() {
            let changed = self.styles_pane_hover(x, y);
            self.needs_redraw |= changed;
            return if changed { Response::Redraw } else { Response::Ignored };
        }

        if self.sliding && held {
            if let Some(slider) = self.slider {
                return self.set_zoom(slider.zoom_at(x));
            }
        }

        if !self.dragging || !held {
            self.dragging = false;
            self.sliding = false;

            // Whatever the pointer is over lights up.
            let over =
                if (y as f32) < self.ribbon_bottom() { self.ribbon.command_at(x, y) } else { None };
            let mut changed = over != self.hovered;
            if changed {
                // The pointer has moved to something else, so the tip that was
                // showing is about the wrong button and the wait starts again.
                changed |= self.tip.take().is_some();
                self.hovered_since = Instant::now();
            }
            self.hovered = over;
            changed |= self.titlebar.hover(x, y);
            if let Some(bar) = &mut self.find_bar {
                changed |= bar.hover(x, y);
            }

            let ribbon_bottom = self.ribbon_bottom();
            let content_bottom = self.window_bottom();
            if let Some(pane) = &mut self.recovery {
                changed |= pane.hover(x, y);
            } else if self.show_navigation
                && crate::chrome::mirror::flip_f(x as f32) < self.navigation.width()
            {
                changed |= self.navigation.hover(x, y, ribbon_bottom, content_bottom);
            }

            if !changed {
                return Response::Ignored;
            }
            self.needs_redraw = true;
            return Response::Redraw;
        }

        // A table's handle being dragged.
        if self.handle_drag.is_some() {
            return self.drag_table_handle(x, y);
        }

        // A line of a table being dragged follows the pointer.
        if self.edge_drag.is_some() {
            return self.drag_table_edge(x, y);
        }

        // A drag begun with Alt takes a rectangle, which is worked out from the
        // two corners rather than grown from one end.
        if self.column_drag.is_some() {
            return self.extend_column_drag(x, y);
        }

        // How much the drag takes at a time depends on the click that started
        // it: one begun on a double click goes on taking whole words, and one
        // begun in the selection bar goes on taking whole lines.
        let response = self.extend_drag(x, y);
        if response == Response::Redraw {
            self.reveal_caret();
        }
        response
    }

    /// Drops open the list of fonts, sizes, styles or zooms.
    pub(super) fn open_list(&mut self, choice: Choice) -> Response {
        if self.popup.as_ref().is_some_and(|popup| popup.choice == choice) {
            self.popup = None;
            self.needs_redraw = true;
            return Response::Redraw;
        }

        let command = match choice {
            // The paste options are not on the ribbon: they hang under the
            // little button at the end of the paste, which knows where it is.
            Choice::PasteOption => return self.open_paste_menu(),
            Choice::AutoCorrectOption => return self.open_correction_options(),
            // The gallery of table styles hangs under its own button, which
            // knows where it is.
            Choice::TableStyle => return self.open_table_styles(),
            Choice::TablePart => return self.open_table_select(),
            Choice::AlignObjects => return self.open_align(),
            Choice::RotateObjects => return self.open_rotate(),
            Choice::GroupObjects => return self.open_grouping(),
            // The menus the ribbon's arrows drop are filled in by the module
            // that owns them, which knows what is in each and which of its
            // entries is in force.
            Choice::BulletLibrary
            | Choice::NumberLibrary
            | Choice::MultilevelLibrary
            | Choice::LineSpacing
            | Choice::LetterCase
            | Choice::PageNumberPlace
            | Choice::Selecting
            | Choice::NoteJump
            | Choice::Accepting
            | Choice::Rejecting
            | Choice::Tracking
            | Choice::DocumentSpacing
            | Choice::AutoFit
            | Choice::AlignmentTab => return self.open_ribbon_menu(choice),
            // The two Arrange menus, whose entries depend on nothing but which
            // way they move a drawing.
            Choice::Forward => return self.open_arrange(true),
            Choice::Backward => return self.open_arrange(false),
            Choice::Markup => return self.open_markup_menu(),
            Choice::Translate => return self.open_translate_menu(),
            Choice::BorderStyle => return self.open_border_styles(),
            Choice::Font => Command::ChooseFont,
            Choice::Size => Command::ChooseSize,
            Choice::Style => Command::ChooseStyle,
            Choice::Zoom => Command::ChooseZoom,
            Choice::Border => Command::Borders,
            Choice::Furniture => Command::Header,
            Choice::Reference => Command::CrossReference,
            Choice::Citation => Command::InsertCitation,
            Choice::Source => Command::ManageSources,
            Choice::Watermark => Command::Watermark,
            Choice::Cover => Command::CoverPage,
            Choice::Authority => Command::TableOfAuthorities,
            Choice::Shape => Command::InsertShape,
            Choice::Wrap => Command::WrapText,
            Choice::Position => Command::Position,
            Choice::QuickPart => Command::QuickParts,
            Choice::WordArt => Command::WordArt,
            Choice::Drawing => Command::SelectionPane,
            Choice::MergeKind => Command::StartMailMerge,
            Choice::MergeField => Command::InsertMergeField,
            Choice::Correction => Command::Spelling,
            Choice::Synonym => Command::Thesaurus,
            Choice::Translation => Command::Translate,
            Choice::Accessibility => Command::CheckAccessibility,
            // The menu the right button opens hangs where the pointer was, not
            // under a button of the ribbon.
            Choice::Context => Command::ExpandGroup(0),
            Choice::Group => Command::ExpandGroup(0),
            Choice::ThemeEffects => Command::Effects,
            Choice::Chart => Command::Chart,
            Choice::Rule => Command::Rules,
            Choice::Macro => Command::Macros,
            Choice::Diagram => Command::SmartArt,
            Choice::DiagramLayout => Command::DiagramLayouts,
            Choice::DiagramColours => Command::DiagramColours,
            Choice::PenLook => return self.open_pen_look(super::inking::PenKind::Pen),
            Choice::PencilLook => return self.open_pen_look(super::inking::PenKind::Pencil),
            Choice::HighlighterLook => {
                return self.open_pen_look(super::inking::PenKind::Highlighter)
            }
            Choice::EraserKind => return self.open_erasers(),
            Choice::Screenshot => Command::Screenshot,
            Choice::OutlineLevel => Command::OutlineView,
            Choice::MatchField | Choice::MatchColumn => Command::MatchFields,
            Choice::TextEffect => Command::TextEffects,
            Choice::Envelope => Command::Envelopes,
            Choice::Label => Command::Labels,
            Choice::Language => Command::Language,
            Choice::Theme => Command::Themes,
            Choice::ThemeColors => Command::ThemeColors,
            Choice::ThemeFonts => Command::ThemeFonts,
            Choice::LineNumbers => Command::LineNumbers,
            Choice::Hyphenation => Command::Hyphenation,
            Choice::TextDirection => Command::TextDirectionSection,
            Choice::AsianLayout => Command::AsianLayout,
            Choice::Comparing => Command::Compare,
            Choice::Finishing => Command::FinishMerge,
            Choice::LegacyField => Command::LegacyFields,
            Choice::StyleSet => Command::StyleSet,
            Choice::RecipientSource => Command::SelectRecipients,
            // The AutoText gallery hangs under the Quick Parts button, which
            // already carries a menu of its own: it is a submenu, and it
            // places itself rather than being looked up by button.
            Choice::AutoText => return self.open_auto_text(),
            // And the page-number designs hang under the Page Number button
            // the same way, once the menu above them has said where the
            // number is going.
            Choice::PageNumberDesign => {
                return self.open_page_number_designs(self.page_number_place)
            }
            // The kinds of editing hang beside the Restrict Editing pane,
            // which is not a button of the ribbon and places its own lists.
            Choice::RestrictMode | Choice::MappedControl | Choice::GalleryBlock => {
                return Response::Ignored
            }
            // Hangs where the caret is rather than under a button.
            Choice::FillIn => Command::LegacyFields,
            // The strip's own menu hangs where it was opened, not under a
            // button of the ribbon.
            Choice::StatusBar => Command::ExpandGroup(0),
            Choice::PageNumbering => Command::FormatPageNumbers,
            Choice::Margin => Command::Margins,
            Choice::Orientation => Command::Orientation,
            Choice::Paper => Command::PageSize,
            Choice::Column => Command::Columns,
            Choice::Break => Command::Breaks,
            // The Print page hangs its lists from its own settings, which it
            // has told the editor the place of.
            Choice::Printer | Choice::PrintWhich | Choice::PrintSides | Choice::PrintPerSheet => {
                Command::Print
            }
        };
        // Under the button that asked for it — or, when the mini toolbar asked,
        // under the box of the mini toolbar that was pressed.
        let anchor = self.popup_anchor.or_else(|| self.ribbon.command_rect(command));
        let Some((left, top, width)) = anchor else {
            return Response::Ignored;
        };

        let (items, current) = match choice {
            // The Print page builds its own lists, because they are about the
            // job rather than about the document.
            Choice::Printer | Choice::PrintWhich | Choice::PrintSides | Choice::PrintPerSheet => {
                return Response::Ignored
            }
            Choice::Font => {
                let wanted = self.document.font_here();
                let index = wanted.as_deref().and_then(|name| {
                    self.families.iter().position(|family| family.eq_ignore_ascii_case(name))
                });
                (self.families.clone(), index)
            }
            Choice::Size => {
                let size = self.document.size_here();
                let index = chrome::SIZES.iter().position(|value| (value - size).abs() < 0.01);
                (chrome::SIZES.iter().map(|value| chrome::format_size(*value)).collect(), index)
            }
            Choice::Style => {
                let gallery = self.style_gallery();
                let here = self.document.style_here();
                let index = gallery.iter().position(|sample| sample.id == here);
                (gallery.into_iter().map(|sample| sample.name).collect(), index)
            }
            // The symbol list is opened by its own command, which fills it in.
            // These two are opened by their own commands, which fill them in.
            Choice::Border
            | Choice::Furniture
            | Choice::Reference
            | Choice::Citation
            | Choice::Source
            | Choice::LineNumbers
            | Choice::Hyphenation
            | Choice::TextDirection
            | Choice::AsianLayout
            | Choice::Comparing
            | Choice::Finishing
            | Choice::LegacyField
            | Choice::StyleSet
            | Choice::RecipientSource
            | Choice::AutoText
            | Choice::FillIn
            | Choice::StatusBar
            | Choice::PageNumbering
            | Choice::Margin
            | Choice::Orientation
            | Choice::Paper
            | Choice::Column
            | Choice::Break
            | Choice::Watermark
            | Choice::PasteOption
            | Choice::AutoCorrectOption
            | Choice::TableStyle
            | Choice::BulletLibrary
            | Choice::NumberLibrary
            | Choice::TablePart
            | Choice::AutoFit
            | Choice::AlignObjects
            | Choice::RotateObjects
            | Choice::GroupObjects
            | Choice::AlignmentTab
            | Choice::MultilevelLibrary
            | Choice::LineSpacing
            | Choice::LetterCase
            | Choice::PageNumberPlace
            | Choice::Selecting
            | Choice::NoteJump
            | Choice::Accepting
            | Choice::Rejecting
            | Choice::Tracking
            | Choice::DocumentSpacing
            | Choice::Cover
            | Choice::Authority
            | Choice::Theme
            | Choice::ThemeColors
            | Choice::ThemeFonts
            | Choice::Language
            | Choice::Shape
            | Choice::Wrap
            | Choice::Position
            | Choice::Forward
            | Choice::Backward
            | Choice::Markup
            | Choice::BorderStyle
            | Choice::QuickPart
            | Choice::WordArt
            | Choice::Drawing
            | Choice::MergeKind
            | Choice::MergeField
            | Choice::Correction
            | Choice::Synonym
            | Choice::Translate
            | Choice::Translation
            | Choice::Accessibility
            | Choice::Context
            | Choice::Group
            | Choice::ThemeEffects
            | Choice::Chart
            | Choice::Rule
            | Choice::Macro
            | Choice::Diagram
            | Choice::DiagramLayout
            | Choice::DiagramColours
            | Choice::PenLook
            | Choice::PencilLook
            | Choice::HighlighterLook
            | Choice::EraserKind
            | Choice::Screenshot
            | Choice::OutlineLevel
            | Choice::MatchField
            | Choice::MatchColumn
            | Choice::TextEffect
            | Choice::Envelope
            | Choice::PageNumberDesign
            | Choice::RestrictMode
            | Choice::MappedControl
            | Choice::GalleryBlock
            | Choice::Label => (Vec::new(), None),
            Choice::Zoom => {
                let index = chrome::ZOOMS.iter().position(|value| (value - self.zoom).abs() < 0.5);
                (chrome::ZOOMS.iter().map(|value| format!("{}%", *value as i32)).collect(), index)
            }
        };

        self.popup = Some(Popup::new(choice, items, current, left, top, width));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Applies whichever item of the open list was pressed.
    fn choose(&mut self, index: usize) -> Response {
        let Some(popup) = &self.popup else { return Response::Ignored };
        let Some(text) = popup.item(index).map(str::to_owned) else {
            return Response::Ignored;
        };
        let choice = popup.choice;
        self.popup = None;

        match choice {
            Choice::Font => {
                let changed = self.document.set_font(&text);
                self.finish_character_change(changed, &format!("Font {text}"))
            }
            Choice::Size => match text.parse::<f32>() {
                Ok(points) => {
                    let changed = self.document.set_size(points);
                    self.finish_character_change(changed, &format!("{text} point"))
                }
                Err(_) => self.report("Not a size"),
            },
            Choice::Style => {
                let style = self.style_gallery().get(index).and_then(|sample| sample.id.clone());
                self.apply_style(style.as_deref())
            }
            Choice::Border => self.choose_border(index),
            Choice::Furniture => self.choose_furniture(index),
            Choice::Reference => self.choose_reference(index),
            Choice::Citation => self.choose_citation(index),
            Choice::Source => self.choose_source(index),
            Choice::Watermark => self.choose_watermark(index),
            // Everything the ribbon's arrows drop goes to one place, which is
            // where each of those menus was filled in.
            Choice::BulletLibrary
            | Choice::NumberLibrary
            | Choice::MultilevelLibrary
            | Choice::LineSpacing
            | Choice::LetterCase
            | Choice::PageNumberPlace
            | Choice::Selecting
            | Choice::NoteJump
            | Choice::Accepting
            | Choice::Rejecting
            | Choice::Tracking
            | Choice::DocumentSpacing
            | Choice::AlignmentTab => self.choose_from_menu(choice, index),
            Choice::PasteOption => self.choose_paste_option(index),
            Choice::AutoCorrectOption => self.choose_correction_option(index),
            Choice::TableStyle => self.choose_table_style(index),
            Choice::Cover => self.choose_cover_page(index),
            Choice::Authority => self.choose_authorities(index),
            Choice::Shape => self.choose_shape(index),
            Choice::TablePart => self.choose_table_part(index),
            Choice::AutoFit => self.choose_autofit(index),
            Choice::AlignObjects => self.choose_align(index),
            Choice::RotateObjects => self.choose_rotate(index),
            Choice::GroupObjects => self.choose_grouping(index),
            Choice::Wrap => self.choose_wrapping(index),
            Choice::Position => self.choose_position(index),
            Choice::Forward => self.choose_arrange(true, index),
            Choice::Backward => self.choose_arrange(false, index),
            Choice::Markup => self.choose_markup(index),
            Choice::BorderStyle => self.choose_border_style(index),
            Choice::QuickPart => self.choose_quick_part(index),
            Choice::WordArt => self.choose_word_art(index),
            Choice::Drawing => self.choose_drawing(index),
            Choice::MergeKind => self.choose_merge_kind(index),
            Choice::MergeField => self.choose_merge_field(index),
            Choice::Correction => self.choose_correction(index),
            Choice::Synonym => self.take_synonym(index),
            Choice::Translate => self.choose_translate(index),
            Choice::Translation => self.take_translation(index),
            Choice::Accessibility => self.choose_accessibility(index),
            Choice::Context => self.choose_context_entry(index),
            Choice::Group => self.choose_group_command(index),
            Choice::ThemeEffects => self.choose_theme_effects(index),
            Choice::Chart => self.choose_chart(index),
            Choice::Rule => self.choose_rule(index),
            Choice::Macro => self.choose_macro(index),
            Choice::Diagram => self.choose_diagram(index),
            Choice::DiagramLayout => self.choose_diagram_layout(index),
            Choice::DiagramColours => self.choose_diagram_colours(index),
            Choice::PenLook => self.choose_pen_look(super::inking::PenKind::Pen, index),
            Choice::PencilLook => self.choose_pen_look(super::inking::PenKind::Pencil, index),
            Choice::HighlighterLook => {
                self.choose_pen_look(super::inking::PenKind::Highlighter, index)
            }
            Choice::EraserKind => self.choose_eraser(index),
            Choice::Screenshot => self.choose_screenshot(index),
            Choice::OutlineLevel => self.choose_outline_level(index),
            Choice::MatchField => self.choose_match_field(index),
            Choice::MatchColumn => self.choose_match_column(index),
            Choice::TextEffect => self.choose_text_effect(index),
            Choice::Envelope => self.choose_envelope(index),
            Choice::Label => self.choose_label_sheet(index),
            Choice::Language => self.choose_language(index),
            Choice::Theme => self.choose_theme(index),
            Choice::ThemeColors => self.choose_theme_colors(index),
            Choice::ThemeFonts => self.choose_theme_fonts(index),
            Choice::LineNumbers => self.choose_line_numbers(index),
            Choice::Hyphenation => self.choose_hyphenation(index),
            Choice::TextDirection => self.choose_text_direction(index),
            Choice::AsianLayout => self.choose_asian_layout(index),
            Choice::Comparing => self.choose_comparing(index),
            Choice::Finishing => self.choose_finishing(index),
            Choice::LegacyField => self.choose_legacy_field(index),
            Choice::StyleSet => self.choose_style_set(index),
            Choice::RecipientSource => self.choose_recipient_source(index),
            Choice::AutoText => self.choose_auto_text(index),
            Choice::PageNumberDesign => self.choose_page_number_design(index),
            Choice::RestrictMode => self.choose_restrict_mode(index),
            Choice::MappedControl => self.choose_mapped_control(index),
            Choice::GalleryBlock => self.choose_gallery_block(index),
            Choice::FillIn => self.choose_fill_in(index),
            Choice::StatusBar => self.choose_status_part(index),
            Choice::PageNumbering => self.choose_page_numbering(index),
            Choice::Margin => self.choose_margins(index),
            Choice::Orientation => self.choose_orientation(index),
            Choice::Paper => self.choose_page_size(index),
            Choice::Column => self.choose_columns(index),
            Choice::Break => self.choose_break(index),
            Choice::Printer | Choice::PrintWhich | Choice::PrintSides | Choice::PrintPerSheet => {
                self.choose_print_setting(choice, index)
            }
            Choice::Zoom => {
                let percent = text.trim_end_matches('%').parse::<f32>().unwrap_or(self.zoom);
                self.set_zoom(percent)
            }
        }
    }

    /// Finishes a change that may only affect what is typed next.
    pub(super) fn finish_character_change(&mut self, changed: bool, note: &str) -> Response {
        self.status = note.to_owned();
        if changed {
            self.relayout();
            self.reveal_caret();
            self.update_title();
        }
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Types one character into the document, and records it if a macro is
    /// being recorded.
    ///
    /// The one place typing happens, so that a macro sees every character and
    /// no path can type without being seen.
    pub(super) fn type_character(&mut self, character: char) -> Response {
        self.record_typing(character);
        // What was typed may not be what goes in, and the word before it may
        // not stay as it was. See [`super::correcting`].
        let character = self.correct_character(character);
        let changed = self.document.type_text(&character.to_string());
        self.correct_word(character);
        self.edited(changed, "")
    }

    /// Reacts to a key that is not ordinary typing.
    fn key(&mut self, key: Key, modifiers: Modifiers) -> Response {
        // A measurement box on the ribbon has the keyboard while the caret is
        // in it: Enter applies what was typed, Escape gives it up, Tab moves to
        // the next box, and the arrows nudge the number.
        if self.typing_in_box() && !modifiers.control {
            match key {
                Key::Enter => return self.finish_box(),
                Key::Escape => return self.leave_box(),
                Key::Tab => return self.next_box(!modifiers.shift),
                Key::Backspace => return self.rub_out_in_box(),
                Key::Up | Key::Down => {
                    let Some((command, _)) = self.ribbon_box else { return Response::Ignored };
                    return self.step_box(command, key == Key::Up);
                }
                _ => return Response::Ignored,
            }
        }

        // The Text Pane takes the keys that walk and edit the chosen
        // diagram's words, while it is open with a diagram chosen.
        if self.text_pane_has_keyboard()
            && !modifiers.control
            && self.popup.is_none()
            && !self.showing_key_tips()
        {
            let response = self.text_pane_key(key, modifiers);
            if response != Response::Ignored {
                return response;
            }
        }

        // An open list takes the keyboard while it is open: the arrows walk it,
        // Enter picks, Escape closes. Word's lists do the same, and a list that
        // could only be reached with the mouse would be half a list.
        if self.popup.is_some() && !modifiers.control && !self.showing_key_tips() {
            match key {
                Key::Up | Key::Down => {
                    let step = if key == Key::Down { 1 } else { -1 };
                    let moved = self.popup.as_mut().is_some_and(|popup| popup.move_by(step));
                    self.needs_redraw |= moved;
                    return if moved { Response::Redraw } else { Response::Ignored };
                }
                Key::Home | Key::End => {
                    let moved =
                        self.popup.as_mut().is_some_and(|popup| popup.move_to_end(key == Key::End));
                    self.needs_redraw |= moved;
                    return if moved { Response::Redraw } else { Response::Ignored };
                }
                Key::Enter => {
                    let Some(index) = self.popup.as_ref().and_then(Popup::highlighted) else {
                        return Response::Ignored;
                    };
                    return self.choose(index);
                }
                Key::Escape => {
                    self.popup = None;
                    self.needs_redraw = true;
                    return Response::Redraw;
                }
                _ => {}
            }
        }

        // The File tab has the keyboard while it is showing. Escape goes back
        // to the document, and the arrows walk the rail, which is how every
        // list in the program is walked.
        if self.in_backstage() && self.popup.is_none() && !modifiers.control {
            return match key {
                Key::Escape => self.close_backstage(),
                Key::Up => self.walk_the_rail(false),
                Key::Down => self.walk_the_rail(true),
                _ => Response::Ignored,
            };
        }

        // The Print page has the keyboard while it is showing: Escape goes
        // back to the document, and the arrows walk the pages.
        if self.printing() && self.popup.is_none() && !modifiers.control {
            return match key {
                Key::Escape => self.close_print(),
                Key::Left | Key::PageUp | Key::Up => self.turn_print_page(false),
                Key::Right | Key::PageDown | Key::Down => self.turn_print_page(true),
                Key::Backspace => {
                    if self.print_pane_character('\u{8}') {
                        Response::Redraw
                    } else {
                        Response::Ignored
                    }
                }
                Key::Enter => self.start_printing(),
                _ => Response::Ignored,
            };
        }

        // While the letters are showing over the ribbon, the keyboard is
        // driving the ribbon and nothing else. Word behaves the same: the
        // document does not get the keystroke that picked a command.
        if self.showing_key_tips() && !modifiers.control {
            return match key {
                Key::Escape => self.leave_key_tips(),
                Key::Letter(letter) => self.press_key_tip(letter),
                Key::Digit(digit) => self.press_key_tip(digit),
                _ => {
                    self.hide_key_tips();
                    Response::Redraw
                }
            };
        }

        if modifiers.control {
            // Ctrl+Alt+1, 2, 3 apply the heading levels, as they do in Word —
            // and the rest of what Word puts on Ctrl+Alt is here too, because
            // this returns before anything below it is looked at.
            if modifiers.alt {
                return match key {
                    Key::Digit(level @ '1'..='3') => {
                        let style: &'static str = match level {
                            '1' => "Heading1",
                            '2' => "Heading2",
                            _ => "Heading3",
                        };
                        self.apply_style(Some(style))
                    }
                    // The characters Word gives keys of their own. Shown beside
                    // each one in the Symbol dialog, and working — which is the
                    // difference between a dialog that documents this program
                    // and one that describes some other program.
                    // Word's key for AutoFormat Now.
                    Key::Letter('k') => self.run(Command::AutoFormatNow),
                    Key::Letter('c') => self.insert_special_character("Copyright"),
                    Key::Letter('r') => self.insert_special_character("Registered"),
                    Key::Letter('t') => self.insert_special_character("Trademark"),
                    Key::Digit('.') => self.insert_special_character("Ellipsis"),
                    Key::Digit('-') if modifiers.shift => self.insert_special_character("Em Dash"),
                    Key::Digit('-') => self.insert_special_character("En Dash"),
                    _ => Response::Ignored,
                };
            }

            return match key {
                Key::Letter('n') if !modifiers.shift => self.run(Command::New),
                // The characters Word gives keys of their own. Shown beside
                // each one in the Symbol dialog, and working, which is the
                // difference between a dialog that documents this program and
                // one that describes some other program. First, because every
                // one of these letters means something else without Alt.
                Key::Letter('c') if modifiers.alt => self.insert_special_character("Copyright"),
                Key::Letter('r') if modifiers.alt => self.insert_special_character("Registered"),
                Key::Letter('t') if modifiers.alt => self.insert_special_character("Trademark"),
                Key::Digit('.') if modifiers.alt => self.insert_special_character("Ellipsis"),
                Key::Digit('-') if modifiers.alt && modifiers.shift => {
                    self.insert_special_character("Em Dash")
                }
                Key::Digit('-') if modifiers.alt => self.insert_special_character("En Dash"),
                Key::Digit('-') if modifiers.shift => {
                    self.insert_special_character("Nonbreaking Hyphen")
                }
                Key::Digit('-') => self.insert_special_character("Optional Hyphen"),
                Key::Space if modifiers.shift => self.insert_special_character("Nonbreaking Space"),

                Key::Letter('o') => self.run(Command::Open),
                Key::Letter('p') => self.run(Command::Print),
                // Word's own shortcut for the styles pane, which has to come
                // before the other two on this letter or they swallow it.
                Key::Letter('s') if modifiers.shift && modifiers.alt => {
                    self.run(Command::StylesPane)
                }
                Key::Letter('s') if modifiers.shift => self.run(Command::SaveAs),
                Key::Letter('s') => self.run(Command::Save),
                Key::Letter('a') => self.run(Command::SelectAll),
                Key::Letter('c') => self.run(Command::Copy),
                Key::Letter('x') => self.run(Command::Cut),
                // Shift with it pastes the words alone, which is Word's Keep
                // Text Only.
                Key::Letter('v') if modifiers.shift => self.paste_plain(),
                Key::Letter('v') => self.run(Command::Paste),
                Key::Letter('f') => self.run(Command::Find),
                Key::Letter('k') => self.run(Command::InsertLink),
                Key::Letter('h') => self.run(Command::Replace),
                // Ctrl+Shift+Z redoes as well, which is what a hand trained on
                // one convention or the other will reach for.
                Key::Letter('z') if modifiers.shift => self.run(Command::Redo),
                Key::Letter('z') => self.run(Command::Undo),
                Key::Letter('y') => self.run(Command::Redo),

                Key::Letter('b') => self.run(Command::Format(CharacterFormat::Bold)),
                Key::Letter('i') => self.run(Command::Format(CharacterFormat::Italic)),
                Key::Letter('u') => self.run(Command::Format(CharacterFormat::Underline)),
                Key::Letter('n') if modifiers.shift => self.apply_style(None),
                // Word's own shortcut for the Font dialog.
                Key::Letter('d') => self.run(Command::FontDialog),

                Key::Letter('l') => self.run(Command::Align(Alignment::Start)),
                Key::Letter('e') => self.run(Command::Align(Alignment::Center)),
                Key::Letter('r') => self.run(Command::Align(Alignment::End)),
                Key::Letter('j') => self.run(Command::Align(Alignment::Both)),
                Key::Letter('m') if modifiers.shift => self.run(Command::IndentLess),
                Key::Letter('m') => self.run(Command::IndentMore),

                Key::Digit('8') if modifiers.shift => self.run(Command::Bullets),
                Key::Digit('=') => self.run(Command::ZoomIn),

                // Two of the three breaks the keyboard puts in: Ctrl+Enter
                // ends the page and Ctrl+Shift+Enter ends the column.
                Key::Enter if modifiers.shift => self.press_column_break(),
                Key::Enter => self.run(Command::PageBreak),

                Key::Home | Key::End => {
                    self.move_to_document_edge(key == Key::End, modifiers.shift);
                    self.moved()
                }

                // Moving and deleting by the word, which is what Ctrl does to
                // every one of these keys in every editor.
                Key::Left => {
                    self.document.word_left(modifiers.shift);
                    self.moved()
                }
                Key::Right => {
                    self.document.word_right(modifiers.shift);
                    self.moved()
                }
                Key::Up => {
                    self.document.paragraph_up(modifiers.shift);
                    self.moved()
                }
                Key::Down => {
                    self.document.paragraph_down(modifiers.shift);
                    self.moved()
                }
                Key::Backspace | Key::Delete if self.is_locked() => self.refuse_locked(),
                // A control somebody marked as one that cannot be deleted is
                // one they meant to keep, whatever is selected round it.
                Key::Backspace | Key::Delete if self.would_delete_a_locked_control().is_some() => {
                    let named = self.would_delete_a_locked_control().unwrap_or_default();
                    self.refuse_deleting_a_control(&named)
                }
                Key::Backspace => {
                    let changed = self.document.delete_word_back();
                    self.edited(changed, "")
                }
                Key::Delete => {
                    let changed = self.document.delete_word_forward();
                    self.edited(changed, "")
                }
                // Tab moves between cells, so a tab inside a cell has to be
                // asked for another way. Word's answer is Ctrl+Tab, and it is
                // the same key outside a table, where it simply types one.
                Key::Tab => {
                    let changed = self.document.type_text("\t");
                    self.edited(changed, "")
                }

                _ => Response::Ignored,
            };
        }

        let extend = modifiers.shift;
        // On a line that runs down the page, the arrows are read as what
        // they do on the page: down is along the text. See
        // [`Editor::arrow_on_the_page`].
        let key = self.arrow_on_the_page(key);
        match key {
            // The function keys Word has always had, which a person who has
            // used Word reaches for without thinking.
            Key::Function(7) if modifiers.shift => self.run(Command::Thesaurus),
            Key::Function(7) => self.run(Command::Spelling),
            Key::Function(12) if modifiers.shift => self.run(Command::Save),
            Key::Function(12) => self.run(Command::SaveAs),
            Key::Function(1) => self.run(Command::ShowTraining),
            Key::Function(_) => Response::Ignored,
            Key::Left => {
                self.document.caret_left(extend);
                self.moved()
            }
            Key::Right => {
                self.document.caret_right(extend);
                self.moved()
            }
            Key::Up | Key::Down => {
                self.move_vertically(key == Key::Down, extend);
                self.moved()
            }
            Key::Home | Key::End => {
                self.move_to_line_edge(key == Key::End, extend);
                self.moved()
            }
            Key::PageDown => self.scroll_by(self.viewport_height() * 0.9),
            Key::PageUp => self.scroll_by(-(self.viewport_height() * 0.9)),
            // Shift+Enter ends the line without ending the paragraph.
            Key::Enter if extend => self.press_line_break(),
            Key::Enter => {
                // Three hyphens on a line of their own become a line under the
                // paragraph above, and Enter has done its work.
                if self.correct_paragraph_end() {
                    return self.edited(true, "");
                }
                let changed = self.document.press_enter();
                self.edited(changed, "")
            }
            // A space with no modifier arrives as typing rather than as a key,
            // so there is nothing to do here with one that got this far.
            Key::Space => Response::Ignored,
            // A control somebody marked as one that cannot be deleted is one
            // they meant to keep, whatever is selected round it.
            Key::Backspace | Key::Delete if self.would_delete_a_locked_control().is_some() => {
                let named = self.would_delete_a_locked_control().unwrap_or_default();
                self.refuse_deleting_a_control(&named)
            }
            Key::Backspace => {
                let changed = self.document.backspace();
                self.edited(changed, "")
            }
            Key::Delete => {
                let changed = self.document.delete_forward();
                self.edited(changed, "")
            }
            // In a table Tab is how the caret gets about: it moves a cell on,
            // or with Shift a cell back, and never types anything. Outside one
            // it types a tab like any other key.
            Key::Tab if self.document.table_here().is_some() => self.step_cell(!extend),
            Key::Tab => {
                let changed = self.document.type_text("\t");
                self.edited(changed, "")
            }
            // Escape closes whatever is open, and then drops the selection. It
            // does not close the window: a word processor that threw away the
            // document on a stray keypress would be one nobody could trust.
            Key::Escape => {
                // Escape gives up whatever is being carried, which is what it
                // does to every drag.
                if self.in_furniture() {
                    return self.leave_furniture();
                }
                if self.dragging_text() {
                    self.cancel_text_drag();
                    self.needs_redraw = true;
                    return Response::Redraw;
                }
                // A pen in hand is something being carried too.
                if self.painting_borders() {
                    return self.toggle_border_painter();
                }
                if let Some(pen) = self.table_pen {
                    return self.toggle_table_pen(pen);
                }
                // A drawing chosen is given up before Select Objects is put
                // down: the two are one gesture undone in the order it was
                // made, and Word gives them up in that order as well.
                if self.drop_chosen_drawing() {
                    return Response::Redraw;
                }
                if self.inking() {
                    return self.put_ink_tool_down();
                }
                if self.choosing_drawings() {
                    return self.toggle_choosing_drawings();
                }
                // Reading mode is left the way it is left in every reader.
                if !self.view.shows_furniture() {
                    return self.set_view(crate::editor::views::View::Print);
                }
                if self.palette.take().is_some() || self.table_grid.take().is_some() {
                    self.needs_redraw = true;
                    return Response::Redraw;
                }
                if self.find_bar.is_some() {
                    return self.close_find();
                }
                if self.popup.take().is_some() {
                    self.needs_redraw = true;
                    return Response::Redraw;
                }
                // The little button at the end of a paste goes the same way as
                // anything else Escape closes.
                if self.offering_paste_options() {
                    self.forget_paste();
                    return Response::Redraw;
                }
                if self.offering_correction_options() {
                    self.forget_correction();
                    return Response::Redraw;
                }
                if self.document.selection().is_some() {
                    self.document.clear_selection();
                    self.needs_redraw = true;
                    return Response::Redraw;
                }
                Response::Ignored
            }
            // Ordinary typing arrives as a character, already composed by the
            // system, so these keys mean nothing on their own.
            Key::Letter(_) | Key::Digit(_) => Response::Ignored,
        }
    }
}

#[cfg(test)]
mod tip_tests {
    use super::*;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    /// An editor that has been drawn once, so the ribbon knows where its
    /// buttons are.
    fn editor() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Words")));
        let bytes = Document::create(&body).expect("a document").save().expect("saving");
        let document = Document::open(&bytes).expect("reopening");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor.draw(1400, 900);
        editor
    }

    /// Rests the pointer on a button for longer than a tip waits.
    fn rest_on(editor: &mut Editor, command: Command) {
        editor.hovered = Some(command);
        editor.hovered_since =
            Instant::now() - Duration::from_millis(crate::chrome::tip::DELAY_MILLIS + 50);
    }

    /// Drops a menu open under that button, as pressing it would.
    fn open_a_menu(editor: &mut Editor) {
        let items = vec!["One".to_owned(), "Two".to_owned()];
        editor.popup = Some(Popup::new(Choice::LineSpacing, items, None, 100.0, 100.0, 200.0));
    }

    #[test]
    fn a_button_the_pointer_rests_on_says_what_it_is() {
        let mut editor = editor();
        rest_on(&mut editor, Command::Format(CharacterFormat::Bold));
        assert!(editor.show_tip().is_some(), "no tip appeared");
        assert_eq!(
            editor.tip.as_ref().map(|tip| tip.command),
            Some(Command::Format(CharacterFormat::Bold))
        );
    }

    #[test]
    fn no_tip_appears_while_a_menu_is_open() {
        // The button that dropped the menu is still under the pointer, and a
        // tip for it would hang exactly over the menu's first row.
        let mut editor = editor();
        open_a_menu(&mut editor);
        rest_on(&mut editor, Command::Format(CharacterFormat::Bold));
        assert!(editor.show_tip().is_none(), "a tip appeared over the menu");
        assert!(editor.tip.is_none());
    }

    #[test]
    fn a_tip_already_up_is_forgotten_when_a_menu_opens() {
        let mut editor = editor();
        rest_on(&mut editor, Command::Format(CharacterFormat::Bold));
        editor.show_tip();
        assert!(editor.tip.is_some(), "there was no tip to take away");

        open_a_menu(&mut editor);
        editor.draw(1400, 900);
        assert!(editor.tip.is_none(), "the tip stayed over the menu");
    }

    #[test]
    fn the_tip_comes_back_once_the_menu_closes() {
        let mut editor = editor();
        open_a_menu(&mut editor);
        rest_on(&mut editor, Command::Format(CharacterFormat::Bold));
        assert!(editor.show_tip().is_none());

        editor.popup = None;
        rest_on(&mut editor, Command::Format(CharacterFormat::Bold));
        assert!(editor.show_tip().is_some(), "the tips did not start again");
    }

    #[test]
    fn a_dialog_stops_the_tips_too() {
        let mut editor = editor();
        editor.open_word_count();
        rest_on(&mut editor, Command::Format(CharacterFormat::Bold));
        assert!(editor.show_tip().is_none(), "a tip appeared over a dialog");
    }
}
