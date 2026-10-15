//! Notes: a place for writing that does not belong to a project yet. Open TrailMark, type, close: nothing is lost.
//!
//! A note is an ordinary Markdown file in a folder the person can see (see [`folder`]). There is no name to choose and
//! no "save changes?": the text is written as it is typed, earlier versions are kept, and a deleted note goes to the
//! system's recycle bin. TrailMark's own data sits next to the notes in `.trailmark/`, can be deleted at any time and is
//! rebuilt from the files; nothing is ever added inside a note.
//!
//! The modules are plain Rust with no window in them:
//!
//! - [`folder`] decides where the notes are and whether the folder is there;
//! - [`text`] and [`names`] make titles from the first line and names for the files;
//! - [`data`] is `notes.json`: pins, the open note, where each note was left;
//! - [`history`] keeps the earlier versions; [`lock`] keeps two programs from writing the same notes; [`recover`] knows
//!   the temporary files a crash leaves behind;
//! - [`service`] ties them together for one folder.

mod data;
pub(crate) mod folder;
mod history;
mod launch;
mod lock;
mod loose;
mod names;
mod pending;
mod recover;
mod service;
mod text;

pub use data::NoteView;
pub use folder::{FolderStatus, NotesFolder};
pub(crate) use launch::{
    claim, notes_launch, serve, show_or_start, Claim, NotesRequest, NOTES_FLAG,
};
pub(crate) use loose::{is_loose_path, ui_path as loose_ui_path};
pub(crate) use names::Moment;
pub use pending::PendingEdits;
pub(crate) use recover::process_is_running;
pub use service::{
    summary, LooseDoc, LooseStatus, NoteDeleted, NoteList, NoteSaved, NoteVersion, NotesError,
    NotesService, NotesSummary, OpenReport, OpenedFile, ReadOnlyReason, Restored,
};
