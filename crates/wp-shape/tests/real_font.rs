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

/// The first font on this machine that says what to do when a mark lands on a
/// letter — the composing rules, which every shaper applies before anything
/// else.
fn composing_font() -> Option<(Vec<u8>, String)> {
    for path in font_files() {
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let Ok(font) = Font::parse(&bytes) else { continue };
        if font.glyph_for('i').is_none() || font.glyph_for('\u{0307}').is_none() {
            continue;
        }
        let Some(table) = font.substitution_table().and_then(Substitutions::parse) else {
            continue;
        };
        if table.lookups_for(b"latn", b"ccmp").is_empty() {
            continue;
        }
        let name = font.full_name().unwrap_or_else(|| path.display().to_string());
        drop(font);
        return Some((bytes, name));
    }
    None
}

#[test]
fn a_letter_loses_its_dot_when_a_mark_lands_on_it() {
    // The rule is written as a context — this glyph, but only with that after
    // it — and a shaper that cannot read one draws a dotted i with a second
    // dot on top of it. The font's own answer is the dotless form.
    let Some((bytes, name)) = composing_font() else {
        eprintln!("no font with composing rules on this machine; skipping");
        return;
    };
    let font = Font::parse(&bytes).expect("a readable font");

    let alone = shape(&font, "i");
    let marked = shape(&font, "i\u{0307}");
    assert_eq!(alone.len(), 1);
    assert_eq!(marked.len(), 2, "the mark should still be a glyph of its own");
    assert_ne!(
        alone[0].glyph, marked[0].glyph,
        "{name}: the i kept its dot under a mark, so the two are drawn on top of each other"
    );
}

#[test]
fn a_letter_with_nothing_on_it_is_left_as_it_is() {
    let Some((bytes, _)) = composing_font() else { return };
    let font = Font::parse(&bytes).expect("a readable font");

    // The same rule, not fired: what follows is a letter and not a mark.
    let plain = shape(&font, "in");
    let alone = shape(&font, "i");
    assert_eq!(plain[0].glyph, alone[0].glyph, "a rule about marks changed a letter");
}

/// The first font on this machine that kerns through its positioning table.
fn kerning_font() -> Option<(Vec<u8>, String)> {
    for path in font_files() {
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let Ok(font) = Font::parse(&bytes) else { continue };
        let has_letters = font.glyph_for('A').is_some() && font.glyph_for('V').is_some();
        let kerns = font
            .positioning_table()
            .and_then(wp_shape::Positions::parse)
            .is_some_and(|table| !table.lookups_for(b"latn", b"kern").is_empty());
        if !has_letters || !kerns {
            continue;
        }
        let name = font.full_name().unwrap_or_else(|| path.display().to_string());
        drop(font);
        return Some((bytes, name));
    }
    None
}

#[test]
fn a_pair_the_font_kerns_is_set_closer_than_its_widths() {
    let Some((bytes, name)) = kerning_font() else {
        eprintln!("no font kerning through its positioning table here; skipping");
        return;
    };
    let font = Font::parse(&bytes).expect("a readable font");

    // A and V lean away from each other, so every font that kerns at all kerns
    // this pair, and kerns it negative.
    let shaped = shape(&font, "AV");
    assert_eq!(shaped.len(), 2);
    assert!(shaped[0].x_advance < 0, "{name} set AV at its full widths: {}", shaped[0].x_advance);

    // And a pair it says nothing about is left at its widths.
    let plain = shape(&font, "AH");
    assert_eq!(plain[0].x_advance, 0, "{name} kerned a pair it has no rule for");
}

#[test]
fn the_old_kern_table_is_not_counted_twice() {
    let Some((bytes, name)) = kerning_font() else { return };
    let font = Font::parse(&bytes).expect("a readable font");

    let (a, v) = (font.glyph_for('A').expect("A"), font.glyph_for('V').expect("V"));
    let through_shaping = shape(&font, "AV")[0].x_advance;
    let asked_directly = wp_shape::kerning_between(&font, b"latn", a, v);
    assert_eq!(
        through_shaping, asked_directly,
        "{name}: shaping and asking gave different answers"
    );
    // The old table is the fallback and not an addition: whatever it says, the
    // answer is the new table's.
    assert!(asked_directly < 0);
}

