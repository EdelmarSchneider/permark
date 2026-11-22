//! The macOS app menu: the system's standard entries, with the app's own Quit. The system's Quit ends the process at
//! once (no window is asked to close), so unsaved edits would be lost without a question; the app's Quit asks every
//! work window to close, as its close button does. Built from the cross-platform menu API, so it builds and is tested
//! on every system; installed on macOS only (other systems have no app menu).
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use tauri::menu::{
    AboutMetadata, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu, HELP_SUBMENU_ID,
    WINDOW_SUBMENU_ID,
};
use tauri::{AppHandle, Manager, Wry};

/// Id of the app's own Quit item.
const QUIT: &str = "quit";
/// Keys of the app's own Quit (Cmd+Q on macOS).
const QUIT_KEYS: &str = "CmdOrCtrl+Q";

/// One entry of the menu bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Entry {
    About,
    Services,
    Hide,
    HideOthers,
    ShowAll,
    /// The app's own item [`QUIT`], never the system's.
    Quit,
    CloseWindow,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Fullscreen,
    Minimize,
    Zoom,
    Separator,
}

/// A submenu of the bar: its title (`None` = the app's name), the system role it plays (Window | Help menu), entries.
struct Group {
    title: Option<&'static str>,
    role: Option<&'static str>,
    entries: &'static [Entry],
}

/// The bar: the system's default menu (as Tauri builds it), Show All added, Quit the app's own.
const LAYOUT: [Group; 6] = {
    use Entry::{
        About, CloseWindow, Copy, Cut, Fullscreen, Hide, HideOthers, Minimize, Paste, Quit, Redo,
        SelectAll, Separator, Services, ShowAll, Undo, Zoom,
    };
    [
        Group {
            title: None,
            role: None,
            entries: &[
                About, Separator, Services, Separator, Hide, HideOthers, ShowAll, Separator, Quit,
            ],
        },
        Group {
            title: Some("File"),
            role: None,
            entries: &[CloseWindow],
        },
        Group {
            title: Some("Edit"),
            role: None,
            entries: &[Undo, Redo, Separator, Cut, Copy, Paste, SelectAll],
        },
        Group {
            title: Some("View"),
            role: None,
            entries: &[Fullscreen],
        },
        Group {
            title: Some("Window"),
            role: Some(WINDOW_SUBMENU_ID),
            entries: &[Minimize, Zoom, Separator, CloseWindow],
        },
        Group {
            title: Some("Help"),
            role: Some(HELP_SUBMENU_ID),
            entries: &[],
        },
    ]
};

/// The menu bar of [`LAYOUT`].
///
/// # Errors
/// The system refused a menu.
pub fn build(handle: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let name = handle.package_info().name.clone();
    let menu = Menu::new(handle)?;
    for group in &LAYOUT {
        let title = group.title.unwrap_or(&name);
        let submenu = match group.role {
            Some(role) => Submenu::with_id(handle, role, title, true)?,
            None => Submenu::new(handle, title, true)?,
        };
        for entry in group.entries {
            append(handle, &submenu, *entry, &name)?;
        }
        menu.append(&submenu)?;
    }
    Ok(menu)
}

fn append(
    handle: &AppHandle,
    submenu: &Submenu<Wry>,
    entry: Entry,
    name: &str,
) -> tauri::Result<()> {
    let item = match entry {
        Entry::Quit => {
            let quit =
                MenuItem::with_id(handle, QUIT, format!("Quit {name}"), true, Some(QUIT_KEYS))?;
            return submenu.append(&quit);
        }
        Entry::About => PredefinedMenuItem::about(handle, None, Some(about(handle)))?,
        Entry::Services => PredefinedMenuItem::services(handle, None)?,
        Entry::Hide => PredefinedMenuItem::hide(handle, None)?,
        Entry::HideOthers => PredefinedMenuItem::hide_others(handle, None)?,
        Entry::ShowAll => PredefinedMenuItem::show_all(handle, None)?,
        Entry::CloseWindow => PredefinedMenuItem::close_window(handle, None)?,
        Entry::Undo => PredefinedMenuItem::undo(handle, None)?,
        Entry::Redo => PredefinedMenuItem::redo(handle, None)?,
        Entry::Cut => PredefinedMenuItem::cut(handle, None)?,
        Entry::Copy => PredefinedMenuItem::copy(handle, None)?,
        Entry::Paste => PredefinedMenuItem::paste(handle, None)?,
        Entry::SelectAll => PredefinedMenuItem::select_all(handle, None)?,
        Entry::Fullscreen => PredefinedMenuItem::fullscreen(handle, None)?,
        Entry::Minimize => PredefinedMenuItem::minimize(handle, None)?,
        Entry::Zoom => PredefinedMenuItem::maximize(handle, None)?,
        Entry::Separator => PredefinedMenuItem::separator(handle)?,
    };
    submenu.append(&item)
}

