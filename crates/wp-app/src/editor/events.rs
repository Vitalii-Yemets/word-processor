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
            if fx < self.navigation.width() {
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

        // Over the page area: an I-beam where there is a place for the caret,
        // and an arrow over the desk beside and between the pages.
        if self.position_at(x, y).is_some() {
            Cursor::Text
        } else {
            Cursor::Arrow
        }
    }

    fn handle(&mut self, event: Event) -> Response {
        match event {
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
                // Over the navigation pane the wheel scrolls the outline.
                if self.show_navigation && self.pointer_x < self.navigation.width() {
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
                self.release_shape();
                if self.pending_text_drag.is_some() || self.dragging_text() {
                    return self.drop_text(x, y, false);
                }
                self.split_dragging = false;
                self.release_ruler();
                let was_dragging = self.dragging;
                self.dragging = false;
                self.sliding = false;
                self.resizing_pane = false;
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
                // The pane's search box takes the keyboard while it has it,
                // the way any box with a caret in it does.
                if self.find_has_keyboard() {
                    return self.type_into_find(character);
                }
                if self.show_navigation && self.navigation.searching {
                    return self.type_into_search(character);
                }
                if self.is_locked() {
                    return self.refuse_locked();
                }
                // Typing is somebody saying they were not after the bar.
                self.hide_mini_bar();
                self.type_character(character)
            }

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
                Response::Ignored
            }
        }
    }

    fn switch_window(&mut self, index: usize) {
        self.use_window(index);
    }

    fn draw(&mut self, width: usize, height: usize) -> &Canvas {
        self.paint(width, height);
        self.canvas()
    }
}

