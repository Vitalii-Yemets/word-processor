//! Editing a diagram: the Text Pane, and the two tabs Word shows while one
//! is chosen.
//!
//! # What editing a diagram is
//!
//! Changing its words. A diagram is a tree of words with a layout that
//! draws them — see [`wp_docx::diagram`] — so every edit here is an edit to
//! the tree, after which the document lays the picture out again: a box
//! added is a word added, a box promoted is a word lifted a level, a
//! different arrangement is the same words drawn by a different layout.
//! Nothing here moves a shape, because the shapes are not what a diagram
//! is made of.
//!
//! # The pane
//!
//! Word's Text Pane lists the words one to a line and takes typing. Here
//! it is a pane down the side, like the others, open while a diagram is
//! chosen and asked for. The line with the caret is the box being typed
//! into; Enter adds a box after it, Backspace on an empty line takes the
//! box away, Tab hangs the box under the one above and Shift+Tab lifts it
//! out, and the arrows move between the lines.

use wp_docx::diagram::{Arrangement, Colouring, Node};
use wp_docx::model::DiagramReference;
use wp_shell::{Key, Modifiers, Response};

use crate::chrome::textpane::{Hit, Shown, WIDTH};
use crate::chrome::{Choice, Command, Popup};
use crate::messages::t;

use super::Editor;

/// One line of the pane: how deep the box hangs, and its words.
type Line = (u8, String);

/// The tree as the pane lists it: a node before its children, each a level
/// deeper than its parent.
fn flatten(nodes: &[Node], level: u8, out: &mut Vec<Line>) {
    for node in nodes {
        out.push((level, node.text.clone()));
        flatten(&node.children, level.saturating_add(1), out);
    }
}

/// The lines as a tree: a line hangs under the nearest line above it that
/// is shallower, and a line with none above it is a root.
fn hang(lines: &[Line]) -> Vec<Node> {
    let mut roots: Vec<Node> = Vec::new();
    // The way down from a root to the last line placed: the level each
    // step stands at, and which child it is.
    let mut path: Vec<(u8, usize)> = Vec::new();
    for (level, text) in lines {
        while path.last().is_some_and(|(held, _)| *held >= *level) {
            path.pop();
        }
        let node = Node::new(text);
        match path.first() {
            None => {
                roots.push(node);
                path.push((*level, roots.len() - 1));
            }
            Some((_, root)) => {
                let mut parent = &mut roots[*root];
                for (_, index) in &path[1..] {
                    parent = &mut parent.children[*index];
                }
                parent.children.push(node);
                path.push((*level, parent.children.len() - 1));
            }
        }
    }
    roots
}

impl Editor {
    /// The diagram the chosen drawing is, if the chosen drawing is one.
    #[must_use]
    pub(super) fn chosen_diagram(&self) -> Option<DiagramReference> {
        let at = self.chosen_drawings.first()?;
        self.document.diagram_at(*at)
    }

    /// How much of the window the pane takes, when it is open.
    #[must_use]
    pub(super) fn text_pane_width(&self) -> f32 {
        if self.show_text_pane {
            WIDTH
        } else {
            0.0
        }
    }

    /// Where its left edge is.
    pub(super) fn text_pane_left(&self) -> f32 {
        self.view_width as f32 - WIDTH
    }

    /// Whether a point is inside it at all.
    pub(super) fn over_text_pane(&self, x: i32) -> bool {
        let x = crate::chrome::mirror::flip(x);
        self.show_text_pane && (x as f32) >= self.text_pane_left()
    }

    /// Word's Text Pane button: opens the pane, or shuts it again.
    pub(super) fn toggle_text_pane(&mut self) -> Response {
        if self.show_text_pane {
            return self.close_text_pane();
        }
        self.open_text_pane()
    }

