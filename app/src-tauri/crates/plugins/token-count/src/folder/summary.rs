//! What a folder count tells: the total, the files counted and skipped (with reasons), the file types, the biggest
//! files. Built as the job goes, pure.

use std::collections::HashMap;

use serde::Serialize;
use ts_rs::TS;

use super::guard::FolderSkip;

/// Skipped files listed by name; the counts are always exact.
pub const SKIPPED_LISTED: usize = 100;
/// File types told apart; the rest are folded into one row.
pub const TYPES_SHOWN: usize = 8;
/// Biggest files listed.
pub const TOP_FILES: usize = 10;
/// The `ext` of the row that folds the other types.
pub const OTHER_TYPES: &str = "*";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FolderSkipped {
    pub path: String,
    pub reason: FolderSkip,
}

/// One kind of file (by extension; `""` = none, `*` = all the others).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FolderTypeRow {
    pub ext: String,
    pub files: u32,
    #[ts(type = "number")]
    pub tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FolderTopFile {
    pub path: String,
    #[ts(type = "number")]
    pub tokens: u64,
}

/// The result of one folder count: one tokenizer, never mixed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FolderSummary {
    pub folder: String,
    pub tokenizer: String,
    /// An estimate (a family, or the stand-in for an exact count whose vocabulary is not here).
    pub approx: bool,
    #[ts(type = "number")]
    pub tokens: u64,
    pub counted: u32,
    pub skipped: u32,
    pub skipped_binary: u32,
    pub skipped_large: u32,
    pub skipped_unreadable: u32,
    /// The first [`SKIPPED_LISTED`] by path.
    pub skipped_list: Vec<FolderSkipped>,
    pub by_type: Vec<FolderTypeRow>,
    pub top: Vec<FolderTopFile>,
    #[ts(type = "number")]
    pub millis: u64,
    /// Files whose count came from the cache.
    pub cached: u32,
}

/// The extension of a project path's file name, lower case (`""`: none).
#[must_use]
pub fn extension(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    permark_core::tree::extension_of(name).unwrap_or_default()
}

#[derive(Debug, Default)]
pub struct Tally {
    tokens: u64,
    counted: u32,
    binary: u32,
    large: u32,
    unreadable: u32,
    skipped: Vec<FolderSkipped>,
    types: HashMap<String, (u32, u64)>,
    /// Most tokens first, then by path; at most [`TOP_FILES`].
    top: Vec<FolderTopFile>,
    cached: u32,
}

impl Tally {
    pub fn counted(&mut self, path: &str, tokens: u64, from_cache: bool) {
        self.tokens += tokens;
        self.counted += 1;
        self.cached += u32::from(from_cache);
        let row = self.types.entry(extension(path)).or_default();
        row.0 += 1;
        row.1 += tokens;
        let at = self.top.partition_point(|t| {
            (std::cmp::Reverse(t.tokens), t.path.as_str()) < (std::cmp::Reverse(tokens), path)
        });
        if at < TOP_FILES {
            self.top.insert(
                at,
                FolderTopFile {
                    path: path.to_owned(),
                    tokens,
                },
            );
            self.top.truncate(TOP_FILES);
        }
    }

    pub fn skipped(&mut self, path: &str, reason: FolderSkip) {
        match reason {
            FolderSkip::Binary => self.binary += 1,
            FolderSkip::TooLarge => self.large += 1,
            FolderSkip::Unreadable => self.unreadable += 1,
        }
        self.skipped.push(FolderSkipped {
            path: path.to_owned(),
            reason,
        });
    }

