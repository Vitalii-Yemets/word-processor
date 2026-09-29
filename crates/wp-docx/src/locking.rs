//! Limiting formatting to a selection of styles.
//!
//! # What the restriction is
//!
//! The top half of Word's Restrict Editing. A document may say that only some
//! of its styles may be applied and that no formatting may be written directly
//! on to a run or a paragraph at all — no bold, no other font, no indent —
//! so that everything in it is formatted by a style and every style is one
//! somebody chose.
//!
//! It exists because a document that is passed round is formatted by whoever
//! touched it last. A style-limited one can only come back looking like
//! itself, and that is a house style enforced by the file rather than by
//! asking.
//!
//! # How it is written down
//!
//! Two parts of the package, and neither means anything without the other:
//!
//! * `w:documentProtection` in the settings carries `w:formatting="1"`. That
//!   is the restriction — see [`crate::protection`].
//! * Every style that may **not** be applied carries `<w:locked/>` in
//!   `styles.xml`. The ones that may are simply not marked.
//!
//! So the lock is on the styles and the switch is on the document, and a
//! document with locked styles and no enforcement behaves as though nothing
//! were locked. This is the format's rule and not a choice made here: Word
//! leaves the marks in place when a restriction is lifted, which is what makes
//! putting the same restriction back a matter of one tick.
//!
//! # The two sorts of style a document holds
//!
//! A `styles.xml` **defines** a few dozen and **mentions** a few hundred. The
//! mentioned ones are `w:lsdException` entries inside `w:latentStyles`: Word's
//! built-in styles that this document has not needed yet, each with its own
//! say about whether it is hidden, how it sorts, and whether it is locked.
//! They are as real as the defined ones to somebody applying a style, so a
//! restriction that dealt only with the defined ones would leave three hundred
//! doors open.
//!
//! # Latent styles
//!
//! A `styles.xml` defines some styles and mentions hundreds. The mentioned
//! ones — `w:latentStyles` — are Word's built-in styles that this document has
//! not needed yet, and a program that locked only what was defined would leave
//! every one of those free to be applied. So the restriction also sets
//! `w:defLockedState="1"`, which locks everything that has not been given a
//! definition, and that is what Word does.

use wp_xml::tree::Element;

use crate::{edit, read, styles::Styles, Document};

/// Where `w:locked` goes inside a style definition.
///
/// The schema fixes the order of a style's children, and a `w:locked` written
/// after the formatting is a document Word refuses to open. Everything in this
/// list comes after it, so the mark goes in front of the first one found.
const AFTER_LOCKED: &[&str] =
    &["pPr", "rPr", "tblPr", "trPr", "tcPr", "tblStylePr", "personal", "personalCompose"];

/// One of the styles a document mentions without defining.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Latent {
    /// The name Word knows it by, which is what the exception carries: there
    /// is no identifier, because there is no definition to identify.
    pub name: String,
    /// Whether this one may not be applied.
    pub locked: bool,
    /// And whether the document puts it forward as one to write with.
    pub recommended: bool,
}

impl Document {
    /// Whether the document limits formatting to a selection of styles right
    /// now.
    #[must_use]
    pub fn formatting_is_limited(&self) -> bool {
        self.protection_rules().is_some_and(|rules| rules.formatting)
    }

    /// Whether the theme is fixed as well.
    #[must_use]
    pub fn theme_is_locked(&self) -> bool {
        self.protection_rules().is_some_and(|rules| rules.theme_locked)
    }

    /// Whether a style is marked as one that may not be applied.
    ///
    /// The mark alone, without asking whether anything is being enforced —
    /// which is the question the dialog asks, because it is showing what is
    /// ticked and not what is allowed this minute.
    #[must_use]
    pub fn style_is_locked(&self, id: &str) -> bool {
        self.styles().get(id).is_some_and(|style| style.locked)
    }

    /// Whether a style may be applied as things stand.
    ///
    /// The question everything else asks: the gallery, to know what to offer,
    /// and the command, to know whether to run. A document with no restriction
    /// allows every style it has, however they are marked.
    #[must_use]
    pub fn style_is_available(&self, id: &str) -> bool {
        !self.formatting_is_limited() || !self.style_is_locked(id)
    }

