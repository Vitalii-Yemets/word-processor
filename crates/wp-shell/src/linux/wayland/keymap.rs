//! The keyboard, as the compositor describes it.
//!
//! # What arrives
//!
//! Not a table of keysyms, as X hands over, but a file of text in xkb's
//! own format: a list of names for the keys — `<AD01> = 24` — and a list of
//! what each name produces at each level — `key <AD01> { [ q, Q ] };`. Two
//! sections of the same file, and the rest of it is the compositor's
//! business rather than this program's.
//!
//! # Why it is read rather than asked about
//!
//! Because there is nobody to ask. On X the server answers "what does
//! keycode 24 mean"; on Wayland the compositor sends the description once
//! and every client works it out for itself. Programs normally hand the
//! file to `libxkbcommon`; this one reads the two sections it needs, which
//! is a few hundred lines against a library of tens of thousands — and one
//! more library this program would have to link.
//!
//! # What is read and what is not
//!
//! The names, the levels, and the groups a layout is switched between are
//! read. Compose sequences, dead keys and the rules a modifier can change
//! a level by are not: those are what an input method is for, and the
//! input method is named in the roadmap.

use std::collections::HashMap;

/// What a key produces, level by level: unshifted, shifted, and the two
/// the third-level modifier reaches.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Levels {
    pub(crate) keysyms: Vec<u32>,
}

/// The whole keyboard: which keysyms each keycode produces.
#[derive(Clone, Debug, Default)]
pub(crate) struct Keymap {
    keys: HashMap<u32, Levels>,
}

impl Keymap {
    /// Reads the file the compositor sent.
    #[must_use]
    pub(crate) fn parse(text: &str) -> Self {
        let names = keycodes_in(text);
        let mut keys = HashMap::new();
        for (name, levels) in symbols_in(text) {
            if let Some(code) = names.get(&name) {
                keys.insert(*code, levels);
            }
        }
        Self { keys }
    }

    /// Whether anything was understood, so that a keymap in a format this
    /// does not read can be told from one that is simply empty.
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// The keysym a key produces with the modifiers held.
    ///
    /// Shift takes the second level; the third-level modifier — AltGr —
    /// takes the third and fourth; Caps Lock turns letters and nothing
    /// else, which is what every keyboard does and what X's own rule says.
    #[must_use]
    pub(crate) fn keysym(&self, keycode: u32, shift: bool, caps: bool, level3: bool) -> u32 {
        let Some(levels) = self.keys.get(&keycode) else { return 0 };
        let at = |index: usize| levels.keysyms.get(index).copied().unwrap_or(0);
        let (base, shifted) =
            if level3 && levels.keysyms.len() > 2 { (at(2), at(3)) } else { (at(0), at(1)) };
        let base = if base == 0 { at(0) } else { base };
        let shifted = if shifted == 0 { upper(base) } else { shifted };
        let letter = super::super::keys::char_of(base).is_some_and(char::is_alphabetic);
        match (shift, caps && letter) {
            (false, false) => base,
            (true, false) | (false, true) => shifted,
            // Shift with Caps Lock gives the small letter back.
            (true, true) => base,
        }
    }
}

/// The capital of a keysym that is a small letter, which is the keysym of
/// the capital: Latin-1 and the ranges that follow its shape.
fn upper(keysym: u32) -> u32 {
    match keysym {
        0x61..=0x7A => keysym - 0x20,
        0xE0..=0xF6 | 0xF8..=0xFE => keysym - 0x20,
        // Cyrillic, which xkb numbers with the capitals below the smalls.
        0x6C0..=0x6DF => keysym - 0x20,
        _ => keysym,
    }
}

/// `<AD01> = 24`, for every key the keyboard has.
fn keycodes_in(text: &str) -> HashMap<String, u32> {
    let mut out = HashMap::new();
    let Some(section) = section_of(text, "xkb_keycodes") else { return out };
    for line in section.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix('<') else { continue };
        let Some((name, rest)) = rest.split_once('>') else { continue };
        let Some((_, value)) = rest.split_once('=') else { continue };
        let value = value.trim().trim_end_matches(';').trim();
        if let Ok(code) = value.parse::<u32>() {
            out.insert(name.to_owned(), code);
        }
    }
    out
}

