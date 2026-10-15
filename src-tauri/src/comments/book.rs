//! [`CommentBook`]: the comments of one project. It reads the documents the comments are on to say where each passage is now,
//! and keeps the file (see [`super::store`]).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;

use crate::render::markdown::is_markdown_path;
use crate::render::{decode_text, MAX_RENDER_BYTES};
use crate::util::{hash_hex, now_ms, on_parser_stack, read_with_retry};
use crate::vault::Vault;

use super::anchor::{Analysis, Placement, Probe, Resolution};
use super::store::CommentStore;
use super::{
    clip, Anchor, Comment, CommentError, CommentView, Done, DoneBy, FileProblem, PassageState,
    MAX_COMMENTS, MAX_COMMENT_CHARS, MAX_QUOTE_CHARS, MAX_REPLY_CHARS,
};

/// Called after a change that someone watching the comments should see.
pub type Listener = Arc<dyn Fn() + Send + Sync>;

/// What the window sends when the person writes a comment on a selection.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewComment {
    pub path: String,
    /// The selected text.
    pub quote: String,
    /// What the person wrote.
    pub text: String,
    /// Where the window saw the selection (the headings above it and its lines). The document decides when it can;
    /// these are the fallback.
    #[serde(default)]
    pub heading_path: Vec<String>,
    #[serde(default)]
    pub line: u32,
    #[serde(default)]
    pub end_line: u32,
}

/// A document as far as a comment is concerned.
enum Doc {
    /// There is no such file (any more).
    Gone,
    /// The file is there but cannot be read as text (too big, binary, locked).
    Unreadable,
    Read(Analysis),
}

pub(crate) struct CommentBook {
    vault: Arc<Vault>,
    store: CommentStore,
    listener: Option<Listener>,
}

/// `path` after `from` was renamed to `to` (a folder, with `is_dir`), if the rename touched it.
fn moved(path: &str, from: &str, to: &str, is_dir: bool) -> Option<String> {
    if path == from {
        return Some(to.to_owned());
    }
    if is_dir {
        let rest = path.strip_prefix(from)?.strip_prefix('/')?;
        return Some(format!("{to}/{rest}"));
    }
    None
}

impl CommentBook {
    pub(crate) fn new(vault: Arc<Vault>, file: PathBuf) -> Self {
        Self {
            vault,
            store: CommentStore::new(file),
            listener: None,
        }
    }

    /// Calls `listener` after every change made through this `CommentBook`.
    #[must_use]
    pub(crate) fn with_listener(mut self, listener: Listener) -> Self {
        self.listener = Some(listener);
        self
    }

    /// The project keeps its state somewhere else from now on.
    pub(crate) fn relocate(&self, file: PathBuf) {
        self.store.relocate(file);
    }

    /// Why the list is empty when the file is the reason (see [`FileProblem`]); `None` when it is fine.
    pub(crate) fn problem(&self) -> Option<FileProblem> {
        self.store.problem()
    }

    fn changed(&self) {
        if let Some(listener) = &self.listener {
            listener();
        }
    }

    // ---- reading the documents ------------------------------------------------------------------------

    fn read_doc(&self, path: &str) -> Doc {
        let Ok(resolved) = self.vault.resolve(path) else {
            return Doc::Gone;
        };
        let meta = match std::fs::metadata(&resolved.abs) {
            Ok(meta) if meta.is_file() => meta,
            Ok(_) => return Doc::Gone,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Doc::Gone,
            Err(_) => return Doc::Unreadable,
        };
        if meta.len() > MAX_RENDER_BYTES || !is_markdown_path(&resolved.abs) {
            return Doc::Unreadable;
        }
        let Some(text) = read_with_retry(&resolved.abs)
            .ok()
            .and_then(|bytes| decode_text(&bytes))
        else {
            return Doc::Unreadable;
        };
        Doc::Read(on_parser_stack(|| Analysis::of(&text)))
    }

