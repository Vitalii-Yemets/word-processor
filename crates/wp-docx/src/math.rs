//! Equations: what they are made of, how they are typed, and how they are
//! written into the file.
//!
//! # Why the equation is stored as an equation
//!
//! Word writes equations in a namespace of its own from the same standard —
//! `m:` — and rebuilds the layout from the structure. A fraction is a numerator
//! element and a denominator element, not a line with a rule drawn across it.
//! Writing it that way is what makes an equation stay an equation when it is
//! opened somewhere else, resized, or set in another font.
//!
//! # Why it is typed as one line
//!
//! Word's equation editor has a ribbon tab of symbols and a grid of templates,
//! and it also has a linear format — `a/b`, `x^2`, `sqrt(x)` — that turns into
//! the built-up form as it is typed. The linear format is the whole of the
//! editor that a person can use without a mouse, and it is what is here.
//!
//! # What this understands
//!
//! Fractions, powers, indices, square roots, brackets, and the Greek letters by
//! name. Not matrices, integrals, sums with limits, or the several hundred
//! other things Word's gallery offers: each of those is its own element with
//! its own layout, and a half-drawn integral is worse than none.

use wp_xml::tree::Element;

use crate::Document;

/// The namespace equations are written in. Part of the standard, so nothing
/// has to be marked ignorable for it.
pub const MATH_NAMESPACE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";
/// The prefix a document written here binds it to.
pub const MATH_PREFIX: &str = "m";

/// A piece of an equation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Math {
    /// Letters, digits or an operator, written as they are.
    Text(String),
    /// One piece after another.
    Row(Vec<Math>),
    /// A numerator over a denominator.
    Fraction(Box<Math>, Box<Math>),
    /// Something with a power on it.
    Superscript(Box<Math>, Box<Math>),
    /// And with an index under it.
    Subscript(Box<Math>, Box<Math>),
    /// A square root.
    Radical(Box<Math>),
    /// Something inside brackets.
    Delimited(Box<Math>),
}

impl Default for Math {
    fn default() -> Self {
        Self::Row(Vec::new())
    }
}

impl Math {
    /// The equation as a line of text, for searching and for reading aloud.
    ///
    /// The linear form it was typed in, near enough: a fraction comes back as
    /// `(a)/(b)` rather than as two lines, because a line of text has only one
    /// line to give.
    #[must_use]
    pub fn plain_text(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Row(pieces) => pieces.iter().map(Self::plain_text).collect(),
            Self::Fraction(top, bottom) => {
                format!("({})/({})", top.plain_text(), bottom.plain_text())
            }
            Self::Superscript(base, power) => {
                format!("{}^{}", base.plain_text(), power.plain_text())
            }
            Self::Subscript(base, index) => format!("{}_{}", base.plain_text(), index.plain_text()),
            Self::Radical(inside) => format!("sqrt({})", inside.plain_text()),
            Self::Delimited(inside) => format!("({})", inside.plain_text()),
        }
    }

    /// Whether there is nothing in it.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Text(text) => text.is_empty(),
            Self::Row(pieces) => pieces.iter().all(Self::is_empty),
            _ => false,
        }
    }
}

/// The Greek letters, by the names they are typed with.
///
/// Word takes `\alpha` and gives back α as it is typed. The list is the letters
/// themselves; anything else after a backslash is left as it was written, so a
/// name nobody here knows comes out visible rather than swallowed.
const GREEK: &[(&str, char)] = &[
    ("alpha", 'α'),
    ("beta", 'β'),
    ("gamma", 'γ'),
    ("delta", 'δ'),
    ("epsilon", 'ε'),
    ("zeta", 'ζ'),
    ("eta", 'η'),
    ("theta", 'θ'),
    ("iota", 'ι'),
    ("kappa", 'κ'),
    ("lambda", 'λ'),
    ("mu", 'μ'),
    ("nu", 'ν'),
    ("xi", 'ξ'),
    ("pi", 'π'),
    ("rho", 'ρ'),
    ("sigma", 'σ'),
    ("tau", 'τ'),
    ("phi", 'φ'),
    ("chi", 'χ'),
    ("psi", 'ψ'),
    ("omega", 'ω'),
    ("Gamma", 'Γ'),
    ("Delta", 'Δ'),
    ("Theta", 'Θ'),
    ("Lambda", 'Λ'),
    ("Xi", 'Ξ'),
    ("Pi", 'Π'),
    ("Sigma", 'Σ'),
    ("Phi", 'Φ'),
    ("Psi", 'Ψ'),
    ("Omega", 'Ω'),
    // The few operators worth a name of their own.
    ("times", '×'),
    ("div", '÷'),
    ("pm", '±'),
    ("le", '≤'),
    ("ge", '≥'),
    ("ne", '≠'),
    ("approx", '≈'),
    ("infty", '∞'),
    ("cdot", '·'),
];

