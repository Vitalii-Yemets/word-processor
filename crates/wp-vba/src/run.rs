//! Running what was read.
//!
//! # Over the tree the parser made
//!
//! There is no second tree and no byte code: the interpreter walks the one
//! [`crate::parse`] built, which still holds every word and every space. That
//! costs a little speed and buys the thing that matters here — what runs is
//! what the file says, and a statement that misbehaves can be pointed at in
//! the source by the line its words carry.
//!
//! # What a procedure is while it runs
//!
//! A frame: its own variables, whether an error is being watched for, and
//! where to carry on if one is. Module-level variables live in the machine
//! and outlive the call, because a macro that sets one in `AutoOpen` and
//! reads it later depends on that.
//!
//! Arguments are passed back as well as in. Visual Basic passes by reference
//! unless told otherwise, so a procedure that changes what it was given has
//! changed the caller's variable; that is done here by remembering where each
//! argument came from and writing the parameter back into it afterwards,
//! which behaves the same for everything a macro can actually write.
//!
//! # Errors
//!
//! A fault is a Rust `Err` travelling up until a statement list catches it.
//! What a list does with it is whatever `On Error` last said: nothing, in
//! which case it keeps travelling; carry on with the next statement; or jump
//! to a label and remember where to come back to if the handler says
//! `Resume`. `Err.Number` and `Err.Description` are the fault that was
//! caught, which is what a macro reads to decide what to do.
//!
//! # A macro that never stops
//!
//! Stops anyway. There is a budget of statements, and running out of it is a
//! fault like any other, because a program that hangs the window it is
//! running in is worse than one that says it gave up.

use std::collections::{HashMap, HashSet};

use crate::lex::{Kind as Word, Token};
use crate::library::{self, Host};
use crate::parse;
use crate::tree::{Complaint, Node, Part};
use crate::value::{self, Array, Fault, Given, Value};

/// How many statements one call may run before it is stopped.
const BUDGET: usize = 20_000_000;

/// What a variable was declared as, which says how a value is put into it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Whatever,
    Whole,
    Number,
    Money,
    Text,
    Truth,
    When,
    Object,
}

impl Kind {
    fn named(name: &str) -> Self {
        match name.to_ascii_lowercase().as_str() {
            "integer" | "long" | "longlong" | "byte" => Self::Whole,
            "single" | "double" => Self::Number,
            "currency" => Self::Money,
            "string" => Self::Text,
            "boolean" => Self::Truth,
            "date" => Self::When,
            "object" => Self::Object,
            _ => Self::Whatever,
        }
    }

    /// What a value becomes on the way into a variable of this kind.
    fn hold(self, value: Value) -> Result<Value, Fault> {
        if value.is_null() || matches!(value, Value::Array(_)) {
            return Ok(value);
        }
        Ok(match self {
            Self::Whatever | Self::Object => value,
            Self::Whole => Value::Long(value.whole()?),
            Self::Number => Value::Double(value.number()?),
            #[allow(clippy::cast_possible_truncation)]
            Self::Money => {
                Value::Currency(value::round_half_even(value.number()? * 10_000.0) as i64)
            }
            Self::Text => Value::Text(value.text()?),
            Self::Truth => Value::Boolean(value.truth()?),
            Self::When => match value {
                Value::Text(text) => {
                    Value::Date(crate::dates::from_text(&text).ok_or_else(|| Fault::of(13))?)
                }
                other => Value::Date(other.number()?),
            },
        })
    }

    /// What a variable of this kind holds before anything is put in it.
    fn empty(self) -> Value {
        match self {
            Self::Whatever => Value::Empty,
            Self::Whole => Value::Long(0),
            Self::Number => Value::Double(0.0),
            Self::Money => Value::Currency(0),
            Self::Text => Value::Text(String::new()),
            Self::Truth => Value::Boolean(false),
            Self::When => Value::Date(0.0),
            Self::Object => Value::Nothing,
        }
    }
}

/// Where a statement said to go next.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Flow {
    /// On to the next one.
    Next,
    /// Out of the procedure.
    Leave,
    /// Out of the innermost `For` or `Do`.
    LeaveFor,
    LeaveDo,
    /// To a label, which the enclosing lists look for in turn.
    Go(String),
    /// Back to where the error happened, or to the statement after it.
    Again(bool),
}

/// What `On Error` last said to do.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Watching {
    Nothing,
    Next,
    Label(String),
}

/// One procedure while it runs.
struct Frame {
    /// What it is called, which is also where a `Function` puts its answer.
    procedure: String,
    locals: HashMap<String, Value>,
    kinds: HashMap<String, Kind>,
    declared: HashSet<String>,
    watching: Watching,
    /// Where to carry on after a fault was handled.
    resume: Option<usize>,
    /// What a `With` block is about, innermost last.
    with: Vec<Value>,
}

impl Frame {
    fn new(procedure: &str) -> Self {
        Self {
            procedure: procedure.to_ascii_lowercase(),
            locals: HashMap::new(),
            kinds: HashMap::new(),
            declared: HashSet::new(),
            watching: Watching::Nothing,
            resume: None,
            with: Vec::new(),
        }
    }
}

/// A file a macro opened.
struct Opened {
    path: std::path::PathBuf,
    /// The lines it holds, for one being read.
    lines: Vec<String>,
    at: usize,
    /// What has been written to it, for one being written.
    written: Option<String>,
}

/// A module, read and ready to run.
pub struct Program {
    tree: Node,
}

impl core::fmt::Debug for Program {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Program").finish_non_exhaustive()
    }
}

impl Program {
    /// Reads a module, and says what could not be read.
    #[must_use]
    pub fn read(source: &str) -> (Self, Vec<Complaint>) {
        let (tree, complaints) = parse::parse(source);
        (Self { tree }, complaints)
    }

    /// The tree it was read into.
    #[must_use]
    pub fn tree(&self) -> &Node {
        &self.tree
    }

    /// A machine holding this program's own variables, which outlive one call
    /// the way a module's variables outlive one macro.
    #[must_use]
    pub fn machine<'a>(&'a self, host: &'a mut dyn Host) -> Machine<'a> {
        Machine::new(&self.tree, host)
    }

    /// Runs one macro and gives back what it answered.
    pub fn run(
        &self,
        name: &str,
        arguments: Vec<Value>,
        host: &mut dyn Host,
    ) -> Result<Value, Fault> {
        self.machine(host).run(name, arguments)
    }
}

/// A module while it is running: its own variables, and everything a macro
/// can reach that outlives one call.
pub struct Machine<'a> {
    module: &'a Node,
    procedures: HashMap<String, &'a Node>,
    globals: HashMap<String, Value>,
    kinds: HashMap<String, Kind>,
    constants: HashMap<String, Value>,
    statics: HashMap<String, HashMap<String, Value>>,
    host: &'a mut dyn Host,
    explicit: bool,
    base: i64,
    fault: Option<Fault>,
    files: HashMap<i64, Opened>,
    seed: u64,
    last_random: f64,
    budget: usize,
    started: bool,
}

impl core::fmt::Debug for Machine<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Machine")
            .field("procedures", &self.procedures.len())
            .finish_non_exhaustive()
    }
}

impl<'a> Machine<'a> {
    fn new(module: &'a Node, host: &'a mut dyn Host) -> Self {
        let mut machine = Self {
            module,
            procedures: HashMap::new(),
            globals: HashMap::new(),
            kinds: HashMap::new(),
            constants: HashMap::new(),
            statics: HashMap::new(),
            host,
            explicit: false,
            base: 0,
            fault: None,
            files: HashMap::new(),
            seed: 0x2545_F491_4F6C_DD1D,
            last_random: 0.0,
            budget: BUDGET,
            started: false,
        };
        for node in module.every(Part::Procedure) {
            if let Some(name) = procedure_name(node) {
                machine.procedures.insert(name.to_ascii_lowercase(), node);
            }
        }
        machine
    }

    /// What the procedure being run can see, for a debugger to show.
    ///
    /// Its own variables first and the module's underneath, because a local
    /// of the same name is the one the line being looked at is about.
    fn watched(&self, frame: &Frame) -> Vec<(String, Value)> {
        let mut out: Vec<(String, Value)> =
            frame.locals.iter().map(|(name, value)| (name.clone(), value.clone())).collect();
        for (name, value) in &self.globals {
            if !frame.locals.contains_key(name) {
                out.push((name.clone(), value.clone()));
            }
        }
        out.sort_by(|one, other| one.0.cmp(&other.0));
        out
    }

    /// Spends one of the statements this run is allowed.
    ///
    /// Counted at every statement and at every turn of every loop, because a
    /// loop with nothing in it — `Do` and `Loop` on two lines — runs no
    /// statements at all and would otherwise never stop.
    fn spend(&mut self) -> Result<(), Fault> {
        self.budget = self.budget.saturating_sub(1);
        if self.budget == 0 {
            return Err(Fault::saying(
                28,
                "This macro ran for longer than it is allowed to and was stopped",
            ));
        }
        Ok(())
    }

    /// Sets how many statements one run may use.
    ///
    /// For a test that means to run out of them, and for whoever is running
    /// a macro to decide how long is too long.
    pub fn allow(&mut self, statements: usize) {
        self.budget = statements;
    }

    /// The names of the macros this module offers.
    #[must_use]
    pub fn procedures(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .procedures
            .values()
            .filter_map(|node| procedure_name(node).map(str::to_owned))
            .collect();
        names.sort();
        names
    }

    /// Runs a macro by name.
    pub fn run(&mut self, name: &str, arguments: Vec<Value>) -> Result<Value, Fault> {
        self.prepare()?;
        let Some(node) = self.procedures.get(&name.to_ascii_lowercase()).copied() else {
            return Err(Fault::saying(5, &format!("Sub or Function not defined: {name}")));
        };
        let mut outside = Frame::new("");
        let given: Vec<Given> = arguments.into_iter().map(Given::just).collect();
        self.enter(node, given, &[], &mut outside)
    }

    /// Reads the module's own lines — its options, its constants and its
    /// variables — which have to be in place before anything runs.
    fn prepare(&mut self) -> Result<(), Fault> {
        if self.started {
            return Ok(());
        }
        self.started = true;
        let mut frame = Frame::new("");
        for statement in parts(self.module) {
            match statement.part() {
                Some(Part::Option) => self.option(statement),
                Some(Part::Constant) => self.declare_constants(statement, &mut frame)?,
                Some(Part::Declaration) => self.declare(statement, &mut frame, true)?,
                Some(Part::EnumBlock) => self.declare_enum(statement, &mut frame)?,
                _ => {}
            }
        }
        // Whatever the module's own lines declared belongs to the module.
        for (name, value) in frame.locals {
            self.globals.insert(name, value);
        }
        for (name, kind) in frame.kinds {
            self.kinds.insert(name, kind);
        }
        Ok(())
    }

    fn option(&mut self, statement: &Node) {
        let words: Vec<String> = parts(statement)
            .iter()
            .filter_map(|node| node.token().map(|token| token.text.to_ascii_lowercase()))
            .collect();
        if words.iter().any(|word| word == "explicit") {
            self.explicit = true;
        }
        if words.iter().any(|word| word == "base") {
            if let Some(number) = words.last().and_then(|word| word.parse::<i64>().ok()) {
                self.base = number;
            }
        }
    }

    // --- Procedures -----------------------------------------------------

