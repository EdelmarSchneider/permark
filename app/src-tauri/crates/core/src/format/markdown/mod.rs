//! Markdown (comrak + ammonia): text -> sanitized blocks, outline, front matter.
//!
//! Pipeline: front matter cut -> prompt-tag markers -> parse -> AST edits (template variables, `@path` references)
//! -> HTML -> prompt markers to `<details>` -> sanitize -> blocks. Line numbers hold through every step: the cuts
//! and markers keep each line in place.

mod blocks;
mod frontmatter;
mod inline;
mod prompt;
mod sanitize;
mod sections;
mod segments;
#[cfg(test)]
mod tests;

use std::borrow::Cow;
use std::collections::HashSet;

use comrak::nodes::{AstNode, NodeValue};
use comrak::{format_html, parse_document, Anchorizer, Arena, Options};

pub use sections::{sections, Section};
pub use segments::{segments, LockReason, Segment, SegmentKind};

use super::{Block, Body, Claim, Format, FormatId, FrontMatter, OutlineItem, RenderCx, Rendered};

/// A line that starts with more container markers (`>` quotes, list bullets) than this is not parsed: comrak's parse
/// time grows quadratically with quote nesting, so the check comes first.
const MAX_CONTAINER_MARKERS: usize = 100;
/// The deepest tree (parsed Markdown, or the HTML made from it) that is sanitized and handed to the webview.
const MAX_TREE_DEPTH: usize = 256;
const DEEP_NESTING_NOTE: &str = "deepNesting";
/// Longest outline label kept.
const MAX_LABEL: usize = 200;

const EXTENSIONS: [&str; 5] = ["md", "markdown", "mdown", "mkd", "mdx"];

/// The core Markdown format.
#[derive(Debug, Clone, Copy)]
pub struct MarkdownFormat;

impl Format for MarkdownFormat {
    fn id(&self) -> FormatId {
        FormatId::MARKDOWN
    }

    fn claims(&self, _name: &str, ext: Option<&str>, _head: &[u8]) -> Claim {
        if ext.is_some_and(|e| EXTENSIONS.contains(&e)) {
            Claim::Strong
        } else {
            Claim::No
        }
    }

    fn render(&self, text: &str, cx: &RenderCx<'_>) -> Rendered {
        render(text, cx)
    }
}

fn options() -> Options<'static> {
    let mut o = Options::default();
    let ext = &mut o.extension;
    ext.table = true;
    ext.tasklist = true;
    ext.strikethrough = true;
    ext.autolink = true;
    ext.footnotes = true;
    ext.alerts = true;
    ext.description_lists = true;
    ext.math_dollars = true;
    ext.math_code = true;
    // Heading ids, without a prefix so that `[x](#my-heading)` works as authored.
    ext.header_id_prefix = Some(String::new());
    o.render.sourcepos = true;
    o.render.tasklist_classes = true;
    // Raw HTML is passed through here and made safe by the sanitizer afterwards.
    o.render.r#unsafe = true;
    o
}

fn render(text: &str, cx: &RenderCx<'_>) -> Rendered {
    if too_many_container_markers(text) {
        return plain(text);
    }
    // 1. front matter
    let cut = frontmatter::find(text);
    let card = cx.features.front_matter_card;
    let prepared: Cow<'_, str> = match &cut {
        Some(cut) => {
            let fence = if card.is_some() {
                None
            } else {
                Some(match cut.syntax {
                    super::FrontMatterSyntax::Yaml => "yaml",
                    super::FrontMatterSyntax::Toml => "toml",
                })
            };
            frontmatter::replace(text, cut, fence)
        }
        None => Cow::Borrowed(text),
    };
    let front_matter = cut.as_ref().zip(card).map(|(cut, parse)| FrontMatter {
        syntax: cut.syntax,
        raw: cut.raw.clone(),
        fields: parse(cut.syntax, &cut.raw),
    });
    // 2. prompt tags
    let prepared = match cx.features.prompt_blocks {
        Some(blocks) => match prompt::mark(&prepared, blocks.tags) {
            Cow::Borrowed(_) => prepared,
            Cow::Owned(marked) => Cow::Owned(marked),
        },
        None => prepared,
    };
    // 3. parse
    let arena = Arena::new();
    let opts = options();
    let root = parse_document(&arena, &prepared, &opts);
    if is_deeper_than(root, MAX_TREE_DEPTH) {
        return plain(text);
    }
    // 4. AST edits + outline (after the edits: the ids are the ones the HTML gets)
    if cx.features.prompt_blocks.is_some() {
        mark_inline(root, cx);
    }
    let headings = headings_of(root);
    // 5. HTML
    let mut raw = String::with_capacity(prepared.len() + prepared.len() / 2);
    // Writing into a String cannot fail; an error here would only mean an empty document.
    let _ = format_html(root, &opts, &mut raw);
    let raw = strip_heading_anchors(&raw);
    let raw = match cx.features.prompt_blocks {
        Some(_) => prompt::to_details(&raw).into_owned(),
        None => raw,
    };
    // 6. sanitize + blocks (raw HTML in the document can nest without limit even when the Markdown tree is shallow)
    let scan = blocks::scan(&raw);
    if scan.deepest > MAX_TREE_DEPTH {
        return plain(text);
    }
    let parts = clean_parts(&scan.html, &scan.cuts, cx.path);
    let ids = blocks::ids(&parts);
    let outline = outline_of(&headings, &parts, &ids);
    let blocks = assemble(parts, ids, cx.known);
    Rendered {
        body: Body::Markdown {
            blocks,
            outline,
            front_matter,
        },
        notes: Vec::new(),
    }
}

