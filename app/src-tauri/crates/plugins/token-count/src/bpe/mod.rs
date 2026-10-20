//! Exact token counts of the byte-pair encodings (`o200k`, `cl100k`): the rank table loaded from the public rank file,
//! the splitting rules ([`split`]) and the merge. Special-token text counts as ordinary text.
//!
//! Counting only: the token ids are never built. A piece that is a token itself is one token; any other piece is
//! merged pair by pair, the pair of lowest rank first (leftmost on a tie), until no pair is a token.

mod split;
mod unicode;
mod unicode_tables;

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::hash::{BuildHasherDefault, Hasher};

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use xxhash_rust::xxh3::{xxh3_64, xxh3_64_with_seed};

pub use split::Rules;

/// Why a rank file was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RanksError {
    /// A line is not `base64 rank`, or the ranks do not run 0, 1, 2, ... in order.
    Malformed { line: usize },
    /// No line at all, or more tokens than a rank can number.
    Size,
}

/// Token bytes by rank, with lookup by bytes: one arena and an open-addressing table keyed by `xxh3`.
pub struct Ranks {
    arena: Vec<u8>,
    /// `starts[rank]..starts[rank + 1]` is the token in the arena.
    starts: Vec<u32>,
    /// Slot = rank + 1 (0 = empty); a power of two at least twice the number of tokens.
    table: Vec<u32>,
}

impl Ranks {
    /// Parses `base64(token bytes) SP rank` lines; ranks must run 0, 1, 2, ... in order (the public files do).
    ///
    /// # Errors
    /// `Malformed` (the first bad line, from 1) | `Size`.
    pub fn parse(file: &[u8]) -> Result<Self, RanksError> {
        let mut arena = Vec::with_capacity(file.len() / 2);
        let mut starts: Vec<u32> = Vec::with_capacity(file.len() / 12);
        for (at, line) in file.split(|b| *b == b'\n').enumerate() {
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            if line.is_empty() {
                continue;
            }
            let bad = RanksError::Malformed { line: at + 1 };
            let space = line.iter().position(|b| *b == b' ').ok_or(bad.clone())?;
            let (encoded, rank) = (&line[..space], &line[space + 1..]);
            let rank: usize = std::str::from_utf8(rank)
                .ok()
                .and_then(|r| r.parse().ok())
                .ok_or(bad.clone())?;
            if rank != starts.len() {
                return Err(bad);
            }
            starts.push(u32::try_from(arena.len()).map_err(|_| RanksError::Size)?);
            STANDARD.decode_vec(encoded, &mut arena).map_err(|_| bad)?;
        }
        if starts.is_empty() || starts.len() >= u32::MAX as usize {
            return Err(RanksError::Size);
        }
        starts.push(u32::try_from(arena.len()).map_err(|_| RanksError::Size)?);
        let slots = (starts.len() * 2).next_power_of_two();
        let mut ranks = Self {
            arena,
            starts,
            table: vec![0; slots],
        };
        let count = ranks.starts.len() - 1;
        for rank in 0..count {
            let mask = slots - 1;
            let mut slot = slot_of(xxh3_64(ranks.token(rank)), mask);
            while ranks.table[slot] != 0 {
                slot = (slot + 1) & mask;
            }
            ranks.table[slot] = u32::try_from(rank + 1).map_err(|_| RanksError::Size)?;
        }
        Ok(ranks)
    }

    fn token(&self, rank: usize) -> &[u8] {
        &self.arena[self.starts[rank] as usize..self.starts[rank + 1] as usize]
    }

