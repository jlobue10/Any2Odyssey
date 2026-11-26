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

use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

use ashpd::desktop::global_shortcuts::{BindShortcutsOptions, GlobalShortcuts, NewShortcut};
use ashpd::desktop::CreateSessionOptions;

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
        other => format!("Global shortcuts: the desktop refused them ({other})."),
    }
}

/// The shortcuts while they are wanted. Dropping it ends them.
pub struct Shortcuts {
    events: Receiver<Event>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl Shortcuts {
    /// Ask the desktop for the shortcuts. `wake` is called (on another
    /// thread) whenever there is an [`Event`] to take.
    pub fn start(wake: impl Fn() + Send + Sync + 'static) -> Shortcuts {
        let (tx, events) = mpsc::channel();
        let task = match crate::capture::portal_runtime() {
            Ok(runtime) => Some(runtime.spawn(async move {
                if let Err(error) = serve(&tx, &wake).await {
                    let _ = tx.send(Event::Failed(failure_line(&error)));
                    wake();
                }
            })),
            Err(why) => {
                let _ = tx.send(Event::Failed(format!("Global shortcuts: {why}.")));
                None
            }
        };
        Shortcuts { events, task }
    }

    /// The next event, if there is one.
    pub fn take(&self) -> Option<Event> {
        match self.events.try_recv() {
            Ok(event) => Some(event),
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => None,
        }
    }

    /// Shortcuts that deliver what a test sends.
    #[cfg(test)]
    pub fn for_test() -> (Shortcuts, Sender<Event>) {
        let (tx, events) = mpsc::channel();
        (Shortcuts { events, task: None }, tx)
    }
}

impl Drop for Shortcuts {
    fn drop(&mut self) {
        // The session goes with the task that holds it.
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

/// Bind the shortcuts and hand over every press, for as long as the desktop
/// keeps the session.
async fn serve(tx: &Sender<Event>, wake: &(impl Fn() + Send + Sync)) -> Result<(), ashpd::Error> {
    let portal = GlobalShortcuts::new().await?;
    let session = portal
        .create_session(CreateSessionOptions::default())
        .await?;
    let wanted: Vec<NewShortcut> = WANTED
        .iter()
        .map(|w| NewShortcut::new(w.id, w.does).preferred_trigger(w.keys))
        .collect();
    // Listening before binding: no press between the two is lost.
    let pressed = portal.receive_activated().await?;
    let bound = portal
        .bind_shortcuts(&session, &wanted, None, BindShortcutsOptions::default())
        .await?
        .response()?;
    let bound: Vec<(String, String)> = bound
        .shortcuts()
        .iter()
        .map(|s| (s.id().to_owned(), s.trigger_description().to_owned()))
        .collect();
    let _ = tx.send(Event::Bound(bound_line(&bound)));
    wake();

    use ashpd::zbus::export::futures_core::Stream;
    let mut pressed = std::pin::pin!(pressed);
    while let Some(press) =
        std::future::poll_fn(|context| pressed.as_mut().poll_next(context)).await
    {
        // (This program has one session of shortcuts, so a press is its.)
        if let Some(action) = action_of(press.shortcut_id()) {
            if tx.send(Event::Pressed(action)).is_err() {
                break;
            }
            wake();
        }
    }
    let _ = session.close().await;
    Ok(())
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
    fn a_desktop_without_the_portal_or_a_declined_dialog_is_said_plainly() {
        let declined = ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled);
        assert!(failure_line(&declined).contains("declined"));
        let other = ashpd::Error::Response(ashpd::desktop::ResponseError::Other);
        assert!(failure_line(&other).starts_with("Global shortcuts: the desktop refused them"));
    }
}