    /// Every style that may be applied, by identifier, in the order the
    /// document defines them.
    #[must_use]
    pub fn allowed_styles(&self) -> Vec<String> {
        self.styles()
            .all()
            .iter()
            .filter(|style| !style.locked)
            .map(|style| style.id.clone())
            .collect()
    }

    /// Every style the document mentions without defining, in the order it
    /// mentions them.
    ///
    /// Empty where the styles part has no `w:latentStyles` at all, which is
    /// what a document made by this program has: the list is Word's, and a
    /// document that never met Word has nothing latent about it.
    #[must_use]
    pub fn latent_styles(&self) -> Vec<Latent> {
        let Some(tree) = self.styles_tree() else { return Vec::new() };
        let Some(latent) = tree.root.child(Some(read::W), "latentStyles") else {
            return Vec::new();
        };
        let default_locked = read::attribute_is_on(latent, "defLockedState");
        latent
            .children_named(Some(read::W), "lsdException")
            .filter_map(|exception| {
                let name = exception.attribute(Some(read::W), "name")?;
                Some(Latent {
                    name: name.to_owned(),
                    locked: exception
                        .attribute(Some(read::W), "locked")
                        .map_or(default_locked, read::on_off_value),
                    recommended: exception
                        .attribute(Some(read::W), "qFormat")
                        .is_some_and(read::on_off_value),
                })
            })
            .collect()
    }

    /// Whether one of them may be applied as things stand.
    #[must_use]
    pub fn latent_style_is_available(&self, name: &str) -> bool {
        !self.formatting_is_limited()
            || !self
                .latent_styles()
                .iter()
                .any(|latent| latent.name.eq_ignore_ascii_case(name) && latent.locked)
    }

    /// Locks one of them by name, or frees it.
    ///
    /// The exception is made where there is none: a latent style covered only
    /// by the default state has nothing of its own to change, and saying
    /// something about one is what makes it an exception.
    pub fn set_latent_style_locked(&mut self, name: &str, locked: bool) -> bool {
        let name = name.trim().to_owned();
        if name.is_empty() {
            return false;
        }
        self.change_styles(move |root, prefix| {
            if root.child(Some(read::W), "latentStyles").is_none() {
                lock_latent(root, false, prefix);
            }
            let Some(latent) =
                root.child_elements_mut().find(|child| child.is(Some(read::W), "latentStyles"))
            else {
                return false;
            };

            if latent
                .child_elements()
                .find(|child| {
                    child.is(Some(read::W), "lsdException")
                        && child
                            .attribute(Some(read::W), "name")
                            .is_some_and(|found| found.eq_ignore_ascii_case(&name))
                })
                .is_none()
            {
                let mut exception =
                    Element::new(&edit::name_with(prefix, "lsdException"), Some(read::W));
                exception.set_namespaced_attribute(
                    &edit::name_with(prefix, "name"),
                    read::W,
                    &name,
                );
                latent.push_element(exception);
            }

            let Some(exception) = latent.child_elements_mut().find(|child| {
                child.is(Some(read::W), "lsdException")
                    && child
                        .attribute(Some(read::W), "name")
                        .is_some_and(|found| found.eq_ignore_ascii_case(&name))
            }) else {
                return false;
            };
            let before = exception.attribute(Some(read::W), "locked").map(str::to_owned);
            exception.set_namespaced_attribute(
                &edit::name_with(prefix, "locked"),
                read::W,
                if locked { "1" } else { "0" },
            );
            before.as_deref() != exception.attribute(Some(read::W), "locked")
        })
    }

    /// Marks one style locked, or unlocks it.
    pub fn set_style_locked(&mut self, id: &str, locked: bool) -> bool {
        let Some(found) = self.styles().get(id).map(|style| style.id.clone()) else { return false };
        self.change_styles(|root, prefix| mark(root, &found, locked, prefix))
    }

