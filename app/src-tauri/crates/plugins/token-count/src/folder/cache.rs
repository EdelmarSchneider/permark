//! The per-file cache of folder counts: what was counted for a file (by tokenizer and basis), tied to the file's size,
//! modified time and content hash, so a repeat count reads only what changed. Kept in memory per project and written
//! to the plugin's cache folder (rebuildable: a file that cannot be read is an empty cache).

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::guard::CACHE_ENTRIES;

/// What the cache knows of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub size: u64,
    pub modified_ms: i64,
    /// xxh3-64 of the bytes.
    pub hash: u64,
    pub verdict: Verdict,
}

/// What a read of the file came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Tokens(u64),
    /// Not text (a NUL in the head).
    Binary,
}

/// The cache file's version.
const VERSION: u32 = 1;

/// `[path, size, modified, hash (hex), tokens | null]`.
type Row = (String, u64, i64, String, Option<u64>);

#[derive(Serialize, Deserialize)]
struct Disk {
    v: u32,
    bases: HashMap<String, Vec<Row>>,
}

#[derive(Debug, Default)]
pub struct FileCache {
    /// Basis (`o200k`, `o200k~` for the interim estimate, `claude`, ...) -> project path -> entry.
    bases: HashMap<String, HashMap<String, Entry>>,
    entries: usize,
    file: Option<PathBuf>,
    dirty: bool,
}

impl FileCache {
    /// The cache kept in `file` (read now; nothing there, or nothing usable: empty). `None`: memory only.
    #[must_use]
    pub fn open(file: Option<PathBuf>) -> Self {
        let mut cache = Self {
            file,
            ..Self::default()
        };
        let Some(bytes) = cache.file.as_ref().and_then(|f| std::fs::read(f).ok()) else {
            return cache;
        };
        let Ok(disk) = serde_json::from_slice::<Disk>(&bytes) else {
            return cache;
        };
        if disk.v != VERSION {
            return cache;
        }
        for (basis, rows) in disk.bases {
            for (path, size, modified_ms, hash, tokens) in rows {
                let Ok(hash) = u64::from_str_radix(&hash, 16) else {
                    continue;
                };
                let verdict = tokens.map_or(Verdict::Binary, Verdict::Tokens);
                cache.insert(
                    &basis,
                    path,
                    Entry {
                        size,
                        modified_ms,
                        hash,
                        verdict,
                    },
                );
            }
        }
        cache.dirty = false;
        cache
    }

    #[must_use]
    pub fn get(&self, basis: &str, path: &str) -> Option<Entry> {
        self.bases.get(basis)?.get(path).copied()
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.entries == 0
    }

    /// Keeps `entry` for the file (a full cache keeps what it has, and takes no new file).
    pub fn put(&mut self, basis: &str, path: &str, entry: Entry) {
        self.insert(basis, path.to_owned(), entry);
        self.dirty = true;
    }

    fn insert(&mut self, basis: &str, path: String, entry: Entry) {
        let map = self.bases.entry(basis.to_owned()).or_default();
        if let Some(slot) = map.get_mut(&path) {
            *slot = entry;
        } else if self.entries < CACHE_ENTRIES {
            map.insert(path, entry);
            self.entries += 1;
        }
    }

    /// Forgets a file (every basis): the next count reads it.
    pub fn drop_path(&mut self, path: &str) {
        for map in self.bases.values_mut() {
            if map.remove(path).is_some() {
                self.entries -= 1;
                self.dirty = true;
            }
        }
    }

    /// Forgets everything below a folder (`""`: everything).
    pub fn drop_below(&mut self, folder: &str) {
        let prefix = format!("{folder}/");
        for map in self.bases.values_mut() {
            let before = map.len();
            map.retain(|path, _| !folder.is_empty() && !path.starts_with(&prefix));
            let gone = before - map.len();
            self.entries -= gone;
            self.dirty |= gone > 0;
        }
    }