    pub(super) fn open_text_pane(&mut self) -> Response {
        // The panes down the right-hand side share one strip of window, and
        // two at once is one hidden behind the other.
        self.show_styles = false;
        self.show_restrict = false;
        self.show_signatures = false;
        self.show_mapping = false;
        self.show_text_pane = true;
        self.diagram_line = 0;
        self.diagram_caret = usize::MAX;
        self.clamp_scroll();
        self.needs_redraw = true;
        Response::Redraw
    }

    pub(super) fn close_text_pane(&mut self) -> Response {
        self.show_text_pane = false;
        self.clamp_scroll();
        self.needs_redraw = true;
        Response::Redraw
    }

    /// The lines of the chosen diagram, as the pane lists them.
    fn diagram_lines(&self) -> Vec<Line> {
        let Some(reference) = self.chosen_diagram() else { return Vec::new() };
        let Some(diagram) = self.document.diagram(&reference) else { return Vec::new() };
        let mut lines = Vec::new();
        flatten(&diagram.nodes, 0, &mut lines);
        lines
    }

    /// Everything the pane draws, worked out afresh from the document.
    pub(super) fn text_pane_shown(&self) -> Shown {
        let lines = self.diagram_lines();
        let chosen = self.diagram_line.min(lines.len().saturating_sub(1));
        let length = lines.get(chosen).map_or(0, |(_, text)| text.chars().count());
        let arrangement = self
            .chosen_diagram()
            .and_then(|reference| self.document.diagram(&reference))
            .and_then(|diagram| diagram.arrangement)
            .map(|arrangement| t(arrangement.label()).to_owned())
            .unwrap_or_default();
        Shown { lines, chosen, caret: self.diagram_caret.min(length), arrangement }
    }

    /// Whether the pane has the keyboard: open, with a diagram to type into.
    pub(super) fn text_pane_has_keyboard(&self) -> bool {
        self.show_text_pane && self.chosen_diagram().is_some()
    }

    /// A press inside the pane.
    pub(super) fn text_pane_press(&mut self, x: i32, y: i32) -> Response {
        match self.text_pane.at(x, y) {
            Some(Hit::Close) => self.close_text_pane(),
            Some(Hit::Line(index)) => {
                self.diagram_line = index;
                // The caret lands on the character the press was nearest,
                // measured by the pane's own font.
                let along = self.text_pane.along(index, x);
                let text =
                    self.diagram_lines().get(index).map(|(_, t)| t.clone()).unwrap_or_default();
                let mut caret = text.chars().count();
                for count in 0..=text.chars().count() {
                    let before: String = text.chars().take(count).collect();
                    let width = if before.is_empty() {
                        0.0
                    } else {
                        let line = self.chrome_engine.simple_line(
                            &before,
                            0.0,
                            0.0,
                            crate::chrome::pane::TEXT,
                            self.theme.text,
                        );
                        line.width
                    };
                    if width >= along {
                        caret = count;
                        break;
                    }
                }
                self.diagram_caret = caret;
                self.needs_redraw = true;
                Response::Redraw
            }
            None => Response::Ignored,
        }
    }

    /// The pointer moving over it.
    pub(super) fn text_pane_hover(&mut self, x: i32, y: i32) -> bool {
        self.text_pane.hover(x, y)
    }

    /// Draws it, over the document's right-hand edge.
    pub(super) fn draw_text_pane(&mut self) {
        if !self.show_text_pane {
            return;
        }
        let shown = self.text_pane_shown();
        let left = self.text_pane_left();
        let top = self.ribbon_bottom();
        let bottom = self.window_bottom();
        let theme = self.theme;

        let mut pane = core::mem::take(&mut self.text_pane);
        pane.draw(
            &mut self.canvas,
            &mut self.chrome_engine,
            &mut self.renderer,
            &shown,
            left,
            top,
            bottom,
            &theme,
        );
        self.text_pane = pane;
    }

    /// Puts new lines into the chosen diagram and lays it out again.
    fn set_diagram_lines(&mut self, lines: &[Line]) -> Response {
        let Some(reference) = self.chosen_diagram() else { return Response::Ignored };
        let nodes = hang(lines);
        if !self.document.set_diagram_nodes(&reference, &nodes) {
            return Response::Ignored;
        }
        self.relayout();
        self.needs_redraw = true;
        Response::Redraw
    }

