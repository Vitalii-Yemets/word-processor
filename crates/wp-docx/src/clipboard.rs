//! Copying and pasting with the formatting kept.
//!
//! # Why the plain text is not enough
//!
//! The system clipboard carries text. Copying a bold heading and pasting it
//! gives back the words and nothing else — not the size, not the weight, not
//! the style it was in. Inside one program that is a loss nobody accepts: a
//! paragraph moved from one page to another has to arrive looking the way it
//! left.
//!
//! So a copy does two things. It puts the words on the system clipboard, so
//! that every other program on the machine can have them; and it keeps the
//! formatted content here, so that a paste back into this program can put it
//! down whole. Which of the two a paste uses is decided by whether the
//! clipboard still holds the words that were copied — if something else has
//! copied since, the system's text wins, because that is what the person last
//! asked for.
//!
//! # What comes across
//!
//! Paragraphs, their properties, their runs and everything in a run: the
//! formatting, the tracked changes, the fields, the links, and every drawing
//! and equation. Not a table as a table — selecting across one copies the text of
//! its cells as paragraphs, which is what the plain text of a table is anyway.
//!
//! # Where a copy is cut
//!
//! Where the caret would cut it. A selection is a stretch of the text the
//! caret moves through, and that text leaves out what somebody deleted while
//! changes were tracked and counts a drawing as one character. So the
//! paragraph is cut in its own element, by the same code that splits a run
//! when something is typed into the middle of it, and only then read: a copy
//! measured any other way takes a different stretch from the one that was
//! selected. What takes no room in the text — a deletion, a mark — is in the
//! copy when it lies inside the stretch, and at an end only when that end is
//! the paragraph's own: there is nothing else it could belong to.
//!
//! # Tracked changes
//!
//! Word carries them across as they are, when nobody is tracking changes
//! where the copy lands: a deletion in the copy is pasted as a deletion, an
//! insertion as an insertion, each with its author. When changes are being
//! tracked there, the paste is a change itself — the copy as it would be with
//! its changes accepted, recorded as one insertion by whoever is pasting.
//! This does the same.
//!
//! # Drawings, and the parts they point at
//!
//! A picture is not in the paragraph that shows it. The paragraph holds a
//! drawing, the drawing names a relationship of the part it is in, and the
//! relationship names another part of the package, where the bytes are; a
//! chart's part names a workbook in turn, and a diagram is five parts. None of
//! that is in the text, and none of it survives being taken out of the
//! package unless it is taken too.
//!
//! So a copy of a drawing is its element, exactly as it was written, and
//! every part the element reaches, with every part those reach — see
//! [`Copied`]. A paste puts the parts into the package it lands in under
//! names nothing there has, makes the relationships again with identifiers
//! of that part's own, and points the element at them. A picture already in
//! that package byte for byte is pointed at again rather than kept twice; a
//! chart, a diagram and ink are always copied, because each can be edited,
//! and editing one must not change the other.

use wp_opc::{Relationships, TargetMode, RELATIONSHIPS_NAMESPACE};
use wp_xml::tree::{Element, Node, XmlTree};

use crate::model::{
    Block, Paragraph, ParagraphProperties, Revision, RevisionKind, Run, RunContent, RunProperties,
};
use crate::read::W;
use crate::{edit, read, Document, TextPosition};

/// How much of the copied formatting comes across on a paste.
///
/// Word offers this every time, because the answer depends on why the text was
/// copied. Text taken from a heading and dropped into a paragraph is usually
/// wanted as a paragraph; a paragraph moved from one page to another is wanted
/// exactly as it was.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Formatting {
    /// Word's Keep Source Formatting: it arrives looking the way it left — the
    /// runs and the shape of the paragraphs both.
    #[default]
    Source,
    /// Word's Merge Formatting: the emphasis comes across and nothing else, so
    /// the text takes the font, the size, the colour and the style of where it
    /// lands.
    ///
    /// What counts as emphasis is what Word keeps: bold, italic, underline,
    /// the two strikethroughs, and whether the text rides above or below the
    /// line. A word that was bold in a heading is still bold in a paragraph;
    /// it is not still twenty-eight point.
    Merged,
}

/// A drawing or an equation taken out of a document, with everything it
/// needs to be put down again somewhere else.
///
/// Made by a copy, and settled by a paste: until then the element names the
/// relationships of the part it was copied from, and only the paste knows the
/// part it is going into. See the note at the top of this file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Copied {
    /// What it is, for everything that reads a copy without pasting it.
    pub content: RunContent,
    /// The element it was written as, declaring every namespace it uses, so
    /// that it means the same thing wherever it lands.
    element: Element,
    /// The relationships of the part it stood in that the element names.
    links: Vec<Link>,
    /// Every part those reach, and every part those reach in turn, once each.
    parts: Vec<Part>,
    /// Which of the namespaces it uses its document let a reader pass over.
    ignorable: Vec<String>,
    /// For the mark of a footnote or an endnote, what the note says, copied
    /// the same way. The note is in another part, and a mark pasted without
    /// it points at the note of that number where it lands — the same note
    /// twice in its own document, and some other note or none in another.
    /// Word makes the pasted mark a note of its own, and so does this.
    note: Option<Vec<Block>>,
    /// For the mark a link leaves on each run it holds: which link, by the
    /// place among the runs read with it where the link began, so that two
    /// links side by side stay two. The element is then the `w:hyperlink`
    /// with nothing in it, and the runs go back inside it when they are
    /// written. See [`Copied::is_link`].
    link: Option<usize>,
}

impl Copied {
    /// The element it was written as.
    #[must_use]
    pub fn element(&self) -> &Element {
        &self.element
    }

    /// Whether it can be written anywhere just as it is.
    ///
    /// True when the element names no relationship of the part it was copied
    /// from: an equation, a group of shapes, or anything a paste has already
    /// pointed at the parts of the package it went into. Anything else is
    /// written only by a paste, which settles it first; written as it is, it
    /// would point at whatever that part's relationships of the same names
    /// happen to be.
    #[must_use]
    pub fn is_settled(&self) -> bool {
        self.links.is_empty() && self.note.is_none()
    }

    /// The parts the copy carries, by the names they had where it was made,
    /// with their bytes.
    pub fn parts(&self) -> impl Iterator<Item = (&str, &[u8])> {
        self.parts.iter().map(|part| (part.name.as_str(), part.bytes.as_slice()))
    }

    /// Whether it is an equation, which is written beside the runs rather
    /// than inside one.
    #[must_use]
    pub(crate) fn is_equation(&self) -> bool {
        self.element.namespace.as_deref() == Some(crate::math::MATH_NAMESPACE)
    }

    /// Whether it is the mark of a link on a run the link holds.
    ///
    /// # Why a mark on every run
    ///
    /// A link is a wrapper round runs, and the model of a paragraph is a list
    /// of runs with nothing round them. Rather than give the model a place
    /// for links, every run a copied link holds carries this mark as a piece
    /// of its own: it reads as nothing — its `content` is the wrapper as an
    /// element carried, which the layout and every reader pass over — and a
    /// run that loses company on the way, a deletion accepted at the paste,
    /// leaves the others still marked. The writer puts the runs that carry
    /// one link's mark, side by side, back inside that link.
    #[must_use]
    pub fn is_link(&self) -> bool {
        self.link.is_some()
    }

    /// Whether two marks are of one link.
    #[must_use]
    pub(crate) fn same_link(&self, other: &Self) -> bool {
        self.link.is_some() && self.link == other.link && self.element == other.element
    }
}

/// One relationship a copied element names, or a carried part names.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Link {
    /// The identifier it is named by where it was.
    id: String,
    /// What the target is, as the relationship's type says.
    kind: String,
    target: Target,
}

