//! Notification-area icon and its menu (Windows). The same command set is
//! used by the controls window's right-click menu.

use std::cell::{Cell, RefCell};
use std::sync::{Mutex, OnceLock};

use muda::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::core::{Core, MenuCommand, UiRequest};

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
    pub fn new(icon_rgba: Vec<u8>, width: u32, height: u32) -> Result<Tray, String> {
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
    while let Ok(event) = TrayIconEvent::receiver().try_recv() {
        if let TrayIconEvent::DoubleClick { .. } = event {
            core.request(UiRequest::ShowControls);
        }
    }
    let Some(ids) = IDS.get() else { return };
    let Ok(guard) = ids.lock() else { return };
    let Some(ids) = guard.as_ref() else { return };
    while let Ok(event) = MenuEvent::receiver().try_recv() {
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
