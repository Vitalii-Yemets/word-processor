//! The X Input Method protocol: how a program on X is given the text an
//! input method composes.
//!
//! # Why
//!
//! Chinese, Japanese and Korean are not typed a key to a character. The
//! keys spell a sound, or the strokes of a syllable, and a program of the
//! person's choosing — the input method — turns them into characters,
//! showing what it has so far and offering the characters that sound could
//! be. On X that program is a server of its own, and a program that does
//! not speak to it gets the bare keys: the languages half the world writes
//! in cannot be typed at all.
//!
//! # The shape of it
//!
//! The input method server is found by the name `XMODIFIERS` gives it, and a
//! line to it is opened out of X client messages and window properties —
//! the transport, which is the shell's: see [`super::xshell`]. On that line
//! this program says who it is, opens the input method in its locale,
//! agrees how text is written, asks which styles the server offers, and
//! makes an input context for each of its windows. From then on each key
//! the server asks for goes to it first, and what comes back is one of
//! three things: the key again, not wanted, to be handled as typed; text
//! committed, to go in as typed; or the text being composed, to be shown
//! where it will go — the on-the-spot style, which is Word's, and the one
//! asked for.
//!
//! This module is the protocol and nothing else: messages in, and what the
//! shell is to do out — send these bytes, give a window this event, handle
//! this key as typed. Written out against "The Input Method Protocol",
//! version 1.0, of the X Consortium.

use std::collections::{HashMap, VecDeque};

use super::keys;
use crate::{CompositionAttribute, Event, Modifiers};

/// The messages, by their major opcode.
mod opcode {
    pub(super) const CONNECT: u8 = 1;
    pub(super) const CONNECT_REPLY: u8 = 2;
    pub(super) const ERROR: u8 = 20;
    pub(super) const OPEN: u8 = 30;
    pub(super) const OPEN_REPLY: u8 = 31;
    pub(super) const REGISTER_TRIGGERKEYS: u8 = 34;
    pub(super) const TRIGGER_NOTIFY: u8 = 35;
    pub(super) const SET_EVENT_MASK: u8 = 37;
    pub(super) const ENCODING_NEGOTIATION: u8 = 38;
    pub(super) const ENCODING_NEGOTIATION_REPLY: u8 = 39;
    pub(super) const GET_IM_VALUES: u8 = 44;
    pub(super) const GET_IM_VALUES_REPLY: u8 = 45;
    pub(super) const CREATE_IC: u8 = 50;
    pub(super) const CREATE_IC_REPLY: u8 = 51;
    pub(super) const DESTROY_IC: u8 = 52;
    pub(super) const SET_IC_VALUES: u8 = 54;
    pub(super) const SET_IC_FOCUS: u8 = 58;
    pub(super) const UNSET_IC_FOCUS: u8 = 59;
    pub(super) const FORWARD_EVENT: u8 = 60;
    pub(super) const SYNC: u8 = 61;
    pub(super) const SYNC_REPLY: u8 = 62;
    pub(super) const COMMIT: u8 = 63;
    pub(super) const PREEDIT_START: u8 = 73;
    pub(super) const PREEDIT_START_REPLY: u8 = 74;
    pub(super) const PREEDIT_DRAW: u8 = 75;
    pub(super) const PREEDIT_CARET: u8 = 76;
    pub(super) const PREEDIT_CARET_REPLY: u8 = 77;
    pub(super) const PREEDIT_DONE: u8 = 78;
}

/// The input styles: who shows the text being composed, and the state.
const PREEDIT_CALLBACKS: u32 = 0x0002;
const PREEDIT_POSITION: u32 = 0x0004;
const PREEDIT_NOTHING: u32 = 0x0008;
const PREEDIT_NONE: u32 = 0x0010;
const STATUS_NOTHING: u32 = 0x0400;
const STATUS_NONE: u32 = 0x0800;

/// The styles this program takes, the one it would rather have first: the
/// text being composed shown by the program where it will go; else by the
/// input method in a window of its own; else not shown, which leaves the
/// committed text.
const PREFERRED_STYLES: [u32; 4] = [
    PREEDIT_CALLBACKS | STATUS_NOTHING,
    PREEDIT_CALLBACKS | STATUS_NONE,
    PREEDIT_NOTHING | STATUS_NOTHING,
    PREEDIT_NONE | STATUS_NONE,
];

/// The X event masks a server asks for keys by.
const KEY_PRESS_MASK: u32 = 0x1;
const KEY_RELEASE_MASK: u32 = 0x2;

/// What a flag on a forwarded key or a commit says: that the other side is
/// to answer before anything more is sent.
const SYNCHRONOUS: u16 = 0x0001;
/// And on a commit, what it carries.
const LOOKUP_CHARS: u16 = 0x0002;
const LOOKUP_KEYSYM: u16 = 0x0004;

/// What a character of the text being composed is marked with.
const FEEDBACK_REVERSE: u32 = 0x0001;
const FEEDBACK_HIGHLIGHT: u32 = 0x0004;

/// What the shell is to do.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Action {
    /// Send this message to the server.
    Send(Vec<u8>),
    /// Give a window this event.
    Deliver(u32, Event),
    /// Handle this key event for a window as typed: the input method gave
    /// it back.
    Key(u32, [u8; 32]),
}

/// How far the conversation with the server has got.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Connecting,
    Opening,
    Negotiating,
    Styling,
    Ready,
    /// The server said no to something that had to be yes; keys are typed
    /// as though there were no input method.
    Failed,
}

/// A key that switches forwarding on or off, where the server works that
/// way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Trigger {
    keysym: u32,
    modifier: u32,
    mask: u32,
}

impl Trigger {
    fn matches(self, keysym: u32, state: u16) -> bool {
        self.keysym == keysym && u32::from(state) & self.mask == self.modifier
    }
}

/// One window's input context.
#[derive(Clone, Debug)]
struct Context {
    window: u32,
    id: u16,
    focused: bool,
    /// Where the server works by trigger keys, whether one has switched
    /// forwarding on for this context.
    triggered: bool,
    /// The text being composed, as the server has drawn it so far, a mark
    /// for each character.
    preedit: Vec<char>,
    feedback: Vec<u32>,
    caret: usize,
    /// Where the server was last told the caret is.
    spot: Option<(i16, i16)>,
}

/// The conversation with one input method server.
#[derive(Debug)]
pub(crate) struct Xim {
    locale: String,
    step: Step,
    im: u16,
    im_attributes: HashMap<String, u16>,
    ic_attributes: HashMap<String, u16>,
    /// Whether text comes as UTF-8 rather than as compound text.
    utf8: bool,
    style: u32,
    /// Windows that want an input context, the one being made at the front
    /// while `making` is set.
    waiting: VecDeque<u32>,
    making: bool,
    contexts: Vec<Context>,
    /// The window with the keyboard, as the focus events last said.
    focus: Option<u32>,
    forward_mask: u32,
    synchronous_mask: u32,
    on_keys: Vec<Trigger>,
    off_keys: Vec<Trigger>,
    /// A key forwarded that the server has yet to answer, where it asked
    /// to be answered first; the keys pressed meanwhile wait.
    in_flight: bool,
    held: VecDeque<(u32, [u8; 32])>,
    /// Keys pressed in a window whose context is still to be made — while
    /// the conversation is being opened, or the context asked for — which
    /// wait for it. See [`Early`].
    early: VecDeque<Early>,
}