impl Editor {
    /// Reacts to a press, wherever in the window it landed.
    fn pressed(&mut self, x: i32, y: i32, modifiers: Modifiers) -> Response {
        // The mini toolbar takes a press before anything else, because it is
        // floating over whatever is under it.
        if let Some(response) = self.mini_bar_press(x, y) {
            return response;
        }
        // A press anywhere else puts it away, and then goes on to mean whatever
        // it would have meant. A tip goes with it: the button has been found.
        self.hide_mini_bar();
        self.hide_key_tips();
        self.stop_autoscroll();
        self.tip = None;

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
                if let Some(button) = self.titlebar.window_button_at(x, y) {
                    wp_shell::window_command(match button {
                        chrome::WindowButton::Minimise => wp_shell::WindowCommand::Minimise,
                        chrome::WindowButton::Maximise => wp_shell::WindowCommand::ToggleMaximise,
                        chrome::WindowButton::Close => wp_shell::WindowCommand::Close,
                    });
                }
                return Response::Ignored;
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

        // The find strip sits between the ribbon and the page.
        if self.find_bar.is_some()
            && (y as f32) >= self.ribbon_bottom()
            && (y as f32) < self.ribbon_bottom() + self.find_bar_height()
        {
            return self.pressed_in_find(x, y);
        }

        // Reading mode draws no ribbon, so nothing up there is pressable — and
        // the places its buttons were last drawn are no longer where they are.
        if (y as f32) < self.ribbon_bottom() && self.view.shows_furniture() {
            if let Some(tab) = self.ribbon.tab_at(x, y) {
                self.ribbon.tab = tab;
                self.needs_redraw = true;
                return Response::Redraw;
            }
            if let Some(command) = self.ribbon.command_at(x, y) {
                return self.run(command);
            }
            return Response::Ignored;
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

        if self.show_navigation {
            let ribbon_bottom = self.ribbon_bottom();
            let content_bottom = self.window_bottom();

            // The handle down the pane's edge, which is outside the pane by
            // half its width and so has to be tested before it.
            if self.navigation.on_splitter(x, y, ribbon_bottom, content_bottom) {
                self.resizing_pane = true;
                return Response::Ignored;
            }
            if (x as f32) < self.navigation.width() {
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

        // A drawing is taken hold of before the page underneath it is asked
        // about the press: a press on a picture is about the picture.
        if self.press_on_shape(x, y) {
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
        // in a document being written is text first and a link second. Where
        // there is no link, it takes the sentence, as Word's does.
        if modifiers.control {
            self.document.move_caret(position, false);
            if self.document.hyperlink_here().is_some() {
                return self.follow_link();
            }
            return self.select_sentence_at(x, y);
        }

        // Shift+click reaches from where the caret already is, which is how a
        // selection is made without dragging.
        self.document.move_caret(position, modifiers.shift);
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

    /// The clock ticked.
    ///
    /// The caret blinks on it, at the rate the system was asked for. Nothing
    /// else happens on most ticks, and a tick that changes nothing costs one
    /// comparison and no drawing — which is what lets the window ask for a
    /// heartbeat at all.
    fn ticked(&mut self) -> Response {
        // The document creeping along under the pointer, while the middle
        // button has it doing that.
        if self.autoscrolling() && self.autoscroll_tick() == Response::Redraw {
            return Response::Redraw;
        }

        // A button the pointer has rested on says what it is — on the ribbon,
        // or on the mini toolbar floating over the text.
        let resting = self.hovered.or_else(|| self.mini_bar.as_ref().and_then(MiniBar::hovered));
        let tip_due = resting.is_some()
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
        let Some((_, y, height)) = self.caret_rect() else { return Response::Ignored };
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
            return self.drag_text(x, y);
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

        // Over the Print page, only the Print page lights up.
        if self.printing() {
            let changed = self.print_pane_hover(x, y);
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
            if self.show_navigation && (x as f32) < self.navigation.width() {
                changed |= self.navigation.hover(x, y, ribbon_bottom, content_bottom);
            }

            if !changed {
                return Response::Ignored;
            }
            self.needs_redraw = true;
            return Response::Redraw;
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
            Choice::Font => Command::ChooseFont,
            Choice::Size => Command::ChooseSize,
            Choice::Style => Command::ChooseStyle,
            Choice::Zoom => Command::ChooseZoom,
            Choice::Symbol => Command::InsertSymbol,
            Choice::Border => Command::Borders,
            Choice::Furniture => Command::Header,
            Choice::Reference => Command::CrossReference,
            Choice::Citation => Command::InsertCitation,
            Choice::Source => Command::ManageSources,
            Choice::Watermark => Command::Watermark,
            Choice::Property => Command::DocumentProperties,
            Choice::Cover => Command::CoverPage,
            Choice::Authority => Command::TableOfAuthorities,
            Choice::Shape => Command::InsertShape,
            Choice::Wrap => Command::WrapText,
            Choice::Position => Command::Position,
            Choice::QuickPart => Command::QuickParts,
            Choice::WordArt => Command::WordArt,
            Choice::Drawing => Command::SelectionPane,
            Choice::MergeKind => Command::StartMailMerge,
            Choice::Recipient => Command::EditRecipientList,
            Choice::MergeField => Command::InsertMergeField,
            Choice::Correction => Command::Spelling,
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
            Choice::Screenshot => Command::Screenshot,
            Choice::OutlineLevel => Command::OutlineView,
            Choice::MatchField | Choice::MatchColumn => Command::MatchFields,
            Choice::TextEffect => Command::TextEffects,
            Choice::Envelope => Command::Envelopes,
            Choice::Label => Command::Labels,
            Choice::TableProperty => Command::TableProperties,
            Choice::Language => Command::Language,
            Choice::Theme => Command::Themes,
            Choice::ThemeColors => Command::ThemeColors,
            Choice::ThemeFonts => Command::ThemeFonts,
            Choice::LineNumbers => Command::LineNumbers,
            Choice::Hyphenation => Command::Hyphenation,
            Choice::Protection => Command::RestrictEditing,
            // The strip's own menu hangs where it was opened, not under a
            // button of the ribbon.
            Choice::StatusBar | Choice::TabStop => Command::ExpandGroup(0),
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
            Choice::Symbol
            | Choice::Border
            | Choice::Furniture
            | Choice::Reference
            | Choice::Citation
            | Choice::Source
            | Choice::LineNumbers
            | Choice::Hyphenation
            | Choice::Protection
            | Choice::StatusBar
            | Choice::TabStop
            | Choice::PageNumbering
            | Choice::Margin
            | Choice::Orientation
            | Choice::Paper
            | Choice::Column
            | Choice::Break
            | Choice::Watermark
            | Choice::Property
            | Choice::Cover
            | Choice::Authority
            | Choice::Theme
            | Choice::ThemeColors
            | Choice::ThemeFonts
            | Choice::Language
            | Choice::Shape
            | Choice::Wrap
            | Choice::Position
            | Choice::QuickPart
            | Choice::WordArt
            | Choice::Drawing
            | Choice::MergeKind
            | Choice::Recipient
            | Choice::MergeField
            | Choice::Correction
            | Choice::Accessibility
            | Choice::Context
            | Choice::Group
            | Choice::ThemeEffects
            | Choice::Chart
            | Choice::Rule
            | Choice::Macro
            | Choice::Diagram
            | Choice::Screenshot
            | Choice::OutlineLevel
            | Choice::MatchField
            | Choice::MatchColumn
            | Choice::TextEffect
            | Choice::Envelope
            | Choice::Label
            | Choice::TableProperty => (Vec::new(), None),
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
            Choice::Symbol => self.choose_symbol(index),
            Choice::Border => self.choose_border(index),
            Choice::Furniture => self.choose_furniture(index),
            Choice::Reference => self.choose_reference(index),
            Choice::Citation => self.choose_citation(index),
            Choice::Source => self.choose_source(index),
            Choice::Watermark => self.choose_watermark(index),
            Choice::Property => self.choose_property(index),
            Choice::Cover => self.choose_cover_page(index),
            Choice::Authority => self.choose_authorities(index),
            Choice::Shape => self.choose_shape(index),
            Choice::Wrap => self.choose_wrapping(index),
            Choice::Position => self.choose_position(index),
            Choice::QuickPart => self.choose_quick_part(index),
            Choice::WordArt => self.choose_word_art(index),
            Choice::Drawing => self.choose_drawing(index),
            Choice::MergeKind => self.choose_merge_kind(index),
            Choice::Recipient => self.choose_recipient(index),
            Choice::MergeField => self.choose_merge_field(index),
            Choice::Correction => self.choose_correction(index),
            Choice::Accessibility => self.choose_accessibility(index),
            Choice::Context => self.choose_context_entry(index),
            Choice::Group => self.choose_group_command(index),
            Choice::ThemeEffects => self.choose_theme_effects(index),
            Choice::Chart => self.choose_chart(index),
            Choice::Rule => self.choose_rule(index),
            Choice::Macro => self.choose_macro(index),
            Choice::Diagram => self.choose_diagram(index),
            Choice::Screenshot => self.choose_screenshot(index),
            Choice::OutlineLevel => self.choose_outline_level(index),
            Choice::MatchField => self.choose_match_field(index),
            Choice::MatchColumn => self.choose_match_column(index),
            Choice::TextEffect => self.choose_text_effect(index),
            Choice::Envelope => self.choose_envelope(index),
            Choice::Label => self.choose_label_sheet(index),
            Choice::TableProperty => self.choose_table_property(index),
            Choice::Language => self.choose_language(index),
            Choice::Theme => self.choose_theme(index),
            Choice::ThemeColors => self.choose_theme_colors(index),
            Choice::ThemeFonts => self.choose_theme_fonts(index),
            Choice::LineNumbers => self.choose_line_numbers(index),
            Choice::Hyphenation => self.choose_hyphenation(index),
            Choice::Protection => self.choose_protection(index),
            Choice::StatusBar => self.choose_status_part(index),
            Choice::TabStop => self.choose_tab_stop_entry(index),
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
        let changed = self.document.type_text(&character.to_string());
        self.edited(changed, "")
    }

    /// Reacts to a key that is not ordinary typing.
    fn key(&mut self, key: Key, modifiers: Modifiers) -> Response {
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
            // Ctrl+Alt+1, 2, 3 apply the heading levels, as they do in Word.
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
                    _ => Response::Ignored,
                };
            }

            return match key {
                Key::Letter('n') if !modifiers.shift => self.run(Command::New),
                Key::Letter('o') => self.run(Command::Open),
                Key::Letter('p') => self.run(Command::Print),
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
                Key::Backspace => {
                    let changed = self.document.delete_word_back();
                    self.edited(changed, "")
                }
                Key::Delete => {
                    let changed = self.document.delete_word_forward();
                    self.edited(changed, "")
                }

                _ => Response::Ignored,
            };
        }

        let extend = modifiers.shift;
        match key {
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
                let changed = self.document.press_enter();
                self.edited(changed, "")
            }
            Key::Backspace => {
                let changed = self.document.backspace();
                self.edited(changed, "")
            }
            Key::Delete => {
                let changed = self.document.delete_forward();
                self.edited(changed, "")
            }
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
