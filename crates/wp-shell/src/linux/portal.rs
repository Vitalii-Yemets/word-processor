//! The desktop's portal: what a program asks the desktop to do for it, over
//! D-Bus, where it may not do the thing itself.
//!
//! # Why a screenshot has to be asked for
//!
//! Because on Wayland a program sees its own windows and nothing else. That
//! is the protocol's rule and the point of it — a program that could read
//! the screen could read the password somebody else is typing — so Word's
//! Screenshot button, which on X or Windows photographs the screen itself,
//! has to ask the desktop instead. The desktop's answer is the portal:
//! `org.freedesktop.portal.Screenshot`, which the desktop carries out in
//! its own way — photographing the whole screen, or letting the person drag
//! out the rectangle they want, which is Word's Screen Clipping — and hands
//! back as a file.
//!
//! # How the answer comes back
//!
//! Not as the call's reply. The call is answered at once with the path of
//! a request object, and the picture follows, whenever the person has
//! finished choosing, as that object's `Response` signal: a number saying
//! whether it was done, cancelled or failed, and the file's address. The
//! path is one the program can work out in advance from its name on the bus
//! and a token of its own, and the bus is told to pass the signal on before
//! the call goes, so that a desktop that answers at once is not answering
//! into nothing.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use super::dbus::{self, Connection, Kind, Message, Value};

const DESKTOP: &str = "org.freedesktop.portal.Desktop";
const DESKTOP_PATH: &str = "/org/freedesktop/portal/desktop";
const SCREENSHOT: &str = "org.freedesktop.portal.Screenshot";

/// How a screenshot is to be taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Taking {
    /// The whole screen, with no questions.
    Screen,
    /// A rectangle the person drags out.
    Clipping,
}

/// How long a portal that is running is given to answer a question about
/// itself, which it answers at once.
const ANSWERING: Duration = Duration::from_secs(3);

/// How long the bus is given to start a portal that is not running. The
/// first question starts it, and before it can answer it starts in turn the
/// desktop's own half of it and the programs that go with that — five
/// programs on sway, a tenth of a second on a quiet machine and more than
/// three seconds on a busy one. This is the time any D-Bus call is given by
/// default; where there is no portal to start, the bus says so at once.
const STARTING: Duration = Duration::from_secs(25);

/// The session bus, where the portal is.
fn session() -> Option<Connection> {
    Connection::open(&dbus::session_address()?).ok()
}

/// What is known of the portal: the session bus it was asked on, and its
/// answer there — nothing yet while it is still being asked. One bus in a
/// program's life, as a rule; a test starts one of its own each time.
#[derive(Debug)]
struct Known {
    bus: String,
    answer: Option<bool>,
}

/// What the program knows of the portal, for every window and thread.
static KNOWN: Mutex<Option<Known>> = Mutex::new(None);

/// Starts finding out, in the background, whether the desktop's portal
/// will take screenshots — which on a desktop where nobody has used the
/// portal yet is what starts it. Called as the window comes up, so that
/// by the time somebody opens the Screenshot list the portal is running
/// and its answer is known.
pub(crate) fn warm_up() {
    let _ = offers_screenshot();
}

/// Whether the desktop's portal will take a screenshot at all, as far as
/// is known — which asks nothing of the bus once the answer is in.
///
/// The question used to be asked every time the Screenshot list opened,
/// and so, the first time, of a portal nobody had started: the portal, the
/// document portal, the permission store and both of the desktop's halves
/// come up before it answers, a tenth of a second on a quiet machine and
/// nearly four on a busy one. Given three, it was taken for absent and the
/// list opened without Screen Clipping; given longer, the list would have
/// kept the window frozen for as long as the portal took. It is now asked
/// in the background, by [`warm_up`], once — or once more the next time
/// the list opens, if nothing answered it in time.
///
/// While it is still being asked the answer is yes. Word offers Screen
/// Clipping whatever the desktop, and a portal still starting is one that
/// is there: where there is none the bus says so at once and the answer is
/// in long before anybody opens the list. If it never comes up, the
/// clipping itself fails, and says that no picture was taken.
pub(crate) fn offers_screenshot() -> bool {
    let Some(bus) = dbus::session_address() else { return false };
    answer_for(&KNOWN, &bus, |bus| {
        let bus = bus.to_owned();
        std::thread::spawn(move || match ask_whether_it_offers_screenshots(&bus) {
            Some(offered) => answered(&KNOWN, &bus, offered),
            None => forget(&KNOWN, &bus),
        });
    })
}