/// Where a relationship goes.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Target {
    /// Out of the package: an address, which comes across as it was written.
    External(String),
    /// A part of the package, by the name it had there.
    Part(String),
}

/// A part of the package a copy carries.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Part {
    name: String,
    content_type: String,
    bytes: Vec<u8>,
    /// Its own relationships, by the identifiers it names them with inside.
    links: Vec<Link>,
    /// The relationships of the part the drawing stood in that this part
    /// names from inside itself.
    ///
    /// Word keeps a diagram's drawing on the document's relationships and has
    /// the data model name it there, in `dsp:dataModelExt/@relId`. A paste
    /// makes that relationship again in its own part, under another name, and
    /// the data model has to be told the new one.
    names_host: Vec<String>,
}

/// The piece a reader read, kept with the element it was read from. The
/// reader's side of a copy: what the element points at is gathered after,
/// by [`Document::copy_selection`], which knows the package.
pub(crate) fn carrying(content: RunContent, element: &Element) -> RunContent {
    RunContent::Copied(Box::new(Copied {
        content,
        element: element.clone(),
        links: Vec::new(),
        parts: Vec::new(),
        ignorable: Vec::new(),
        note: None,
        link: None,
    }))
}

/// The mark a copied link leaves on a run it holds: `wrapper` is the link's
/// element with its runs taken out, and `began` where among the runs read
/// with it the link began. See [`Copied::is_link`].
pub(crate) fn linking(wrapper: &Element, began: usize) -> RunContent {
    RunContent::Copied(Box::new(Copied {
        content: RunContent::Carried(Box::new(wrapper.clone())),
        element: wrapper.clone(),
        links: Vec::new(),
        parts: Vec::new(),
        ignorable: Vec::new(),
        note: None,
        link: Some(began),
    }))
}

impl Document {
    /// The selection as blocks, with everything about it kept.
    ///
    /// Empty when nothing is selected.
    #[must_use]
    pub fn copy_selection(&self) -> Vec<Block> {
        let mut out = Vec::new();

        // Every stretch, one after another. Two stretches taken from the same
        // paragraph come out as two paragraphs, which is what Word does too:
        // there is nothing between them in the copy to say they were once side
        // by side, and running them together would join words that were never
        // next to each other.
        for (start, end) in self.selections() {
            for index in start.paragraph..=end.paragraph {
                let Some(element) = self.paragraph_element(index) else { continue };
                let (from, to) = self.range_within(index, start, end);
                let length = self.paragraph_text(index).map_or(0, |text| text.len());
                let mut paragraph = read::read_paragraph(&cut(element, from, to, length));
                let host = self.main_part().to_owned();
                for run in &mut paragraph.runs {
                    for piece in &mut run.content {
                        match piece {
                            RunContent::Copied(copied) => {
                                self.carry_from(copied, &host, &self.tree().root);
                            }
                            RunContent::NoteReference { id, endnote } if *id > 0 => {
                                if let Some(copied) = self.copied_note(*id, *endnote) {
                                    *piece = RunContent::Copied(Box::new(copied));
                                }
                            }
                            _ => {}
                        }
                    }
                }
                out.push(Block::Paragraph(paragraph));
            }
        }
        out
    }

    /// Puts blocks in at the caret, one paragraph after another, keeping the
    /// formatting they were copied with.
    ///
    /// Returns whether anything was put in. The selection is replaced, the same
    /// way typing over a selection replaces it.
    pub fn paste_blocks(&mut self, blocks: &[Block]) -> bool {
        self.paste_blocks_as(blocks, Formatting::Source)
    }

    /// The same, told how much of the copied formatting to bring.
    pub fn paste_blocks_as(&mut self, blocks: &[Block], formatting: Formatting) -> bool {
        let paragraphs: Vec<Paragraph> = blocks
            .iter()
            .filter_map(|block| match block {
                Block::Paragraph(paragraph) => Some(paragraph.clone()),
                Block::Table(_) => None,
            })
            .collect();
        // Nothing that can be put down is nothing to take the selection away
        // for, and nothing to say was pasted.
        let tracking = self.tracking_changes();
        if paragraphs.len() < 2
            && paragraphs.iter().all(|paragraph| pastable(&paragraph.runs, tracking).is_empty())
        {
            return false;
        }

        // One gesture: a paste is one thing, however many paragraphs it is
        // made of, and one undo has to take the whole of it back. The paste
        // options depend on that — choosing another one takes the last paste
        // back and puts it down again the other way.
        self.begin_gesture();
        if self.selection().is_some() {
            self.delete_selection();
        }

        let mut changed = false;
        for (index, paragraph) in paragraphs.iter().enumerate() {
            if index > 0 {
                self.press_enter();
                changed = true;
            }

            // Whether this paragraph brings its own shape — its style, its
            // alignment, its indents — or takes the one it lands in.
            //
            // Every paragraph after the first was made by this paste and has no
            // shape of its own to lose. The first one is different: it is a
            // paragraph that was already there, and Word overwrites its shape
            // only when there is nothing in it to disagree with the pasted one.
            let empty = self
                .paragraph_text(self.caret().paragraph)
                .is_none_or(|text| text.trim().is_empty());
            if formatting == Formatting::Source && (index > 0 || empty) {
                self.reshape_paragraph(&paragraph.properties);
            }

            let runs: Vec<Run> = match formatting {
                Formatting::Source => paragraph.runs.clone(),
                Formatting::Merged => paragraph.runs.iter().map(merged).collect(),
            };
            if self.insert_runs(&runs) {
                changed = true;
            }
        }
        self.end_gesture();
        changed
    }

    /// Gives the paragraph the caret is in the shape a copied one had.
    ///
    /// Everything it had before goes first. A paragraph that is being made to
    /// look like another one has to lose what the other one does not say as
    /// well as gain what it does — otherwise a heading pasted into a
    /// right-aligned paragraph comes out right-aligned, which is neither where
    /// it came from nor what was asked for.
    fn reshape_paragraph(&mut self, wanted: &ParagraphProperties) {
        let caret = self.caret();
        let prefix = self.prefix();
        let Some(path) = crate::position::paragraph_path(&self.tree().root, caret.paragraph) else {
            return;
        };
        let Some(paragraph) =
            crate::edit::element_at_path_mut(&mut self.tree_to_edit().root, &path)
        else {
            return;
        };

        paragraph.remove_children_named(Some(read::W), "pPr");
        crate::format::set_paragraph_style(paragraph, wanted.style.as_deref(), prefix.as_deref());
        crate::format::set_paragraph_numbering(paragraph, wanted.numbering, prefix.as_deref());
        crate::format::apply_paragraph_properties(paragraph, wanted, prefix.as_deref());
        self.note_change();
    }

