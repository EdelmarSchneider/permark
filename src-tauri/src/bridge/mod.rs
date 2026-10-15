//! The agent connection: what an AI agent (or any program that speaks the Model Context Protocol) can learn about the
//! open project and do in it. Plain Rust with no Tauri dependency, so it runs without a window.
//!
//! ```text
//!  agent (any MCP client) --stdio--> trailmark mcp --local socket--> TrailMark window of that project
//!                                         |
//!                                         +-- reads the project's files when the window is closed
//! ```
//!
//! - [`context`] is what the person is looking at; [`protocol`] the lines on the socket; [`socket`] where the socket
//!   is and who may use it.
//! - The window's side: [`hub`] keeps the state and answers, [`server`] listens, [`file`] keeps `agent.json`, and
//!   [`service`] ties them to the switch in the settings.
//! - The agent's side: [`link`] connects, [`tools`] is what each tool does, [`mcp`] speaks MCP, and [`cli`] runs
//!   `trailmark mcp` and `trailmark context`. [`project`] reads a project from its files, [`answers`] builds the
//!   answers both sides share.

use std::ffi::OsString;

pub(crate) mod answers;
pub(crate) mod cli;
pub(crate) mod context;
pub(crate) mod file;
pub(crate) mod hub;
pub(crate) mod link;
pub(crate) mod mcp;
pub(crate) mod project;
pub(crate) mod protocol;
pub(crate) mod server;
pub(crate) mod service;
pub(crate) mod socket;
pub(crate) mod tools;

/// The commands the program runs without a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Subcommand {
    /// `trailmark mcp`: serve MCP over standard input and output.
    Mcp,
    /// `trailmark context`: print what the person is looking at and exit.
    Context,
}

/// The command the program was started as: its first argument after the program name, if it is one of ours.
pub(crate) fn subcommand(args: impl IntoIterator<Item = OsString>) -> Option<Subcommand> {
    let arg = args.into_iter().nth(1)?;
    match arg.to_str()? {
        "mcp" => Some(Subcommand::Mcp),
        "context" => Some(Subcommand::Context),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn a_command_is_the_first_argument() {
        assert_eq!(
            subcommand(args(&["trailmark", "mcp"])),
            Some(Subcommand::Mcp)
        );
        assert_eq!(
            subcommand(args(&["trailmark", "mcp", "--extra"])),
            Some(Subcommand::Mcp)
        );
        assert_eq!(
            subcommand(args(&["trailmark", "context"])),
            Some(Subcommand::Context)
        );
    }

    #[test]
    fn other_invocations_open_the_window() {
        assert_eq!(subcommand(args(&["trailmark"])), None);
        assert_eq!(subcommand(args(&[])), None);
        assert_eq!(subcommand(args(&["trailmark", "notes.md"])), None);
        assert_eq!(subcommand(args(&["trailmark", "notes.md", "mcp"])), None);
        assert_eq!(subcommand(args(&["mcp"])), None);
        // Only the exact word: a folder is opened by its path.
        assert_eq!(subcommand(args(&["trailmark", "./context"])), None);
        assert_eq!(subcommand(args(&["trailmark", "MCP"])), None);
    }
}
