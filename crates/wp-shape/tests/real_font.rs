//! Shaping against a real font.
//!
//! A shaper tested only against a table this project also built proves nothing:
//! the whole question is whether it reads what font designers actually ship.
//! These run against whatever fonts the machine has, and skip themselves when a
//! suitable one is not installed — which is the honest thing for a test that
//! depends on the machine rather than on the repository.

use std::path::{Path, PathBuf};

use wp_font::Font;
use wp_shape::{script_of, shape, Substitutions};

/// Where fonts live, on each of the systems this is built for.
fn font_directories() -> Vec<PathBuf> {
    ["/usr/share/fonts", "/usr/local/share/fonts", "C:/Windows/Fonts"]
        .iter()
        .map(PathBuf::from)
        .filter(|path| path.exists())
        .collect()
}

/// Every font file on the machine, without reading any of them.
fn font_files() -> Vec<PathBuf> {
    fn walk(directory: &Path, out: &mut Vec<PathBuf>, depth: usize) {
        if depth > 4 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(directory) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out, depth + 1);
            } else if path.extension().and_then(|extension| extension.to_str()).is_some_and(
                |extension| matches!(extension.to_ascii_lowercase().as_str(), "ttf" | "otf"),
            ) {
                out.push(path);
            }
        }
    }

    let mut out = Vec::new();
    for directory in font_directories() {
        walk(&directory, &mut out, 0);
    }
    out
}

/// The first font on this machine that carries Arabic substitution rules.
fn arabic_font() -> Option<(Vec<u8>, String)> {
    for path in font_files() {
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let Ok(font) = Font::parse(&bytes) else { continue };

        // It has to have the letters and the rules for joining them.
        if font.glyph_for('\u{0628}').is_none() {
            continue;
        }
        let Some(table) = font.substitution_table().and_then(Substitutions::parse) else {
            continue;
        };
        if table.lookups_for(b"arab", b"init").is_empty() {
            continue;
        }

        let name = font.full_name().unwrap_or_else(|| path.display().to_string());
        drop(font);
        return Some((bytes, name));
    }
    None
}

#[test]
fn a_font_with_arabic_rules_gives_different_glyphs_for_different_forms() {
    let Some((bytes, name)) = arabic_font() else {
        eprintln!("no font with Arabic substitution rules on this machine; skipping");
        return;
    };
    let font = Font::parse(&bytes).expect("a readable font");

    // Beh three times: initial, medial, final. All three are the same
    // character, and a shaper that ignored the joining would give one glyph
    // three times.
    let shaped = shape(&font, "\u{0628}\u{0628}\u{0628}");
    assert_eq!(shaped.len(), 3, "one glyph per letter");

    let glyphs: Vec<u16> = shaped.iter().map(|entry| entry.glyph.0).collect();
    assert!(
        glyphs[0] != glyphs[1] || glyphs[1] != glyphs[2],
        "{name} shaped three joined forms as the same glyph: {glyphs:?}"
    );
}

#[test]
fn the_isolated_form_differs_from_the_joined_one() {
    let Some((bytes, name)) = arabic_font() else { return };
    let font = Font::parse(&bytes).expect("a readable font");

    let alone = shape(&font, "\u{0628}");
    let joined = shape(&font, "\u{0628}\u{0628}");

    assert_eq!(alone.len(), 1);
    assert_ne!(
        alone[0].glyph, joined[0].glyph,
        "{name}: a letter on its own should not look like one that opens a join"
    );
}

#[test]
fn every_glyph_says_which_character_it_came_from() {
    let Some((bytes, _)) = arabic_font() else { return };
    let font = Font::parse(&bytes).expect("a readable font");

    // Two-byte characters, so the offsets step by two.
    let shaped = shape(&font, "\u{0628}\u{0633}\u{0645}");
    let clusters: Vec<usize> = shaped.iter().map(|entry| entry.cluster).collect();
    assert_eq!(clusters, vec![0, 2, 4]);
}

#[test]
fn plain_text_comes_through_a_glyph_at_a_time() {
    let Some((bytes, _)) = arabic_font() else { return };
    let font = Font::parse(&bytes).expect("a readable font");

    let shaped = shape(&font, "abc");
    assert_eq!(shaped.len(), 3, "nothing should be joined or dropped");
    for (index, entry) in shaped.iter().enumerate() {
        assert_eq!(entry.cluster, index);
    }
    assert_eq!(script_of("abc"), *b"latn");
}

#[test]
fn shaping_nothing_produces_nothing() {
    let Some((bytes, _)) = arabic_font() else { return };
    let font = Font::parse(&bytes).expect("a readable font");
    assert!(shape(&font, "").is_empty());
}

#[test]
fn every_font_on_this_machine_is_read_without_panicking() {
    // Font files are data from outside, and a great many of them are slightly
    // wrong. Reading one must never bring the program down.
    let mut read = 0usize;
    for path in font_files().into_iter().take(200) {
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let Ok(font) = Font::parse(&bytes) else { continue };
        read += 1;

        if let Some(table) = font.substitution_table().and_then(Substitutions::parse) {
            for feature in [b"init", b"medi", b"fina", b"isol", b"liga"] {
                for lookup in table.lookups_for(b"arab", feature) {
                    let mut glyphs = vec![wp_font::GlyphId(1), wp_font::GlyphId(2)];
                    let mut clusters = vec![0, 1];
                    table.apply(lookup, &mut glyphs, &mut clusters);
                }
            }
        }
        let _ = shape(&font, "\u{0628}\u{0633}\u{0645} abc");
    }

    assert!(read > 0, "no fonts could be read on this machine at all");
}
