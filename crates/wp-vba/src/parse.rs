//! Reading Visual Basic into a tree.
//!
//! # How it reads
//!
//! One statement at a time, each one ending where its line does, because that
//! is how the language is written: there is no semicolon, a statement ends at
//! the end of the line unless an underscore says it carries on, and a colon
//! may put two of them on one line. So the parser is a loop over lines, and
//! every line is dispatched on the word it starts with.
//!
//! Expressions are the other half, and they are read by climbing: the
//! loosest-binding operator is tried first and each one hands over to the one
//! that binds tighter, down to a name or a number. Visual Basic has fourteen
//! levels between `Imp` and `^`, and they are written out here one function
//! apiece, in order, so that the precedence can be read off the file rather
//! than worked out from a table of numbers.
//!
//! # What happens to a line nobody understands
//!
//! It is kept. A line this parser cannot make sense of becomes an
//! [`Part::Unknown`] holding its words, and a complaint is written down
//! saying which line and what was expected. Nothing is dropped, so the module
//! still comes back byte for byte; and nothing is pretended, so a caller can
//! ask whether a module parsed by asking whether there were complaints.
//!
//! That matters more than it looks. Visual Basic is a large language with a
//! long history, and a parser that quietly swallowed what it did not know
//! would report every module as read and be wrong about half of them.

use crate::lex::{tokens, Kind, Token};
use crate::tree::{Complaint, Node, Part};

/// Reads a module into a tree, and says what it could not make sense of.
#[must_use]
pub fn parse(source: &str) -> (Node, Vec<Complaint>) {
    let words = tokens(source);
    let mut reader =
        Reader { words: &words, at: 0, complaints: Vec::new(), inside_a_line_if: false };
    let module = reader.module();
    (module, reader.complaints)
}

/// Where the reading has got to.
struct Reader<'a> {
    words: &'a [Token],
    at: usize,
    complaints: Vec<Complaint>,
    /// Whether an `Else` would end the statement being read.
    ///
    /// Inside a one-line `If`, it does: `If a Then b = 1 Else b = 2` is three
    /// statements on one line, and the first of them stops at the `Else`.
    /// Everywhere else an `Else` begins a line of its own.
    inside_a_line_if: bool,
}

