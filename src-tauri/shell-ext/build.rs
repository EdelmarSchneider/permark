// The DLL is loaded by a Windows helper process on computers that may not have the Visual C++ runtime installed, so it carries
// the runtime part it needs (the Universal CRT is part of Windows). The program does the same: `tauri-build` links its
// runtime statically too.
fn main() {
    static_vcruntime::metabuild();
}