    /// A character typed into the pane goes into the chosen line.
    pub(super) fn text_pane_character(&mut self, character: char) -> Response {
        if character.is_control() {
            return Response::Ignored;
        }
        let mut lines = self.diagram_lines();
        let Some((_, text)) = lines.get_mut(self.diagram_line) else { return Response::Ignored };
        let caret = self.diagram_caret.min(text.chars().count());
        let at = text.char_indices().nth(caret).map_or(text.len(), |(index, _)| index);
        text.insert(at, character);
        self.diagram_caret = caret + 1;
        self.set_diagram_lines(&lines)
    }

    /// A key pressed while the pane has the keyboard.
    pub(super) fn text_pane_key(&mut self, key: Key, modifiers: Modifiers) -> Response {
        let mut lines = self.diagram_lines();
        if lines.is_empty() {
            return Response::Ignored;
        }
        let line = self.diagram_line.min(lines.len() - 1);
        let length = lines[line].1.chars().count();
        let caret = self.diagram_caret.min(length);
        match key {
            Key::Escape => self.close_text_pane(),
            Key::Up => {
                self.diagram_line = line.saturating_sub(1);
                self.needs_redraw = true;
                Response::Redraw
            }
            Key::Down => {
                self.diagram_line = (line + 1).min(lines.len() - 1);
                self.needs_redraw = true;
                Response::Redraw
            }
            Key::Left => {
                self.diagram_caret = caret.saturating_sub(1);
                self.needs_redraw = true;
                Response::Redraw
            }
            Key::Right => {
                self.diagram_caret = (caret + 1).min(length);
                self.needs_redraw = true;
                Response::Redraw
            }
            Key::Home => {
                self.diagram_caret = 0;
                self.needs_redraw = true;
                Response::Redraw
            }
            Key::End => {
                self.diagram_caret = length;
                self.needs_redraw = true;
                Response::Redraw
            }
            // Enter splits the line at the caret into a box and a new box
            // after it at the same level, as Word's pane does.
            Key::Enter => {
                let (level, text) = lines[line].clone();
                let split = text.char_indices().nth(caret).map_or(text.len(), |(index, _)| index);
                let (before, after) = text.split_at(split);
                lines[line] = (level, before.to_owned());
                lines.insert(line + 1, (level, after.to_owned()));
                self.diagram_line = line + 1;
                self.diagram_caret = 0;
                self.set_diagram_lines(&lines)
            }
            // Backspace rubs a character out — or, at the start of a line,
            // joins the box onto the one before, which is how a box is taken
            // away.
            Key::Backspace => {
                if caret > 0 {
                    let text = &mut lines[line].1;
                    let at = text.char_indices().nth(caret - 1).map(|(index, _)| index);
                    if let Some(at) = at {
                        text.remove(at);
                    }
                    self.diagram_caret = caret - 1;
                    return self.set_diagram_lines(&lines);
                }
                if line == 0 {
                    return Response::Ignored;
                }
                let (_, taken) = lines.remove(line);
                let previous = &mut lines[line - 1].1;
                self.diagram_caret = previous.chars().count();
                previous.push_str(&taken);
                self.diagram_line = line - 1;
                self.set_diagram_lines(&lines)
            }
            Key::Delete => {
                let text = &mut lines[line].1;
                if caret < length {
                    let at = text.char_indices().nth(caret).map(|(index, _)| index);
                    if let Some(at) = at {
                        text.remove(at);
                    }
                    return self.set_diagram_lines(&lines);
                }
                if line + 1 < lines.len() {
                    let (_, taken) = lines.remove(line + 1);
                    lines[line].1.push_str(&taken);
                    return self.set_diagram_lines(&lines);
                }
                Response::Ignored
            }
            Key::Tab if modifiers.shift => self.promote_box(),
            Key::Tab => self.demote_box(),
            _ => Response::Ignored,
        }
    }

