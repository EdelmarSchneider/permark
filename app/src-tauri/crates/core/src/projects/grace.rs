//! Grace window of an included project whose folder vanished while open (`projects.md` §4.4 step 1). The OS may
//! report a move's two halves in two watch batches (macOS: the removal, then the creation one batch later): the entry
//! stays Included for [`GRACE`] and new folders wait with it. A folder that appears meanwhile and is the same folder on
//! disk (same file id) -> the entry follows it (one `moved` row; no gone, no new-project strip). No match by the end of
//! the window -> gone, and the waiting folders are offered as usual. Pure: the caller gives the time.

use std::path::Path;
use std::time::{Duration, Instant, SystemTime};

use super::fsm::{Presence, ProjectMarker};
use super::uid::ProjectUid;
use crate::paths::VaultPath;

/// Twice the watch's longest batch (`watch::Timing::max`, 1 s): the second half of a split move lands in the batch
/// after the first one's, at most one `max` later.
pub(super) const GRACE: Duration = Duration::from_secs(2);

/// One folder on disk, across a rename on the same volume: volume + file id; creation time where the system keeps
/// one (a freed file id given to a new folder at once gets another creation time).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FolderId {
    volume: u64,
    file: u64,
    born: Option<SystemTime>,
}

impl FolderId {
    /// The folder at `abs` (never through a link); `None`: unknown.
    pub(super) fn of(abs: &Path) -> Option<Self> {
        let (volume, file) = crate::fsx::file_identity(abs)?;
        let born = std::fs::symlink_metadata(abs).ok()?.created().ok();
        Some(Self { volume, file, born })
    }

    fn same(self, other: Self) -> bool {
        self.volume == other.volume
            && self.file == other.file
            && match (self.born, other.born) {
                (Some(a), Some(b)) => a == b,
                _ => true,
            }
    }
}

/// An included entry whose folder is not on disk now, held as here until `until`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Vanished {
    pub until: Instant,
    /// What it was while here (the map keeps it).
    pub marker: ProjectMarker,
    /// Its folder while here; `None`: unknown (the name | first-commit rule pairs it).
    pub id: Option<FolderId>,
}

/// An entry not on disk this pass: still held (`Some`) or gone. `held`: its running window; `was`: its presence
/// before this pass (only an entry that was here starts a window); `id`: its folder while here.
pub(super) fn hold(
    held: Option<&Vanished>,
    was: Option<&Presence>,
    id: Option<FolderId>,
    now: Instant,
) -> Option<Vanished> {
    match (held, was) {
        (Some(v), _) => (now < v.until).then(|| v.clone()),
        (None, Some(Presence::Here(marker))) => Some(Vanished {
            until: now + GRACE,
            marker: *marker,
            id,
        }),
        (None, _) => None,
    }
}

/// A held entry, as pairing sees it.
#[derive(Debug)]
pub(super) struct Left<'a> {
    pub uid: &'a ProjectUid,
    pub path: &'a VaultPath,
    pub id: Option<FolderId>,
    pub first_commit: Option<&'a str>,
}

/// A folder that appeared during a window.
#[derive(Debug)]
pub(super) struct Came<'a> {
    pub path: &'a VaultPath,
    pub id: Option<FolderId>,
    /// Its root commit, when learned already.
    pub root_commit: Option<&'a str>,
}