/// Reads an equation out of the one line it was typed on.
#[must_use]
pub fn parse(typed: &str) -> Math {
    let characters: Vec<char> = typed.chars().collect();
    let mut reader = Reader { characters: &characters, at: 0 };
    let parsed = reader.expression();
    if parsed.is_empty() {
        return Math::Row(Vec::new());
    }
    parsed
}

/// Where the parser has got to.
struct Reader<'a> {
    characters: &'a [char],
    at: usize,
}

impl Reader<'_> {
    fn peek(&self) -> Option<char> {
        self.characters.get(self.at).copied()
    }

    fn skip_spaces(&mut self) {
        while self.peek().is_some_and(|character| character == ' ') {
            self.at += 1;
        }
    }

    /// A whole expression: pieces one after another until a bracket closes.
    fn expression(&mut self) -> Math {
        let mut pieces = Vec::new();
        loop {
            self.skip_spaces();
            match self.peek() {
                None | Some(')') => break,
                _ => {}
            }
            let piece = self.fraction();
            if piece.is_empty() {
                break;
            }
            pieces.push(piece);
        }
        flatten(pieces)
    }

    /// A power, or a division of powers.
    fn fraction(&mut self) -> Math {
        let mut left = self.power();
        loop {
            self.skip_spaces();
            if self.peek() != Some('/') {
                return left;
            }
            self.at += 1;
            let right = self.power();
            left = Math::Fraction(Box::new(left), Box::new(right));
        }
    }

    /// Something with a power or an index on it.
    fn power(&mut self) -> Math {
        let mut base = self.atom();
        loop {
            self.skip_spaces();
            match self.peek() {
                Some('^') => {
                    self.at += 1;
                    let above = self.atom();
                    base = Math::Superscript(Box::new(base), Box::new(above));
                }
                Some('_') => {
                    self.at += 1;
                    let below = self.atom();
                    base = Math::Subscript(Box::new(base), Box::new(below));
                }
                _ => return base,
            }
        }
    }

    /// One indivisible piece.
    fn atom(&mut self) -> Math {
        self.skip_spaces();
        let Some(character) = self.peek() else { return Math::Row(Vec::new()) };

        match character {
            '(' => {
                self.at += 1;
                let inside = self.expression();
                // A bracket that never closes closes at the end, which is
                // friendlier than refusing the whole equation over one key.
                if self.peek() == Some(')') {
                    self.at += 1;
                }
                Math::Delimited(Box::new(inside))
            }
            ')' => Math::Row(Vec::new()),
            '\\' => {
                self.at += 1;
                let word = self.word();
                match GREEK.iter().find(|(name, _)| *name == word) {
                    Some((_, letter)) => Math::Text(letter.to_string()),
                    None => Math::Text(format!("\\{word}")),
                }
            }
            character if character.is_alphabetic() => {
                let word = self.word();
                // The one function name the linear format needs.
                if word == "sqrt" {
                    let inside = self.atom();
                    return Math::Radical(Box::new(unwrap_brackets(inside)));
                }
                Math::Text(word)
            }
            character if character.is_ascii_digit() || character == '.' => {
                Math::Text(self.number())
            }
            // Anything else is one symbol: an operator, a comma, a bracket of
            // another kind.
            _ => {
                self.at += 1;
                Math::Text(character.to_string())
            }
        }
    }

    /// A run of letters.
    fn word(&mut self) -> String {
        let mut out = String::new();
        while let Some(character) = self.peek() {
            if !character.is_alphabetic() {
                break;
            }
            out.push(character);
            self.at += 1;
        }
        out
    }

    /// A run of digits, with a decimal point if there is one.
    fn number(&mut self) -> String {
        let mut out = String::new();
        while let Some(character) = self.peek() {
            if !character.is_ascii_digit() && character != '.' {
                break;
            }
            out.push(character);
            self.at += 1;
        }
        out
    }
}

