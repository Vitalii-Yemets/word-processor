//! Stopping a macro in the middle, and looking at it.
//!
//! # Why a thread
//!
//! A debugger has to stop a program between two statements and give the
//! window back to whoever is using it — and then carry on from exactly there.
//! An interpreter that walks a tree with ordinary function calls cannot be
//! stopped in the middle without either rewriting it as a machine with its
//! own stack, or putting it somewhere it can wait.
//!
//! It is put somewhere it can wait: its own thread. The macro runs there and
//! the document stays here, and every time the macro wants something of the
//! document it asks down a channel and waits for the answer. Stopping is then
//! not a mechanism at all — it is the window *not answering yet*. Press F5 and
//! the answer goes back and the macro carries on from the statement it was
//! about to run.
//!
//! # Why that is safe
//!
//! Because the macro's thread never touches the document. It has the program
//! and its own variables, and everything else is a question. What crosses the
//! channel is values and handles — numbers, strings, and which object — and
//! the window is the only thing that ever holds the document. There is no
//! lock to forget and nothing shared to race over.
//!
//! # What the window has to do
//!
//! Answer. [`Debugger::pump`] is called on every tick and after every event:
//! it takes whatever the macro has asked, answers it, and comes back. While a
//! macro is running with nothing to stop it, that loop is where the time
//! goes, which is the same as running it straight out — the difference is
//! that it *can* stop.
//!
//! Three questions are left unanswered on purpose, and each is a way of
//! waiting: the step before a line with a breakpoint on it, which is being
//! stopped; a form the macro has put up, answered when something is done on
//! it; and a message box or an input box, answered when the person answers
//! the dialog. Every macro runs this way — the Run button, F5, and a
//! document's own events — so that all three can happen from any of them.

use std::collections::BTreeSet;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

use wp_vba::forms::{Form, Happening};
use wp_vba::library::Host;
use wp_vba::run::{Program, Source};
use wp_vba::value::{Fault, Given, Handle, Value};

use super::objects::Model;
use super::Editor;

/// How long the window waits for the macro before going back to its own work.
///
/// Long enough that an ordinary macro runs to the end inside one pump, short
/// enough that one doing arithmetic for a minute does not freeze the window.
const PATIENCE: Duration = Duration::from_millis(20);

/// What the macro asks the window for.
#[derive(Debug)]
enum Ask {
    Message {
        text: String,
        buttons: i64,
        title: String,
    },
    Prompt {
        prompt: String,
        title: String,
        default: String,
    },
    Note(String),
    Root(String),
    Member {
        object: Handle,
        member: String,
        given: Vec<Given>,
    },
    SetMember {
        object: Handle,
        member: String,
        value: Value,
    },
    Items(Handle),
    AsText(Handle),
    Watching(Vec<(String, Value)>),
    /// About to run the statement on this line. The answer may not come for
    /// a while, and that is what being stopped is.
    Step(usize),
    /// A form to put up, as it stands; answered when something is done on
    /// it, which may be a while too.
    Form(Box<Form>),
    /// And the last thing it says: its answer, and what it left the
    /// arguments as.
    Done(Box<Result<(Value, Vec<Value>), Fault>>),
}

/// And what the window answers.
#[derive(Debug)]
enum Answer {
    Number(i64),
    Words(Option<String>),
    One(Box<Result<Value, Fault>>),
    Maybe(Option<Value>),
    Many(Box<Result<Vec<Value>, Fault>>),
    Text(Box<Result<String, Fault>>),
    Nothing,
    /// Whether to carry on.
    Go(bool),
    /// What was done on the form.
    Happened(Box<Happening>),
}

/// The macro's side: everything it wants is a question down the channel.
struct Proxy {
    asks: Sender<Ask>,
    answers: Receiver<Answer>,
}

impl Proxy {
    /// Asks, and waits. A window that has gone away ends the macro.
    fn ask(&mut self, ask: Ask) -> Option<Answer> {
        self.asks.send(ask).ok()?;
        self.answers.recv().ok()
    }
}

impl Host for Proxy {
    fn message(&mut self, text: &str, buttons: i64, title: &str) -> i64 {
        match self.ask(Ask::Message { text: text.to_owned(), buttons, title: title.to_owned() }) {
            Some(Answer::Number(number)) => number,
            _ => 1,
        }
    }

