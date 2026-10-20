//! The size guard of a folder count: what is counted at once, what is asked first, what is refused, and how long the
//! work is expected to take. Pure.

use permark_core::tree::{Category, ScanFile};
use serde::Serialize;
use ts_rs::TS;

/// A file larger than this is skipped, not read.
pub const MAX_FILE_BYTES: u64 = 5_000_000;
/// Entries of the cache file and of the per-file cache.
pub const CACHE_ENTRIES: usize = 200_000;

/// Why a folder is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FolderRefusal {
    Files,
    Bytes,
}

/// What the guard says about a folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Go,
    Confirm,
    Refuse(FolderRefusal),
}

/// The sizes at which the guard acts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Thresholds {
    /// More files than this: the person is asked first.
    pub confirm_files: u64,
    /// More bytes to read than this: the person is asked first.
    pub confirm_bytes: u64,
    /// More files than this: refused.
    pub refuse_files: u64,
    /// More bytes to read than this: refused.
    pub refuse_bytes: u64,
    /// A folder counted by itself (the totals shown automatically) holds at most this many files ...
    pub auto_files: u64,
    /// ... and at most this many bytes to read.
    pub auto_bytes: u64,
}

impl Thresholds {
    pub const STANDARD: Self = Self {
        confirm_files: 2_000,
        confirm_bytes: 50_000_000,
        refuse_files: 50_000,
        refuse_bytes: 1_000_000_000,
        auto_files: 200,
        auto_bytes: 5_000_000,
    };

    #[must_use]
    pub fn verdict(&self, files: u64, bytes: u64) -> Verdict {
        if files > self.refuse_files {
            Verdict::Refuse(FolderRefusal::Files)
        } else if bytes > self.refuse_bytes {
            Verdict::Refuse(FolderRefusal::Bytes)
        } else if files > self.confirm_files || bytes > self.confirm_bytes {
            Verdict::Confirm
        } else {
            Verdict::Go
        }
    }

    /// Where the walk of a folder the person asked for stops.
    #[must_use]
    pub fn refuse_limits(&self) -> Limits {
        Limits {
            files: self.refuse_files,
            bytes: self.refuse_bytes,
        }
    }

    /// Where the walk of an automatic count stops.
    #[must_use]
    pub fn auto_limits(&self) -> Limits {
        Limits {
            files: self.auto_files,
            bytes: self.auto_bytes,
        }
    }
}

/// Where the walk stops: the first limit passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub files: u64,
    pub bytes: u64,
}

/// Why a file is not counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum FolderSkip {
    /// A picture, sound, video or office document by its name, or a NUL in the first 8 KB.
    Binary,
    /// More than [`MAX_FILE_BYTES`].
    TooLarge,
    /// Could not be opened or read (gone meanwhile, no access).
    Unreadable,
}

/// Whether a file is left out before anything is read: by what its name says it is, or by its size.
#[must_use]
pub fn skip_before_reading(file: &ScanFile) -> Option<FolderSkip> {
    let name = file.path.rsplit('/').next().unwrap_or(&file.path);
    let svg = name.to_ascii_lowercase().ends_with(".svg");
    match file.category {
        Category::Audio | Category::Video | Category::Office => Some(FolderSkip::Binary),
        Category::Image if !svg => Some(FolderSkip::Binary),
        Category::Markdown
        | Category::Text
        | Category::Config
        | Category::Data
        | Category::Image
        | Category::Other => (file.size > MAX_FILE_BYTES).then_some(FolderSkip::TooLarge),
    }
}

/// Files found and bytes that would be read, as the walk goes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Found {
    pub files: u64,
    pub bytes: u64,
}

impl Found {
    /// Takes a file in; `true` when a limit is passed (the walk ends there).
    pub fn add(&mut self, file: &ScanFile, limits: Limits) -> bool {
        self.files += 1;
        if skip_before_reading(file).is_none() {
            self.bytes += file.size;
        }
        self.files > limits.files || self.bytes > limits.bytes
    }

    /// Which limit was passed (files first).
    #[must_use]
    pub fn passed(&self, limits: Limits) -> Option<FolderRefusal> {
        if self.files > limits.files {
            Some(FolderRefusal::Files)
        } else if self.bytes > limits.bytes {
            Some(FolderRefusal::Bytes)
        } else {
            None
        }
    }
}

/// Microseconds a file costs apart from its bytes (open, stat, hash, bookkeeping): measured 40 on a warm disk, taken
/// five times that for a cold one.
const PER_FILE_MICROS: u64 = 200;
/// Bytes a second an exact count gets through, taken on the slow side (measured about 4 times this).
const EXACT_BYTES_PER_SECOND: u64 = 20_000_000;
/// The same for an estimate.
const ESTIMATE_BYTES_PER_SECOND: u64 = 80_000_000;