    /// Puts formatted runs in at the caret, and the caret after them.
    ///
    /// The run under the caret is cut in two so there is a place between them,
    /// which is how everything that is not text goes into a paragraph — see
    /// [`Document::insert_equation`]. Returns whether anything went in: a
    /// drawing that names a relationship of some part nothing here knows —
    /// one read from another document rather than copied out of it — is not
    /// something that can be put down, and neither is an empty run.
    pub fn insert_runs(&mut self, runs: &[Run]) -> bool {
        let tracking = self.tracking_changes();
        let mut runs = pastable(runs, tracking);
        if runs.is_empty() {
            return false;
        }

        let caret = self.caret();
        self.record(crate::history::EditKind::Structural, caret, false);
        let prefix = self.prefix();
        let before = self.paragraph_text(caret.paragraph).map_or(0, |text| text.len());

        // Every copied drawing's parts brought into this package first, and
        // its element pointed at them; one that cannot be is left out rather
        // than put down pointing at nothing.
        let mut brought = Brought::default();
        let host = self.main_part().to_owned();
        for run in &mut runs {
            run.content.retain_mut(|piece| match piece {
                RunContent::Copied(copied) => {
                    self.settle_into(copied, &mut brought, &host, true)
                        && self.settle_note(copied, &mut brought)
                }
                _ => true,
            });
        }
        runs.retain(|run| !run.content.is_empty());
        if runs.is_empty() {
            return false;
        }

        // The tracked changes: each wrapper a number of its own here, and
        // the whole paste one insertion when changes are being tracked.
        let mut next = self.next_revision_id();
        renumber(&mut runs, &mut next);
        if tracking {
            let paste = Revision {
                kind: RevisionKind::Inserted,
                author: self.reviser.author.clone(),
                date: self.reviser.date.clone(),
                id: next,
            };
            next += 1;
            for run in &mut runs {
                run.revision = Some(paste.clone());
            }
        }
        let elements = edit::runs_elements(&runs, prefix.as_deref());

        let Some(path) = crate::position::paragraph_path(&self.tree().root, caret.paragraph) else {
            return false;
        };
        let Some(paragraph) = edit::element_at_path_mut(&mut self.tree_to_edit().root, &path)
        else {
            return false;
        };
        crate::format::split_runs_at_offset(paragraph, caret.offset);
        let links = elements.iter().any(|element| element.is(Some(W), "hyperlink"));
        let (inside, position) = insertion_point(paragraph, caret.offset, next, links);
        let Some(parent) = edit::element_at_path_mut(paragraph, &inside) else { return false };
        for (offset, element) in elements.into_iter().enumerate() {
            parent.insert_element(position + offset, element);
        }

        // After everything pasted, counted the way the caret counts: a
        // drawing is a character to the caret and nothing to the plain text,
        // and a deletion is the other way round.
        let after = self.paragraph_text(caret.paragraph).map_or(before, |text| text.len());
        self.set_caret(TextPosition::new(
            caret.paragraph,
            caret.offset + after.saturating_sub(before),
        ));
        self.note_change();
        true
    }

    /// Gathers what a copied element points at from this document's package:
    /// the relationships of `host`, the part it was in, whose root is `root`.
    fn carry_from(&self, copied: &mut Copied, host: &str, root: &Element) {
        let used = declare_what_it_uses(&mut copied.element, root);
        let ignorable = ignorable_namespaces(root);
        copied.ignorable = used.into_iter().filter(|uri| ignorable.contains(uri)).collect();

        let relationships = self.package().relationships(host).unwrap_or_default();
        let mut links = Vec::new();
        let mut parts = Vec::new();
        for id in relationship_ids(&copied.element) {
            if let Some(relationship) = relationships.by_id(&id) {
                if let Some(link) = self.link(relationship, host, &mut parts) {
                    links.push(link);
                }
            }
        }

        // A diagram's data model may name the drawing by a relationship of
        // the part the frame is in rather than one of its own, and then that
        // relationship goes with the copy as well. Walked by index, because
        // following it can take more parts in, and they are looked at too.
        let mut index = 0;
        while index < parts.len() {
            for id in host_names(&parts[index]) {
                let Some(relationship) = relationships.by_id(&id) else { continue };
                parts[index].names_host.push(id.clone());
                if !links.iter().any(|link| link.id == id) {
                    if let Some(link) = self.link(relationship, host, &mut parts) {
                        links.push(link);
                    }
                }
            }
            index += 1;
        }
        copied.links = links;
        copied.parts = parts;
    }

    /// One relationship, and the part it reaches with every part that one
    /// reaches, gathered into `parts` once each.
    fn link(
        &self,
        relationship: &wp_opc::Relationship,
        source: &str,
        parts: &mut Vec<Part>,
    ) -> Option<Link> {
        let target = match relationship.resolved_target(source) {
            None => Target::External(relationship.target.clone()),
            Some(Ok(name)) => {
                // Never the text itself, which a part may name back: what is
                // copied is a drawing, not the document it stood in.
                let text = [self.main_part(), self.document_part.as_str()];
                if text.iter().any(|part| part.eq_ignore_ascii_case(&name))
                    || !self.take_part(&name, parts)
                {
                    return None;
                }
                Target::Part(name)
            }
            // A target that leads nowhere in the package is one nothing can
            // follow, here or anywhere it is pasted.
            Some(Err(_)) => return None,
        };
        Some(Link { id: relationship.id.clone(), kind: relationship.kind.clone(), target })
    }

    /// Takes a part into a copy, with every part its own relationships reach.
    /// Says whether the part is there to take.
    fn take_part(&self, name: &str, parts: &mut Vec<Part>) -> bool {
        if parts.iter().any(|part| part.name.eq_ignore_ascii_case(name)) {
            return true;
        }
        let Some(bytes) = self.package().part(name) else { return false };
        let content_type =
            self.package().content_type(name).unwrap_or("application/octet-stream").to_owned();
        // In before its own relationships are followed, so that a part which
        // points back at this one on the way finds it taken already.
        parts.push(Part {
            name: name.to_owned(),
            content_type,
            bytes: bytes.to_vec(),
            links: Vec::new(),
            names_host: Vec::new(),
        });
        let index = parts.len() - 1;
        let own = self.package().relationships(name).unwrap_or_default();
        let mut links = Vec::new();
        for relationship in own.all() {
            if let Some(link) = self.link(relationship, name, parts) {
                links.push(link);
            }
        }
        parts[index].links = links;
        true
    }

    /// Makes a copied element the own of `host`, a part of this document:
    /// its parts put into this package, its relationships made in that part,
    /// and — when `here`, the part being edited — its namespaces declared on
    /// the part's root. Anywhere else the element goes on declaring them
    /// itself, which is XML wherever it is put.
    ///
    /// Says whether it can now be written there; a relationship this package
    /// refuses to take is a drawing that would point at nothing.
    fn settle_into(
        &mut self,
        copied: &mut Copied,
        brought: &mut Brought,
        host: &str,
        here: bool,
    ) -> bool {
        let host = host.to_owned();

        // Every part a name here first, so that a relationship between two of
        // them can be written with the other's new name.
        for part in &copied.parts {
            if brought.find(part).is_none() {
                let (name, reused) = self.place_for(part, brought);
                brought.places.push(Place {
                    was: part.name.clone(),
                    bytes: part.bytes.clone(),
                    now: name,
                    written: reused,
                });
            }
        }

        // The relationships of the part the drawing lands in. Ones that
        // cannot be read cannot be added to either: writing a fresh set over
        // them would lose every relationship the part had.
        let Ok(mut relationships) = self.package().relationships(&host) else { return false };
        let mut renamed: Vec<(String, String)> = Vec::new();
        for link in &copied.links {
            let (target, mode) = match &link.target {
                Target::External(address) => (address.clone(), TargetMode::External),
                Target::Part(name) => {
                    let Some(now) = brought.now(name, &copied.parts) else { return false };
                    (relative_target(&host, &now), TargetMode::Internal)
                }
            };
            // One already there to the same thing is used again rather than a
            // second made beside it: a picture pasted twice into the part it
            // came from goes on naming the relationship it named.
            let same = relationships.all().iter().find(|held| {
                held.kind == link.kind
                    && held.mode == mode
                    && match mode {
                        TargetMode::External => held.target == target,
                        TargetMode::Internal => {
                            held.resolved_target(&host).and_then(Result::ok)
                                == wp_opc::resolve_target(&host, &target).ok()
                        }
                    }
            });
            let id = match same {
                Some(held) => held.id.clone(),
                None => relationships.add(&link.kind, &target, mode).id.clone(),
            };
            renamed.push((link.id.clone(), id));
        }
        if !copied.links.is_empty() && self.package_mut().set_relationships(&relationships).is_err()
        {
            return false;
        }

        // Then the parts themselves, with their own relationships.
        for part in &copied.parts {
            let Some(place) = brought.find_mut(part) else { continue };
            if place.written {
                continue;
            }
            place.written = true;
            let now = place.now.clone();
            let bytes = if part.names_host.is_empty() {
                part.bytes.clone()
            } else {
                renamed_in_part(&part.bytes, &renamed)
            };
            self.package_mut().add_part(&now, &part.content_type, bytes);
            if !self.write_part_relationships(&now, part, &copied.parts, brought) {
                return false;
            }
        }

        repoint(&mut copied.element, &renamed);
        if here {
            self.declare_namespaces(copied);
        }
        copied.links.clear();
        copied.parts.clear();
        true
    }