    /// Allows exactly these styles and locks every other one.
    ///
    /// The whole answer at once, because that is the question the dialog asks:
    /// a list with ticks against some of it. Passing a style the document has
    /// not got allows nothing and locks nothing — there is no such style to
    /// mark.
    pub fn allow_only_styles(&mut self, allowed: &[String]) -> bool {
        let wanted: Vec<String> = self
            .styles()
            .all()
            .iter()
            .filter(|style| !allowed.iter().any(|id| id.eq_ignore_ascii_case(&style.id)))
            .map(|style| style.id.clone())
            .collect();
        let latent = !allowed.is_empty() || !wanted.is_empty();

        self.change_styles(|root, prefix| {
            let mut changed = false;
            let ids: Vec<String> = root
                .children_named(Some(read::W), "style")
                .filter_map(|style| style.attribute(Some(read::W), "styleId"))
                .map(str::to_owned)
                .collect();
            for id in ids {
                let locked = wanted.iter().any(|other| other.eq_ignore_ascii_case(&id));
                changed |= mark(root, &id, locked, prefix);
            }
            // And the hundreds a `styles.xml` mentions without defining.
            if latent {
                changed |= lock_latent(root, true, prefix);
            }
            changed
        })
    }

    /// Lets every style be applied again, which is what unticking the
    /// restriction's own box means.
    pub fn allow_every_style(&mut self) -> bool {
        self.change_styles(|root, prefix| {
            let mut changed = lock_latent(root, false, prefix);
            for style in root.child_elements_mut() {
                if style.is(Some(read::W), "style")
                    && style.child(Some(read::W), "locked").is_some()
                {
                    style.remove_children_named(Some(read::W), "locked");
                    changed = true;
                }
            }
            changed
        })
    }

    /// Reads the styles part, lets a caller change it, and writes it back.
    fn change_styles(&mut self, change: impl FnOnce(&mut Element, Option<&str>) -> bool) -> bool {
        let Some(mut tree) = self.styles_tree() else { return false };
        let prefix = edit::prefix_for(&tree.root, crate::WORDPROCESSING_NAMESPACE);
        if !change(&mut tree.root, prefix.as_deref()) {
            return false;
        }
        self.styles = Styles::parse(&tree.root).with_theme(self.styles.theme().clone());
        self.save_styles_tree(&tree);
        self.note_change();
        true
    }
}

/// Puts `w:locked` on to one style definition, or takes it off.
fn mark(root: &mut Element, id: &str, locked: bool, prefix: Option<&str>) -> bool {
    let Some(style) = root.child_elements_mut().find(|style| {
        style.is(Some(read::W), "style")
            && style
                .attribute(Some(read::W), "styleId")
                .is_some_and(|found| found.eq_ignore_ascii_case(id))
    }) else {
        return false;
    };

    let marked = style.child(Some(read::W), "locked").is_some();
    if marked == locked {
        return false;
    }
    if locked {
        let element = Element::new(&edit::name_with(prefix, "locked"), Some(read::W));
        let at = style
            .child_elements()
            .position(|child| AFTER_LOCKED.iter().any(|local| child.is(Some(read::W), local)));
        match at {
            Some(at) => style.insert_element(at, element),
            None => style.push_element(element),
        }
    } else {
        style.remove_children_named(Some(read::W), "locked");
    }
    true
}

/// Locks, or frees, every style the document mentions without defining.
fn lock_latent(root: &mut Element, locked: bool, prefix: Option<&str>) -> bool {
    if root.child(Some(read::W), "latentStyles").is_none() {
        if !locked {
            // Nothing to free, and an empty element written only to say that
            // nothing is locked would be a change that changes nothing.
            return false;
        }
        let element = Element::new(&edit::name_with(prefix, "latentStyles"), Some(read::W));
        // After `w:docDefaults` and before the definitions, which is where the
        // schema puts it.
        let at = usize::from(root.child(Some(read::W), "docDefaults").is_some());
        root.insert_element(at, element);
    }

    let Some(latent) =
        root.child_elements_mut().find(|child| child.is(Some(read::W), "latentStyles"))
    else {
        return false;
    };
    let before = latent.attribute(Some(read::W), "defLockedState").map(str::to_owned);
    if locked {
        latent.set_namespaced_attribute(&edit::name_with(prefix, "defLockedState"), read::W, "1");
    } else {
        latent.remove_namespaced_attribute(read::W, "defLockedState");
    }
    before.as_deref() != latent.attribute(Some(read::W), "defLockedState")
}