/// The answer known for a bus, or yes while it is being asked; where
/// nothing is known, `ask` is told to find out, and the answer is yes
/// until it has. `ask` is the only way anything is asked of the bus.
fn answer_for(known: &Mutex<Option<Known>>, bus: &str, ask: impl FnOnce(&str)) -> bool {
    let mut known = known.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(found) = known.as_ref().filter(|found| found.bus == bus) {
        return found.answer.unwrap_or(true);
    }
    *known = Some(Known { bus: bus.to_owned(), answer: None });
    drop(known);
    ask(bus);
    true
}

/// Keeps the portal's answer on a bus, unless another bus has been asked
/// about since.
fn answered(known: &Mutex<Option<Known>>, bus: &str, answer: bool) {
    let mut known = known.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(found) = known.as_mut().filter(|found| found.bus == bus) {
        found.answer = Some(answer);
    }
}

/// Forgets that a bus was being asked about, so that the next time the
/// list opens it is asked again: the question went unanswered, which is
/// not the same as no.
fn forget(known: &Mutex<Option<Known>>, bus: &str) {
    let mut known = known.lock().unwrap_or_else(PoisonError::into_inner);
    if known.as_ref().is_some_and(|found| found.bus == bus && found.answer.is_none()) {
        *known = None;
    }
}

/// Asks the portal on a bus whether it takes screenshots, giving it time
/// to start if it is not running. Nothing if nothing answered in time: a
/// portal that on a busy machine took longer than that to start has gone
/// on to take a screenshot a moment later, and is not to be taken for
/// one that will not.
fn ask_whether_it_offers_screenshots(bus: &str) -> Option<bool> {
    let mut bus = match Connection::open(bus) {
        Ok(bus) => bus,
        Err(failure) => return (!failure.is_no_answer()).then_some(false),
    };
    let wait = if is_running(&mut bus) { ANSWERING } else { STARTING };
    let asked = Message::call(
        DESKTOP,
        DESKTOP_PATH,
        "org.freedesktop.DBus.Properties",
        "Get",
        vec![Value::str(SCREENSHOT), Value::str("version")],
    );
    match bus.call(&asked, wait) {
        Ok(_) => Some(true),
        Err(failure) => (!failure.is_no_answer()).then_some(false),
    }
}

/// Whether the portal is on the bus already — which the bus answers itself,
/// without starting it.
fn is_running(bus: &mut Connection) -> bool {
    let asked = Message::call(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "NameHasOwner",
        vec![Value::str(DESKTOP)],
    );
    bus.call(&asked, ANSWERING)
        .is_ok_and(|reply| matches!(reply.body.first(), Some(Value::Bool(true))))
}

/// A screenshot, as the portal hands it over: the bytes of the file it
/// wrote, which is a PNG. Nothing if there is no portal, or the person
/// cancelled, or the desktop could not do it.
pub(crate) fn screenshot(taking: Taking) -> Option<Vec<u8>> {
    static ASKED: AtomicU32 = AtomicU32::new(0);
    let mut bus = session()?;
    let token = format!("wp{}_{}", std::process::id(), ASKED.fetch_add(1, Ordering::Relaxed));
    let expected = request_path(&bus.name, &token);
    listen_for_response(&mut bus, &expected)?;

    let interactive = taking == Taking::Clipping;
    let options = vec![
        option("handle_token", Value::str(&token)),
        option("interactive", Value::Bool(interactive)),
        option("modal", Value::Bool(true)),
    ];
    let call = Message::call(
        DESKTOP,
        DESKTOP_PATH,
        SCREENSHOT,
        "Screenshot",
        // No parent window: naming one on Wayland takes a protocol this
        // program does not speak, and the portal takes none as a window
        // of no one's.
        vec![Value::str(""), Value::Array("{sv}".to_owned(), options)],
    );
    // A portal still starting — the one [`warm_up`] woke, on a busy
    // machine — is waited for as long as a start takes; one running
    // answers the call at once, with the request, whatever the person then
    // takes to finish it.
    let wait = if is_running(&mut bus) { Duration::from_secs(10) } else { STARTING };
    let reply = bus.call(&call, wait).ok()?;
    let handle = reply.body.first().and_then(Value::as_str)?.to_owned();
    // An old portal names the request itself; it is listened for too.
    if handle != expected {
        listen_for_response(&mut bus, &handle)?;
    }

    // A person dragging out a rectangle takes their time; the whole screen
    // does not.
    let wait = match taking {
        Taking::Screen => Duration::from_secs(30),
        Taking::Clipping => Duration::from_secs(300),
    };
    let response = bus.wait_for(wait, |message| {
        message.kind == Kind::Signal
            && message.member.as_deref() == Some("Response")
            && message.path.as_deref() == Some(handle.as_str())
    })?;
    // Nought is done; one is cancelled; two is anything else.
    if response.body.first().and_then(Value::as_i64) != Some(0) {
        return None;
    }
    let address = response.body.get(1).and_then(|results| result(results, "uri"))?;
    let path = file_of(&address)?;
    std::fs::read(path).ok()
}