    /// Word's Add Shape: a box after the chosen one, at the same level.
    pub(super) fn add_box(&mut self) -> Response {
        let mut lines = self.diagram_lines();
        if lines.is_empty() {
            return self.report(t("Choose a diagram to add a box to"));
        }
        let line = self.diagram_line.min(lines.len() - 1);
        let level = lines[line].0;
        lines.insert(line + 1, (level, t("[Text]").to_owned()));
        self.diagram_line = line + 1;
        self.diagram_caret = 0;
        let response = self.set_diagram_lines(&lines);
        if response == Response::Redraw {
            self.open_text_pane();
        }
        response
    }

    /// Word's Promote: the chosen box lifted a level, out from under the
    /// box it hangs under.
    pub(super) fn promote_box(&mut self) -> Response {
        let mut lines = self.diagram_lines();
        let Some((level, _)) = lines.get_mut(self.diagram_line) else {
            return Response::Ignored;
        };
        if *level == 0 {
            return Response::Ignored;
        }
        *level -= 1;
        self.set_diagram_lines(&lines)
    }

    /// Word's Demote: the chosen box hung under the one before it.
    pub(super) fn demote_box(&mut self) -> Response {
        let mut lines = self.diagram_lines();
        let line = self.diagram_line;
        if line == 0 || line >= lines.len() {
            return Response::Ignored;
        }
        // No deeper than one under the box before, or it would hang under
        // nothing.
        let above = lines[line - 1].0;
        if lines[line].0 > above {
            return Response::Ignored;
        }
        lines[line].0 += 1;
        self.set_diagram_lines(&lines)
    }

    /// Word's Move Up and Move Down: the chosen box swapped with the one
    /// before or after it.
    pub(super) fn move_box(&mut self, up: bool) -> Response {
        let mut lines = self.diagram_lines();
        let line = self.diagram_line;
        let other = if up { line.checked_sub(1) } else { line.checked_add(1) };
        let Some(other) = other.filter(|other| *other < lines.len() && line < lines.len()) else {
            return Response::Ignored;
        };
        lines.swap(line, other);
        self.diagram_line = other;
        self.set_diagram_lines(&lines)
    }

