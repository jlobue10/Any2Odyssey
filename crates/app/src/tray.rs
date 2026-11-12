//! Notification-area icon and its menu (Windows). The same command set is
//! used by the controls window's right-click menu.

use std::cell::{Cell, RefCell};
use std::sync::{Mutex, OnceLock};

use muda::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::core::{Core, Forwarded, MenuCommand, UiRequest};

/// Icon and menu events handed over by the libraries' callbacks; see
/// [`Tray::new`].
static ICON_EVENTS: Forwarded<TrayIconEvent> = Forwarded::new();
static MENU_EVENTS: Forwarded<MenuEvent> = Forwarded::new();

pub struct Tray {
    icon: TrayIcon,
    toggle_item: MenuItem,
    keep_item: CheckMenuItem,
    last_wanted: Cell<Option<bool>>,
    last_keep: Cell<Option<bool>>,
    last_tooltip: RefCell<Option<String>>,
}

struct Ids {
    map: Vec<(MenuId, MenuCommand)>,
    keep: MenuId,
}

static IDS: OnceLock<Mutex<Option<Ids>>> = OnceLock::new();

/// Whether [`drain`] does anything with an icon event. The library reports
/// every mouse move over the icon, and entering, leaving and each button
/// press and release; only a double-click is acted on.
fn acted_on(event: &TrayIconEvent) -> bool {
    matches!(event, TrayIconEvent::DoubleClick { .. })
}

/// The icon-event handler: keep what [`drain`] acts on, drop the rest.
/// Every kept event wakes the poller for a whole supervision tick (the
/// shared state locked, the worker's status file read), so forwarding the
/// moves would run one tick after another for as long as the pointer
/// travels across the icon.
fn forward_icon_event(event: TrayIconEvent) {
    if acted_on(&event) {
        ICON_EVENTS.push(event);
    }
}

pub fn build_menu() -> (Menu, MenuItem, CheckMenuItem) {
    let menu = Menu::new();
    let mut map = Vec::new();
    let mut add = |text: &str, cmd: MenuCommand| {
        let item = MenuItem::new(text, true, None);
        map.push((item.id().clone(), cmd));
        let _ = menu.append(&item);
        item
    };
    add("Show controls", MenuCommand::ShowControls);
    let toggle = add("Turn 3D on", MenuCommand::Toggle);
    add("Restore overlay (Ctrl+Alt+F8)", MenuCommand::RestoreOverlay);
    let keep = CheckMenuItem::new("Keep overlay above apps", true, true, None);
    let _ = menu.append(&keep);
    let _ = menu.append(&PredefinedMenuItem::separator());
    add(
        "Always convert desktop (AI)",
        MenuCommand::AlwaysConvertDesktop,
    );
    add("Side-by-side mode", MenuCommand::SideBySide);
    add("AI conversion mode", MenuCommand::AiMode);
    add("Restart conversion", MenuCommand::Restart);
    let _ = menu.append(&PredefinedMenuItem::separator());
    add("Help", MenuCommand::Help);
    add("About Any2Odyssey", MenuCommand::About);
    add("Check for updates", MenuCommand::CheckUpdates);
    add("Toggle light / dark mode", MenuCommand::ToggleTheme);
    add("View logs", MenuCommand::ViewLogs);
    add("Minimize to tray", MenuCommand::MinimizeToTray);
    let _ = menu.append(&PredefinedMenuItem::separator());
    add("Quit completely", MenuCommand::Quit);
    let ids = Ids {
        map,
        keep: keep.id().clone(),
    };
    let _ = IDS.set(Mutex::new(Some(ids)));
    (menu, toggle, keep)
}