/// Where the portal will put the request for a call made with a token:
/// under the caller's name on the bus, its dots made underscores.
fn request_path(name: &str, token: &str) -> String {
    let sender = name.trim_start_matches(':').replace('.', "_");
    format!("{DESKTOP_PATH}/request/{sender}/{token}")
}

/// Asks the bus to pass on the answer to a request.
fn listen_for_response(bus: &mut Connection, path: &str) -> Option<()> {
    let rule = format!(
        "type='signal',interface='org.freedesktop.portal.Request',member='Response',path='{path}'"
    );
    let asked = Message::call(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "AddMatch",
        vec![Value::str(&rule)],
    );
    bus.call(&asked, Duration::from_secs(3)).ok().map(|_| ())
}

/// One entry of a dictionary of options.
fn option(key: &str, value: Value) -> Value {
    Value::DictEntry(Box::new(Value::str(key)), Box::new(Value::Variant(Box::new(value))))
}

/// A string out of a dictionary of results.
fn result(results: &Value, key: &str) -> Option<String> {
    results.items().iter().find_map(|entry| match entry {
        Value::DictEntry(found, value) if found.as_str() == Some(key) => {
            value.inner().as_str().map(str::to_owned)
        }
        _ => None,
    })
}

/// The file a `file:` address names on this machine.
fn file_of(address: &str) -> Option<PathBuf> {
    super::files::paths_of_uri_list(address).into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Once the portal has answered, the Screenshot list is told without a
    /// word to the bus; while it is being asked, the answer is yes and it
    /// is not asked twice; on another bus it is asked afresh.
    #[test]
    fn the_portal_is_asked_once_and_its_answer_kept() {
        let known = Mutex::new(None);
        let bus = "unix:path=/run/user/1000/bus";
        let asked = std::cell::Cell::new(0);
        let ask = |_: &str| asked.set(asked.get() + 1);

        assert!(answer_for(&known, bus, ask), "yes, while nothing is known");
        assert_eq!(asked.get(), 1, "and it is asked");
        assert!(answer_for(&known, bus, ask), "yes, while it is being asked");
        assert_eq!(asked.get(), 1, "and not asked again");

        answered(&known, bus, false);
        let never = |_: &str| panic!("the bus was asked with the answer known");
        assert!(!answer_for(&known, bus, never), "the answer, and no question");
        answered(&known, bus, true);
        assert!(answer_for(&known, bus, never));

        let other = "unix:path=/tmp/dbus-another";
        assert!(answer_for(&known, other, ask));
        assert_eq!(asked.get(), 2, "another bus is another portal");
        answered(&known, bus, false);
        assert!(answer_for(&known, other, never), "an answer from the old bus is not taken");

        // A question nothing answered in time is forgotten, and asked again
        // the next time; an answer is not.
        forget(&known, other);
        assert!(answer_for(&known, other, ask));
        assert_eq!(asked.get(), 3, "asked again");
        answered(&known, other, false);
        forget(&known, other);
        assert!(!answer_for(&known, other, never), "the answer stands");
    }

    #[test]
    fn the_request_is_found_where_the_portal_will_put_it() {
        assert_eq!(
            request_path(":1.42", "wp7_0"),
            "/org/freedesktop/portal/desktop/request/1_42/wp7_0"
        );
    }

    #[test]
    fn the_file_is_read_out_of_the_results() {
        let results = Value::Array(
            "{sv}".to_owned(),
            vec![option("uri", Value::str("file:///home/ann/Pictures/Screenshot%20one.png"))],
        );
        let address = result(&results, "uri").expect("the address");
        assert_eq!(file_of(&address), Some(PathBuf::from("/home/ann/Pictures/Screenshot one.png")));
        assert_eq!(result(&results, "other"), None);
    }
}