/// What About shows, filled as Tauri's default menu fills it.
fn about(handle: &AppHandle) -> AboutMetadata<'static> {
    let (info, config) = (handle.package_info(), handle.config());
    AboutMetadata {
        name: Some(info.name.clone()),
        version: Some(info.version.to_string()),
        copyright: config.bundle.copyright.clone(),
        authors: config.bundle.publisher.clone().map(|p| vec![p]),
        ..AboutMetadata::default()
    }
}

/// The app's Quit: every work window is asked to close, as by its close button (unsaved edits -> that window's
/// prompt; Cancel keeps it and the app). All are asked before any closes, so the last one asked saves the session with
/// every window in it.
pub fn on_event(handle: &AppHandle, event: &MenuEvent) {
    if *event.id() != QUIT {
        return;
    }
    let closing = quit_plan(
        handle.windows().into_keys(),
        crate::print::is_sheet,
        |label| crate::request_close(handle, label),
    );
    for label in closing {
        if let Some(window) = handle.get_window(&label) {
            let _ = window.destroy();
        }
    }
}

/// The windows Quit closes: each one of `labels` but a print sheet is asked (`ask`), every one even after a no (each
/// answers for its own unsaved edits); only those that agreed are returned.
fn quit_plan(
    labels: impl IntoIterator<Item = String>,
    is_sheet: impl Fn(&str) -> bool,
    mut ask: impl FnMut(&str) -> bool,
) -> Vec<String> {
    labels
        .into_iter()
        .filter(|label| !is_sheet(label))
        .filter(|label| ask(label))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::Entry::{
        About, Copy, Cut, Hide, HideOthers, Minimize, Paste, Quit, Redo, SelectAll, Separator,
        Services, ShowAll, Undo, Zoom,
    };
    use super::*;

    fn group(title: Option<&str>) -> &'static Group {
        LAYOUT.iter().find(|g| g.title == title).unwrap()
    }

    #[test]
    fn quit_is_the_apps_own_item_on_cmd_q_once_last_in_the_app_menu() {
        assert_eq!((QUIT, QUIT_KEYS), ("quit", "CmdOrCtrl+Q"));
        assert_eq!(group(None).entries.last(), Some(&Quit));
        let quits = LAYOUT
            .iter()
            .flat_map(|g| g.entries)
            .filter(|e| **e == Quit)
            .count();
        assert_eq!(quits, 1);
    }

    #[test]
    fn the_app_menu_keeps_the_system_entries() {
        let named: Vec<Entry> = group(None)
            .entries
            .iter()
            .copied()
            .filter(|e| *e != Separator)
            .collect();
        assert_eq!(
            named,
            [About, Services, Hide, HideOthers, ShowAll, Quit].to_vec()
        );
    }

    #[test]
    fn the_edit_menu_keeps_the_text_keys() {
        // the key equivalents of these entries carry Cmd+Z | Shift+Cmd+Z | Cmd+X | C | V | A to fields and editors
        let edit = group(Some("Edit"));
        for entry in [Undo, Redo, Cut, Copy, Paste, SelectAll] {
            assert!(edit.entries.contains(&entry), "{entry:?}");
        }
    }

    #[test]
    fn quit_asks_every_work_window_even_after_a_no_and_closes_only_the_agreeing_ones() {
        let labels = ["main", "export-1", "w2", "w3"].map(String::from);
        let mut asked = Vec::new();
        let closing = quit_plan(
            labels,
            |label| label.starts_with("export-"),
            |label| {
                asked.push(label.to_owned());
                label != "main"
            },
        );
        // the first window's no stops no other question; a print sheet is never asked
        assert_eq!(asked, ["main", "w2", "w3"]);
        assert_eq!(closing, ["w2", "w3"]);
        // nobody agrees -> nothing closes
        let none = quit_plan(["a".to_owned(), "b".to_owned()], |_| false, |_| false);
        assert_eq!(none, Vec::<String>::new());
    }

    #[test]
    fn the_window_and_help_menus_play_the_system_roles() {
        let window = group(Some("Window"));
        assert_eq!(window.role, Some(WINDOW_SUBMENU_ID));
        assert!(window.entries.contains(&Minimize) && window.entries.contains(&Zoom));
        assert_eq!(group(Some("Help")).role, Some(HELP_SUBMENU_ID));
    }
}