    fn ask(&mut self, prompt: &str, title: &str, default: &str) -> Option<String> {
        match Self::ask(
            self,
            Ask::Prompt {
                prompt: prompt.to_owned(),
                title: title.to_owned(),
                default: default.to_owned(),
            },
        ) {
            Some(Answer::Words(words)) => words,
            _ => None,
        }
    }

    fn note(&mut self, text: &str) {
        let _ = self.ask(Ask::Note(text.to_owned()));
    }

    fn root(&mut self, name: &str) -> Option<Value> {
        match self.ask(Ask::Root(name.to_owned())) {
            Some(Answer::Maybe(value)) => value,
            _ => None,
        }
    }

    fn member(&mut self, object: &Handle, member: &str, given: &[Given]) -> Result<Value, Fault> {
        match self.ask(Ask::Member {
            object: object.clone(),
            member: member.to_owned(),
            given: given.to_vec(),
        }) {
            Some(Answer::One(answer)) => *answer,
            _ => Err(gone()),
        }
    }

    fn set_member(&mut self, object: &Handle, member: &str, value: Value) -> Result<(), Fault> {
        match self.ask(Ask::SetMember { object: object.clone(), member: member.to_owned(), value })
        {
            Some(Answer::One(answer)) => answer.map(|_| ()),
            _ => Err(gone()),
        }
    }

    fn items(&mut self, object: &Handle) -> Result<Vec<Value>, Fault> {
        match self.ask(Ask::Items(object.clone())) {
            Some(Answer::Many(answer)) => *answer,
            _ => Err(gone()),
        }
    }

    fn as_text(&mut self, object: &Handle) -> Result<String, Fault> {
        match self.ask(Ask::AsText(object.clone())) {
            Some(Answer::Text(answer)) => *answer,
            _ => Err(gone()),
        }
    }

    fn watching(&mut self, values: &[(String, Value)]) {
        let _ = self.ask(Ask::Watching(values.to_vec()));
    }

    fn step(&mut self, line: usize) -> bool {
        matches!(self.ask(Ask::Step(line)), Some(Answer::Go(true)))
    }

    fn show_form(&mut self, form: &Form) -> Result<Happening, Fault> {
        match self.ask(Ask::Form(Box::new(form.clone()))) {
            Some(Answer::Happened(happening)) => Ok(*happening),
            _ => Err(gone()),
        }
    }
}

/// A question a macro has put to the person, which the window shows as a
/// dialog and answers when the dialog is answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Prompt {
    /// `MsgBox`: the text, Word's buttons code, and the title.
    Message { text: String, buttons: i64, title: String },
    /// `InputBox`: the prompt, the title, and what the box starts with.
    Input { prompt: String, title: String, default: String },
}

/// What a macro is told when the window has gone.
fn gone() -> Fault {
    Fault::saying(18, "The window this macro was running in has gone")
}

/// A macro running, and the window's side of the conversation.
pub struct Debugger {
    asks: Receiver<Ask>,
    answers: Sender<Answer>,
    thread: Option<std::thread::JoinHandle<()>>,
    /// What the macro is holding on to, which outlives one question.
    model: Model,
    /// Which macro is running, for what is said at the end.
    pub name: String,
    /// The lines with a breakpoint on them, counting from one.
    pub breakpoints: BTreeSet<usize>,
    /// Whether to stop before the next statement whatever line it is on.
    pub stepping: bool,
    /// Which line it is stopped on, if it is stopped.
    pub stopped: Option<usize>,
    /// The form it is waiting on, if it is: the window puts it up and
    /// answers with what was done on it.
    pub form: Option<Form>,
    /// The question it is waiting on, if it is: a message box or an input
    /// box, which the window puts up as a dialog.
    pub prompt: Option<Prompt>,
    /// Whether to say nothing when it ends well: an event's macro is not
    /// something anybody asked to run.
    pub quiet: bool,
    /// What the stopped procedure can see.
    pub watched: Vec<(String, Value)>,
    /// What it has said so far.
    pub said: Vec<String>,
    /// And how it ended, once it has: its answer and what it left its
    /// arguments as.
    pub ended: Option<Result<(Value, Vec<Value>), Fault>>,
}

