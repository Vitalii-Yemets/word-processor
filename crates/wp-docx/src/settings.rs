//! The document's settings part, and the switches that live in it.
//!
//! # Why so many things end up here
//!
//! `word/settings.xml` is where Word keeps everything that is true of the
//! document rather than of any paragraph in it: whether changes are being
//! recorded, whether words are hyphenated, whether the document may be edited
//! at all. None of it is text, none of it is formatting, and all of it has to
//! survive being saved.
//!
//! # The order matters
//!
//! `w:settings` is a schema sequence, not a bag: a setting written in the wrong
//! place makes a file Word opens and then repairs. That is what
//! [`SETTINGS_ORDER`] is for — every setting this program writes is put where
//! the schema says it goes, whatever order it was switched on in.

use wp_xml::tree::{Element, XmlTree};

use crate::{edit, read, Document};

/// Content type of the settings part.
pub(crate) const CONTENT_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml";

/// Relationship type of that part.
pub(crate) const RELATIONSHIP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings";

/// The order the schema requires, as far as this program writes into it.
///
/// Not the whole sequence — the rest of it is settings nothing here touches,
/// and an unknown name is put at the end, which is where a setting this list
/// does not mention belongs anyway.
pub(crate) const SETTINGS_ORDER: &[&str] = &[
    "writeProtection",
    "view",
    "zoom",
    "removePersonalInformation",
    "removeDateAndTime",
    "doNotDisplayPageBoundaries",
    "displayBackgroundShape",
    "printPostScriptOverText",
    "printFractionalCharacterWidth",
    "printFormsData",
    "embedTrueTypeFonts",
    "embedSystemFonts",
    "saveSubsetFonts",
    "saveFormsData",
    "mirrorMargins",
    "alignBordersAndEdges",
    "bordersDoNotSurroundHeader",
    "bordersDoNotSurroundFooter",
    "gutterAtTop",
    "hideSpellingErrors",
    "hideGrammaticalErrors",
    "activeWritingStyle",
    "proofState",
    "formsDesign",
    "attachedTemplate",
    "linkStyles",
    "stylePaneFormatFilter",
    "stylePaneSortMethod",
    "documentType",
    "mailMerge",
    "revisionView",
    "trackChanges",
    "doNotTrackMoves",
    "doNotTrackFormatting",
    "documentProtection",
    "autoFormatOverride",
    "styleLockTheme",
    "styleLockQFSet",
    "defaultTabStop",
    "autoHyphenation",
    "consecutiveHyphenLimit",
    "hyphenationZone",
    "doNotHyphenateCaps",
    "showEnvelope",
    "summaryLength",
    "clickAndTypeStyle",
    "defaultTableStyle",
    "evenAndOddHeaders",
    "bookFoldRevPrinting",
    "bookFoldPrinting",
    "bookFoldPrintingSheets",
    "characterSpacingControl",
    "printTwoOnOne",
    "savePreviewPicture",
    "updateFields",
    "footnotePr",
    "endnotePr",
    "compat",
    "docVars",
    "rsids",
    "mathPr",
    "themeFontLang",
    "clrSchemeMapping",
    "shapeDefaults",
    "decimalSymbol",
    "listSeparator",
];

impl Document {
    /// The name of the settings part, if the document has one.
    pub(crate) fn settings_part(&self) -> Option<String> {
        let relationships = self.package().relationships(self.main_part()).ok()?;
        let relationship = relationships.single_by_type(RELATIONSHIP)?;
        relationship.resolved_target(self.main_part())?.ok()
    }

    /// The `w:settings` element, read afresh.
    ///
    /// Not held anywhere: the settings are small and this is asked only when a
    /// menu is drawn or a setting is changed.
    pub(crate) fn settings_root(&self) -> Option<Element> {
        let part = self.settings_part()?;
        let text = self.package().xml_part(&part)?.ok()?;
        XmlTree::parse(&text).ok().map(|tree| tree.root)
    }

