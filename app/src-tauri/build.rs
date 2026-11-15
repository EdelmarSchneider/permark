fn main() {
    tauri_build::build();
    // Tests that open a real system web view (feature `webview-tests`): on Windows the test binary needs the Common
    // Controls v6 dependency the app's manifest carries (the runtime links `TaskDialogIndirect`), else it cannot load.
    // Only with `--lib`: the app binary already embeds the full manifest.
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if msvc && std::env::var_os("CARGO_FEATURE_WEBVIEW_TESTS").is_some() {
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' \
             version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );
    }
}
