//! `ProjectUid`: the stable identity of a project inside records + cards (`projects.md` §1.2). Base32 lowercase
//! (RFC 4648 alphabet), drawn from xxh3: no new dependency, no clock trust (a collision is drawn again).

use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use ts_rs::TS;

const ALPHABET: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz234567";
/// Length of a drawn id (50 bits).
const DRAWN: usize = 10;
/// Shortest | longest id accepted on read.
const MIN_LEN: usize = 8;
const MAX_LEN: usize = 16;

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Stable id of a project in one record: 8..16 chars of `[a-z2-7]`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, TS)]
#[ts(export, type = "string")]
pub struct ProjectUid(String);

impl Serialize for ProjectUid {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ProjectUid {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).ok_or_else(|| serde::de::Error::custom("not a project id"))
    }
}

impl std::fmt::Display for ProjectUid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// `chars` base32 characters from the top bits of `bits`.
fn encode(bits: u64, chars: usize) -> String {
    (0..chars)
        .map(|i| {
            let shift = 64 - 5 * (i + 1);
            char::from(ALPHABET[usize::try_from((bits >> shift) & 31).unwrap_or(0)])
        })
        .collect()
}

impl ProjectUid {
    /// An id as written in a file: `None` when it is not 8..16 chars of the alphabet.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let ok = (MIN_LEN..=MAX_LEN).contains(&text.len())
            && text.bytes().all(|b| ALPHABET.contains(&b));
        ok.then(|| Self(text.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// A fresh id for the project at `path_key`; drawn again while `taken` says it is used (in the record, removed
    /// this session).
    #[must_use]
    pub fn draw(path_key: &str, taken: &dyn Fn(&Self) -> bool) -> Self {
        loop {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let mut seed = Vec::with_capacity(32 + path_key.len());
            seed.extend_from_slice(&nanos.to_le_bytes());
            seed.extend_from_slice(&std::process::id().to_le_bytes());
            seed.extend_from_slice(&n.to_le_bytes());
            seed.extend_from_slice(path_key.as_bytes());
            let uid = Self(encode(xxhash_rust::xxh3::xxh3_64(&seed), DRAWN));
            if !taken(&uid) {
                return uid;
            }
        }
    }

    /// Id of a record entry written without one: `h` + 9 chars of xxh3 of its path key (stable while the path is).
    #[must_use]
    pub fn derived(path_key: &str) -> Self {
        let mut text = String::from("h");
        text.push_str(&encode(
            xxhash_rust::xxh3::xxh3_64(path_key.as_bytes()),
            DRAWN - 1,
        ));
        Self(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_accepts_the_alphabet_only() {
        for good in ["k7m2q7xd2a", "abcdefgh", "a234567abcdefghi"] {
            assert!(ProjectUid::parse(good).is_some(), "{good}");
        }
        for bad in [
            "",
            "abcdefg",
            "a234567abcdefghij",
            "K7M2Q9XD2A",
            "k7m2q9xd1a",
            "k7m2q9xd0a",
            "k7m2q9xd-a",
            "k7m2 q9xda",
            "ħħħħħħħħ",
        ] {
            assert!(ProjectUid::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn drawn_ids_are_valid_distinct_and_skip_taken_ones() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..2000 {
            let uid = ProjectUid::draw("hubs/a", &|_| false);
            assert_eq!(uid.as_str().len(), DRAWN);
            assert!(ProjectUid::parse(uid.as_str()).is_some());
            assert!(seen.insert(uid), "collision in 2000 draws");
        }
        let first = ProjectUid::draw("x", &|_| false);
        let calls = std::cell::Cell::new(0);
        let again = ProjectUid::draw("x", &|u| {
            calls.set(calls.get() + 1);
            calls.get() == 1 || u == &first
        });
        assert!(calls.get() >= 2, "a taken id is drawn again");
        assert_ne!(again, first);
    }

    #[test]
    fn derived_ids_are_stable_and_valid() {
        let a = ProjectUid::derived("hubs/hub-b");
        assert_eq!(a, ProjectUid::derived("hubs/hub-b"));
        assert_ne!(a, ProjectUid::derived("hubs/hub-c"));
        assert!(a.as_str().starts_with('h'));
        assert!(ProjectUid::parse(a.as_str()).is_some());
    }

    #[test]
    fn serde_refuses_invalid_ids() {
        assert!(serde_json::from_str::<ProjectUid>("\"k7m2q7xd2a\"").is_ok());
        assert!(serde_json::from_str::<ProjectUid>("\"../../x\"").is_err());
    }
}
