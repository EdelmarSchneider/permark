//! Where the app's window may navigate.
//!
//! The app is a single page. Everything a document links to is handled by the UI (another document opens inside the app, a web
//! address opens in the system browser), so the webview itself never has to leave the page. Anything that would take it
//! elsewhere, such as a plain link in markup the UI did not intercept, would replace the whole app, and with it every unsaved
//! edit. [`is_app_url`] is the answer to "may the window go there?", and the window refuses everything else.

use tauri::Url;

/// Whether `url` is the app's own page: where the bundled UI is served from, or the development server in a debug build.
///
/// The bundled UI is `tauri://localhost` on macOS and Linux and `http(s)://tauri.localhost` on Windows. `dev_url` is the
/// development server (`None` in a release build), compared by origin so that its port counts.
pub(crate) fn is_app_url(url: &Url, dev_url: Option<&Url>) -> bool {
    match url.scheme() {
        "tauri" => url.host_str() == Some("localhost"),
        "http" | "https" => {
            url.host_str() == Some("tauri.localhost")
                || dev_url.is_some_and(|dev| dev.origin() == url.origin())
        }
        "about" => url.as_str() == "about:blank",
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(text: &str) -> Url {
        Url::parse(text).expect("a valid URL")
    }

    #[test]
    fn the_bundled_ui_is_the_app() {
        for text in [
            "tauri://localhost/",
            "tauri://localhost/index.html",
            "http://tauri.localhost/",
            "https://tauri.localhost/#section",
        ] {
            assert!(is_app_url(&url(text), None), "{text}");
        }
    }

    #[test]
    fn nothing_else_is_the_app() {
        for text in [
            "https://example.com/",
            "http://example.com/tauri.localhost",
            "http://tauri.localhost.example.com/",
            "http://tauri.localhost@example.com/",
            "tauri://example/",
            "file:///C:/Windows/win.ini",
            "javascript:alert(1)",
            "data:text/html,<p>hi</p>",
            "blob:http://tauri.localhost/1234",
            "about:srcdoc",
            "ftp://tauri.localhost/",
        ] {
            assert!(!is_app_url(&url(text), None), "{text}");
        }
    }

    #[test]
    fn the_development_server_is_the_app_only_when_it_is_given() {
        let dev = url("http://localhost:1420/");
        assert!(is_app_url(
            &url("http://localhost:1420/index.html"),
            Some(&dev)
        ));
        assert!(is_app_url(&url("http://localhost:1420/#x"), Some(&dev)));
        // another port, another host, another scheme
        assert!(!is_app_url(&url("http://localhost:9999/"), Some(&dev)));
        assert!(!is_app_url(&url("http://127.0.0.1:1420/"), Some(&dev)));
        assert!(!is_app_url(&url("https://localhost:1420/"), Some(&dev)));
        // a release build has no development server
        assert!(!is_app_url(&url("http://localhost:1420/"), None));
    }

    #[test]
    fn a_blank_page_is_allowed() {
        assert!(is_app_url(&url("about:blank"), None));
    }
}
