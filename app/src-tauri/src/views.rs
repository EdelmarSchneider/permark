//! Views: one child webview per tab (+ a window's Welcome view), all running the same frontend. Applies the
//! registry's view changes to webviews: the view on screen is shown, the others of its window hidden (state kept).
//! A new view starts on the page color of the theme until its page paints; a new window shows once its first view
//! painted (`view_ready`, else after a fallback). Only the view on screen sets the native title and takes the
//! keyboard focus and OS file drops. A view asked about a close has a deadline to say it heard (its prompt then waits
//! for the person, untimed); past it a system dialog offers Close anyway | Wait, so a dead or hung page never leaves a
//! window that cannot close. A view that cannot be made never leaves a window hidden for good: the window closes, or
//! the app exits when no window is left on screen.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use permark_core::workspace::{ClosingTab, Resolution, TabId, ViewChange};
use serde::{Deserialize, Serialize};
use tauri::webview::{NewWindowResponse, WebviewBuilder};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, Rect, WebviewUrl};
use tauri_plugin_dialog::{
    DialogExt, MessageDialogBuilder, MessageDialogButtons, MessageDialogKind,
};
use ts_rs::TS;

use crate::shell::{state, Shell};

/// A window whose view never says it painted is shown anyway after this (a broken page must not hide the app).
const READY_FALLBACK: Duration = Duration::from_secs(3);

/// How long a view may take to say it heard a close ask, or to answer a Save all (no step of the person's in it).
const ROUND_DEADLINE: Duration = Duration::from_secs(10);

/// Longest dialog word the frontend may push (a system dialog has no scroll).
const MAX_WORDS: usize = 600;

/// A view's page that ended loads again at most once in this time: a page that ends after each boot must not load
/// for ever (a boot does not count).
const RELOAD_SPACING: Duration = Duration::from_secs(60);

/// The window-failed text until the frontend sends its own.
const WINDOW_FAILED: &str =
    "PerMark couldn't show one of its windows, so that window was closed. Your other windows keep working.";

/// The text before the app exits because no window could get its page, until the frontend sends its own.
const VIEW_FAILED_EXIT: &str =
    "PerMark couldn't open its window, so it has to close. Your open tabs come back the \
                                next time you start PerMark.";

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

/// Event `view-moved`: the view now lives in window `to` (its chips are that window's tabs).
#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ViewMoved {
    pub window: String,
    pub to: String,
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

/// Words of the app's own system dialogs, in the person's language. The backend holds no translations: the frontend
/// pushes them (boot, language change); English until then.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct NativeWords {
    /// Title of the dialog when a page does not answer.
    pub not_responding: String,
    /// Its text.
    pub not_responding_detail: String,
    /// Its button that closes without saving.
    pub close_anyway: String,
    /// Its button that keeps waiting.
    pub wait: String,
    /// Text of the error before the app closes because the system web view stopped.
    pub engine_stopped: String,
    /// Text of the error when a window could not get its page and closed (the other windows keep working). English
    /// until the frontend sends it.
    #[serde(default)]
    #[ts(optional)]
    pub window_failed: Option<String>,
    /// Text of the error before the app closes because its window could not get its page and no other window works
    /// (no edits are lost). English until the frontend sends it.
    #[serde(default)]
    #[ts(optional)]
    pub view_failed_exit: Option<String>,
}

impl Default for NativeWords {
    fn default() -> Self {
        Self {
            not_responding: "PerMark isn't responding".into(),
            // Close anyway on a window's close round lets go of every tab's unsaved edits; on one page only its own
            not_responding_detail: "A page in this window isn't answering. If you close anyway, unsaved changes \
                                    in this window's tabs may be lost."
                .into(),
            close_anyway: "Close anyway".into(),
            wait: "Wait".into(),
            engine_stopped: "The part of PerMark that shows its windows stopped working, so PerMark has to close. \
                Unsaved changes are lost; your open tabs come back the next time you start PerMark."
                .into(),
            window_failed: None,
            view_failed_exit: None,
        }
    }
}

impl NativeWords {
    /// Fit for a system dialog: no empty | overlong word, no control character but a line break, and two different
    /// button labels (the dialog tells its buttons apart by label).
    fn usable(&self) -> bool {
        let fits = |word: &String| {
            !word.trim().is_empty()
                && word.chars().count() <= MAX_WORDS
                && !word.chars().any(|c| c.is_control() && c != '\n')
        };
        [
            &self.not_responding,
            &self.not_responding_detail,
            &self.close_anyway,
            &self.wait,
            &self.engine_stopped,
        ]
        .into_iter()
        .chain(self.window_failed.as_ref())
        .chain(self.view_failed_exit.as_ref())
        .all(fits)
            && self.close_anyway.trim() != self.wait.trim()
    }

