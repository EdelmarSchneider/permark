//! Net, not log: what a run of journal entries amounts to, per file. Created then deleted -> nothing; a rename chain
//! A -> B -> C -> one rename A -> C; many saves -> one `edited` with the union of sections; undone operations go;
//! a comment added and marked done -> omitted (unless asked); a tag added and removed -> nothing. Then each fact is
//! checked against the disk and the state now: a path gone (or moved outside, followed), a section heading gone, a
//! file changed outside after the person's last action on it, a comment already done.

use std::collections::HashMap;

use serde::Serialize;

use super::{By, CommentOp, Entry, Fact, FileOp, OutsideChange, ReviewOp};

/// The section label of the lines before a document's first heading.
pub const TOP: &str = "(top)";

/// A change on disk this long after the person's action is not that action's own write.
const OWN_WRITE_SLACK_MS: u64 = 2_000;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NetOptions {
    /// Keep comments added and marked done in the same run.
    pub include_done_comments: bool,
}

/// What one net row says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Kind {
    New,
    Imported,
    Deleted,
    /// Same folder, other name.
    Renamed {
        from: String,
    },
    /// Another folder.
    Moved {
        from: String,
    },
    Edited {
        sections: Vec<String>,
        blocks: u32,
    },
    Converted {
        to: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        saved: Option<String>,
    },
    Comment {
        op: CommentOp,
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        heading: Option<String>,
    },
    Tag {
        added: bool,
        tag: String,
    },
    Review {
        op: ReviewOp,
        #[serde(skip_serializing_if = "Option::is_none")]
        section: Option<String>,
    },
    Outside {
        change: OutsideChange,
        #[serde(skip_serializing_if = "Option::is_none")]
        from: Option<String>,
    },
}

