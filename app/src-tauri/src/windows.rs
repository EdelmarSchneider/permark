//! Window system glue: create windows, put them where they were, read their geometry, focus them.

use permark_core::session::{cascade, place, to_save, Geometry, Limits, Rect, Screen};
use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

use crate::shell::{state, Shell};

pub const MAIN: &str = "main";
/// Smallest content area (logical px).
const MIN_SIZE: (f64, f64) = (900.0, 600.0);

/// A hidden window (its frontend shows it after the first paint), in the registry; its first normal geometry is the
/// one it got. Its web view keeps its data in PerMark's own cache folder (Windows, Linux; debug builds in the
/// `PerMark-dev` one), never in a folder named after the app identifier: a development build and an installed
/// PerMark never share it.
fn build(handle: &AppHandle, label: &str) -> tauri::Result<WebviewWindow> {
    let builder = WebviewWindowBuilder::new(handle, label, WebviewUrl::App("index.html".into()))
        .title("PerMark")
        .inner_size(1280.0, 800.0)
        .min_inner_size(MIN_SIZE.0, MIN_SIZE.1)
        .visible(false);
    #[cfg(not(target_os = "macos"))]
    let builder = builder.data_directory(state(handle).env.dirs.cache.join("webview"));
    let window = builder.build()?;
    let shell = state(handle);
    shell.registry.add_window(label);
    if let Some(geometry) = current(&window) {
        shell.remember_normal(label, geometry);
    }
    Ok(window)
}

/// The first window (`main`).
///
/// # Errors
/// Window creation failure.
pub fn create_main(handle: &AppHandle) -> tauri::Result<()> {
    build(handle, MAIN).map(|_| ())
}

/// New hidden window (shown by its frontend after first paint, like `main`), cascaded from the focused | newest one.
///
/// # Errors
/// Window creation failure.
pub fn create(handle: &AppHandle) -> tauri::Result<WebviewWindow> {
    create_at(handle, None)
}

/// New hidden window at `saved` (a session window), else cascaded from the focused | newest window (+30, +30 logical,
/// same size, its normal rect when maximized); no source geometry -> the default size at the system's spot.
///
/// # Errors
/// Window creation failure.
pub fn create_at(handle: &AppHandle, saved: Option<&Geometry>) -> tauri::Result<WebviewWindow> {
    let shell = state(handle);
    // read before the new window exists (it may take the focus)
    let source = saved.copied().or_else(|| cascade_source(handle, &shell));
    let window = build(handle, &shell.new_label())?;
    match (saved, source) {
        (Some(saved), _) => restore(&window, saved),
        (None, Some(source)) => restore(
            &window,
            &cascade(&source, &screens(&window), limits(&window)),
        ),
        (None, None) => {}
    }
    Ok(window)
}

/// Geometry of the window a new one cascades from: the focused one, else the newest.
fn cascade_source(handle: &AppHandle, shell: &Shell) -> Option<Geometry> {
    let focused = handle
        .webview_windows()
        .into_values()
        .find(|w| w.is_focused().unwrap_or(false))
        .map(|w| w.label().to_owned());
    let label = focused.or_else(|| {
        shell
            .registry
            .session_windows()
            .last()
            .map(|(label, _)| label.clone())
    })?;
    geometry(handle, &label, shell)
}

/// Screens as `place` sees them.
fn screens(window: &WebviewWindow) -> Vec<Screen> {
    let primary = window
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| *m.position());
    window
        .available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|m| {
            let work = m.work_area();
            Screen {
                work: Rect {
                    x: work.position.x,
                    y: work.position.y,
                    width: work.size.width,
                    height: work.size.height,
                },
                scale: m.scale_factor(),
                primary: primary == Some(*m.position()),
            }
        })
        .collect()
}

/// Window system demands of `window`: min content size, what its frame + title bar add (logical px).
fn limits(window: &WebviewWindow) -> Limits {
    let scale = window.scale_factor().unwrap_or(1.0);
    let frame = match (window.outer_size(), window.inner_size()) {
        (Ok(outer), Ok(inner)) => (
            f64::from(outer.width.saturating_sub(inner.width)) / scale,
            f64::from(outer.height.saturating_sub(inner.height)) / scale,
        ),
        _ => (0.0, 0.0),
    };
    Limits {
        min: MIN_SIZE,
        frame,
    }
}

/// Puts a window where `saved` says, clamped onto today's screens (off every screen -> centered on the primary).
/// The placed rect is its normal geometry from now on: a maximized window's own Moved | Resized events arrive after
/// `maximize` and are skipped, so without this it would save no geometry.
pub fn restore(window: &WebviewWindow, saved: &Geometry) {
    let Some(saved) = saved.sane() else {
        return;
    };
    let Some(placement) = place(&saved, &screens(window), limits(window)) else {
        return;
    };
    let _ = window.set_size(PhysicalSize::new(placement.width, placement.height));
    let _ = window.set_position(PhysicalPosition::new(placement.x, placement.y));
    state(window.app_handle()).remember_normal(window.label(), placement.normal());
    if placement.maximized {
        let _ = window.maximize();
    }
}

fn current(window: &WebviewWindow) -> Option<Geometry> {
    let position = window.outer_position().ok()?;
    let size = window.inner_size().ok()?;
    Some(Geometry {
        x: position.x,
        y: position.y,
        width: size.width,
        height: size.height,
        scale: window.scale_factor().unwrap_or(1.0),
        maximized: false,
    })
}

/// Geometry to save: maximized | minimized windows keep their last normal size + position (see [`to_save`]).
pub fn geometry(handle: &AppHandle, label: &str, shell: &Shell) -> Option<Geometry> {
    let window = handle.get_webview_window(label)?;
    to_save(
        window.is_maximized().unwrap_or(false),
        window.is_minimized().unwrap_or(false),
        shell.normal_geometry(label),
        current(&window),
    )
}

/// Moved | resized: remember the normal geometry (never a maximized | minimized one).
pub fn remember_geometry(handle: &AppHandle, label: &str) {
    let Some(window) = handle.get_webview_window(label) else {
        return;
    };
    if window.is_maximized().unwrap_or(false) || window.is_minimized().unwrap_or(false) {
        return;
    }
    if let Some(geometry) = current(&window) {
        state(handle).remember_normal(label, geometry);
    }
}

/// Brings a window forward.
pub fn focus(handle: &AppHandle, label: &str) {
    if let Some(window) = handle.get_webview_window(label) {
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