/// `key <AD01> { [ q, Q ] };`, in whichever of the shapes xkb writes it.
fn symbols_in(text: &str) -> Vec<(String, Levels)> {
    let mut out = Vec::new();
    let Some(section) = section_of(text, "xkb_symbols") else { return out };
    let mut rest = section;
    while let Some(at) = rest.find("key <") {
        rest = &rest[at + 5..];
        let Some((name, after)) = rest.split_once('>') else { break };
        let Some(open) = after.find('{') else { break };
        let Some(close) = after[open..].find('}') else { break };
        let body = &after[open + 1..open + close];
        rest = &after[open + close..];
        let mut levels = Levels::default();
        if let Some(inside) = levels_in(body) {
            for name in inside.split(',') {
                levels.keysyms.push(keysym_named(name.trim()));
            }
        }
        if !levels.keysyms.is_empty() {
            out.push((name.to_owned(), levels));
        }
    }
    out
}

/// What is inside the brackets that hold the levels.
///
/// A key is written two ways. The short one is the brackets alone —
/// `key <AE01> { [ 1, exclam ] };` — and the long one says more about the
/// key first: `key <AD01> { type= "FOUR_LEVEL", symbols[Group1]= [ q, Q ]
/// }`, where the first brackets in the line are the group's name and not
/// the levels at all. So the brackets after an equals sign win, and the
/// first brackets are taken only where there is no equals sign.
fn levels_in(body: &str) -> Option<&str> {
    let after_equals =
        body.find("= [").map(|at| at + 2).or_else(|| body.find("=[").map(|at| at + 1));
    let start = match after_equals {
        Some(at) => at,
        None => body.find('[')?,
    };
    let end = body[start..].find(']')?;
    Some(&body[start + 1..start + end])
}

/// One `xkb_…` section of the file, between its braces.
fn section_of<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let at = text.find(name)?;
    let rest = &text[at..];
    let open = rest.find('{')?;
    let mut depth = 0i32;
    for (index, character) in rest[open..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&rest[open + 1..open + index]);
                }
            }
            _ => {}
        }
    }
    None
}

/// The number of a keysym written by name.
///
/// The names that matter are the letters and digits, which are their own
/// characters; the ones with names of their own — `Return`, `BackSpace` —
/// which are the keys a program reacts to; and `U0041` and `0x41`, which
/// are how a layout writes a character that has no name.
#[must_use]
pub(crate) fn keysym_named(name: &str) -> u32 {
    if name.is_empty() || name == "NoSymbol" || name == "VoidSymbol" {
        return 0;
    }
    // A single character is its own keysym in Latin-1, which is most of a
    // keyboard: q, Q, 1, comma.
    let mut characters = name.chars();
    if let (Some(character), None) = (characters.next(), characters.next()) {
        if (character as u32) < 0x100 {
            return character as u32;
        }
        return 0x0100_0000 + character as u32;
    }
    if let Some(hex) = name.strip_prefix("0x") {
        return u32::from_str_radix(hex, 16).unwrap_or(0);
    }
    if let Some(digits) = name.strip_prefix('U') {
        if let Ok(code) = u32::from_str_radix(digits, 16) {
            return if code < 0x100 { code } else { 0x0100_0000 + code };
        }
    }
    NAMED.iter().find(|(known, _)| *known == name).map_or(0, |(_, keysym)| *keysym)
}

