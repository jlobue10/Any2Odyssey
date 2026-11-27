//! Global shortcuts of the Linux viewer, through the desktop portal.
//!
//! The Windows edition registers its shortcuts with the system itself
//! (`hotkeys`). A Wayland desktop gives a program no keys while another
//! window has the keyboard; it has to be asked to deliver them, and the way
//! to ask that works on more than one desktop is the portal's
//! `GlobalShortcuts` (KDE Plasma 6 has it, backed by its own global
//! shortcuts: they appear, and are changed, in System Settings →
//! Shortcuts). The desktop decides which keys they are: what is named here
//! is only a preference, and the desktop may ask the user about it.
//!
//! The keys preferred are those of Samsung's Odyssey 3D Hub, which
//! odysseyvk-linux (the-cave-dweller, MIT) uses as well: Ctrl+Shift+1 for
//! the 3D picture, 3 and 4 for depth, 5 and 6 for pop-out.
//!
//! Nothing here knows the viewer: presses come out as [`Action`]s for it
//! to take at its next frame.
//!
//! The desktop keeps a session of shortcuts, and their keys, until the
//! session is closed or the program leaves the bus. So the session is
//! closed whenever the shortcuts end here: the box cleared, the desktop's
//! dialog declined, a refusal. And there is one session at a time.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, PoisonError};
use std::task::Poll;

use ashpd::desktop::global_shortcuts::{BindShortcutsOptions, GlobalShortcuts, NewShortcut};
use ashpd::desktop::{CreateSessionOptions, Session};
use ashpd::zbus::export::futures_core::Stream;
use tokio::sync::oneshot::{self, error::TryRecvError};
use tokio::task::JoinHandle;

/// What a shortcut asks of the viewer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Start the output, or stop it.
    Output,
    DepthDown,
    DepthUp,
    PopoutDown,
    PopoutUp,
}

/// A shortcut as the desktop is told of it: the name it is kept under, what
/// the desktop's own pages say it does, and the keys asked for (in the
/// notation of the XDG shortcuts specification).
struct Wanted {
    action: Action,
    id: &'static str,
    does: &'static str,
    keys: &'static str,
}

const WANTED: [Wanted; 5] = [
    Wanted {
        action: Action::Output,
        id: "output",
        does: "Start or stop the output",
        keys: "CTRL+SHIFT+1",
    },
    Wanted {
        action: Action::DepthDown,
        id: "depth-down",
        does: "Less depth",
        keys: "CTRL+SHIFT+3",
    },
    Wanted {
        action: Action::DepthUp,
        id: "depth-up",
        does: "More depth",
        keys: "CTRL+SHIFT+4",
    },
    Wanted {
        action: Action::PopoutDown,
        id: "popout-down",
        does: "Picture further behind the screen",
        keys: "CTRL+SHIFT+5",
    },
    Wanted {
        action: Action::PopoutUp,
        id: "popout-up",
        does: "Picture further in front of the screen",
        keys: "CTRL+SHIFT+6",
    },
];

/// The action the desktop means by the name `id`.
fn action_of(id: &str) -> Option<Action> {
    WANTED.iter().find(|w| w.id == id).map(|w| w.action)
}

/// What the shortcuts have to say to the viewer.
#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    Pressed(Action),
    /// The desktop has bound them: a line for the log that says to which
    /// keys.
    Bound(String),
    /// There are none, and why.
    Failed(String),
}

/// The line for the log once the desktop has answered: each shortcut with
/// the keys the desktop gave it, in the desktop's words.
fn bound_line(bound: &[(String, String)]) -> String {
    let given: Vec<String> = WANTED
        .iter()
        .filter_map(|w| {
            let (_, keys) = bound.iter().find(|(id, _)| id == w.id)?;
            Some(if keys.is_empty() {
                format!("{} (no keys yet)", w.does)
            } else {
                format!("{}: {keys}", w.does)
            })
        })
        .collect();
    if given.is_empty() {
        "Global shortcuts: the desktop bound none. They can be set in its shortcut settings."
            .to_owned()
    } else {
        format!("Global shortcuts: {}.", given.join("; "))
    }
}