/// A key pressed before its window's input context was made, kept with
/// what it means so that it can be put through the input method as though
/// it were pressed once the context is there.
///
/// Xlib makes a window's context before the window takes a key, and so a
/// program typing through it never has keys that come too soon. This
/// program makes it by messages that go back and forth in the event loop,
/// and the keys pressed meanwhile used to be typed as they were: Shift and
/// Space pressed as the window came up, to switch a Korean method on, went
/// in as a space, and the letters after them as Latin letters.
#[derive(Clone, Debug, PartialEq)]
struct Early {
    window: u32,
    event: [u8; 32],
    keysym: u32,
    state: u16,
    press: bool,
}

impl Xim {
    /// A conversation about to start, in the program's locale.
    pub(crate) fn new(locale: &str) -> Self {
        Self {
            locale: locale.to_owned(),
            step: Step::Connecting,
            im: 0,
            im_attributes: HashMap::new(),
            ic_attributes: HashMap::new(),
            utf8: false,
            style: 0,
            waiting: VecDeque::new(),
            making: false,
            contexts: Vec::new(),
            focus: None,
            // Until the server says otherwise, a key press goes to it and is
            // answered before the next.
            forward_mask: KEY_PRESS_MASK,
            synchronous_mask: KEY_PRESS_MASK,
            on_keys: Vec::new(),
            off_keys: Vec::new(),
            in_flight: false,
            held: VecDeque::new(),
            early: VecDeque::new(),
        }
    }

    /// The first message, once the transport is open: who this is, and in
    /// which byte order everything after is written — least significant
    /// first, as the X connection is.
    pub(crate) fn connect(&mut self) -> Vec<u8> {
        self.step = Step::Connecting;
        Message::new(opcode::CONNECT).u8(0x6C).u8(0).u16(1).u16(0).u16(0).finish()
    }

    /// Whether keys go through the input method now.
    #[cfg(test)]
    pub(crate) fn is_ready(&self) -> bool {
        self.step == Step::Ready
    }

    /// Whether the conversation has broken down for good.
    pub(crate) fn has_failed(&self) -> bool {
        self.step == Step::Failed
    }

    /// Whether a key is waiting on the server: gone to it and not come
    /// back, or waiting for its window's context to be made.
    pub(crate) fn is_waiting(&self) -> bool {
        self.in_flight || !self.early.is_empty()
    }

    /// A window that is to type through the input method.
    pub(crate) fn add_window(&mut self, window: u32) -> Vec<Action> {
        if self.contexts.iter().any(|context| context.window == window)
            || self.waiting.contains(&window)
        {
            return Vec::new();
        }
        self.waiting.push_back(window);
        self.make_next_context()
    }

    /// A window that has gone.
    pub(crate) fn remove_window(&mut self, window: u32) -> Vec<Action> {
        self.held.retain(|(held, _)| *held != window);
        self.early.retain(|early| early.window != window);
        if self.focus == Some(window) {
            self.focus = None;
        }
        let Some(index) = self.contexts.iter().position(|context| context.window == window) else {
            return Vec::new();
        };
        let context = self.contexts.remove(index);
        vec![Action::Send(Message::new(opcode::DESTROY_IC).u16(self.im).u16(context.id).finish())]
    }

    /// The keyboard went to a window, or left it.
    pub(crate) fn focus(&mut self, window: u32, on: bool) -> Vec<Action> {
        if on {
            self.focus = Some(window);
        } else if self.focus == Some(window) {
            self.focus = None;
        }
        let im = self.im;
        let ready = self.step == Step::Ready;
        let Some(context) = self.contexts.iter_mut().find(|context| context.window == window)
        else {
            return Vec::new();
        };
        if !ready || context.focused == on {
            return Vec::new();
        }
        context.focused = on;
        let kind = if on { opcode::SET_IC_FOCUS } else { opcode::UNSET_IC_FOCUS };
        vec![Action::Send(Message::new(kind).u16(im).u16(context.id).finish())]
    }

    /// A key event on a window. Whether the input method took it — it is
    /// then not to be handled as typed, since it will come back if it is
    /// not wanted — and what to send.
    pub(crate) fn key(
        &mut self,
        window: u32,
        event: &[u8; 32],
        keysym: u32,
        state: u16,
        press: bool,
    ) -> (bool, Vec<Action>) {
        let mut actions = Vec::new();
        if self.step == Step::Failed {
            return (false, actions);
        }
        let im = self.im;
        let Some(index) = self.contexts.iter().position(|context| context.window == window) else {
            // A window whose context is still to be made keeps its keys
            // until it is: see [`Early`].
            if self.waiting.contains(&window) {
                self.early.push_back(Early { window, event: *event, keysym, state, press });
                return (true, actions);
            }
            return (false, actions);
        };
        // A key means the window has the keyboard, whatever the focus
        // events did or did not say.
        if !self.contexts[index].focused {
            self.contexts[index].focused = true;
            self.focus = Some(window);
            let id = self.contexts[index].id;
            actions.push(Action::Send(Message::new(opcode::SET_IC_FOCUS).u16(im).u16(id).finish()));
        }
        let id = self.contexts[index].id;
        // A server that works by trigger keys is sent nothing until one is
        // pressed, and nothing after the key that switches it off.
        if !self.on_keys.is_empty() {
            if !self.contexts[index].triggered {
                let found = self.on_keys.iter().position(|key| key.matches(keysym, state));
                if let (true, Some(which)) = (press, found) {
                    self.contexts[index].triggered = true;
                    actions.push(Action::Send(trigger_notify(
                        im,
                        id,
                        0,
                        which,
                        KEY_PRESS_MASK | KEY_RELEASE_MASK,
                    )));
                    return (true, actions);
                }
                return (false, actions);
            }
            let found = self.off_keys.iter().position(|key| key.matches(keysym, state));
            if let (true, Some(which)) = (press, found) {
                self.contexts[index].triggered = false;
                actions.push(Action::Send(trigger_notify(im, id, 1, which, 0)));
                return (true, actions);
            }
        }
        let mask = if press { KEY_PRESS_MASK } else { KEY_RELEASE_MASK };
        if self.forward_mask & mask == 0 {
            return (false, actions);
        }
        if self.in_flight {
            self.held.push_back((window, *event));
            return (true, actions);
        }
        actions.push(self.forward(id, event, mask));
        (true, actions)
    }

    /// The message that sends a key to the server, marked to be answered
    /// first where the server asked for that.
    fn forward(&mut self, id: u16, event: &[u8; 32], mask: u32) -> Action {
        let synchronous = self.synchronous_mask & mask != 0;
        if synchronous {
            self.in_flight = true;
        }
        let flag = if synchronous { SYNCHRONOUS } else { 0 };
        Action::Send(
            Message::new(opcode::FORWARD_EVENT)
                .u16(self.im)
                .u16(id)
                .u16(flag)
                // The high half of the event's serial number; the low half
                // is in the event itself.
                .u16(0)
                .bytes(event)
                .finish(),
        )
    }