    /// Makes the note a copied mark carried a note of this document's own,
    /// and points the mark at it.
    ///
    /// Says whether the mark can be written: a mark whose note could not be
    /// made would point at a note that is not there.
    fn settle_note(&mut self, copied: &mut Copied, brought: &mut Brought) -> bool {
        let Some(mut blocks) = copied.note.take() else { return true };
        let RunContent::NoteReference { endnote, .. } = copied.content else { return false };
        let kind = if endnote { crate::notes::Kind::Endnote } else { crate::notes::Kind::Footnote };

        // A number no entry of the part has, the separators' included: Word
        // numbers those −1 and 0, and LibreOffice 0 and 1.
        let id = self.notes_root(kind).map_or(0, |root| {
            root.children_named(Some(W), kind.entry())
                .filter_map(|entry| entry.attribute(Some(W), "id")?.parse::<i32>().ok())
                .max()
                .unwrap_or(0)
        });
        let id = id.max(0) + 1;
        // The entry first, which makes the part if there is none, so that the
        // drawings in the note have a part to be settled into.
        if !matches!(self.put_note(kind, id, &crate::model::Body::default()), Ok(true)) {
            return false;
        }
        let Some(part) = self.notes_part(kind) else { return false };
        // A note is at least one paragraph, which is where its mark goes.
        if blocks.is_empty() {
            blocks.push(Block::Paragraph(Paragraph::default()));
        }
        for block in &mut blocks {
            let Block::Paragraph(paragraph) = block else { continue };
            for run in &mut paragraph.runs {
                run.content.retain_mut(|piece| match piece {
                    RunContent::Copied(inner) => self.settle_into(inner, brought, &part, false),
                    _ => true,
                });
            }
        }
        if !self.set_note_body(kind, id, &crate::model::Body { blocks }) {
            return false;
        }

        let name = edit::name_with(copied.element.prefix(), "id");
        copied.element.set_namespaced_attribute(&name, W, &id.to_string());
        copied.content = RunContent::NoteReference { id, endnote };
        true
    }

    /// A note's mark, copied with what the note says.
    fn copied_note(&self, id: i32, endnote: bool) -> Option<Copied> {
        let kind = if endnote { crate::notes::Kind::Endnote } else { crate::notes::Kind::Footnote };
        let part = self.notes_part(kind)?;
        let root = self.notes_root(kind)?;
        let wanted = id.to_string();
        let entry = root
            .children_named(Some(W), kind.entry())
            .find(|entry| entry.attribute(Some(W), "id") == Some(wanted.as_str()))?;

        let mut blocks = Vec::new();
        for child in entry.child_elements() {
            if child.is(Some(W), "p") {
                let mut paragraph = read::read_paragraph(child);
                for run in &mut paragraph.runs {
                    for piece in &mut run.content {
                        if let RunContent::Copied(inner) = piece {
                            self.carry_from(inner, &part, &root);
                        }
                    }
                }
                blocks.push(Block::Paragraph(paragraph));
            } else if child.is(Some(W), "tbl") {
                // A table in a note comes as its model, which carries no
                // parts: what in it points at a part is left out rather
                // than pasted pointing at nothing.
                let mut alone = Element::new("w:body", Some(W));
                alone.push_element(child.clone());
                for mut block in read::read_part(&alone).blocks {
                    keep_pastable(&mut block);
                    blocks.push(block);
                }
            }
        }
        // The note's own mark comes off: a note is written with a mark of its
        // own, which names the note it is in. See [`Document::set_note_body`].
        if let Some(Block::Paragraph(first)) = blocks.first_mut() {
            for run in &mut first.runs {
                run.content
                    .retain(|piece| !matches!(piece, RunContent::NoteReference { id: 0, .. }));
            }
            first.runs.retain(|run| !run.content.is_empty());
        }

        let local = if endnote { "endnoteReference" } else { "footnoteReference" };
        let mut element = Element::new(&format!("w:{local}"), Some(W));
        element.declarations.push((Some("w".to_owned()), W.to_owned()));
        element.set_namespaced_attribute("w:id", W, &wanted);
        Some(Copied {
            content: RunContent::NoteReference { id, endnote },
            element,
            links: Vec::new(),
            parts: Vec::new(),
            ignorable: Vec::new(),
            note: Some(blocks),
            link: None,
        })
    }

    /// Where a carried part goes in this package, and whether it is there
    /// already.
    fn place_for(&self, part: &Part, brought: &Brought) -> (String, bool) {
        // A picture this package holds byte for byte is the same picture.
        if part.content_type.starts_with("image/") {
            let package = self.package();
            if let Some(same) = package.entries().iter().find(|entry| {
                !entry.is_directory()
                    && entry.data == part.bytes
                    && package.content_type(&entry.name) == Some(part.content_type.as_str())
            }) {
                return (same.name.clone(), true);
            }
        }
        let name = fresh_name(&part.name, |candidate| {
            self.package().part(candidate).is_none()
                && !brought.places.iter().any(|place| place.now.eq_ignore_ascii_case(candidate))
        });
        (name, false)
    }

    /// Writes a carried part's own relationships, under the identifiers its
    /// content names them by, pointing at where their targets now are.
    fn write_part_relationships(
        &mut self,
        name: &str,
        part: &Part,
        carried: &[Part],
        brought: &Brought,
    ) -> bool {
        let rels_part = wp_opc::relationships_part_for(name);
        if part.links.is_empty() {
            // Nothing of a part that was once called this may speak for it.
            if self.package().part(&rels_part).is_some() {
                self.package_mut().remove_part(&rels_part);
            }
            return true;
        }

        let mut root = Element::new("Relationships", Some(RELATIONSHIPS_NAMESPACE));
        root.declarations.push((None, RELATIONSHIPS_NAMESPACE.to_owned()));
        for link in &part.links {
            let (target, external) = match &link.target {
                Target::External(address) => (address.clone(), true),
                Target::Part(target) => match brought.now(target, carried) {
                    Some(now) => (relative_target(name, &now), false),
                    None => continue,
                },
            };
            let mut relationship = Element::new("Relationship", Some(RELATIONSHIPS_NAMESPACE));
            relationship.set_attribute("Id", &link.id);
            relationship.set_attribute("Type", &link.kind);
            relationship.set_attribute("Target", &target);
            if external {
                relationship.set_attribute("TargetMode", "External");
            }
            root.push_element(relationship);
        }
        let tree = XmlTree {
            standalone: Some(true),
            has_declaration: true,
            doctype: None,
            before_root: Vec::new(),
            root,
            after_root: Vec::new(),
        };
        let Ok(xml) = tree.to_xml() else { return false };
        let Ok(relationships) = Relationships::parse(name, &xml) else { return false };
        self.package_mut().set_relationships(&relationships).is_ok()
    }

