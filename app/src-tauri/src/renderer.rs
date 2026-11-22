//! A view's page process failing (Windows `WebView2`; the web view library leaves it unhandled there): the page process
//! ended -> the view loads again (its next boot says so); it hangs -> the not-responding dialog; the browser process
//! ended (every view is dead) -> an error, then the app exits with the session kept. Other systems: nothing yet
//! (macOS: the web view library already loads a dead page again).
// WebView2 is a COM API: every call into it is `unsafe` (no safe wrapper exists); each call runs on the UI thread
// inside `with_webview`, on interfaces the web view handed out.
#![allow(unsafe_code)]

use tauri::AppHandle;

/// What a failed web view process asks of the app.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Failure {
    /// The page process ended: load the page again.
    PageEnded,
    /// The page stopped answering.
    PageHangs,
    /// The browser process ended: no view of the app works any more.
    EngineEnded,
}

/// Listens for the page process of `webview` failing.
#[cfg(windows)]
pub fn watch(handle: &AppHandle, webview: &tauri::Webview) {
    let (handle, label) = (handle.clone(), webview.label().to_owned());
    let made = webview.with_webview(move |platform| {
        if let Err(error) = webview2::listen(&platform, handle, label) {
            eprintln!("PerMark: page process failures not watched: {error}");
        }
    });
    if let Err(error) = made {
        eprintln!("PerMark: page process failures not watched: {error}");
    }
}

/// Listens for the page process of `webview` failing (nothing here yet).
#[cfg(not(windows))]
pub fn watch(_handle: &AppHandle, _webview: &tauri::Webview) {}

/// Runs off the UI thread (the event comes on it): registry locks, a reload, a dialog.
#[cfg_attr(not(windows), allow(dead_code))]
fn failed(handle: &AppHandle, view: &str, failure: Failure) {
    use tauri::Manager;
    match failure {
        Failure::PageEnded => {
            let Some(webview) = handle.get_webview(view) else {
                return;
            };
            // it ended again before its page booted: no endless reloads, the person may close the window
            if !crate::views::reload(handle, &webview) {
                crate::views::hung(handle, view);
            }
        }
        Failure::PageHangs => crate::views::hung(handle, view),
        // during the session's restore the registry holds part of it: the session stays as last saved
        Failure::EngineEnded => crate::views::fatal(
            handle,
            crate::shell::state(handle).restored(),
            crate::views::words(handle).engine_stopped,
        ),
    }
}

#[cfg(windows)]
mod webview2 {
    use tauri::webview::PlatformWebview;
    use tauri::AppHandle;
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_PROCESS_FAILED_KIND, COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED,
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED,
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE,
    };
    use webview2_com::ProcessFailedEventHandler;

    use super::{failed, Failure};

    /// The app's answer to a failed process kind; `None`: the web view recovers by itself (GPU, utility processes)
    /// or it is no page of the app's (a frame).
    pub(super) fn failure_of(kind: COREWEBVIEW2_PROCESS_FAILED_KIND) -> Option<Failure> {
        match kind {
            COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED => Some(Failure::PageEnded),
            COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE => {
                Some(Failure::PageHangs)
            }
            COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED => Some(Failure::EngineEnded),
            _ => None,
        }
    }

    /// Adds the `ProcessFailed` handler to the view's `CoreWebView2` (it lives as long as the view: never removed).
    pub(super) fn listen(
        platform: &PlatformWebview,
        handle: AppHandle,
        label: String,
    ) -> windows::core::Result<()> {
        let handler = ProcessFailedEventHandler::create(Box::new(move |_sender, args| {
            let Some(args) = args else {
                return Ok(());
            };
            let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
            // SAFETY: `args` is the live event-args interface WebView2 passed to this handler, called on the UI thread;
            // `kind` is a valid out pointer for the call.
            unsafe { args.ProcessFailedKind(&raw mut kind)? };
            if let Some(failure) = failure_of(kind) {
                let (handle, label) = (handle.clone(), label.clone());
                std::thread::spawn(move || failed(&handle, &label, failure));
            }
            Ok(())
        }));
        let mut token = 0_i64;
        // SAFETY: the controller is the view's own, handed out by `with_webview` on the UI thread; the handler is a COM
        // object WebView2 keeps a reference to; `token` is a valid out pointer.
        unsafe {
            platform
                .controller()
                .CoreWebView2()?
                .add_ProcessFailed(&handler, &raw mut token)
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED,
        COREWEBVIEW2_PROCESS_FAILED_KIND_FRAME_RENDER_PROCESS_EXITED,
        COREWEBVIEW2_PROCESS_FAILED_KIND_GPU_PROCESS_EXITED,
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED,
        COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE,
    };

    use super::webview2::failure_of;
    use super::Failure;

    #[test]
    fn only_page_and_browser_failures_are_the_apps_to_handle() {
        assert_eq!(
            failure_of(COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED),
            Some(Failure::PageEnded)
        );
        assert_eq!(
            failure_of(COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_UNRESPONSIVE),
            Some(Failure::PageHangs)
        );
        assert_eq!(
            failure_of(COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED),
            Some(Failure::EngineEnded)
        );
        assert_eq!(
            failure_of(COREWEBVIEW2_PROCESS_FAILED_KIND_GPU_PROCESS_EXITED),
            None
        );
        assert_eq!(
            failure_of(COREWEBVIEW2_PROCESS_FAILED_KIND_FRAME_RENDER_PROCESS_EXITED),
            None
        );
    }
}