/// What the check against the disk found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "flag", content = "value", rename_all = "camelCase")]
pub enum Flag {
    /// The path is not there any more.
    Gone,
    /// Moved outside PerMark after the person's action: it is here now.
    MovedTo(String),
    /// A section the save touched has no heading of that name any more.
    SectionGone(String),
    /// Changed outside PerMark after the person's last action on it.
    ChangedSince,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Row {
    /// Seq of the last entry the row stands for.
    pub seq: u64,
    pub at: u64,
    pub by: By,
    pub path: String,
    #[serde(flatten)]
    pub kind: Kind,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<Flag>,
}

fn parent(path: &str) -> &str {
    path.rfind('/').map_or("", |at| &path[..at])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Born {
    New,
    Imported,
}

/// Where a fact happened: (seq, time).
type When = (u64, u64);

#[derive(Debug, Default)]
struct Comments {
    added: Option<When>,
    /// Done | Reopen in order.
    toggles: Vec<(CommentOp, When)>,
    replied: Option<When>,
    removed: bool,
    heading: Option<String>,
}

/// One file followed through the run.
#[derive(Debug)]
struct Line {
    /// The path before the run (`None`: created in it).
    origin: Option<String>,
    born: Option<(Born, When)>,
    deleted: Option<When>,
    path: String,
    moved: Option<When>,
    sections: Vec<String>,
    blocks: u32,
    edited: Option<When>,
    converts: Vec<(String, Option<String>, When)>,
    /// Tag -> (+1 added | -1 removed net, last time).
    tags: Vec<(String, i32, When)>,
    comments: Vec<(String, Comments)>,
    reviews: Vec<(Option<String>, ReviewOp, When)>,
}

impl Line {
    fn new(path: &str, born: Option<(Born, When)>) -> Self {
        Self {
            origin: born.is_none().then(|| path.to_owned()),
            born,
            deleted: None,
            path: path.to_owned(),
            moved: None,
            sections: Vec::new(),
            blocks: 0,
            edited: None,
            converts: Vec::new(),
            tags: Vec::new(),
            comments: Vec::new(),
            reviews: Vec::new(),
        }
    }

    fn comment(&mut self, id: &str) -> &mut Comments {
        if let Some(at) = self.comments.iter().position(|(i, _)| i == id) {
            return &mut self.comments[at].1;
        }
        self.comments.push((id.to_owned(), Comments::default()));
        let last = self.comments.len() - 1;
        &mut self.comments[last].1
    }
}

#[derive(Default)]
struct Lines {
    all: Vec<Line>,
    /// Current path -> live line.
    live: HashMap<String, usize>,
    /// Path -> the latest line deleted there (a restore brings it back).
    deleted: HashMap<String, usize>,
}

impl Lines {
    fn at(&mut self, path: &str) -> usize {
        if let Some(&i) = self.live.get(path) {
            return i;
        }
        self.all.push(Line::new(path, None));
        let i = self.all.len() - 1;
        self.live.insert(path.to_owned(), i);
        i
    }

    fn born(&mut self, path: &str, born: Born, when: When) {
        // a new file where a deleted one was: a fresh line (the deleted one keeps its row)
        self.live.remove(path);
        self.all.push(Line::new(path, Some((born, when))));
        let i = self.all.len() - 1;
        self.live.insert(path.to_owned(), i);
    }

    fn delete(&mut self, path: &str, when: When) {
        let i = self.at(path);
        self.live.remove(path);
        self.all[i].deleted = Some(when);
        let key = self.all[i]
            .origin
            .clone()
            .unwrap_or_else(|| path.to_owned());
        self.deleted.insert(path.to_owned(), i);
        self.deleted.insert(key, i);
    }

    fn restore(&mut self, path: &str, when: When) {
        if let Some(i) = self.deleted.remove(path) {
            if self.all[i].deleted.is_some() && !self.live.contains_key(path) {
                self.all[i].deleted = None;
                path.clone_into(&mut self.all[i].path);
                self.live.insert(path.to_owned(), i);
                return;
            }
        }
        // restored without the delete in the run: it simply is there again
        let i = self.at(path);
        self.all[i].moved.get_or_insert(when);
    }

    fn rename(&mut self, from: &str, to: &str, when: When) {
        let i = self.at(from);
        self.live.remove(from);
        self.live.insert(to.to_owned(), i);
        to.clone_into(&mut self.all[i].path);
        self.all[i].moved = Some(when);
        // a folder takes the files below it along (their rows follow, the folder's row says it moved)
        let below = format!("{from}/");
        let inside: Vec<(String, usize)> = self
            .live
            .iter()
            .filter(|(p, _)| p.starts_with(&below))
            .map(|(p, i)| (p.clone(), *i))
            .collect();
        for (path, j) in inside {
            let moved = format!("{to}/{}", &path[below.len()..]);
            self.live.remove(&path);
            self.all[j].path.clone_from(&moved);
            self.live.insert(moved, j);
        }
    }
}

fn person_lines(entries: &[Entry]) -> Lines {
    let mut lines = Lines::default();
    for entry in entries.iter().filter(|e| e.by == By::Person) {
        let when = (entry.seq, entry.at);
        match &entry.fact {
            Fact::File { op, path, from, .. } => match op {
                FileOp::New => lines.born(path, Born::New, when),
                FileOp::Import => lines.born(path, Born::Imported, when),
                FileOp::Delete => lines.delete(path, when),
                FileOp::Restore => lines.restore(path, when),
                FileOp::Move => {
                    if let Some(from) = from.as_deref().filter(|f| *f != path) {
                        lines.rename(from, path, when);
                    }
                }
            },
            Fact::Save {
                path,
                sections,
                blocks,
            } => {
                let i = lines.at(path);
                let line = &mut lines.all[i];
                for section in sections {
                    if !line.sections.contains(section) {
                        line.sections.push(section.clone());
                    }
                }
                line.blocks = line.blocks.saturating_add(*blocks);
                line.edited = Some(when);
            }
            Fact::Convert { path, to, saved } => {
                let i = lines.at(path);
                let line = &mut lines.all[i];
                line.converts.retain(|(t, s, _)| !(t == to && s == saved));
                line.converts.push((to.clone(), saved.clone(), when));
            }
            Fact::Comment {
                op,
                path,
                id,
                heading,
            } => {
                let i = lines.at(path);
                let comment = lines.all[i].comment(id);
                if heading.is_some() {
                    comment.heading.clone_from(heading);
                }
                match op {
                    CommentOp::Add => comment.added = Some(when),
                    CommentOp::Reply => comment.replied = Some(when),
                    CommentOp::Done | CommentOp::Reopen => comment.toggles.push((*op, when)),
                    CommentOp::Remove => comment.removed = true,
                }
            }
            Fact::Tag { added, path, tag } => {
                let i = lines.at(path);
                let line = &mut lines.all[i];
                let step = if *added { 1 } else { -1 };
                match line.tags.iter_mut().find(|(t, _, _)| t == tag) {
                    // add then add (or remove then remove) is still one change
                    Some(found) => {
                        found.1 = (found.1 + step).clamp(-1, 1);
                        found.2 = when;
                    }
                    None => line.tags.push((tag.clone(), step, when)),
                }
            }
            Fact::Review { op, path, section } => {
                let i = lines.at(path);
                let line = &mut lines.all[i];
                line.reviews.retain(|(s, _, _)| s != section);
                line.reviews.push((section.clone(), *op, when));
            }
            Fact::Outside { .. } | Fact::HandOff { .. } | Fact::Closed => {}
        }
    }
    lines
}

fn row(when: When, by: By, path: &str, kind: Kind) -> Row {
    Row {
        seq: when.0,
        at: when.1,
        by,
        path: path.to_owned(),
        kind,
        flags: Vec::new(),
    }
}

fn comment_rows(line: &Line, options: NetOptions, out: &mut Vec<Row>) {
    for (id, c) in &line.comments {
        if c.removed {
            continue;
        }
        let heading = c.heading.clone();
        let kind = |op| Kind::Comment {
            op,
            id: id.clone(),
            heading: heading.clone(),
        };
        let last_toggle = c.toggles.last().copied();
        if let Some(added) = c.added {
            let done = last_toggle.is_some_and(|(op, _)| op == CommentOp::Done);
            if !done {
                out.push(row(
                    c.replied.map_or(added, |r| r.max(added)),
                    By::Person,
                    &line.path,
                    kind(CommentOp::Add),
                ));
            } else if options.include_done_comments {
                out.push(row(added, By::Person, &line.path, kind(CommentOp::Add)));
                if let Some((_, when)) = last_toggle {
                    out.push(row(when, By::Person, &line.path, kind(CommentOp::Done)));
                }
            }
            continue;
        }
        if let (Some((first, _)), Some((last, when))) = (c.toggles.first(), last_toggle) {
            // done then reopened (or the other way round) is where it was before the run
            let changed = c.toggles.len() % 2 == 1 || first == &last;
            if changed {
                out.push(row(when, By::Person, &line.path, kind(last)));
            }
        }
        if let Some(when) = c.replied {
            out.push(row(when, By::Person, &line.path, kind(CommentOp::Reply)));
        }
    }
}

/// The person's rows of a run of entries (any order of input; rows by seq).
#[must_use]
pub fn net(entries: &[Entry], options: NetOptions) -> Vec<Row> {
    let lines = person_lines(entries);
    let mut out = Vec::new();
    for line in &lines.all {
        match (line.born, line.deleted) {
            // created and gone again in the same run: nothing happened
            (Some(_), Some(_)) => continue,
            (None, Some(when)) => {
                let path = line.origin.as_deref().unwrap_or(&line.path);
                out.push(row(when, By::Person, path, Kind::Deleted));
                continue;
            }
            (Some((born, when)), None) => {
                let kind = match born {
                    Born::New => Kind::New,
                    Born::Imported => Kind::Imported,
                };
                out.push(row(when, By::Person, &line.path, kind));
            }
            (None, None) => {
                if let (Some(origin), Some(when)) = (&line.origin, line.moved) {
                    if *origin != line.path {
                        let from = origin.clone();
                        let kind = if parent(origin) == parent(&line.path) {
                            Kind::Renamed { from }
                        } else {
                            Kind::Moved { from }
                        };
                        out.push(row(when, By::Person, &line.path, kind));
                    }
                }
                if let Some(when) = line.edited {
                    out.push(row(
                        when,
                        By::Person,
                        &line.path,
                        Kind::Edited {
                            sections: line.sections.clone(),
                            blocks: line.blocks,
                        },
                    ));
                }
            }
        }
        for (to, saved, when) in &line.converts {
            out.push(row(
                *when,
                By::Person,
                &line.path,
                Kind::Converted {
                    to: to.clone(),
                    saved: saved.clone(),
                },
            ));
        }
        comment_rows(line, options, &mut out);
        for (tag, net, when) in &line.tags {
            if *net != 0 {
                out.push(row(
                    *when,
                    By::Person,
                    &line.path,
                    Kind::Tag {
                        added: *net > 0,
                        tag: tag.clone(),
                    },
                ));
            }
        }
        for (section, op, when) in &line.reviews {
            out.push(row(
                *when,
                By::Person,
                &line.path,
                Kind::Review {
                    op: *op,
                    section: section.clone(),
                },
            ));
        }
    }
    out.sort_by_key(|r| (r.seq, r.path.clone()));
    out
}

/// A file changed outside, followed through a run.
struct Track {
    /// The path before the run (`None`: created in it).
    origin: Option<String>,
    path: String,
    created: bool,
    deleted: bool,
    modified: bool,
    when: When,
}

/// The one row a followed file amounts to (`None`: nothing in the end).
fn outside_row(t: &Track) -> Option<Row> {
    let (change, from) = match (t.created, t.deleted) {
        (true, true) => return None,
        (true, false) => (OutsideChange::Created, None),
        (false, true) => {
            return Some(row(
                t.when,
                By::Outside,
                t.origin.as_deref().unwrap_or(&t.path),
                Kind::Outside {
                    change: OutsideChange::Deleted,
                    from: None,
                },
            ))
        }
        (false, false) => match &t.origin {
            Some(origin) if *origin != t.path => (OutsideChange::Renamed, Some(origin.clone())),
            _ if t.modified => (OutsideChange::Modified, None),
            // renamed back where it was, nothing else
            _ => return None,
        },
    };
    Some(row(
        t.when,
        By::Outside,
        &t.path,
        Kind::Outside { change, from },
    ))
}

/// The changes made outside PerMark in a run, per file (created + deleted -> nothing, rename chains, one row per
/// file).
#[must_use]
pub fn net_outside(entries: &[Entry]) -> Vec<Row> {
    let mut all: Vec<Track> = Vec::new();
    let mut live: HashMap<String, usize> = HashMap::new();
    for entry in entries.iter().filter(|e| e.by == By::Outside) {
        let Fact::Outside { change, path, from } = &entry.fact else {
            continue;
        };
        let when = (entry.seq, entry.at);
        let at = |p: &str, all: &mut Vec<Track>, live: &mut HashMap<String, usize>| {
            *live.entry(p.to_owned()).or_insert_with(|| {
                all.push(Track {
                    origin: Some(p.to_owned()),
                    path: p.to_owned(),
                    created: false,
                    deleted: false,
                    modified: false,
                    when,
                });
                all.len() - 1
            })
        };
        match change {
            OutsideChange::Created => {
                let i = match live.get(path) {
                    Some(&i) if all[i].deleted => i,
                    Some(&i) => i,
                    None => {
                        all.push(Track {
                            origin: None,
                            path: path.clone(),
                            created: true,
                            deleted: false,
                            modified: false,
                            when,
                        });
                        live.insert(path.clone(), all.len() - 1);
                        all.len() - 1
                    }
                };
                let t = &mut all[i];
                if t.deleted {
                    // deleted then created again: changed
                    t.deleted = false;
                    t.modified = true;
                }
                t.when = when;
            }
            OutsideChange::Modified => {
                let i = at(path, &mut all, &mut live);
                all[i].modified = true;
                all[i].when = when;
            }
            OutsideChange::Deleted => {
                let i = at(path, &mut all, &mut live);
                all[i].deleted = true;
                all[i].when = when;
            }
            OutsideChange::Renamed => {
                let from = from.as_deref().unwrap_or(path);
                let i = at(from, &mut all, &mut live);
                live.remove(from);
                live.insert(path.clone(), i);
                all[i].path.clone_from(path);
                all[i].when = when;
            }
        }
    }
    let mut out: Vec<Row> = all.iter().filter_map(outside_row).collect();
    out.sort_by_key(|r| (r.seq, r.path.clone()));
    out
}

/// What the check reads: the disk and the state now.
pub trait Disk {
    /// A file of the project is at `path` now.
    fn exists(&self, path: &str) -> bool;
    /// The heading paths (`A > B`) of a document now; `None`: not a document with headings | unreadable.
    fn headings(&self, path: &str) -> Option<Vec<String>>;
    /// Last modification (ms since the epoch).
    fn modified(&self, path: &str) -> Option<u64>;
    /// Whether a comment is done now (`None`: unknown).
    fn comment_done(&self, id: &str) -> Option<bool>;
}

/// Where a path went through renames outside after `seq`.
fn followed(path: &str, after: u64, later: &[Entry]) -> Option<String> {
    let mut now = path.to_owned();
    let mut moved = false;
    for entry in later
        .iter()
        .filter(|e| e.seq > after && e.by == By::Outside)
    {
        if let Fact::Outside {
            change: OutsideChange::Renamed,
            path,
            from: Some(from),
        } = &entry.fact
        {
            if *from == now {
                now.clone_from(path);
                moved = true;
            }
        }
    }
    moved.then_some(now)
}

/// Whether something outside changed `path` after `seq`.
fn touched_after(path: &str, after: u64, later: &[Entry]) -> bool {
    later.iter().any(|e| {
        e.seq > after
            && e.by == By::Outside
            && matches!(&e.fact, Fact::Outside { change, path: p, from }
                if (p == path && *change != OutsideChange::Deleted) || from.as_deref() == Some(path) && *change == OutsideChange::Modified)
    })
}

/// Checks each row against the disk now. `later`: every journal entry (the changes outside after a row are read
/// from it). Rows about comments already done are dropped (unless `options` keep them).
#[must_use]
pub fn verify(rows: Vec<Row>, disk: &dyn Disk, later: &[Entry], options: NetOptions) -> Vec<Row> {
    // the person's last action on each path
    let mut last: HashMap<String, (u64, u64)> = HashMap::new();
    for r in rows.iter().filter(|r| r.by == By::Person) {
        let e = last.entry(r.path.clone()).or_insert((r.seq, r.at));
        if r.seq > e.0 {
            *e = (r.seq, r.at);
        }
    }
    let mut out = Vec::with_capacity(rows.len());
    for mut r in rows {
        if r.by != By::Person {
            out.push(r);
            continue;
        }
        if let Kind::Comment {
            op: CommentOp::Add,
            id,
            ..
        } = &r.kind
        {
            if !options.include_done_comments && disk.comment_done(id) == Some(true) {
                continue;
            }
        }
        if r.kind == Kind::Deleted {
            out.push(r);
            continue;
        }
        let (last_seq, last_at) = last.get(&r.path).copied().unwrap_or((r.seq, r.at));
        if disk.exists(&r.path) {
            if let Kind::Edited { sections, .. } = &r.kind {
                let headings = disk.headings(&r.path).unwrap_or_default();
                for section in sections {
                    if section != TOP && !headings.iter().any(|h| h == section) {
                        r.flags.push(Flag::SectionGone(section.clone()));
                    }
                }
            }
            let outside = touched_after(&r.path, last_seq, later);
            let by_time = disk
                .modified(&r.path)
                .is_some_and(|m| m > last_at.saturating_add(OWN_WRITE_SLACK_MS));
            if outside || by_time {
                r.flags.push(Flag::ChangedSince);
            }
        } else {
            match followed(&r.path, r.seq, later).filter(|p| disk.exists(p)) {
                Some(now) => r.flags.push(Flag::MovedTo(now)),
                None => r.flags.push(Flag::Gone),
            }
        }
        out.push(r);
    }
    out
}