/// How long counting `files` files with `bytes` bytes to read is expected to take, in whole seconds (at least 1).
#[must_use]
pub fn seconds(files: u64, bytes: u64, exact: bool) -> u32 {
    let rate = if exact {
        EXACT_BYTES_PER_SECOND
    } else {
        ESTIMATE_BYTES_PER_SECOND
    };
    let micros = files
        .saturating_mul(PER_FILE_MICROS)
        .saturating_add(bytes.saturating_mul(1_000_000) / rate);
    u32::try_from(micros.div_ceil(1_000_000).max(1)).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, category: Category, size: u64) -> ScanFile {
        ScanFile {
            path: path.to_owned(),
            category,
            abs: None,
            size,
            modified_ms: 0,
        }
    }

    #[test]
    fn the_thresholds_are_edges() {
        let verdict = |files, bytes| Thresholds::STANDARD.verdict(files, bytes);
        assert_eq!(verdict(2_000, 50_000_000), Verdict::Go);
        assert_eq!(verdict(2_001, 0), Verdict::Confirm);
        assert_eq!(verdict(0, 50_000_001), Verdict::Confirm);
        assert_eq!(verdict(50_000, 1_000_000_000), Verdict::Confirm);
        assert_eq!(verdict(50_001, 0), Verdict::Refuse(FolderRefusal::Files));
        assert_eq!(
            verdict(0, 1_000_000_001),
            Verdict::Refuse(FolderRefusal::Bytes)
        );
        // files win when both are over
        assert_eq!(
            verdict(60_000, 2_000_000_000),
            Verdict::Refuse(FolderRefusal::Files)
        );
        assert_eq!(verdict(0, 0), Verdict::Go);
    }

    #[test]
    fn the_automatic_limits_are_edges() {
        let at = |files, bytes| {
            let found = Found { files, bytes };
            found.passed(Thresholds::STANDARD.auto_limits()).is_none()
        };
        assert!(at(200, 5_000_000));
        assert!(!at(201, 0));
        assert!(!at(0, 5_000_001));
    }

    #[test]
    fn files_are_left_out_by_name_or_size_before_reading() {
        let skip = |f: &ScanFile| skip_before_reading(f);
        assert_eq!(skip(&file("a.md", Category::Markdown, 10)), None);
        assert_eq!(skip(&file("a.rs", Category::Other, 10)), None);
        assert_eq!(skip(&file("a.svg", Category::Image, 10)), None);
        assert_eq!(skip(&file("dir/A.SVG", Category::Image, 10)), None);
        assert_eq!(
            skip(&file("a.png", Category::Image, 10)),
            Some(FolderSkip::Binary)
        );
        assert_eq!(
            skip(&file("a.mp4", Category::Video, 10)),
            Some(FolderSkip::Binary)
        );
        assert_eq!(
            skip(&file("a.mp3", Category::Audio, 10)),
            Some(FolderSkip::Binary)
        );
        assert_eq!(
            skip(&file("a.docx", Category::Office, 10)),
            Some(FolderSkip::Binary)
        );
        // exactly 5 MB is counted, one byte more is not
        assert_eq!(skip(&file("a.json", Category::Data, 5_000_000)), None);
        assert_eq!(
            skip(&file("a.json", Category::Data, 5_000_001)),
            Some(FolderSkip::TooLarge)
        );
        // a big picture is binary, not "too large"
        assert_eq!(
            skip(&file("a.png", Category::Image, 90_000_000)),
            Some(FolderSkip::Binary)
        );
    }

    #[test]
    fn the_walk_counts_every_file_but_only_the_bytes_it_would_read() {
        let mut found = Found::default();
        let limits = Limits {
            files: 3,
            bytes: 100,
        };
        assert!(!found.add(&file("a.md", Category::Markdown, 60), limits));
        assert!(!found.add(&file("b.png", Category::Image, 9_000), limits));
        assert!(!found.add(&file("c.md", Category::Markdown, 40), limits));
        assert_eq!(
            found,
            Found {
                files: 3,
                bytes: 100
            }
        );
        assert_eq!(found.passed(limits), None);
        // the fourth file passes the files limit, a fifth would pass the bytes limit first
        assert!(found.add(&file("d.png", Category::Image, 1), limits));
        assert_eq!(found.passed(limits), Some(FolderRefusal::Files));
        let mut found = Found::default();
        assert!(found.add(&file("big.md", Category::Markdown, 101), limits));
        assert_eq!(found.passed(limits), Some(FolderRefusal::Bytes));
    }

    #[test]
    fn the_estimated_time_grows_with_the_work_and_is_never_zero() {
        assert_eq!(seconds(0, 0, true), 1);
        assert_eq!(seconds(1, 1, false), 1);
        let small = seconds(2_001, 10_000_000, true);
        let more_files = seconds(20_000, 10_000_000, true);
        let more_bytes = seconds(2_001, 500_000_000, true);
        assert!(small < more_files && small < more_bytes);
        assert!(seconds(2_001, 100_000_000, false) < seconds(2_001, 100_000_000, true));
        // 1 GB read exactly: a minute or so, not an hour
        assert!((30..=120).contains(&seconds(0, Thresholds::STANDARD.refuse_bytes, true)));
        // nothing overflows
        assert!(seconds(u64::MAX, u64::MAX, true) > 1_000_000);
    }
}
