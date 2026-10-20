//! Navigation guard: a window never leaves the app page (a stray link would replace the app and lose unsaved work).

use tauri::plugin::{Builder, TauriPlugin};
use tauri::{Url, Wry};

/// The app's own page: bundled UI (`tauri://localhost`, `http(s)://tauri.localhost`) or the dev server (debug).
pub fn is_app_url(url: &Url, dev_url: Option<&Url>) -> bool {
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

pub fn guard(dev_url: Option<Url>) -> TauriPlugin<Wry> {
    Builder::new("navigation-guard")
        .on_navigation(move |_webview, url| is_app_url(url, dev_url.as_ref()))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    #[test]
    fn the_bundled_ui_is_the_app() {
        for text in [
            "tauri://localhost/",
            "tauri://localhost/index.html#window=w-2",
            "http://tauri.localhost/",
            "https://tauri.localhost/#x",
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
    fn the_dev_server_counts_only_when_given_and_by_origin() {
        let dev = url("http://localhost:1420/");
        assert!(is_app_url(
            &url("http://localhost:1420/src/main.ts"),
            Some(&dev)
        ));
        assert!(!is_app_url(&url("http://localhost:1421/"), Some(&dev)));
        assert!(!is_app_url(&url("http://localhost:1420/"), None));
    }
}