/// A row of one is that one, and a row of none is nothing.
fn flatten(mut pieces: Vec<Math>) -> Math {
    if pieces.len() == 1 {
        return pieces.remove(0);
    }
    Math::Row(pieces)
}

/// A root takes what is under it without the brackets it was typed with.
fn unwrap_brackets(math: Math) -> Math {
    match math {
        Math::Delimited(inside) => *inside,
        other => other,
    }
}

// --- Writing -----------------------------------------------------------------

/// Builds the `m:oMath` element for an equation.
#[must_use]
pub fn math_element(math: &Math, prefix: &str) -> Element {
    let mut root = Element::new(&named(prefix, "oMath"), Some(MATH_NAMESPACE));
    for child in children_of(math, prefix) {
        root.push_element(child);
    }
    root
}

fn named(prefix: &str, local: &str) -> String {
    format!("{prefix}:{local}")
}

/// What one piece of an equation becomes, which for a row is several elements.
fn children_of(math: &Math, prefix: &str) -> Vec<Element> {
    match math {
        Math::Row(pieces) => pieces.iter().flat_map(|piece| children_of(piece, prefix)).collect(),
        other => vec![element_of(other, prefix)],
    }
}

/// And what a single piece becomes.
fn element_of(math: &Math, prefix: &str) -> Element {
    match math {
        Math::Row(_) => {
            // A row inside something is written as the argument holding all of
            // its pieces, which the callers below do for themselves. Reaching
            // here means a row of rows, which flattens to the same thing.
            let mut wrapper = Element::new(&named(prefix, "e"), Some(MATH_NAMESPACE));
            for child in children_of(math, prefix) {
                wrapper.push_element(child);
            }
            wrapper
        }
        Math::Text(text) => {
            let mut run = Element::new(&named(prefix, "r"), Some(MATH_NAMESPACE));
            let mut body = Element::new(&named(prefix, "t"), Some(MATH_NAMESPACE));
            body.set_text(text);
            // Spaces at either end of a piece are part of the equation.
            body.set_namespaced_attribute("xml:space", crate::edit::XML_NAMESPACE, "preserve");
            run.push_element(body);
            run
        }
        Math::Fraction(top, bottom) => {
            let mut fraction = Element::new(&named(prefix, "f"), Some(MATH_NAMESPACE));
            fraction.push_element(argument(prefix, "num", top));
            fraction.push_element(argument(prefix, "den", bottom));
            fraction
        }
        Math::Superscript(base, power) => {
            let mut element = Element::new(&named(prefix, "sSup"), Some(MATH_NAMESPACE));
            element.push_element(argument(prefix, "e", base));
            element.push_element(argument(prefix, "sup", power));
            element
        }
        Math::Subscript(base, index) => {
            let mut element = Element::new(&named(prefix, "sSub"), Some(MATH_NAMESPACE));
            element.push_element(argument(prefix, "e", base));
            element.push_element(argument(prefix, "sub", index));
            element
        }
        Math::Radical(inside) => {
            let mut element = Element::new(&named(prefix, "rad"), Some(MATH_NAMESPACE));
            // No degree written above the sign, which is what makes it a square
            // root rather than a root of something unstated.
            let mut properties = Element::new(&named(prefix, "radPr"), Some(MATH_NAMESPACE));
            let mut hide = Element::new(&named(prefix, "degHide"), Some(MATH_NAMESPACE));
            hide.set_namespaced_attribute(&named(prefix, "val"), MATH_NAMESPACE, "1");
            properties.push_element(hide);
            element.push_element(properties);
            element.push_element(Element::new(&named(prefix, "deg"), Some(MATH_NAMESPACE)));
            element.push_element(argument(prefix, "e", inside));
            element
        }
        Math::Delimited(inside) => {
            let mut element = Element::new(&named(prefix, "d"), Some(MATH_NAMESPACE));
            element.push_element(argument(prefix, "e", inside));
            element
        }
    }
}

