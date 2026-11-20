//! This install's own record of one owner's `comments.json`: the kept copy (the text it last wrote or saw there) and
//! the comments an outside write removed since (set aside for a while, restorable). Both are sealed with the install
//! key and bound to the folder (`Machine::seal`): a kept copy | aside that came with a copied folder, from a sync
//! partner, or was planted fails its seal and is ignored, so nothing from elsewhere is ever restored. Without a kept
//! copy (none yet, lost, or not this install's) nothing is compared until this install writes the file again.

use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::Arc;

use permark_core::state::{
    Loaded, Machine, RawFile, Seal, StateData, StateError, StateFile, StateStore,
};
use serde::{Deserialize, Serialize};

use crate::model::Comment;
use crate::service::{book_of, now_ms};

const KEPT_FILE: &str = "comments-kept.json";
const ASIDES_FILE: &str = "comments-asides.json";
const KEPT_SEAL: &str = "comments-kept";
const ASIDE_SEAL: &str = "comments-aside";
/// Most asides one owner keeps: a newer one pushes the oldest out.
const MAX_ASIDES: usize = 3;
/// How long an aside is kept (30 days).
const ASIDE_MS: u64 = 30 * 24 * 60 * 60 * 1000;

#[derive(Default, Serialize, Deserialize)]
struct Kept {
    /// The comments file's text.
    text: String,
    seal: String,
}

impl StateData for Kept {
    const VERSION: u32 = 1;
    /// The comments file as one JSON string: escaping can double it.
    const MAX_BYTES: u64 = 2 * crate::MAX_FILE_BYTES + 1024 * 1024;
}

#[derive(Clone, Serialize, Deserialize)]
struct Record {
    id: u64,
    at: u64,
    big: bool,
    /// The removed comments as the file held them (a JSON list).
    comments: String,
    seal: String,
}

#[derive(Default, Serialize, Deserialize)]
struct Records {
    asides: Vec<Record>,
}

impl StateData for Records {
    const VERSION: u32 = 1;
    const MAX_BYTES: u64 = MAX_ASIDES as u64 * Kept::MAX_BYTES;
}

/// Comments an outside write removed from the file.
pub(crate) struct Aside {
    pub id: u64,
    /// When they were removed (ms since the epoch).
    pub at: u64,
    /// A large part of the comments, or the whole file (no longer comments), was lost.
    pub big: bool,
    pub comments: Vec<Comment>,
}

/// The text of the file's bytes (`None`: no file, the same as an empty kept copy: no comments).
pub(crate) fn text_of(bytes: Option<&[u8]>) -> Cow<'_, str> {
    bytes.map_or(Cow::Borrowed(""), String::from_utf8_lossy)
}

/// What an outside write removed: the comments of `kept` the file (`now`) no longer holds, every one of them when the
/// file is no longer comments, and whether that is a big loss. `None`: nothing removed (additions and changes are not
/// removals).
fn removed(kept: &str, now: Option<&[u8]>) -> Option<(Vec<Comment>, bool)> {
    let kept = book_of(kept.as_bytes())?;
    let (present, unreadable): (HashSet<String>, bool) = match now.map(book_of) {
        None => (HashSet::new(), false),
        Some(Some(book)) => (book.comments.into_iter().map(|c| c.id).collect(), false),
        Some(None) => (HashSet::new(), true),
    };
    let total = kept.comments.len();
    let gone: Vec<Comment> = kept
        .comments
        .into_iter()
        .filter(|c| !present.contains(&c.id))
        .collect();
    if gone.is_empty() {
        return None;
    }
    let big = unreadable || gone.len() >= total.div_ceil(2).max(3);
    Some((gone, big))
}

/// The kept copy and the asides of one owner's comments file.
pub(crate) struct Keeper {
    machine: Machine,
    kept: StateFile<Kept>,
    /// The kept copy's file again, to remove it when a newer text could not be written: a stale kept copy would show
    /// this install's own changes as removals.
    kept_raw: RawFile,
    asides: StateFile<Records>,
    /// Called when an aside was made or went away.
    told: Arc<dyn Fn() + Send + Sync>,
}

impl Keeper {
    /// `None`: no usable install key (nothing kept, nothing set aside: the comments work as before).
    pub(crate) fn of(store: &StateStore, told: Arc<dyn Fn() + Send + Sync>) -> Option<Self> {
        Some(Self {
            machine: store.machine()?.clone(),
            kept: store.machine_file(KEPT_FILE)?,
            kept_raw: store.machine_raw_file(KEPT_FILE)?,
            asides: store.machine_file(ASIDES_FILE)?,
            told,
        })
    }

    fn kept_seal(&self, text: &str) -> Seal {
        let mut seal = self.machine.seal(KEPT_SEAL);
        seal.field(text.as_bytes());
        seal
    }

    fn aside_seal(&self, record: &Record) -> Seal {
        let mut seal = self.machine.seal(ASIDE_SEAL);
        seal.num(record.id)
            .num(record.at)
            .num(u64::from(record.big))
            .field(record.comments.as_bytes());
        seal
    }