/// Why the desktop gave no shortcuts, for the status line.
fn failure_line(error: &ashpd::Error) -> String {
    match error {
        ashpd::Error::PortalNotFound(_) => {
            "Global shortcuts: this desktop's portal has none (KDE Plasma 6 has them).".to_owned()
        }
        ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled) => {
            "Global shortcuts: declined in the desktop's dialog.".to_owned()
        }
        // The desktop does not know which program asks: see
        // `name_this_program`.
        ashpd::Error::Portal(ashpd::PortalError::NotAllowed(why)) => format!(
            "Global shortcuts: the desktop gives them only to a program it knows by name, and it does not know this one ({why}). It knows a program by its desktop file: any2odyssey.desktop, which install.sh and the package install."
        ),
        other => format!("Global shortcuts: the desktop refused them ({other})."),
    }
}

/// What the status line says when the desktop itself ended the shortcuts.
const ENDED_BY_DESKTOP: &str =
    "Global shortcuts: the desktop ended them. Tick the box to ask again.";

/// The shortcuts while they are wanted. Dropping it ends them: the desktop
/// is told, and has its keys back.
pub struct Shortcuts {
    events: Receiver<Event>,
    /// Nothing is ever sent: that this end is dropped with the rest is what
    /// tells the task to close the desktop's session. (A task that is only
    /// aborted leaves the session open, and the desktop keeps the keys for
    /// as long as this program runs.)
    _wanted: Option<oneshot::Sender<()>>,
}

/// The task of the shortcuts before these, which may still be closing its
/// session.
static BEFORE: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

impl Shortcuts {
    /// Ask the desktop for the shortcuts. `wake` is called (on another
    /// thread) whenever there is an [`Event`] to take.
    pub fn start(wake: impl Fn() + Send + Sync + 'static) -> Shortcuts {
        let (tx, events) = mpsc::channel();
        let (wanted, unwanted) = oneshot::channel();
        match crate::capture::portal_runtime() {
            Ok(runtime) => {
                let mut before = BEFORE.lock().unwrap_or_else(PoisonError::into_inner);
                let earlier = before.take();
                *before = Some(runtime.spawn(async move {
                    // One session at a time: the desktop tells every session
                    // of a press, and with two a press would be taken twice.
                    if let Some(earlier) = earlier {
                        let _ = earlier.await;
                    }
                    let said = match serve(&tx, &wake, unwanted).await {
                        Ok(End::Unwanted) => return,
                        Ok(End::ByDesktop) => ENDED_BY_DESKTOP.to_owned(),
                        Err(error) => failure_line(&error),
                    };
                    let _ = tx.send(Event::Failed(said));
                    wake();
                }));
            }
            Err(why) => {
                let _ = tx.send(Event::Failed(format!("Global shortcuts: {why}.")));
            }
        }
        Shortcuts {
            events,
            _wanted: Some(wanted),
        }
    }

    /// The next event, if there is one.
    pub fn take(&self) -> Option<Event> {
        // (an ended source has nothing more to say)
        self.events.try_recv().ok()
    }

    /// Shortcuts that deliver what a test sends.
    #[cfg(test)]
    pub fn for_test() -> (Shortcuts, Sender<Event>) {
        let (tx, events) = mpsc::channel();
        (
            Shortcuts {
                events,
                _wanted: None,
            },
            tx,
        )
    }
}

/// How the shortcuts ended, where it was not a refusal.
#[derive(Debug, PartialEq, Eq)]
enum End {
    /// The viewer no longer wants them.
    Unwanted,
    /// The desktop closed the session, stopped telling of presses, or its
    /// portal left the bus.
    ByDesktop,
}

/// What `future` gives, or nothing once the other end of `unwanted` is gone.
/// After nothing, `unwanted` is not asked again.
async fn unless<T>(
    unwanted: &mut oneshot::Receiver<()>,
    future: impl Future<Output = T>,
) -> Option<T> {
    let mut future = std::pin::pin!(future);
    std::future::poll_fn(|context| {
        if Pin::new(&mut *unwanted).poll(context).is_ready() {
            return Poll::Ready(None);
        }
        future.as_mut().poll(context).map(Some)
    })
    .await
}

/// The name the desktop knows this program by: its desktop file's
/// (`any2odyssey.desktop`), which is also the windows' application name.
const APP_ID: &str = "any2odyssey";

