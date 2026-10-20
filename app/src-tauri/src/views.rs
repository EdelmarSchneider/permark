//! Views: one child webview per tab (+ a window's Welcome view), all running the same frontend. Applies the
//! registry's view changes to webviews: the view on screen is shown, the others of its window hidden (state kept).
//! A new view starts on the page color of the theme until its page paints; a new window shows once its first view
//! painted (`view_ready`, else after a fallback). Only the view on screen sets the native title and takes the
//! keyboard focus and OS file drops.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use permark_core::workspace::{ClosingTab, Resolution, TabId, ViewChange};
use serde::{Deserialize, Serialize};
use tauri::webview::WebviewBuilder;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, Rect, WebviewUrl};
use ts_rs::TS;

use crate::shell::state;

/// A window whose view never says it painted is shown anyway after this (a broken page must not hide the app).
const READY_FALLBACK: Duration = Duration::from_secs(3);

/// The native title bar's theme (`None` in the command: the system's).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum BarTheme {
    Light,
    Dark,
}

/// Event `close-requested`: the window holds unsaved edits in these tabs; its view on screen asks once.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CloseRequested {
    /// The view that asks.
    pub window: String,
    pub tabs: Vec<ClosingTab>,
}

/// Event `save-requested` | `tab-close-requested`: to one view.
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ViewRequest {
    pub window: String,
}

/// The person's answer to the window close prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum CloseAnswer {
    SaveAll,
    Discard,
    Cancel,
}

/// A window close prompt in progress: the tabs listed; during Save all, the views still saving + the tabs that
/// failed.
#[derive(Debug, Default)]
struct CloseRound {
    tabs: Vec<ClosingTab>,
    saving: BTreeSet<String>,
    failed: Vec<TabId>,
}

#[derive(Debug, Default)]
struct Inner {
    /// Views whose page painted.
    ready: HashSet<String>,
    /// View -> the title its page asked for.
    titles: HashMap<String, String>,
    theme: Option<BarTheme>,
    rounds: HashMap<String, CloseRound>,
}

#[derive(Debug, Default)]
pub struct Views {
    inner: Mutex<Inner>,
}

fn lock(mutex: &Mutex<Inner>) -> MutexGuard<'_, Inner> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Webviews are made with a round trip to the main thread: never wait for it from there.
fn on_main_thread() -> bool {
    std::thread::current().name() == Some("main")
}

/// Applies view changes in order (off the main thread when views are to be made).
pub fn apply(handle: &AppHandle, changes: Vec<ViewChange>) {
    if changes.is_empty() {
        return;
    }
    let creates = changes
        .iter()
        .any(|c| matches!(c, ViewChange::Create { .. }));
    if creates && on_main_thread() {
        let handle = handle.clone();
        std::thread::spawn(move || apply(&handle, changes));
        return;
    }
    for change in changes {
        match change {
            ViewChange::Create { window, view } => create(handle, &window, &view),
            ViewChange::Show { window, view } => show(handle, &window, &view),
            // showing a view hides the others of its window (one visible at a time); a move is run by `open::move_tab`
            ViewChange::Hide { .. } | ViewChange::Transfer { .. } => {}
            ViewChange::Destroy { view } => destroy(handle, &view),
            ViewChange::CloseWindow { window } => close_window(handle, &window),
            ViewChange::AskClose { window, view, tabs } => ask_close(handle, &window, &view, tabs),
        }
    }
}

fn create(handle: &AppHandle, window: &str, view: &str) {
    let Some(native) = handle.get_window(window) else {
        return;
    };
    let size = native.inner_size().unwrap_or(PhysicalSize::new(1280, 800));
    let builder = WebviewBuilder::new(view, WebviewUrl::App("index.html".into()))
        .background_color(page_color(handle, &native));
    // the web view keeps its data in PerMark's own cache folder (Windows, Linux; debug builds in the `PerMark-dev`
    // one), never in a folder named after the app identifier: a development build and an installed one never share it
    #[cfg(not(target_os = "macos"))]
    let builder = builder.data_directory(state(handle).env.dirs.cache.join("webview"));
    match native.add_child(builder, PhysicalPosition::new(0, 0), size) {
        Ok(_) => {
            let (handle, view) = (handle.clone(), view.to_owned());
            std::thread::spawn(move || {
                std::thread::sleep(READY_FALLBACK);
                ready(&handle, &view);
            });
        }
        Err(error) => eprintln!("PerMark: tab view not made: {error}"),
    }
}