/// The keys that have names rather than characters. The same numbers X
/// uses, so that [`super::super::keys`] can read either keyboard.
const NAMED: &[(&str, u32)] = &[
    ("BackSpace", 0xFF08),
    ("Tab", 0xFF09),
    ("Linefeed", 0xFF0A),
    ("Clear", 0xFF0B),
    ("Return", 0xFF0D),
    ("Pause", 0xFF13),
    ("Scroll_Lock", 0xFF14),
    ("Sys_Req", 0xFF15),
    ("Escape", 0xFF1B),
    ("Multi_key", 0xFF20),
    ("Home", 0xFF50),
    ("Left", 0xFF51),
    ("Up", 0xFF52),
    ("Right", 0xFF53),
    ("Down", 0xFF54),
    ("Prior", 0xFF55),
    ("Page_Up", 0xFF55),
    ("Next", 0xFF56),
    ("Page_Down", 0xFF56),
    ("End", 0xFF57),
    ("Begin", 0xFF58),
    ("Select", 0xFF60),
    ("Print", 0xFF61),
    ("Execute", 0xFF62),
    ("Insert", 0xFF63),
    ("Undo", 0xFF65),
    ("Redo", 0xFF66),
    ("Menu", 0xFF67),
    ("Find", 0xFF68),
    ("Cancel", 0xFF69),
    ("Help", 0xFF6A),
    ("Break", 0xFF6B),
    ("Mode_switch", 0xFF7E),
    ("ISO_Level3_Shift", 0xFE03),
    ("ISO_Left_Tab", 0xFE20),
    ("Num_Lock", 0xFF7F),
    ("KP_Space", 0xFF80),
    ("KP_Tab", 0xFF89),
    ("KP_Enter", 0xFF8D),
    ("KP_Home", 0xFF95),
    ("KP_Left", 0xFF96),
    ("KP_Up", 0xFF97),
    ("KP_Right", 0xFF98),
    ("KP_Down", 0xFF99),
    ("KP_Prior", 0xFF9A),
    ("KP_Next", 0xFF9B),
    ("KP_End", 0xFF9C),
    ("KP_Begin", 0xFF9D),
    ("KP_Insert", 0xFF9E),
    ("KP_Delete", 0xFF9F),
    ("KP_Multiply", 0xFFAA),
    ("KP_Add", 0xFFAB),
    ("KP_Separator", 0xFFAC),
    ("KP_Subtract", 0xFFAD),
    ("KP_Decimal", 0xFFAE),
    ("KP_Divide", 0xFFAF),
    ("KP_0", 0xFFB0),
    ("KP_1", 0xFFB1),
    ("KP_2", 0xFFB2),
    ("KP_3", 0xFFB3),
    ("KP_4", 0xFFB4),
    ("KP_5", 0xFFB5),
    ("KP_6", 0xFFB6),
    ("KP_7", 0xFFB7),
    ("KP_8", 0xFFB8),
    ("KP_9", 0xFFB9),
    ("F1", 0xFFBE),
    ("F2", 0xFFBF),
    ("F3", 0xFFC0),
    ("F4", 0xFFC1),
    ("F5", 0xFFC2),
    ("F6", 0xFFC3),
    ("F7", 0xFFC4),
    ("F8", 0xFFC5),
    ("F9", 0xFFC6),
    ("F10", 0xFFC7),
    ("F11", 0xFFC8),
    ("F12", 0xFFC9),
    ("Shift_L", 0xFFE1),
    ("Shift_R", 0xFFE2),
    ("Control_L", 0xFFE3),
    ("Control_R", 0xFFE4),
    ("Caps_Lock", 0xFFE5),
    ("Shift_Lock", 0xFFE6),
    ("Meta_L", 0xFFE7),
    ("Meta_R", 0xFFE8),
    ("Alt_L", 0xFFE9),
    ("Alt_R", 0xFFEA),
    ("Super_L", 0xFFEB),
    ("Super_R", 0xFFEC),
    ("Hyper_L", 0xFFED),
    ("Hyper_R", 0xFFEE),
    ("Delete", 0xFFFF),
    ("space", 0x20),
    ("exclam", 0x21),
    ("quotedbl", 0x22),
    ("numbersign", 0x23),
    ("dollar", 0x24),
    ("percent", 0x25),
    ("ampersand", 0x26),
    ("apostrophe", 0x27),
    ("quoteright", 0x27),
    ("parenleft", 0x28),
    ("parenright", 0x29),
    ("asterisk", 0x2A),
    ("plus", 0x2B),
    ("comma", 0x2C),
    ("minus", 0x2D),
    ("period", 0x2E),
    ("slash", 0x2F),
    ("colon", 0x3A),
    ("semicolon", 0x3B),
    ("less", 0x3C),
    ("equal", 0x3D),
    ("greater", 0x3E),
    ("question", 0x3F),
    ("at", 0x40),
    ("bracketleft", 0x5B),
    ("backslash", 0x5C),
    ("bracketright", 0x5D),
    ("asciicircum", 0x5E),
    ("underscore", 0x5F),
    ("grave", 0x60),
    ("quoteleft", 0x60),
    ("braceleft", 0x7B),
    ("bar", 0x7C),
    ("braceright", 0x7D),
    ("asciitilde", 0x7E),
    ("nobreakspace", 0xA0),
    ("sterling", 0xA3),
    ("section", 0xA7),
    ("degree", 0xB0),
    ("periodcentered", 0xB7),
    ("adiaeresis", 0xE4),
    ("odiaeresis", 0xF6),
    ("udiaeresis", 0xFC),
    ("ssharp", 0xDF),
    ("Adiaeresis", 0xC4),
    ("Odiaeresis", 0xD6),
    ("Udiaeresis", 0xDC),
];