    /// Gives up on the server: whatever was waiting to go to it is handed
    /// back to be typed.
    pub(crate) fn abandon(&mut self) -> Vec<Action> {
        self.step = Step::Failed;
        self.in_flight = false;
        let early = self.early.drain(..).map(|early| (early.window, early.event));
        let held: Vec<_> = early.chain(self.held.drain(..)).collect();
        held.into_iter().map(|(window, event)| Action::Key(window, event)).collect()
    }

    /// Where the caret is in a window, in its pixels, so that the input
    /// method's list of candidates opens beside it. Said only when it moved.
    pub(crate) fn spot(&mut self, window: u32, x: i16, y: i16) -> Vec<Action> {
        if self.step != Step::Ready || self.style & (PREEDIT_CALLBACKS | PREEDIT_POSITION) == 0 {
            return Vec::new();
        }
        let (Some(&nest), Some(&spot)) =
            (self.ic_attributes.get("preeditAttributes"), self.ic_attributes.get("spotLocation"))
        else {
            return Vec::new();
        };
        let im = self.im;
        let Some(context) = self.contexts.iter_mut().find(|context| context.window == window)
        else {
            return Vec::new();
        };
        if context.spot == Some((x, y)) {
            return Vec::new();
        }
        context.spot = Some((x, y));
        // The spot is one of the attributes of the text being composed,
        // which go as a list inside one attribute.
        let inner = Message::attribute(spot, &point(x, y));
        let attributes = Message::attribute(nest, &inner);
        vec![Action::Send(
            Message::new(opcode::SET_IC_VALUES)
                .u16(im)
                .u16(context.id)
                .u16(attributes.len() as u16)
                .u16(0)
                .bytes(&attributes)
                .finish(),
        )]
    }