    /// Declares, on the part's own root, the namespaces a settled element
    /// declares for itself, wherever the root can say them without changing
    /// what a prefix already means there.
    ///
    /// An element that declares its own namespaces is XML wherever it is put,
    /// and that is how a copy travels. At home it is tidier for the root to
    /// say them, as it does for everything else in the part; and an extension
    /// its own document let a reader pass over has to be one this document
    /// lets it pass over too, or a reader that does not know it stops there.
    fn declare_namespaces(&mut self, copied: &mut Copied) {
        let root = &mut self.tree_to_edit().root;
        let declared = core::mem::take(&mut copied.element.declarations);
        let mut kept = Vec::new();
        for (prefix, uri) in declared {
            let bound = root.declarations.iter().find(|(held, _)| *held == prefix).cloned();
            match (bound, prefix.as_deref()) {
                (Some((_, held)), _) if held == uri => {
                    if let (true, Some(name)) = (copied.ignorable.contains(&uri), &prefix) {
                        edit::declare_extension(root, name, &uri);
                    }
                }
                // The prefix means something else here, or the namespace is
                // a default one: said where it is used, and only there.
                (Some(_), _) | (None, None) => kept.push((prefix.clone(), uri)),
                (None, Some(name)) => {
                    if copied.ignorable.contains(&uri) {
                        edit::declare_extension(root, name, &uri);
                    } else {
                        root.declarations.push((prefix.clone(), uri));
                    }
                }
            }
        }
        copied.element.declarations = kept;
    }
}

/// What one paste has put into the package so far.
#[derive(Default)]
struct Brought {
    places: Vec<Place>,
}

/// Where one carried part went.
struct Place {
    /// Its name where it was copied from.
    was: String,
    /// Its bytes, because two documents can each have a part of one name.
    bytes: Vec<u8>,
    /// Its name here.
    now: String,
    /// Whether it is in the package, which a picture found here already is.
    written: bool,
}

impl Brought {
    fn find(&self, part: &Part) -> Option<&Place> {
        self.places
            .iter()
            .find(|place| place.was.eq_ignore_ascii_case(&part.name) && place.bytes == part.bytes)
    }

    fn find_mut(&mut self, part: &Part) -> Option<&mut Place> {
        self.places
            .iter_mut()
            .find(|place| place.was.eq_ignore_ascii_case(&part.name) && place.bytes == part.bytes)
    }

    /// Where the part a copy called `name` is now.
    fn now(&self, name: &str, carried: &[Part]) -> Option<String> {
        let part = carried.iter().find(|part| part.name.eq_ignore_ascii_case(name))?;
        self.find(part).map(|place| place.now.clone())
    }
}

/// The runs of a paste that can be put down.
///
/// When changes are being tracked where they land, the copy's own changes
/// are accepted first — what somebody deleted goes, what somebody inserted
/// stays as plain text — because the paste is then a change of its own.
fn pastable(runs: &[Run], tracking: bool) -> Vec<Run> {
    runs.iter()
        .filter_map(|run| {
            let mut run = run.clone();
            if tracking {
                match run.revision.as_ref().map(|change| change.kind) {
                    Some(RevisionKind::Deleted) => return None,
                    Some(RevisionKind::Inserted) => run.revision = None,
                    None => {}
                }
            }
            run.content.retain(can_be_put_down);
            (!run.content.is_empty()).then_some(run)
        })
        .collect()
}

/// Whether a piece of a run can be written into whatever part it is pasted
/// into.
///
/// A copy can, because it carries what it points at. A drawing read from a
/// document and not copied names a relationship of the part it was read
/// from, and nothing says which part that was: written into another, it
/// points at whatever that part's relationship of the same name is, if it
/// has one — and a drawing Word finds pointing at the styles is a document
/// Word calls damaged.
fn can_be_put_down(piece: &RunContent) -> bool {
    match piece {
        RunContent::Copied(_)
        | RunContent::Text(_)
        | RunContent::Break(_)
        | RunContent::Tab
        | RunContent::PositionTab(_)
        | RunContent::Ruby(_)
        | RunContent::Math(_)
        | RunContent::NoteReference { .. } => true,
        RunContent::Picture(_)
        | RunContent::Chart(_)
        | RunContent::Ink(_)
        | RunContent::Diagram(_) => false,
        RunContent::Group(group) => !group_holds_a_picture(group),
        RunContent::Carried(element) => relationship_ids(element).is_empty(),
        RunContent::Shape(shape) => shape.text.iter().all(|paragraph| {
            paragraph.runs.iter().all(|run| run.content.iter().all(can_be_put_down))
        }),
    }
}

/// Takes out of a block whatever cannot be put down anywhere but where it was
/// read. See [`can_be_put_down`].
fn keep_pastable(block: &mut Block) {
    match block {
        Block::Paragraph(paragraph) => {
            for run in &mut paragraph.runs {
                run.content.retain(can_be_put_down);
            }
        }
        Block::Table(table) => {
            for row in &mut table.rows {
                for cell in &mut row.cells {
                    for block in &mut cell.blocks {
                        keep_pastable(block);
                    }
                }
            }
        }
    }
}

fn group_holds_a_picture(group: &crate::group::Group) -> bool {
    group.members.iter().any(|member| match &member.what {
        crate::group::Inside::Picture(_) => true,
        crate::group::Inside::Group(inner) => group_holds_a_picture(inner),
        crate::group::Inside::Shape(_) => false,
    })
}

/// Gives every tracked change in a paste a number of its own in the document
/// it lands in, keeping runs that were one change one change.
fn renumber(runs: &mut [Run], next: &mut i32) {
    let mut previous: Option<(Revision, i32)> = None;
    for run in runs {
        let Some(change) = &mut run.revision else {
            previous = None;
            continue;
        };
        let id = match &previous {
            Some((was, id)) if *was == *change => *id,
            _ => {
                let id = *next;
                *next += 1;
                previous = Some((change.clone(), id));
                id
            }
        };
        change.id = id;
    }
}

/// Where among a paragraph's elements what is pasted at an offset goes: the
/// path to the element to put it in, from the paragraph, and the place there.
///
/// Into a link, a content control or a smart tag when the offset is inside
/// one, because what is pasted into the middle of a link is part of the link,
/// as it is in Word — unless what is pasted holds a link of its own, `links`,
/// which a link cannot hold: then the link it lands in is cut in two round it.
/// Beside somebody's tracked insertion rather than inside it — the insertion
/// is cut in two there, the second half numbered `split` — because what is
/// pasted has a history of its own. After the paragraph's properties, always.
fn insertion_point(
    element: &mut Element,
    offset: usize,
    split: i32,
    links: bool,
) -> (Vec<usize>, usize) {
    let mut seen = 0usize;
    for index in 0..element.children.len() {
        let Some(child) = element.children[index].as_element() else { continue };
        if child.is(Some(W), "pPr") || child.is(Some(W), "del") {
            continue;
        }
        let length = edit::measured_length(child);
        if child.namespace.as_deref() != Some(W) && length == 0 {
            continue;
        }
        if seen >= offset {
            return (Vec::new(), index);
        }
        if offset < seen + length {
            let local = offset - seen;
            let local_name = child.local_name().to_owned();
            let Some(child) = element.children[index].as_element_mut() else { break };
            return match local_name.as_str() {
                "ins" | "moveTo" => {
                    let tail = split_wrapper(child, local, Some(split));
                    element.insert_element(index + 1, tail);
                    (Vec::new(), index + 1)
                }
                "hyperlink" if links => {
                    let tail = split_wrapper(child, local, None);
                    element.insert_element(index + 1, tail);
                    (Vec::new(), index + 1)
                }
                "hyperlink" | "smartTag" | "customXml" | "dir" | "bdo" => {
                    let (mut path, at) = insertion_point(child, local, split, links);
                    path.insert(0, index);
                    (path, at)
                }
                "sdt" => {
                    let Some(content) = child.position_of(Some(W), "sdtContent") else {
                        return (Vec::new(), index + 1);
                    };
                    let Some(inner) = child.children[content].as_element_mut() else {
                        return (Vec::new(), index + 1);
                    };
                    let (mut path, at) = insertion_point(inner, local, split, links);
                    path.insert(0, content);
                    path.insert(0, index);
                    (path, at)
                }
                // A field's answer or a word under a reading is not somewhere
                // to put things: what is pasted goes after it.
                _ => (Vec::new(), index + 1),
            };
        }
        seen += length;
    }
    (Vec::new(), element.children.len())
}

