//! What a module is, once it has been read.
//!
//! # A tree of the words themselves
//!
//! Every leaf of the tree is a token, exactly as it was written, and every
//! branch says what the words under it are. Nothing is thrown away and
//! nothing is invented: printing the tree back out is walking it and putting
//! each token down again with the spaces that came before it, so a module
//! that parsed comes back byte for byte or the parse was wrong.
//!
//! That is a different shape from the tree an interpreter would want, where
//! a statement holds named fields and the punctuation is gone. It is the
//! right shape here, because the first thing that has to be provable is that
//! nothing was lost — and the second, when the interpreter is written, is
//! that it reads the same file everybody else does. A tree of named fields
//! can be built from this one; a file cannot be built back from that one.

use crate::lex::{Kind, Token};

/// What a branch of the tree is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Part {
    /// The whole module.
    Module,
    /// A line Word writes at the top of a module: `Attribute VB_Name = "…"`.
    Attribute,
    /// `Option Explicit` and its three companions.
    Option,
    /// A blank line, or a line with only a comment on it.
    Blank,
    /// `Dim`, `Private`, `Public`, `Global`, `Static` at the top of a module
    /// or inside a procedure.
    Declaration,
    /// One name of a declaration, with its dimensions and its type.
    Declared,
    /// `Const X As Long = 5`.
    Constant,
    /// `Type … End Type` and `Enum … End Enum`, and one line of either.
    TypeBlock,
    EnumBlock,
    Member,
    /// `Declare Function … Lib "…"`, which has no body.
    Declare,
    /// `Implements`, `Event`.
    Implements,
    Event,
    /// `Sub`, `Function`, `Property Get/Let/Set`, with everything in them.
    Procedure,
    /// What a procedure is given, and one of them.
    Parameters,
    Parameter,
    /// The statements inside a block.
    Body,
    /// `If … Then … End If`, and the parts of it.
    If,
    ElseIf,
    Else,
    /// An `If` written on one line, which has no `End If`.
    LineIf,
    For,
    ForEach,
    Do,
    While,
    Select,
    Case,
    With,
    /// An assignment, with or without `Set` or `Let` in front of it.
    Assign,
    /// A procedure called as a statement, with or without `Call`.
    Call,
    /// `GoTo`, `GoSub`, `Return`, `Resume`, `Exit`, `Stop`, `End`, `Erase`,
    /// `ReDim`, and the rest of the statements that are a word and a little.
    Jump,
    OnError,
    Exit,
    Redim,
    Erase,
    Stop,
    /// A statement that opens, closes, reads or writes a file.
    File,
    /// A label a `GoTo` can name, or a line number.
    Label,
    /// `#If`, `#Else`, `#End If`, `#Const`: the compiler's own statements.
    Directive,
    /// An expression, and the shapes one can take.
    Binary,
    Unary,
    Index,
    Dotted,
    Arguments,
    Argument,
    New,
    TypeOf,
    AddressOf,
    Parenthesised,
    Name,
    Literal,
    /// A line nothing here could make sense of, kept whole so that the module
    /// still comes back as it went in.
    Unknown,
}

/// A branch of the tree, or a word of the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Word(Token),
    Branch { part: Part, children: Vec<Node> },
}

impl Node {
    /// A branch of the part given.
    #[must_use]
    pub fn branch(part: Part, children: Vec<Node>) -> Self {
        Self::Branch { part, children }
    }

    /// What this branch is, or nothing for a word.
    #[must_use]
    pub fn part(&self) -> Option<Part> {
        match self {
            Self::Word(_) => None,
            Self::Branch { part, .. } => Some(*part),
        }
    }

    /// What is under it.
    #[must_use]
    pub fn children(&self) -> &[Self] {
        match self {
            Self::Word(_) => &[],
            Self::Branch { children, .. } => children,
        }
    }

    /// The token, for a leaf.
    #[must_use]
    pub fn token(&self) -> Option<&Token> {
        match self {
            Self::Word(token) => Some(token),
            Self::Branch { .. } => None,
        }
    }

    /// The source this was read from, byte for byte.
    #[must_use]
    pub fn written(&self) -> String {
        let mut out = String::new();
        self.write_into(&mut out);
        out
    }

    fn write_into(&self, out: &mut String) {
        match self {
            Self::Word(token) => {
                out.push_str(&token.before);
                out.push_str(&token.text);
            }
            Self::Branch { children, .. } => {
                for child in children {
                    child.write_into(out);
                }
            }
        }
    }

    /// Every branch of a part, however deep, in the order they were written.
    #[must_use]
    pub fn every(&self, part: Part) -> Vec<&Self> {
        let mut found = Vec::new();
        self.gather(part, &mut found);
        found
    }

    fn gather<'a>(&'a self, part: Part, found: &mut Vec<&'a Self>) {
        if self.part() == Some(part) {
            found.push(self);
        }
        for child in self.children() {
            child.gather(part, found);
        }
    }

    /// The first word under this branch, which for most statements is the one
    /// that says what it is.
    #[must_use]
    pub fn first_word(&self) -> Option<&Token> {
        match self {
            Self::Word(token) => (token.kind != Kind::NewLine).then_some(token),
            Self::Branch { children, .. } => children.iter().find_map(Self::first_word),
        }
    }

    /// The line it begins on.
    #[must_use]
    pub fn line(&self) -> usize {
        self.first_word().map_or(0, |token| token.line)
    }

    /// The name a declaration or a procedure gives, which is the first word
    /// that is not one of the words the language itself uses.
    #[must_use]
    pub fn named(&self) -> Option<&str> {
        self.children().iter().find_map(|child| match child {
            Self::Word(token) if token.kind == Kind::Word && !is_keyword(&token.text) => {
                Some(token.text.as_str())
            }
            _ => None,
        })
    }
}

/// Something the parser could not make sense of, and where.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Complaint {
    /// The line it is on, counting from one, which is how an editor counts
    /// and how Word's own says it.
    pub line: usize,
    /// What was expected, in the words Word uses: "Expected: expression".
    pub said: String,
}

impl core::fmt::Display for Complaint {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "line {}: {}", self.line, self.said)
    }
}

/// The words the language keeps for itself.
///
/// Used only to tell a name from a keyword when looking for what a statement
/// declares. Visual Basic lets a good many of these be used as names in the
/// right place, so this is a list for reading and not a rule for writing.
#[must_use]
pub fn is_keyword(word: &str) -> bool {
    const WORDS: [&str; 71] = [
        "as",
        "byref",
        "byval",
        "call",
        "case",
        "const",
        "declare",
        "dim",
        "do",
        "each",
        "else",
        "elseif",
        "end",
        "enum",
        "erase",
        "event",
        "exit",
        "explicit",
        "for",
        "friend",
        "function",
        "get",
        "global",
        "gosub",
        "goto",
        "if",
        "implements",
        "in",
        "is",
        "let",
        "lib",
        "like",
        "loop",
        "me",
        "mod",
        "new",
        "next",
        "not",
        "nothing",
        "on",
        "option",
        "optional",
        "paramarray",
        "preserve",
        "private",
        "property",
        "public",
        "redim",
        "rem",
        "resume",
        "return",
        "select",
        "set",
        "static",
        "step",
        "stop",
        "sub",
        "then",
        "to",
        "type",
        "typeof",
        "until",
        "wend",
        "while",
        "with",
        "withevents",
        "write",
        "alias",
        "base",
        "compare",
        "addressof",
    ];
    WORDS.iter().any(|keyword| word.eq_ignore_ascii_case(keyword))
}
