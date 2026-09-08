//! The proofing language: which dictionary a stretch of text is checked
//! against.
//!
//! # Why text carries a language at all
//!
//! Because a document is not written in one. A quotation in French inside an
//! English paper is French, and a spelling checker that does not know it will
//! underline every word of it. So the language is a property of the *run*, like
//! the font — not of the document, and not of the program.
//!
//! # What is stored
//!
//! A tag such as `en-GB` or `ru-RU`: the language, then the place it is written
//! in. The place matters — `en-GB` and `en-US` disagree about words a reader
//! notices — which is why the tag has two halves and not one.
//!
//! # About the list
//!
//! Word offers every language it has a dictionary for, which is over a hundred.
//! What is here is the tags and the names, which is what a document needs; the
//! dictionaries themselves are a separate matter and there are none here yet.
//! A tag this list does not name is still read, kept and written back — it is
//! shown as the tag itself rather than thrown away.

use crate::model::RunProperties;
use crate::Document;

/// One language a document can be written in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Language {
    /// The tag written into the file, such as `en-GB`.
    pub tag: &'static str,
    /// What it is called, in English, as Word names it.
    pub name: &'static str,
}

/// The languages offered, in the order their names sort.
///
/// Not every language Word knows — see the note at the top of this module — but
/// every one with enough writers that leaving it out would be a decision rather
/// than an omission.
pub const LANGUAGES: &[Language] = &[
    Language { tag: "af-ZA", name: "Afrikaans" },
    Language { tag: "sq-AL", name: "Albanian" },
    Language { tag: "ar-EG", name: "Arabic (Egypt)" },
    Language { tag: "ar-SA", name: "Arabic (Saudi Arabia)" },
    Language { tag: "hy-AM", name: "Armenian" },
    Language { tag: "az-Latn-AZ", name: "Azerbaijani" },
    Language { tag: "eu-ES", name: "Basque" },
    Language { tag: "be-BY", name: "Belarusian" },
    Language { tag: "bn-IN", name: "Bengali" },
    Language { tag: "bs-Latn-BA", name: "Bosnian" },
    Language { tag: "bg-BG", name: "Bulgarian" },
    Language { tag: "ca-ES", name: "Catalan" },
    Language { tag: "zh-CN", name: "Chinese (Simplified)" },
    Language { tag: "zh-TW", name: "Chinese (Traditional)" },
    Language { tag: "hr-HR", name: "Croatian" },
    Language { tag: "cs-CZ", name: "Czech" },
    Language { tag: "da-DK", name: "Danish" },
    Language { tag: "nl-NL", name: "Dutch" },
    Language { tag: "en-AU", name: "English (Australia)" },
    Language { tag: "en-CA", name: "English (Canada)" },
    Language { tag: "en-IN", name: "English (India)" },
    Language { tag: "en-IE", name: "English (Ireland)" },
    Language { tag: "en-NZ", name: "English (New Zealand)" },
    Language { tag: "en-ZA", name: "English (South Africa)" },
    Language { tag: "en-GB", name: "English (United Kingdom)" },
    Language { tag: "en-US", name: "English (United States)" },
    Language { tag: "et-EE", name: "Estonian" },
    Language { tag: "fi-FI", name: "Finnish" },
    Language { tag: "fr-BE", name: "French (Belgium)" },
    Language { tag: "fr-CA", name: "French (Canada)" },
    Language { tag: "fr-FR", name: "French (France)" },
    Language { tag: "fr-CH", name: "French (Switzerland)" },
    Language { tag: "gl-ES", name: "Galician" },
    Language { tag: "ka-GE", name: "Georgian" },
    Language { tag: "de-AT", name: "German (Austria)" },
    Language { tag: "de-DE", name: "German (Germany)" },
    Language { tag: "de-CH", name: "German (Switzerland)" },
    Language { tag: "el-GR", name: "Greek" },
    Language { tag: "gu-IN", name: "Gujarati" },
    Language { tag: "he-IL", name: "Hebrew" },
    Language { tag: "hi-IN", name: "Hindi" },
    Language { tag: "hu-HU", name: "Hungarian" },
    Language { tag: "is-IS", name: "Icelandic" },
    Language { tag: "id-ID", name: "Indonesian" },
    Language { tag: "ga-IE", name: "Irish" },
    Language { tag: "it-IT", name: "Italian" },
    Language { tag: "ja-JP", name: "Japanese" },
    Language { tag: "kn-IN", name: "Kannada" },
    Language { tag: "kk-KZ", name: "Kazakh" },
    Language { tag: "km-KH", name: "Khmer" },
    Language { tag: "ko-KR", name: "Korean" },
    Language { tag: "ky-KG", name: "Kyrgyz" },
    Language { tag: "lo-LA", name: "Lao" },
    Language { tag: "lv-LV", name: "Latvian" },
    Language { tag: "lt-LT", name: "Lithuanian" },
    Language { tag: "mk-MK", name: "Macedonian" },
    Language { tag: "ms-MY", name: "Malay" },
    Language { tag: "ml-IN", name: "Malayalam" },
    Language { tag: "mt-MT", name: "Maltese" },
    Language { tag: "mr-IN", name: "Marathi" },
    Language { tag: "mn-MN", name: "Mongolian" },
    Language { tag: "ne-NP", name: "Nepali" },
    Language { tag: "nb-NO", name: "Norwegian (Bokmal)" },
    Language { tag: "nn-NO", name: "Norwegian (Nynorsk)" },
    Language { tag: "fa-IR", name: "Persian" },
    Language { tag: "pl-PL", name: "Polish" },
    Language { tag: "pt-BR", name: "Portuguese (Brazil)" },
    Language { tag: "pt-PT", name: "Portuguese (Portugal)" },
    Language { tag: "pa-IN", name: "Punjabi" },
    Language { tag: "ro-RO", name: "Romanian" },
    Language { tag: "ru-RU", name: "Russian" },
    Language { tag: "sr-Cyrl-RS", name: "Serbian (Cyrillic)" },
    Language { tag: "sr-Latn-RS", name: "Serbian (Latin)" },
    Language { tag: "si-LK", name: "Sinhala" },
    Language { tag: "sk-SK", name: "Slovak" },
    Language { tag: "sl-SI", name: "Slovenian" },
    Language { tag: "es-AR", name: "Spanish (Argentina)" },
    Language { tag: "es-MX", name: "Spanish (Mexico)" },
    Language { tag: "es-ES", name: "Spanish (Spain)" },
    Language { tag: "sw-KE", name: "Swahili" },
    Language { tag: "sv-SE", name: "Swedish" },
    Language { tag: "ta-IN", name: "Tamil" },
    Language { tag: "te-IN", name: "Telugu" },
    Language { tag: "th-TH", name: "Thai" },
    Language { tag: "tr-TR", name: "Turkish" },
    Language { tag: "uk-UA", name: "Ukrainian" },
    Language { tag: "ur-PK", name: "Urdu" },
    Language { tag: "uz-Latn-UZ", name: "Uzbek" },
    Language { tag: "vi-VN", name: "Vietnamese" },
    Language { tag: "cy-GB", name: "Welsh" },
    Language { tag: "zu-ZA", name: "Zulu" },
];