/// One named argument of an element, holding whatever is inside it.
fn argument(prefix: &str, local: &str, math: &Math) -> Element {
    let mut wrapper = Element::new(&named(prefix, local), Some(MATH_NAMESPACE));
    for child in children_of(math, prefix) {
        wrapper.push_element(child);
    }
    wrapper
}

// --- Reading -----------------------------------------------------------------

/// Reads an equation back out of an `m:oMath` element.
#[must_use]
pub fn read_math(element: &Element) -> Math {
    flatten(element.child_elements().filter_map(read_piece).collect())
}

/// Reads one piece, or nothing for an element this does not know.
fn read_piece(element: &Element) -> Option<Math> {
    if element.namespace.as_deref() != Some(MATH_NAMESPACE) {
        return None;
    }
    match element.local_name() {
        "r" => {
            let text: String = element
                .child_elements()
                .filter(|child| child.is(Some(MATH_NAMESPACE), "t"))
                .map(Element::text_content)
                .collect();
            (!text.is_empty()).then_some(Math::Text(text))
        }
        "f" => {
            let top = part(element, "num");
            let bottom = part(element, "den");
            Some(Math::Fraction(Box::new(top), Box::new(bottom)))
        }
        "sSup" => {
            Some(Math::Superscript(Box::new(part(element, "e")), Box::new(part(element, "sup"))))
        }
        "sSub" => {
            Some(Math::Subscript(Box::new(part(element, "e")), Box::new(part(element, "sub"))))
        }
        "rad" => Some(Math::Radical(Box::new(part(element, "e")))),
        "d" => Some(Math::Delimited(Box::new(part(element, "e")))),
        // A row written as a bare argument, which is what a nested row becomes.
        "e" => Some(read_math(element)),
        _ => None,
    }
}

/// One named argument of an element, read back.
fn part(element: &Element, local: &str) -> Math {
    element.child(Some(MATH_NAMESPACE), local).map(read_math).unwrap_or_default()
}

impl Document {
    /// Puts an equation at the caret.
    ///
    /// Returns whether anything was put in: an equation with nothing in it is
    /// not one.
    pub fn insert_equation(&mut self, math: &Math) -> bool {
        if math.is_empty() {
            return false;
        }
        let caret = self.caret();
        self.record(crate::history::EditKind::Structural, caret, false);

        // The namespace has to be declared before anything in it is written.
        // It is part of the standard, so unlike the extensions it needs no
        // marking as ignorable — a reader that meets it either knows it or
        // skips it by the rules it already follows.
        declare_namespace(&mut self.tree_mut().root);

        let element = math_element(math, MATH_PREFIX);
        let Some(path) = crate::position::paragraph_path(&self.tree().root, caret.paragraph) else {
            return false;
        };
        let Some(paragraph) = crate::edit::element_at_path_mut(&mut self.tree_mut().root, &path)
        else {
            return false;
        };

        // An equation goes beside the runs rather than inside one: it is not
        // something a run contains. The run under the caret is cut in two so
        // that there is a place between them to put it. Bookmarks go in the
        // same way, and for the same reason.
        crate::format::split_runs_at_offset(paragraph, caret.offset);
        let position = crate::edit::child_position_at_offset(paragraph, caret.offset);
        paragraph.insert_element(position, element);

        // Past the equation, which counts as one character.
        self.set_caret(crate::TextPosition::new(caret.paragraph, caret.offset + 1));
        self.mark_modified();
        true
    }
}