    /// Number of tokens.
    #[must_use]
    pub fn len(&self) -> usize {
        self.starts.len() - 1
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The rank of exactly these bytes, if they are a token.
    #[must_use]
    pub fn rank(&self, bytes: &[u8]) -> Option<u32> {
        let mask = self.table.len() - 1;
        let mut slot = slot_of(xxh3_64(bytes), mask);
        loop {
            let entry = self.table[slot];
            if entry == 0 {
                return None;
            }
            let rank = entry - 1;
            if self.token(rank as usize) == bytes {
                return Some(rank);
            }
            slot = (slot + 1) & mask;
        }
    }
}

/// The first slot of a hash in a table of `mask + 1` slots (a power of two).
fn slot_of(hash: u64, mask: usize) -> usize {
    usize::try_from(hash & mask as u64).unwrap_or(0)
}

const NO_RANK: u32 = u32::MAX;

/// Pieces up to this many bytes use the quadratic scan; longer ones the heap (same result).
const SCAN_LIMIT: usize = 128;

/// Tokens of one piece with more than one byte that is not a token itself.
fn merged_count(ranks: &Ranks, piece: &[u8]) -> u32 {
    if piece.len() < SCAN_LIMIT {
        scan_merge(ranks, piece)
    } else {
        heap_merge(ranks, piece)
    }
}

/// Rank of `piece[start..end]`, `NO_RANK` when it is not a token.
fn rank_of(ranks: &Ranks, piece: &[u8], start: usize, end: usize) -> u32 {
    ranks.rank(&piece[start..end]).unwrap_or(NO_RANK)
}

/// The merge as the reference does it: parts hold the start of each part and the rank of the pair it begins.
fn scan_merge(ranks: &Ranks, piece: &[u8]) -> u32 {
    let mut parts: Vec<(usize, u32)> = Vec::with_capacity(piece.len() + 1);
    let mut min = (NO_RANK, usize::MAX);
    for at in 0..piece.len() - 1 {
        let rank = rank_of(ranks, piece, at, at + 2);
        if rank < min.0 {
            min = (rank, at);
        }
        parts.push((at, rank));
    }
    parts.push((piece.len() - 1, NO_RANK));
    parts.push((piece.len(), NO_RANK));
    let pair_rank = |parts: &[(usize, u32)], at: usize| {
        if at + 3 < parts.len() {
            rank_of(ranks, piece, parts[at].0, parts[at + 3].0)
        } else {
            NO_RANK
        }
    };
    while min.0 != NO_RANK {
        let at = min.1;
        if at > 0 {
            parts[at - 1].1 = pair_rank(&parts, at - 1);
        }
        parts[at].1 = pair_rank(&parts, at);
        parts.remove(at + 1);
        min = (NO_RANK, usize::MAX);
        for (at, &(_, rank)) in parts[..parts.len() - 1].iter().enumerate() {
            if rank < min.0 {
                min = (rank, at);
            }
        }
    }
    u32::try_from(parts.len() - 1).unwrap_or(u32::MAX)
}

/// The same merge with a heap of pair ranks and a linked list of parts: for long pieces.
fn heap_merge(ranks: &Ranks, piece: &[u8]) -> u32 {
    #[derive(Clone, Copy)]
    struct Part {
        start: usize,
        next: usize,
        prev: usize,
        /// Rank of the pair this part begins; `NO_RANK` when none.
        rank: u32,
        /// Bumped whenever the part changes, so older heap entries are recognized as stale.
        stamp: u32,
    }
    let n = piece.len();
    let mut parts: Vec<Part> = (0..n)
        .map(|i| Part {
            start: i,
            next: i + 1,
            prev: i.wrapping_sub(1),
            rank: NO_RANK,
            stamp: 0,
        })
        .collect();
    let end_of = |parts: &[Part], i: usize| -> usize {
        if parts[i].next >= n {
            n
        } else {
            parts[parts[i].next].start
        }
    };
    let mut heap: BinaryHeap<Reverse<(u32, usize, u32)>> = BinaryHeap::new();
    for (i, part) in parts.iter_mut().enumerate().take(n - 1) {
        part.rank = rank_of(ranks, piece, i, i + 2);
        if part.rank != NO_RANK {
            heap.push(Reverse((part.rank, i, 0)));
        }
    }
    let mut count = n;
    while let Some(Reverse((rank, i, stamp))) = heap.pop() {
        if parts[i].stamp != stamp || parts[i].rank != rank {
            continue;
        }
        // merge part i with the next one
        let j = parts[i].next;
        let after = parts[j].next;
        parts[i].next = after;
        if after < n {
            parts[after].prev = i;
        }
        parts[j].stamp = parts[j].stamp.wrapping_add(1);
        parts[j].rank = NO_RANK;
        count -= 1;
        // the pair that begins at the merged part
        parts[i].stamp = parts[i].stamp.wrapping_add(1);
        parts[i].rank = if after < n {
            rank_of(ranks, piece, parts[i].start, end_of(&parts, after))
        } else {
            NO_RANK
        };
        if parts[i].rank != NO_RANK {
            heap.push(Reverse((parts[i].rank, i, parts[i].stamp)));
        }
        // the pair that ends at it
        let before = parts[i].prev;
        if before < n {
            parts[before].stamp = parts[before].stamp.wrapping_add(1);
            parts[before].rank = rank_of(ranks, piece, parts[before].start, end_of(&parts, i));
            if parts[before].rank != NO_RANK {
                heap.push(Reverse((parts[before].rank, before, parts[before].stamp)));
            }
        }
    }
    u32::try_from(count).unwrap_or(u32::MAX)
}

/// A hasher for the piece cache: `xxh3` over the bytes written.
#[derive(Default)]
struct PieceHasher(u64);

impl Hasher for PieceHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        self.0 = xxh3_64_with_seed(bytes, self.0);
    }
}

/// Longest piece kept in the per-call cache.
const CACHE_PIECE: usize = 32;

/// One encoding: its rank table and its splitting rules.
pub struct Bpe {
    ranks: Ranks,
    rules: Rules,
}

impl Bpe {
    #[must_use]
    pub fn new(ranks: Ranks, rules: Rules) -> Self {
        Self { ranks, rules }
    }

    #[must_use]
    pub fn ranks(&self) -> &Ranks {
        &self.ranks
    }

    /// Tokens of `text` (special-token text is ordinary text).
    #[must_use]
    pub fn count(&self, text: &str) -> u64 {
        let mut cache: HashMap<&[u8], u32, BuildHasherDefault<PieceHasher>> = HashMap::default();
        let mut total = 0_u64;
        for piece in split::pieces(text, self.rules) {
            let bytes = piece.as_bytes();
            total += u64::from(self.piece_count(bytes, &mut cache));
        }
        total
    }

    fn piece_count<'t>(
        &self,
        bytes: &'t [u8],
        cache: &mut HashMap<&'t [u8], u32, BuildHasherDefault<PieceHasher>>,
    ) -> u32 {
        if bytes.len() <= 1 {
            return u32::from(!bytes.is_empty());
        }
        if bytes.len() > CACHE_PIECE {
            return self.uncached(bytes);
        }
        if let Some(count) = cache.get(bytes) {
            return *count;
        }
        let count = self.uncached(bytes);
        cache.insert(bytes, count);
        count
    }

    fn uncached(&self, bytes: &[u8]) -> u32 {
        if self.ranks.rank(bytes).is_some() {
            1
        } else {
            merged_count(&self.ranks, bytes)
        }
    }
}

#[cfg(test)]
mod tests;