/// Cuts a wrapper in two at an offset inside it, keeping the first half where
/// it is and giving back the second — numbered `id`, when the wrapper is a
/// tracked change, which has to have a number of its own.
fn split_wrapper(wrapper: &mut Element, offset: usize, id: Option<i32>) -> Element {
    let mut seen = 0usize;
    let mut at = wrapper.children.len();
    for (index, node) in wrapper.children.iter().enumerate() {
        let Some(child) = node.as_element() else { continue };
        if seen >= offset {
            at = index;
            break;
        }
        seen += edit::measured_length(child);
    }
    let mut tail = wrapper.clone();
    tail.children = wrapper.children.split_off(at);
    if let Some(id) = id {
        let name = edit::name_with(wrapper.prefix(), "id");
        tail.set_namespaced_attribute(&name, W, &id.to_string());
    }
    tail
}

/// A run with everything but its emphasis taken off.
///
/// See [`Formatting::Merged`] for what is kept and why.
#[must_use]
fn merged(run: &Run) -> Run {
    Run {
        properties: RunProperties {
            bold: run.properties.bold,
            italic: run.properties.italic,
            underline: run.properties.underline.clone(),
            strike: run.properties.strike,
            double_strike: run.properties.double_strike,
            vertical_align: run.properties.vertical_align,
            ..RunProperties::default()
        },
        content: run.content.clone(),
        field: run.field.clone(),
        revision: run.revision.clone(),
        format_change: None,
    }
}

// --- Cutting a paragraph where the caret would -------------------------------

/// The stretch of a paragraph a copy takes, in the caret's own offsets.
struct Stretch {
    from: usize,
    to: usize,
    /// How long the paragraph is, which says whether an end of the stretch is
    /// an end of the paragraph.
    length: usize,
}

impl Stretch {
    /// Whether something that takes no room in the text, standing at an
    /// offset, is inside the stretch. See the note at the top of this file.
    fn holds_nothing_at(&self, at: usize) -> bool {
        (self.from < at && at < self.to)
            || (at == self.from && self.from == 0)
            || (at == self.to && self.to == self.length)
    }
}

/// The part of a paragraph between two offsets, as an element of its own:
/// the runs cut where the caret would cut them, and whatever lies outside the
/// stretch taken out.
fn cut(paragraph: &Element, from: usize, to: usize, length: usize) -> Element {
    let mut cut = paragraph.clone();
    // The far end first, so that cutting there leaves the near end where it
    // was.
    crate::format::split_runs_at_offset(&mut cut, to);
    crate::format::split_runs_at_offset(&mut cut, from);
    let stretch = Stretch { from, to, length };
    let mut offset = 0usize;
    trim(&mut cut, &stretch, &mut offset);
    cut
}

/// Takes out of an element everything outside the stretch, walking its
/// children the way the caret's text is walked.
fn trim(parent: &mut Element, stretch: &Stretch, offset: &mut usize) {
    let mut index = 0;
    while index < parent.children.len() {
        let keep = match &mut parent.children[index] {
            Node::Element(child) => keeps(child, stretch, offset),
            _ => true,
        };
        if keep {
            index += 1;
        } else {
            parent.children.remove(index);
        }
    }
}

/// Whether one child stays in the copy; a child that holds runs of its own is
/// trimmed inside instead.
fn keeps(child: &mut Element, stretch: &Stretch, offset: &mut usize) -> bool {
    if child.namespace.as_deref() == Some(W) {
        match child.local_name() {
            // What an element says about itself stays with it.
            "pPr" | "rPr" | "sdtPr" | "sdtEndPr" => return true,
            // What holds runs is walked into, the way the caret walks into
            // it: a link, a content control, a change somebody made.
            "ins" | "moveTo" | "hyperlink" | "smartTag" | "customXml" | "fldSimple" | "dir"
            | "bdo" | "sdtContent" => {
                trim(child, stretch, offset);
                return true;
            }
            "sdt" => {
                if let Some(content) = child.child_mut(Some(W), "sdtContent") {
                    trim(content, stretch, offset);
                }
                return true;
            }
            _ => {}
        }
    }

    let width = edit::measured_length(child);
    let at = *offset;
    *offset += width;
    if width > 0 {
        return at < stretch.to && at + width > stretch.from;
    }
    // Taking no room: a mark of a field or of a bookmark is kept whatever it
    // is, because a field's answer read without its marks is not a field;
    // anything that reads as something is kept where it lies inside.
    !reads_as_something(child) || stretch.holds_nothing_at(at)
}

/// Whether an element that takes no room in the text reads as something:
/// a deletion, or a run holding more than a field's marks.
fn reads_as_something(element: &Element) -> bool {
    if element.namespace.as_deref() != Some(W) {
        return false;
    }
    match element.local_name() {
        "del" | "moveFrom" | "ruby" => true,
        "r" => {
            crate::fields::marker_of(element).is_none()
                && crate::fields::instruction_of(element).is_none()
                && !read::read_run(element).content.is_empty()
        }
        _ => false,
    }
}

// --- Relationships and namespaces ---------------------------------------------

/// The namespace of the older drawings' own attributes, one of which —
/// `o:relid` — names a relationship as well.
const OFFICE: &str = "urn:schemas-microsoft-com:office:office";

/// Whether an attribute names a relationship.
fn names_relationship(attribute: &wp_xml::tree::Attribute) -> bool {
    match attribute.namespace.as_deref() {
        Some(edit::RELATIONSHIPS) => true,
        Some(OFFICE) => attribute.name.ends_with(":relid") || attribute.name == "relid",
        _ => false,
    }
}

/// Every relationship an element names anywhere inside it, once each, in the
/// order they come.
fn relationship_ids(element: &Element) -> Vec<String> {
    fn walk(element: &Element, out: &mut Vec<String>) {
        for attribute in &element.attributes {
            if names_relationship(attribute) && !out.contains(&attribute.value) {
                out.push(attribute.value.clone());
            }
        }
        for child in element.child_elements() {
            walk(child, out);
        }
    }
    let mut out = Vec::new();
    walk(element, &mut out);
    out
}

/// Points every relationship an element names at its new identifier.
fn repoint(element: &mut Element, renamed: &[(String, String)]) {
    for attribute in &mut element.attributes {
        if names_relationship(attribute) {
            if let Some((_, now)) = renamed.iter().find(|(was, _)| *was == attribute.value) {
                attribute.value = now.clone();
            }
        }
    }
    for child in element.child_elements_mut() {
        repoint(child, renamed);
    }
}

/// The relationships of the host part a carried part names from inside
/// itself: `dsp:dataModelExt/@relId`, in a diagram's data model.
fn host_names(part: &Part) -> Vec<String> {
    if part.content_type != crate::diagram::DATA_CONTENT_TYPE {
        return Vec::new();
    }
    let Ok(text) = core::str::from_utf8(&part.bytes) else { return Vec::new() };
    let Ok(tree) = XmlTree::parse(text) else { return Vec::new() };
    let mut out = Vec::new();
    find_named_relationships(&tree.root, &mut out);
    out.retain(|id| !part.links.iter().any(|link| link.id == *id));
    out
}

fn find_named_relationships(element: &Element, out: &mut Vec<String>) {
    if element.local_name() == "dataModelExt" {
        if let Some(id) = element.attribute_by_name("relId").filter(|id| !id.is_empty()) {
            out.push(id.to_owned());
        }
    }
    for child in element.child_elements() {
        find_named_relationships(child, out);
    }
}