impl Reader<'_> {
    // --- The words -----------------------------------------------------

    /// The word being looked at. Never past the end: the last one is the end
    /// of the file, and it stays there however often it is asked for.
    fn here(&self) -> &Token {
        self.ahead(0)
    }

    fn ahead(&self, by: usize) -> &Token {
        let at = (self.at + by).min(self.words.len().saturating_sub(1));
        &self.words[at]
    }

    fn kind(&self) -> Kind {
        self.here().kind
    }

    fn done(&self) -> bool {
        self.kind() == Kind::End
    }

    /// Whether the word here is the one given.
    fn word(&self, word: &str) -> bool {
        self.here().is(word)
    }

    /// Whether the words from here on are the ones given, in order.
    fn looks_like(&self, words: &[&str]) -> bool {
        words.iter().enumerate().all(|(by, word)| self.ahead(by).is(word))
    }

    fn symbol(&self, symbol: &str) -> bool {
        self.here().symbol(symbol)
    }

    /// Takes the word here and moves on.
    fn take(&mut self) -> Node {
        let token = self.words[self.at.min(self.words.len().saturating_sub(1))].clone();
        if token.kind != Kind::End {
            self.at += 1;
        }
        Node::Word(token)
    }

    /// Takes it if it is the symbol given, and says whether it was.
    fn take_symbol(&mut self, symbol: &str, into: &mut Vec<Node>) -> bool {
        if self.symbol(symbol) {
            into.push(self.take());
            return true;
        }
        false
    }

    /// Takes it if it is the word given, and says whether it was.
    fn take_word(&mut self, word: &str, into: &mut Vec<Node>) -> bool {
        if self.word(word) {
            into.push(self.take());
            return true;
        }
        false
    }

    /// Writes down what was expected and where.
    fn complain(&mut self, said: &str) {
        let line = self.here().line;
        // One complaint a line is enough: a line that went wrong at its
        // third word is not three mistakes.
        if self.complaints.last().is_some_and(|last| last.line == line) {
            return;
        }
        self.complaints.push(Complaint { line, said: said.to_owned() });
    }

    /// Whether the statement ends here.
    fn at_end_of_statement(&self) -> bool {
        matches!(self.kind(), Kind::NewLine | Kind::End | Kind::Comment)
            || self.symbol(":")
            || (self.inside_a_line_if && self.word("else"))
    }

    /// Takes whatever ends the statement: a comment, a colon, the line.
    fn finish(&mut self, into: &mut Vec<Node>) {
        loop {
            if self.inside_a_line_if && self.word("else") {
                return;
            }
            match self.kind() {
                Kind::Comment => into.push(self.take()),
                Kind::NewLine | Kind::End => {
                    if self.kind() == Kind::NewLine {
                        into.push(self.take());
                    }
                    return;
                }
                _ if self.symbol(":") => {
                    into.push(self.take());
                    return;
                }
                _ => {
                    self.complain("Expected: end of statement");
                    into.push(self.rest_of_line());
                    return;
                }
            }
        }
    }

    /// Everything left on the line, kept whole.
    fn rest_of_line(&mut self) -> Node {
        let mut held = Vec::new();
        while !matches!(self.kind(), Kind::NewLine | Kind::End) {
            held.push(self.take());
        }
        if self.kind() == Kind::NewLine {
            held.push(self.take());
        }
        Node::branch(Part::Unknown, held)
    }

    // --- The module ----------------------------------------------------

    fn module(&mut self) -> Node {
        let mut children = Vec::new();
        while !self.done() {
            children.push(self.statement());
        }
        children.push(self.take());
        Node::branch(Part::Module, children)
    }

    /// The statements of a block, up to any of the lines that would close it.
    fn body(&mut self, stops: &[&[&str]]) -> Node {
        let mut children = Vec::new();
        while !self.done() && !stops.iter().any(|words| self.looks_like(words)) {
            children.push(self.statement());
        }
        Node::branch(Part::Body, children)
    }

    /// One statement, whatever kind it is.
    fn statement(&mut self) -> Node {
        match self.kind() {
            Kind::NewLine => Node::branch(Part::Blank, vec![self.take()]),
            Kind::Comment => {
                let mut held = vec![self.take()];
                self.finish(&mut held);
                Node::branch(Part::Blank, held)
            }
            // A line number, which is a label written as a number.
            Kind::Number if self.ahead(1).kind != Kind::Symbol || self.ahead(1).symbol(":") => {
                let mut held = vec![self.take()];
                self.take_symbol(":", &mut held);
                if self.at_end_of_statement() {
                    self.finish(&mut held);
                    return Node::branch(Part::Label, held);
                }
                held.push(self.statement());
                Node::branch(Part::Label, held)
            }
            Kind::Symbol if self.symbol("#") => self.directive(),
            Kind::Symbol if self.symbol(":") => {
                // An empty statement, which a stray colon leaves behind.
                let mut held = vec![self.take()];
                self.finish(&mut held);
                Node::branch(Part::Blank, held)
            }
            Kind::Word => self.word_statement(),
            _ => self.expression_statement(),
        }
    }

    /// A statement that begins with a word, which is nearly all of them.
    fn word_statement(&mut self) -> Node {
        // A label: a name and a colon, at the start of a line.
        if self.kind() == Kind::Word && self.ahead(1).symbol(":") && !self.word("rem") {
            let mut held = vec![self.take(), self.take()];
            if self.at_end_of_statement() {
                self.finish(&mut held);
            } else {
                held.push(self.statement());
            }
            return Node::branch(Part::Label, held);
        }

        if self.word("attribute") {
            return self.simple(Part::Attribute);
        }
        if self.word("option") {
            return self.simple(Part::Option);
        }
        if self.word("implements") {
            return self.simple(Part::Implements);
        }
        if self.looks_like(&["if"]) {
            return self.if_statement();
        }
        if self.looks_like(&["for", "each"]) {
            return self.for_each();
        }
        if self.word("for") {
            return self.for_statement();
        }
        if self.word("do") {
            return self.do_statement();
        }
        if self.word("while") {
            return self.while_statement();
        }
        if self.looks_like(&["select", "case"]) {
            return self.select_statement();
        }
        if self.word("with") {
            return self.with_statement();
        }
        if self.word("on") {
            return self.on_statement();
        }
        if self.word("exit") {
            return self.simple(Part::Exit);
        }
        if self.word("redim") {
            return self.redim();
        }
        if self.word("erase") {
            return self.erase();
        }
        if self.word("stop") || self.looks_like(&["end"]) && self.ahead(1).kind != Kind::Word {
            return self.simple(Part::Stop);
        }
        if self.word("goto") || self.word("gosub") || self.word("return") || self.word("resume") {
            return self.jump();
        }
        if self.word("set") || self.word("let") {
            return self.assignment_with_word();
        }
        if self.word("call") {
            return self.call_statement();
        }
        if self.is_file_statement() {
            return self.file_statement();
        }
        if self.starts_a_declaration() {
            return self.declaration_or_procedure();
        }
        self.expression_statement()
    }

    /// A statement that is a word and then whatever is left of the line, kept
    /// as the words it was written with: `Option Explicit`, `Attribute VB_Name
    /// = "Module1"`, `Exit Sub`.
    fn simple(&mut self, part: Part) -> Node {
        let mut held = Vec::new();
        while !self.at_end_of_statement() {
            held.push(self.take());
        }
        self.finish(&mut held);
        Node::branch(part, held)
    }

    // --- Declarations and procedures ------------------------------------

    fn starts_a_declaration(&self) -> bool {
        const STARTS: [&str; 12] = [
            "dim",
            "private",
            "public",
            "global",
            "static",
            "const",
            "type",
            "enum",
            "declare",
            "event",
            "friend",
            "withevents",
        ];
        STARTS.iter().any(|word| self.word(word))
            || self.word("sub")
            || self.word("function")
            || self.word("property")
    }

    fn declaration_or_procedure(&mut self) -> Node {
        let mut modifiers = Vec::new();
        while self.word("public")
            || self.word("private")
            || self.word("friend")
            || self.word("global")
            || self.word("static")
            || self.word("dim")
        {
            modifiers.push(self.take());
        }

        if self.word("sub") || self.word("function") || self.word("property") {
            return self.procedure(modifiers);
        }
        if self.word("type") {
            return self.type_block(modifiers);
        }
        if self.word("enum") {
            return self.enum_block(modifiers);
        }
        if self.word("declare") {
            modifiers.push(self.take());
            return self.simple_from(Part::Declare, modifiers);
        }
        if self.word("const") {
            modifiers.push(self.take());
            return self.constant(modifiers);
        }
        if self.word("event") {
            modifiers.push(self.take());
            return self.simple_from(Part::Event, modifiers);
        }
        self.variables(modifiers)
    }

    /// The rest of a line, on the end of what has already been taken.
    fn simple_from(&mut self, part: Part, mut held: Vec<Node>) -> Node {
        while !self.at_end_of_statement() {
            held.push(self.take());
        }
        self.finish(&mut held);
        Node::branch(part, held)
    }

    /// `Dim a As Long, b(1 To 5) As String`.
    fn variables(&mut self, mut held: Vec<Node>) -> Node {
        loop {
            held.push(self.declared());
            if !self.take_symbol(",", &mut held) {
                break;
            }
        }
        self.finish(&mut held);
        Node::branch(Part::Declaration, held)
    }

    /// One name of a declaration: what it is called, how big it is, and what
    /// kind of thing it holds.
    fn declared(&mut self) -> Node {
        let mut held = Vec::new();
        self.take_word("withevents", &mut held);
        if self.kind() == Kind::Word {
            held.push(self.take());
        } else {
            self.complain("Expected: identifier");
        }
        held.extend(self.type_suffix());
        if self.symbol("(") {
            held.push(self.arguments());
        }
        if self.take_word("as", &mut held) {
            self.take_word("new", &mut held);
            held.push(self.type_name());
        }
        Node::branch(Part::Declared, held)
    }

    /// The one character a name may carry to say what kind it holds.
    ///
    /// `s$` is a string and `n&` is a long. The same characters are operators
    /// — `&` joins two strings — and what tells them apart is the space: a
    /// suffix is written against its name and an operator is written beside
    /// it, which is why the spaces in front of every word are kept.
    fn type_suffix(&mut self) -> Vec<Node> {
        const SUFFIXES: [&str; 6] = ["$", "%", "&", "!", "#", "@"];
        if self.here().before.is_empty()
            && SUFFIXES.iter().any(|suffix| self.symbol(suffix))
            && !self.ahead(1).symbol("(")
        {
            return vec![self.take()];
        }
        Vec::new()
    }

    /// The name of a type, which may be somebody else's: `Excel.Range`, and
    /// `String * 10` for the fixed-length sort.
    fn type_name(&mut self) -> Node {
        let mut held = Vec::new();
        if self.kind() == Kind::Word {
            held.push(self.take());
            while self.symbol(".") {
                held.push(self.take());
                if self.kind() == Kind::Word {
                    held.push(self.take());
                }
            }
        } else {
            self.complain("Expected: identifier");
        }
        if self.symbol("*") {
            held.push(self.take());
            held.push(self.expression());
        }
        Node::branch(Part::Name, held)
    }

    /// `Const A As Long = 1, B = 2`.
    fn constant(&mut self, mut held: Vec<Node>) -> Node {
        loop {
            let mut one = Vec::new();
            if self.kind() == Kind::Word {
                one.push(self.take());
            } else {
                self.complain("Expected: identifier");
            }
            if self.take_word("as", &mut one) {
                one.push(self.type_name());
            }
            if self.take_symbol("=", &mut one) {
                one.push(self.expression());
            } else {
                self.complain("Expected: =");
            }
            held.push(Node::branch(Part::Declared, one));
            if !self.take_symbol(",", &mut held) {
                break;
            }
        }
        self.finish(&mut held);
        Node::branch(Part::Constant, held)
    }

    /// `Type Point … End Type`.
    fn type_block(&mut self, mut held: Vec<Node>) -> Node {
        held.push(self.take());
        if self.kind() == Kind::Word {
            held.push(self.take());
        } else {
            self.complain("Expected: identifier");
        }
        self.finish(&mut held);

        let mut inside = Vec::new();
        while !self.done() && !self.looks_like(&["end", "type"]) {
            match self.kind() {
                Kind::NewLine | Kind::Comment => inside.push(self.statement()),
                _ => {
                    let mut one = vec![self.declared()];
                    self.finish(&mut one);
                    inside.push(Node::branch(Part::Member, one));
                }
            }
        }
        held.push(Node::branch(Part::Body, inside));
        self.close(&["end", "type"], &mut held, "Expected: End Type");
        Node::branch(Part::TypeBlock, held)
    }

    /// `Enum Colours … End Enum`.
    fn enum_block(&mut self, mut held: Vec<Node>) -> Node {
        held.push(self.take());
        if self.kind() == Kind::Word {
            held.push(self.take());
        } else {
            self.complain("Expected: identifier");
        }
        self.finish(&mut held);

        let mut inside = Vec::new();
        while !self.done() && !self.looks_like(&["end", "enum"]) {
            match self.kind() {
                Kind::NewLine | Kind::Comment => inside.push(self.statement()),
                _ => {
                    let mut one = Vec::new();
                    if self.kind() == Kind::Word {
                        one.push(self.take());
                    } else {
                        self.complain("Expected: identifier");
                    }
                    if self.take_symbol("=", &mut one) {
                        one.push(self.expression());
                    }
                    self.finish(&mut one);
                    inside.push(Node::branch(Part::Member, one));
                }
            }
        }
        held.push(Node::branch(Part::Body, inside));
        self.close(&["end", "enum"], &mut held, "Expected: End Enum");
        Node::branch(Part::EnumBlock, held)
    }

    /// `Sub`, `Function`, `Property Get|Let|Set`, and everything in them.
    fn procedure(&mut self, mut held: Vec<Node>) -> Node {
        let ending: &[&str] = if self.word("sub") {
            &["end", "sub"]
        } else if self.word("function") {
            &["end", "function"]
        } else {
            &["end", "property"]
        };
        held.push(self.take());
        // `Property Get`, `Property Let`, `Property Set`.
        if ending[1] == "property" {
            if self.word("get") || self.word("let") || self.word("set") {
                held.push(self.take());
            } else {
                self.complain("Expected: Get, Let or Set");
            }
        }
        if self.kind() == Kind::Word {
            held.push(self.take());
        } else {
            self.complain("Expected: identifier");
        }
        if self.symbol("(") {
            held.push(self.parameters());
        }
        if self.take_word("as", &mut held) {
            held.push(self.type_name());
        }
        self.finish(&mut held);

        held.push(self.body(&[ending]));
        let said = format!(
            "Expected: End {}",
            if ending[1] == "sub" {
                "Sub"
            } else if ending[1] == "function" {
                "Function"
            } else {
                "Property"
            }
        );
        self.close(ending, &mut held, &said);
        Node::branch(Part::Procedure, held)
    }

    /// What a procedure is given.
    fn parameters(&mut self) -> Node {
        let mut held = vec![self.take()];
        while !self.symbol(")") && !self.at_end_of_statement() {
            let mut one = Vec::new();
            self.take_word("optional", &mut one);
            if !self.take_word("byval", &mut one) {
                self.take_word("byref", &mut one);
            }
            self.take_word("paramarray", &mut one);
            one.push(self.declared());
            if self.take_symbol("=", &mut one) {
                one.push(self.expression());
            }
            held.push(Node::branch(Part::Parameter, one));
            if !self.take_symbol(",", &mut held) {
                break;
            }
        }
        if !self.take_symbol(")", &mut held) {
            self.complain("Expected: )");
        }
        Node::branch(Part::Parameters, held)
    }

    /// Takes the words that close a block, or says they are missing.
    fn close(&mut self, words: &[&str], held: &mut Vec<Node>, said: &str) {
        if self.looks_like(words) {
            for _ in words {
                held.push(self.take());
            }
            // `Next i`, `Loop While x`: whatever else the closing line says.
            while !self.at_end_of_statement() {
                held.push(self.take());
            }
            self.finish(held);
        } else {
            self.complain(said);
        }
    }

    // --- The blocks -----------------------------------------------------

    fn if_statement(&mut self) -> Node {
        let mut held = vec![self.take()];
        held.push(self.expression());
        if !self.take_word("then", &mut held) {
            self.complain("Expected: Then");
        }

        // An `If` with anything after `Then` on the same line is the whole
        // statement: there is no `End If` to look for.
        if !self.at_end_of_statement() {
            held.push(self.statement_after_then());
            while self.word("else") {
                held.push(self.take());
                if self.at_end_of_statement() {
                    break;
                }
                held.push(self.statement_after_then());
            }
            if self.at_end_of_statement() {
                self.finish(&mut held);
            }
            return Node::branch(Part::LineIf, held);
        }
        self.finish(&mut held);

        held.push(self.body(&[&["elseif"], &["else"], &["end", "if"]]));
        while self.word("elseif") {
            let mut branch = vec![self.take()];
            branch.push(self.expression());
            if !self.take_word("then", &mut branch) {
                self.complain("Expected: Then");
            }
            self.finish(&mut branch);
            branch.push(self.body(&[&["elseif"], &["else"], &["end", "if"]]));
            held.push(Node::branch(Part::ElseIf, branch));
        }
        if self.word("else") {
            let mut branch = vec![self.take()];
            self.finish(&mut branch);
            branch.push(self.body(&[&["end", "if"]]));
            held.push(Node::branch(Part::Else, branch));
        }
        self.close(&["end", "if"], &mut held, "Expected: End If");
        Node::branch(Part::If, held)
    }

    /// One statement of a single-line `If`, which ends at `Else` as well as
    /// at the end of the line.
    fn statement_after_then(&mut self) -> Node {
        if self.word("else") {
            return Node::branch(Part::Blank, Vec::new());
        }
        let held = self.inside_a_line_if;
        self.inside_a_line_if = true;
        let before = self.at;
        let node = self.statement();
        self.inside_a_line_if = held;
        if self.at == before {
            return self.rest_of_line();
        }
        node
    }

    fn for_statement(&mut self) -> Node {
        let mut held = vec![self.take()];
        held.push(self.after());
        if !self.take_symbol("=", &mut held) {
            self.complain("Expected: =");
        }
        held.push(self.expression());
        if !self.take_word("to", &mut held) {
            self.complain("Expected: To");
        }
        held.push(self.expression());
        if self.take_word("step", &mut held) {
            held.push(self.expression());
        }
        self.finish(&mut held);
        held.push(self.body(&[&["next"]]));
        self.close(&["next"], &mut held, "Expected: Next");
        Node::branch(Part::For, held)
    }

    fn for_each(&mut self) -> Node {
        let mut held = vec![self.take(), self.take()];
        held.push(self.expression());
        if !self.take_word("in", &mut held) {
            self.complain("Expected: In");
        }
        held.push(self.expression());
        self.finish(&mut held);
        held.push(self.body(&[&["next"]]));
        self.close(&["next"], &mut held, "Expected: Next");
        Node::branch(Part::ForEach, held)
    }

    fn do_statement(&mut self) -> Node {
        let mut held = vec![self.take()];
        if self.take_word("while", &mut held) || self.take_word("until", &mut held) {
            held.push(self.expression());
        }
        self.finish(&mut held);
        held.push(self.body(&[&["loop"]]));
        // The condition at the bottom is read here rather than by `close`,
        // because it is an expression and not the words that end a block:
        // kept as words, nothing could ask what it says.
        if self.word("loop") {
            held.push(self.take());
            if self.take_word("while", &mut held) || self.take_word("until", &mut held) {
                held.push(self.expression());
            }
            self.finish(&mut held);
        } else {
            self.complain("Expected: Loop");
        }
        Node::branch(Part::Do, held)
    }

    fn while_statement(&mut self) -> Node {
        let mut held = vec![self.take()];
        held.push(self.expression());
        self.finish(&mut held);
        held.push(self.body(&[&["wend"]]));
        self.close(&["wend"], &mut held, "Expected: Wend");
        Node::branch(Part::While, held)
    }

    fn select_statement(&mut self) -> Node {
        let mut held = vec![self.take(), self.take()];
        held.push(self.expression());
        self.finish(&mut held);

        // Between `Select Case` and the first `Case` there is nothing but
        // blank lines and comments, and Word keeps them where they are.
        held.push(self.body(&[&["case"], &["end", "select"]]));
        while self.word("case") {
            let mut branch = vec![self.take()];
            if self.take_word("else", &mut branch) {
                // `Case Else` names nothing.
            } else {
                loop {
                    if self.take_word("is", &mut branch) {
                        // `Case Is > 5`: the comparison without its left side.
                        while !self.at_end_of_statement()
                            && !self.symbol(",")
                            && self.kind() == Kind::Symbol
                        {
                            branch.push(self.take());
                        }
                    }
                    branch.push(self.expression());
                    if self.take_word("to", &mut branch) {
                        branch.push(self.expression());
                    }
                    if !self.take_symbol(",", &mut branch) {
                        break;
                    }
                }
            }
            self.finish(&mut branch);
            branch.push(self.body(&[&["case"], &["end", "select"]]));
            held.push(Node::branch(Part::Case, branch));
        }
        self.close(&["end", "select"], &mut held, "Expected: End Select");
        Node::branch(Part::Select, held)
    }

    fn with_statement(&mut self) -> Node {
        let mut held = vec![self.take()];
        held.push(self.expression());
        self.finish(&mut held);
        held.push(self.body(&[&["end", "with"]]));
        self.close(&["end", "with"], &mut held, "Expected: End With");
        Node::branch(Part::With, held)
    }

    // --- The one-line statements ----------------------------------------

    fn on_statement(&mut self) -> Node {
        // `On Error GoTo …`, `On Error Resume Next`, `On x GoTo a, b`.
        self.simple(Part::OnError)
    }

    fn jump(&mut self) -> Node {
        self.simple(Part::Jump)
    }

    fn redim(&mut self) -> Node {
        let mut held = vec![self.take()];
        self.take_word("preserve", &mut held);
        loop {
            held.push(self.declared());
            if !self.take_symbol(",", &mut held) {
                break;
            }
        }
        self.finish(&mut held);
        Node::branch(Part::Redim, held)
    }

    fn erase(&mut self) -> Node {
        let mut held = vec![self.take()];
        loop {
            held.push(self.expression());
            if !self.take_symbol(",", &mut held) {
                break;
            }
        }
        self.finish(&mut held);
        Node::branch(Part::Erase, held)
    }

    /// `Set a = b` and `Let a = b`.
    fn assignment_with_word(&mut self) -> Node {
        let mut held = vec![self.take()];
        held.push(self.after());
        if !self.take_symbol("=", &mut held) {
            self.complain("Expected: =");
        }
        held.push(self.expression());
        self.finish(&mut held);
        Node::branch(Part::Assign, held)
    }

    /// `Call Foo(1, 2)`.
    fn call_statement(&mut self) -> Node {
        let mut held = vec![self.take()];
        held.push(self.expression());
        self.finish(&mut held);
        Node::branch(Part::Call, held)
    }

    /// Whether this line opens, closes, reads or writes a file.
    fn is_file_statement(&self) -> bool {
        const WORDS: [&str; 8] = ["open", "close", "print", "write", "input", "get", "put", "seek"];
        if self.looks_like(&["line", "input"]) {
            return true;
        }
        // `Print` and the rest are ordinary names as well — `Get` is a common
        // one — so only a line where the next thing is a file number or a
        // word counts, and never one where it is a bracket or an equals.
        WORDS.iter().any(|word| self.word(word))
            && (self.ahead(1).symbol("#")
                || (self.word("open") && self.ahead(1).kind != Kind::Symbol)
                || (self.word("close") && self.at_end_of_next()))
    }

    fn at_end_of_next(&self) -> bool {
        matches!(self.ahead(1).kind, Kind::NewLine | Kind::End | Kind::Comment)
    }

    /// A statement about a file, whose arguments are separated by commas and
    /// semicolons and may begin with a hash.
    fn file_statement(&mut self) -> Node {
        let mut held = vec![self.take()];
        if self.word("input") {
            held.push(self.take());
        }
        while !self.at_end_of_statement() {
            if self.symbol("#") || self.symbol(",") || self.symbol(";") {
                held.push(self.take());
                continue;
            }
            if self.word("for") || self.word("as") || self.word("access") || self.word("len") {
                held.push(self.take());
                continue;
            }
            let before = self.at;
            held.push(self.expression());
            if self.at == before {
                held.push(self.take());
            }
        }
        self.finish(&mut held);
        Node::branch(Part::File, held)
    }

    /// `#If`, `#Else`, `#End If`, `#Const`: the compiler's own lines, which
    /// are kept as they are written. What is inside them is read as ordinary
    /// statements, because it is ordinary Visual Basic whether or not this
    /// build would have compiled it.
    fn directive(&mut self) -> Node {
        let mut held = Vec::new();
        while !matches!(self.kind(), Kind::NewLine | Kind::End) {
            held.push(self.take());
        }
        self.finish(&mut held);
        Node::branch(Part::Directive, held)
    }

    /// A line that is an expression: an assignment, or a call.
    ///
    /// What is on the left of an assignment is a name and whatever follows
    /// it — brackets, full stops — and never a whole expression, because `=`
    /// is the comparison as well and a whole expression would swallow the
    /// assignment and leave a line that compares two things and does nothing
    /// with the answer.
    fn expression_statement(&mut self) -> Node {
        let before = self.at;
        let left = self.after();
        if self.at == before {
            // Nothing was read at all, so this is a line nobody here
            // understands. Keeping it whole is what lets the module come back
            // the way it went in.
            self.complain("Expected: statement");
            return self.rest_of_line();
        }

        if self.symbol("=") {
            let mut held = vec![left, self.take()];
            held.push(self.expression());
            self.finish(&mut held);
            return Node::branch(Part::Assign, held);
        }

        // A call written without brackets: `MsgBox "Hello", vbOK`.
        let mut held = vec![left];
        if !self.at_end_of_statement() {
            let mut arguments = Vec::new();
            loop {
                if self.at_end_of_statement() {
                    break;
                }
                let before = self.at;
                arguments.push(self.argument());
                if self.at == before {
                    break;
                }
                if !self.take_symbol(",", &mut arguments) {
                    break;
                }
            }
            held.push(Node::branch(Part::Arguments, arguments));
        }
        self.finish(&mut held);
        Node::branch(Part::Call, held)
    }

    // --- Expressions -----------------------------------------------------

    /// An expression, from the operator that binds loosest.
    fn expression(&mut self) -> Node {
        self.imply()
    }

    /// Takes a run of operators of one level, left to right.
    fn binary(&mut self, words: &[&str], symbols: &[&str], next: fn(&mut Self) -> Node) -> Node {
        let mut left = next(self);
        loop {
            let matched = words.iter().any(|word| self.word(word))
                || symbols.iter().any(|symbol| self.symbol(symbol));
            if !matched {
                return left;
            }
            let operator = self.take();
            let right = next(self);
            left = Node::branch(Part::Binary, vec![left, operator, right]);
        }
    }

    fn imply(&mut self) -> Node {
        self.binary(&["imp"], &[], Self::equivalent)
    }

    fn equivalent(&mut self) -> Node {
        self.binary(&["eqv"], &[], Self::exclusive_or)
    }

    fn exclusive_or(&mut self) -> Node {
        self.binary(&["xor"], &[], Self::inclusive_or)
    }

    fn inclusive_or(&mut self) -> Node {
        self.binary(&["or"], &[], Self::conjunction)
    }

    fn conjunction(&mut self) -> Node {
        self.binary(&["and"], &[], Self::negation)
    }

    /// `Not` binds tighter than `And` and looser than a comparison, so `Not a
    /// = b` asks whether `a = b` is false.
    fn negation(&mut self) -> Node {
        if self.word("not") {
            let operator = self.take();
            let operand = self.negation();
            return Node::branch(Part::Unary, vec![operator, operand]);
        }
        self.comparison()
    }

    fn comparison(&mut self) -> Node {
        self.binary(&["is", "like"], &["=", "<>", "<", ">", "<=", ">="], Self::concatenation)
    }

    fn concatenation(&mut self) -> Node {
        self.binary(&[], &["&"], Self::sum)
    }

    fn sum(&mut self) -> Node {
        self.binary(&[], &["+", "-"], Self::remainder)
    }

    fn remainder(&mut self) -> Node {
        self.binary(&["mod"], &[], Self::whole_division)
    }

    fn whole_division(&mut self) -> Node {
        self.binary(&[], &["\\"], Self::product)
    }

    fn product(&mut self) -> Node {
        self.binary(&[], &["*", "/"], Self::sign)
    }

    /// A minus in front of something, which binds looser than a power: minus
    /// two squared is minus four.
    fn sign(&mut self) -> Node {
        if self.symbol("-") || self.symbol("+") {
            let operator = self.take();
            let operand = self.sign();
            return Node::branch(Part::Unary, vec![operator, operand]);
        }
        self.power()
    }

    fn power(&mut self) -> Node {
        let left = self.after();
        if self.symbol("^") {
            let operator = self.take();
            // Right to left, and the exponent may carry its own sign.
            let right = self.sign();
            return Node::branch(Part::Binary, vec![left, operator, right]);
        }
        left
    }

    /// What follows a name: brackets, a full stop, a bang.
    fn after(&mut self) -> Node {
        let mut node = self.thing();
        loop {
            if self.symbol("(") {
                node = Node::branch(Part::Index, vec![node, self.arguments()]);
                continue;
            }
            if self.symbol(".") || self.symbol("!") {
                let mut held = vec![node, self.take()];
                if self.kind() == Kind::Word {
                    held.push(self.take());
                } else {
                    self.complain("Expected: identifier");
                }
                node = Node::branch(Part::Dotted, held);
                continue;
            }
            return node;
        }
    }

    /// A name, a number, a string, or something in brackets.
    fn thing(&mut self) -> Node {
        match self.kind() {
            Kind::Number | Kind::Text | Kind::Date => {
                Node::branch(Part::Literal, vec![self.take()])
            }
            Kind::Word => {
                if self.word("new") {
                    let operator = self.take();
                    return Node::branch(Part::New, vec![operator, self.type_name()]);
                }
                if self.word("typeof") {
                    let mut held = vec![self.take(), self.after()];
                    if self.take_word("is", &mut held) {
                        held.push(self.type_name());
                    } else {
                        self.complain("Expected: Is");
                    }
                    return Node::branch(Part::TypeOf, held);
                }
                if self.word("addressof") {
                    let operator = self.take();
                    return Node::branch(Part::AddressOf, vec![operator, self.after()]);
                }
                let mut held = vec![self.take()];
                held.extend(self.type_suffix());
                Node::branch(Part::Name, held)
            }
            Kind::Symbol if self.symbol("(") => {
                let mut held = vec![self.take()];
                held.push(self.expression());
                if !self.take_symbol(")", &mut held) {
                    self.complain("Expected: )");
                }
                Node::branch(Part::Parenthesised, held)
            }
            // Inside a `With`, a full stop begins an expression of its own.
            Kind::Symbol if self.symbol(".") || self.symbol("!") => {
                let mut held = vec![self.take()];
                if self.kind() == Kind::Word {
                    held.push(self.take());
                } else {
                    self.complain("Expected: identifier");
                }
                Node::branch(Part::Dotted, held)
            }
            _ => {
                self.complain("Expected: expression");
                Node::branch(Part::Unknown, Vec::new())
            }
        }
    }

    /// The brackets after a name, and what is between them.
    fn arguments(&mut self) -> Node {
        let mut held = vec![self.take()];
        while !self.symbol(")") && !self.at_end_of_statement() {
            let before = self.at;
            held.push(self.argument());
            if self.at == before && !self.symbol(",") {
                break;
            }
            if !self.take_symbol(",", &mut held) {
                break;
            }
        }
        if !self.take_symbol(")", &mut held) {
            self.complain("Expected: )");
        }
        Node::branch(Part::Arguments, held)
    }

    /// One argument, which may be named, may be left out, and may say how it
    /// is passed.
    fn argument(&mut self) -> Node {
        let mut held = Vec::new();
        // An argument that is not there at all: `Foo(1, , 3)`.
        if self.symbol(",") || self.symbol(")") {
            return Node::branch(Part::Argument, held);
        }
        // `Foo Bar:=1`, which is a name, a colon-equals and a value.
        if self.kind() == Kind::Word && self.ahead(1).symbol(":") && self.ahead(2).symbol("=") {
            held.push(self.take());
            held.push(self.take());
            held.push(self.take());
            held.push(self.expression());
            return Node::branch(Part::Argument, held);
        }
        if self.word("byval") || self.word("byref") {
            held.push(self.take());
        }
        held.push(self.expression());
        // `a(1 To 5)`, which is a declaration's bounds rather than a call's
        // argument, and is written in the same brackets.
        if self.take_word("to", &mut held) {
            held.push(self.expression());
        }
        Node::branch(Part::Argument, held)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tree, written out so that a test can say what shape it expects.
    ///
    /// Branches are named after what they are and hold what is under them;
    /// the ends of lines are left out, because a shape is about the words.
    fn sketch(node: &Node) -> String {
        match node {
            Node::Word(token) => match token.kind {
                Kind::NewLine | Kind::End => String::new(),
                _ => token.text.clone(),
            },
            Node::Branch { part, children } => {
                let inside: Vec<String> =
                    children.iter().map(sketch).filter(|written| !written.is_empty()).collect();
                format!("{part:?}({})", inside.join(" "))
            }
        }
    }

    /// The shape of the first statement of a line of Visual Basic.
    fn shape(source: &str) -> String {
        let (tree, complaints) = parse(source);
        assert!(complaints.is_empty(), "{source:?} complained: {complaints:?}");
        assert_eq!(tree.written(), source, "{source:?} did not come back the same");
        sketch(&tree.children()[0])
    }

    /// A module using every form this item names, and a few besides.
    const EVERYTHING: &str = "Attribute VB_Name = \"Module1\"\r\n\
         Option Explicit\r\n\
         Option Base 1\r\n\
         \r\n\
         Private Const Limit As Long = 10\r\n\
         Public Colours(1 To 3) As String\r\n\
         Dim counter%, name$\r\n\
         \r\n\
         Public Type Point\r\n    \
             X As Long\r\n    \
             Y As Long   ' the other one\r\n\
         End Type\r\n\
         \r\n\
         Public Enum Weekday\r\n    \
             Monday = 1\r\n    \
             Tuesday\r\n\
         End Enum\r\n\
         \r\n\
         Private Declare Function GetTickCount Lib \"kernel32\" () As Long\r\n\
         \r\n\
         ' What everybody writes first.\r\n\
         Public Sub Everything(ByVal count As Long, Optional ByRef note As String = \"\")\r\n    \
             Dim index As Long, total As Double\r\n    \
             Dim shape As New Collection\r\n\
         \r\n    \
             On Error GoTo Sorry\r\n    \
             If count > Limit Then\r\n        \
                 total = count * 2 + 1\r\n    \
             ElseIf count = 0 Then\r\n        \
                 total = 0\r\n    \
             Else\r\n        \
                 total = -count ^ 2\r\n    \
             End If\r\n\
         \r\n    \
             For index = 1 To count Step 2\r\n        \
                 total = total + index\r\n        \
                 If total > 100 Then Exit For\r\n    \
             Next index\r\n\
         \r\n    \
             For Each item In shape\r\n        \
                 Debug.Print item\r\n    \
             Next\r\n\
         \r\n    \
             Do While index > 0\r\n        \
                 index = index - 1\r\n    \
             Loop\r\n\
         \r\n    \
             Do\r\n        \
                 index = index + 1\r\n    \
             Loop Until index >= count\r\n\
         \r\n    \
             While index < 10\r\n        \
                 index = index + 1\r\n    \
             Wend\r\n\
         \r\n    \
             Select Case count\r\n        \
                 Case 1, 2\r\n            \
                     note = \"a few\"\r\n        \
                 Case 3 To 9\r\n            \
                     note = \"several\"\r\n        \
                 Case Is > 9\r\n            \
                     note = \"many\"\r\n        \
                 Case Else\r\n            \
                     note = \"none\"\r\n    \
             End Select\r\n\
         \r\n    \
             With shape\r\n        \
                 .Add index\r\n        \
                 .Add Item:=index, Key:=note\r\n    \
             End With\r\n\
         \r\n    \
             ReDim Preserve Colours(1 To count)\r\n    \
             Erase Colours\r\n    \
             Set shape = Nothing\r\n    \
             MsgBox \"Total: \" & total, vbOKOnly, _\r\n        \
                 \"Everything\"\r\n    \
             Exit Sub\r\n\
         Sorry:\r\n    \
             Resume Next\r\n\
         End Sub\r\n\
         \r\n\
         Private Function Twice(ByVal n As Long) As Long\r\n    \
             Twice = n * 2\r\n\
         End Function\r\n\
         \r\n\
         Public Property Get Width() As Long\r\n    \
             Width = 3\r\n\
         End Property\r\n\
         \r\n\
         Public Property Let Width(ByVal value As Long)\r\n\
         End Property\r\n";

    #[test]
    fn every_form_this_item_names_is_read_and_comes_back_the_same() {
        let (tree, complaints) = parse(EVERYTHING);
        assert!(complaints.is_empty(), "{complaints:#?}");
        assert_eq!(tree.written(), EVERYTHING, "the module did not come back as it went in");
        assert!(
            tree.every(Part::Unknown).is_empty(),
            "some of it was kept without being read: {:?}",
            tree.every(Part::Unknown).iter().map(|node| node.written()).collect::<Vec<_>>()
        );

        // And the shape is there to be asked about, which is the other half
        // of reading it.
        assert_eq!(tree.every(Part::Procedure).len(), 4);
        assert_eq!(tree.every(Part::If).len(), 1);
        assert_eq!(tree.every(Part::Case).len(), 4);
        assert_eq!(tree.every(Part::Parameter).len(), 4);
    }

    #[test]
    fn a_line_that_goes_on_is_one_statement() {
        let source = "x = a + _\r\n    b\r\n";
        assert_eq!(
            shape(source),
            "Assign(Name(x) = Binary(Name(a) + Name(b)))",
            "a continued line was read as two"
        );
    }

    #[test]
    fn what_binds_tighter_than_what() {
        // Multiplying before adding is the easy half; the interesting ones
        // are that a comparison binds tighter than Not, that Not binds
        // tighter than And, and that a power binds tighter than the minus in
        // front of it — minus two squared is minus four.
        assert_eq!(
            shape("x = a + b * c"),
            "Assign(Name(x) = Binary(Name(a) + Binary(Name(b) * Name(c))))"
        );
        assert_eq!(
            shape("x = (a + b) * c"),
            "Assign(Name(x) = Binary(Parenthesised(( Binary(Name(a) + Name(b)) )) * Name(c)))"
        );
        assert_eq!(
            shape("x = Not a = b"),
            "Assign(Name(x) = Unary(Not Binary(Name(a) = Name(b))))"
        );
        assert_eq!(
            shape("x = a And Not b Or c"),
            "Assign(Name(x) = Binary(Binary(Name(a) And Unary(Not Name(b))) Or Name(c)))"
        );
        assert_eq!(shape("x = -a ^ 2"), "Assign(Name(x) = Unary(- Binary(Name(a) ^ Literal(2))))");
        assert_eq!(
            shape("x = a & b + c"),
            "Assign(Name(x) = Binary(Name(a) & Binary(Name(b) + Name(c))))"
        );
    }

    #[test]
    fn a_call_written_without_brackets_is_still_a_call() {
        assert_eq!(
            shape("MsgBox \"Hello\", vbOKOnly"),
            "Call(Name(MsgBox) Arguments(Argument(Literal(\"Hello\")) , Argument(Name(vbOKOnly))))"
        );
        assert_eq!(
            shape("Call Foo(1)"),
            "Call(Call Index(Name(Foo) Arguments(( Argument(Literal(1)) ))))"
        );
    }

    #[test]
    fn an_if_on_one_line_has_no_end_if_to_look_for() {
        assert_eq!(
            shape("If a Then b = 1 Else b = 2"),
            "LineIf(If Name(a) Then Assign(Name(b) = Literal(1)) Else Assign(Name(b) = Literal(2)))"
        );
        // And one that is only an If still ends where its line does.
        assert_eq!(shape("If a Then Exit Sub"), "LineIf(If Name(a) Then Exit(Exit Sub))");
    }

    #[test]
    fn a_full_stop_may_begin_an_expression_inside_a_with() {
        let source = "With a\r\n    .b = .c(1)\r\nEnd With\r\n";
        let (tree, complaints) = parse(source);
        assert!(complaints.is_empty(), "{complaints:?}");
        assert_eq!(tree.written(), source);
        assert_eq!(tree.every(Part::Dotted).len(), 2);
    }

    #[test]
    fn a_syntax_error_names_its_line() {
        // Word's own editor points at the line and says what it wanted. The
        // line number is what a person needs to find it again.
        let (tree, complaints) = parse("Sub A()\r\n    x = \r\nEnd Sub\r\n");
        assert_eq!(complaints.len(), 1, "{complaints:?}");
        assert_eq!(complaints[0].line, 2);
        assert!(complaints[0].said.contains("Expected"), "{:?}", complaints[0].said);
        // And even then, nothing is lost.
        assert_eq!(tree.written(), "Sub A()\r\n    x = \r\nEnd Sub\r\n");
    }

    #[test]
    fn a_block_left_open_is_complained_about_and_not_guessed_at() {
        let (tree, complaints) = parse("Sub A()\r\n    x = 1\r\n");
        assert_eq!(complaints.len(), 1, "{complaints:?}");
        assert!(complaints[0].said.contains("End Sub"), "{:?}", complaints[0].said);
        assert_eq!(tree.written(), "Sub A()\r\n    x = 1\r\n");
    }

    #[test]
    fn a_line_nobody_here_understands_is_kept_whole() {
        // Written down as a complaint, kept as it was, and the rest of the
        // module read as usual. A parser that swallowed it would report a
        // module as read and be wrong.
        let source = "Sub A()\r\n    ]] nonsense ]]\r\n    x = 1\r\nEnd Sub\r\n";
        let (tree, complaints) = parse(source);
        assert_eq!(complaints.len(), 1, "{complaints:?}");
        assert_eq!(complaints[0].line, 2);
        assert_eq!(tree.written(), source);
        assert_eq!(tree.every(Part::Unknown).len(), 1);
        assert_eq!(tree.every(Part::Assign).len(), 1, "the line after it was not read");
    }

    #[test]
    fn a_label_and_a_line_number_are_both_places_to_jump_to() {
        assert_eq!(shape("Sorry:"), "Label(Sorry :)");
        assert_eq!(shape("10 x = 1"), "Label(10 Assign(Name(x) = Literal(1)))");
    }

    #[test]
    fn the_compilers_own_lines_are_kept_as_they_are() {
        let source = "#If Win64 Then\r\n    x = 1\r\n#Else\r\n    x = 2\r\n#End If\r\n";
        let (tree, complaints) = parse(source);
        assert!(complaints.is_empty(), "{complaints:?}");
        assert_eq!(tree.written(), source);
        assert_eq!(tree.every(Part::Directive).len(), 3);
        // What is inside them is ordinary Visual Basic and is read as such.
        assert_eq!(tree.every(Part::Assign).len(), 2);
    }

    #[test]
    fn a_statement_about_a_file_is_read_and_not_mistaken_for_a_name() {
        let source =
            "Open \"a.txt\" For Input As #1\r\nLine Input #1, s\r\nPrint #1, x; y\r\nClose #1\r\n";
        let (tree, complaints) = parse(source);
        assert!(complaints.is_empty(), "{complaints:?}");
        assert_eq!(tree.written(), source);
        assert_eq!(tree.every(Part::File).len(), 4);
        // And a name that happens to be one of those words is a name.
        assert_eq!(
            shape("x = Get(1)"),
            "Assign(Name(x) = Index(Name(Get) Arguments(( Argument(Literal(1)) ))))"
        );
    }

    #[test]
    fn an_empty_module_is_a_module() {
        let (tree, complaints) = parse("");
        assert!(complaints.is_empty());
        assert_eq!(tree.written(), "");
        assert_eq!(tree.part(), Some(Part::Module));
    }
}