    #[must_use]
    pub fn finish(
        mut self,
        folder: &str,
        tokenizer: &str,
        approx: bool,
        millis: u64,
    ) -> FolderSummary {
        self.skipped.sort_by(|a, b| a.path.cmp(&b.path));
        let skipped = self.binary + self.large + self.unreadable;
        self.skipped.truncate(SKIPPED_LISTED);
        let mut types: Vec<FolderTypeRow> = self
            .types
            .into_iter()
            .map(|(ext, (files, tokens))| FolderTypeRow { ext, files, tokens })
            .collect();
        types.sort_by(|a, b| b.tokens.cmp(&a.tokens).then_with(|| a.ext.cmp(&b.ext)));
        if types.len() > TYPES_SHOWN {
            let rest = types.split_off(TYPES_SHOWN);
            types.push(FolderTypeRow {
                ext: OTHER_TYPES.to_owned(),
                files: rest.iter().map(|r| r.files).sum(),
                tokens: rest.iter().map(|r| r.tokens).sum(),
            });
        }
        FolderSummary {
            folder: folder.to_owned(),
            tokenizer: tokenizer.to_owned(),
            approx,
            tokens: self.tokens,
            counted: self.counted,
            skipped,
            skipped_binary: self.binary,
            skipped_large: self.large,
            skipped_unreadable: self.unreadable,
            skipped_list: self.skipped,
            by_type: types,
            top: self.top,
            millis,
            cached: self.cached,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_are_lower_case_and_dotfiles_have_none() {
        assert_eq!(extension("docs/A.MD"), "md");
        assert_eq!(extension("a/b/c.tar.gz"), "gz");
        assert_eq!(extension("Makefile"), "");
        assert_eq!(extension(".env"), "");
        assert_eq!(extension("dir.d/file"), "");
        assert_eq!(extension("x."), "");
    }

    #[test]
    fn totals_types_and_the_top_files_add_up() {
        let mut tally = Tally::default();
        tally.counted("a.md", 100, false);
        tally.counted("docs/b.md", 300, true);
        tally.counted("src/c.rs", 200, false);
        tally.counted("Makefile", 5, false);
        tally.skipped("img/z.png", FolderSkip::Binary);
        tally.skipped("data/big.json", FolderSkip::TooLarge);
        tally.skipped("a/gone.md", FolderSkip::Unreadable);
        let s = tally.finish("docs", "o200k", false, 12);
        assert_eq!((s.tokens, s.counted, s.skipped), (605, 4, 3));
        assert_eq!(
            (s.skipped_binary, s.skipped_large, s.skipped_unreadable),
            (1, 1, 1)
        );
        assert_eq!(s.cached, 1);
        assert_eq!(
            (s.folder.as_str(), s.tokenizer.as_str(), s.millis),
            ("docs", "o200k", 12)
        );
        assert!(!s.approx);
        let types: Vec<(&str, u32, u64)> = s
            .by_type
            .iter()
            .map(|r| (r.ext.as_str(), r.files, r.tokens))
            .collect();
        assert_eq!(types, [("md", 2, 400), ("rs", 1, 200), ("", 1, 5)]);
        let top: Vec<(&str, u64)> = s.top.iter().map(|t| (t.path.as_str(), t.tokens)).collect();
        assert_eq!(
            top,
            [
                ("docs/b.md", 300),
                ("src/c.rs", 200),
                ("a.md", 100),
                ("Makefile", 5)
            ]
        );
        let listed: Vec<&str> = s.skipped_list.iter().map(|x| x.path.as_str()).collect();
        assert_eq!(listed, ["a/gone.md", "data/big.json", "img/z.png"]);
    }

    #[test]
    fn only_the_ten_biggest_are_kept_and_ties_go_by_path() {
        let mut tally = Tally::default();
        for n in 0..25_u64 {
            tally.counted(&format!("f{n:02}.md"), n % 5, false);
        }
        tally.counted("big.md", 1000, false);
        let s = tally.finish("", "chars4", true, 0);
        assert_eq!(s.top.len(), TOP_FILES);
        assert_eq!(s.top[0].path, "big.md");
        // five files have 4 tokens: they come next, in path order
        let fours: Vec<&str> = s.top[1..6].iter().map(|t| t.path.as_str()).collect();
        assert_eq!(fours, ["f04.md", "f09.md", "f14.md", "f19.md", "f24.md"]);
        assert!(s.top.windows(2).all(|w| w[0].tokens >= w[1].tokens));
        assert!(s.approx);
    }

    #[test]
    fn many_types_fold_into_one_row_and_the_sums_hold() {
        let mut tally = Tally::default();
        for n in 0..12_u64 {
            tally.counted(&format!("f.t{n:02}"), 100 - n, false);
        }
        let s = tally.finish("", "o200k", false, 0);
        assert_eq!(s.by_type.len(), TYPES_SHOWN + 1);
        let other = s.by_type.last().unwrap();
        assert_eq!(other.ext, OTHER_TYPES);
        assert_eq!(other.files, 4);
        assert_eq!(other.tokens, (8..12).map(|n| 100 - n).sum::<u64>());
        assert_eq!(s.by_type.iter().map(|r| r.tokens).sum::<u64>(), s.tokens);
        assert_eq!(s.by_type.iter().map(|r| r.files).sum::<u32>(), s.counted);
    }

    #[test]
    fn the_skipped_list_is_cut_but_the_counts_stay_exact() {
        let mut tally = Tally::default();
        for n in 0..250 {
            tally.skipped(&format!("p{n:03}.png"), FolderSkip::Binary);
        }
        for n in 0..3 {
            tally.skipped(&format!("z{n}.json"), FolderSkip::TooLarge);
        }
        let s = tally.finish("", "o200k", false, 0);
        assert_eq!(s.skipped, 253);
        assert_eq!((s.skipped_binary, s.skipped_large), (250, 3));
        assert_eq!(s.skipped_list.len(), SKIPPED_LISTED);
        assert_eq!(s.skipped_list[0].path, "p000.png");
    }
}