#[test]
fn a_mark_is_drawn_over_its_letter_and_not_beside_it() {
    let Some((bytes, name)) = composing_font() else { return };
    let font = Font::parse(&bytes).expect("a readable font");
    let Some(table) = font.positioning_table().and_then(wp_shape::Positions::parse) else {
        eprintln!("{name} has no positioning table; skipping");
        return;
    };
    if table.lookups_for(b"latn", b"mark").is_empty() {
        eprintln!("{name} says nothing about where marks go; skipping");
        return;
    }

    // The letter is drawn first and the mark after it, with the pen already
    // past the letter. Where the mark belongs is back over the letter, so what
    // the font says has to be negative by about the letter's own width.
    let shaped = shape(&font, "e\u{0301}");
    assert_eq!(shaped.len(), 2);
    let letter = i32::from(font.advance(shaped[0].glyph));
    let mark = shaped[1];
    assert!(
        mark.x_offset < 0,
        "{name}: the accent was left at the edge of the letter ({} units)",
        mark.x_offset
    );

    // And it lands over the letter rather than past either end of it. Where
    // the ink of a mark sits inside its own glyph is the font's business — a
    // mark is often drawn to the left of its own origin — so the check is on
    // the ink and not on the origin.
    let ink = |glyph| {
        font.outline(glyph)
            .ok()
            .flatten()
            .map(|outline| (i32::from(outline.bounds.min_x), i32::from(outline.bounds.max_x)))
    };
    let (Some((letter_left, letter_right)), Some((mark_left, mark_right))) =
        (ink(shaped[0].glyph), ink(mark.glyph))
    else {
        return;
    };
    let drawn = (letter + mark.x_offset + mark_left, letter + mark.x_offset + mark_right);
    assert!(
        drawn.0 >= letter_left - 50 && drawn.1 <= letter_right + 50,
        "{name}: the accent is drawn at {drawn:?}, the letter at {:?}",
        (letter_left, letter_right)
    );
}

/// Whatever font this machine has: for the tests below, which are about the
/// order glyphs come out in rather than about which glyphs they are.
fn any_font() -> Option<Vec<u8>> {
    for path in font_files() {
        let Ok(bytes) = std::fs::read(&path) else { continue };
        if Font::parse(&bytes).is_ok() {
            return Some(bytes);
        }
    }
    None
}

#[test]
fn devanagari_is_drawn_in_the_order_it_is_read() {
    // कि is stored consonant-then-sign and drawn sign-then-consonant. Which
    // glyphs a font has for them is beside the point here: what is being
    // tested is that the pieces come out in the order they are drawn, and
    // every glyph says which character it came from.
    let Some(bytes) = any_font() else { return };
    let font = Font::parse(&bytes).expect("a readable font");

    let shaped = shape(&font, "\u{0915}\u{093F}");
    let clusters: Vec<usize> = shaped.iter().map(|entry| entry.cluster).collect();
    assert_eq!(clusters, vec![3, 0], "the vowel sign was left after the consonant");
}

#[test]
fn the_hook_of_a_cluster_is_drawn_at_the_end_of_it_when_the_font_can_make_one() {
    // र्क: the r is stored first and drawn last — by a font that has a hook
    // to draw. A font with no rule for the hook draws the r as the letter it
    // is, at the front, which is what every shaper does with such a font.
    let Some(bytes) = any_font() else { return };
    let font = Font::parse(&bytes).expect("a readable font");
    let has_hook = font.substitution_table().and_then(Substitutions::parse).is_some_and(|table| {
        wp_shape::indic::TAGS.iter().any(|tag| !table.lookups_for(tag, b"rphf").is_empty())
    });

    let shaped = shape(&font, "\u{0930}\u{094D}\u{0915}");
    let clusters: Vec<usize> = shaped.iter().map(|entry| entry.cluster).collect();
    if has_hook {
        assert_eq!(clusters.first(), Some(&6), "the hook was left at the front");
    } else {
        assert_eq!(clusters, vec![0, 3, 6], "a font with no hook moved the r");
    }
}