    /// The file and the bytes to write when the cache changed since they were last asked for (serialized here, so
    /// the caller can write them without holding the cache).
    pub fn pending(&mut self) -> Option<(PathBuf, Vec<u8>)> {
        let file = self.file.clone()?;
        if !self.dirty {
            return None;
        }
        let bases = self
            .bases
            .iter()
            .filter(|(_, map)| !map.is_empty())
            .map(|(basis, map)| {
                let mut rows: Vec<_> = map
                    .iter()
                    .map(|(path, e)| {
                        let tokens = match e.verdict {
                            Verdict::Tokens(n) => Some(n),
                            Verdict::Binary => None,
                        };
                        (
                            path.clone(),
                            e.size,
                            e.modified_ms,
                            format!("{:x}", e.hash),
                            tokens,
                        )
                    })
                    .collect();
                rows.sort_unstable_by(|a, b| a.0.cmp(&b.0));
                (basis.clone(), rows)
            })
            .collect();
        let bytes = serde_json::to_vec(&Disk { v: VERSION, bases }).ok()?;
        self.dirty = false;
        Some((file, bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn save(cache: &mut FileCache) {
        let (file, bytes) = cache.pending().unwrap();
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, bytes).unwrap();
    }

    fn entry(tokens: u64) -> Entry {
        Entry {
            size: 10,
            modified_ms: 5,
            hash: 0xabc,
            verdict: Verdict::Tokens(tokens),
        }
    }

    #[test]
    fn entries_are_kept_apart_by_basis() {
        let mut cache = FileCache::default();
        cache.put("o200k", "a.md", entry(3));
        cache.put("o200k~", "a.md", entry(4));
        cache.put("claude", "a.md", entry(5));
        assert_eq!(cache.get("o200k", "a.md"), Some(entry(3)));
        assert_eq!(cache.get("o200k~", "a.md"), Some(entry(4)));
        assert_eq!(cache.get("claude", "a.md"), Some(entry(5)));
        assert_eq!(cache.get("gemini", "a.md"), None);
        assert_eq!(cache.get("o200k", "b.md"), None);
        assert_eq!(cache.len(), 3);
        // a newer entry replaces, it does not add
        cache.put("o200k", "a.md", entry(9));
        assert_eq!(cache.len(), 3);
        assert_eq!(cache.get("o200k", "a.md"), Some(entry(9)));
    }

    #[test]
    fn a_file_or_a_folder_can_be_forgotten() {
        let mut cache = FileCache::default();
        for path in ["a.md", "docs/b.md", "docs/sub/c.md", "docsx/d.md", "e.md"] {
            cache.put("o200k", path, entry(1));
            cache.put("claude", path, entry(1));
        }
        cache.drop_path("a.md");
        assert_eq!(cache.get("o200k", "a.md"), None);
        assert_eq!(cache.get("claude", "a.md"), None);
        assert_eq!(cache.len(), 8);
        cache.drop_below("docs");
        assert_eq!(cache.get("o200k", "docs/b.md"), None);
        assert_eq!(cache.get("o200k", "docs/sub/c.md"), None);
        // a folder whose name only starts the same stays
        assert!(cache.get("o200k", "docsx/d.md").is_some());
        assert!(cache.get("o200k", "e.md").is_some());
        assert_eq!(cache.len(), 4);
        cache.drop_below("");
        assert!(cache.is_empty());
    }

    #[test]
    fn the_cache_is_written_and_read_back_and_a_bad_file_is_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("deep").join("cache.json");
        let mut cache = FileCache::open(Some(file.clone()));
        assert!(cache.is_empty());
        cache.put("o200k", "docs/ü é 日本.md", entry(7));
        cache.put(
            "o200k",
            "bin.dat",
            Entry {
                verdict: Verdict::Binary,
                ..entry(0)
            },
        );
        cache.put("o200k~", "docs/ü é 日本.md", entry(8));
        save(&mut cache);
        let back = FileCache::open(Some(file.clone()));
        assert_eq!(back.len(), 3);
        assert_eq!(back.get("o200k", "docs/ü é 日本.md"), Some(entry(7)));
        assert_eq!(back.get("o200k~", "docs/ü é 日本.md"), Some(entry(8)));
        assert_eq!(
            back.get("o200k", "bin.dat").map(|e| e.verdict),
            Some(Verdict::Binary)
        );
        // nothing changed: nothing written
        let mut back = back;
        assert!(
            back.pending().is_none(),
            "nothing changed: nothing to write"
        );
        // a damaged file, or another version, is an empty cache
        std::fs::write(&file, b"{ not json").unwrap();
        assert!(FileCache::open(Some(file.clone())).is_empty());
        std::fs::write(
            &file,
            br#"{"v":99,"bases":{"o200k":[["a.md",1,1,"ff",3]]}}"#,
        )
        .unwrap();
        assert!(FileCache::open(Some(file.clone())).is_empty());
        std::fs::write(
            &file,
            br#"{"v":1,"bases":{"o200k":[["a.md",1,1,"zz",3],["b.md",1,1,"ff",3]]}}"#,
        )
        .unwrap();
        let partial = FileCache::open(Some(file));
        assert_eq!(partial.len(), 1, "the row with a bad hash is skipped");
    }

    #[test]
    fn a_full_cache_takes_no_new_file_but_updates_the_ones_it_has() {
        let mut cache = FileCache::default();
        for n in 0..CACHE_ENTRIES {
            cache.put("o200k", &format!("f{n}"), entry(1));
        }
        assert_eq!(cache.len(), CACHE_ENTRIES);
        cache.put("o200k", "one-too-many", entry(2));
        assert_eq!(cache.len(), CACHE_ENTRIES);
        assert_eq!(cache.get("o200k", "one-too-many"), None);
        cache.put("o200k", "f0", entry(9));
        assert_eq!(cache.get("o200k", "f0"), Some(entry(9)));
    }
}