    fn window_failed(&self) -> String {
        self.window_failed
            .clone()
            .unwrap_or_else(|| WINDOW_FAILED.to_owned())
    }

    fn view_failed_exit(&self) -> String {
        self.view_failed_exit
            .clone()
            .unwrap_or_else(|| VIEW_FAILED_EXIT.to_owned())
    }
}

/// Boot fact of a view whose page was loaded again after its page process ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Reloaded {
    /// The document whose unsaved edits went with the page (the backend never holds unsaved text).
    pub lost: Option<String>,
}

/// The app waits on a view since `since`; `asking`: the not-responding dialog about this wait is up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Wait {
    since: Instant,
    asking: bool,
}

impl Wait {
    const fn new(now: Instant) -> Self {
        Self {
            since: now,
            asking: false,
        }
    }

    /// The view had its time, and no dialog about this wait is up yet.
    fn overdue(&self, now: Instant) -> bool {
        !self.asking && now.saturating_duration_since(self.since) >= ROUND_DEADLINE
    }

    /// Overdue: the dialog goes up (one per wait); the wait it is about.
    fn ask(&mut self, now: Instant) -> Option<Instant> {
        self.overdue(now).then(|| {
            self.asking = true;
            self.since
        })
    }

    /// The person answered the dialog about the wait `since`: Wait (`close` false) starts the clock again. `false`:
    /// the dialog is stale (the view answered meanwhile, or another wait began), nothing to do.
    fn answered(&mut self, since: Instant, close: bool, now: Instant) -> bool {
        let current = self.asking && self.since == since;
        if current && !close {
            *self = Self::new(now);
        }
        current
    }
}

/// A window close prompt in progress: the tabs listed; during Save all, the views still saving + the tabs that
/// failed.
#[derive(Debug, Default)]
struct CloseRound {
    tabs: Vec<ClosingTab>,
    saving: BTreeSet<String>,
    failed: Vec<TabId>,
    /// The view asked, until it says it heard (the person's answer is not timed).
    asked: Option<String>,
    /// While the app waits on the views: the asked one to hear, the saving ones to answer.
    wait: Option<Wait>,
}

impl CloseRound {
    fn asking(tabs: Vec<ClosingTab>, view: &str, now: Instant) -> Self {
        Self {
            tabs,
            asked: Some(view.to_owned()),
            wait: Some(Wait::new(now)),
            ..Self::default()
        }
    }

    /// `view` says it heard the ask: its prompt is up and waits for the person.
    fn heard(&mut self, view: &str) {
        if self.asked.as_deref() == Some(view) {
            self.asked = None;
            if self.saving.is_empty() {
                self.wait = None;
            }
        }
    }

    /// Save all: the views save with no step of the person's, so their answers are timed.
    fn save_all(&mut self, views: BTreeSet<String>, now: Instant) {
        self.asked = None;
        self.saving = views;
        self.wait = Some(Wait::new(now));
    }
}

/// What the app does when a view of a window could not be made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CreateFailed {
    /// Another view of the window works: it stays.
    Ignore,
    /// The window holds no view (it would stay hidden | empty for good) while another window is on screen: it closes
    /// and an error says why.
    CloseWindow,
    /// No other window is on screen: an error says why, then the app exits.
    Exit,
}

/// `other_views`: the window holds another view; `other_shown_windows`: another window is on screen. The first failure
/// decides: a web view that cannot make one view usually cannot make the next either, so no retry is tried.
const fn on_create_failed(other_views: bool, other_shown_windows: bool) -> CreateFailed {
    match (other_views, other_shown_windows) {
        (true, _) => CreateFailed::Ignore,
        (false, true) => CreateFailed::CloseWindow,
        (false, false) => CreateFailed::Exit,
    }
}

/// What the app waits on a view for.
#[derive(Debug, Clone)]
enum Awaited {
    /// The close round of a window (keyed by window).
    Round(String),
    /// The unsaved prompt before a tab closes (keyed by its view).
    Tab(String),
}