/// A carried part's bytes with the host relationships it names renamed.
fn renamed_in_part(bytes: &[u8], renamed: &[(String, String)]) -> Vec<u8> {
    let Ok(text) = core::str::from_utf8(bytes) else { return bytes.to_vec() };
    let Ok(mut tree) = XmlTree::parse(text) else { return bytes.to_vec() };
    fn walk(element: &mut Element, renamed: &[(String, String)]) {
        if element.local_name() == "dataModelExt" {
            for attribute in &mut element.attributes {
                if attribute.name == "relId" {
                    if let Some((_, now)) = renamed.iter().find(|(was, _)| *was == attribute.value)
                    {
                        attribute.value = now.clone();
                    }
                }
            }
        }
        for child in element.child_elements_mut() {
            walk(child, renamed);
        }
    }
    walk(&mut tree.root, renamed);
    tree.to_xml().map_or_else(|_| bytes.to_vec(), String::into_bytes)
}

/// Declares on an element every namespace used inside it that nothing
/// inside it declares, reading what each prefix means from the root of the
/// document it is in. Gives back every namespace it uses.
///
/// Word declares everything once, on the root, and an element taken out of
/// the tree leaves those declarations behind: `wp:inline` with nothing to say
/// what `wp` is, which is not XML.
fn declare_what_it_uses(element: &mut Element, root: &Element) -> Vec<String> {
    let mut used: Vec<(Option<String>, String)> = Vec::new();
    collect_used(element, root, &mut used);
    let mut declared: Vec<(Option<String>, String)> = Vec::new();
    collect_declared(element, &mut declared);
    for (prefix, uri) in &used {
        if !declared.iter().any(|(held, bound)| held == prefix && bound == uri) {
            element.declarations.push((prefix.clone(), uri.clone()));
            declared.push((prefix.clone(), uri.clone()));
        }
    }
    used.into_iter().map(|(_, uri)| uri).collect()
}

fn collect_used(element: &Element, root: &Element, out: &mut Vec<(Option<String>, String)>) {
    let mut add = |prefix: Option<&str>, uri: &str| {
        let pair = (prefix.map(str::to_owned), uri.to_owned());
        if !out.contains(&pair) {
            out.push(pair);
        }
    };
    if let Some(uri) = element.namespace.as_deref() {
        add(element.prefix(), uri);
    }
    for attribute in &element.attributes {
        let Some(uri) = attribute.namespace.as_deref() else { continue };
        let Some((prefix, _)) = attribute.name.split_once(':') else { continue };
        if prefix != "xml" {
            add(Some(prefix), uri);
        }
    }
    // A choice names what it needs by prefix, in the value of an attribute,
    // where a reader looks the prefix up.
    if element.local_name() == "Choice" {
        if let Some(requires) = element.attribute_by_name("Requires") {
            for prefix in requires.split_whitespace() {
                if let Some(uri) = bound_to(root, element, prefix) {
                    add(Some(prefix), &uri);
                }
            }
        }
    }
    for child in element.child_elements() {
        collect_used(child, root, out);
    }
}

/// What a prefix means, said on the element or on the root.
fn bound_to(root: &Element, element: &Element, prefix: &str) -> Option<String> {
    element
        .declarations
        .iter()
        .chain(root.declarations.iter())
        .find(|(held, _)| held.as_deref() == Some(prefix))
        .map(|(_, uri)| uri.clone())
}

fn collect_declared(element: &Element, out: &mut Vec<(Option<String>, String)>) {
    out.extend(element.declarations.iter().cloned());
    for child in element.child_elements() {
        collect_declared(child, out);
    }
}

/// The namespaces a root lets a reader pass over, by what they are rather
/// than by the prefixes that name them.
fn ignorable_namespaces(root: &Element) -> Vec<String> {
    let Some(listed) = root.attribute(Some(read::MC), "Ignorable") else { return Vec::new() };
    listed
        .split_whitespace()
        .filter_map(|prefix| {
            root.declarations
                .iter()
                .find(|(held, _)| held.as_deref() == Some(prefix))
                .map(|(_, uri)| uri.clone())
        })
        .collect()
}

/// A target for a relationship of one part that reaches another, written
/// relative to the first part's folder as Word writes them.
fn relative_target(from: &str, to: &str) -> String {
    let mut folder: Vec<&str> = from.split('/').collect();
    folder.pop();
    let target: Vec<&str> = to.split('/').collect();
    let (file, target_folder) = target.split_last().map_or(("", &[][..]), |(f, rest)| (*f, rest));
    let shared = folder
        .iter()
        .zip(target_folder.iter())
        .take_while(|(a, b)| a.eq_ignore_ascii_case(b))
        .count();
    let mut out = String::new();
    for _ in shared..folder.len() {
        out.push_str("../");
    }
    for segment in &target_folder[shared..] {
        out.push_str(segment);
        out.push('/');
    }
    out.push_str(file);
    out
}

