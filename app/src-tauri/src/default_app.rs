//! The default app, by the person's choice: what the settings show, the first-run question, setting it where the
//! system allows (macOS, an installed Linux package) and the hand-off to Windows' Default Apps page.

use permark_core::conditions::Distribution;
use permark_core::desktop::{
    is_ours, should_ask, DefaultAppMode, DefaultAppState, DefaultAppView, FileGroup, GroupView,
};
use permark_core::state::{AppDirs, StateFile, StateStore};
use permark_core::{ErrorKind, IpcError};
use tauri::AppHandle;

/// Windows' Default Apps page for PerMark (the installer registers the name under `RegisteredApplications`).
#[cfg(windows)]
const DEFAULT_APPS_DIRECT: &str = "ms-settings:defaultapps?registeredAppUser=PerMark";
/// The Store package's own page needs its app id; the general page lists it too.
#[cfg(windows)]
const DEFAULT_APPS_STORE: &str = "ms-settings:defaultapps";

fn state_file(dirs: &AppDirs) -> StateFile<DefaultAppState> {
    StateStore::app(dirs).file("default-app.json")
}

fn load(dirs: &AppDirs) -> DefaultAppState {
    state_file(dirs).load().into_value()
}

fn save(dirs: &AppDirs, state: &DefaultAppState) -> Result<(), IpcError> {
    state_file(dirs).save(state).map_err(|e| {
        IpcError::new(ErrorKind::Io)
            .subject("default-app.json")
            .detail(e.to_string())
    })
}

/// The older PerMark's data folder (identifier `dev.permark.app`): the person was asked by it before.
fn older_data(dirs: &AppDirs) -> bool {
    dirs.profile
        .parent()
        .is_some_and(|data| data.join("dev.permark.app").is_dir())
}

/// Where PerMark sets the default itself (`Option` for the signature every platform shares).
#[cfg(target_os = "macos")]
#[allow(clippy::unnecessary_wraps)]
fn handlers(handle: &AppHandle) -> Option<Box<dyn permark_core::desktop::Handlers>> {
    Some(Box::new(permark_core::desktop::macos::LaunchServices::new(
        handle.config().identifier.clone(),
    )))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn handlers(_handle: &AppHandle) -> Option<Box<dyn permark_core::desktop::Handlers>> {
    use permark_core::desktop::linux::{install, mimeapps_path, Install, MimeApps, DESKTOP_ID};
    let var = |name: &str| std::env::var(name).ok();
    if install(var, std::path::Path::is_file) != Install::Desktop {
        return None;
    }
    mimeapps_path(var).map(|path| Box::new(MimeApps::new(path, DESKTOP_ID)) as Box<_>)
}

#[cfg(windows)]
#[allow(clippy::unnecessary_wraps)]
fn handlers(_handle: &AppHandle) -> Option<Box<dyn permark_core::desktop::Handlers>> {
    None
}

fn mode() -> DefaultAppMode {
    use permark_core::desktop::linux::{install, Install};
    use permark_core::desktop::Unsupported;
    if cfg!(windows) {
        return DefaultAppMode::Handoff;
    }
    if cfg!(target_os = "macos") {
        return DefaultAppMode::Direct;
    }
    match install(|name| std::env::var(name).ok(), std::path::Path::is_file) {
        Install::Desktop => DefaultAppMode::Direct,
        Install::Snap => DefaultAppMode::Unsupported {
            reason: Unsupported::Confined,
        },
        Install::Loose => DefaultAppMode::Unsupported {
            reason: Unsupported::NotInstalled,
        },
    }
}

fn ours(handle: &AppHandle, group: FileGroup) -> bool {
    #[cfg(windows)]
    {
        let _ = handle;
        permark_core::desktop::windows::group_is_ours(group)
    }
    #[cfg(not(windows))]
    {
        handlers(handle).is_some_and(|h| is_ours(h.as_ref(), group))
    }
}

/// What the settings show, and whether to ask now.
pub fn view(handle: &AppHandle, dirs: &AppDirs, distribution: Distribution) -> DefaultAppView {
    let mode = mode();
    let state = load(dirs);
    // the Windows installer asked already; a Store package did not
    let installer_asked = cfg!(windows) && distribution == Distribution::Direct;
    let ask = !installer_asked && should_ask(&state, older_data(dirs), &mode);
    let groups = FileGroup::ALL
        .iter()
        .map(|group| GroupView {
            group: *group,
            extensions: group.extensions().iter().map(|e| (*e).to_owned()).collect(),
            ours: !matches!(mode, DefaultAppMode::Unsupported { .. }) && ours(handle, *group),
        })
        .collect();
    DefaultAppView { mode, groups, ask }
}

/// Makes PerMark the default of `group` (`on`), or gives it back; the question counts as answered.
///
/// # Errors
/// This copy cannot set it (`Unsupported`) | the system refused.
pub fn set(
    handle: &AppHandle,
    dirs: &AppDirs,
    distribution: Distribution,
    group: FileGroup,
    on: bool,
) -> Result<DefaultAppView, IpcError> {
    let Some(handlers) = handlers(handle) else {
        return Err(IpcError::new(ErrorKind::Unsupported).subject("defaultApp"));
    };
    let mut state = load(dirs);
    state.asked = true;
    let result = is_ours(handlers.as_ref(), group) != on;
    let outcome = if result {
        permark_core::desktop::choose(handlers.as_ref(), &mut state, group, on)
    } else {
        Ok(())
    };
    // what was recorded is kept even when a later type failed: a later "off" can still give it back
    save(dirs, &state)?;
    outcome?;
    Ok(view(handle, dirs, distribution))
}

/// The first-run question was answered (or put off): it is not asked again.
///
/// # Errors
/// The state file cannot be written.
pub fn asked(dirs: &AppDirs) -> Result<(), IpcError> {
    let mut state = load(dirs);
    if state.asked {
        return Ok(());
    }
    state.asked = true;
    save(dirs, &state)
}

/// Opens Windows' Default Apps page (a fixed system URI; nothing from a document).
///
/// # Errors
/// Not Windows (`Unsupported`) | the page did not open.
pub fn handoff(
    handle: &AppHandle,
    dirs: &AppDirs,
    distribution: Distribution,
) -> Result<(), IpcError> {
    #[cfg(windows)]
    {
        use tauri_plugin_opener::OpenerExt;
        asked(dirs)?;
        let uri = if distribution == Distribution::Store {
            DEFAULT_APPS_STORE
        } else {
            DEFAULT_APPS_DIRECT
        };
        handle.opener().open_url(uri, None::<&str>).map_err(|e| {
            IpcError::new(ErrorKind::Io)
                .subject(uri)
                .detail(e.to_string())
        })
    }
    #[cfg(not(windows))]
    {
        let _ = (handle, dirs, distribution);
        Err(IpcError::new(ErrorKind::Unsupported).subject("defaultApp"))
    }
}