/// Declares the equation namespace on the root, if it is not there already.
fn declare_namespace(root: &mut Element) {
    if root.declarations.iter().any(|(_, uri)| uri == MATH_NAMESPACE) {
        return;
    }
    root.declarations.push((Some(MATH_PREFIX.to_owned()), MATH_NAMESPACE.to_owned()));
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_letter_on_its_own_is_a_letter() {
        assert_eq!(parse("x"), Math::Text("x".to_owned()));
    }

    #[test]
    fn nothing_typed_is_nothing() {
        assert!(parse("").is_empty());
        assert!(parse("   ").is_empty());
    }

    #[test]
    fn a_slash_makes_a_fraction() {
        assert_eq!(
            parse("a/b"),
            Math::Fraction(
                Box::new(Math::Text("a".to_owned())),
                Box::new(Math::Text("b".to_owned()))
            )
        );
    }

    #[test]
    fn a_power_binds_tighter_than_a_fraction() {
        // a/b^2 is a over b squared, not a over b, all squared.
        let parsed = parse("a/b^2");
        let Math::Fraction(_, bottom) = parsed else { panic!("a fraction, got {parsed:?}") };
        assert!(matches!(*bottom, Math::Superscript(..)), "got {bottom:?}");
    }

    #[test]
    fn brackets_group_what_is_inside_them() {
        let parsed = parse("(a+b)/2");
        let Math::Fraction(top, _) = parsed else { panic!("a fraction") };
        assert!(matches!(*top, Math::Delimited(_)), "got {top:?}");
    }

    #[test]
    fn a_bracket_left_open_closes_at_the_end() {
        let parsed = parse("(a+b");
        assert!(matches!(parsed, Math::Delimited(_)), "got {parsed:?}");
    }

    #[test]
    fn a_root_takes_what_follows_without_its_brackets() {
        assert_eq!(parse("sqrt(x)"), Math::Radical(Box::new(Math::Text("x".to_owned()))));
    }

    #[test]
    fn an_index_goes_under_rather_than_over() {
        assert!(matches!(parse("x_1"), Math::Subscript(..)));
    }

    #[test]
    fn a_greek_letter_is_typed_by_name() {
        assert_eq!(parse("\\alpha"), Math::Text("α".to_owned()));
        assert_eq!(parse("\\Omega"), Math::Text("Ω".to_owned()));
    }

    #[test]
    fn a_name_nobody_here_knows_stays_as_it_was_typed() {
        assert_eq!(parse("\\wobble"), Math::Text("\\wobble".to_owned()));
    }

    #[test]
    fn several_pieces_in_a_row_stay_in_order() {
        let parsed = parse("2x+1");
        let Math::Row(pieces) = &parsed else { panic!("a row, got {parsed:?}") };
        let text: Vec<String> = pieces.iter().map(Math::plain_text).collect();
        assert_eq!(text, vec!["2", "x", "+", "1"]);
    }

    #[test]
    fn every_shape_survives_being_written_and_read_back() {
        for typed in ["x", "a/b", "x^2", "x_1", "sqrt(x)", "(a+b)/2", "2x+1", "\\pi r^2"] {
            let parsed = parse(typed);
            let element = math_element(&parsed, MATH_PREFIX);
            assert_eq!(read_math(&element), parsed, "{typed}");
        }
    }

    #[test]
    fn an_equation_is_written_in_the_math_namespace() {
        let element = math_element(&parse("a/b"), MATH_PREFIX);
        assert_eq!(element.namespace.as_deref(), Some(MATH_NAMESPACE));
        assert_eq!(element.local_name(), "oMath");
        assert!(element.child(Some(MATH_NAMESPACE), "f").is_some());
    }

    #[test]
    fn a_fraction_has_a_numerator_and_a_denominator() {
        let element = math_element(&parse("a/b"), MATH_PREFIX);
        let fraction = element.child(Some(MATH_NAMESPACE), "f").expect("a fraction");
        assert!(fraction.child(Some(MATH_NAMESPACE), "num").is_some());
        assert!(fraction.child(Some(MATH_NAMESPACE), "den").is_some());
    }

    #[test]
    fn a_square_root_hides_its_degree() {
        let element = math_element(&parse("sqrt(2)"), MATH_PREFIX);
        let root = element.child(Some(MATH_NAMESPACE), "rad").expect("a root");
        let properties = root.child(Some(MATH_NAMESPACE), "radPr").expect("properties");
        assert!(properties.child(Some(MATH_NAMESPACE), "degHide").is_some());
    }

    #[test]
    fn an_equation_reads_back_as_a_line_of_text() {
        assert_eq!(parse("a/b").plain_text(), "(a)/(b)");
        assert_eq!(parse("x^2+1").plain_text(), "x^2+1");
    }
}