/// What Close anyway on hung pages of a window does.
#[derive(Debug, PartialEq, Eq)]
enum HungClose {
    /// No other view of the window works: the window closes as Discard does.
    Window,
    /// Only these views let go of their unsaved edits and their tabs close; the window's other views keep theirs.
    /// `end_round`: the window's close prompt is up in one of them (it can never be answered, and no tab closes while
    /// it is up): the close round ends as Stay first.
    Tabs { views: Vec<String>, end_round: bool },
}

/// `window_views`: the views of the window now; `hung`: the views reported hung; `prompt_in`: the view showing the
/// window's close prompt, when one is up.
fn on_hung_close(
    window_views: &[String],
    hung: &BTreeSet<String>,
    prompt_in: Option<&str>,
) -> HungClose {
    if window_views.iter().all(|view| hung.contains(view)) {
        return HungClose::Window;
    }
    HungClose::Tabs {
        views: window_views
            .iter()
            .filter(|view| hung.contains(*view))
            .cloned()
            .collect(),
        end_round: prompt_in.is_some_and(|view| hung.contains(view)),
    }
}

/// What a not-responding dialog is about.
#[derive(Debug, Clone)]
enum Stuck {
    /// A wait past its deadline (the wait's start tells a stale answer).
    Awaited(Awaited, Instant),
    /// A window with pages that stopped answering (the system web view says so).
    Hung(String),
}

#[derive(Debug, Default)]
struct Inner {
    /// Views whose page painted.
    ready: HashSet<String>,
    /// View -> the title its page asked for.
    titles: HashMap<String, String>,
    theme: Option<BarTheme>,
    rounds: HashMap<String, CloseRound>,
    /// View -> the wait for it to hear its tab's close ask.
    tab_waits: HashMap<String, Wait>,
    /// Window -> its views that stopped answering, while the not-responding dialog about them is up (one per window).
    hung: HashMap<String, BTreeSet<String>>,
    /// View -> its page was loaded again and has not booted since.
    reloaded: HashMap<String, Reloaded>,
    /// View -> when its page last loaded again after its process ended (kept across boots).
    reloads: HashMap<String, Instant>,
    words: NativeWords,
}

impl Inner {
    fn wait_of(&mut self, awaited: &Awaited) -> Option<&mut Wait> {
        match awaited {
            Awaited::Round(window) => self.rounds.get_mut(window).and_then(|r| r.wait.as_mut()),
            Awaited::Tab(view) => self.tab_waits.get_mut(view),
        }
    }

    /// The page of `view` ended: it may load again now (recorded as loaded), unless it did less than
    /// [`RELOAD_SPACING`] ago.
    fn reload_due(&mut self, view: &str, now: Instant) -> bool {
        let due = self
            .reloads
            .get(view)
            .is_none_or(|at| now.saturating_duration_since(*at) >= RELOAD_SPACING);
        if due {
            self.reloads.insert(view.to_owned(), now);
        }
        due
    }

