//! The document that asks to be opened read-only, and the password that opens
//! it for writing.
//!
//! # A different question from the other two passwords
//!
//! A `.docx` can carry three, and they answer three questions:
//!
//! * **Password to open** — [`crate::sealing`]. Without it there is no
//!   readable text in the file at all. That is encryption.
//! * **Password to modify** — this one. The file is readable by anybody; what
//!   the password says is whether the program opens it for writing or hands
//!   back something that can be read and not changed.
//! * **The restriction** — [`crate::protection`]. The file is open for
//!   writing and the document says which parts of it may be changed.
//!
//! Word sets the first two together, under Save As ▸ Tools ▸ General Options,
//! which is why they are so often confused. They are not the same: this one
//! protects nothing from a program that ignores it, and neither does the
//! third. What it does is stop the person who was sent the file editing it by
//! habit — it is the difference between being handed a document and being
//! handed the master copy.
//!
//! # What is written
//!
//! `w:writeProtection`, the first element of the settings part:
//!
//! ```xml
//! <w:writeProtection w:recommended="1"
//!                    w:algorithmName="SHA-512" w:hashValue="…"
//!                    w:saltValue="…" w:spinCount="100000"/>
//! ```
//!
//! `w:recommended` on its own is Word's "Read-only recommended", which asks
//! and takes no for an answer. The hash is the password, written exactly as
//! [`crate::protection`] writes the other one — both sets of attributes are
//! read, and the newer set is written.

use wp_xml::tree::Element;

use crate::protection::Password;
use crate::{edit, read, Document};

/// What a document says about being opened for writing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WriteProtection {
    /// Whether the document merely asks. Word's "Read-only recommended": the
    /// program says what the author wanted and opens it either way.
    pub recommended: bool,
    /// The password that opens it for writing, if there is one.
    pub password: Option<Password>,
}

impl WriteProtection {
    /// A document that asks to be read and not written.
    #[must_use]
    pub fn recommended() -> Self {
        Self { recommended: true, password: None }
    }

    /// One that asks for a password before it is written.
    #[must_use]
    pub fn behind(word: &str, salt: &[u8]) -> Self {
        Self { recommended: false, password: Some(Password::new(word, salt)) }
    }

    /// Whether what was typed opens it for writing.
    ///
    /// A document that only recommends is opened for writing by anybody, and
    /// says so: the recommendation is a request, and a request that could not
    /// be refused would be a restriction wearing a request's clothes.
    #[must_use]
    pub fn opens_with(&self, attempt: &str) -> bool {
        self.password.as_ref().is_none_or(|password| password.accepts(attempt))
    }

    /// Whether it says anything at all.
    #[must_use]
    pub fn asks_anything(&self) -> bool {
        self.recommended || self.password.is_some()
    }
}

impl Document {
    /// What the document asks about being opened for writing, if it asks.
    #[must_use]
    pub fn write_protection(&self) -> Option<WriteProtection> {
        let root = self.settings_root()?;
        let element = root.child(Some(read::W), "writeProtection")?;
        let found = WriteProtection {
            recommended: read::attribute_is_on(element, "recommended"),
            password: Password::read(element),
        };
        found.asks_anything().then_some(found)
    }

    /// Sets it, or takes it off.
    ///
    /// Nothing is checked here, as nothing is checked when a restriction is
    /// set: a document does not refuse to have its own settings written, and
    /// the asking is done by the program in front of the person.
    pub fn set_write_protection(&mut self, wanted: Option<&WriteProtection>) -> bool {
        if self.write_protection().as_ref() == wanted {
            return false;
        }
        let Some(mut root) = self.settings_root() else { return false };

        let Some(wanted) = wanted.filter(|asked| asked.asks_anything()) else {
            root.remove_children_named(Some(read::W), "writeProtection");
            if !self.save_settings_root(root) {
                return false;
            }
            self.mark_modified();
            return true;
        };

        let prefix = self.prefix();
        let name = |local: &str| edit::name_with(prefix.as_deref(), local);
        // Edited where it stands rather than written again, for the reason
        // the other restriction is: a document must not lose what it came
        // with because something this program does model was changed.
        if root.child(Some(read::W), "writeProtection").is_none() {
            let element = Element::new(&name("writeProtection"), Some(read::W));
            edit::insert_ordered(&mut root, element, crate::settings::SETTINGS_ORDER);
        }
        let Some(element) =
            root.child_elements_mut().find(|child| child.is(Some(read::W), "writeProtection"))
        else {
            return false;
        };

        if wanted.recommended {
            element.set_namespaced_attribute(&name("recommended"), read::W, "1");
        } else {
            element.remove_namespaced_attribute(read::W, "recommended");
        }
        match &wanted.password {
            Some(password) => password.write(element, prefix.as_deref()),
            None => Password::unwrite(element),
        }

        if !self.save_settings_root(root) {
            return false;
        }
        self.mark_modified();
        true
    }
}