/// Documents of this size or more are sanitized on several threads.
const PARALLEL_FROM: usize = 256 * 1024;
/// Smallest piece of HTML handed to one sanitizer call.
const PIECE: usize = 32 * 1024;
const MAX_THREADS: usize = 8;

/// Sanitizes the top-level pieces of `raw` (each on its own, so a block never depends on its neighbours) and cuts
/// them into blocks. Big documents use several threads; the result is the same in the same order.
fn clean_parts(raw: &str, cuts: &[usize], doc: &str) -> Vec<blocks::RawBlock> {
    let pieces = blocks::pieces(raw, cuts, PIECE);
    let cleaner = sanitize::Cleaner::new(doc);
    let work = |piece: &&str| blocks::split(&cleaner.clean(piece));
    let threads = if raw.len() < PARALLEL_FROM {
        1
    } else {
        std::thread::available_parallelism()
            .map_or(1, std::num::NonZero::get)
            .min(MAX_THREADS)
    };
    if threads < 2 || pieces.len() < 2 {
        return pieces.iter().flat_map(work).collect();
    }
    // contiguous groups of about equal size
    let per_group = raw.len() / threads + 1;
    let mut groups: Vec<&[&str]> = Vec::new();
    let (mut from, mut size) = (0, 0);
    for (at, piece) in pieces.iter().enumerate() {
        size += piece.len();
        if size >= per_group {
            groups.push(&pieces[from..=at]);
            from = at + 1;
            size = 0;
        }
    }
    if from < pieces.len() {
        groups.push(&pieces[from..]);
    }
    std::thread::scope(|scope| {
        let handles: Vec<_> = groups
            .iter()
            .map(|group| scope.spawn(|| group.iter().flat_map(work).collect::<Vec<_>>()))
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().unwrap_or_default())
            .collect()
    })
}

fn assemble(parts: Vec<blocks::RawBlock>, ids: Vec<String>, known: &HashSet<String>) -> Vec<Block> {
    parts
        .into_iter()
        .zip(ids)
        .map(|(part, id)| {
            let html = (!known.contains(&id)).then(|| blocks::mark(&part.html, &id));
            Block {
                id,
                kind: part.tag,
                line_start: part.line_start,
                line_end: part.line_end,
                html,
            }
        })
        .collect()
}

/// The document as escaped plain text in one block, for sources too deeply nested to render safely.
fn plain(text: &str) -> Rendered {
    let mut html = String::with_capacity(text.len() + 40);
    html.push_str("<pre><code>");
    for c in text.chars() {
        match c {
            '&' => html.push_str("&amp;"),
            '<' => html.push_str("&lt;"),
            '>' => html.push_str("&gt;"),
            _ => html.push(c),
        }
    }
    html.push_str("</code></pre>");
    let id = crate::fsx::hash(html.as_bytes()).hex();
    let lines = u32::try_from(text.lines().count().max(1)).unwrap_or(u32::MAX);
    Rendered {
        body: Body::Markdown {
            blocks: vec![Block {
                html: Some(blocks::mark(&html, &id)),
                id,
                kind: "pre".to_owned(),
                line_start: 1,
                line_end: lines,
            }],
            outline: Vec::new(),
            front_matter: None,
        },
        notes: vec![DEEP_NESTING_NOTE],
    }
}

// ---- AST ------------------------------------------------------------------------------------------------------------