    /// The page of `view` loads again; its next boot tells that `lost` (or the file lost by an earlier load it never
    /// booted after) went with the page.
    fn loaded_again(&mut self, view: &str, lost: Option<String>) {
        let told = self
            .reloaded
            .entry(view.to_owned())
            .or_insert(Reloaded { lost: None });
        if lost.is_some() {
            told.lost = lost;
        }
    }
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

/// Moves `webview` into `window`, run on the main thread (waited for here). Tauri's `reparent` holds the webview's
/// runtime lock while it waits for the main thread: called from another thread, anything the main thread does to that
/// webview meanwhile (a window resize fitting its views) waits for that lock forever and the app freezes. On the main
/// thread there is no wait (events it causes are queued until it returns).
///
/// # Errors
/// The reparent failed, the main thread is gone.
pub fn reparent(
    handle: &AppHandle,
    webview: &tauri::Webview,
    window: &tauri::Window,
) -> tauri::Result<()> {
    if on_main_thread() {
        return webview.reparent(window);
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let (webview, window) = (webview.clone(), window.clone());
    handle.run_on_main_thread(move || {
        let _ = tx.send(webview.reparent(&window));
    })?;
    rx.recv()
        .unwrap_or(Err(tauri::Error::FailedToReceiveMessage))
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
        .background_color(page_color(handle, &native))
        // a middle | Shift click on a link, or `window.open`, never opens a web view window the app does not manage:
        // the app follows links itself; refused here, not left to the web view library's default
        .on_new_window(|_url, _features| NewWindowResponse::Deny);
    #[cfg(not(target_os = "macos"))]
    let builder = match data_directory(&state(handle).env) {
        Some(dir) => builder.data_directory(dir),
        None => builder,
    };
    match native.add_child(builder, PhysicalPosition::new(0, 0), size) {
        Ok(webview) => {
            crate::renderer::watch(handle, &webview);
            let (handle, view) = (handle.clone(), view.to_owned());
            std::thread::spawn(move || {
                std::thread::sleep(READY_FALLBACK);
                ready(&handle, &view);
            });
        }
        Err(error) => {
            eprintln!("PerMark: tab view not made: {error}");
            let other_views = !native.webviews().is_empty();
            let other_shown = !state(handle).registry.is_last_on_screen(window);
            match on_create_failed(other_views, other_shown) {
                CreateFailed::Ignore => {}
                CreateFailed::CloseWindow => window_failed(handle, &native),
                // the restore may still be under way: the session stays as last saved, never saved partial
                CreateFailed::Exit => fatal(handle, false, words(handle).view_failed_exit()),
            }
        }
    }
}

/// `native` could not get a view and holds none: it closes and an error over a window on screen says why.
fn window_failed(handle: &AppHandle, native: &tauri::Window) {
    let parent = state(handle)
        .registry
        .last_focused_work()
        .filter(|w| w != native.label())
        .and_then(|w| handle.get_window(&w));
    let _ = native.destroy();
    error_dialog(handle, words(handle).window_failed(), parent.as_ref()).show(|_| {});
}

/// No view of the app can work any more: the session is kept (`save`: saved now as the registry holds it; else as
/// last saved) and frozen so nothing after overwrites it (the windows going must not save one without their tabs),
/// an error (`text`) says why, then the app exits. Once, however many views report it.
pub fn fatal(handle: &AppHandle, save: bool, text: String) {
    static ENDED: AtomicBool = AtomicBool::new(false);
    if ENDED.swap(true, Ordering::SeqCst) {
        return;
    }
    let shell = state(handle);
    if save {
        shell.save_session_now(handle);
    }
    shell.saver.freeze();
    let exit = handle.clone();
    error_dialog(handle, text, None).show(move |_| exit.exit(1));
}

/// The app's own error dialog (one OK button), over `parent` when given. Shown with a callback, never waited for.
fn error_dialog(
    handle: &AppHandle,
    text: String,
    parent: Option<&tauri::Window>,
) -> MessageDialogBuilder<tauri::Wry> {
    let dialog = handle
        .dialog()
        .message(text)
        .title("PerMark")
        .kind(MessageDialogKind::Error)
        .buttons(MessageDialogButtons::Ok);
    match parent {
        Some(parent) => dialog.parent(parent),
        None => dialog,
    }
}

/// The web view data folder of every webview the app makes: PerMark's own cache folder (Windows, Linux; debug builds
/// in the `PerMark-dev` one), never a folder named after the app identifier, so a development build and an installed
/// one never share it. `None` on macOS (the system keeps it).
pub fn data_directory(env: &permark_core::workspace::CoreEnv) -> Option<std::path::PathBuf> {
    cfg!(not(target_os = "macos")).then(|| env.dirs.cache.join("webview"))
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
        (true, _) => (0x14, 0x15, 0x19),
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

/// A window still hidden shows now, focused, its view on screen sized + focused. The registry counts it on screen
/// from now on (minimized later or not).
pub fn show_window(handle: &AppHandle, window: &str) {
    let Some(native) = handle.get_window(window) else {
        return;
    };
    state(handle).registry.window_shown(window);
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

/// `window` is brought forward: still hidden with its view on screen painted already, it shows now (nothing else
/// would show it); still being made, it shows, focused, once that view paints (or after the fallback).
pub fn show_painted(handle: &AppHandle, window: &str) {
    let shell = state(handle);
    let painted = shell
        .registry
        .shown_view(window)
        .is_some_and(|view| lock(&shell.views.inner).ready.contains(&view));
    if painted {
        show_window(handle, window);
    }
}

/// A view that could not go back after a refused move lives in `window` (no tab of it in the registry): the window
/// shows with it, sized + focused, so it never stays hidden holding the view.
pub fn reveal(handle: &AppHandle, window: &tauri::Window, webview: &tauri::Webview) {
    state(handle).registry.window_shown(window.label());
    let _ = window.show();
    let _ = window.set_focus();
    fit_one(window, webview);
    let _ = webview.show();
    let _ = webview.set_focus();
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
        inner.tab_waits.remove(view);
        inner.reloaded.remove(view);
        inner.reloads.remove(view);
    }
    shell.view_gone(view);
}

/// A window is gone: its marks dropped.
pub fn forget_window(handle: &AppHandle, window: &str) {
    let shell = state(handle);
    let mut inner = lock(&shell.views.inner);
    inner.rounds.remove(window);
    inner.hung.remove(window);
}

fn close_window(handle: &AppHandle, window: &str) {
    let shell = state(handle);
    // the last window: the session keeps it (saved now, frozen until exit)
    if shell.registry.is_last_on_screen(window) {
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
        CloseRound::asking(tabs.clone(), view, Instant::now()),
    );
    arm(handle, Awaited::Round(window.to_owned()));
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
        CloseAnswer::Discard => discard(handle, window),
        CloseAnswer::SaveAll => {
            let views: Vec<String> = tabs
                .iter()
                .filter_map(|t| shell.registry.view_of_tab(t.tab))
                .collect();
            // no view holds those tabs any more: nothing to save, and no answer would ever end the round
            if views.is_empty() {
                resolve(handle, window, Resolution::Close);
                return;
            }
            if let Some(round) = lock(&shell.views.inner).rounds.get_mut(window) {
                round.save_all(views.iter().cloned().collect(), Instant::now());
            }
            arm(handle, Awaited::Round(window.to_owned()));
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

/// `window` closes without saving (Discard | Close anyway): nothing written, the unsaved marks of its views let go,
/// then its close round ends as Close, or with no round up the window is asked to close again (nothing unsaved now:
/// it closes).
fn discard(handle: &AppHandle, window: &str) {
    let shell = state(handle);
    for view in views_in(&shell, window) {
        let_go(&shell, &view);
    }
    let in_round = lock(&shell.views.inner).rounds.contains_key(window);
    if in_round {
        resolve(handle, window, Resolution::Close);
    } else if crate::request_close(handle, window) {
        if let Some(native) = handle.get_window(window) {
            let _ = native.destroy();
        }
    }
}

/// The views of `window` now.
fn views_in(shell: &Shell, window: &str) -> Vec<String> {
    shell
        .registry
        .views()
        .into_iter()
        .filter(|view| shell.registry.window_of(view).as_deref() == Some(window))
        .collect()
}

/// The person chose Close anyway on the pages `hung` of `window` that stopped answering ([`on_hung_close`]). A close
/// prompt up in a working view stays: its tabs close once it is answered (the hung tab may stay open, its edits let go).
fn close_hung(handle: &AppHandle, window: &str, hung: &BTreeSet<String>) {
    let shell = state(handle);
    let in_round = lock(&shell.views.inner).rounds.contains_key(window);
    let prompt_in = in_round
        .then(|| shell.registry.shown_view(window))
        .flatten();
    match on_hung_close(&views_in(&shell, window), hung, prompt_in.as_deref()) {
        HungClose::Window => discard(handle, window),
        HungClose::Tabs { views, end_round } => {
            if end_round {
                resolve(handle, window, Resolution::Stay { show: None });
            }
            for view in views {
                close_tab_anyway(handle, &view);
            }
        }
    }
}

/// The view's unsaved mark lets go (its edits are given up).
fn let_go(shell: &Shell, view: &str) {
    if let Some((_, workspace)) = shell.registry.workspace_of(view) {
        workspace.documents().set_unsaved(view, None);
    }
}

/// Asks `view` (holding unsaved edits) about closing its tab: it is to say it heard in time.
pub fn ask_tab_close(handle: &AppHandle, view: &str) {
    lock(&state(handle).views.inner)
        .tab_waits
        .insert(view.to_owned(), Wait::new(Instant::now()));
    arm(handle, Awaited::Tab(view.to_owned()));
    let _ = handle.emit_to(
        view,
        "tab-close-requested",
        ViewRequest {
            window: view.to_owned(),
        },
    );
}

/// `view` received a close | tab-close ask and its prompt is up: the person's answer is not timed.
pub fn heard(handle: &AppHandle, view: &str) {
    let shell = state(handle);
    let mut inner = lock(&shell.views.inner);
    inner.tab_waits.remove(view);
    for round in inner.rounds.values_mut() {
        round.heard(view);
    }
}

/// Checks `awaited` once its deadline passed (a timer thread each: asks are few, a stale check finds nothing due).
fn arm(handle: &AppHandle, awaited: Awaited) {
    let handle = handle.clone();
    std::thread::spawn(move || {
        std::thread::sleep(ROUND_DEADLINE);
        let now = Instant::now();
        let since = lock(&state(&handle).views.inner)
            .wait_of(&awaited)
            .and_then(|wait| wait.ask(now));
        if let Some(since) = since {
            not_responding(&handle, Stuck::Awaited(awaited, since));
        }
    });
}

/// The system dialog "PerMark isn't responding": Close anyway | Wait (also Esc | the dialog's close button). Shown
/// with a callback, never waited for: it may come from any thread, the main one included.
fn not_responding(handle: &AppHandle, stuck: Stuck) {
    let shell = state(handle);
    let window = match &stuck {
        Stuck::Awaited(Awaited::Round(window), _) | Stuck::Hung(window) => Some(window.clone()),
        Stuck::Awaited(Awaited::Tab(view), _) => shell.registry.window_of(view),
    };
    let words = words(handle);
    let mut dialog = handle
        .dialog()
        .message(words.not_responding_detail)
        .title(words.not_responding)
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            words.close_anyway,
            words.wait,
        ));
    if let Some(native) = window.and_then(|window| handle.get_window(&window)) {
        dialog = dialog.parent(&native);
    }
    let handle = handle.clone();
    dialog.show(move |close| settle(&handle, stuck, close));
}

/// The person answered the not-responding dialog: Close anyway (`close`) | Wait. Nothing when what it was about ended
/// meanwhile (the view answered, the round ended, the window went).
fn settle(handle: &AppHandle, stuck: Stuck, close: bool) {
    let now = Instant::now();
    let shell = state(handle);
    match stuck {
        Stuck::Awaited(awaited, since) => {
            let current = lock(&shell.views.inner)
                .wait_of(&awaited)
                .is_some_and(|wait| wait.answered(since, close, now));
            match awaited {
                _ if !current => {}
                awaited if !close => arm(handle, awaited),
                // the window's close was asked: every tab of it goes unsaved (the dialog's text says so)
                Awaited::Round(window) => discard(handle, &window),
                Awaited::Tab(view) => close_tab_anyway(handle, &view),
            }
        }
        Stuck::Hung(window) => {
            let hung = lock(&shell.views.inner).hung.remove(&window);
            if let Some(hung) = hung.filter(|_| close) {
                close_hung(handle, &window, &hung);
            }
        }
    }
}

/// The tab of `view` closes without saving (the person chose Close anyway).
fn close_tab_anyway(handle: &AppHandle, view: &str) {
    let shell = state(handle);
    lock(&shell.views.inner).tab_waits.remove(view);
    let_go(&shell, view);
    if let Some(place) = shell.registry.place_of_view(view) {
        crate::open::run(handle, shell.registry.close_tab(place.tab));
    }
}

/// The page of `view` stopped answering (the system web view says so): the not-responding dialog for its window,
/// once while it is up (more pages of the window hanging meanwhile join it).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn hung(handle: &AppHandle, view: &str) {
    let shell = state(handle);
    let Some(window) = shell.registry.window_of(view) else {
        return;
    };
    let first = {
        let mut inner = lock(&shell.views.inner);
        let views = inner.hung.entry(window.clone()).or_default();
        let first = views.is_empty();
        views.insert(view.to_owned());
        first
    };
    if first {
        not_responding(handle, Stuck::Hung(window));
    }
}

/// The page process of `webview` ended: its unsaved mark lets go (the text went with the page), the page loads again
/// and its next boot says so. `false`: it loaded again less than [`RELOAD_SPACING`] ago (booted since or not),
/// nothing loaded again: a page that cannot stay up must not reload for ever.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn reload(handle: &AppHandle, webview: &tauri::Webview) -> bool {
    let shell = state(handle);
    let view = webview.label();
    if !lock(&shell.views.inner).reload_due(view, Instant::now()) {
        return false;
    }
    let lost = shell
        .registry
        .workspace_of(view)
        .and_then(|(_, workspace)| {
            let documents = workspace.documents();
            let lost = documents.unsaved(view);
            documents.set_unsaved(view, None);
            lost
        });
    lock(&shell.views.inner).loaded_again(view, lost);
    if let Err(error) = webview.reload() {
        eprintln!("PerMark: view not loaded again: {error}");
    }
    true
}

/// The view boots: whether its page was loaded again after its process ended (told once; the reload limit stays).
pub fn take_reloaded(handle: &AppHandle, view: &str) -> Option<Reloaded> {
    lock(&state(handle).views.inner).reloaded.remove(view)
}

/// The frontend's words for the system dialogs (unfit ones are ignored: the earlier words stay).
pub fn set_words(handle: &AppHandle, words: NativeWords) {
    if words.usable() {
        lock(&state(handle).views.inner).words = words;
    }
}

/// The words of the system dialogs now.
pub fn words(handle: &AppHandle) -> NativeWords {
    lock(&state(handle).views.inner).words.clone()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn overdue(round: &CloseRound, now: Instant) -> bool {
        round.wait.is_some_and(|wait| wait.overdue(now))
    }

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn a_close_ask_runs_out_after_the_deadline_while_the_view_has_not_heard() {
        let start = Instant::now();
        let round = CloseRound::asking(Vec::new(), "view-1", start);
        assert!(!overdue(&round, start + secs(9)));
        assert!(overdue(&round, start + ROUND_DEADLINE));
        assert!(overdue(&round, start + secs(60)));
    }

    #[test]
    fn once_the_asked_view_heard_the_persons_answer_is_never_timed() {
        let start = Instant::now();
        let mut round = CloseRound::asking(Vec::new(), "view-1", start);
        round.heard("view-2");
        assert!(
            overdue(&round, start + ROUND_DEADLINE),
            "another view's word"
        );
        round.heard("view-1");
        assert!(!overdue(&round, start + secs(3600)));
    }

    #[test]
    fn save_all_answers_are_timed_from_the_answer() {
        let start = Instant::now();
        let mut round = CloseRound::asking(Vec::new(), "view-1", start);
        round.heard("view-1");
        let saving = start + secs(30);
        round.save_all(["view-1".to_owned(), "view-2".to_owned()].into(), saving);
        assert!(!overdue(&round, saving + secs(9)));
        assert!(overdue(&round, saving + ROUND_DEADLINE));
        // a late "heard" does not stop the clock on the saves
        round.heard("view-1");
        assert!(overdue(&round, saving + ROUND_DEADLINE));
    }

    #[test]
    fn one_dialog_per_wait_and_wait_starts_the_clock_again() {
        let start = Instant::now();
        let mut round = CloseRound::asking(Vec::new(), "view-1", start);
        let due = start + ROUND_DEADLINE;
        let wait = round.wait.as_mut().unwrap();
        assert_eq!(wait.ask(start + secs(5)), None, "not due yet");
        assert_eq!(wait.ask(due), Some(start));
        assert_eq!(wait.ask(due + secs(1)), None, "the dialog is up already");
        let answered = due + secs(20);
        assert!(wait.answered(start, false, answered));
        assert!(!wait.overdue(answered + secs(9)));
        assert!(wait.overdue(answered + ROUND_DEADLINE));
    }

    #[test]
    fn a_stale_dialog_answer_changes_nothing() {
        let start = Instant::now();
        let mut wait = Wait::new(start);
        assert!(!wait.answered(start, true, start + secs(1)), "no dialog up");
        let since = wait.ask(start + ROUND_DEADLINE).unwrap();
        // the person chose Wait in another dialog: a new wait began
        let later = start + secs(40);
        assert!(wait.answered(since, false, later));
        assert!(!wait.answered(since, true, later + secs(1)));
        assert!(!wait.answered(since, false, later + secs(1)));
        assert_eq!(wait, Wait::new(later));
    }

    /// Regression (a window whose view could not be made stayed hidden for good): the window goes unless another of
    /// its views works; the app exits when no window is left on screen.
    #[test]
    fn a_view_that_cannot_be_made_never_leaves_a_hidden_window() {
        assert_eq!(on_create_failed(true, true), CreateFailed::Ignore);
        assert_eq!(on_create_failed(true, false), CreateFailed::Ignore);
        assert_eq!(on_create_failed(false, true), CreateFailed::CloseWindow);
        assert_eq!(on_create_failed(false, false), CreateFailed::Exit);
    }

    fn names(views: &[&str]) -> Vec<String> {
        views.iter().map(|&v| v.to_owned()).collect()
    }

    /// Regression (Close anyway on a hung page dropped the unsaved edits of every tab of its window): only the hung
    /// views let go; another view with unsaved edits keeps them and asks about them itself later.
    #[test]
    fn close_anyway_on_a_hung_page_lets_go_of_that_page_only() {
        let window = names(&["view-1", "view-2"]);
        let hung: BTreeSet<String> = names(&["view-1"]).into_iter().collect();
        assert_eq!(
            on_hung_close(&window, &hung, None),
            HungClose::Tabs {
                views: names(&["view-1"]),
                end_round: false
            }
        );
        // the window's close prompt is up in the hung page: it can never be answered, the round ends first
        assert_eq!(
            on_hung_close(&window, &hung, Some("view-1")),
            HungClose::Tabs {
                views: names(&["view-1"]),
                end_round: true
            }
        );
        // the prompt is in the working view: it stays up
        assert_eq!(
            on_hung_close(&window, &hung, Some("view-2")),
            HungClose::Tabs {
                views: names(&["view-1"]),
                end_round: false
            }
        );
        // no view of the window works: the window closes
        let all: BTreeSet<String> = window.iter().cloned().collect();
        assert_eq!(on_hung_close(&window, &all, None), HungClose::Window);
        // a hung view gone from the window meanwhile: nothing of the window is let go
        let gone: BTreeSet<String> = names(&["view-9"]).into_iter().collect();
        assert_eq!(
            on_hung_close(&window, &gone, None),
            HungClose::Tabs {
                views: Vec::new(),
                end_round: false
            }
        );
    }

    /// Regression (a page that died after each boot loaded again for ever: the boot reset the guard).
    #[test]
    fn a_page_loads_again_at_most_once_a_minute_whatever_its_boots() {
        let start = Instant::now();
        let mut inner = Inner::default();
        assert!(inner.reload_due("view-1", start));
        inner.loaded_again("view-1", Some("a.md".to_owned()));
        assert_eq!(
            inner.reloaded.remove("view-1"),
            Some(Reloaded {
                lost: Some("a.md".to_owned())
            })
        );
        // it booted and ended again: nothing loads, nothing to tell
        assert!(!inner.reload_due("view-1", start + secs(5)));
        assert!(!inner.reloaded.contains_key("view-1"));
        assert!(inner.reload_due("view-2", start + secs(5)), "another view");
        assert!(inner.reload_due("view-1", start + RELOAD_SPACING));
        // ended again before it booted: the file lost first is still told
        inner.loaded_again("view-1", Some("b.md".to_owned()));
        inner.loaded_again("view-1", None);
        assert_eq!(
            inner.reloaded.get("view-1"),
            Some(&Reloaded {
                lost: Some("b.md".to_owned())
            })
        );
    }

    #[test]
    fn dialog_words_must_fit_a_system_dialog() {
        assert!(NativeWords::default().usable());
        let with = |change: fn(&mut NativeWords)| {
            let mut words = NativeWords::default();
            change(&mut words);
            words.usable()
        };
        assert!(with(
            |w| w.not_responding_detail = "Line one\nline two".into()
        ));
        assert!(!with(|w| w.wait = "  ".into()));
        assert!(!with(|w| w.close_anyway = "x".repeat(MAX_WORDS + 1)));
        assert!(!with(|w| w.engine_stopped = "bell \u{7}".into()));
        assert!(!with(|w| w.wait.clone_from(&w.close_anyway)));
        assert!(with(|w| w.window_failed = Some("Closed.".into())));
        assert!(!with(|w| w.window_failed = Some(String::new())));
        assert_eq!(NativeWords::default().window_failed(), WINDOW_FAILED);
        assert!(with(|w| w.view_failed_exit = Some("Closing.".into())));
        assert!(!with(|w| w.view_failed_exit = Some("tab\tstop".into())));
        assert_eq!(NativeWords::default().view_failed_exit(), VIEW_FAILED_EXIT);
        // a start that never showed a window loses no edits: the text says so, unlike the engine's
        assert!(!VIEW_FAILED_EXIT.contains("lost"));
        // Close anyway after a close round ends every tab of the window unsaved: its text says so
        assert!(NativeWords::default()
            .not_responding_detail
            .contains("this window's tabs"));
    }

    /// Regression (move froze the app): every reparent goes through [`super::reparent`] (main thread). The freeze
    /// itself needs the real runtime; this keeps a direct call off another thread from coming back.
    #[test]
    fn webviews_are_reparented_only_on_the_main_thread() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut direct = Vec::new();
        for entry in std::fs::read_dir(&src).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let calls = text.matches(".reparent(").count();
            let allowed = if path.ends_with("views.rs") { 3 } else { 0 };
            if calls > allowed {
                direct.push(path.display().to_string());
            }
        }
        assert!(direct.is_empty(), "direct reparent in {direct:?}");
    }
}