#[test]
fn devanagari_that_needs_no_moving_is_not_moved() {
    let Some(bytes) = any_font() else { return };
    let font = Font::parse(&bytes).expect("a readable font");

    // का: consonant then a sign written to the right of it.
    let shaped = shape(&font, "\u{0915}\u{093E}");
    let clusters: Vec<usize> = shaped.iter().map(|entry| entry.cluster).collect();
    assert_eq!(clusters, vec![0, 3]);
}

/// The first font on this machine that can both draw Devanagari and shape it.
fn devanagari_font() -> Option<(Vec<u8>, String)> {
    for path in font_files() {
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let Ok(font) = Font::parse(&bytes) else { continue };
        if font.glyph_for('\u{0915}').is_none() {
            continue;
        }
        let known = font
            .substitution_table()
            .and_then(Substitutions::parse)
            .is_some_and(|table| wp_shape::indic::TAGS.iter().any(|tag| table.has_script(tag)));
        if !known {
            continue;
        }
        let name = font.full_name().unwrap_or_else(|| path.display().to_string());
        drop(font);
        return Some((bytes, name));
    }
    None
}

#[test]
fn a_font_with_devanagari_rules_draws_a_cluster_as_fewer_glyphs_than_it_has_letters() {
    // क्क is two consonants joined by a halant, and a font that knows the
    // script draws it as one conjunct or as a half form and a letter — in
    // either case as fewer glyphs than the three characters written.
    let Some((bytes, name)) = devanagari_font() else {
        eprintln!("no font with Devanagari rules on this machine; skipping");
        return;
    };
    let font = Font::parse(&bytes).expect("a readable font");

    let shaped = shape(&font, "\u{0915}\u{094D}\u{0915}");
    assert!(
        shaped.len() < 3,
        "{name} drew a joined cluster as {} glyphs, one for each character",
        shaped.len()
    );
}

#[test]
fn a_font_with_devanagari_rules_makes_the_hook_one_glyph() {
    let Some((bytes, name)) = devanagari_font() else { return };
    let font = Font::parse(&bytes).expect("a readable font");

    // र्क: the r and the halant become the hook, so three characters come out
    // as two glyphs, and the hook is drawn after the consonant.
    let shaped = shape(&font, "\u{0930}\u{094D}\u{0915}");
    assert_eq!(shaped.len(), 2, "{name} did not make the hook one glyph");
    assert_eq!(shaped[0].cluster, 6, "{name} drew the hook first");
}

/// The first font on this machine that can draw Thai and says where its marks
/// go.
fn thai_font() -> Option<(Vec<u8>, String)> {
    for path in font_files() {
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let Ok(font) = Font::parse(&bytes) else { continue };
        if font.glyph_for('\u{0E01}').is_none() || font.glyph_for('\u{0E48}').is_none() {
            continue;
        }
        let places = font
            .positioning_table()
            .and_then(wp_shape::Positions::parse)
            .is_some_and(|table| !table.lookups_for(b"thai", b"mark").is_empty());
        if !places {
            continue;
        }
        let name = font.full_name().unwrap_or_else(|| path.display().to_string());
        drop(font);
        return Some((bytes, name));
    }
    None
}