    /// Writes the settings part back.
    pub(crate) fn save_settings_root(&mut self, root: Element) -> bool {
        let Some(part) = self.settings_part() else { return false };
        let tree = XmlTree {
            standalone: Some(true),
            has_declaration: true,
            doctype: None,
            before_root: Vec::new(),
            root,
            after_root: Vec::new(),
        };
        let Ok(xml) = tree.to_xml() else { return false };
        self.package_mut().add_part(&part, CONTENT_TYPE, xml.into_bytes());
        true
    }

    /// What a setting that carries a number or a word says, if it says
    /// anything.
    pub(crate) fn settings_value(&self, local: &str) -> Option<String> {
        self.settings_root().and_then(|root| {
            root.child(Some(read::W), local).and_then(read::value).map(str::to_owned)
        })
    }

    /// Whether a switch in the settings is on.
    ///
    /// An element that is there means yes unless it says otherwise, which is
    /// the rule every on-off property in the format follows.
    pub(crate) fn setting_is_on(&self, local: &str) -> bool {
        self.settings_root()
            .and_then(|root| root.child(Some(read::W), local).map(read::on_off))
            .unwrap_or(false)
    }

    /// Turns a switch in the settings on or off.
    ///
    /// Returns whether anything changed, so a command can say nothing happened
    /// rather than claim it did.
    pub(crate) fn set_setting_flag(&mut self, local: &str, on: bool) -> bool {
        let Some(mut root) = self.settings_root() else { return false };
        if root.child(Some(read::W), local).is_some_and(read::on_off) == on {
            return false;
        }

        root.remove_children_named(Some(read::W), local);
        if on {
            let prefix = self.prefix();
            let element = Element::new(&edit::name_with(prefix.as_deref(), local), Some(read::W));
            edit::insert_ordered(&mut root, element, SETTINGS_ORDER);
        }

        if !self.save_settings_root(root) {
            return false;
        }
        self.mark_modified();
        true
    }

    /// The value of a settings element's `w:val`, if it has one.
    pub(crate) fn setting_value(&self, local: &str) -> Option<String> {
        let root = self.settings_root()?;
        let child = root.child(Some(read::W), local)?;
        child.attribute(Some(read::W), "val").map(str::to_owned)
    }

    /// Writes a settings element carrying a `w:val`, or takes it away.
    pub(crate) fn set_setting_value(&mut self, local: &str, value: Option<&str>) -> bool {
        let Some(mut root) = self.settings_root() else { return false };
        let already = root
            .child(Some(read::W), local)
            .and_then(|child| child.attribute(Some(read::W), "val"))
            .map(str::to_owned);
        if already.as_deref() == value {
            return false;
        }

        root.remove_children_named(Some(read::W), local);
        if let Some(value) = value {
            let prefix = self.prefix();
            let mut element =
                Element::new(&edit::name_with(prefix.as_deref(), local), Some(read::W));
            element.set_namespaced_attribute(
                &edit::name_with(prefix.as_deref(), "val"),
                read::W,
                value,
            );
            edit::insert_ordered(&mut root, element, SETTINGS_ORDER);
        }

        if !self.save_settings_root(root) {
            return false;
        }
        self.mark_modified();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::SETTINGS_ORDER;

    #[test]
    fn the_settings_this_program_writes_are_all_in_the_order() {
        for name in ["displayBackgroundShape", "trackChanges", "documentProtection"] {
            assert!(SETTINGS_ORDER.contains(&name), "{name} is missing from the order");
        }
        for name in ["autoHyphenation", "hyphenationZone", "doNotHyphenateCaps"] {
            assert!(SETTINGS_ORDER.contains(&name), "{name} is missing from the order");
        }
    }

    #[test]
    fn the_order_names_nothing_twice() {
        let mut seen = SETTINGS_ORDER.to_vec();
        seen.sort_unstable();
        let before = seen.len();
        seen.dedup();
        assert_eq!(seen.len(), before, "a setting is named twice");
    }

    #[test]
    fn tracking_comes_before_protection_and_hyphenation_after_it() {
        let at = |name: &str| SETTINGS_ORDER.iter().position(|entry| *entry == name).expect(name);
        assert!(at("trackChanges") < at("documentProtection"));
        assert!(at("documentProtection") < at("autoHyphenation"));
        assert!(at("displayBackgroundShape") < at("trackChanges"));
    }
}