    /// Word's Right to Left: the diagram turned round.
    pub(super) fn turn_diagram_round(&mut self) -> Response {
        let Some(reference) = self.chosen_diagram() else { return Response::Ignored };
        let reads_right_to_left =
            self.document.diagram(&reference).is_some_and(|diagram| diagram.right_to_left);
        if !self.document.set_diagram_direction(&reference, !reads_right_to_left) {
            return Response::Ignored;
        }
        self.relayout();
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Word's Layouts gallery: the arrangements a diagram can take.
    pub(super) fn open_diagram_layouts(&mut self) -> Response {
        if self.close_popup_if(Choice::DiagramLayout) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::DiagramLayouts) else {
            return Response::Ignored;
        };
        let current = self
            .chosen_diagram()
            .and_then(|reference| self.document.diagram(&reference))
            .and_then(|diagram| diagram.arrangement)
            .and_then(|arrangement| Arrangement::ALL.iter().position(|a| *a == arrangement));
        let items = Arrangement::ALL.iter().map(|entry| t(entry.label()).to_owned()).collect();
        self.popup = Some(Popup::new(Choice::DiagramLayout, items, current, left, top, 240.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// One arrangement chosen from the gallery.
    pub(super) fn choose_diagram_layout(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some(arrangement) = Arrangement::ALL.get(index).copied() else {
            return Response::Ignored;
        };
        let Some(reference) = self.chosen_diagram() else { return Response::Ignored };
        if !self.document.set_diagram_arrangement(&reference, arrangement) {
            return Response::Ignored;
        }
        self.relayout();
        self.needs_redraw = true;
        self.report(t(arrangement.label()))
    }

    /// Word's Change Colors: which of the theme's colours the boxes take.
    pub(super) fn open_diagram_colours(&mut self) -> Response {
        if self.close_popup_if(Choice::DiagramColours) {
            return Response::Redraw;
        }
        let Some((left, top, _)) = self.ribbon.command_rect(Command::DiagramColours) else {
            return Response::Ignored;
        };
        let current = self
            .chosen_diagram()
            .and_then(|reference| self.document.diagram(&reference))
            .and_then(|diagram| diagram.colouring)
            .and_then(|colouring| Colouring::ALL.iter().position(|c| *c == colouring));
        let items = Colouring::ALL.iter().map(|entry| t(entry.label()).to_owned()).collect();
        self.popup = Some(Popup::new(Choice::DiagramColours, items, current, left, top, 260.0));
        self.needs_redraw = true;
        Response::Redraw
    }

    /// One colouring chosen.
    pub(super) fn choose_diagram_colours(&mut self, index: usize) -> Response {
        self.popup = None;
        let Some(colouring) = Colouring::ALL.get(index).copied() else {
            return Response::Ignored;
        };
        let Some(reference) = self.chosen_diagram() else { return Response::Ignored };
        if !self.document.set_diagram_colouring(&reference, colouring) {
            return Response::Ignored;
        }
        self.relayout();
        self.needs_redraw = true;
        self.report(t(colouring.label()))
    }

    /// Word's Reset Graphic: the first accent again, read left to right,
    /// which is how a diagram starts.
    pub(super) fn reset_diagram(&mut self) -> Response {
        let Some(reference) = self.chosen_diagram() else { return Response::Ignored };
        let colours = self.document.set_diagram_colouring(&reference, Colouring::default());
        let direction = self.document.set_diagram_direction(&reference, false);
        if !colours && !direction {
            return Response::Ignored;
        }
        self.relayout();
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Word's Larger and Smaller, on the Format tab: the frame grown or
    /// shrunk by a fifth, and the picture laid out again to fill it.
    pub(super) fn resize_diagram(&mut self, larger: bool) -> Response {
        let Some(reference) = self.chosen_diagram() else { return Response::Ignored };
        let factor = if larger { 1.2 } else { 1.0 / 1.2 };
        if !self.document.resize_diagram(&reference, factor) {
            return Response::Ignored;
        }
        self.relayout();
        self.needs_redraw = true;
        Response::Redraw
    }

    /// Choosing a diagram brings its tab up, as choosing a table does.
    pub(super) fn note_diagram_chosen(&mut self) {
        if self.chosen_diagram().is_some() {
            self.ribbon.tab = crate::chrome::ribbon::Tab::SmartArtDesign;
            self.diagram_line = 0;
            self.diagram_caret = usize::MAX;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_docx::model::{Block, Body, Paragraph};
    use wp_docx::Document;
    use wp_layout::FontLibrary;
    use wp_shell::{App, Event};

    use crate::chrome::ribbon::Tab;

    fn library() -> &'static FontLibrary {
        Box::leak(Box::new(FontLibrary::scan_system()))
    }

    /// An editor with a process diagram of three boxes chosen, and the Text
    /// Pane open on it.
    fn editor_with_diagram() -> Editor {
        let mut body = Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("Before after")));
        let document = Document::create(&body).expect("a document");
        let mut editor = Editor::new(library(), document, None);
        editor.handle(Event::Resized { width: 1400, height: 900 });
        editor.document.set_caret(wp_docx::TextPosition::new(0, 7));
        let items: Vec<String> =
            ["Plan", "Draw", "Check"].iter().map(|s| (*s).to_owned()).collect();
        let room = editor.text_width_emu();
        assert!(editor
            .document
            .insert_diagram(Arrangement::Process, &items, room)
            .expect("inserting"));
        editor.relayout();
        editor.choose_drawing_here();
        editor.note_diagram_chosen();
        editor.open_text_pane();
        editor
    }

    fn words(editor: &Editor) -> Vec<(u8, String)> {
        editor.diagram_lines()
    }

    fn key(editor: &mut Editor, key: Key) {
        editor.handle(Event::KeyDown { key, modifiers: Modifiers::default() });
    }

    fn shift_key(editor: &mut Editor, key: Key) {
        editor.handle(Event::KeyDown {
            key,
            modifiers: Modifiers { shift: true, ..Modifiers::default() },
        });
    }

    #[test]
    fn choosing_a_diagram_brings_its_tab_up_and_the_pane_lists_its_words() {
        let editor = editor_with_diagram();
        assert!(editor.chosen_diagram().is_some(), "the diagram was not chosen");
        assert_eq!(editor.ribbon.tab, Tab::SmartArtDesign);
        assert!(Tab::SmartArtDesign.applies(&editor.toolbar_state()));
        assert!(Tab::SmartArtFormat.applies(&editor.toolbar_state()));
        let shown = editor.text_pane_shown();
        assert_eq!(
            shown.lines,
            vec![(0, "Plan".to_owned()), (0, "Draw".to_owned()), (0, "Check".to_owned())]
        );
        assert_eq!(shown.arrangement, "Basic Process");
    }

    #[test]
    fn typing_in_the_pane_changes_the_box_and_the_picture() {
        let mut editor = editor_with_diagram();
        editor.diagram_line = 1;
        editor.diagram_caret = usize::MAX;
        for character in " it".chars() {
            editor.handle(Event::Char(character));
        }
        assert_eq!(words(&editor)[1].1, "Draw it");
        // The drawing carries the new words: the frame is laid out again.
        let reference = editor.chosen_diagram().expect("the diagram");
        let diagram = editor.document.diagram(&reference).expect("read back");
        let drawn = diagram.drawing.expect("a drawing");
        let boxes: Vec<String> = drawn
            .members
            .iter()
            .filter_map(|member| match &member.what {
                wp_docx::group::Inside::Shape(shape) if !shape.text.is_empty() => {
                    Some(shape.text[0].plain_text())
                }
                _ => None,
            })
            .collect();
        assert_eq!(boxes, vec!["Plan", "Draw it", "Check"]);
    }

    #[test]
    fn enter_adds_a_box_and_backspace_at_the_start_takes_it_away() {
        let mut editor = editor_with_diagram();
        editor.diagram_line = 0;
        editor.diagram_caret = usize::MAX;
        key(&mut editor, Key::Enter);
        assert_eq!(words(&editor).len(), 4);
        assert_eq!(editor.diagram_line, 1);
        for character in "New".chars() {
            editor.handle(Event::Char(character));
        }
        assert_eq!(words(&editor)[1].1, "New");

        editor.diagram_caret = 0;
        key(&mut editor, Key::Backspace);
        assert_eq!(words(&editor).len(), 3);
        assert_eq!(words(&editor)[0].1, "PlanNew");
    }

    #[test]
    fn tab_hangs_a_box_under_the_one_above_and_shift_tab_lifts_it_out() {
        let mut editor = editor_with_diagram();
        editor.diagram_line = 1;
        key(&mut editor, Key::Tab);
        assert_eq!(words(&editor)[1].0, 1, "Draw did not go under Plan");
        let reference = editor.chosen_diagram().expect("the diagram");
        let diagram = editor.document.diagram(&reference).expect("read back");
        assert_eq!(diagram.nodes.len(), 2);
        assert_eq!(diagram.nodes[0].children[0].text, "Draw");

        shift_key(&mut editor, Key::Tab);
        assert_eq!(words(&editor)[1].0, 0);
        // The first box has nothing to hang under.
        editor.diagram_line = 0;
        key(&mut editor, Key::Tab);
        assert_eq!(words(&editor)[0].0, 0);
    }

    #[test]
    fn the_boxes_are_moved_up_and_down_and_the_words_go_with_them() {
        let mut editor = editor_with_diagram();
        editor.diagram_line = 2;
        editor.move_box(true);
        assert_eq!(words(&editor)[1].1, "Check");
        assert_eq!(editor.diagram_line, 1);
        editor.move_box(false);
        assert_eq!(words(&editor)[2].1, "Check");
    }

    #[test]
    fn add_shape_puts_a_box_after_the_chosen_one() {
        let mut editor = editor_with_diagram();
        editor.diagram_line = 0;
        editor.run(Command::DiagramAddShape);
        assert_eq!(words(&editor).len(), 4);
        assert_eq!(words(&editor)[1].1, "[Text]");
    }

    #[test]
    fn the_layout_and_the_colours_can_be_changed_and_the_file_says_so() {
        let mut editor = editor_with_diagram();
        let hierarchy = Arrangement::ALL.iter().position(|a| *a == Arrangement::Hierarchy).unwrap();
        editor.choose_diagram_layout(hierarchy);
        let reference = editor.chosen_diagram().expect("the diagram");
        let diagram = editor.document.diagram(&reference).expect("read back");
        assert_eq!(diagram.arrangement, Some(Arrangement::Hierarchy));
        // A row becomes a tree with the first box over the rest.
        assert_eq!(diagram.nodes.len(), 1);
        assert_eq!(diagram.nodes[0].children.len(), 2);

        let colourful = Colouring::ALL.iter().position(|c| *c == Colouring::Colorful).unwrap();
        editor.choose_diagram_colours(colourful);
        let diagram = editor.document.diagram(&reference).expect("read back");
        assert_eq!(diagram.colouring, Some(Colouring::Colorful));

        editor.run(Command::DiagramRightToLeft);
        let diagram = editor.document.diagram(&reference).expect("read back");
        assert!(diagram.right_to_left);

        editor.run(Command::DiagramReset);
        let diagram = editor.document.diagram(&reference).expect("read back");
        assert!(!diagram.right_to_left);
        assert_eq!(diagram.colouring, Some(Colouring::Accent1));
    }

    #[test]
    fn larger_and_smaller_change_the_frame() {
        let mut editor = editor_with_diagram();
        let before = editor.chosen_diagram().expect("the diagram");
        editor.run(Command::DiagramLarger);
        let after = editor.chosen_diagram().expect("the diagram");
        assert!(after.width_emu > before.width_emu);
        editor.run(Command::DiagramSmaller);
        let back = editor.chosen_diagram().expect("the diagram");
        assert!(back.width_emu < after.width_emu);
    }

    #[test]
    fn every_edit_is_one_undo() {
        let mut editor = editor_with_diagram();
        editor.diagram_line = 0;
        key(&mut editor, Key::Enter);
        assert_eq!(words(&editor).len(), 4);
        assert!(editor.document.undo());
        assert_eq!(words(&editor).len(), 3);
    }

    #[test]
    fn the_pane_shuts_with_escape_and_the_other_panes_shut_it() {
        let mut editor = editor_with_diagram();
        assert!(editor.show_text_pane);
        key(&mut editor, Key::Escape);
        assert!(!editor.show_text_pane);
        editor.open_text_pane();
        editor.open_mapping();
        assert!(!editor.show_text_pane);
        assert!(editor.show_mapping);
    }

    #[test]
    fn lines_hang_under_the_line_above_when_they_are_deeper() {
        let lines = vec![
            (0, "Top".to_owned()),
            (1, "Under".to_owned()),
            (2, "Deeper".to_owned()),
            (1, "Also under".to_owned()),
            (0, "Second".to_owned()),
        ];
        let tree = hang(&lines);
        assert_eq!(tree.len(), 2);
        assert_eq!(tree[0].children.len(), 2);
        assert_eq!(tree[0].children[0].children[0].text, "Deeper");
        let mut back = Vec::new();
        flatten(&tree, 0, &mut back);
        assert_eq!(back, lines);
    }

    #[test]
    fn a_line_deeper_than_anything_above_it_is_read_at_the_top() {
        let lines = vec![(2, "Stray".to_owned()), (0, "Top".to_owned())];
        let tree = hang(&lines);
        assert_eq!(tree.len(), 2);
        assert_eq!(tree[0].text, "Stray");
    }
}