impl core::fmt::Debug for Debugger {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Debugger")
            .field("name", &self.name)
            .field("stopped", &self.stopped)
            .finish_non_exhaustive()
    }
}

impl Debugger {
    /// Starts a macro, stopping before its first statement if asked to.
    ///
    /// The whole project goes with it, because the macro may call into any
    /// module of it; `name` says which module and which procedure, as
    /// `Module1.Hello`.
    #[must_use]
    pub fn start(
        modules: Vec<Source>,
        name: &str,
        arguments: Vec<Value>,
        breakpoints: BTreeSet<usize>,
        stepping: bool,
    ) -> Self {
        let (asks, from_macro) = std::sync::mpsc::channel();
        let (to_macro, answers) = std::sync::mpsc::channel();
        let wanted = name.to_owned();

        let thread = std::thread::spawn(move || {
            let mut proxy = Proxy { asks: asks.clone(), answers };
            let (program, complaints) = Program::of(&modules);
            let answer = match complaints.first() {
                Some((module, complaint)) => {
                    Err(Fault::saying(5, &format!("{module}: {complaint}")))
                }
                None => program.run_back(&wanted, arguments, &mut proxy),
            };
            let _ = asks.send(Ask::Done(Box::new(answer)));
        });

        Self {
            asks: from_macro,
            answers: to_macro,
            thread: Some(thread),
            model: Model::default(),
            name: name.to_owned(),
            breakpoints,
            stepping,
            stopped: None,
            form: None,
            prompt: None,
            quiet: false,
            watched: Vec::new(),
            said: Vec::new(),
            ended: None,
        }
    }

    /// Whether the macro is still going.
    #[must_use]
    pub fn running(&self) -> bool {
        self.ended.is_none()
    }

    /// Answers whatever the macro has asked, until it stops or finishes or
    /// has nothing to say for the moment.
    pub fn pump(&mut self, editor: &mut Editor) {
        while self.stopped.is_none()
            && self.form.is_none()
            && self.prompt.is_none()
            && self.ended.is_none()
        {
            let ask = match self.asks.recv_timeout(PATIENCE) {
                Ok(ask) => ask,
                // Still thinking: come back on the next tick rather than
                // holding the window.
                Err(RecvTimeoutError::Timeout) => return,
                Err(RecvTimeoutError::Disconnected) => {
                    self.ended = Some(Err(gone()));
                    return;
                }
            };
            self.answer(ask, editor);
        }
    }

    /// One question.
    fn answer(&mut self, ask: Ask, editor: &mut Editor) {
        let answer = match ask {
            Ask::Done(answer) => {
                self.ended = Some(*answer);
                self.said.extend(self.model.said());
                return;
            }
            Ask::Step(line) => {
                // The one question the window may leave unanswered, which is
                // what being stopped is.
                if self.stepping || self.breakpoints.contains(&line) {
                    self.stopped = Some(line);
                    return;
                }
                Answer::Go(true)
            }
            // The other: a form is up until something is done on it.
            Ask::Form(form) => {
                self.form = Some(*form);
                return;
            }
            Ask::Watching(values) => {
                self.watched = values;
                Answer::Nothing
            }
            // A message box and an input box are modal, as they are in
            // Word: the macro waits while the person reads or types, and is
            // told which button was pressed. The window puts them up.
            Ask::Message { text, buttons, title } => {
                self.prompt = Some(Prompt::Message { text, buttons, title });
                return;
            }
            Ask::Prompt { prompt, title, default } => {
                self.prompt = Some(Prompt::Input { prompt, title, default });
                return;
            }
            Ask::Note(text) => {
                self.model.on(editor).note(&text);
                Answer::Nothing
            }
            Ask::Root(name) => Answer::Maybe(self.model.on(editor).root(&name)),
            Ask::Member { object, member, given } => {
                Answer::One(Box::new(self.model.on(editor).member(&object, &member, &given)))
            }
            Ask::SetMember { object, member, value } => Answer::One(Box::new(
                self.model.on(editor).set_member(&object, &member, value).map(|()| Value::Empty),
            )),
            Ask::Items(object) => Answer::Many(Box::new(self.model.on(editor).items(&object))),
            Ask::AsText(object) => Answer::Text(Box::new(self.model.on(editor).as_text(&object))),
        };
        let _ = self.answers.send(answer);
    }