/// Held entries paired with folders that came (`(entry, index in came)`; each folder once). Both ids known -> the
/// same folder only. Either unknown -> the watch's own rule: same first commit, or same folder name with exactly one of
/// that name on each side (the next batch never invents a pairing the watch would refuse).
pub(super) fn pair(left: &[Left<'_>], came: &[Came<'_>]) -> Vec<(ProjectUid, usize)> {
    let name = |path: &VaultPath| {
        let p = path.as_str();
        crate::paths::fold(p.rsplit('/').next().unwrap_or(p))
    };
    let mut used = vec![false; came.len()];
    let mut out = Vec::new();
    for entry in left {
        let by_id = |c: &Came<'_>| match (entry.id, c.id) {
            (Some(a), Some(b)) => Some(a.same(b)),
            _ => None,
        };
        let only_one_named = |n: &str| {
            left.iter().filter(|l| name(l.path) == n).count() == 1
                && came.iter().filter(|c| name(c.path) == n).count() == 1
        };
        let hit = came.iter().enumerate().position(|(i, c)| {
            !used[i]
                && by_id(c).unwrap_or_else(|| {
                    let commit = entry.first_commit.is_some_and(|f| c.root_commit == Some(f));
                    let n = name(c.path);
                    commit || (name(entry.path) == n && only_one_named(&n))
                })
        });
        if let Some(i) = hit {
            used[i] = true;
            out.push((entry.uid.clone(), i));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uid(s: &str) -> ProjectUid {
        ProjectUid::parse(s).unwrap()
    }

    fn vp(s: &str) -> VaultPath {
        VaultPath::parse(s).unwrap()
    }

    #[allow(clippy::unnecessary_wraps)] // reads as the field it fills
    fn id(file: u64) -> Option<FolderId> {
        Some(FolderId {
            volume: 7,
            file,
            born: None,
        })
    }

    #[test]
    fn a_window_starts_only_for_an_entry_that_was_here_and_ends_at_its_deadline() {
        let t0 = Instant::now();
        let here = Presence::Here(ProjectMarker::Git);
        let v = hold(None, Some(&here), id(1), t0).unwrap();
        assert_eq!(v.until, t0 + GRACE);
        assert_eq!(v.marker, ProjectMarker::Git);
        assert_eq!(hold(None, Some(&Presence::Gone), id(1), t0), None);
        assert_eq!(hold(None, None, id(1), t0), None);
        assert_eq!(hold(None, Some(&Presence::Ignored), id(1), t0), None);
        // the clock, moved by hand: held up to the deadline, gone at it (never extended by a later pass)
        let later = t0 + GRACE.saturating_sub(Duration::from_millis(1));
        assert_eq!(hold(Some(&v), Some(&here), id(9), later), Some(v.clone()));
        assert_eq!(hold(Some(&v), Some(&here), id(1), t0 + GRACE), None);
        assert_eq!(hold(Some(&v), None, None, t0 + GRACE * 10), None);
    }

    #[test]
    fn the_same_folder_pairs_whatever_its_new_name() {
        let (a, pa) = (uid("aaaaaaaaaa"), vp("inner/beta2"));
        let (c1, c2) = (vp("inner/other"), vp("inner/beta3"));
        let left = [Left {
            uid: &a,
            path: &pa,
            id: id(5),
            first_commit: None,
        }];
        let came = [
            Came {
                path: &c1,
                id: id(6),
                root_commit: None,
            },
            Came {
                path: &c2,
                id: id(5),
                root_commit: None,
            },
        ];
        assert_eq!(pair(&left, &came), vec![(a, 1)]);
    }

    #[test]
    fn known_ids_that_differ_never_pair_even_by_name_or_commit() {
        let (a, pa) = (uid("aaaaaaaaaa"), vp("x/beta"));
        let pc = vp("y/beta");
        let left = [Left {
            uid: &a,
            path: &pa,
            id: id(5),
            first_commit: Some("c0ffee"),
        }];
        let came = [Came {
            path: &pc,
            id: id(6),
            root_commit: Some("c0ffee"),
        }];
        assert!(pair(&left, &came).is_empty(), "another folder");
    }

    #[test]
    fn a_reused_file_id_with_another_birth_is_another_folder() {
        let t = SystemTime::UNIX_EPOCH;
        let old = FolderId {
            volume: 1,
            file: 2,
            born: Some(t),
        };
        let new = FolderId {
            born: Some(t + Duration::from_secs(1)),
            ..old
        };
        assert!(!old.same(new));
        assert!(old.same(FolderId { born: None, ..old }));
        assert!(!old.same(FolderId { volume: 3, ..old }));
    }

    #[test]
    fn unknown_ids_fall_back_to_the_watch_rule() {
        let (a, pa) = (uid("aaaaaaaaaa"), vp("x/beta"));
        let (b, pb) = (uid("bbbbbbbbbb"), vp("x/gamma"));
        let (c1, c2, c3) = (vp("y/beta"), vp("z/delta"), vp("q/beta"));
        let left = |ida, idb| {
            [
                Left {
                    uid: &a,
                    path: &pa,
                    id: ida,
                    first_commit: None,
                },
                Left {
                    uid: &b,
                    path: &pb,
                    id: idb,
                    first_commit: Some("c1"),
                },
            ]
        };
        // same name, one each side -> paired; renamed + same first commit -> paired
        let came = [
            Came {
                path: &c1,
                id: None,
                root_commit: None,
            },
            Came {
                path: &c2,
                id: id(9),
                root_commit: Some("c1"),
            },
        ];
        assert_eq!(
            pair(&left(None, None), &came),
            vec![(a.clone(), 0), (b.clone(), 1)]
        );
        // two folders of that name came: no guess
        let came2 = [
            Came {
                path: &c1,
                id: None,
                root_commit: None,
            },
            Came {
                path: &c3,
                id: None,
                root_commit: None,
            },
        ];
        assert!(pair(&left(None, None), &came2).is_empty(), "two alike");
        // nothing came: nothing paired
        assert!(pair(&left(id(1), id(2)), &[]).is_empty(), "none came");
    }

    #[test]
    fn one_folder_never_serves_two_entries() {
        let (a, pa) = (uid("aaaaaaaaaa"), vp("x/beta"));
        let (b, pb) = (uid("bbbbbbbbbb"), vp("y/beta"));
        let pc = vp("z/beta");
        let left = [
            Left {
                uid: &a,
                path: &pa,
                id: id(4),
                first_commit: None,
            },
            Left {
                uid: &b,
                path: &pb,
                id: id(4),
                first_commit: None,
            },
        ];
        let came = [Came {
            path: &pc,
            id: id(4),
            root_commit: None,
        }];
        assert_eq!(pair(&left, &came), vec![(a, 0)]);
    }

    #[test]
    fn a_real_folder_keeps_its_id_across_a_rename_and_a_new_one_differs() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a b ü");
        std::fs::create_dir(&a).unwrap();
        let before = FolderId::of(&a).unwrap();
        let b = dir.path().join("moved");
        std::fs::rename(&a, &b).unwrap();
        assert_eq!(FolderId::of(&a), None, "gone: unknown");
        assert!(FolderId::of(&b).unwrap().same(before));
        let c = dir.path().join("fresh");
        std::fs::create_dir(&c).unwrap();
        assert!(!FolderId::of(&c).unwrap().same(before));
    }
}