#[test]
fn a_thai_vowel_is_moved_off_a_consonant_that_reaches_up_into_it() {
    // Thai stacks: a consonant, a vowel above it, and a tone mark above that.
    // A Thai font draws its marks so that they land right where the pen
    // leaves them for most consonants — and says, for the few that reach up
    // into the place the vowel wants, where the vowel goes instead. ป is one
    // of those: it has an ascender, and the vowel has to move aside.
    let Some((bytes, name)) = thai_font() else {
        eprintln!("no font with Thai rules on this machine; skipping");
        return;
    };
    let font = Font::parse(&bytes).expect("a readable font");

    let plain = shape(&font, "\u{0E01}\u{0E34}\u{0E48}");
    let tall = shape(&font, "\u{0E1B}\u{0E34}\u{0E48}");
    assert_eq!(tall.len(), 3, "{name}: a letter and two marks");
    assert!(
        tall[1].x_offset != plain[1].x_offset,
        "{name}: the vowel sits in the same place over a letter that reaches into it"
    );
}

#[test]
fn a_thai_tone_mark_follows_the_vowel_it_stands_on() {
    // The tone mark is the third storey: it sits on the vowel, not on the
    // consonant, so wherever the vowel went the tone mark goes too. Without
    // the mark-on-mark rules it would be left over the consonant with the
    // vowel beside it.
    let Some((bytes, name)) = thai_font() else { return };
    let font = Font::parse(&bytes).expect("a readable font");

    let plain = shape(&font, "\u{0E01}\u{0E34}\u{0E48}");
    let tall = shape(&font, "\u{0E1B}\u{0E34}\u{0E48}");

    // It moved with the vowel rather than staying where it would have been
    // over an ordinary consonant.
    assert_ne!(
        tall[2].x_offset, plain[2].x_offset,
        "{name}: the tone mark was left behind by the vowel it stands on"
    );
    // And it stayed with it: a tone mark a quarter of an em away from the
    // vowel under it is a tone mark over the wrong part of the letter.
    let apart = (tall[2].x_offset - tall[1].x_offset).abs();
    assert!(
        apart * 4 < i32::from(font.units_per_em()),
        "{name}: the tone mark ended {apart} units from the vowel it stands on"
    );
}

/// The first font on this machine that can draw a script written on
/// Devanagari's plan and knows its rules.
fn indic_font(script: wp_shape::indic::Script, sample: char) -> Option<(Vec<u8>, String)> {
    for path in font_files() {
        let Ok(bytes) = std::fs::read(&path) else { continue };
        let Ok(font) = Font::parse(&bytes) else { continue };
        if font.glyph_for(sample).is_none() {
            continue;
        }
        let known = font
            .substitution_table()
            .and_then(Substitutions::parse)
            .is_some_and(|table| script.tags().iter().any(|tag| table.has_script(tag)));
        if !known {
            continue;
        }
        let name = font.full_name().unwrap_or_else(|| path.display().to_string());
        drop(font);
        return Some((bytes, name));
    }
    None
}

#[test]
fn a_vowel_written_in_two_parts_is_drawn_round_the_consonant_in_every_script_that_has_one() {
    use wp_shape::indic::Script;
    // Bengali কো, Oriya କୋ, Tamil கொ, Kannada ಕೊ, Malayalam കോ and Sinhala
    // කේ: the consonant is stored first and the sign after it, and the sign
    // is drawn in two parts, the first of them before the consonant.
    for (script, text) in [
        (Script::Bengali, "\u{0995}\u{09CB}"),
        (Script::Oriya, "\u{0B15}\u{0B4B}"),
        (Script::Tamil, "\u{0B95}\u{0BCA}"),
        (Script::Kannada, "\u{0C95}\u{0CCA}"),
        (Script::Malayalam, "\u{0D15}\u{0D4B}"),
        (Script::Sinhala, "\u{0D9A}\u{0DDA}"),
    ] {
        let sample = text.chars().next().unwrap();
        let Some((bytes, name)) = indic_font(script, sample) else {
            eprintln!("no font with {script:?} rules on this machine; skipping");
            continue;
        };
        let font = Font::parse(&bytes).expect("a readable font");
        let shaped = shape(&font, text);
        assert!(shaped.len() >= 2, "{name}: {script:?} drew {} glyphs", shaped.len());
        // Kannada's two parts both sit to the right; every other script's
        // first part is drawn before the consonant.
        if script != Script::Kannada {
            assert_eq!(shaped[0].cluster, 3, "{name}: {script:?} did not draw the sign first");
            assert!(
                shaped[1..].iter().any(|glyph| glyph.cluster == 0),
                "{name}: {script:?} lost the consonant"
            );
        }
        // Nothing has a glyph of nought: the font drew every part.
        assert!(shaped.iter().all(|glyph| glyph.glyph.0 != 0), "{name}: {script:?} drew a notdef");
    }
}