    /// Carries on: to the end, or to the next statement.
    pub fn go(&mut self, stepping: bool) {
        if self.stopped.take().is_none() {
            return;
        }
        self.stepping = stepping;
        let _ = self.answers.send(Answer::Go(true));
    }

    /// Stops the macro where it stands, which is Word's Reset.
    pub fn reset(&mut self) {
        if self.stopped.take().is_some() {
            let _ = self.answers.send(Answer::Go(false));
        }
        // A macro waiting on a form is told the form was shut, and its own
        // code decides what that means; nothing else can reach it there.
        if self.form.take().is_some() {
            let _ = self.answers.send(Answer::Happened(Box::new(Happening::Closed)));
        }
        // And one waiting on a question is told it was cancelled.
        match self.prompt.take() {
            Some(Prompt::Message { buttons, .. }) => {
                let _ = self.answers.send(Answer::Number(cancel_code(buttons)));
            }
            Some(Prompt::Input { .. }) => {
                let _ = self.answers.send(Answer::Words(None));
            }
            None => {}
        }
    }

    /// Whether it is waiting for the window: stopped, on a form, or on a
    /// question.
    #[must_use]
    pub fn waiting(&self) -> bool {
        self.stopped.is_some() || self.form.is_some() || self.prompt.is_some()
    }

    /// Answers the message box it is waiting on with Word's number for the
    /// button: `vbOK` is one, `vbCancel` two, and so on to `vbNo`, seven.
    pub fn message_answered(&mut self, button: i64) {
        if matches!(self.prompt.take(), Some(Prompt::Message { .. })) {
            let _ = self.answers.send(Answer::Number(button));
        }
    }

    /// Answers the input box it is waiting on, or says it was cancelled.
    pub fn input_answered(&mut self, words: Option<String>) {
        if matches!(self.prompt.take(), Some(Prompt::Input { .. })) {
            let _ = self.answers.send(Answer::Words(words));
        }
    }

    /// Answers the form it is waiting on.
    pub fn form_happened(&mut self, happening: Happening) {
        if self.form.take().is_some() {
            let _ = self.answers.send(Answer::Happened(Box::new(happening)));
        }
    }

    /// What the macro said and what it showed, for the window to put up.
    #[must_use]
    pub fn finished(&mut self) -> Option<Result<(Value, Vec<Value>), Fault>> {
        let ended = self.ended.take()?;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        Some(ended)
    }
}

/// Word's buttons for a `MsgBox`, by the low bits of its buttons code, each
/// with the number `MsgBox` answers when it is pressed: the labels are
/// `vbOKOnly` to `vbRetryCancel`, and the numbers `vbOK` to `vbNo`.
#[must_use]
pub fn message_buttons(buttons: i64) -> &'static [(&'static str, i64)] {
    match buttons & 0xF {
        1 => &[("OK", 1), ("Cancel", 2)],
        2 => &[("Abort", 3), ("Retry", 4), ("Ignore", 5)],
        3 => &[("Yes", 6), ("No", 7), ("Cancel", 2)],
        4 => &[("Yes", 6), ("No", 7)],
        5 => &[("Retry", 4), ("Cancel", 2)],
        _ => &[("OK", 1)],
    }
}

/// Which of them Enter presses: the first unless the code says the second
/// or the third, which `vbDefaultButton2` and `vbDefaultButton3` do.
#[must_use]
pub fn default_button(buttons: i64) -> usize {
    match buttons & 0xF00 {
        0x100 => 1,
        0x200 => 2,
        _ => 0,
    }
}

/// What Escape answers: the button that means no, where there is one, and
/// otherwise the only button there is.
#[must_use]
pub fn cancel_code(buttons: i64) -> i64 {
    let offered = message_buttons(buttons);
    offered
        .iter()
        .find(|(_, code)| *code == 2)
        .or_else(|| offered.last())
        .map_or(1, |(_, code)| *code)
}

impl Drop for Debugger {
    fn drop(&mut self) {
        // A window that is going away lets the macro go too: the channel
        // closing is what tells it, and a macro waiting for an answer that
        // will never come would otherwise keep a thread for ever.
        self.reset();
    }
}