/// The portal's name on the bus.
const PORTAL: &str = "org.freedesktop.portal.Desktop";

/// Tell the portal which program is on `connection`.
///
/// The desktop gives global shortcuts only to a program it can name: it
/// keeps them, and shows them in its settings, under that name. A program
/// started from the desktop's menu is named by the desktop itself. One
/// started from a terminal, a script or another program is not, and its
/// request is refused ("An app id is required") unless it says who it is
/// through the portal's registry of programs outside a sandbox. The registry
/// answers only the first call made on a connection, and only for a name
/// that has a desktop file. A failure here is not the end: an older portal
/// has no registry, and the desktop may know the name anyway.
async fn name_this_program(connection: &ashpd::zbus::Connection) -> ashpd::zbus::Result<()> {
    let registry = ashpd::zbus::Proxy::new(
        connection,
        PORTAL,
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.host.portal.Registry",
    )
    .await?;
    let options: HashMap<&str, ashpd::zvariant::Value<'_>> = HashMap::new();
    registry
        .call_method("Register", &(APP_ID, options))
        .await
        .map(drop)
}

/// Have a session of shortcuts with the desktop for as long as they are
/// wanted and the desktop keeps it, and close it whatever ends it.
async fn serve(
    tx: &Sender<Event>,
    wake: &(impl Fn() + Send + Sync),
    mut unwanted: oneshot::Receiver<()>,
) -> Result<End, ashpd::Error> {
    // A connection of their own, on which this program first says who it
    // is (see `name_this_program`): only the first call on a connection can.
    let connection = ashpd::zbus::Connection::session().await?;
    let _ = name_this_program(&connection).await;
    let portal = GlobalShortcuts::with_connection(connection.clone()).await?;
    // (Not while the session is being made: one that the desktop has made
    // and this program has not heard of could not be closed.)
    if !matches!(unwanted.try_recv(), Err(TryRecvError::Empty)) {
        return Ok(End::Unwanted);
    }
    let session = portal
        .create_session(CreateSessionOptions::default())
        .await?;
    let end = hold(&connection, &portal, &session, tx, wake, &mut unwanted).await;
    let _ = session.close().await;
    end
}

/// Bind the shortcuts in `session` and hand over every press.
async fn hold(
    connection: &ashpd::zbus::Connection,
    portal: &GlobalShortcuts,
    session: &Session<GlobalShortcuts>,
    tx: &Sender<Event>,
    wake: &(impl Fn() + Send + Sync),
    unwanted: &mut oneshot::Receiver<()>,
) -> Result<End, ashpd::Error> {
    let wanted: Vec<NewShortcut> = WANTED
        .iter()
        .map(|w| NewShortcut::new(w.id, w.does).preferred_trigger(w.keys))
        .collect();
    // Listening before binding: no press between the two is lost.
    let pressed = portal.receive_activated().await?;
    let closed = session.receive_closed().await?;
    // A portal that leaves the bus (restarted by an update of its package,
    // or fallen over) says nothing to its sessions, and the one that comes
    // after it knows none of them.
    let bus = ashpd::zbus::fdo::DBusProxy::new(connection).await?;
    let left = bus
        .receive_name_owner_changed_with_args(&[(0, PORTAL)])
        .await?;
    // The desktop may ask the user here, and the box may be cleared while
    // it does: closing the session takes its dialog away.
    let bind = async {
        portal
            .bind_shortcuts(session, &wanted, None, BindShortcutsOptions::default())
            .await?
            .response()
    };
    let Some(bound) = unless(unwanted, bind).await else {
        return Ok(End::Unwanted);
    };
    let bound: Vec<(String, String)> = bound?
        .shortcuts()
        .iter()
        .map(|s| (s.id().to_owned(), s.trigger_description().to_owned()))
        .collect();
    let _ = tx.send(Event::Bound(bound_line(&bound)));
    wake();

    enum Next<P> {
        Unwanted,
        Ended,
        Press(P),
    }
    let mut pressed = std::pin::pin!(pressed);
    let mut closed = std::pin::pin!(closed);
    let mut left = std::pin::pin!(left);
    loop {
        let next = std::future::poll_fn(|context| {
            if Pin::new(&mut *unwanted).poll(context).is_ready() {
                return Poll::Ready(Next::Unwanted);
            }
            if closed.as_mut().poll_next(context).is_ready()
                || left.as_mut().poll_next(context).is_ready()
            {
                return Poll::Ready(Next::Ended);
            }
            pressed
                .as_mut()
                .poll_next(context)
                .map(|press| press.map_or(Next::Ended, Next::Press))
        })
        .await;
        match next {
            Next::Unwanted => return Ok(End::Unwanted),
            Next::Ended => return Ok(End::ByDesktop),
            Next::Press(press) => {
                // (The session before this one was closed before this one
                // was asked for, so a press is this one's.)
                if let Some(action) = action_of(press.shortcut_id()) {
                    if tx.send(Event::Pressed(action)).is_err() {
                        return Ok(End::Unwanted);
                    }
                    wake();
                }
            }
        }
    }
}