#[cfg(test)]
mod tests {
    use super::*;

    /// A keymap of the shape a compositor sends, cut down to three keys.
    const KEYMAP: &str = r#"
xkb_keymap {
xkb_keycodes "(unnamed)" {
    minimum = 8;
    maximum = 708;
    <TLDE> = 49;
    <AE01> = 10;
    <AD01> = 24;
    <RTRN> = 36;
    indicator 1 = "Caps Lock";
};
xkb_types "(unnamed)" {
    virtual_modifiers NumLock,Alt,LevelThree;
    type "ONE_LEVEL" {
        modifiers= none;
        level_name[Level1]= "Any";
    };
};
xkb_symbols "(unnamed)" {
    name[group1]="English (UK)";
    key <AE01> {         [               1,          exclam ] };
    key <AD01> {
        type= "FOUR_LEVEL",
        symbols[Group1]= [ q, Q, at, Greek_OMEGA ]
    };
    key <RTRN> {         [          Return ] };
};
};
"#;

    #[test]
    fn the_names_and_the_levels_come_out_of_the_file() {
        let keymap = Keymap::parse(KEYMAP);
        assert!(!keymap.is_empty());
        // 24 is the q key on every PC keyboard.
        assert_eq!(keymap.keysym(24, false, false, false), 0x71, "q");
        assert_eq!(keymap.keysym(24, true, false, false), 0x51, "Q");
        assert_eq!(keymap.keysym(24, false, true, false), 0x51, "caps lock gives the capital");
        assert_eq!(keymap.keysym(24, true, true, false), 0x71, "and with shift, the small letter");
        assert_eq!(keymap.keysym(24, false, false, true), 0x40, "the third level is @");
        assert_eq!(keymap.keysym(10, false, false, false), 0x31, "1");
        assert_eq!(keymap.keysym(10, true, false, false), 0x21, "!");
        assert_eq!(keymap.keysym(10, false, true, false), 0x31, "caps lock does not turn a digit");
        assert_eq!(keymap.keysym(36, false, false, false), 0xFF0D, "Return");
        assert_eq!(keymap.keysym(99, false, false, false), 0, "a key that is not on it");
    }

    #[test]
    fn a_keysym_is_named_by_its_character_its_name_or_its_number() {
        assert_eq!(keysym_named("q"), 0x71);
        assert_eq!(keysym_named("Return"), 0xFF0D);
        assert_eq!(keysym_named("0x1001234"), 0x0100_1234);
        assert_eq!(keysym_named("U20AC"), 0x0100_20AC, "the euro sign");
        assert_eq!(keysym_named("NoSymbol"), 0);
        assert_eq!(keysym_named("something nobody has heard of"), 0);
    }

    #[test]
    fn a_file_that_is_not_a_keymap_gives_an_empty_one() {
        assert!(Keymap::parse("").is_empty());
        assert!(Keymap::parse("this is not a keymap at all").is_empty());
    }
}