/// Template variables and `@path` references in text nodes (not in links, images or code).
fn mark_inline<'a>(root: &'a AstNode<'a>, cx: &RenderCx<'_>) {
    let nodes: Vec<&AstNode<'_>> = root.descendants().collect();
    for node in nodes {
        let replacement = match &node.data().value {
            NodeValue::Text(text) => inline::mark(text, cx.env),
            _ => None,
        };
        let Some(html) = replacement else {
            continue;
        };
        let in_link = node
            .ancestors()
            .skip(1)
            .any(|a| matches!(a.data().value, NodeValue::Link(_) | NodeValue::Image(_)));
        if !in_link {
            node.data_mut().value = NodeValue::HtmlInline(html);
        }
    }
}

struct Heading {
    label: String,
    level: u8,
    id: String,
    line: u32,
}

/// Headings in document order with the ids the HTML gives them.
fn headings_of<'a>(root: &'a AstNode<'a>) -> Vec<Heading> {
    let mut anchorizer = Anchorizer::new();
    root.descendants()
        .filter_map(|node| {
            let data = node.data();
            let NodeValue::Heading(heading) = &data.value else {
                return None;
            };
            let level = heading.level;
            let line = u32::try_from(data.sourcepos.start.line).unwrap_or(u32::MAX);
            drop(data);
            let text = node.collect_text();
            let id = anchorizer.anchorize(&text);
            let label: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
            Some(Heading {
                label: label.chars().take(MAX_LABEL).collect(),
                level,
                id,
                line,
            })
        })
        .collect()
}

fn outline_of(
    headings: &[Heading],
    parts: &[blocks::RawBlock],
    ids: &[String],
) -> Vec<OutlineItem> {
    headings
        .iter()
        .map(|heading| {
            let block = parts
                .iter()
                .position(|p| p.line_start <= heading.line && heading.line <= p.line_end)
                .and_then(|at| ids.get(at))
                .cloned()
                .unwrap_or_default();
            OutlineItem {
                label: heading.label.clone(),
                level: heading.level,
                id: heading.id.clone(),
                line: heading.line,
                block,
            }
        })
        .collect()
}

/// comrak ends every heading with an empty `<a class="anchor">` link (and repeats the heading text in it); the heading
/// already has its `id` and the UI adds its own anchor, so the extra link is dropped.
fn strip_heading_anchors(html: &str) -> String {
    const OPEN: &str = "<a href=\"#";
    const CLOSE: &str = "</a>";
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(at) = rest.find(OPEN) {
        let after = &rest[at..];
        let Some(end) = after.find(CLOSE) else { break };
        let candidate = &after[..end + CLOSE.len()];
        out.push_str(&rest[..at]);
        if !candidate.ends_with("class=\"anchor\"></a>") {
            out.push_str(candidate);
        }
        rest = &after[candidate.len()..];
    }
    out.push_str(rest);
    out
}

// ---- guards ---------------------------------------------------------------------------------------------------------

/// How many container markers (`>` and list bullets) start this line, ignoring spaces between them.
fn container_markers(line: &str) -> usize {
    let bytes = line.as_bytes();
    let (mut at, mut count) = (0, 0);
    loop {
        while at < bytes.len() && matches!(bytes[at], b' ' | b'\t') {
            at += 1;
        }
        match bytes.get(at) {
            Some(b'>') => {
                count += 1;
                at += 1;
            }
            Some(b'-' | b'*' | b'+') if matches!(bytes.get(at + 1), Some(b' ' | b'\t')) => {
                count += 1;
                at += 1;
            }
            Some(b'0'..=b'9') => {
                let digits = bytes[at..]
                    .iter()
                    .take_while(|b| b.is_ascii_digit())
                    .count();
                let after = at + digits;
                if digits <= 9
                    && matches!(bytes.get(after), Some(b'.' | b')'))
                    && matches!(bytes.get(after + 1), Some(b' ' | b'\t'))
                {
                    count += 1;
                    at = after + 1;
                } else {
                    return count;
                }
            }
            _ => return count,
        }
    }
}

/// Whether some line opens more nested quotes or list items than comrak parses in reasonable time.
fn too_many_container_markers(text: &str) -> bool {
    text.lines().any(|line| {
        line.len() > MAX_CONTAINER_MARKERS && container_markers(line) > MAX_CONTAINER_MARKERS
    })
}

/// Whether the tree under `root` is deeper than `limit` levels (walked iteratively).
fn is_deeper_than<'a>(root: &'a AstNode<'a>, limit: usize) -> bool {
    let mut stack = vec![(root, 1_usize)];
    while let Some((node, depth)) = stack.pop() {
        if depth > limit {
            return true;
        }
        stack.extend(node.children().map(|child| (child, depth + 1)));
    }
    false
}