/// A name for a part that nothing has yet: the one it had, if that is free,
/// and otherwise the same name with the first number after it that is.
fn fresh_name(original: &str, free: impl Fn(&str) -> bool) -> String {
    if free(original) {
        return original.to_owned();
    }
    let (folder, file) = original.rsplit_once('/').map_or(("", original), |(f, n)| (f, n));
    let (stem, extension) = file.rsplit_once('.').map_or((file, None), |(s, e)| (s, Some(e)));
    let base = stem.trim_end_matches(|c: char| c.is_ascii_digit());
    let mut number = 1usize;
    loop {
        let mut candidate = String::new();
        if !folder.is_empty() {
            candidate.push_str(folder);
            candidate.push('/');
        }
        candidate.push_str(base);
        candidate.push_str(&number.to_string());
        if let Some(extension) = extension {
            candidate.push('.');
            candidate.push_str(extension);
        }
        if free(&candidate) {
            return candidate;
        }
        number += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORDS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

    /// "plain bold after" with "bold" in bold, as it is written.
    fn paragraph() -> Element {
        XmlTree::parse(&format!(
            "<w:p xmlns:w=\"{WORDS}\"><w:pPr><w:pStyle w:val=\"Heading1\"/></w:pPr>\
             <w:r><w:t xml:space=\"preserve\">plain </w:t></w:r>\
             <w:r><w:rPr><w:b/></w:rPr><w:t>bold</w:t></w:r>\
             <w:r><w:t xml:space=\"preserve\"> after</w:t></w:r></w:p>"
        ))
        .expect("the paragraph parses")
        .root
    }

    fn copied(paragraph: &Element, from: usize, to: usize) -> Paragraph {
        let length = crate::position::paragraph_text(paragraph).len();
        read::read_paragraph(&cut(paragraph, from, to, length))
    }

    #[test]
    fn the_whole_paragraph_comes_back_whole() {
        let copy = copied(&paragraph(), 0, 16);
        assert_eq!(copy.plain_text(), "plain bold after");
        assert_eq!(copy.runs.len(), 3);
    }

    #[test]
    fn a_part_of_one_run_comes_back_as_that_part() {
        let copy = copied(&paragraph(), 0, 5);
        assert_eq!(copy.plain_text(), "plain");
        assert_eq!(copy.runs.len(), 1);
    }

    #[test]
    fn the_formatting_of_every_run_comes_with_it() {
        let copy = copied(&paragraph(), 6, 10);
        assert_eq!(copy.plain_text(), "bold");
        assert_eq!(copy.runs[0].properties.bold, Some(true));
    }

    #[test]
    fn a_selection_across_runs_keeps_each_run_as_itself() {
        let copy = copied(&paragraph(), 3, 12);
        assert_eq!(copy.plain_text(), "in bold a");
        assert_eq!(copy.runs.len(), 3, "{:?}", copy.runs);
        assert_eq!(copy.runs[1].properties.bold, Some(true));
    }

    #[test]
    fn nothing_selected_is_nothing_copied() {
        assert!(copied(&paragraph(), 4, 4).runs.is_empty());
    }

    #[test]
    fn the_paragraph_keeps_its_own_properties() {
        assert_eq!(copied(&paragraph(), 0, 5).properties.style.as_deref(), Some("Heading1"));
    }

    /// A paragraph of "ab", a picture, and "cd".
    fn with_a_drawing() -> Element {
        XmlTree::parse(&format!(
            "<w:p xmlns:w=\"{WORDS}\"><w:r><w:t>ab</w:t><w:drawing><wp:inline \
             xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\">\
             <wp:extent cx=\"9\" cy=\"9\"/><a:graphic \
             xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:graphicData>\
             <pic:pic xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">\
             <pic:blipFill><a:blip xmlns:r=\"{}\" r:embed=\"rId7\"/></pic:blipFill></pic:pic>\
             </a:graphicData></a:graphic></wp:inline></w:drawing><w:t>cd</w:t></w:r></w:p>",
            edit::RELATIONSHIPS
        ))
        .expect("the paragraph parses")
        .root
    }

    fn drawings(paragraph: &Paragraph) -> usize {
        paragraph
            .runs
            .iter()
            .flat_map(|run| &run.content)
            .filter(|piece| matches!(piece.bare(), RunContent::Picture(_)))
            .count()
    }

    #[test]
    fn a_picture_inside_the_selection_comes_with_it_and_its_element() {
        let copy = copied(&with_a_drawing(), 1, 4);
        assert_eq!(drawings(&copy), 1);
        let copied = copy
            .runs
            .iter()
            .flat_map(|run| &run.content)
            .find_map(|piece| match piece {
                RunContent::Copied(copied) => Some(copied),
                _ => None,
            })
            .expect("the picture is not carried with its element");
        assert_eq!(copied.element().local_name(), "drawing");
        assert_eq!(relationship_ids(copied.element()), vec!["rId7".to_owned()]);
    }

    #[test]
    fn a_picture_outside_the_selection_stays_behind() {
        let copy = copied(&with_a_drawing(), 0, 2);
        assert_eq!(copy.plain_text(), "ab");
        assert_eq!(drawings(&copy), 0);
        let copy = copied(&with_a_drawing(), 3, 5);
        assert_eq!(copy.plain_text(), "cd");
        assert_eq!(drawings(&copy), 0);
    }

    /// "old", deleted; "NEW"; "X" deleted; "end".
    fn with_deletions() -> Element {
        XmlTree::parse(&format!(
            "<w:p xmlns:w=\"{WORDS}\">\
             <w:del w:id=\"1\" w:author=\"Ann\"><w:r><w:delText>old</w:delText></w:r></w:del>\
             <w:r><w:t>NEW</w:t></w:r>\
             <w:del w:id=\"2\" w:author=\"Ann\"><w:r><w:delText>X</w:delText></w:r></w:del>\
             <w:r><w:t>end</w:t></w:r></w:p>"
        ))
        .expect("the paragraph parses")
        .root
    }

    fn deleted(paragraph: &Paragraph) -> Vec<String> {
        paragraph
            .runs
            .iter()
            .filter(|run| {
                run.revision.as_ref().is_some_and(|change| change.kind == RevisionKind::Deleted)
            })
            .flat_map(|run| &run.content)
            .filter_map(|piece| match piece {
                RunContent::Text(text) => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_deletion_between_what_is_copied_comes_with_it() {
        // "NEWend": the second deletion lies between the W and the e.
        let copy = copied(&with_deletions(), 1, 5);
        assert_eq!(copy.plain_text(), "EWen");
        assert_eq!(deleted(&copy), vec!["X".to_owned()]);
    }

    #[test]
    fn a_deletion_at_an_edge_of_the_copy_comes_only_at_an_edge_of_the_paragraph() {
        // At the start of the paragraph: nothing else it could belong to.
        assert_eq!(deleted(&copied(&with_deletions(), 0, 2)), vec!["old".to_owned()]);
        // Between the copy and what was left out: left out with it.
        assert!(deleted(&copied(&with_deletions(), 3, 6)).is_empty());
        assert!(deleted(&copied(&with_deletions(), 1, 3)).is_empty());
    }

    #[test]
    fn a_copy_is_written_only_once_a_paste_has_made_it_where_it_lands() {
        let mut body = crate::model::Body::default();
        body.blocks.push(Block::Paragraph(Paragraph::text("")));
        let mut document = Document::create(&body).expect("a document");
        assert!(document.insert_picture(b"bytes", "png", 9, 9).expect("a picture"));
        document.set_caret(TextPosition::new(0, 0));
        document.extend_selection_to(TextPosition::new(0, 1));
        let copied = document.copy_selection();
        let Some(Block::Paragraph(paragraph)) = copied.first() else { panic!("{copied:?}") };
        let run = &paragraph.runs[0];
        let RunContent::Copied(copy) = &run.content[0] else { panic!("{:?}", run.content) };
        assert!(!copy.is_settled(), "a copy of a picture names nothing it has to bring");
        assert_eq!(copy.parts().map(|(_, bytes)| bytes).collect::<Vec<_>>(), vec![&b"bytes"[..]]);

        // Written by anything but a paste, it would point at whatever the
        // part it went into calls rId of that number; so it is not written.
        fn holds_a_drawing(element: &Element) -> bool {
            element.local_name() == "drawing" || element.child_elements().any(holds_a_drawing)
        }
        let written = edit::run_elements(run, Some("w"));
        assert!(!written.iter().any(holds_a_drawing), "{written:?}");
    }

    #[test]
    fn a_name_nothing_has_is_the_one_it_had_or_the_next_number() {
        let taken = ["word/charts/chart1.xml", "word/charts/chart2.xml"];
        let free = |name: &str| !taken.contains(&name);
        assert_eq!(fresh_name("word/charts/chart1.xml", free), "word/charts/chart3.xml");
        assert_eq!(fresh_name("word/media/image9.png", free), "word/media/image9.png");
        let free = |name: &str| name != "word/embeddings/Book.xlsx";
        assert_eq!(fresh_name("word/embeddings/Book.xlsx", free), "word/embeddings/Book1.xlsx");
    }

    #[test]
    fn a_target_is_written_from_the_folder_of_the_part_that_names_it() {
        assert_eq!(
            relative_target("word/document.xml", "word/media/image1.png"),
            "media/image1.png"
        );
        assert_eq!(
            relative_target("word/charts/chart2.xml", "word/embeddings/Book2.xlsx"),
            "../embeddings/Book2.xlsx"
        );
        assert_eq!(
            relative_target("word/diagrams/data1.xml", "word/diagrams/drawing1.xml"),
            "drawing1.xml"
        );
    }

    #[test]
    fn an_element_taken_out_declares_what_it_uses() {
        let root = XmlTree::parse(&format!(
            "<w:document xmlns:w=\"{WORDS}\" \
             xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\">\
             <w:body><w:p><w:r><w:drawing><wp:inline/></w:drawing></w:r></w:p></w:body>\
             </w:document>"
        ))
        .expect("parses")
        .root;
        let mut drawing = root
            .child_elements()
            .next()
            .and_then(|body| body.child_elements().next())
            .and_then(|paragraph| paragraph.child_elements().next())
            .and_then(|run| run.child_elements().next())
            .expect("the drawing")
            .clone();
        declare_what_it_uses(&mut drawing, &root);
        let alone = XmlTree {
            standalone: None,
            has_declaration: false,
            doctype: None,
            before_root: Vec::new(),
            root: drawing,
            after_root: Vec::new(),
        };
        let text = alone.to_xml().expect("writes");
        let again = XmlTree::parse(&text).expect("what was taken out is XML on its own");
        assert_eq!(again.root.local_name(), "drawing");
    }
}
