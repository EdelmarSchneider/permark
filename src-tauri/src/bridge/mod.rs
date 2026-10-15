//! The agent connection: what an AI agent (or any program that speaks the Model Context Protocol) can learn about the
//! open project and do in it. Plain Rust with no Tauri dependency, so it runs without a window.
//!
//! [`mcp`] is the server behind `trailmark mcp`: it speaks MCP over the program's standard input and output.

use std::ffi::OsString;

pub(crate) mod mcp;

/// True when the program was started as `trailmark mcp` (the first argument after the program name).
pub(crate) fn wants_mcp(args: impl IntoIterator<Item = OsString>) -> bool {
    args.into_iter().nth(1).is_some_and(|arg| arg == "mcp")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn mcp_is_the_first_argument() {
        assert!(wants_mcp(args(&["trailmark", "mcp"])));
        assert!(wants_mcp(args(&["trailmark", "mcp", "--extra"])));
    }

    #[test]
    fn other_invocations_open_the_window() {
        assert!(!wants_mcp(args(&["trailmark"])));
        assert!(!wants_mcp(args(&[])));
        assert!(!wants_mcp(args(&["trailmark", "notes.md"])));
        assert!(!wants_mcp(args(&["trailmark", "notes.md", "mcp"])));
        assert!(!wants_mcp(args(&["mcp"])));
    }
}