    /// Calls a procedure, and writes back what it was passed by reference.
    fn enter(
        &mut self,
        node: &'a Node,
        arguments: Vec<Given>,
        places: &[Option<&'a Node>],
        caller: &mut Frame,
    ) -> Result<Value, Fault> {
        let name = procedure_name(node).unwrap_or_default().to_owned();
        let mut frame = Frame::new(&name);
        let wanted = parameters(node);
        if arguments.len() > wanted.len() && !wanted.iter().any(|one| one.rest) {
            return Err(Fault::of(450));
        }

        for (at, parameter) in wanted.iter().enumerate() {
            // By its name where the caller wrote one, and by its place
            // otherwise: `Foo Colour:="red"` puts it where it says.
            let value = match Given::find(&arguments, &parameter.name, at) {
                Some(value) => parameter.kind.hold(value.clone())?,
                None if parameter.optional || parameter.rest => match &parameter.default {
                    Some(default) => self.value_of(default, caller)?,
                    None => Value::Empty,
                },
                None => return Err(Fault::of(450)),
            };
            frame.locals.insert(parameter.name.to_ascii_lowercase(), value);
            frame.kinds.insert(parameter.name.to_ascii_lowercase(), parameter.kind);
            frame.declared.insert(parameter.name.to_ascii_lowercase());
        }

        // A function's answer is a variable of its own name, which starts
        // empty and is whatever it was left as.
        let answer_kind = returns(node);
        frame.locals.insert(name.to_ascii_lowercase(), answer_kind.empty());
        frame.kinds.insert(name.to_ascii_lowercase(), answer_kind);

        let body = parts(node).into_iter().find(|child| child.part() == Some(Part::Body));
        if let Some(body) = body {
            match self.run_body(body, &mut frame)? {
                Flow::Go(label) => {
                    return Err(Fault::saying(5, &format!("Label not defined: {label}")))
                }
                _ => {}
            }
        }

        // What was passed by reference goes back where it came from.
        for (at, parameter) in wanted.iter().enumerate() {
            if parameter.by_value || parameter.rest {
                continue;
            }
            let (Some(Some(place)), Some(value)) =
                (places.get(at), frame.locals.get(&parameter.name.to_ascii_lowercase()).cloned())
            else {
                continue;
            };
            self.assign_to(place, value, caller)?;
        }

        Ok(frame.locals.remove(&name.to_ascii_lowercase()).unwrap_or(Value::Empty))
    }

    // --- Statements -----------------------------------------------------

    /// Runs a list of statements, catching whatever `On Error` is watching
    /// for and finding the labels a `GoTo` names.
    fn run_body(&mut self, body: &'a Node, frame: &mut Frame) -> Result<Flow, Fault> {
        let statements = parts(body);
        let mut at = 0usize;
        while at < statements.len() {
            self.spend()?;
            // Before every statement, whoever is running this may look at it
            // and may stop it. A program with no debugger says yes.
            let line = statements[at].line();
            if line > 0 {
                let watched = self.watched(frame);
                self.host.watching(&watched);
                if !self.host.step(line) {
                    return Err(Fault::saying(18, "This macro was stopped before it had finished"));
                }
            }
            let outcome = self.run_statement(statements[at], frame);
            let flow = match outcome {
                Ok(flow) => flow,
                Err(fault) => match frame.watching.clone() {
                    Watching::Nothing => return Err(fault),
                    Watching::Next => {
                        self.fault = Some(fault);
                        at += 1;
                        continue;
                    }
                    Watching::Label(label) => {
                        self.fault = Some(fault);
                        frame.resume = Some(at);
                        match label_at(&statements, &label) {
                            Some(found) => {
                                at = found;
                                continue;
                            }
                            None => return Ok(Flow::Go(label)),
                        }
                    }
                },
            };

            match flow {
                Flow::Next => at += 1,
                Flow::Go(label) => match label_at(&statements, &label) {
                    Some(found) => at = found,
                    None => return Ok(Flow::Go(label)),
                },
                Flow::Again(next) => match frame.resume.take() {
                    Some(found) => {
                        self.fault = None;
                        at = if next { found + 1 } else { found };
                    }
                    None => return Ok(Flow::Again(next)),
                },
                other => return Ok(other),
            }
        }
        Ok(Flow::Next)
    }

    #[allow(clippy::too_many_lines)]
    fn run_statement(&mut self, statement: &'a Node, frame: &mut Frame) -> Result<Flow, Fault> {
        match statement.part() {
            Some(Part::Blank | Part::Attribute | Part::Option | Part::Declare)
            | Some(Part::Implements | Part::Event | Part::TypeBlock | Part::Directive) => {
                Ok(Flow::Next)
            }
            Some(Part::EnumBlock) => Ok(Flow::Next),
            Some(Part::Declaration) => {
                self.declare(statement, frame, false)?;
                Ok(Flow::Next)
            }
            Some(Part::Constant) => {
                self.declare_constants(statement, frame)?;
                Ok(Flow::Next)
            }
            Some(Part::Assign) => self.assign(statement, frame),
            Some(Part::Call) => {
                self.call_statement(statement, frame)?;
                Ok(Flow::Next)
            }
            Some(Part::If) => self.if_statement(statement, frame),
            Some(Part::LineIf) => self.line_if(statement, frame),
            Some(Part::For) => self.for_statement(statement, frame),
            Some(Part::ForEach) => self.for_each(statement, frame),
            Some(Part::Do) => self.do_statement(statement, frame),
            Some(Part::While) => self.while_statement(statement, frame),
            Some(Part::Select) => self.select(statement, frame),
            Some(Part::With) => self.with(statement, frame),
            Some(Part::Exit) => Ok(self.exit(statement)),
            Some(Part::Jump) => self.jump(statement, frame),
            Some(Part::OnError) => {
                self.on_error(statement, frame);
                Ok(Flow::Next)
            }
            Some(Part::Label) => {
                // A label may have a statement on the same line.
                match parts(statement).into_iter().find(|child| child.part().is_some()) {
                    Some(inner) => self.run_statement(inner, frame),
                    None => Ok(Flow::Next),
                }
            }
            Some(Part::Redim) => {
                self.redim(statement, frame)?;
                Ok(Flow::Next)
            }
            Some(Part::Erase) => {
                self.erase(statement, frame)?;
                Ok(Flow::Next)
            }
            // `End` stops the macro; `Stop` would wait for a debugger, and
            // with none to wait for it stops as well.
            Some(Part::Stop) => Ok(Flow::Leave),
            Some(Part::File) => {
                self.file_statement(statement, frame)?;
                Ok(Flow::Next)
            }
            Some(Part::Unknown) => Err(Fault::saying(
                5,
                &format!("This line was never read: {}", statement.written().trim()),
            )),
            _ => Ok(Flow::Next),
        }
    }

    /// `Dim`, `Private`, `Public`, `Static` and the rest.
    fn declare(
        &mut self,
        statement: &'a Node,
        frame: &mut Frame,
        global: bool,
    ) -> Result<(), Fault> {
        let is_static = parts(statement)
            .iter()
            .any(|node| node.token().is_some_and(|token| token.is("static")));
        for declared in statement.every(Part::Declared) {
            let Some(name) = first_name(declared) else { continue };
            let lowered = name.to_ascii_lowercase();
            let kind = declared_kind(declared);
            let value = match bounds_of(declared) {
                Some(brackets) => {
                    let bounds = self.bounds(brackets, frame)?;
                    Value::Array(Box::new(Array::filled(bounds, &kind.empty())))
                }
                None => kind.empty(),
            };

            if global {
                self.globals.insert(lowered.clone(), value);
                self.kinds.insert(lowered.clone(), kind);
            } else if is_static {
                let held = self.statics.entry(frame.procedure.clone()).or_default();
                held.entry(lowered.clone()).or_insert(value);
                frame.kinds.insert(lowered.clone(), kind);
            } else {
                frame.locals.insert(lowered.clone(), value);
                frame.kinds.insert(lowered.clone(), kind);
            }
            frame.declared.insert(lowered);
        }
        Ok(())
    }

    /// The bounds in the brackets of a declaration: `(5)`, `(1 To 5)`,
    /// `(1 To 5, 1 To 2)`.
    fn bounds(&mut self, brackets: &'a Node, frame: &mut Frame) -> Result<Vec<(i64, i64)>, Fault> {
        let mut bounds = Vec::new();
        for argument in
            parts(brackets).into_iter().filter(|node| node.part() == Some(Part::Argument))
        {
            let inside = parts(argument);
            let numbers: Vec<&Node> =
                inside.iter().copied().filter(|node| node.part().is_some()).collect();
            match numbers.len() {
                0 => bounds.push((self.base, self.base - 1)),
                1 => {
                    let high = self.value_of(numbers[0], frame)?.whole()?;
                    bounds.push((self.base, high));
                }
                _ => {
                    let low = self.value_of(numbers[0], frame)?.whole()?;
                    let high = self.value_of(numbers[1], frame)?.whole()?;
                    bounds.push((low, high));
                }
            }
        }
        if bounds.is_empty() {
            bounds.push((self.base, self.base - 1));
        }
        Ok(bounds)
    }

    fn declare_constants(&mut self, statement: &'a Node, frame: &mut Frame) -> Result<(), Fault> {
        for declared in statement.every(Part::Declared) {
            let Some(name) = first_name(declared) else { continue };
            let expression = parts(declared).into_iter().rev().find(|node| node.part().is_some());
            let Some(expression) = expression else { continue };
            let value = self.value_of(expression, frame)?;
            let value = declared_kind(declared).hold(value)?;
            self.constants.insert(name.to_ascii_lowercase(), value);
            frame.declared.insert(name.to_ascii_lowercase());
        }
        Ok(())
    }

    /// `Enum` members are constants, and one with no value is the one before
    /// it and one more.
    fn declare_enum(&mut self, statement: &'a Node, frame: &mut Frame) -> Result<(), Fault> {
        let mut next = 0i64;
        for member in statement.every(Part::Member) {
            let Some(name) = first_name(member) else { continue };
            let expression = parts(member).into_iter().find(|node| node.part().is_some());
            let value = match expression {
                Some(expression) => self.value_of(expression, frame)?.whole()?,
                None => next,
            };
            next = value + 1;
            self.constants.insert(name.to_ascii_lowercase(), Value::Long(value));
        }
        Ok(())
    }

    fn assign(&mut self, statement: &'a Node, frame: &mut Frame) -> Result<Flow, Fault> {
        let inside = parts(statement);
        let equals = inside
            .iter()
            .position(|node| node.token().is_some_and(|token| token.symbol("=")))
            .ok_or_else(|| Fault::of(13))?;
        let target = inside[..equals]
            .iter()
            .copied()
            .find(|node| node.part().is_some())
            .ok_or_else(|| Fault::of(13))?;
        let expression = inside[equals + 1..]
            .iter()
            .copied()
            .find(|node| node.part().is_some())
            .ok_or_else(|| Fault::of(13))?;

        let value = self.value_of(expression, frame)?;
        self.assign_to(target, value, frame)?;
        Ok(Flow::Next)
    }

    /// Puts a value where a target says.
    fn assign_to(
        &mut self,
        target: &'a Node,
        value: Value,
        frame: &mut Frame,
    ) -> Result<(), Fault> {
        match target.part() {
            Some(Part::Name) => {
                let name = name_of(target).ok_or_else(|| Fault::of(13))?.to_ascii_lowercase();
                self.set(&name, value, frame)
            }
            Some(Part::Index) => {
                let inside = parts(target);
                let base = inside.first().copied().ok_or_else(|| Fault::of(13))?;
                let name = name_of(base).ok_or_else(|| Fault::of(13))?.to_ascii_lowercase();
                let subscripts = self.subscripts(target, frame)?;
                let mut held = self.get(&name, frame)?;
                let Value::Array(array) = &mut held else { return Err(Fault::of(13)) };
                let at = array.at(&subscripts)?;
                array.values[at] = value;
                self.set_exactly(&name, held, frame)
            }
            Some(Part::Dotted) => {
                let inside = parts(target);
                let member = last_word(&inside).unwrap_or_default();
                let head = inside.first().copied().filter(|child| child.part().is_some());

                if head.is_some_and(|head| {
                    head.part() == Some(Part::Name)
                        && first_name(head).is_some_and(|name| name.eq_ignore_ascii_case("err"))
                }) {
                    let mut fault = self.fault.clone().unwrap_or(Fault::saying(0, ""));
                    match member.to_ascii_lowercase().as_str() {
                        "number" => fault.number = i32::try_from(value.whole()?).unwrap_or(0),
                        "description" => fault.description = value.text()?,
                        _ => return Err(Fault::of(424)),
                    }
                    self.fault = Some(fault);
                    return Ok(());
                }

                let object = match head {
                    Some(head) => self.value_of(head, frame)?,
                    None => frame.with.last().cloned().unwrap_or(Value::Nothing),
                };
                match object {
                    Value::Object(handle) => self.host.set_member(&handle, &member, value),
                    Value::Nothing => Err(Fault::of(91)),
                    _ => Err(Fault::of(424)),
                }
            }
            _ => Err(Fault::of(13)),
        }
    }

    /// Puts a value into a variable, in whatever kind it was declared as.
    fn set(&mut self, name: &str, value: Value, frame: &mut Frame) -> Result<(), Fault> {
        let kind = frame
            .kinds
            .get(name)
            .or_else(|| self.kinds.get(name))
            .copied()
            .unwrap_or(Kind::Whatever);
        self.set_exactly(name, kind.hold(value)?, frame)
    }

    /// And the same without asking what kind it is, for an array whose cell
    /// has already been put in place.
    fn set_exactly(&mut self, name: &str, value: Value, frame: &mut Frame) -> Result<(), Fault> {
        if let Some(held) =
            self.statics.get_mut(&frame.procedure).and_then(|held| held.get_mut(name))
        {
            *held = value;
            return Ok(());
        }
        if frame.locals.contains_key(name) || !self.globals.contains_key(name) {
            if self.explicit && !frame.declared.contains(name) && !frame.locals.contains_key(name) {
                return Err(Fault::saying(5, &format!("Variable not defined: {name}")));
            }
            frame.locals.insert(name.to_owned(), value);
            return Ok(());
        }
        self.globals.insert(name.to_owned(), value);
        Ok(())
    }

    /// What a name holds.
    fn get(&mut self, name: &str, frame: &mut Frame) -> Result<Value, Fault> {
        if let Some(value) = frame.locals.get(name) {
            return Ok(value.clone());
        }
        if let Some(value) = self.statics.get(&frame.procedure).and_then(|held| held.get(name)) {
            return Ok(value.clone());
        }
        if let Some(value) = self.globals.get(name) {
            return Ok(value.clone());
        }
        if let Some(value) = self.constants.get(name) {
            return Ok(value.clone());
        }
        if let Some(value) = library::constant(name) {
            return Ok(value);
        }
        // A name with nothing in it may be a procedure taking no arguments.
        if let Some(node) = self.procedures.get(name).copied() {
            return self.enter(node, Vec::new(), &[], frame);
        }
        if let Some(answer) = self.builtin(name, &[])? {
            return Ok(answer);
        }
        // A name the program running the macro answers to: `ActiveDocument`,
        // `Selection`, and the rest of what a macro starts from.
        if let Some(value) = self.host.root(name) {
            return Ok(value);
        }
        if self.explicit {
            return Err(Fault::saying(5, &format!("Variable not defined: {name}")));
        }
        Ok(Value::Empty)
    }

    // --- The blocks -----------------------------------------------------

    fn if_statement(&mut self, statement: &'a Node, frame: &mut Frame) -> Result<Flow, Fault> {
        let inside = parts(statement);
        let condition = inside
            .iter()
            .copied()
            .find(|node| node.part().is_some())
            .ok_or_else(|| Fault::of(13))?;
        if self.value_of(condition, frame)?.truth()? {
            let body = inside
                .iter()
                .copied()
                .find(|node| node.part() == Some(Part::Body))
                .ok_or_else(|| Fault::of(13))?;
            return self.run_body(body, frame);
        }
        for branch in inside.iter().copied() {
            match branch.part() {
                Some(Part::ElseIf) => {
                    let pieces = parts(branch);
                    let condition = pieces
                        .iter()
                        .copied()
                        .find(|node| node.part().is_some())
                        .ok_or_else(|| Fault::of(13))?;
                    if self.value_of(condition, frame)?.truth()? {
                        let body = pieces
                            .iter()
                            .copied()
                            .find(|node| node.part() == Some(Part::Body))
                            .ok_or_else(|| Fault::of(13))?;
                        return self.run_body(body, frame);
                    }
                }
                Some(Part::Else) => {
                    let body = parts(branch)
                        .into_iter()
                        .find(|node| node.part() == Some(Part::Body))
                        .ok_or_else(|| Fault::of(13))?;
                    return self.run_body(body, frame);
                }
                _ => {}
            }
        }
        Ok(Flow::Next)
    }

    /// `If a Then b = 1 Else b = 2`, which is the whole statement.
    fn line_if(&mut self, statement: &'a Node, frame: &mut Frame) -> Result<Flow, Fault> {
        let inside = parts(statement);
        let condition = inside
            .iter()
            .copied()
            .find(|node| node.part().is_some())
            .ok_or_else(|| Fault::of(13))?;
        let truth = self.value_of(condition, frame)?.truth()?;

        let mut after_else = false;
        for node in inside.iter().copied() {
            if node.token().is_some_and(|token| token.is("else")) {
                after_else = true;
                continue;
            }
            if node.part().is_none() || core::ptr::eq(node, condition) {
                continue;
            }
            if node.part() == Some(Part::Blank) {
                continue;
            }
            if truth != after_else {
                return self.run_statement(node, frame);
            }
        }
        Ok(Flow::Next)
    }

    fn for_statement(&mut self, statement: &'a Node, frame: &mut Frame) -> Result<Flow, Fault> {
        let inside = parts(statement);
        let pieces: Vec<&Node> =
            inside.iter().copied().filter(|node| node.part().is_some()).collect();
        let [counter, from, to, rest @ ..] = pieces.as_slice() else {
            return Err(Fault::of(13));
        };
        let body = pieces.iter().copied().find(|node| node.part() == Some(Part::Body));

        let mut at = self.value_of(from, frame)?.number()?;
        let last = self.value_of(to, frame)?.number()?;
        let step = match rest.iter().find(|node| node.part() != Some(Part::Body)) {
            Some(node) => self.value_of(node, frame)?.number()?,
            None => 1.0,
        };
        if step == 0.0 {
            return Err(Fault::of(5));
        }

        loop {
            self.spend()?;
            if (step > 0.0 && at > last) || (step < 0.0 && at < last) {
                return Ok(Flow::Next);
            }
            self.assign_to(counter, whole_or_double(at), frame)?;
            if let Some(body) = body {
                match self.run_body(body, frame)? {
                    Flow::Next => {}
                    Flow::LeaveFor => return Ok(Flow::Next),
                    other => return Ok(other),
                }
            }
            // The counter is read back, because the body may have moved it.
            at = self.value_of(counter, frame)?.number()? + step;
        }
    }

    fn for_each(&mut self, statement: &'a Node, frame: &mut Frame) -> Result<Flow, Fault> {
        let pieces: Vec<&Node> =
            parts(statement).into_iter().filter(|node| node.part().is_some()).collect();
        let [item, over, rest @ ..] = pieces.as_slice() else { return Err(Fault::of(13)) };
        let body = rest.iter().copied().find(|node| node.part() == Some(Part::Body));

        // A collection is walked by asking the program that owns it what it
        // holds, which is the one question `For Each` needs of an object.
        let items = match self.value_of(over, frame)? {
            Value::Array(array) => array.values,
            Value::Object(handle) => self.host.items(&handle)?,
            _ => return Err(Fault::of(424)),
        };
        for value in items {
            self.spend()?;
            self.assign_to(item, value, frame)?;
            if let Some(body) = body {
                match self.run_body(body, frame)? {
                    Flow::Next => {}
                    Flow::LeaveFor => return Ok(Flow::Next),
                    other => return Ok(other),
                }
            }
        }
        Ok(Flow::Next)
    }

    fn do_statement(&mut self, statement: &'a Node, frame: &mut Frame) -> Result<Flow, Fault> {
        let inside = parts(statement);
        let body = inside.iter().copied().find(|node| node.part() == Some(Part::Body));
        // `Do While x` and `Do Until x` are tested at the top; `Loop While x`
        // and `Loop Until x` at the bottom, so the body runs once first.
        let (top, bottom) = do_conditions(&inside);

        loop {
            self.spend()?;
            if let Some((condition, until)) = top {
                let truth = self.value_of(condition, frame)?.truth()?;
                if truth == until {
                    return Ok(Flow::Next);
                }
            }
            if let Some(body) = body {
                match self.run_body(body, frame)? {
                    Flow::Next => {}
                    Flow::LeaveDo => return Ok(Flow::Next),
                    other => return Ok(other),
                }
            }
            if let Some((condition, until)) = bottom {
                let truth = self.value_of(condition, frame)?.truth()?;
                if truth == until {
                    return Ok(Flow::Next);
                }
            }
            if top.is_none() && bottom.is_none() {
                // `Do … Loop` with no condition at either end, which only a
                // `Exit Do` or a `GoTo` leaves.
                continue;
            }
        }
    }

    fn while_statement(&mut self, statement: &'a Node, frame: &mut Frame) -> Result<Flow, Fault> {
        let inside = parts(statement);
        let condition = inside
            .iter()
            .copied()
            .find(|node| node.part().is_some() && node.part() != Some(Part::Body))
            .ok_or_else(|| Fault::of(13))?;
        let body = inside.iter().copied().find(|node| node.part() == Some(Part::Body));
        while self.value_of(condition, frame)?.truth()? {
            self.spend()?;
            if let Some(body) = body {
                match self.run_body(body, frame)? {
                    Flow::Next => {}
                    Flow::LeaveDo => return Ok(Flow::Next),
                    other => return Ok(other),
                }
            }
        }
        Ok(Flow::Next)
    }

    fn select(&mut self, statement: &'a Node, frame: &mut Frame) -> Result<Flow, Fault> {
        let inside = parts(statement);
        let subject = inside
            .iter()
            .copied()
            .find(|node| node.part().is_some() && node.part() != Some(Part::Body))
            .ok_or_else(|| Fault::of(13))?;
        let subject = self.value_of(subject, frame)?;

        for branch in inside.iter().copied().filter(|node| node.part() == Some(Part::Case)) {
            if self.case_matches(branch, &subject, frame)? {
                let body = parts(branch)
                    .into_iter()
                    .find(|node| node.part() == Some(Part::Body))
                    .ok_or_else(|| Fault::of(13))?;
                return self.run_body(body, frame);
            }
        }
        Ok(Flow::Next)
    }

    /// Whether one `Case` line is about the value being looked at.
    fn case_matches(
        &mut self,
        branch: &'a Node,
        subject: &Value,
        frame: &mut Frame,
    ) -> Result<bool, Fault> {
        let inside = parts(branch);
        if inside.iter().any(|node| node.token().is_some_and(|token| token.is("else"))) {
            return Ok(true);
        }

        let mut at = 0usize;
        while at < inside.len() {
            let node = inside[at];
            // `Case Is > 5`: the comparison with its left side left out.
            if node.token().is_some_and(|token| token.is("is")) {
                let operator = inside.get(at + 1).and_then(|node| node.token()).cloned();
                let against = inside.get(at + 2).copied();
                if let (Some(operator), Some(against)) = (operator, against) {
                    let against = self.value_of(against, frame)?;
                    if compares(subject, &operator.text, &against)? {
                        return Ok(true);
                    }
                }
                at += 3;
                continue;
            }
            if node.part().is_some() && node.part() != Some(Part::Body) {
                let value = self.value_of(node, frame)?;
                // `Case 3 To 9`.
                let is_range = inside
                    .get(at + 1)
                    .is_some_and(|next| next.token().is_some_and(|token| token.is("to")));
                if is_range {
                    let Some(top) = inside.get(at + 2).copied() else { return Ok(false) };
                    let top = self.value_of(top, frame)?;
                    let low = value::compare(subject, &value)?;
                    let high = value::compare(subject, &top)?;
                    if low.is_some_and(|one| one != core::cmp::Ordering::Less)
                        && high.is_some_and(|one| one != core::cmp::Ordering::Greater)
                    {
                        return Ok(true);
                    }
                    at += 3;
                    continue;
                }
                if value::compare(subject, &value)? == Some(core::cmp::Ordering::Equal) {
                    return Ok(true);
                }
            }
            at += 1;
        }
        Ok(false)
    }

    fn with(&mut self, statement: &'a Node, frame: &mut Frame) -> Result<Flow, Fault> {
        let inside = parts(statement);
        let subject = inside
            .iter()
            .copied()
            .find(|node| node.part().is_some() && node.part() != Some(Part::Body))
            .ok_or_else(|| Fault::of(13))?;
        let value = self.value_of(subject, frame)?;
        frame.with.push(value);
        let body = inside.iter().copied().find(|node| node.part() == Some(Part::Body));
        let flow = match body {
            Some(body) => self.run_body(body, frame)?,
            None => Flow::Next,
        };
        frame.with.pop();
        Ok(flow)
    }

    fn exit(&self, statement: &Node) -> Flow {
        let words: Vec<String> = parts(statement)
            .iter()
            .filter_map(|node| node.token().map(|token| token.text.to_ascii_lowercase()))
            .collect();
        if words.iter().any(|word| word == "for") {
            return Flow::LeaveFor;
        }
        if words.iter().any(|word| word == "do") {
            return Flow::LeaveDo;
        }
        Flow::Leave
    }

    fn jump(&mut self, statement: &'a Node, frame: &mut Frame) -> Result<Flow, Fault> {
        let words: Vec<&Token> = parts(statement).iter().filter_map(|node| node.token()).collect();
        let first = words.first().map(|token| token.text.to_ascii_lowercase()).unwrap_or_default();
        match first.as_str() {
            "goto" => {
                Ok(Flow::Go(words.get(1).map(|token| token.text.clone()).unwrap_or_default()))
            }
            "resume" => {
                let next = words.get(1).map(|token| token.text.to_ascii_lowercase());
                match next.as_deref() {
                    Some("next") => Ok(Flow::Again(true)),
                    Some(label) if !label.is_empty() => Ok(Flow::Go(label.to_owned())),
                    _ => Ok(Flow::Again(false)),
                }
            }
            // `GoSub` and `Return` are from before procedures had names, and
            // nothing written since 1990 uses them.
            "gosub" | "return" => {
                let _ = frame;
                Err(Fault::saying(5, "GoSub and Return are not run by this program"))
            }
            _ => Ok(Flow::Next),
        }
    }

    fn on_error(&mut self, statement: &Node, frame: &mut Frame) {
        let words: Vec<String> = parts(statement)
            .iter()
            .filter_map(|node| node.token().map(|token| token.text.to_ascii_lowercase()))
            .collect();
        if words.iter().any(|word| word == "resume") {
            frame.watching = Watching::Next;
            return;
        }
        if let Some(at) = words.iter().position(|word| word == "goto") {
            match words.get(at + 1).map(String::as_str) {
                Some("0") => frame.watching = Watching::Nothing,
                Some(label) => frame.watching = Watching::Label(label.to_owned()),
                None => frame.watching = Watching::Nothing,
            }
        }
    }

    fn redim(&mut self, statement: &'a Node, frame: &mut Frame) -> Result<(), Fault> {
        let keep = parts(statement)
            .iter()
            .any(|node| node.token().is_some_and(|token| token.is("preserve")));
        for declared in statement.every(Part::Declared) {
            let Some(name) = first_name(declared) else { continue };
            let lowered = name.to_ascii_lowercase();
            let Some(brackets) = bounds_of(declared) else { continue };
            let bounds = self.bounds(brackets, frame)?;
            let mut made = Array::filled(bounds, &declared_kind(declared).empty());

            if keep {
                // What was there stays where it was, by subscript rather than
                // by position: growing an array must not shuffle it.
                if let Ok(Value::Array(old)) = self.get(&lowered, frame) {
                    copy_across(&old, &mut made);
                }
            }
            frame.declared.insert(lowered.clone());
            self.set_exactly(&lowered, Value::Array(Box::new(made)), frame)?;
        }
        Ok(())
    }

    fn erase(&mut self, statement: &'a Node, frame: &mut Frame) -> Result<(), Fault> {
        for node in parts(statement).into_iter().filter(|node| node.part().is_some()) {
            let Some(name) = first_name(node) else { continue };
            let lowered = name.to_ascii_lowercase();
            let kind = frame.kinds.get(&lowered).or_else(|| self.kinds.get(&lowered)).copied();
            if let Ok(Value::Array(array)) = self.get(&lowered, frame) {
                let emptied =
                    Array::filled(array.bounds.clone(), &kind.unwrap_or(Kind::Whatever).empty());
                self.set_exactly(&lowered, Value::Array(Box::new(emptied)), frame)?;
            }
        }
        Ok(())
    }

    // --- Expressions ----------------------------------------------------

    fn value_of(&mut self, node: &'a Node, frame: &mut Frame) -> Result<Value, Fault> {
        match node.part() {
            Some(Part::Literal) => literal(node),
            Some(Part::Name) => {
                let name = name_of(node).ok_or_else(|| Fault::of(13))?.to_ascii_lowercase();
                self.get(&name, frame)
            }
            Some(Part::Parenthesised) => {
                let inner = parts(node)
                    .into_iter()
                    .find(|child| child.part().is_some())
                    .ok_or_else(|| Fault::of(13))?;
                self.value_of(inner, frame)
            }
            Some(Part::Binary) => self.binary(node, frame),
            Some(Part::Unary) => self.unary(node, frame),
            Some(Part::Index) => self.index(node, frame),
            Some(Part::Dotted) => self.dotted(node, frame),
            Some(Part::New) => Err(Fault::saying(
                424,
                "There are no objects to make yet: the document's own are a later item",
            )),
            Some(Part::TypeOf) => Err(Fault::of(424)),
            Some(Part::AddressOf) => Err(Fault::of(5)),
            Some(Part::Argument) => {
                let inner = parts(node).into_iter().find(|child| child.part().is_some());
                match inner {
                    Some(inner) => self.value_of(inner, frame),
                    None => Ok(Value::Empty),
                }
            }
            _ => Err(Fault::of(13)),
        }
    }

    fn binary(&mut self, node: &'a Node, frame: &mut Frame) -> Result<Value, Fault> {
        let inside = parts(node);
        let [left, operator, right] = inside.as_slice() else { return Err(Fault::of(13)) };
        let operator = operator.token().ok_or_else(|| Fault::of(13))?.text.to_ascii_lowercase();

        // The two that do not always want both sides: everything else does.
        let left_value = self.value_of(left, frame)?;
        if operator == "and" && !left_value.is_null() && !left_value.truth()? {
            return Ok(Value::Boolean(false));
        }
        if operator == "or" && !left_value.is_null() && left_value.truth()? {
            return Ok(Value::Boolean(true));
        }
        let right_value = self.value_of(right, frame)?;

        // `Is` is the one operator that is about the objects themselves.
        // Everywhere else an object stands for what it says: Word gives a
        // range's text where a string is wanted, and a macro writing
        // `MsgBox Selection` depends on it.
        let (left_value, right_value) = if operator == "is" {
            (left_value, right_value)
        } else {
            (self.plain(left_value)?, self.plain(right_value)?)
        };

        Ok(match operator.as_str() {
            "+" => value::add(&left_value, &right_value)?,
            "-" => value::subtract(&left_value, &right_value)?,
            "*" => value::multiply(&left_value, &right_value)?,
            "/" => value::divide(&left_value, &right_value)?,
            "\\" => value::divide_whole(&left_value, &right_value)?,
            "mod" => value::remainder(&left_value, &right_value)?,
            "^" => value::power(&left_value, &right_value)?,
            "&" => value::join(&left_value, &right_value)?,
            "=" | "<>" | "<" | ">" | "<=" | ">=" | "is" => {
                Value::Boolean(compares(&left_value, &operator, &right_value)?)
            }
            "like" => Value::Boolean(matches_pattern(&left_value.text()?, &right_value.text()?)),
            "and" | "or" | "xor" | "eqv" | "imp" => {
                if left_value.is_null() || right_value.is_null() {
                    return Ok(Value::Null);
                }
                let (one, other) = (left_value.whole()?, right_value.whole()?);
                Value::Long(match operator.as_str() {
                    "and" => one & other,
                    "or" => one | other,
                    "xor" => one ^ other,
                    "eqv" => !(one ^ other),
                    _ => !one | other,
                })
            }
            _ => return Err(Fault::of(13)),
        })
    }

    fn unary(&mut self, node: &'a Node, frame: &mut Frame) -> Result<Value, Fault> {
        let inside = parts(node);
        let [operator, operand] = inside.as_slice() else { return Err(Fault::of(13)) };
        let operator = operator.token().ok_or_else(|| Fault::of(13))?.text.to_ascii_lowercase();
        let value = self.value_of(operand, frame)?;
        if value.is_null() {
            return Ok(Value::Null);
        }
        Ok(match operator.as_str() {
            "-" => value::subtract(&Value::Long(0), &value)?,
            "+" => value,
            "not" => match value {
                Value::Boolean(on) => Value::Boolean(!on),
                other => Value::Long(!other.whole()?),
            },
            _ => return Err(Fault::of(13)),
        })
    }

    /// `a(1)`, which is an array's cell or a call, depending on what `a` is.
    fn index(&mut self, node: &'a Node, frame: &mut Frame) -> Result<Value, Fault> {
        let inside = parts(node);
        let base = inside.first().copied().ok_or_else(|| Fault::of(13))?;
        if base.part() == Some(Part::Name) {
            let name = name_of(base).ok_or_else(|| Fault::of(13))?.to_ascii_lowercase();
            let held = frame
                .locals
                .get(&name)
                .or_else(|| self.statics.get(&frame.procedure).and_then(|held| held.get(&name)))
                .or_else(|| self.globals.get(&name))
                .cloned();
            if let Some(Value::Array(array)) = held {
                let subscripts = self.subscripts(node, frame)?;
                let at = array.at(&subscripts)?;
                return Ok(array.values[at].clone());
            }
            // A collection in brackets is the one it holds: `Documents(1)`
            // is `Documents.Item(1)`, which is what Word means by it.
            if let Some(Value::Object(handle)) = held {
                let given = self.arguments_of(node, frame)?;
                return self.host.member(&handle, "Item", &given);
            }
            return self.call_named(&name, node, frame);
        }
        // `a.b(1)`: the brackets belong to the member, which may be a method
        // taking arguments or a collection being asked for one of its own.
        if base.part() == Some(Part::Dotted) {
            let given = self.arguments_of(node, frame)?;
            return self.member_of(base, &given, frame);
        }
        Err(Fault::of(424))
    }

    /// The arguments in the brackets of an index, with their names.
    fn arguments_of(&mut self, node: &'a Node, frame: &mut Frame) -> Result<Vec<Given>, Fault> {
        let arguments: Vec<&Node> = parts(node)
            .into_iter()
            .filter(|child| child.part() == Some(Part::Arguments))
            .flat_map(|child| parts(child))
            .filter(|child| child.part() == Some(Part::Argument))
            .collect();
        self.given(&arguments, frame)
    }

    /// The numbers inside the brackets of an index.
    fn subscripts(&mut self, node: &'a Node, frame: &mut Frame) -> Result<Vec<i64>, Fault> {
        let mut out = Vec::new();
        for argument in node.every(Part::Argument) {
            // Only the arguments of this index, not of one nested in it.
            if !parts(node)
                .iter()
                .any(|child| child.part() == Some(Part::Arguments) && ptr_holds(child, argument))
            {
                continue;
            }
            let value = self.value_of(argument, frame)?;
            out.push(value.whole()?);
        }
        Ok(out)
    }

    /// `a.b`, whatever `a` turns out to be.
    fn dotted(&mut self, node: &'a Node, frame: &mut Frame) -> Result<Value, Fault> {
        self.member_of(node, &[], frame)
    }

    /// `a.b` and `a.b(1, 2)`, which are one question asked with and without
    /// arguments: whether a member is a property or a method is the business
    /// of the program that owns the object, not of the language.
    fn member_of(
        &mut self,
        node: &'a Node,
        given: &[Given],
        frame: &mut Frame,
    ) -> Result<Value, Fault> {
        let inside = parts(node);
        let member = last_word(&inside).unwrap_or_default();
        let head = inside.first().copied().filter(|child| child.part().is_some());

        // A full stop with nothing in front of it belongs to the `With` block
        // it is written inside.
        let Some(head) = head else {
            let Some(Value::Object(handle)) = frame.with.last().cloned() else {
                return Err(Fault::saying(
                    91,
                    "This full stop is not inside a With block that has an object",
                ));
            };
            return self.host.member(&handle, &member, given);
        };

        // `Err` is the language's own and not the program's.
        if head.part() == Some(Part::Name)
            && first_name(head).is_some_and(|name| name.eq_ignore_ascii_case("err"))
        {
            let fault = self.fault.clone().unwrap_or(Fault::saying(0, ""));
            return Ok(match member.to_ascii_lowercase().as_str() {
                "number" => Value::Long(i64::from(fault.number)),
                "description" => Value::Text(fault.description),
                "source" => Value::Text(String::new()),
                _ => return Err(Fault::of(424)),
            });
        }

        match self.value_of(head, frame)? {
            Value::Object(handle) => self.host.member(&handle, &member, given),
            Value::Nothing => Err(Fault::of(91)),
            _ => Err(Fault::saying(
                424,
                &format!("{} is not an object, so it has no {member}", head.written().trim()),
            )),
        }
    }

    /// A statement that is a call: `MsgBox "Hello"`, `Foo 1, 2`, `Call Foo`.
    fn call_statement(&mut self, statement: &'a Node, frame: &mut Frame) -> Result<Value, Fault> {
        let inside: Vec<&Node> =
            parts(statement).into_iter().filter(|node| node.part().is_some()).collect();
        let Some(first) = inside.first().copied() else { return Ok(Value::Empty) };

        // `Debug.Print x`, which is the one object every macro uses.
        if first.part() == Some(Part::Dotted) {
            let (head, member) = dotted(first);
            if head.eq_ignore_ascii_case("debug") && member.eq_ignore_ascii_case("print") {
                let mut said = Vec::new();
                for argument in inside.iter().skip(1).flat_map(|node| node.every(Part::Argument)) {
                    let value = self.value_of(argument, frame)?;
                    said.push(self.plain(value)?.text()?);
                }
                self.host.note(&said.join(" "));
                return Ok(Value::Empty);
            }
            if head.eq_ignore_ascii_case("err") && !head.is_empty() {
                return self.err_member(&member, &inside, frame);
            }

            // A member called as a statement, with its arguments beside it
            // rather than in brackets: `Selection.TypeText "Hello"`.
            let arguments: Vec<&Node> = inside
                .iter()
                .skip(1)
                .filter(|node| node.part() == Some(Part::Arguments))
                .flat_map(|node| parts(node))
                .filter(|node| node.part() == Some(Part::Argument))
                .collect();
            let given = self.given(&arguments, frame)?;
            return self.member_of(first, &given, frame);
        }
        // `a.b(1)` as a statement, which is a method with its arguments in
        // brackets and whose answer nobody wants.
        if first.part() == Some(Part::Index) {
            return self.value_of(first, frame);
        }

        // `Foo 1, 2`: the arguments are beside the name rather than in
        // brackets, and are a branch of their own.
        if first.part() == Some(Part::Name) {
            let name = name_of(first).ok_or_else(|| Fault::of(13))?.to_ascii_lowercase();
            let arguments: Vec<&Node> = inside
                .iter()
                .skip(1)
                .filter(|node| node.part() == Some(Part::Arguments))
                .flat_map(|node| parts(node))
                .filter(|node| node.part() == Some(Part::Argument))
                .collect();
            return self.call_with(&name, &arguments, frame);
        }
        self.value_of(first, frame)
    }

    /// `Err.Raise 5`, `Err.Clear`.
    fn err_member(
        &mut self,
        member: &str,
        inside: &[&'a Node],
        frame: &mut Frame,
    ) -> Result<Value, Fault> {
        match member.to_ascii_lowercase().as_str() {
            "clear" => {
                self.fault = None;
                Ok(Value::Empty)
            }
            "raise" => {
                let mut arguments = Vec::new();
                for argument in inside.iter().skip(1).flat_map(|node| node.every(Part::Argument)) {
                    arguments.push(self.value_of(argument, frame)?);
                }
                let number = arguments.first().map_or(Ok(0), Value::whole)?;
                let description = match arguments.get(2) {
                    Some(value) => value.text()?,
                    None => value::described(i32::try_from(number).unwrap_or(0)).to_owned(),
                };
                Err(Fault::saying(i32::try_from(number).unwrap_or(0), &description))
            }
            _ => Err(Fault::of(424)),
        }
    }

    /// A call written with brackets.
    fn call_named(
        &mut self,
        name: &str,
        node: &'a Node,
        frame: &mut Frame,
    ) -> Result<Value, Fault> {
        let arguments: Vec<&Node> = parts(node)
            .into_iter()
            .filter(|child| child.part() == Some(Part::Arguments))
            .flat_map(|child| parts(child))
            .filter(|child| child.part() == Some(Part::Argument))
            .collect();
        self.call_with(name, &arguments, frame)
    }

    /// Whatever the name turns out to be: the macro's own, the library's, or
    /// one of the few the machine answers itself.
    fn call_with(
        &mut self,
        name: &str,
        arguments: &[&'a Node],
        frame: &mut Frame,
    ) -> Result<Value, Fault> {
        let values = self.given(arguments, frame)?;

        if let Some(node) = self.procedures.get(name).copied() {
            // Where each argument came from, so that what a procedure changes
            // in what it was given changes the caller's own variable.
            let places: Vec<Option<&Node>> = arguments
                .iter()
                .map(|argument| {
                    parts(argument)
                        .into_iter()
                        .find(|inner| matches!(inner.part(), Some(Part::Name | Part::Index)))
                })
                .collect();
            return self.enter(node, values, &places, frame);
        }
        let plain = self.plainly(&values)?;
        if let Some(answer) = self.builtin(name, &plain)? {
            return Ok(answer);
        }
        if let Some(answer) = library::call(name, &plain, self.host) {
            return answer;
        }
        // A name the macro does not define and the library does not know may
        // still be one the program running it answers to.
        if let Some(Value::Object(handle)) = self.host.root(name) {
            return self.host.member(&handle, "Item", &values);
        }
        Err(Fault::saying(5, &format!("Sub or Function not defined: {name}")))
    }

    /// The arguments of a call, with the names the macro wrote on them.
    fn given(&mut self, arguments: &[&'a Node], frame: &mut Frame) -> Result<Vec<Given>, Fault> {
        let mut out = Vec::with_capacity(arguments.len());
        for argument in arguments {
            let inside = parts(argument);
            // `Name:=value` is a word, a colon, an equals and the value.
            let named = inside.len() >= 4
                && inside[1].token().is_some_and(|token| token.symbol(":"))
                && inside[2].token().is_some_and(|token| token.symbol("="));
            if named {
                let name = inside[0].token().map(|token| token.text.clone());
                let value = self.value_of(inside[3], frame)?;
                out.push(Given { name, value });
                continue;
            }
            out.push(Given::just(self.value_of(argument, frame)?));
        }
        Ok(out)
    }

    /// The same values with every object turned into what it says, for the
    /// library, which knows nothing about objects.
    fn plainly(&mut self, given: &[Given]) -> Result<Vec<Value>, Fault> {
        let mut out = Vec::with_capacity(given.len());
        for one in given {
            out.push(self.plain(one.value.clone())?);
        }
        Ok(out)
    }

    /// And one of them.
    fn plain(&mut self, value: Value) -> Result<Value, Fault> {
        match value {
            Value::Object(handle) => Ok(Value::Text(self.host.as_text(&handle)?)),
            other => Ok(other),
        }
    }

    /// The few the machine answers rather than the library: the ones that
    /// need a clock, a seed or the open files.
    fn builtin(&mut self, name: &str, arguments: &[Value]) -> Result<Option<Value>, Fault> {
        Ok(Some(match name {
            "now" => Value::Date(now()),
            "date" => Value::Date(now().floor()),
            "time" => Value::Date(now().fract()),
            "timer" => Value::Double(now().fract() * 86_400.0),
            "rnd" => {
                // A generator of this program's own, so that a macro that
                // seeds one and asks for numbers gets the same ones twice.
                let given = arguments.first().map_or(Ok(1.0), Value::number)?;
                if given == 0.0 {
                    Value::Double(self.last_random)
                } else {
                    if given < 0.0 {
                        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                        {
                            self.seed = (given.abs() * 1e9) as u64 | 1;
                        }
                    }
                    self.seed ^= self.seed << 13;
                    self.seed ^= self.seed >> 7;
                    self.seed ^= self.seed << 17;
                    #[allow(clippy::cast_precision_loss)]
                    let next = (self.seed >> 11) as f64 / (1u64 << 53) as f64;
                    self.last_random = next;
                    Value::Double(next)
                }
            }
            "randomize" => {
                let given = arguments.first().map_or(Ok(now()), Value::number)?;
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                {
                    self.seed = (given.abs() * 1e6) as u64 | 1;
                }
                Value::Empty
            }
            "eof" => {
                let number = arguments.first().map_or(Ok(0), Value::whole)?;
                let file = self.files.get(&number).ok_or_else(|| Fault::of(52))?;
                Value::Boolean(file.at >= file.lines.len())
            }
            "freefile" => {
                let mut number = 1i64;
                while self.files.contains_key(&number) {
                    number += 1;
                }
                Value::Long(number)
            }
            "lof" => {
                let number = arguments.first().map_or(Ok(0), Value::whole)?;
                let file = self.files.get(&number).ok_or_else(|| Fault::of(52))?;
                #[allow(clippy::cast_possible_wrap)]
                Value::Long(file.lines.iter().map(|line| line.len() + 2).sum::<usize>() as i64)
            }
            _ => return Ok(None),
        }))
    }

    // --- Files ----------------------------------------------------------

    /// `Open`, `Close`, `Print #`, `Input #`, `Line Input #`.
    fn file_statement(&mut self, statement: &'a Node, frame: &mut Frame) -> Result<(), Fault> {
        let inside = parts(statement);
        let first = inside
            .first()
            .and_then(|node| node.token())
            .map(|token| token.text.to_ascii_lowercase());
        match first.as_deref() {
            Some("open") => self.open_file(&inside, frame),
            Some("close") => self.close_files(&inside, frame),
            Some("print") | Some("write") => self.print_to_file(&inside, frame),
            Some("line") => self.read_line(&inside, frame, true),
            Some("input") => self.read_line(&inside, frame, false),
            _ => Err(Fault::saying(5, "This file statement is not one this program runs")),
        }
    }

    fn open_file(&mut self, inside: &[&'a Node], frame: &mut Frame) -> Result<(), Fault> {
        let mut path = None;
        let mut mode = String::new();
        let mut number = None;
        let mut after_as = false;
        let mut after_for = false;

        for node in inside.iter().skip(1) {
            if let Some(token) = node.token() {
                if token.is("for") {
                    after_for = true;
                    continue;
                }
                if token.is("as") {
                    after_as = true;
                    continue;
                }
                if token.symbol("#") {
                    continue;
                }
                if after_for && token.kind == Word::Word {
                    mode = token.text.to_ascii_lowercase();
                    after_for = false;
                    continue;
                }
            }
            // `Input` and `Output` are ordinary names as far as the parser is
            // concerned, so what follows `For` is read as one and looked at
            // here rather than evaluated.
            if after_for {
                if let Some(name) = first_name(node) {
                    mode = name.to_ascii_lowercase();
                    after_for = false;
                    continue;
                }
            }
            if node.part().is_some() {
                let value = self.value_of(node, frame)?;
                if after_as {
                    number = Some(value.whole()?);
                } else if path.is_none() {
                    path = Some(value.text()?);
                }
            }
        }

        let (Some(path), Some(number)) = (path, number) else { return Err(Fault::of(52)) };
        let path = std::path::PathBuf::from(path);
        let file = match mode.as_str() {
            "input" => {
                let text = std::fs::read_to_string(&path).map_err(|_| Fault::of(53))?;
                Opened {
                    path,
                    lines: text.lines().map(str::to_owned).collect(),
                    at: 0,
                    written: None,
                }
            }
            "append" => {
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                Opened { path, lines: Vec::new(), at: 0, written: Some(text) }
            }
            _ => Opened { path, lines: Vec::new(), at: 0, written: Some(String::new()) },
        };
        self.files.insert(number, file);
        Ok(())
    }

    fn close_files(&mut self, inside: &[&'a Node], frame: &mut Frame) -> Result<(), Fault> {
        let mut numbers = Vec::new();
        for node in inside.iter().skip(1).filter(|node| node.part().is_some()) {
            numbers.push(self.value_of(node, frame)?.whole()?);
        }
        if numbers.is_empty() {
            numbers = self.files.keys().copied().collect();
        }
        for number in numbers {
            if let Some(file) = self.files.remove(&number) {
                if let Some(written) = file.written {
                    std::fs::write(&file.path, written).map_err(|_| Fault::of(52))?;
                }
            }
        }
        Ok(())
    }

    fn print_to_file(&mut self, inside: &[&'a Node], frame: &mut Frame) -> Result<(), Fault> {
        let mut number = None;
        let mut pieces = Vec::new();
        let mut same_line = false;
        for node in inside.iter().skip(1) {
            if let Some(token) = node.token() {
                if token.symbol(";") {
                    same_line = true;
                    continue;
                }
                if token.symbol("#") || token.symbol(",") {
                    continue;
                }
            }
            if node.part().is_some() {
                let value = self.value_of(node, frame)?;
                if number.is_none() {
                    number = Some(value.whole()?);
                    continue;
                }
                same_line = false;
                pieces.push(value.text()?);
            }
        }

        let number = number.ok_or_else(|| Fault::of(52))?;
        let file = self.files.get_mut(&number).ok_or_else(|| Fault::of(52))?;
        let written = file.written.as_mut().ok_or_else(|| Fault::of(54))?;
        written.push_str(&pieces.join(""));
        if !same_line {
            written.push_str("\r\n");
        }
        Ok(())
    }

    fn read_line(
        &mut self,
        inside: &[&'a Node],
        frame: &mut Frame,
        whole_line: bool,
    ) -> Result<(), Fault> {
        let mut number = None;
        let mut targets = Vec::new();
        for node in inside.iter().skip(usize::from(whole_line) + 1) {
            if node.token().is_some() {
                continue;
            }
            if number.is_none() {
                number = Some(self.value_of(node, frame)?.whole()?);
                continue;
            }
            targets.push(*node);
        }

        let number = number.ok_or_else(|| Fault::of(52))?;
        let file = self.files.get_mut(&number).ok_or_else(|| Fault::of(52))?;
        let line = file.lines.get(file.at).cloned().ok_or_else(|| Fault::of(62))?;
        file.at += 1;

        if whole_line {
            if let Some(target) = targets.first() {
                self.assign_to(target, Value::Text(line), frame)?;
            }
            return Ok(());
        }
        // `Input #1, a, b` takes the line apart at the commas.
        for (target, piece) in targets.iter().zip(line.split(',')) {
            let piece = piece.trim().trim_matches('"');
            let value = match value::number_in(piece) {
                Some(number) if !piece.is_empty() => Value::Double(number),
                _ => Value::Text(piece.to_owned()),
            };
            self.assign_to(target, value, frame)?;
        }
        Ok(())
    }
}

// --- Reading the tree ------------------------------------------------------

/// The children that are not the ends of lines or comments.
fn parts(node: &Node) -> Vec<&Node> {
    node.children()
        .iter()
        .filter(|child| {
            !child.token().is_some_and(|token| {
                matches!(token.kind, Word::NewLine | Word::Comment | Word::End)
            })
        })
        .collect()
}

/// Whether a branch holds another, by where they are in memory.
fn ptr_holds(branch: &Node, wanted: &Node) -> bool {
    if core::ptr::eq(branch, wanted) {
        return true;
    }
    branch.children().iter().any(|child| ptr_holds(child, wanted))
}

/// The word a `Name` branch holds.
///
/// Not the same question as the one below: a declaration's name is the first
/// word that is *not* one of the language's own, because the keywords come
/// first — `Dim`, `Public`, `As`. A name being read is simply the word,
/// keyword or not, because `Nothing` and `Me` are names where they stand and
/// looking past them finds nothing at all.
fn name_of(node: &Node) -> Option<&str> {
    node.children()
        .iter()
        .find_map(|child| child.token())
        .filter(|token| token.kind == Word::Word)
        .map(|token| token.text.as_str())
}

/// The first word of a branch that is a name rather than a keyword.
fn first_name(node: &Node) -> Option<&str> {
    node.children().iter().find_map(|child| match child.token() {
        Some(token) if token.kind == Word::Word && !crate::tree::is_keyword(&token.text) => {
            Some(token.text.as_str())
        }
        _ => None,
    })
}

/// What a procedure is called.
fn procedure_name(node: &Node) -> Option<&str> {
    first_name(node)
}

/// What a `Function` gives back.
fn returns(node: &Node) -> Kind {
    let inside = parts(node);
    let at = inside.iter().position(|child| child.token().is_some_and(|token| token.is("as")));
    match at.and_then(|at| inside.get(at + 1)) {
        Some(name) => Kind::named(first_name(name).unwrap_or_default()),
        None => Kind::Whatever,
    }
}

/// What a declaration says a name holds.
fn declared_kind(declared: &Node) -> Kind {
    let inside = parts(declared);
    let at = inside.iter().position(|child| child.token().is_some_and(|token| token.is("as")));
    match at.and_then(|at| inside.get(at + 1)) {
        Some(name) => Kind::named(first_name(name).unwrap_or_default()),
        None => Kind::Whatever,
    }
}

/// The brackets of a declaration, which say it is an array.
fn bounds_of(declared: &Node) -> Option<&Node> {
    declared.children().iter().find(|child| child.part() == Some(Part::Arguments))
}

/// One parameter of a procedure.
struct Parameter<'a> {
    name: String,
    kind: Kind,
    by_value: bool,
    optional: bool,
    rest: bool,
    default: Option<&'a Node>,
}

/// What a procedure is given.
fn parameters(node: &Node) -> Vec<Parameter<'_>> {
    let Some(brackets) =
        node.children().iter().find(|child| child.part() == Some(Part::Parameters))
    else {
        return Vec::new();
    };
    brackets
        .children()
        .iter()
        .filter(|child| child.part() == Some(Part::Parameter))
        .filter_map(|child| {
            let inside = parts(child);
            let declared = inside.iter().find(|node| node.part() == Some(Part::Declared))?;
            let words: Vec<String> = inside
                .iter()
                .filter_map(|node| node.token().map(|token| token.text.to_ascii_lowercase()))
                .collect();
            let default = inside
                .iter()
                .rev()
                .find(|node| node.part().is_some() && node.part() != Some(Part::Declared))
                .copied();
            Some(Parameter {
                name: first_name(declared)?.to_owned(),
                kind: declared_kind(declared),
                by_value: words.iter().any(|word| word == "byval"),
                optional: words.iter().any(|word| word == "optional"),
                rest: words.iter().any(|word| word == "paramarray"),
                default,
            })
        })
        .collect()
}

/// The last word of a list of nodes, which for `a.b` is the member.
fn last_word(inside: &[&Node]) -> Option<String> {
    inside
        .iter()
        .rev()
        .find_map(|child| child.token())
        .filter(|token| token.kind == Word::Word)
        .map(|token| token.text.clone())
}

/// The two halves of `a.b`, and an empty head for a `.b` inside a `With`.
fn dotted(node: &Node) -> (String, String) {
    let inside = parts(node);
    let words: Vec<&Token> = inside.iter().filter_map(|child| child.token()).collect();
    let head = inside
        .first()
        .filter(|child| child.part().is_some())
        .and_then(|child| first_name(child))
        .unwrap_or_default()
        .to_owned();
    let member = words
        .iter()
        .rev()
        .find(|token| token.kind == Word::Word)
        .map(|token| token.text.clone())
        .unwrap_or_default();
    (head, member)
}

/// The conditions at the two ends of a `Do` loop, and whether each is an
/// `Until` rather than a `While`.
fn do_conditions<'a>(inside: &[&'a Node]) -> (Option<(&'a Node, bool)>, Option<(&'a Node, bool)>) {
    let body_at = inside.iter().position(|node| node.part() == Some(Part::Body));
    let mut top = None;
    let mut bottom = None;
    let mut until = false;
    for (at, node) in inside.iter().enumerate() {
        if let Some(token) = node.token() {
            if token.is("while") {
                until = false;
            }
            if token.is("until") {
                until = true;
            }
            continue;
        }
        if node.part() == Some(Part::Body) {
            continue;
        }
        if body_at.is_some_and(|body| at < body) {
            top = Some((*node, until));
        } else {
            bottom = Some((*node, until));
        }
    }
    (top, bottom)
}

/// Where a label is in a list of statements.
fn label_at(statements: &[&Node], label: &str) -> Option<usize> {
    statements.iter().position(|statement| {
        statement.part() == Some(Part::Label)
            && statement
                .children()
                .iter()
                .find_map(|child| child.token())
                .is_some_and(|token| token.text.eq_ignore_ascii_case(label))
    })
}

/// The value a literal holds.
fn literal(node: &Node) -> Result<Value, Fault> {
    let token = node.children().iter().find_map(Node::token).ok_or_else(|| Fault::of(13))?;
    Ok(match token.kind {
        Word::Number => {
            let text = token.text.trim_end_matches(['&', '%', '!', '@', '#']);
            match value::number_in(text) {
                Some(number) if number.fract() == 0.0 && number.abs() < 9.2e18 =>
                {
                    #[allow(clippy::cast_possible_truncation)]
                    Value::Long(number as i64)
                }
                Some(number) => Value::Double(number),
                None => return Err(Fault::of(13)),
            }
        }
        Word::Text => {
            let inside = token.text.trim_matches('"');
            Value::Text(inside.replace("\"\"", "\""))
        }
        Word::Date => {
            Value::Date(crate::dates::from_text(&token.text).ok_or_else(|| Fault::of(13))?)
        }
        _ => return Err(Fault::of(13)),
    })
}

/// A number as a whole one where it is whole, which is what a `For` counter
/// should stay so that `1` does not become `1.0` in a joined string.
fn whole_or_double(number: f64) -> Value {
    if number.fract() == 0.0 && number.abs() < 9.2e18 {
        #[allow(clippy::cast_possible_truncation)]
        return Value::Long(number as i64);
    }
    Value::Double(number)
}

/// Whether two values stand in the relation an operator names.
fn compares(left: &Value, operator: &str, right: &Value) -> Result<bool, Fault> {
    use core::cmp::Ordering;
    // `Is` compares two object references, and `Nothing` is the only one
    // there is so far.
    if operator == "is" {
        return Ok(match (left, right) {
            (Value::Nothing, Value::Nothing) => true,
            (Value::Object(one), Value::Object(other)) => one == other,
            _ => false,
        });
    }
    let Some(order) = value::compare(left, right)? else {
        return Ok(false);
    };
    Ok(match operator {
        "=" => order == Ordering::Equal,
        "<>" => order != Ordering::Equal,
        "<" => order == Ordering::Less,
        ">" => order == Ordering::Greater,
        "<=" => order != Ordering::Greater,
        ">=" => order != Ordering::Less,
        _ => return Err(Fault::of(13)),
    })
}

/// `Like`, with the patterns it understands: `?` for one letter, `*` for
/// any number of them, `#` for a digit, and `[abc]` for one of a few.
fn matches_pattern(text: &str, pattern: &str) -> bool {
    let letters: Vec<char> = text.chars().collect();
    let wanted: Vec<char> = pattern.chars().collect();
    matches_from(&letters, 0, &wanted, 0)
}

fn matches_from(text: &[char], at: usize, pattern: &[char], from: usize) -> bool {
    if from >= pattern.len() {
        return at >= text.len();
    }
    match pattern[from] {
        '*' => (at..=text.len()).any(|next| matches_from(text, next, pattern, from + 1)),
        '?' => at < text.len() && matches_from(text, at + 1, pattern, from + 1),
        '#' => {
            at < text.len()
                && text[at].is_ascii_digit()
                && matches_from(text, at + 1, pattern, from + 1)
        }
        '[' => {
            let Some(close) = pattern[from..].iter().position(|letter| *letter == ']') else {
                return false;
            };
            let inside: Vec<char> = pattern[from + 1..from + close].to_vec();
            let (wanted, inside) = match inside.split_first() {
                Some(('!', rest)) => (false, rest.to_vec()),
                _ => (true, inside),
            };
            at < text.len()
                && inside.contains(&text[at]) == wanted
                && matches_from(text, at + 1, pattern, from + close + 1)
        }
        letter => {
            at < text.len() && text[at] == letter && matches_from(text, at + 1, pattern, from + 1)
        }
    }
}

/// What was in an array before it was made bigger, put back where it was.
fn copy_across(old: &Array, made: &mut Array) {
    if old.bounds.len() != made.bounds.len() {
        return;
    }
    let mut subscripts: Vec<i64> = old.bounds.iter().map(|(low, _)| *low).collect();
    loop {
        if let (Ok(from), Ok(to)) = (old.at(&subscripts), made.at(&subscripts)) {
            made.values[to] = old.values[from].clone();
        }
        // The next set of subscripts, counting the last one fastest.
        let mut at = subscripts.len();
        loop {
            if at == 0 {
                return;
            }
            at -= 1;
            subscripts[at] += 1;
            if subscripts[at] <= old.bounds[at].1 {
                break;
            }
            subscripts[at] = old.bounds[at].0;
        }
    }
}

/// This moment, as a date.
fn now() -> f64 {
    let since = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs_f64())
        .unwrap_or_default();
    // Seconds from 1970, in days, from a count that starts in 1899.
    since / 86_400.0 + 25_569.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Quiet;

    /// Runs a module's `Test` and gives back what it answered.
    fn answer(source: &str) -> Value {
        let (program, complaints) = Program::read(source);
        assert!(complaints.is_empty(), "it did not parse: {complaints:?}");
        let mut host = Quiet::default();
        program.run("Test", Vec::new(), &mut host).expect("it ran")
    }

    /// The same, and what the program was asked to show along the way.
    fn watched(source: &str) -> (Value, Quiet) {
        let (program, complaints) = Program::read(source);
        assert!(complaints.is_empty(), "it did not parse: {complaints:?}");
        let mut host = Quiet::default();
        let value = program.run("Test", Vec::new(), &mut host).expect("it ran");
        (value, host)
    }

    /// And what went wrong, for a macro that was meant to.
    fn fault(source: &str) -> Fault {
        let (program, complaints) = Program::read(source);
        assert!(complaints.is_empty(), "it did not parse: {complaints:?}");
        let mut host = Quiet::default();
        program.run("Test", Vec::new(), &mut host).expect_err("it should not have run")
    }

    fn text(what: &str) -> Value {
        Value::Text(what.to_owned())
    }

    #[test]
    fn a_function_answers_with_its_own_name() {
        assert_eq!(
            answer("Function Test() As Long\r\n    Test = 6 * 7\r\nEnd Function\r\n"),
            Value::Long(42)
        );
    }

    #[test]
    fn a_procedure_is_given_things_and_may_hand_them_back_changed() {
        // By reference unless told otherwise, which is the rule everybody
        // forgets: `Double` changes the caller's own variable and `Keep`
        // does not.
        let source = "Sub Double(ByRef n As Long)\r\n\
             \x20   n = n * 2\r\n\
             End Sub\r\n\
             Sub Keep(ByVal n As Long)\r\n\
             \x20   n = n * 2\r\n\
             End Sub\r\n\
             Function Test() As String\r\n\
             \x20   Dim a As Long, b As Long\r\n\
             \x20   a = 5\r\n\
             \x20   b = 5\r\n\
             \x20   Double a\r\n\
             \x20   Keep b\r\n\
             \x20   Test = a & \",\" & b\r\n\
             End Function\r\n";
        assert_eq!(answer(source), text("10,5"));
    }

    #[test]
    fn a_function_may_call_itself() {
        let source = "Function Factorial(ByVal n As Long) As Double\r\n\
             \x20   If n <= 1 Then\r\n\
             \x20       Factorial = 1\r\n\
             \x20   Else\r\n\
             \x20       Factorial = n * Factorial(n - 1)\r\n\
             \x20   End If\r\n\
             End Function\r\n\
             Function Test() As Double\r\n\
             \x20   Test = Factorial(10)\r\n\
             End Function\r\n";
        assert_eq!(answer(source), Value::Double(3_628_800.0));
    }

    #[test]
    fn the_loops_all_count_the_same_way() {
        let source = "Function Test() As String\r\n\
             \x20   Dim total As Long, i As Long, out As String\r\n\
             \x20   For i = 1 To 5\r\n\
             \x20       total = total + i\r\n\
             \x20   Next i\r\n\
             \x20   out = total\r\n\
             \x20   total = 0\r\n\
             \x20   For i = 10 To 1 Step -2\r\n\
             \x20       total = total + 1\r\n\
             \x20   Next\r\n\
             \x20   out = out & \",\" & total\r\n\
             \x20   i = 0\r\n\
             \x20   Do While i < 4\r\n\
             \x20       i = i + 1\r\n\
             \x20   Loop\r\n\
             \x20   out = out & \",\" & i\r\n\
             \x20   i = 0\r\n\
             \x20   Do\r\n\
             \x20       i = i + 1\r\n\
             \x20   Loop Until i >= 3\r\n\
             \x20   out = out & \",\" & i\r\n\
             \x20   i = 0\r\n\
             \x20   While i < 2\r\n\
             \x20       i = i + 1\r\n\
             \x20   Wend\r\n\
             \x20   Test = out & \",\" & i\r\n\
             End Function\r\n";
        assert_eq!(answer(source), text("15,5,4,3,2"));
    }

    #[test]
    fn a_loop_can_be_left_early_and_leaves_only_the_one_it_is_in() {
        let source = "Function Test() As Long\r\n\
             \x20   Dim i As Long, j As Long, count As Long\r\n\
             \x20   For i = 1 To 3\r\n\
             \x20       For j = 1 To 10\r\n\
             \x20           If j = 3 Then Exit For\r\n\
             \x20           count = count + 1\r\n\
             \x20       Next j\r\n\
             \x20   Next i\r\n\
             \x20   Test = count\r\n\
             End Function\r\n";
        assert_eq!(answer(source), Value::Long(6));
    }

    #[test]
    fn an_array_is_counted_from_where_it_was_declared() {
        let source = "Function Test() As String\r\n\
             \x20   Dim a(1 To 3) As Long, b(2) As Long\r\n\
             \x20   a(1) = 10\r\n\
             \x20   a(3) = 30\r\n\
             \x20   b(0) = 1\r\n\
             \x20   Test = LBound(a) & \",\" & UBound(a) & \",\" & a(1) + a(3) & \",\" & LBound(b) & \",\" & UBound(b)\r\n\
             End Function\r\n";
        assert_eq!(answer(source), text("1,3,40,0,2"));
    }

    #[test]
    fn redim_preserve_keeps_what_was_there_by_subscript() {
        // Growing an array must not shuffle it, which is the whole point of
        // Preserve and the easiest thing to get wrong.
        let source = "Function Test() As String\r\n\
             \x20   Dim a() As Long\r\n\
             \x20   ReDim a(1 To 2)\r\n\
             \x20   a(1) = 11\r\n\
             \x20   a(2) = 22\r\n\
             \x20   ReDim Preserve a(1 To 4)\r\n\
             \x20   a(4) = 44\r\n\
             \x20   Test = a(1) & \",\" & a(2) & \",\" & a(4) & \",\" & UBound(a)\r\n\
             End Function\r\n";
        assert_eq!(answer(source), text("11,22,44,4"));
    }

    #[test]
    fn erase_empties_an_array_and_leaves_its_shape() {
        let source = "Function Test() As String\r\n\
             \x20   Dim a(1 To 2) As Long\r\n\
             \x20   a(1) = 5\r\n\
             \x20   Erase a\r\n\
             \x20   Test = a(1) & \",\" & UBound(a)\r\n\
             End Function\r\n";
        assert_eq!(answer(source), text("0,2"));
    }

    #[test]
    fn for_each_walks_what_split_made() {
        let source = "Function Test() As String\r\n\
             \x20   Dim piece As Variant, out As String\r\n\
             \x20   For Each piece In Split(\"a,b,c\", \",\")\r\n\
             \x20       out = out & piece & \"-\"\r\n\
             \x20   Next piece\r\n\
             \x20   Test = out\r\n\
             End Function\r\n";
        assert_eq!(answer(source), text("a-b-c-"));
    }

    #[test]
    fn select_case_takes_lists_and_ranges_and_is() {
        let source = "Function Which(ByVal n As Long) As String\r\n\
             \x20   Select Case n\r\n\
             \x20       Case 1, 2\r\n\
             \x20           Which = \"a few\"\r\n\
             \x20       Case 3 To 9\r\n\
             \x20           Which = \"several\"\r\n\
             \x20       Case Is > 9\r\n\
             \x20           Which = \"many\"\r\n\
             \x20       Case Else\r\n\
             \x20           Which = \"none\"\r\n\
             \x20   End Select\r\n\
             End Function\r\n\
             Function Test() As String\r\n\
             \x20   Test = Which(1) & \"/\" & Which(5) & \"/\" & Which(20) & \"/\" & Which(0)\r\n\
             End Function\r\n";
        assert_eq!(answer(source), text("a few/several/many/none"));
    }

    #[test]
    fn the_variant_rules_hold_inside_a_macro_as_well() {
        let source = "Function Test() As String\r\n\
             \x20   Dim v As Variant, out As String\r\n\
             \x20   out = \"3\" + 4\r\n\
             \x20   out = out & \"|\" & (\"3\" & 4)\r\n\
             \x20   out = out & \"|\" & (v + 1)\r\n\
             \x20   out = out & \"|\" & (True + 1)\r\n\
             \x20   out = out & \"|\" & (7 / 2) & \"|\" & (7 \\ 2)\r\n\
             \x20   Test = out\r\n\
             End Function\r\n";
        assert_eq!(answer(source), text("7|34|1|0|3.5|3"));
    }

    #[test]
    fn a_variable_declared_as_a_kind_holds_that_kind() {
        // Putting one and a half into a Long rounds it, and to the even
        // number when it is exactly between.
        let source = "Function Test() As String\r\n\
             \x20   Dim n As Long, s As String, c As Currency\r\n\
             \x20   n = 2.5\r\n\
             \x20   s = 42\r\n\
             \x20   c = 0.1\r\n\
             \x20   Test = n & \",\" & s & \",\" & (c + c + c)\r\n\
             End Function\r\n";
        assert_eq!(answer(source), text("2,42,0.3"));
    }

    #[test]
    fn a_static_variable_remembers_between_calls() {
        let source = "Function Counted() As Long\r\n\
             \x20   Static seen As Long\r\n\
             \x20   seen = seen + 1\r\n\
             \x20   Counted = seen\r\n\
             End Function\r\n\
             Function Test() As String\r\n\
             \x20   Test = Counted() & Counted() & Counted()\r\n\
             End Function\r\n";
        assert_eq!(answer(source), text("123"));
    }

    #[test]
    fn a_module_level_variable_outlives_a_call() {
        let source = "Dim total As Long\r\n\
             Sub AddOne()\r\n\
             \x20   total = total + 1\r\n\
             End Sub\r\n\
             Function Test() As Long\r\n\
             \x20   AddOne\r\n\
             \x20   AddOne\r\n\
             \x20   Test = total\r\n\
             End Function\r\n";
        assert_eq!(answer(source), Value::Long(2));
    }

    #[test]
    fn a_constant_and_an_enum_are_names_for_numbers() {
        let source = "Const Limit As Long = 7\r\n\
             Public Enum Colour\r\n\
             \x20   Red = 1\r\n\
             \x20   Green\r\n\
             \x20   Blue\r\n\
             End Enum\r\n\
             Function Test() As String\r\n\
             \x20   Test = Limit & \",\" & Red & \",\" & Green & \",\" & Blue\r\n\
             End Function\r\n";
        assert_eq!(answer(source), text("7,1,2,3"));
    }

    #[test]
    fn on_error_resume_next_carries_on_and_says_what_happened() {
        let source = "Function Test() As String\r\n\
             \x20   Dim n As Long\r\n\
             \x20   On Error Resume Next\r\n\
             \x20   n = 1 / 0\r\n\
             \x20   Test = Err.Number & \":\" & Err.Description\r\n\
             End Function\r\n";
        assert_eq!(answer(source), text("11:Division by zero"));
    }

    #[test]
    fn on_error_goto_jumps_and_resume_next_comes_back() {
        let source = "Function Test() As String\r\n\
             \x20   Dim out As String\r\n\
             \x20   On Error GoTo Sorry\r\n\
             \x20   out = \"a\"\r\n\
             \x20   Err.Raise 5\r\n\
             \x20   out = out & \"c\"\r\n\
             \x20   Test = out\r\n\
             \x20   Exit Function\r\n\
             Sorry:\r\n\
             \x20   out = out & \"b\"\r\n\
             \x20   Resume Next\r\n\
             End Function\r\n";
        assert_eq!(answer(source), text("abc"));
    }

    #[test]
    fn an_error_nobody_is_watching_for_stops_the_macro_and_says_where() {
        let fault = fault("Sub Test()\r\n    Dim n As Long\r\n    n = 1 / 0\r\nEnd Sub\r\n");
        assert_eq!(fault.number, 11);
        assert_eq!(fault.description, "Division by zero");
    }

    #[test]
    fn a_goto_goes_and_a_label_is_where_it_goes_to() {
        let source = "Function Test() As String\r\n\
             \x20   Dim out As String\r\n\
             \x20   out = \"a\"\r\n\
             \x20   GoTo Skip\r\n\
             \x20   out = out & \"b\"\r\n\
             Skip:\r\n\
             \x20   Test = out & \"c\"\r\n\
             End Function\r\n";
        assert_eq!(answer(source), text("ac"));
    }

    #[test]
    fn option_explicit_refuses_a_variable_nobody_declared() {
        let source = "Option Explicit\r\n\
             Sub Test()\r\n\
             \x20   undeclared = 1\r\n\
             End Sub\r\n";
        assert!(fault(source).description.contains("Variable not defined"), "{:?}", fault(source));

        // And says nothing about one that was.
        let fine = "Option Explicit\r\n\
             Function Test() As Long\r\n\
             \x20   Dim n As Long\r\n\
             \x20   n = 1\r\n\
             \x20   Test = n\r\n\
             End Function\r\n";
        assert_eq!(answer(fine), Value::Long(1));
    }

    #[test]
    fn what_a_macro_shows_goes_to_the_program_and_not_to_the_language() {
        let source = "Sub Test()\r\n\
             \x20   MsgBox \"Saved \" & 2 & \" files\"\r\n\
             \x20   Debug.Print \"done\"\r\n\
             End Sub\r\n";
        let (_, host) = watched(source);
        assert_eq!(host.messages, vec!["Saved 2 files".to_owned()]);
        assert_eq!(host.notes, vec!["done".to_owned()]);
    }

    #[test]
    fn the_library_is_reachable_from_a_macro() {
        let source = "Function Test() As String\r\n\
             \x20   Dim s As String\r\n\
             \x20   s = \"Hello, world\"\r\n\
             \x20   Test = Left(s, 5) & \"|\" & Mid(s, 8) & \"|\" & InStr(s, \"world\") & \"|\" & _\r\n\
             \x20       UCase(Replace(s, \"world\", \"there\")) & \"|\" & Format(3.14159, \"0.00\")\r\n\
             End Function\r\n";
        assert_eq!(answer(source), text("Hello|world|8|HELLO, THERE|3.14"));
    }

    #[test]
    fn a_macro_that_never_stops_is_stopped() {
        // A program that hangs the window it is running in is worse than one
        // that says it gave up.
        let (program, complaints) =
            Program::read("Sub Test()\r\n    Do\r\n    Loop\r\nEnd Sub\r\n");
        assert!(complaints.is_empty(), "{complaints:?}");
        let mut host = Quiet::default();
        let mut machine = program.machine(&mut host);
        machine.allow(1_000);
        let fault = machine.run("Test", Vec::new()).expect_err("it should have been stopped");
        assert_eq!(fault.number, 28, "{fault}");
    }

    #[test]
    fn a_file_is_written_and_read_back() {
        let directory = std::env::temp_dir().join(format!("wp-vba-run-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("a directory to work in");
        let path = directory.join("notes.txt");
        let written = path.to_string_lossy().replace('\\', "\\\\");

        let source = format!(
            "Function Test() As String\r\n\
             \x20   Dim line1 As String, line2 As String\r\n\
             \x20   Open \"{written}\" For Output As #1\r\n\
             \x20   Print #1, \"one\"\r\n\
             \x20   Print #1, \"two \" & 2\r\n\
             \x20   Close #1\r\n\
             \x20   Open \"{written}\" For Input As #1\r\n\
             \x20   Line Input #1, line1\r\n\
             \x20   Line Input #1, line2\r\n\
             \x20   Close #1\r\n\
             \x20   Test = line1 & \"|\" & line2\r\n\
             End Function\r\n"
        );
        assert_eq!(answer(&source), text("one|two 2"));
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn like_matches_the_patterns_it_promises() {
        let source = "Function Test() As String\r\n\
             \x20   Test = (\"hello\" Like \"h*o\") & \",\" & (\"abc\" Like \"a?c\") & \",\" & _\r\n\
             \x20       (\"a1\" Like \"a#\") & \",\" & (\"dog\" Like \"[cd]og\") & \",\" & (\"x\" Like \"y\")\r\n\
             End Function\r\n";
        assert_eq!(answer(source), text("True,True,True,True,False"));
    }

    #[test]
    fn a_macro_that_is_not_there_says_so_rather_than_doing_nothing() {
        let (program, _) = Program::read("Sub Other()\r\nEnd Sub\r\n");
        let mut host = Quiet::default();
        let fault = program.run("Test", Vec::new(), &mut host).expect_err("there is no Test");
        assert!(fault.description.contains("not defined"), "{fault}");
    }

    #[test]
    fn a_line_that_never_parsed_is_not_run() {
        // The parser keeps what it could not read so that the module comes
        // back whole; running it would be running a guess.
        let (program, complaints) = Program::read("Sub Test()\r\n    ]] nonsense\r\nEnd Sub\r\n");
        assert_eq!(complaints.len(), 1);
        let mut host = Quiet::default();
        let fault = program.run("Test", Vec::new(), &mut host).expect_err("it should not run");
        assert!(fault.description.contains("never read"), "{fault}");
    }
}

#[cfg(test)]
mod objects {
    use super::*;
    use crate::library::Host;
    use crate::value::{Given, Handle};

    /// A program with three things in it, as small as an object model can be
    /// and still be one: a box holding a word, a list of boxes, and a name
    /// for each.
    ///
    /// Here so that the language's side of objects can be proved without the
    /// document's side, which is a great deal larger and lives in the program
    /// that has a document.
    #[derive(Default)]
    struct Toy {
        words: Vec<String>,
        shown: Vec<String>,
    }

    impl Host for Toy {
        fn message(&mut self, text: &str, _buttons: i64, _title: &str) -> i64 {
            self.shown.push(text.to_owned());
            1
        }

        fn root(&mut self, name: &str) -> Option<Value> {
            match name.to_ascii_lowercase().as_str() {
                "boxes" => Some(Value::Object(Handle::of("Boxes", 0))),
                _ => None,
            }
        }

        fn member(
            &mut self,
            object: &Handle,
            member: &str,
            given: &[Given],
        ) -> Result<Value, Fault> {
            match (object.kind.as_str(), member.to_ascii_lowercase().as_str()) {
                ("Boxes", "count") => Ok(Value::Long(self.words.len() as i64)),
                ("Boxes", "item") => {
                    let which = Given::find(given, "Index", 0)
                        .cloned()
                        .unwrap_or(Value::Long(1))
                        .whole()?;
                    if which < 1 || which as usize > self.words.len() {
                        return Err(Fault::of(9));
                    }
                    Ok(Value::Object(Handle::of("Box", which as u64)))
                }
                ("Boxes", "add") => {
                    let word =
                        Given::find(given, "Word", 0).cloned().unwrap_or(Value::Empty).text()?;
                    self.words.push(word);
                    Ok(Value::Object(Handle::of("Box", self.words.len() as u64)))
                }
                ("Box", "text") => Ok(Value::Text(
                    self.words.get(object.id as usize - 1).cloned().unwrap_or_default(),
                )),
                ("Box", "number") => Ok(Value::Long(object.id as i64)),
                _ => Err(Fault::saying(438, &format!("{}.{member} is not here", object.kind))),
            }
        }

        fn set_member(&mut self, object: &Handle, member: &str, value: Value) -> Result<(), Fault> {
            match (object.kind.as_str(), member.to_ascii_lowercase().as_str()) {
                ("Box", "text") => {
                    let at = object.id as usize - 1;
                    if at < self.words.len() {
                        self.words[at] = value.text()?;
                    }
                    Ok(())
                }
                _ => Err(Fault::saying(438, &format!("{}.{member} is not set here", object.kind))),
            }
        }

        fn items(&mut self, object: &Handle) -> Result<Vec<Value>, Fault> {
            match object.kind.as_str() {
                "Boxes" => Ok((1..=self.words.len())
                    .map(|at| Value::Object(Handle::of("Box", at as u64)))
                    .collect()),
                _ => Err(Fault::saying(438, &format!("{} is not a list", object.kind))),
            }
        }

        fn as_text(&mut self, object: &Handle) -> Result<String, Fault> {
            self.member(object, "Text", &[])?.text()
        }
    }

    fn with_toy(source: &str) -> (Value, Toy) {
        let (program, complaints) = Program::read(source);
        assert!(complaints.is_empty(), "it did not parse: {complaints:?}");
        let mut toy = Toy::default();
        let value = program.run("Test", Vec::new(), &mut toy).expect("it ran");
        (value, toy)
    }

    #[test]
    fn a_name_the_program_answers_to_is_an_object() {
        let (value, _) = with_toy(
            "Function Test() As Long\r\n    Boxes.Add \"one\"\r\n    Test = Boxes.Count\r\nEnd Function\r\n",
        );
        assert_eq!(value, Value::Long(1));
    }

    #[test]
    fn a_collection_in_brackets_is_the_one_it_holds() {
        // `Boxes(2)` is `Boxes.Item(2)`, which is what Word means by it.
        let (value, _) = with_toy(
            "Function Test() As String\r\n\
             \x20   Boxes.Add \"one\"\r\n\
             \x20   Boxes.Add \"two\"\r\n\
             \x20   Test = Boxes(2).Text & \"/\" & Boxes.Item(1).Text\r\n\
             End Function\r\n",
        );
        assert_eq!(value, Value::Text("two/one".to_owned()));
    }

    #[test]
    fn a_member_can_be_written_to_as_well_as_read() {
        let (value, toy) = with_toy(
            "Function Test() As String\r\n\
             \x20   Boxes.Add \"one\"\r\n\
             \x20   Boxes(1).Text = \"changed\"\r\n\
             \x20   Test = Boxes(1).Text\r\n\
             End Function\r\n",
        );
        assert_eq!(value, Value::Text("changed".to_owned()));
        assert_eq!(toy.words, vec!["changed".to_owned()]);
    }

    #[test]
    fn with_holds_an_object_and_a_full_stop_means_it() {
        let (value, _) = with_toy(
            "Function Test() As String\r\n\
             \x20   Boxes.Add \"one\"\r\n\
             \x20   With Boxes(1)\r\n\
             \x20       .Text = \"inside\"\r\n\
             \x20       Test = .Text & \" \" & .Number\r\n\
             \x20   End With\r\n\
             End Function\r\n",
        );
        assert_eq!(value, Value::Text("inside 1".to_owned()));
    }

    #[test]
    fn for_each_walks_a_collection_the_program_owns() {
        let (value, _) = with_toy(
            "Function Test() As String\r\n\
             \x20   Dim one As Object, out As String\r\n\
             \x20   Boxes.Add \"a\"\r\n\
             \x20   Boxes.Add \"b\"\r\n\
             \x20   For Each one In Boxes\r\n\
             \x20       out = out & one.Text\r\n\
             \x20   Next one\r\n\
             \x20   Test = out\r\n\
             End Function\r\n",
        );
        assert_eq!(value, Value::Text("ab".to_owned()));
    }

    #[test]
    fn an_argument_may_be_named_the_way_word_macros_name_them() {
        let (_, toy) = with_toy("Sub Test()\r\n    Boxes.Add Word:=\"named\"\r\nEnd Sub\r\n");
        assert_eq!(toy.words, vec!["named".to_owned()]);
    }

    #[test]
    fn an_object_stands_for_what_it_says_where_a_string_is_wanted() {
        // Word gives a range's text when one is used as a string, and a
        // macro that shows a selection depends on it.
        let (_, toy) = with_toy(
            "Sub Test()\r\n\
             \x20   Boxes.Add \"shown\"\r\n\
             \x20   MsgBox Boxes(1)\r\n\
             End Sub\r\n",
        );
        assert_eq!(toy.shown, vec!["shown".to_owned()]);
    }

    #[test]
    fn set_keeps_an_object_and_is_compares_two_of_them() {
        let (value, _) = with_toy(
            "Function Test() As String\r\n\
             \x20   Dim one As Object, other As Object\r\n\
             \x20   Boxes.Add \"a\"\r\n\
             \x20   Boxes.Add \"b\"\r\n\
             \x20   Set one = Boxes(1)\r\n\
             \x20   Set other = Boxes(1)\r\n\
             \x20   Test = (one Is other) & \",\" & (one Is Boxes(2)) & \",\" & (one Is Nothing)\r\n\
             End Function\r\n",
        );
        assert_eq!(value, Value::Text("True,False,False".to_owned()));
    }

    #[test]
    fn a_member_nothing_has_says_which_one_rather_than_guessing() {
        // The rule this whole item is written under: a property this program
        // cannot answer must say so and stop.
        let (program, _) = Program::read(
            "Sub Test()\r\n    Boxes.Add \"a\"\r\n    Boxes(1).Colour = 3\r\nEnd Sub\r\n",
        );
        let mut toy = Toy::default();
        let fault = program.run("Test", Vec::new(), &mut toy).expect_err("it should have stopped");
        assert_eq!(fault.number, 438);
        assert!(fault.description.contains("Colour"), "{fault}");
    }

    #[test]
    fn a_full_stop_outside_a_with_block_says_so() {
        let (program, _) = Program::read("Sub Test()\r\n    .Text = \"x\"\r\nEnd Sub\r\n");
        let mut toy = Toy::default();
        let fault = program.run("Test", Vec::new(), &mut toy).expect_err("it should have stopped");
        assert_eq!(fault.number, 91, "{fault}");
    }

    #[test]
    fn a_name_the_program_does_not_answer_to_is_still_not_an_object() {
        let (program, _) = Program::read("Sub Test()\r\n    Crates.Add 1\r\nEnd Sub\r\n");
        let mut toy = Toy::default();
        let fault = program.run("Test", Vec::new(), &mut toy).expect_err("it should have stopped");
        assert!(fault.description.contains("Crates") || fault.number == 424, "{fault}");
    }
}