impl Tray {
    /// `Err` means the icon image was rejected or the tray's hidden window
    /// could not be created; the caller then treats the tray as absent
    /// (Minimize and Close act normally, a notice says so).
    ///
    /// It does not mean "the shell showed no icon". tray-icon 0.21.3 returns
    /// `Ok` from `build()` even when the shell refuses the registration, for
    /// instance while Explorer is not running, and registers again when the
    /// shell broadcasts `TaskbarCreated`. That also covers an Explorer
    /// restart later on. So with no shell at launch the controls still hide
    /// to a tray icon that appears only once Explorer does; until then
    /// Ctrl+Alt+F9, `--show` or launching again brings them back. The crate
    /// gives no way to ask whether the icon is currently registered.
    pub fn new(icon_rgba: Vec<u8>, width: u32, height: u32) -> Result<Tray, String> {
        // Without handlers both libraries queue their events on channels
        // nothing waits on, and a menu command sat there until the poller's
        // next tick. The handlers run on the GUI thread; they only store the
        // event and wake the poller. Should a library ignore its handler
        // (one was set before), events keep arriving on its channel, which
        // `drain` still reads.
        TrayIconEvent::set_event_handler(Some(forward_icon_event));
        MenuEvent::set_event_handler(Some(|event| MENU_EVENTS.push(event)));
        let (menu, toggle_item, keep_item) = build_menu();
        let icon = Icon::from_rgba(icon_rgba, width, height).map_err(|e| e.to_string())?;
        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("Any2Odyssey — off")
            .with_icon(icon)
            .with_menu_on_left_click(false)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Tray {
            icon,
            toggle_item,
            keep_item,
            last_wanted: Cell::new(None),
            last_keep: Cell::new(None),
            last_tooltip: RefCell::new(None),
        })
    }

    /// Keep dynamic items in sync with state (called from the GUI thread).
    /// `update` is the version an update check found, if any.
    pub fn sync(&self, wanted: bool, keep_on_top: bool, update: Option<&str>) {
        if self.last_wanted.get() != Some(wanted) {
            self.toggle_item
                .set_text(if wanted { "Turn 3D off" } else { "Turn 3D on" });
            self.last_wanted.set(Some(wanted));
        }
        let state = if wanted { "on" } else { "off" };
        let tooltip = match update {
            Some(version) => format!("Any2Odyssey — {state} · update {version} available"),
            None => format!("Any2Odyssey — {state}"),
        };
        if self.last_tooltip.borrow().as_deref() != Some(tooltip.as_str())
            && self.icon.set_tooltip(Some(tooltip.as_str())).is_ok()
        {
            *self.last_tooltip.borrow_mut() = Some(tooltip);
        }
        if self.last_keep.get() != Some(keep_on_top) {
            self.keep_item.set_checked(keep_on_top);
            self.last_keep.set(Some(keep_on_top));
        }
    }
}

/// Deliver tray and menu events to the core (any thread).
pub fn drain(core: &mut Core) {
    let mut icon_events = ICON_EVENTS.take();
    icon_events.extend(TrayIconEvent::receiver().try_iter());
    for event in icon_events {
        if acted_on(&event) {
            core.request(UiRequest::ShowControls);
        }
    }
    let Some(ids) = IDS.get() else { return };
    let Ok(guard) = ids.lock() else { return };
    let Some(ids) = guard.as_ref() else { return };
    let mut menu_events = MENU_EVENTS.take();
    menu_events.extend(MenuEvent::receiver().try_iter());
    for event in menu_events {
        if event.id == ids.keep {
            let value = !core.prefs.keep_overlay_on_top;
            core.menu(MenuCommand::KeepOverlayOnTop(value));
            continue;
        }
        if let Some((_, cmd)) = ids.map.iter().find(|(id, _)| *id == event.id) {
            core.menu(*cmd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tray_icon::dpi::PhysicalPosition;
    use tray_icon::{MouseButton, MouseButtonState, Rect, TrayIconId};

    #[test]
    fn only_a_double_click_is_forwarded_to_the_poller() {
        let id = || TrayIconId::new("test");
        let position = PhysicalPosition::new(10.0, 10.0);
        let rect = Rect::default();
        // What the library sends while the pointer crosses the icon and for
        // a single click: none of it may queue an event or wake the poller.
        for ignored in [
            TrayIconEvent::Enter {
                id: id(),
                position,
                rect,
            },
            TrayIconEvent::Move {
                id: id(),
                position,
                rect,
            },
            TrayIconEvent::Leave {
                id: id(),
                position,
                rect,
            },
            TrayIconEvent::Click {
                id: id(),
                position,
                rect,
                button: MouseButton::Left,
                button_state: MouseButtonState::Down,
            },
            TrayIconEvent::Click {
                id: id(),
                position,
                rect,
                button: MouseButton::Right,
                button_state: MouseButtonState::Up,
            },
        ] {
            assert!(!acted_on(&ignored), "{ignored:?}");
            forward_icon_event(ignored);
        }
        assert!(ICON_EVENTS.take().is_empty());
        forward_icon_event(TrayIconEvent::DoubleClick {
            id: id(),
            position,
            rect,
            button: MouseButton::Left,
        });
        let kept = ICON_EVENTS.take();
        assert_eq!(kept.len(), 1);
        assert!(matches!(kept[0], TrayIconEvent::DoubleClick { .. }));
    }
}
