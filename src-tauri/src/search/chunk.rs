//! Splitting a Markdown document into heading-based sections for section-level search.

use comrak::nodes::NodeValue;
use comrak::{parse_document, Arena};

use crate::render::markdown::{append_plain_text, options, plain_text, too_many_container_markers};

/// One section of a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    /// Breadcrumb of ancestor headings, ending with this section's own heading (empty before the first one).
    pub heading_path: Vec<String>,
    /// 1-based line of the section's heading (or of its first block when there is no heading).
    pub start_line: u32,
    /// Plain text of the section body, without Markdown syntax.
    pub text: String,
}

fn line_of<'a>(node: &'a comrak::nodes::AstNode<'a>) -> u32 {
    u32::try_from(node.data.borrow().sourcepos.start.line).unwrap_or(u32::MAX)
}

/// Splits `src` (already decoded, `\n` line endings) by headings.
pub fn chunk_markdown(src: &str) -> Vec<Chunk> {
    // Absurdly nested quotes or lists are not parsed (the parse time is quadratic); the raw text is
    // still searchable as one section.
    if too_many_container_markers(src) {
        return vec![Chunk {
            heading_path: Vec::new(),
            start_line: 1,
            text: src.trim().to_owned(),
        }];
    }
    let arena = Arena::new();
    let root = parse_document(&arena, src, &options());

    let mut chunks: Vec<Chunk> = Vec::new();
    // (level, text) of the open headings.
    let mut stack: Vec<(u8, String)> = Vec::new();
    let mut cur = Chunk {
        heading_path: Vec::new(),
        start_line: 1,
        text: String::new(),
    };
    let mut cur_has_heading = false;

    let flush = |cur: &mut Chunk, has_heading: bool, chunks: &mut Vec<Chunk>| {
        cur.text = cur.text.trim().to_owned();
        if has_heading || !cur.text.is_empty() {
            chunks.push(cur.clone());
        }
    };

    for node in root.children() {
        let value = node.data.borrow().value.clone();
        match value {
            NodeValue::Heading(h) => {
                flush(&mut cur, cur_has_heading, &mut chunks);
                while stack.last().is_some_and(|(lvl, _)| *lvl >= h.level) {
                    stack.pop();
                }
                stack.push((h.level, plain_text(node)));
                cur = Chunk {
                    heading_path: stack.iter().map(|(_, t)| t.clone()).collect(),
                    start_line: line_of(node),
                    text: String::new(),
                };
                cur_has_heading = true;
            }
            NodeValue::FrontMatter(raw) => {
                let pairs = crate::render::front_matter_pairs(&raw);
                for (k, v) in pairs {
                    for part in [&k, " ", &v, " "] {
                        cur.text.push_str(part);
                    }
                }
            }
            _ => {
                if cur.text.trim().is_empty() && !cur_has_heading {
                    cur.start_line = line_of(node);
                }
                append_plain_text(node, &mut cur.text);
                cur.text.push(' ');
            }
        }
    }
    flush(&mut cur, cur_has_heading, &mut chunks);
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_by_headings_with_breadcrumbs_and_lines() {
        let src = "Intro text.\n\n# Characters\n\nOverview of the cast.\n\n## Mira\n\nMira is a *pilot* with `a scar`.\n\n## Joss\n\nJoss repairs things.\n\n# Setting\n\nA station.\n";
        let c = chunk_markdown(src);
        let summary: Vec<(Vec<&str>, u32)> = c
            .iter()
            .map(|c| {
                (
                    c.heading_path.iter().map(String::as_str).collect(),
                    c.start_line,
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                (vec![], 1),
                (vec!["Characters"], 3),
                (vec!["Characters", "Mira"], 7),
                (vec!["Characters", "Joss"], 11),
                (vec!["Setting"], 15),
            ]
        );
        assert_eq!(c[0].text, "Intro text.");
        assert_eq!(c[2].text, "Mira is a pilot with a scar.");
    }

    #[test]
    fn headings_inside_code_fences_do_not_split() {
        let c = chunk_markdown("# Real\n\n```md\n# Not a heading\n```\n\nafter\n");
        assert_eq!(c.len(), 1);
        assert!(c[0].text.contains("# Not a heading"));
        assert!(c[0].text.contains("after"));
    }

    #[test]
    fn heading_without_body_still_makes_a_chunk() {
        let c = chunk_markdown("# Only a title\n");
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].heading_path, ["Only a title"]);
        assert_eq!(c[0].text, "");
    }

    #[test]
    fn skipped_heading_levels_nest_sensibly() {
        let c = chunk_markdown("# A\n\n### C\n\ntext\n\n## B\n\nmore\n");
        let paths: Vec<Vec<&str>> = c
            .iter()
            .map(|c| c.heading_path.iter().map(String::as_str).collect())
            .collect();
        assert_eq!(paths, vec![vec!["A"], vec!["A", "C"], vec!["A", "B"]]);
    }

    #[test]
    fn front_matter_values_are_searchable_in_the_preamble() {
        let c = chunk_markdown("---\ntitle: Plan\ntags: [alpha, beta]\n---\n\n# Body\n\ntext\n");
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].heading_path.len(), 0);
        assert!(c[0].text.contains("alpha, beta"));
        assert_eq!(c[1].heading_path, ["Body"]);
    }

    #[test]
    fn tables_lists_and_quotes_become_plain_text() {
        let c = chunk_markdown(
            "# T\n\n| a | b |\n|--|--|\n| one | two |\n\n- item x\n- item y\n\n> quoted words\n",
        );
        let t = &c[0].text;
        for w in ["one", "two", "item x", "item y", "quoted words"] {
            assert!(t.contains(w), "missing {w} in {t}");
        }
        assert!(!t.contains('|') && !t.contains("- "));
    }

    #[test]
    fn deeply_nested_quotes_are_indexed_as_plain_text_without_overflowing() {
        let src = format!("{} findme\n", ">".repeat(100_000));
        let started = std::time::Instant::now();
        let c = chunk_markdown(&src);
        assert_eq!(c.len(), 1);
        assert!(c[0].text.contains("findme"));
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }

    #[test]
    fn deep_inline_nesting_is_chunked_without_overflowing() {
        let src = format!("{}b{}\n", "_a ".repeat(100_000), " a_".repeat(100_000));
        let c = chunk_markdown(&src);
        assert_eq!(c.len(), 1);
        assert!(c[0].text.contains('b'));
    }

    #[test]
    fn front_matter_with_endless_brackets_is_chunked() {
        let src = format!(
            "---\ntags: {}{}\n---\n\n# Body\n\ntext\n",
            "[".repeat(100_000),
            "]".repeat(100_000)
        );
        let c = chunk_markdown(&src);
        assert_eq!(
            c.last().map(|c| c.heading_path.clone()),
            Some(vec!["Body".to_owned()])
        );
    }

    #[test]
    fn empty_and_whitespace_documents_have_no_chunks() {
        assert_eq!(chunk_markdown("").len(), 0);
        assert_eq!(chunk_markdown("\n\n  \n").len(), 0);
    }
}