    /// A message from the server, and what it asks for.
    pub(crate) fn receive(&mut self, message: &[u8]) -> Vec<Action> {
        let Some(&major) = message.first() else { return Vec::new() };
        let length =
            message.get(2..4).map_or(0, |two| usize::from(u16::from_le_bytes([two[0], two[1]])));
        let body = message.get(4..(4 + length * 4).min(message.len())).unwrap_or(&[]);
        let mut read = Reader::new(body);
        match major {
            opcode::CONNECT_REPLY => {
                self.step = Step::Opening;
                let locale = self.locale.clone();
                vec![Action::Send(Message::new(opcode::OPEN).str8(&locale).finish())]
            }
            opcode::OPEN_REPLY => self.opened(&mut read),
            opcode::ENCODING_NEGOTIATION_REPLY => {
                let _im = read.u16();
                let category = read.u16().unwrap_or(0);
                let index = read.i16().unwrap_or(-1);
                // The encodings offered were UTF-8 then compound text; the
                // answer is which, by where it was in the list.
                self.utf8 = category == 0 && index == 0;
                self.step = Step::Styling;
                self.ask_styles()
            }
            opcode::GET_IM_VALUES_REPLY => self.styles(&mut read),
            opcode::CREATE_IC_REPLY => self.context_made(&mut read),
            opcode::SET_EVENT_MASK => {
                let _im = read.u16();
                let _ic = read.u16();
                if let (Some(forward), Some(synchronous)) = (read.u32(), read.u32()) {
                    self.forward_mask = forward;
                    self.synchronous_mask = synchronous;
                }
                Vec::new()
            }
            opcode::REGISTER_TRIGGERKEYS => {
                let _im = read.u16();
                read.skip(2);
                self.on_keys = triggers(&mut read);
                self.off_keys = triggers(&mut read);
                Vec::new()
            }
            opcode::FORWARD_EVENT => self.given_back(&mut read),
            opcode::COMMIT => self.committed(&mut read),
            opcode::SYNC => {
                let im = read.u16().unwrap_or(self.im);
                let ic = read.u16().unwrap_or(0);
                vec![Action::Send(sync_reply(im, ic))]
            }
            opcode::SYNC_REPLY => {
                self.in_flight = false;
                self.release_held()
            }
            opcode::PREEDIT_START => {
                let im = read.u16().unwrap_or(self.im);
                let ic = read.u16().unwrap_or(0);
                // No limit on how long the text being composed may be.
                vec![Action::Send(
                    Message::new(opcode::PREEDIT_START_REPLY).u16(im).u16(ic).i32(-1).finish(),
                )]
            }
            opcode::PREEDIT_DRAW => self.preedit_drawn(&mut read),
            opcode::PREEDIT_CARET => self.preedit_caret(&mut read),
            opcode::PREEDIT_DONE => {
                let _im = read.u16();
                let ic = read.u16().unwrap_or(0);
                let Some(context) = self.contexts.iter_mut().find(|context| context.id == ic)
                else {
                    return Vec::new();
                };
                context.preedit.clear();
                context.feedback.clear();
                context.caret = 0;
                vec![Action::Deliver(context.window, Event::ComposeEnd)]
            }
            opcode::ERROR => {
                // Before the conversation is under way an error means there
                // is no input method to be had; after, it is about one
                // request, and the next goes on regardless.
                if self.step != Step::Ready {
                    return self.abandon();
                }
                // A key that was being answered is not going to be.
                if self.in_flight {
                    self.in_flight = false;
                    return self.release_held();
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// The input method is open: its number, and the names of the
    /// attributes it has, by the numbers it gave them.
    fn opened(&mut self, read: &mut Reader) -> Vec<Action> {
        let Some(im) = read.u16() else { return self.abandon() };
        self.im = im;
        let im_length = usize::from(read.u16().unwrap_or(0));
        self.im_attributes = attribute_names(read.take(im_length).unwrap_or(&[]));
        let ic_length = usize::from(read.u16().unwrap_or(0));
        read.skip(2);
        self.ic_attributes = attribute_names(read.take(ic_length).unwrap_or(&[]));
        self.step = Step::Negotiating;
        let mut names = Vec::new();
        for name in ["UTF-8", "COMPOUND_TEXT"] {
            names.push(name.len() as u8);
            names.extend_from_slice(name.as_bytes());
        }
        let message = Message::new(opcode::ENCODING_NEGOTIATION)
            .u16(im)
            .u16(names.len() as u16)
            .bytes(&names)
            .align()
            .u16(0)
            .u16(0)
            .finish();
        vec![Action::Send(message)]
    }

    /// Asks which styles the input method offers.
    fn ask_styles(&mut self) -> Vec<Action> {
        let Some(&query) = self.im_attributes.get("queryInputStyle") else {
            // Nothing to ask: take the style every server has.
            self.style = PREEDIT_NOTHING | STATUS_NOTHING;
            self.step = Step::Ready;
            return self.make_next_context();
        };
        vec![Action::Send(
            Message::new(opcode::GET_IM_VALUES).u16(self.im).u16(2).u16(query).align().finish(),
        )]
    }

    /// The styles offered, and the one taken.
    fn styles(&mut self, read: &mut Reader) -> Vec<Action> {
        let _im = read.u16();
        let length = usize::from(read.u16().unwrap_or(0));
        let mut list = Reader::new(read.take(length).unwrap_or(&[]));
        let query = self.im_attributes.get("queryInputStyle").copied();
        let mut offered = Vec::new();
        while let (Some(id), Some(size)) = (list.u16(), list.u16()) {
            let value = list.take(usize::from(size)).unwrap_or(&[]);
            list.skip((4 - usize::from(size) % 4) % 4);
            if Some(id) != query {
                continue;
            }
            let mut styles = Reader::new(value);
            let count = styles.u16().unwrap_or(0);
            styles.skip(2);
            for _ in 0..count {
                if let Some(style) = styles.u32() {
                    offered.push(style);
                }
            }
        }
        let Some(style) = PREFERRED_STYLES.into_iter().find(|style| offered.contains(style)) else {
            return self.abandon();
        };
        self.style = style;
        self.step = Step::Ready;
        self.make_next_context()
    }

    /// Asks for the next window's input context, one at a time.
    fn make_next_context(&mut self) -> Vec<Action> {
        if self.step != Step::Ready || self.making {
            return Vec::new();
        }
        let Some(&window) = self.waiting.front() else { return Vec::new() };
        let (Some(&style), Some(&client), Some(&focus)) = (
            self.ic_attributes.get("inputStyle"),
            self.ic_attributes.get("clientWindow"),
            self.ic_attributes.get("focusWindow"),
        ) else {
            return self.abandon();
        };
        self.making = true;
        let mut attributes = Message::attribute(style, &self.style.to_le_bytes());
        attributes.extend(Message::attribute(client, &window.to_le_bytes()));
        attributes.extend(Message::attribute(focus, &window.to_le_bytes()));
        vec![Action::Send(
            Message::new(opcode::CREATE_IC)
                .u16(self.im)
                .u16(attributes.len() as u16)
                .bytes(&attributes)
                .finish(),
        )]
    }

    /// A window's input context is made.
    fn context_made(&mut self, read: &mut Reader) -> Vec<Action> {
        let _im = read.u16();
        let Some(id) = read.u16() else { return Vec::new() };
        self.making = false;
        let Some(window) = self.waiting.pop_front() else { return Vec::new() };
        let focused = self.focus == Some(window);
        self.contexts.push(Context {
            window,
            id,
            focused,
            triggered: false,
            preedit: Vec::new(),
            feedback: Vec::new(),
            caret: 0,
            spot: None,
        });
        let mut actions = Vec::new();
        if focused {
            actions.push(Action::Send(
                Message::new(opcode::SET_IC_FOCUS).u16(self.im).u16(id).finish(),
            ));
        }
        // The keys that waited for this context, in the order they were
        // pressed, as though pressed now; one the input method does not
        // take is typed as it was.
        let (mine, others): (VecDeque<Early>, VecDeque<Early>) =
            std::mem::take(&mut self.early).into_iter().partition(|early| early.window == window);
        self.early = others;
        for early in mine {
            let (taken, sent) =
                self.key(early.window, &early.event, early.keysym, early.state, early.press);
            actions.extend(sent);
            if !taken {
                actions.push(Action::Key(early.window, early.event));
            }
        }
        actions.extend(self.make_next_context());
        actions
    }

    /// A key the input method did not want, to be handled as typed.
    fn given_back(&mut self, read: &mut Reader) -> Vec<Action> {
        let im = read.u16().unwrap_or(self.im);
        let ic = read.u16().unwrap_or(0);
        let flag = read.u16().unwrap_or(0);
        let _serial = read.u16();
        let mut actions = Vec::new();
        if let (Some(event), Some(window)) = (read.take(32), self.window_of(ic)) {
            let mut bytes = [0u8; 32];
            bytes.copy_from_slice(event);
            actions.push(Action::Key(window, bytes));
        }
        if flag & SYNCHRONOUS != 0 {
            actions.push(Action::Send(sync_reply(im, ic)));
        }
        actions
    }

    /// Text the input method finished, to go in as typed.
    fn committed(&mut self, read: &mut Reader) -> Vec<Action> {
        let im = read.u16().unwrap_or(self.im);
        let ic = read.u16().unwrap_or(0);
        let flag = read.u16().unwrap_or(0);
        let window = self.window_of(ic);
        let mut actions = Vec::new();
        let mut keysym = None;
        let mut text = String::new();
        if flag & LOOKUP_KEYSYM != 0 {
            read.skip(2);
            keysym = read.u32();
            if flag & LOOKUP_CHARS != 0 {
                let length = usize::from(read.u16().unwrap_or(0));
                text = self.text_of(read.take(length).unwrap_or(&[]));
            }
        } else if flag & LOOKUP_CHARS != 0 {
            let length = usize::from(read.u16().unwrap_or(0));
            text = self.text_of(read.take(length).unwrap_or(&[]));
        }
        if let Some(window) = window {
            if !text.is_empty() {
                actions.push(Action::Deliver(window, Event::Commit(text)));
            } else if let Some(keysym) = keysym {
                // A key the input method stands for rather than text: the
                // same as that key typed.
                if let Some(key) = keys::key_of(keysym) {
                    actions.push(Action::Deliver(
                        window,
                        Event::KeyDown { key, modifiers: Modifiers::default() },
                    ));
                } else if let Some(character) = keys::char_of(keysym) {
                    actions.push(Action::Deliver(window, Event::Commit(character.to_string())));
                }
            }
        }
        if flag & SYNCHRONOUS != 0 {
            actions.push(Action::Send(sync_reply(im, ic)));
        }
        actions
    }

    /// The server changed the text being composed: some characters of it
    /// replaced by others.
    fn preedit_drawn(&mut self, read: &mut Reader) -> Vec<Action> {
        let _im = read.u16();
        let ic = read.u16().unwrap_or(0);
        let caret = read.i32().unwrap_or(0);
        let first = read.i32().unwrap_or(0);
        let length = read.i32().unwrap_or(0);
        let status = read.u32().unwrap_or(0);
        let string_length = usize::from(read.u16().unwrap_or(0));
        let string = read.take(string_length).unwrap_or(&[]).to_vec();
        read.skip((4 - (2 + string_length) % 4) % 4);
        let feedback_length = usize::from(read.u16().unwrap_or(0));
        read.skip(2);
        let mut feedback = Vec::new();
        for _ in 0..feedback_length / 4 {
            feedback.push(read.u32().unwrap_or(0));
        }
        let text: Vec<char> =
            if status & 0x1 != 0 { Vec::new() } else { self.text_of(&string).chars().collect() };
        if status & 0x2 != 0 || feedback.len() != text.len() {
            feedback.resize(text.len(), 0);
        }
        let Some(context) = self.contexts.iter_mut().find(|context| context.id == ic) else {
            return Vec::new();
        };
        let total = context.preedit.len();
        let start = usize::try_from(first).unwrap_or(0).min(total);
        let end = start.saturating_add(usize::try_from(length).unwrap_or(0)).min(total);
        context.preedit.splice(start..end, text);
        context.feedback.resize(total, 0);
        context.feedback.splice(start..end, feedback);
        context.caret = usize::try_from(caret).unwrap_or(0).min(context.preedit.len());
        vec![Action::Deliver(context.window, composition(context))]
    }

    /// The server moved the caret in the text being composed.
    fn preedit_caret(&mut self, read: &mut Reader) -> Vec<Action> {
        let im = read.u16().unwrap_or(self.im);
        let ic = read.u16().unwrap_or(0);
        let position = read.i32().unwrap_or(0);
        let direction = read.u32().unwrap_or(0);
        let Some(context) = self.contexts.iter_mut().find(|context| context.id == ic) else {
            return Vec::new();
        };
        let length = context.preedit.len();
        // Absolute, or a step one way or the other; the rest are about
        // lines and words, which a line of composition does not have.
        context.caret = match direction {
            0 => context.caret.saturating_add(1).min(length),
            1 => context.caret.saturating_sub(1),
            8 => 0,
            9 => length,
            10 => usize::try_from(position).unwrap_or(0).min(length),
            _ => context.caret,
        };
        let caret = context.caret as u32;
        vec![
            Action::Deliver(context.window, composition(context)),
            Action::Send(
                Message::new(opcode::PREEDIT_CARET_REPLY).u16(im).u16(ic).u32(caret).finish(),
            ),
        ]
    }

    /// The keys that waited for the server's answer, sent now it has come.
    fn release_held(&mut self) -> Vec<Action> {
        let mut actions = Vec::new();
        while !self.in_flight {
            let Some((window, event)) = self.held.pop_front() else { break };
            let press = event[0] & 0x7F == 2;
            let mask = if press { KEY_PRESS_MASK } else { KEY_RELEASE_MASK };
            match self.contexts.iter().find(|context| context.window == window) {
                Some(context) if self.step == Step::Ready => {
                    let id = context.id;
                    actions.push(self.forward(id, &event, mask));
                }
                _ => actions.push(Action::Key(window, event)),
            }
        }
        actions
    }

    fn window_of(&self, ic: u16) -> Option<u32> {
        self.contexts.iter().find(|context| context.id == ic).map(|context| context.window)
    }

    fn text_of(&self, bytes: &[u8]) -> String {
        if self.utf8 {
            String::from_utf8_lossy(bytes).into_owned()
        } else {
            compound_text(bytes)
        }
    }
}

/// The text being composed in a context, as the event the window is given.
fn composition(context: &Context) -> Event {
    let attributes = context
        .feedback
        .iter()
        .map(|mark| {
            if mark & FEEDBACK_REVERSE != 0 {
                CompositionAttribute::Target
            } else if mark & FEEDBACK_HIGHLIGHT != 0 {
                CompositionAttribute::Converted
            } else {
                CompositionAttribute::Input
            }
        })
        .collect();
    Event::Compose { text: context.preedit.iter().collect(), caret: context.caret, attributes }
}

fn sync_reply(im: u16, ic: u16) -> Vec<u8> {
    Message::new(opcode::SYNC_REPLY).u16(im).u16(ic).finish()
}

fn trigger_notify(im: u16, ic: u16, flag: u32, index: usize, mask: u32) -> Vec<u8> {
    Message::new(opcode::TRIGGER_NOTIFY)
        .u16(im)
        .u16(ic)
        .u32(flag)
        .u32(index as u32)
        .u32(mask)
        .finish()
}

/// A point, as an attribute's value.
fn point(x: i16, y: i16) -> Vec<u8> {
    let mut value = x.to_le_bytes().to_vec();
    value.extend_from_slice(&y.to_le_bytes());
    value
}

/// The names of a list of attributes, by the numbers the server gave them.
fn attribute_names(list: &[u8]) -> HashMap<String, u16> {
    let mut read = Reader::new(list);
    let mut names = HashMap::new();
    while let (Some(id), Some(_kind), Some(length)) = (read.u16(), read.u16(), read.u16()) {
        let length = usize::from(length);
        let Some(name) = read.take(length) else { break };
        read.skip((4 - (2 + length) % 4) % 4);
        names.insert(String::from_utf8_lossy(name).into_owned(), id);
    }
    names
}

/// A list of trigger keys: a length in bytes, then twelve bytes a key.
fn triggers(read: &mut Reader) -> Vec<Trigger> {
    let length = read.u32().unwrap_or(0) as usize;
    let mut list = Reader::new(read.take(length).unwrap_or(&[]));
    let mut keys = Vec::new();
    while let (Some(keysym), Some(modifier), Some(mask)) = (list.u32(), list.u32(), list.u32()) {
        keys.push(Trigger { keysym, modifier, mask });
    }
    keys
}

/// A message being put together.
struct Message {
    bytes: Vec<u8>,
}

impl Message {
    fn new(major: u8) -> Self {
        Self { bytes: vec![major, 0, 0, 0] }
    }

    fn u8(mut self, value: u8) -> Self {
        self.bytes.push(value);
        self
    }

    fn u16(mut self, value: u16) -> Self {
        self.bytes.extend_from_slice(&value.to_le_bytes());
        self
    }

    fn u32(mut self, value: u32) -> Self {
        self.bytes.extend_from_slice(&value.to_le_bytes());
        self
    }

    fn i32(self, value: i32) -> Self {
        self.u32(value as u32)
    }

    fn bytes(mut self, data: &[u8]) -> Self {
        self.bytes.extend_from_slice(data);
        self
    }

    /// A string as the protocol writes the short ones: its length in a
    /// byte, then its bytes.
    fn str8(mut self, text: &str) -> Self {
        let bytes = &text.as_bytes()[..text.len().min(255)];
        self.bytes.push(bytes.len() as u8);
        self.bytes.extend_from_slice(bytes);
        self
    }

    /// Up to a four-byte boundary.
    fn align(mut self) -> Self {
        while self.bytes.len() % 4 != 0 {
            self.bytes.push(0);
        }
        self
    }

    /// The finished message, its length filled in, in four-byte units after
    /// the header.
    fn finish(self) -> Vec<u8> {
        let mut bytes = self.align().bytes;
        let units = ((bytes.len() - 4) / 4) as u16;
        bytes[2..4].copy_from_slice(&units.to_le_bytes());
        bytes
    }

    /// One attribute and its value, as a list of them holds it: the
    /// attribute's number, the value's length, the value, and padding.
    fn attribute(id: u16, value: &[u8]) -> Vec<u8> {
        let mut out = id.to_le_bytes().to_vec();
        out.extend_from_slice(&(value.len() as u16).to_le_bytes());
        out.extend_from_slice(value);
        while out.len() % 4 != 0 {
            out.push(0);
        }
        out
    }
}

/// Reads a message's body, least significant byte first.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let slice = self.bytes.get(self.at..self.at.checked_add(count)?)?;
        self.at += count;
        Some(slice)
    }

    fn skip(&mut self, count: usize) {
        self.at = self.at.saturating_add(count);
    }

    fn u16(&mut self) -> Option<u16> {
        self.take(2).map(|two| u16::from_le_bytes([two[0], two[1]]))
    }

    fn i16(&mut self) -> Option<i16> {
        self.u16().map(|value| value as i16)
    }

    fn u32(&mut self) -> Option<u32> {
        self.take(4).map(|four| u32::from_le_bytes([four[0], four[1], four[2], four[3]]))
    }

    fn i32(&mut self) -> Option<i32> {
        self.u32().map(|value| value as i32)
    }
}

// --- Compound text ----------------------------------------------------------

/// The character sets compound text switches between.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Set {
    Ascii,
    /// JIS X 0201's Roman half: ASCII with a yen sign and an overline.
    JisRoman,
    /// JIS X 0201's katakana, in the right half.
    JisKana,
    /// The right half of an ISO 8859 part.
    Latin(wp_text::Encoding),
    /// The two-byte sets, as the code page whose two-byte half they are.
    Gb2312,
    Jis0208,
    Ksc5601,
    /// A set this does not read: its characters are shown as the
    /// replacement character.
    Unknown {
        wide: bool,
    },
}

/// Compound text, the X Consortium's encoding for text between programs:
/// ISO 2022, in which escape sequences say which character set the bytes
/// after them are in, the left half and the right half separately.
///
/// What is read: ASCII and the ISO 8859 right halves this program has
/// tables for; the three East Asian national sets — GB 2312, JIS X 0208 and
/// KS C 5601 — through the code pages that contain them; JIS X 0201; the
/// UTF-8 segment X programs put everything else in; and the extended
/// segments naming Big5, GBK or UTF-8. A set it does not know is read as
/// replacement characters rather than as the wrong ones.
pub(crate) fn compound_text(bytes: &[u8]) -> String {
    use wp_text::Encoding;
    let mut out = String::new();
    let mut left = Set::Ascii;
    let mut right = Set::Latin(Encoding::CodePage(28591));
    let mut at = 0;
    while at < bytes.len() {
        let byte = bytes[at];
        match byte {
            0x1B => {
                let rest = &bytes[at + 1..];
                match rest {
                    // UTF-8 until the way back.
                    [b'%', b'G', ..] => {
                        let start = at + 3;
                        let end = find(bytes, start, &[0x1B, b'%', b'@']).unwrap_or(bytes.len());
                        out.push_str(&String::from_utf8_lossy(&bytes[start..end]));
                        at = (end + 3).min(bytes.len());
                        continue;
                    }
                    // An extended segment: its length, its encoding's name,
                    // then its bytes.
                    [b'%', b'/', _, m, l, ..] if *m >= 0x80 && *l >= 0x80 => {
                        let length = usize::from(m - 0x80) * 128 + usize::from(l - 0x80);
                        let start = at + 6;
                        let end = (start + length).min(bytes.len());
                        let segment = &bytes[start..end];
                        let (name, data) = match segment.iter().position(|&b| b == 0x02) {
                            Some(stx) => (&segment[..stx], &segment[stx + 1..]),
                            None => (segment, &[][..]),
                        };
                        let name = String::from_utf8_lossy(name).to_ascii_lowercase();
                        out.push_str(&match name.as_str() {
                            "big5-0" => Encoding::CodePage(950).decode(data),
                            "gbk-0" => Encoding::CodePage(936).decode(data),
                            "utf-8" => String::from_utf8_lossy(data).into_owned(),
                            "iso10646-1" => data
                                .chunks_exact(2)
                                .map(|pair| {
                                    char::from_u32(u32::from(u16::from_be_bytes([
                                        pair[0], pair[1],
                                    ])))
                                    .unwrap_or('\u{FFFD}')
                                })
                                .collect(),
                            _ => "\u{FFFD}".repeat(data.len().max(1)),
                        });
                        at = end;
                        continue;
                    }
                    // A 94-character set to the left or the right half.
                    [b'(', final_byte, ..] => {
                        left = match final_byte {
                            b'B' => Set::Ascii,
                            b'J' => Set::JisRoman,
                            b'I' => Set::JisKana,
                            _ => Set::Unknown { wide: false },
                        };
                        at += 3;
                        continue;
                    }
                    [b')', final_byte, ..] => {
                        right = match final_byte {
                            b'I' => Set::JisKana,
                            b'B' => Set::Ascii,
                            _ => Set::Unknown { wide: false },
                        };
                        at += 3;
                        continue;
                    }
                    // A 96-character set, always to the right.
                    [b'-', final_byte, ..] => {
                        let page = match final_byte {
                            b'A' => Some(28591),
                            b'B' => Some(28592),
                            b'L' => Some(28595),
                            b'F' => Some(28597),
                            b'M' => Some(28599),
                            b'b' => Some(28605),
                            _ => None,
                        };
                        right = match page.and_then(Encoding::code_page) {
                            Some(encoding) => Set::Latin(encoding),
                            None => Set::Unknown { wide: false },
                        };
                        at += 3;
                        continue;
                    }
                    // A 94-by-94 set, to the left or the right.
                    [b'$', side @ (b'(' | b')'), final_byte, ..] => {
                        let set = match final_byte {
                            b'A' => Set::Gb2312,
                            b'B' => Set::Jis0208,
                            b'C' => Set::Ksc5601,
                            _ => Set::Unknown { wide: true },
                        };
                        if *side == b'(' {
                            left = set;
                        } else {
                            right = set;
                        }
                        at += 4;
                        continue;
                    }
                    _ => {
                        // An escape this does not know: stepped over.
                        at += 1;
                        continue;
                    }
                }
            }
            // Which way the text runs: said, and not needed to read it.
            0x9B => {
                at += 1;
                while at < bytes.len() && !(0x40..=0x7E).contains(&bytes[at]) {
                    at += 1;
                }
                at += 1;
                continue;
            }
            b'\t' | b'\n' => {
                out.push(byte as char);
                at += 1;
                continue;
            }
            _ => {}
        }
        let (set, high) = if byte >= 0xA0 { (right, true) } else { (left, false) };
        if byte < 0x20 || (0x7F..0xA0).contains(&byte) {
            at += 1;
            continue;
        }
        let low = byte & 0x7F;
        match set {
            Set::Ascii => out.push(low as char),
            Set::JisRoman => out.push(match low {
                0x5C => '\u{A5}',
                0x7E => '\u{203E}',
                other => other as char,
            }),
            Set::JisKana => {
                out.push(char::from_u32(0xFF61 + u32::from(low) - 0x21).unwrap_or('\u{FFFD}'));
            }
            Set::Latin(encoding) => {
                out.push_str(&encoding.decode(&[low | if high { 0x80 } else { 0 }]));
            }
            Set::Gb2312 | Set::Jis0208 | Set::Ksc5601 | Set::Unknown { wide: true } => {
                let Some(&next) = bytes.get(at + 1) else { break };
                let (first, second) = (low, next & 0x7F);
                out.push_str(&match set {
                    Set::Gb2312 => Encoding::CodePage(936).decode(&[first | 0x80, second | 0x80]),
                    Set::Ksc5601 => Encoding::CodePage(949).decode(&[first | 0x80, second | 0x80]),
                    Set::Jis0208 => {
                        let (lead, trail) = shift_jis(first, second);
                        Encoding::CodePage(932).decode(&[lead, trail])
                    }
                    _ => "\u{FFFD}".to_owned(),
                });
                at += 2;
                continue;
            }
            Set::Unknown { wide: false } => out.push('\u{FFFD}'),
        }
        at += 1;
    }
    out
}

/// Where a run of bytes is next found, from a place on.
fn find(bytes: &[u8], from: usize, wanted: &[u8]) -> Option<usize> {
    bytes.get(from..)?.windows(wanted.len()).position(|window| window == wanted).map(|at| at + from)
}

/// A JIS X 0208 character, row and cell, as the two bytes Shift-JIS writes
/// it with — which is the arithmetic the code page is built on.
fn shift_jis(row: u8, cell: u8) -> (u8, u8) {
    let lead = ((row + 1) >> 1) + if row <= 0x5E { 0x70 } else { 0xB0 };
    let trail =
        if row % 2 == 1 { cell + if cell >= 0x60 { 0x20 } else { 0x1F } } else { cell + 0x7E };
    (lead, trail)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A message as the server would write it: its opcode, and a body padded
    /// to four bytes.
    fn from_server(major: u8, body: &[u8]) -> Vec<u8> {
        let mut message = Message::new(major).bytes(body).finish();
        message[1] = 0;
        message
    }

    fn attribute_list(names: &[(u16, &str)]) -> Vec<u8> {
        let mut list = Vec::new();
        for (id, name) in names {
            list.extend_from_slice(&id.to_le_bytes());
            list.extend_from_slice(&3u16.to_le_bytes());
            list.extend_from_slice(&(name.len() as u16).to_le_bytes());
            list.extend_from_slice(name.as_bytes());
            while (list.len()) % 4 != 0 {
                list.push(0);
            }
        }
        list
    }

    fn sent(actions: &[Action]) -> Vec<u8> {
        actions
            .iter()
            .filter_map(|action| match action {
                Action::Send(bytes) => Some(bytes[0]),
                _ => None,
            })
            .collect()
    }

    /// A conversation taken to the point where a window has its context:
    /// the server's replies as a server writes them.
    fn ready(style: u32, utf8: bool) -> Xim {
        let mut xim = agreed(style, utf8);
        let create = xim.add_window(0x0040_0001);
        let Action::Send(create) = &create[0] else { panic!("no context asked for") };
        assert_eq!(create[0], opcode::CREATE_IC);
        let made = xim.receive(&from_server(opcode::CREATE_IC_REPLY, &[7, 0, 3, 0]));
        assert!(made.is_empty(), "no focus yet, so nothing more to say");
        xim
    }

    /// A conversation taken to the point where contexts can be made, and
    /// none has been asked for.
    fn agreed(style: u32, utf8: bool) -> Xim {
        let mut xim = Xim::new("ko_KR.UTF-8");
        let connect = xim.connect();
        assert_eq!(connect, vec![1, 0, 2, 0, 0x6C, 0, 1, 0, 0, 0, 0, 0]);
        let open = xim.receive(&from_server(opcode::CONNECT_REPLY, &[1, 0, 0, 0]));
        let Action::Send(open) = &open[0] else { panic!("no open") };
        assert_eq!(open[0], opcode::OPEN);
        assert_eq!(&open[4..16], b"\x0bko_KR.UTF-8", "the locale, as a counted string");

        let im_list = attribute_list(&[(1, "queryInputStyle")]);
        let ic_list = attribute_list(&[
            (10, "inputStyle"),
            (11, "clientWindow"),
            (12, "focusWindow"),
            (13, "preeditAttributes"),
            (14, "spotLocation"),
        ]);
        let mut body = 7u16.to_le_bytes().to_vec();
        body.extend_from_slice(&(im_list.len() as u16).to_le_bytes());
        body.extend_from_slice(&im_list);
        body.extend_from_slice(&(ic_list.len() as u16).to_le_bytes());
        body.extend_from_slice(&[0, 0]);
        body.extend_from_slice(&ic_list);
        let negotiate = xim.receive(&from_server(opcode::OPEN_REPLY, &body));
        assert_eq!(sent(&negotiate), vec![opcode::ENCODING_NEGOTIATION]);

        let index: i16 = if utf8 { 0 } else { 1 };
        let mut reply = 7u16.to_le_bytes().to_vec();
        reply.extend_from_slice(&0u16.to_le_bytes());
        reply.extend_from_slice(&index.to_le_bytes());
        reply.extend_from_slice(&[0, 0]);
        let ask = xim.receive(&from_server(opcode::ENCODING_NEGOTIATION_REPLY, &reply));
        assert_eq!(sent(&ask), vec![opcode::GET_IM_VALUES]);

        let mut styles = 2u16.to_le_bytes().to_vec();
        styles.extend_from_slice(&[0, 0]);
        styles.extend_from_slice(&(PREEDIT_NOTHING | STATUS_NOTHING).to_le_bytes());
        styles.extend_from_slice(&style.to_le_bytes());
        let mut values = 7u16.to_le_bytes().to_vec();
        let attribute = Message::attribute(1, &styles);
        values.extend_from_slice(&(attribute.len() as u16).to_le_bytes());
        values.extend_from_slice(&attribute);
        assert!(xim.receive(&from_server(opcode::GET_IM_VALUES_REPLY, &values)).is_empty());
        assert!(xim.is_ready());
        xim
    }

    #[test]
    fn a_conversation_opens_negotiates_and_makes_a_context() {
        let xim = ready(PREEDIT_CALLBACKS | STATUS_NOTHING, true);
        assert!(xim.utf8);
        assert_eq!(xim.contexts[0].id, 3);
        assert_eq!(
            xim.style,
            PREEDIT_CALLBACKS | STATUS_NOTHING,
            "the style this would rather have"
        );
        assert_eq!(xim.contexts[0].window, 0x0040_0001);
    }

    #[test]
    fn a_key_goes_to_the_server_and_the_next_waits_for_its_answer() {
        let mut xim = ready(PREEDIT_CALLBACKS | STATUS_NOTHING, true);
        let mut event = [0u8; 32];
        event[0] = 2;
        event[1] = 42;
        let (taken, actions) = xim.key(0x0040_0001, &event, 'g' as u32, 0, true);
        assert!(taken);
        assert_eq!(sent(&actions), vec![opcode::SET_IC_FOCUS, opcode::FORWARD_EVENT]);
        let Action::Send(forward) = &actions[1] else { panic!() };
        assert_eq!(u16::from_le_bytes([forward[8], forward[9]]), SYNCHRONOUS);
        assert_eq!(&forward[12..44], &event, "the event itself, as the X server wrote it");

        // The next key waits, and goes once the first is answered.
        let (taken, actions) = xim.key(0x0040_0001, &event, 'k' as u32, 0, true);
        assert!(taken && actions.is_empty());
        let given = xim.receive(&from_server(opcode::FORWARD_EVENT, &{
            let mut body = vec![7, 0, 3, 0, 1, 0, 0, 0];
            body.extend_from_slice(&event);
            body
        }));
        assert_eq!(given[0], Action::Key(0x0040_0001, event), "not wanted: typed as it was");
        assert_eq!(sent(&given), vec![opcode::SYNC_REPLY], "and answered, as it asked");
        let released = xim.receive(&from_server(opcode::SYNC_REPLY, &[7, 0, 3, 0]));
        assert_eq!(sent(&released), vec![opcode::FORWARD_EVENT]);
    }

    #[test]
    fn the_text_being_composed_is_drawn_changed_and_committed() {
        let mut xim = ready(PREEDIT_CALLBACKS | STATUS_NOTHING, true);
        let start = xim.receive(&from_server(opcode::PREEDIT_START, &[7, 0, 3, 0]));
        assert_eq!(sent(&start), vec![opcode::PREEDIT_START_REPLY]);

        let draw = |caret: i32, first: i32, length: i32, text: &str, marks: &[u32]| {
            let mut body = vec![7, 0, 3, 0];
            for value in [caret, first, length, 0] {
                body.extend_from_slice(&value.to_le_bytes());
            }
            body.extend_from_slice(&(text.len() as u16).to_le_bytes());
            body.extend_from_slice(text.as_bytes());
            while (body.len() - 20) % 4 != 0 {
                body.push(0);
            }
            body.extend_from_slice(&((marks.len() * 4) as u16).to_le_bytes());
            body.extend_from_slice(&[0, 0]);
            for mark in marks {
                body.extend_from_slice(&mark.to_le_bytes());
            }
            from_server(opcode::PREEDIT_DRAW, &body)
        };
        let drawn = xim.receive(&draw(1, 0, 0, "ㅎ", &[2]));
        assert_eq!(
            drawn,
            vec![Action::Deliver(
                0x0040_0001,
                Event::Compose {
                    text: "ㅎ".to_owned(),
                    caret: 1,
                    attributes: vec![CompositionAttribute::Input]
                }
            )]
        );
        // The one character replaced by the syllable it has become, and a
        // second added in reverse, which is the clause being chosen.
        xim.receive(&draw(1, 0, 1, "하", &[2]));
        let drawn = xim.receive(&draw(2, 1, 0, "ㄴ", &[1]));
        assert_eq!(
            drawn,
            vec![Action::Deliver(
                0x0040_0001,
                Event::Compose {
                    text: "하ㄴ".to_owned(),
                    caret: 2,
                    attributes: vec![CompositionAttribute::Input, CompositionAttribute::Target]
                }
            )]
        );
        let mut commit = vec![7, 0, 3, 0, 3, 0];
        commit.extend_from_slice(&("한".len() as u16).to_le_bytes());
        commit.extend_from_slice("한".as_bytes());
        let committed = xim.receive(&from_server(opcode::COMMIT, &commit));
        assert_eq!(committed[0], Action::Deliver(0x0040_0001, Event::Commit("한".to_owned())));
        assert_eq!(sent(&committed), vec![opcode::SYNC_REPLY]);
        let done = xim.receive(&from_server(opcode::PREEDIT_DONE, &[7, 0, 3, 0]));
        assert_eq!(done, vec![Action::Deliver(0x0040_0001, Event::ComposeEnd)]);
    }

    #[test]
    fn a_server_that_offers_only_its_own_window_is_taken_as_that() {
        let xim = ready(PREEDIT_POSITION | STATUS_NOTHING, false);
        assert_eq!(xim.style, PREEDIT_NOTHING | STATUS_NOTHING, "over the spot is not taken");
        assert!(!xim.utf8);
    }

    #[test]
    fn the_caret_is_told_to_the_server_when_it_moves() {
        let mut xim = ready(PREEDIT_CALLBACKS | STATUS_NOTHING, true);
        let first = xim.spot(0x0040_0001, 30, 40);
        let Action::Send(message) = &first[0] else { panic!("the spot was not said") };
        assert_eq!(message[0], opcode::SET_IC_VALUES);
        // preeditAttributes holding spotLocation holding the point.
        assert_eq!(&message[12..24], &[13, 0, 8, 0, 14, 0, 4, 0, 30, 0, 40, 0]);
        assert!(xim.spot(0x0040_0001, 30, 40).is_empty(), "not again while it stands");
        assert_eq!(xim.spot(0x0040_0001, 31, 40).len(), 1);
    }

    #[test]
    fn a_server_that_goes_quiet_gives_the_keys_back() {
        let mut xim = ready(PREEDIT_CALLBACKS | STATUS_NOTHING, true);
        let mut event = [0u8; 32];
        event[0] = 2;
        let _ = xim.key(0x0040_0001, &event, 'a' as u32, 0, true);
        let _ = xim.key(0x0040_0001, &event, 'b' as u32, 0, true);
        assert!(xim.is_waiting());
        let back = xim.abandon();
        assert_eq!(back, vec![Action::Key(0x0040_0001, event)], "the key that waited is typed");
        assert!(xim.has_failed());
        assert!(!xim.key(0x0040_0001, &event, 'c' as u32, 0, true).0, "and the next is typed");
    }

    /// Keys pressed while a window's context is still being asked for wait
    /// for it, and then go to the server in the order they were pressed —
    /// the first at once, the next once the first is answered.
    #[test]
    fn keys_pressed_before_the_context_is_made_go_through_it_once_it_is() {
        let mut xim = agreed(PREEDIT_CALLBACKS | STATUS_NOTHING, true);
        let create = xim.add_window(0x0040_0001);
        assert_eq!(sent(&create), vec![opcode::CREATE_IC]);
        let mut shift = [0u8; 32];
        shift[0] = 2;
        shift[1] = 50;
        let mut space = shift;
        space[1] = 65;
        let (taken, actions) = xim.key(0x0040_0001, &shift, 0xFFE1, 0, true);
        assert!(taken && actions.is_empty(), "kept, not typed");
        let (taken, actions) = xim.key(0x0040_0001, &space, 0x20, 1, true);
        assert!(taken && actions.is_empty(), "kept, not typed");
        assert!(xim.is_waiting(), "and waited for");
        assert!(!xim.key(0x0040_0002, &space, 0x20, 0, true).0, "a window nobody asked for types");

        let made = xim.receive(&from_server(opcode::CREATE_IC_REPLY, &[7, 0, 3, 0]));
        assert_eq!(sent(&made), vec![opcode::SET_IC_FOCUS, opcode::FORWARD_EVENT]);
        let Action::Send(forward) = &made[1] else { panic!() };
        assert_eq!(&forward[12..44], &shift, "the first key pressed goes first");
        assert!(xim.is_waiting(), "the second waits for the first to be answered");
        let answered = xim.receive(&from_server(opcode::SYNC_REPLY, &[7, 0, 3, 0]));
        assert_eq!(sent(&answered), vec![opcode::FORWARD_EVENT]);
        let Action::Send(forward) = &answered[0] else { panic!() };
        assert_eq!(&forward[12..44], &space);
    }

    /// Keys pressed before the server has so much as answered wait as
    /// well — and are typed as they were if it never does.
    #[test]
    fn keys_pressed_before_the_server_answers_are_typed_if_it_never_does() {
        let mut xim = Xim::new("ko_KR.UTF-8");
        let _ = xim.connect();
        assert!(xim.add_window(0x0040_0001).is_empty(), "nothing to ask until it answers");
        let mut event = [0u8; 32];
        event[0] = 2;
        event[1] = 42;
        let (taken, actions) = xim.key(0x0040_0001, &event, 'g' as u32, 0, true);
        assert!(taken && actions.is_empty());
        assert!(xim.is_waiting());
        assert_eq!(xim.abandon(), vec![Action::Key(0x0040_0001, event)], "typed after all");
        assert!(!xim.is_waiting());
    }

    #[test]
    fn compound_text_switches_between_the_sets_it_names() {
        assert_eq!(compound_text(b"plain"), "plain");
        // Latin-1's right half without a word said: the start state.
        assert_eq!(compound_text(b"caf\xE9"), "café");
        // KS C 5601 to the right: 한글 as EUC-KR writes it.
        assert_eq!(compound_text(b"\x1b$)C\xC7\xD1\xB1\xDB"), "한글");
        // GB 2312 to the right: 中文.
        assert_eq!(compound_text(b"\x1b$)A\xD6\xD0\xCE\xC4"), "中文");
        // JIS X 0208 to the left: 日本 as row and cell.
        assert_eq!(compound_text(b"\x1b$(BF|K\\\x1b(B!"), "日本!");
        // UTF-8 for what the national sets do not have, and back.
        assert_eq!(compound_text("a\x1b%G😀\x1b%@b".as_bytes()), "a😀b");
        // Cyrillic from ISO 8859-5.
        assert_eq!(compound_text(b"\x1b-L\xB4\xD0"), "Да");
    }
}