    /// The comment with its place worked out, and the place when the quote was found.
    fn view(comment: &Comment, doc: &Doc) -> (CommentView, Option<Placement>) {
        let stored = &comment.anchor;
        let (state, line, end_line, heading_path, placement) = match doc {
            Doc::Gone => (
                PassageState::DocumentGone,
                stored.line,
                stored.end_line,
                stored.heading_path.clone(),
                None,
            ),
            Doc::Unreadable => (
                PassageState::PassageChanged,
                stored.line,
                stored.end_line,
                stored.heading_path.clone(),
                None,
            ),
            Doc::Read(analysis) => match analysis.resolve(stored) {
                Resolution::Found(place) => (
                    PassageState::Anchored,
                    place.start_line,
                    place.end_line,
                    place.heading_path.clone(),
                    Some(place),
                ),
                Resolution::Changed { line } => (
                    PassageState::PassageChanged,
                    line,
                    line,
                    stored.heading_path.clone(),
                    None,
                ),
            },
        };
        let view = CommentView {
            id: comment.id.clone(),
            path: comment.path.clone(),
            text: comment.text.clone(),
            quote: stored.quote.clone(),
            heading_path,
            line,
            end_line,
            created_ms: comment.created_ms,
            state,
            done: comment.done.is_some(),
            done_ms: comment.done.as_ref().map(|done| done.at_ms),
            done_by: comment.done.as_ref().map(|done| done.by),
            reply: comment.done.as_ref().and_then(|done| done.reply.clone()),
        };
        (view, placement)
    }

    // ---- listing ----------------------------------------------------------------------------------------

    /// Every comment, by document and line. `keep_places` writes down the places that moved, so a later edit that
    /// removes the passage still finds its way back to roughly the same section.
    pub(crate) fn list(&self, keep_places: bool) -> Vec<CommentView> {
        let comments = self.store.load();
        let mut docs: HashMap<&str, Doc> = HashMap::new();
        let mut views = Vec::with_capacity(comments.len());
        let mut moved_places: Vec<(&str, Placement)> = Vec::new();
        for comment in &comments {
            let doc = docs
                .entry(comment.path.as_str())
                .or_insert_with(|| self.read_doc(&comment.path));
            let (view, placement) = Self::view(comment, doc);
            if let Some(place) = placement {
                if keep_places && differs(&comment.anchor, &place) {
                    moved_places.push((comment.id.as_str(), place));
                }
            }
            views.push(view);
        }
        if !moved_places.is_empty() {
            // a comment that cannot be written down is still shown where it is
            let _ = self.store.update(|stored| {
                let mut changed = false;
                for (id, place) in &moved_places {
                    if let Some(comment) = stored.iter_mut().find(|comment| comment.id == *id) {
                        if differs(&comment.anchor, place) {
                            apply(&mut comment.anchor, place);
                            changed = true;
                        }
                    }
                }
                Ok(((), changed))
            });
        }
        views.sort_by(|a, b| {
            (&a.path, a.line, a.created_ms, &a.id).cmp(&(&b.path, b.line, b.created_ms, &b.id))
        });
        views
    }

    /// One comment as it is now.
    fn view_of(&self, id: &str) -> Result<CommentView, CommentError> {
        let comment = self
            .store
            .load()
            .into_iter()
            .find(|comment| comment.id == id)
            .ok_or_else(|| CommentError::NotFound(id.to_owned()))?;
        let doc = self.read_doc(&comment.path);
        Ok(Self::view(&comment, &doc).0)
    }

    // ---- changing ---------------------------------------------------------------------------------------

    /// Writes a comment on a passage of a document.
    pub(crate) fn add(&self, new: NewComment) -> Result<CommentView, CommentError> {
        if self.vault.is_shallow() {
            return Err(CommentError::SingleFile);
        }
        let quote = clip(new.quote.trim(), MAX_QUOTE_CHARS);
        let text = clip(new.text.trim(), MAX_COMMENT_CHARS);
        if quote.is_empty() || text.is_empty() {
            return Err(CommentError::Empty);
        }
        let resolved = self
            .vault
            .resolve(&new.path)
            .map_err(|_| CommentError::NotADocument(new.path.clone()))?;
        if !resolved.abs.is_file() || !is_markdown_path(&resolved.abs) {
            return Err(CommentError::NotADocument(new.path));
        }
        let line = new.line.max(1);
        let mut anchor = Anchor {
            quote,
            heading_path: new.heading_path,
            line,
            end_line: new.end_line.max(line),
            ..Anchor::default()
        };
        // the document knows better than the window where the passage is, and what surrounds it
        if let Doc::Read(analysis) = self.read_doc(&resolved.rel) {
            let probe = Probe {
                line: anchor.line,
                ..Probe::default()
            };
            if let Some(place) = analysis.locate(&anchor.quote, &probe) {
                apply(&mut anchor, &place);
            }
        }
        let created_ms = now_ms();
        let path = resolved.rel;
        let id = self.store.update(|comments| {
            if comments.len() >= MAX_COMMENTS {
                return Err(CommentError::TooMany);
            }
            let id = new_id(comments, &path, &anchor.quote, created_ms);
            comments.push(Comment {
                id: id.clone(),
                path: path.clone(),
                text,
                anchor,
                created_ms,
                done: None,
            });
            Ok((id, true))
        })?;
        self.changed();
        self.view_of(&id)
    }