#[test]
fn a_font_with_the_script_s_rules_draws_a_cluster_as_fewer_glyphs_than_it_has_letters() {
    use wp_shape::indic::Script;
    // Two consonants joined by a halant: a half form and a letter, a letter
    // with a small one beneath it, or one conjunct — in every case fewer
    // glyphs than the three characters written.
    for (script, text) in [
        (Script::Bengali, "\u{0995}\u{09CD}\u{0995}"),
        (Script::Gurmukhi, "\u{0A15}\u{0A4D}\u{0A35}"),
        (Script::Gujarati, "\u{0A95}\u{0ACD}\u{0A95}"),
        (Script::Oriya, "\u{0B15}\u{0B4D}\u{0B15}"),
        (Script::Telugu, "\u{0C15}\u{0C4D}\u{0C15}"),
        (Script::Kannada, "\u{0C95}\u{0CCD}\u{0C95}"),
        (Script::Malayalam, "\u{0D15}\u{0D4D}\u{0D15}"),
    ] {
        let sample = text.chars().next().unwrap();
        let Some((bytes, name)) = indic_font(script, sample) else {
            eprintln!("no font with {script:?} rules on this machine; skipping");
            continue;
        };
        let font = Font::parse(&bytes).expect("a readable font");
        let shaped = shape(&font, text);
        assert!(
            shaped.len() < 3,
            "{name}: {script:?} drew a joined cluster as {} glyphs, one for each character",
            shaped.len()
        );
    }
}

#[test]
fn the_hook_is_one_glyph_drawn_after_the_letter_where_the_script_has_one() {
    use wp_shape::indic::Script;
    // Bengali র্ক and Gujarati ર્ક: the r and the halant become the hook, so
    // three characters come out as two glyphs, and the hook comes after.
    for (script, text) in [
        (Script::Bengali, "\u{09B0}\u{09CD}\u{0995}"),
        (Script::Gujarati, "\u{0AB0}\u{0ACD}\u{0A95}"),
    ] {
        let sample = text.chars().next().unwrap();
        let Some((bytes, name)) = indic_font(script, sample) else {
            eprintln!("no font with {script:?} rules on this machine; skipping");
            continue;
        };
        let font = Font::parse(&bytes).expect("a readable font");
        let shaped = shape(&font, text);
        assert_eq!(shaped.len(), 2, "{name}: {script:?} did not make the hook one glyph");
        assert_eq!(shaped[0].cluster, 6, "{name}: {script:?} drew the hook first");
    }
}

#[test]
fn a_tamil_font_without_a_hook_draws_the_ra_as_the_letter_it_is() {
    use wp_shape::indic::Script;
    // Tamil ர்க: no Tamil font makes a hook of ra and the pulli, so the ra
    // stays a letter at the front, with its pulli over it.
    let text = "\u{0BB0}\u{0BCD}\u{0B95}";
    let Some((bytes, name)) = indic_font(Script::Tamil, '\u{0B95}') else {
        eprintln!("no font with Tamil rules on this machine; skipping");
        return;
    };
    let font = Font::parse(&bytes).expect("a readable font");
    let shaped = shape(&font, text);
    assert_eq!(shaped[0].cluster, 0, "{name}: the ra was moved");
    assert_eq!(shaped.last().map(|glyph| glyph.cluster), Some(6), "{name}: the ka is not last");
}