    /// The text this install last wrote or saw in the file; `None`: none, or not sealed by this install for this
    /// folder.
    pub(crate) fn kept(&self) -> Option<String> {
        match self.kept.load() {
            Loaded::Found(kept) if self.kept_seal(&kept.text).verify(&kept.seal) => Some(kept.text),
            _ => None,
        }
    }

    /// Keeps `text` as the file's.
    pub(crate) fn keep(&self, text: &str) {
        let kept = Kept {
            text: text.to_owned(),
            seal: self.kept_seal(text).sign(),
        };
        if self.kept.save(&kept).is_err() && self.kept().is_some() {
            let _ = self.kept_raw.remove();
        }
    }

    /// The file holds `now`: what an outside write removed since the kept copy is set aside, then the kept copy follows
    /// the file. `true`: an aside was made.
    pub(crate) fn follow(&self, now: Option<&[u8]>, at: u64) -> bool {
        let Some(kept) = self.kept() else {
            return false;
        };
        let text = text_of(now);
        if text == kept {
            return false;
        }
        match self.observe(&kept, now, at) {
            Ok(made) => {
                self.keep(&text);
                made
            }
            // not set aside: the kept copy stays, so the next look | write finds the loss again
            Err(_) => false,
        }
    }

    /// Sets aside what the file (`now`) lost against `reference`, a text this install wrote or saw there.
    ///
    /// # Errors
    /// The asides could not be written.
    pub(crate) fn observe(
        &self,
        reference: &str,
        now: Option<&[u8]>,
        at: u64,
    ) -> Result<bool, StateError> {
        match removed(reference, now) {
            Some((gone, big)) => self.set_aside(at, &gone, big),
            None => Ok(false),
        }
    }

    /// The valid asides of `records`: sealed by this install for this folder, not older than 30 days at `now`.
    fn valid(&self, records: &[Record], now: u64) -> Vec<Record> {
        records
            .iter()
            .filter(|r| now.saturating_sub(r.at) < ASIDE_MS && self.aside_seal(r).verify(&r.seal))
            .cloned()
            .collect()
    }

    /// Sets `removed` aside as removed at `at`; comments an aside already holds as they are are not set aside twice (a
    /// restore that could not be written makes no second aside). `Ok(false)`: nothing new to set aside.
    ///
    /// # Errors
    /// The asides could not be written.
    pub(crate) fn set_aside(
        &self,
        at: u64,
        removed: &[Comment],
        big: bool,
    ) -> Result<bool, StateError> {
        let mut made = false;
        self.asides.update(|records| {
            made = false;
            let old = records.map_or(&[][..], |r| r.asides.as_slice());
            let mut list = self.valid(old, now_ms().max(at));
            let held: Vec<Comment> = list
                .iter()
                .filter_map(|r| serde_json::from_str::<Vec<Comment>>(&r.comments).ok())
                .flatten()
                .collect();
            let fresh: Vec<&Comment> = removed.iter().filter(|c| !held.contains(c)).collect();
            if fresh.is_empty() {
                return (list.len() != old.len()).then_some(Records { asides: list });
            }
            let comments = serde_json::to_string(&fresh).ok()?;
            let id = list
                .iter()
                .map(|r| r.id)
                .max()
                .map_or(at, |last| at.max(last.saturating_add(1)));
            let mut record = Record {
                id,
                at,
                big,
                comments,
                seal: String::new(),
            };
            record.seal = self.aside_seal(&record).sign();
            list.push(record);
            let extra = list.len().saturating_sub(MAX_ASIDES);
            list.drain(..extra);
            made = true;
            Some(Records { asides: list })
        })?;
        if made {
            (self.told)();
        }
        Ok(made)
    }

    /// This install's asides for this folder, oldest first; expired and unsealed ones are taken out of the file.
    pub(crate) fn asides(&self, now: u64) -> Vec<Aside> {
        let Loaded::Found(records) = self.asides.load() else {
            return Vec::new();
        };
        let valid = self.valid(&records.asides, now);
        if valid.len() != records.asides.len() {
            let pruned = self.asides.update(|records| {
                let records = records?;
                let valid = self.valid(&records.asides, now);
                (valid.len() != records.asides.len()).then_some(Records { asides: valid })
            });
            if pruned.is_ok() {
                (self.told)();
            }
        }
        valid
            .into_iter()
            .filter_map(|r| {
                Some(Aside {
                    id: r.id,
                    at: r.at,
                    big: r.big,
                    comments: serde_json::from_str(&r.comments).ok()?,
                })
            })
            .collect()
    }

    /// Takes aside `id` out of the file. `Ok(false)`: no such aside (restored | dismissed meanwhile).
    ///
    /// # Errors
    /// The asides could not be written.
    pub(crate) fn drop_aside(&self, id: u64) -> Result<bool, StateError> {
        let mut had = false;
        self.asides.update(|records| {
            had = false;
            let mut list = records?.asides.clone();
            list.retain(|r| r.id != id);
            had = list.len() != records?.asides.len();
            had.then_some(Records { asides: list })
        })?;
        if had {
            (self.told)();
        }
        Ok(had)
    }
}
