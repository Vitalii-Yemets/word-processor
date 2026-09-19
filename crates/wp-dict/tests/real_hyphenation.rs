//! Tests against the hyphenation patterns somebody else published.
//!
//! A handful of patterns written for the test prove the algorithm; only a
//! language's own thousands prove that the file is read as it is written —
//! its encoding, its comments, its levels — and that what comes out is where
//! the language breaks its words. The files for English and German are
//! installed in the build container for this.
//!
//! What each word should come out as was asked of `libhyphen` 2.8.8, the
//! library those files were written for and the one LibreOffice hyphenates
//! with, given the same files. Where its answer and TeX's differ, its answer
//! is the one held to: the files are its.

use std::path::Path;

const ENGLISH: &str = "/usr/share/hyphen/hyph_en_US.dic";
const GERMAN: &str = "/usr/share/hyphen/hyph_de_DE.dic";

fn read(path: &str) -> wp_dict::hyphenation::Patterns {
    wp_dict::hyphenation::read_file(Path::new(path)).unwrap_or_else(|error| {
        panic!(
            "cannot read {path}: {error}\n\
             the build image should install hyphen-en-us and hyphen-de"
        )
    })
}

#[test]
fn english_words_break_where_the_library_breaks_them() {
    let patterns = read(ENGLISH);
    assert_eq!(patterns.left_min, 2);
    assert_eq!(patterns.right_min, 3);
    for (word, broken) in [
        ("hyphenation", "hy-phen-ation"),
        ("computer", "com-puter"),
        ("processor", "pro-ces-sor"),
        ("typography", "ty-pog-ra-phy"),
        ("international", "in-ter-na-tional"),
        ("document", "doc-u-ment"),
        ("associate", "as-so-ciate"),
        ("paragraph", "para-graph"),
        ("algorithm", "al-go-rithm"),
        ("information", "in-for-ma-tion"),
        ("beautiful", "beau-ti-ful"),
        ("university", "uni-ver-sity"),
        ("extraordinary", "ex-tra-or-di-nary"),
        ("photograph", "pho-to-graph"),
        ("telephone", "tele-phone"),
        ("development", "de-vel-op-ment"),
        ("knowledge", "knowl-edge"),
        ("government", "gov-ern-ment"),
        ("understanding", "un-der-stand-ing"),
        ("responsibility", "re-spon-si-bil-ity"),
    ] {
        assert_eq!(patterns.marked(word), broken, "{word}");
    }
    // A capital makes no difference to where, and the offsets are the
    // word's own.
    assert_eq!(patterns.marked("Hyphenation"), "Hy-phen-ation");
    // Short words are left alone, and so are the ends of long ones.
    assert!(patterns.breaks("the").is_empty());
    assert!(patterns.breaks("word").is_empty());
}

#[test]
fn german_words_break_where_the_library_breaks_them_across_both_levels() {
    let patterns = read(GERMAN);
    assert_eq!(patterns.compound_left_min, Some(2));
    for (word, broken) in [
        ("zusammenarbeit", "zu-sam-men-ar-beit"),
        ("textverarbeitung", "text-ver-ar-bei-tung"),
        ("dampfschiff", "dampf-schiff"),
        ("silbentrennung", "sil-ben-tren-nung"),
        ("bundesrepublik", "bun-des-re-pu-blik"),
        ("geschwindigkeit", "ge-schwin-dig-keit"),
        ("wissenschaft", "wis-sen-schaft"),
        ("sonnenblume", "son-nen-blu-me"),
        ("freundschaft", "freund-schaft"),
        ("arbeitsplatz", "ar-beits-platz"),
        ("donaudampfschifffahrt", "do-nau-dampf-schiff-fahrt"),
        ("kindergarten", "kin-der-gar-ten"),
        ("verantwortung", "ver-ant-wor-tung"),
    ] {
        assert_eq!(patterns.marked(word), broken, "{word}");
    }
}

#[test]
fn the_patterns_of_the_machine_are_found_by_language() {
    let installed = wp_dict::hyphenation::installed();
    assert!(installed.iter().any(|(name, _)| name == "en_US"), "{installed:?}");
    let by_tag = wp_dict::hyphenation::path_for_language("en-US").expect("en-US");
    assert!(by_tag.ends_with("hyph_en_US.dic"));
    // A region the machine lacks falls back to the language.
    assert!(wp_dict::hyphenation::path_for_language("en-ZZ").is_some());
    assert!(wp_dict::hyphenation::for_language("de-DE").is_some());
    assert!(wp_dict::hyphenation::path_for_language("zz").is_none());
}