/// The language a document falls back to when nothing says otherwise.
pub const DEFAULT_TAG: &str = "en-GB";

/// What to call a tag.
///
/// A tag the list does not name is shown as itself: a document written in a
/// language this program has never heard of still says which, and showing the
/// tag is more use than showing nothing.
#[must_use]
pub fn name_of(tag: &str) -> String {
    if let Some(found) = LANGUAGES.iter().find(|entry| entry.tag.eq_ignore_ascii_case(tag)) {
        return found.name.to_owned();
    }
    // A tag with a place this list does not have — `en-JM`, say — is still
    // recognisably the language before the dash.
    if let Some((language, _)) = tag.split_once('-') {
        if let Some(found) =
            LANGUAGES.iter().find(|entry| entry.tag.split('-').next() == Some(language))
        {
            let bare = found.name.split(" (").next().unwrap_or(found.name);
            return format!("{bare} ({tag})");
        }
    }
    tag.to_owned()
}

impl Document {
    /// The proofing language where the caret is.
    #[must_use]
    pub fn language_here(&self) -> String {
        self.resolved_at_caret().language.unwrap_or_else(|| DEFAULT_TAG.to_owned())
    }

    /// Sets the proofing language of the selection, or of what is typed next.
    pub fn set_language(&mut self, tag: &str) -> bool {
        self.apply_character_change(&RunProperties {
            language: Some(tag.to_owned()),
            ..RunProperties::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{name_of, DEFAULT_TAG, LANGUAGES};

    #[test]
    fn a_tag_in_the_list_is_named() {
        assert_eq!(name_of("ru-RU"), "Russian");
        assert_eq!(name_of("en-GB"), "English (United Kingdom)");
    }

    #[test]
    fn a_tag_is_named_whatever_case_it_is_written_in() {
        assert_eq!(name_of("RU-ru"), "Russian");
    }

    #[test]
    fn a_place_the_list_does_not_have_still_names_the_language() {
        assert_eq!(name_of("en-JM"), "English (en-JM)");
        assert_eq!(name_of("de-LI"), "German (de-LI)");
    }

    #[test]
    fn a_tag_the_list_has_never_heard_of_is_shown_as_itself() {
        assert_eq!(name_of("xx-YY"), "xx-YY");
    }

    #[test]
    fn every_tag_is_named_once() {
        for (at, entry) in LANGUAGES.iter().enumerate() {
            assert!(
                !LANGUAGES[..at].iter().any(|earlier| earlier.tag == entry.tag),
                "{} is listed twice",
                entry.tag
            );
        }
    }

    #[test]
    fn every_name_is_used_once() {
        for (at, entry) in LANGUAGES.iter().enumerate() {
            assert!(
                !LANGUAGES[..at].iter().any(|earlier| earlier.name == entry.name),
                "{} names two languages",
                entry.name
            );
        }
    }

    #[test]
    fn the_names_are_in_order_so_the_list_can_be_read() {
        for pair in LANGUAGES.windows(2) {
            assert!(pair[0].name < pair[1].name, "{} comes after {}", pair[0].name, pair[1].name);
        }
    }

    #[test]
    fn every_tag_names_a_place_as_well_as_a_language() {
        for entry in LANGUAGES {
            assert!(entry.tag.contains('-'), "{} has no place in it", entry.tag);
        }
    }

    #[test]
    fn the_fallback_is_one_of_the_languages_offered() {
        assert!(LANGUAGES.iter().any(|entry| entry.tag == DEFAULT_TAG));
    }
}
