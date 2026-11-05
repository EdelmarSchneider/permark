//! Worker binary (feature `llama`, Unix): loads its llama/ggml shared libs from its own folder (flat worker unit,
//! D-6). Linux: `DT_RPATH` (not `RUNPATH`) so the libs' own dependencies resolve there too. Windows: DLLs next to the
//! exe load first anyway. Nothing for the default build.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var_os("CARGO_FEATURE_LLAMA").is_none() {
        return;
    }
    let arg = match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("linux") => "-Wl,--disable-new-dtags,-rpath,$ORIGIN",
        Ok("macos") => "-Wl,-rpath,@executable_path",
        _ => return,
    };
    println!("cargo:rustc-link-arg-bin=permark-llm-worker={arg}");
}
