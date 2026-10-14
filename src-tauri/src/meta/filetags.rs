//! Tags written in a file's front matter (`tags:`). They are read-only for the app.

use std::io::Read;
use std::path::Path;

use super::normalize::normalize_tags;
use crate::render::{decode_text, front_matter_value, RawValue};
use crate::util::retry_io;

/// How much of the start of a file is read to find its front matter.
const HEAD_BYTES: u64 = 64 * 1024;
const READ_RETRY_DELAYS: [std::time::Duration; 2] = [
    std::time::Duration::from_millis(50),
    std::time::Duration::from_millis(150),
];

/// Files whose front matter is read (the Markdown family the renderer knows).
pub fn has_front_matter(rel: &str) -> bool {
    Path::new(rel).extension().is_some_and(|e| {
        let e = e.to_string_lossy().to_ascii_lowercase();
        matches!(e.as_str(), "md" | "markdown" | "mdown" | "mkd" | "mdx")
    })
}

/// The normalized tags of a document's front matter. A list gives one tag per item. A single string
/// gives one tag per comma-separated part, or per word when it has no comma.
pub fn from_text(doc: &str) -> Vec<String> {
    match front_matter_value(doc, "tags") {
        None => Vec::new(),
        Some(RawValue::List(items)) => normalize_tags(&items),
        Some(RawValue::Scalar(text)) => {
            let parts: Vec<&str> = if text.contains(',') {
                text.split(',').collect()
            } else {
                text.split_whitespace().collect()
            };
            normalize_tags(&parts)
        }
    }
}

/// The tags in the front matter of a file on disk. Anything unreadable has none.
pub fn read(abs: &Path) -> Vec<String> {
    let Some(bytes) = read_head(abs) else {
        return Vec::new();
    };
    decode_text(&bytes).map_or_else(Vec::new, |text| from_text(&text))
}

fn read_head(abs: &Path) -> Option<Vec<u8>> {
    retry_io(&READ_RETRY_DELAYS, || {
        let mut bytes = Vec::new();
        std::fs::File::open(abs)?
            .take(HEAD_BYTES)
            .read_to_end(&mut bytes)?;
        Ok(bytes)
    })
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(doc: &str) -> Vec<String> {
        from_text(doc)
    }

    #[test]
    fn a_yaml_list_gives_one_tag_per_item() {
        assert_eq!(
            tags("---\ntags:\n  - Spec\n  - needs review\n---\nbody"),
            ["spec", "needs-review"]
        );
        assert_eq!(
            tags("---\ntags: [Feed, \"Watcher API\"]\n---\n"),
            ["feed", "watcher-api"]
        );
    }

    #[test]
    fn a_string_splits_on_commas_or_on_words() {
        assert_eq!(tags("---\ntags: feed, watcher\n---\n"), ["feed", "watcher"]);
        assert_eq!(tags("---\ntags: feed watcher\n---\n"), ["feed", "watcher"]);
        assert_eq!(
            tags("---\ntags: \"needs review, spec\"\n---\n"),
            ["needs-review", "spec"]
        );
        assert_eq!(tags("---\ntags: solo\n---\n"), ["solo"]);
    }

    #[test]
    fn repeats_and_empty_items_are_dropped() {
        assert_eq!(tags("---\ntags: [a, A, , b, ]\n---\n"), ["a", "b"]);
        assert_eq!(tags("---\ntags:\n---\n"), Vec::<String>::new());
        assert_eq!(tags("---\ntags: []\n---\n"), Vec::<String>::new());
    }

    #[test]
    fn documents_without_a_usable_front_matter_have_no_tags() {
        assert_eq!(tags("# Title\n\ntags: spec\n"), Vec::<String>::new());
        assert_eq!(tags("---\ntitle: A\n---\n"), Vec::<String>::new());
        assert_eq!(
            tags("---\ntags: spec\nno closing line"),
            Vec::<String>::new()
        );
        assert_eq!(tags(""), Vec::<String>::new());
    }

    #[test]
    fn only_the_markdown_family_is_read() {
        for yes in ["a.md", "docs/B.MARKDOWN", "x.mdx", "n.mkd", "n.mdown"] {
            assert!(has_front_matter(yes), "{yes}");
        }
        for no in ["a.txt", "a.json", "README", "a.md.bak"] {
            assert!(!has_front_matter(no), "{no}");
        }
    }

    #[test]
    fn reads_the_head_of_a_file_and_survives_odd_bytes() {
        let d = tempfile::tempdir().expect("tempdir");
        let md = d.path().join("a.md");
        std::fs::write(&md, "\u{feff}---\r\ntags: [x, y]\r\n---\r\n# A\r\n").expect("write");
        assert_eq!(read(&md), ["x", "y"]);
        let bin = d.path().join("b.md");
        std::fs::write(&bin, b"---\ntags: z\n\0\0---\n").expect("write");
        assert_eq!(read(&bin), Vec::<String>::new());
        assert_eq!(read(&d.path().join("missing.md")), Vec::<String>::new());
        let big = d.path().join("big.md");
        // A front matter that only ends after the part of the file that is read has no tags.
        let body = format!("---\ntags: z\n{}---\n", "key: value\n".repeat(20_000));
        std::fs::write(&big, body).expect("write");
        assert_eq!(read(&big), Vec::<String>::new());
    }
}