    /// Marks a comment done by the person, or open again.
    pub(crate) fn set_done(&self, id: &str, done: bool) -> Result<CommentView, CommentError> {
        self.edit(id, |comment| match (done, &comment.done) {
            (true, None) => {
                comment.done = Some(Done {
                    at_ms: now_ms(),
                    by: DoneBy::Person,
                    reply: None,
                });
                true
            }
            (false, Some(_)) => {
                comment.done = None;
                true
            }
            _ => false,
        })
    }

    /// Marks a comment done by an agent, with a short reply. A comment that is done already keeps who did it and when; the
    /// reply is added to it.
    pub(crate) fn resolve(
        &self,
        id: &str,
        reply: Option<&str>,
    ) -> Result<CommentView, CommentError> {
        let reply = reply
            .map(|text| clip(text.trim(), MAX_REPLY_CHARS))
            .filter(|text| !text.is_empty());
        self.edit(id, |comment| {
            let before = comment.done.clone();
            let done = match before.clone() {
                Some(done) => Done {
                    reply: reply.clone().or(done.reply),
                    ..done
                },
                None => Done {
                    at_ms: now_ms(),
                    by: DoneBy::Agent,
                    reply: reply.clone(),
                },
            };
            let changed = before.as_ref() != Some(&done);
            comment.done = Some(done);
            changed
        })
    }

    fn edit(
        &self,
        id: &str,
        change: impl FnOnce(&mut Comment) -> bool,
    ) -> Result<CommentView, CommentError> {
        let changed = self.store.update(|comments| {
            let comment = comments
                .iter_mut()
                .find(|comment| comment.id == id)
                .ok_or_else(|| CommentError::NotFound(id.to_owned()))?;
            let changed = change(comment);
            Ok((changed, changed))
        })?;
        if changed {
            self.changed();
        }
        self.view_of(id)
    }

    /// Removes a comment.
    pub(crate) fn delete(&self, id: &str) -> Result<(), CommentError> {
        self.store.update(|comments| {
            let before = comments.len();
            comments.retain(|comment| comment.id != id);
            if comments.len() == before {
                return Err(CommentError::NotFound(id.to_owned()));
            }
            Ok(((), true))
        })?;
        self.changed();
        Ok(())
    }

    /// A document (or a folder, with `is_dir`) was renamed or moved in the Explorer: its comments follow it.
    pub(crate) fn renamed(&self, from: &str, to: &str, is_dir: bool) {
        let followed = self.store.update(|comments| {
            let mut any = false;
            for comment in comments.iter_mut() {
                if let Some(path) = moved(&comment.path, from, to, is_dir) {
                    comment.path = path;
                    any = true;
                }
            }
            Ok((any, any))
        });
        if followed.unwrap_or(false) {
            self.changed();
        }
    }
}

/// Whether the place found says something the comment does not know yet.
fn differs(anchor: &Anchor, place: &Placement) -> bool {
    anchor.line != place.start_line
        || anchor.end_line != place.end_line
        || anchor.heading_path != place.heading_path
        || anchor.before != place.before
        || anchor.after != place.after
}

fn apply(anchor: &mut Anchor, place: &Placement) {
    anchor.line = place.start_line;
    anchor.end_line = place.end_line;
    anchor.heading_path.clone_from(&place.heading_path);
    anchor.before.clone_from(&place.before);
    anchor.after.clone_from(&place.after);
}

/// A short id that no comment has: `n` and eight hexadecimal digits.
fn new_id(comments: &[Comment], path: &str, quote: &str, created_ms: u64) -> String {
    for attempt in 0_u32.. {
        let seed = format!(
            "{created_ms}\n{path}\n{quote}\n{}\n{attempt}",
            comments.len()
        );
        let id = format!("c{}", &hash_hex(seed.as_bytes())[..8]);
        if comments.iter().all(|comment| comment.id != id) {
            return id;
        }
    }
    unreachable!("the attempts do not run out")
}