/// The page background of the theme (tokens `--bg`), so a view loading shows the app's color, not white.
fn page_color(handle: &AppHandle, native: &tauri::Window) -> tauri::window::Color {
    use permark_core::settings::{ThemePref, Tone};
    let settings = state(handle).env.settings();
    let dark = match settings.theme {
        ThemePref::Dark => true,
        ThemePref::Light => false,
        ThemePref::System => native.theme().is_ok_and(|t| t == tauri::Theme::Dark),
    };
    let (r, g, b) = match (dark, settings.tone) {
        (true, _) => (0x16, 0x17, 0x1a),
        (false, Tone::White) => (0xff, 0xff, 0xff),
        (false, Tone::Paper) => (0xf6, 0xf0, 0xe1),
        (false, Tone::Sage) => (0xea, 0xf1, 0xe3),
        (false, Tone::Gray) => (0xef, 0xed, 0xe9),
    };
    tauri::window::Color(r, g, b, 255)
}

/// The view's page painted: a window still hidden shows now (with that view, when it is the one on screen).
pub fn ready(handle: &AppHandle, view: &str) {
    let shell = state(handle);
    if !lock(&shell.views.inner).ready.insert(view.to_owned()) {
        return;
    }
    if !shell.registry.is_shown(view) {
        return;
    }
    if let Some(window) = shell.registry.window_of(view) {
        show_window(handle, &window);
    }
}

/// A window still hidden shows now, focused, its view on screen sized + focused.
pub fn show_window(handle: &AppHandle, window: &str) {
    let Some(native) = handle.get_window(window) else {
        return;
    };
    if native.is_visible().unwrap_or(true) {
        return;
    }
    let _ = native.show();
    let _ = native.set_focus();
    if let Some(webview) = state(handle)
        .registry
        .shown_view(window)
        .and_then(|view| handle.get_webview(&view))
    {
        fit_one(&native, &webview);
        let _ = webview.set_focus();
    }
}

/// Makes `view` the one on screen: others of the window hidden first (Linux: tabs share one box), then shown, sized to
/// the window again after showing (Linux: a first show can come out half height), focused, titled.
fn show(handle: &AppHandle, window: &str, view: &str) {
    let shell = state(handle);
    let (Some(native), Some(webview)) = (handle.get_window(window), handle.get_webview(view))
    else {
        return;
    };
    for other in native.webviews() {
        if other.label() != view {
            let _ = other.hide();
        }
    }
    fit_one(&native, &webview);
    let _ = webview.show();
    fit_one(&native, &webview);
    let (title, theme) = {
        let inner = lock(&shell.views.inner);
        (inner.titles.get(view).cloned(), inner.theme)
    };
    let _ = native.set_title(title.as_deref().unwrap_or("PerMark"));
    let _ = native.set_theme(theme.map(tauri_theme));
    let _ = webview.set_focus();
}

fn fit_one(native: &tauri::Window, webview: &tauri::Webview) {
    if let Ok(size) = native.inner_size() {
        let _ = webview.set_bounds(Rect {
            position: PhysicalPosition::new(0, 0).into(),
            size: PhysicalSize::new(size.width, size.height).into(),
        });
    }
}

/// The window resized: every view of it fills it.
pub fn fit(handle: &AppHandle, window: &str, size: PhysicalSize<u32>) {
    let Some(native) = handle.get_window(window) else {
        return;
    };
    for webview in native.webviews() {
        let _ = webview.set_bounds(Rect {
            position: PhysicalPosition::new(0, 0).into(),
            size: size.into(),
        });
    }
}

fn destroy(handle: &AppHandle, view: &str) {
    if let Some(webview) = handle.get_webview(view) {
        let _ = webview.close();
    }
    forget(handle, view);
}

/// A view is gone: its marks dropped.
pub fn forget(handle: &AppHandle, view: &str) {
    let shell = state(handle);
    {
        let mut inner = lock(&shell.views.inner);
        inner.ready.remove(view);
        inner.titles.remove(view);
    }
    shell.view_gone(view);
}

/// A window is gone: its marks dropped.
pub fn forget_window(handle: &AppHandle, window: &str) {
    lock(&state(handle).views.inner).rounds.remove(window);
}

