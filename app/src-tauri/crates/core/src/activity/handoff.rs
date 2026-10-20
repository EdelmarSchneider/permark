//! What a hand-off offers by default, nothing left to chance. Terms: a **session** = a run of the person's entries
//! with no pause over [`SESSION_GAP_MS`] and no project close in between; **stale** = a session that ended
//! [`STALE_MS`] or more before the hand-off. Cases:
//!
//! * S1 one session since the last hand-off -> that session.
//! * S2 several sessions, none stale -> all of them, grouped by session.
//! * S3 stale sessions present -> only the others checked; stale ones listed unchecked; all stale -> the latest checked
//!   and a note that older work is unchecked.
//! * S4 no action of the person, changes outside only -> the outside summary only.
//! * S5 nothing at all -> no dialog.
//! * S6 first hand-off ever -> as S3, staleness measured from the latest session's end.
//! * S7 retention dropped entries of the scope -> the package says older entries were not kept.

use serde::Serialize;
use ts_rs::TS;

use super::{By, Entry, Fact, Journal, SESSION_GAP_MS, STALE_MS};

/// One session of the person's work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "HandOffSession")]
pub struct Session {
    /// 1 = the oldest of the scope.
    pub id: u32,
    #[ts(type = "number")]
    pub first_seq: u64,
    #[ts(type = "number")]
    pub last_seq: u64,
    #[ts(type = "number")]
    pub start: u64,
    #[ts(type = "number")]
    pub end: u64,
    /// The person's entries in it (before netting).
    pub actions: u32,
    pub stale: bool,
    /// In the hand-off unless the person changes it.
    pub checked: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, rename = "HandOffCase")]
pub enum Case {
    /// S1
    One,
    /// S2
    Several,
    /// S3
    Stale,
    /// S4
    OutsideOnly,
    /// S5
    Nothing,
    /// S6
    First,
}

/// The default of a hand-off, built from the journal now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub case: Case,
    pub sessions: Vec<Session>,
    /// S3 with every session stale: the latest is checked anyway, older work unchecked.
    pub all_stale: bool,
    /// S7: retention dropped entries the scope would have held.
    pub trimmed: bool,
    /// The journal format changed since the last hand-off.
    pub upgraded: bool,
    /// The highest seq the scope looked at: confirming hands off exactly this.
    pub upto: u64,
    /// The changes outside in the scope (raw; netted when packaged).
    pub outside: Vec<Entry>,
    /// The person's entries in the scope (all sessions, checked or not).
    pub person: Vec<Entry>,
}

/// Splits the person's entries into sessions (markers: a close ends a session; hand-off markers are skipped).
#[must_use]
pub fn sessions(entries: &[Entry]) -> Vec<Session> {
    let mut out: Vec<Session> = Vec::new();
    let mut open = false;
    for entry in entries.iter().filter(|e| e.by == By::Person) {
        match entry.fact {
            Fact::Closed => {
                open = false;
                continue;
            }
            Fact::HandOff { .. } => continue,
            _ => {}
        }
        let joins = open
            && out
                .last()
                .is_some_and(|last| entry.at.saturating_sub(last.end) <= SESSION_GAP_MS);
        match out.last_mut().filter(|_| joins) {
            Some(session) => {
                session.last_seq = entry.seq;
                session.end = session.end.max(entry.at);
                session.actions += 1;
            }
            None => out.push(Session {
                id: u32::try_from(out.len() + 1).unwrap_or(u32::MAX),
                first_seq: entry.seq,
                last_seq: entry.seq,
                start: entry.at,
                end: entry.at,
                actions: 1,
                stale: false,
                checked: false,
            }),
        }
        open = true;
    }
    out
}

/// The default scope of a hand-off made at `now`.
#[must_use]
pub fn scope(journal: &Journal, now: u64) -> Scope {
    let since = journal.since_checkpoint();
    let upto = journal.entries.last().map_or(0, |e| e.seq);
    let person: Vec<Entry> = since
        .iter()
        .filter(|e| e.by == By::Person && !e.fact.is_marker())
        .cloned()
        .collect();
    let markers: Vec<Entry> = since
        .iter()
        .filter(|e| e.by == By::Person)
        .cloned()
        .collect();
    let outside: Vec<Entry> = since
        .iter()
        .filter(|e| e.by == By::Outside)
        .cloned()
        .collect();
    let first = journal.checkpoint.is_none();
    let trimmed = match &journal.checkpoint {
        None => journal.trimmed_through > 0,
        Some(c) => journal.trimmed_through > c.seq,
    };
    let mut sessions = sessions(&markers);
    let mut all_stale = false;
    let case = if person.is_empty() {
        if outside.is_empty() {
            Case::Nothing
        } else {
            Case::OutsideOnly
        }
    } else {
        // the time staleness is measured from: now, or (first hand-off) the latest session's end
        let anchor = if first {
            sessions.last().map_or(now, |s| s.end)
        } else {
            now
        };
        for s in &mut sessions {
            s.stale = anchor.saturating_sub(s.end) >= STALE_MS;
            s.checked = !s.stale;
        }
        if sessions.iter().all(|s| s.stale) {
            all_stale = true;
            if let Some(latest) = sessions.last_mut() {
                latest.checked = true;
            }
        }
        if first {
            Case::First
        } else if sessions.len() == 1 && !all_stale {
            Case::One
        } else if sessions.iter().any(|s| s.stale) {
            Case::Stale
        } else {
            Case::Several
        }
    };
    Scope {
        case,
        sessions,
        all_stale,
        trimmed,
        upgraded: journal.upgraded_from.is_some(),
        upto,
        outside,
        person,
    }
}

/// The person's entries of the given sessions.
#[must_use]
pub fn entries_of(scope: &Scope, chosen: &[u32]) -> Vec<Entry> {
    let ranges: Vec<(u64, u64)> = scope
        .sessions
        .iter()
        .filter(|s| chosen.contains(&s.id))
        .map(|s| (s.first_seq, s.last_seq))
        .collect();
    scope
        .person
        .iter()
        .filter(|e| ranges.iter().any(|(a, b)| e.seq >= *a && e.seq <= *b))
        .cloned()
        .collect()
}

/// The session a seq belongs to.
#[must_use]
pub fn session_of(sessions: &[Session], seq: u64) -> Option<u32> {
    sessions
        .iter()
        .find(|s| seq >= s.first_seq && seq <= s.last_seq)
        .map(|s| s.id)
}