/// The icon beside a word on the screen: the program's own, by the name its
/// desktop file gives it.
const ICON: &str = APP_ID;

/// How long a notification with the word stays where the desktop has no
/// display of its own for such words, in milliseconds.
const WORD_STAYS_MS: i32 = 1500;

/// Say on the screen what a shortcut did: the page that would show it is
/// behind another window, or not shown at all, when a global shortcut is
/// what was pressed.
///
/// On KDE Plasma the word goes to the desktop's own display for such things
/// (the one that shows the volume when its keys are pressed): it is over
/// whatever is on the screen, goes by itself and leaves nothing behind. On
/// another desktop it is a notification that goes after a moment, is not
/// kept in the list, and takes the place of the one before it. A desktop
/// with neither shows nothing, and nothing is said about that.
///
/// Returns at once: the desktop is told on the portal's threads, and of
/// words that pile up while it answers only the newest is shown.
pub fn say(word: String) {
    use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
    static WORDS: Mutex<Option<UnboundedSender<String>>> = Mutex::new(None);
    let mut words = WORDS.lock().unwrap_or_else(PoisonError::into_inner);
    let word = match words.as_ref() {
        Some(tx) => match tx.send(word) {
            Ok(()) => return,
            // (the task that showed them has ended: one is started again)
            Err(unsent) => unsent.0,
        },
        None => word,
    };
    let Ok(runtime) = crate::capture::portal_runtime() else {
        return;
    };
    let (tx, mut rx) = unbounded_channel();
    let _ = tx.send(word);
    *words = Some(tx);
    runtime.spawn(async move {
        let Ok(connection) = ashpd::zbus::Connection::session().await else {
            return;
        };
        let mut notification = 0;
        while let Some(mut word) = rx.recv().await {
            while let Ok(newer) = rx.try_recv() {
                word = newer;
            }
            if on_plasma(&connection, &word).await.is_err() {
                if let Ok(shown) = notify(&connection, &word, notification).await {
                    notification = shown;
                }
            }
        }
    });
}

/// Show `word` on KDE Plasma's on-screen display.
async fn on_plasma(connection: &ashpd::zbus::Connection, word: &str) -> ashpd::zbus::Result<()> {
    connection
        .call_method(
            Some("org.kde.plasmashell"),
            "/org/kde/osdService",
            Some("org.kde.osdService"),
            "showText",
            &(ICON, word),
        )
        .await
        .map(drop)
}