fn close_window(handle: &AppHandle, window: &str) {
    let shell = state(handle);
    // the last window: the session keeps it (saved now, frozen until exit)
    if handle.windows().len() <= 1 {
        shell.save_session_now(handle);
        shell.saver.freeze();
    }
    if let Some(native) = handle.get_window(window) {
        let _ = native.destroy();
    }
}

fn ask_close(handle: &AppHandle, window: &str, view: &str, tabs: Vec<ClosingTab>) {
    lock(&state(handle).views.inner).rounds.insert(
        window.to_owned(),
        CloseRound {
            tabs: tabs.clone(),
            ..CloseRound::default()
        },
    );
    crate::windows::focus(handle, window);
    let _ = handle.emit_to(
        view,
        "close-requested",
        CloseRequested {
            window: view.to_owned(),
            tabs,
        },
    );
}

/// The close prompt of `window` was answered.
pub fn answer_close(handle: &AppHandle, window: &str, answer: CloseAnswer) {
    let shell = state(handle);
    let tabs = match lock(&shell.views.inner).rounds.get(window) {
        Some(round) if round.saving.is_empty() => round.tabs.clone(),
        // no prompt up, or Save all already running: nothing to do
        _ => return,
    };
    match answer {
        CloseAnswer::Cancel => resolve(handle, window, Resolution::Stay { show: None }),
        CloseAnswer::Discard => {
            // nothing written: the guards let go, the views go with the window
            for tab in &tabs {
                if let Some(view) = shell.registry.view_of_tab(tab.tab) {
                    if let Some((_, workspace)) = shell.registry.workspace_of(&view) {
                        workspace.documents().set_unsaved(&view, None);
                    }
                }
            }
            resolve(handle, window, Resolution::Close);
        }
        CloseAnswer::SaveAll => {
            let views: Vec<String> = tabs
                .iter()
                .filter_map(|t| shell.registry.view_of_tab(t.tab))
                .collect();
            if let Some(round) = lock(&shell.views.inner).rounds.get_mut(window) {
                round.saving = views.iter().cloned().collect();
            }
            for view in views {
                let _ = handle.emit_to(
                    view.as_str(),
                    "save-requested",
                    ViewRequest {
                        window: view.clone(),
                    },
                );
            }
        }
    }
}

/// A view answered `save-requested`: once every listed view did, the window closes (all saved) or stays with the
/// first failed tab shown.
pub fn view_saved(handle: &AppHandle, view: &str, saved: bool) {
    let shell = state(handle);
    let Some(window) = shell.registry.window_of(view) else {
        return;
    };
    let tab = shell.registry.place_of_view(view).map(|p| p.tab);
    let done = {
        let mut inner = lock(&shell.views.inner);
        let Some(round) = inner.rounds.get_mut(&window) else {
            return;
        };
        if !round.saving.remove(view) {
            return;
        }
        if !saved {
            round.failed.extend(tab);
        }
        round.saving.is_empty().then(|| {
            round
                .tabs
                .iter()
                .map(|t| t.tab)
                .find(|t| round.failed.contains(t))
        })
    };
    match done {
        None => {}
        Some(None) => resolve(handle, &window, Resolution::Close),
        Some(Some(failed)) => resolve(handle, &window, Resolution::Stay { show: Some(failed) }),
    }
}

fn resolve(handle: &AppHandle, window: &str, resolution: Resolution) {
    let shell = state(handle);
    lock(&shell.views.inner).rounds.remove(window);
    let outcome = shell.registry.resolve_close(window, resolution);
    crate::open::run(handle, outcome);
}

/// The view's page asks for this title: set when that view is the one on screen.
pub fn set_title(handle: &AppHandle, view: &str, title: &str) {
    let shell = state(handle);
    lock(&shell.views.inner)
        .titles
        .insert(view.to_owned(), title.to_owned());
    if shell.registry.is_shown(view) {
        if let Some(native) = shell
            .registry
            .window_of(view)
            .and_then(|w| handle.get_window(&w))
        {
            let _ = native.set_title(title);
        }
    }
}

/// The theme preference (app-wide): every window's title bar follows.
pub fn set_theme(handle: &AppHandle, theme: Option<BarTheme>) {
    lock(&state(handle).views.inner).theme = theme;
    for native in handle.windows().into_values() {
        let _ = native.set_theme(theme.map(tauri_theme));
    }
}

fn tauri_theme(theme: BarTheme) -> tauri::Theme {
    match theme {
        BarTheme::Light => tauri::Theme::Light,
        BarTheme::Dark => tauri::Theme::Dark,
    }
}
