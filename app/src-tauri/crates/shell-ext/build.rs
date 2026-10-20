// Links the Visual C++ runtime statically (the Universal CRT is part of Windows): the DLL is loaded by a Windows helper
// process on computers that may not have the runtime installed. No-op off Windows.
fn main() {
    static_vcruntime::metabuild();
}