/// Show `word` as a notification in the place of the one numbered
/// `before` (0: none), and give its number.
async fn notify(
    connection: &ashpd::zbus::Connection,
    word: &str,
    before: u32,
) -> ashpd::zbus::Result<u32> {
    let mut hints: HashMap<&str, ashpd::zvariant::Value<'_>> = HashMap::new();
    // Not kept in the desktop's list of notifications.
    hints.insert("transient", true.into());
    hints.insert("desktop-entry", APP_ID.into());
    let actions: [&str; 0] = [];
    connection
        .call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &(
                "Any2Odyssey",
                before,
                ICON,
                word,
                "",
                &actions[..],
                hints,
                WORD_STAYS_MS,
            ),
        )
        .await?
        .body()
        .deserialize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shortcut_has_a_name_of_its_own_and_its_action_back() {
        for (index, wanted) in WANTED.iter().enumerate() {
            assert_eq!(action_of(wanted.id), Some(wanted.action));
            assert!(WANTED[..index]
                .iter()
                .all(|w| w.id != wanted.id && w.action != wanted.action && w.keys != wanted.keys));
            // The notation of the shortcuts specification: modifiers in
            // capitals, joined to the key by plus signs.
            assert!(wanted.keys.starts_with("CTRL+SHIFT+"), "{}", wanted.keys);
        }
        assert_eq!(action_of("something-else"), None);
        assert_eq!(action_of(""), None);
    }

    #[test]
    fn the_line_for_the_log_says_what_the_desktop_gave() {
        let bound = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
            pairs
                .iter()
                .map(|(id, keys)| ((*id).to_owned(), (*keys).to_owned()))
                .collect()
        };
        assert_eq!(
            bound_line(&bound(&[
                ("depth-up", "Ctrl+Shift+4"),
                ("output", "Meta+F3")
            ])),
            "Global shortcuts: Start or stop the output: Meta+F3; More depth: Ctrl+Shift+4."
        );
        // One the user has left without keys, and one this program never
        // asked for.
        assert_eq!(
            bound_line(&bound(&[("output", ""), ("stranger", "Ctrl+Q")])),
            "Global shortcuts: Start or stop the output (no keys yet)."
        );
        assert!(bound_line(&[]).contains("bound none"));
    }

    #[test]
    fn events_come_out_in_order_and_an_ended_source_is_just_empty() {
        let (shortcuts, tx) = Shortcuts::for_test();
        assert_eq!(shortcuts.take(), None);
        tx.send(Event::Pressed(Action::DepthUp)).unwrap();
        tx.send(Event::Pressed(Action::Output)).unwrap();
        drop(tx);
        assert_eq!(shortcuts.take(), Some(Event::Pressed(Action::DepthUp)));
        assert_eq!(shortcuts.take(), Some(Event::Pressed(Action::Output)));
        assert_eq!(shortcuts.take(), None);
    }

    #[test]
    fn what_is_waited_for_is_given_up_once_the_shortcuts_are_not_wanted() {
        let runtime = crate::capture::portal_runtime().unwrap();
        // Still wanted: the answer comes through.
        let (wanted, mut unwanted) = oneshot::channel::<()>();
        assert_eq!(
            runtime.block_on(unless(&mut unwanted, async { 7 })),
            Some(7)
        );
        // No longer wanted while waiting for an answer that never comes (the
        // desktop's dialog left open): the wait ends.
        let waiting =
            runtime.spawn(async move { unless(&mut unwanted, std::future::pending::<()>()).await });
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(!waiting.is_finished());
        drop(wanted);
        assert_eq!(runtime.block_on(waiting).unwrap(), None);
        // Not wanted from the start: nothing is waited for at all.
        let (wanted, mut unwanted) = oneshot::channel::<()>();
        drop(wanted);
        assert_eq!(
            runtime.block_on(unless(&mut unwanted, std::future::pending::<()>())),
            None
        );
    }

    #[test]
    fn dropping_the_shortcuts_is_what_tells_their_task() {
        let (wanted, mut unwanted) = oneshot::channel::<()>();
        let (_tx, events) = mpsc::channel();
        let shortcuts = Shortcuts {
            events,
            _wanted: Some(wanted),
        };
        assert!(matches!(unwanted.try_recv(), Err(TryRecvError::Empty)));
        drop(shortcuts);
        assert!(matches!(unwanted.try_recv(), Err(TryRecvError::Closed)));
    }

    #[test]
    fn a_desktop_without_the_portal_or_a_declined_dialog_is_said_plainly() {
        let declined = ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled);
        assert!(failure_line(&declined).contains("declined"));
        let other = ashpd::Error::Response(ashpd::desktop::ResponseError::Other);
        assert!(failure_line(&other).starts_with("Global shortcuts: the desktop refused them"));
        // What KDE Plasma 6.8 answers a program it cannot name.
        let unnamed = ashpd::Error::Portal(ashpd::PortalError::NotAllowed(
            "An app id is required".to_owned(),
        ));
        let line = failure_line(&unnamed);
        assert!(line.contains("An app id is required") && line.contains("any2odyssey.desktop"));
        // The name given to the desktop is the desktop file's.
        assert_eq!(APP_ID, "any2odyssey");
    }
}
