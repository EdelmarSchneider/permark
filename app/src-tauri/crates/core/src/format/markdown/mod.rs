//! Markdown (comrak + ammonia): text -> sanitized blocks, outline, front matter.
//!
//! Pipeline: front matter cut -> prompt-tag markers -> parse -> AST edits (template variables, `@path` references)
//! -> HTML -> prompt markers to `<details>` -> sanitize -> blocks. Line numbers hold through every step: the cuts
//! and markers keep each line in place.

mod blocks;
mod callouts;
mod fences;
mod frontmatter;
mod inline;
mod latex;
mod nesting;
mod prompt;
mod refs;
mod sanitize;
mod sections;
mod segments;
#[cfg(test)]
mod tests;

use std::borrow::Cow;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet};

use std::fmt::Write as _;

use comrak::html::format_node_default;
use comrak::nodes::{AstNode, NodeValue};
use comrak::options::Plugins;
use comrak::{create_formatter, Anchorizer, Arena, Options};

pub(crate) use latex::parse_document;
pub use refs::{at_refs, AtRef};
pub use sections::{sections, units, Section, TextUnit};
pub use segments::{segments, LockReason, Segment, SegmentKind};

use super::{
    Block, Body, Claim, Format, FormatId, FrontMatter, MarkdownFeatures, OutlineItem, RenderCx,
    Rendered,
};

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
    // `\(..\)` and `\[..\]`: the LaTeX delimiters generated text often uses
    ext.math_latex = true;
    // `==marked==`, `:smile:` (GitHub's shortcode names; an unknown name stays text), `^[an inline note]`, `:::kind`
    // containers. Single tildes stay GitHub's strikethrough: no subscript, superscript, underline or spoiler syntax.
    ext.highlight = true;
    ext.shortcodes = true;
    ext.inline_footnotes = true;
    ext.block_directive = true;
    // Heading ids, without a prefix so that `[x](#my-heading)` works as authored.
    ext.header_id_prefix = Some(String::new());
    o.render.sourcepos = true;
    o.render.tasklist_classes = true;
    // the rest of a fence's info string reaches `fences::Fences`, which keeps only its checked extras
    o.render.full_info_string = true;
    // Raw HTML is passed through here and made safe by the sanitizer afterwards.
    o.render.r#unsafe = true;
    o
}

fn render(text: &str, cx: &RenderCx<'_>) -> Rendered {
    if too_many_container_markers(text) {
        return plain(text);
    }
    // 1. front matter, 2. prompt tags
    let cut = frontmatter::find(text);
    let prepared = prepared(text, cut.as_ref(), cx.features);
    let front_matter = cut
        .as_ref()
        .zip(cx.features.front_matter_card)
        .map(|(cut, parse)| FrontMatter {
            syntax: cut.syntax,
            raw: cut.raw.clone(),
            fields: parse(cut.syntax, &cut.raw),
        });
    // 3. parse
    let arena = Arena::new();
    let opts = options();
    let Some(root) = parsed(&arena, &prepared, &opts) else {
        return plain(text);
    };
    // 4. AST edits + outline (after the edits: the ids are the ones the HTML gets)
    if cx.features.prompt_blocks.is_some() {
        mark_inline(root, cx);
    }
    let headings = headings_of(root);
    let boxes = callouts::take(root);
    // 5. HTML
    let mut raw = String::with_capacity(prepared.len() + prepared.len() / 2);
    let mut plugins = Plugins::default();
    plugins.render.codefence_syntax_highlighter = Some(&fences::Fences);
    // Writing into a String cannot fail; an error here would only mean an empty document.
    let _ = PageHtml::format_document_with_plugins(root, &opts, &mut raw, &plugins, boxes);
    let raw = strip_heading_anchors(&raw);
    let raw = match cx.features.prompt_blocks {
        Some(_) => prompt::to_details(&raw).into_owned(),
        None => raw,
    };
    // 6. sanitize + blocks (raw HTML in the document can nest without limit even when the Markdown tree is shallow)
    let scan = blocks::scan(&raw);
    let Some(parts) = clean_parts(&scan.html, &scan.cuts, cx.path) else {
        return plain(text);
    };
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

/// The text the parser reads: the front matter cut (a fenced block when no card shows it), the prompt tags marked
/// when the prompt features are on. Every line keeps its number and every text its column, so a node's source position
/// is its place in `text`.
fn prepared<'t>(
    text: &'t str,
    cut: Option<&frontmatter::Cut<'_>>,
    features: &MarkdownFeatures,
) -> Cow<'t, str> {
    let cut_out: Cow<'_, str> = match cut {
        Some(cut) => {
            let fence = features
                .front_matter_card
                .is_none()
                .then_some(match cut.syntax {
                    super::FrontMatterSyntax::Yaml => "yaml",
                    super::FrontMatterSyntax::Toml => "toml",
                });
            frontmatter::replace(text, cut, fence)
        }
        None => Cow::Borrowed(text),
    };
    match features.prompt_blocks {
        Some(blocks) => match prompt::mark(&cut_out, blocks.tags) {
            Cow::Borrowed(_) => cut_out,
            Cow::Owned(marked) => Cow::Owned(marked),
        },
        None => cut_out,
    }
}

/// [`prepared`] with the front matter `text` opens with (`text` without a byte order mark): what reading parses, for
/// scans that must see the document as reading does.
pub(crate) fn reading_text<'t>(text: &'t str, features: &MarkdownFeatures) -> Cow<'t, str> {
    prepared(text, frontmatter::find(text).as_ref(), features)
}

/// The tree of a [`prepared`] text; `None` when it is too deep to walk and hand on.
fn parsed<'a>(arena: &'a Arena<'a>, prepared: &str, opts: &Options<'_>) -> Option<&'a AstNode<'a>> {
    let root = parse_document(arena, prepared, opts);
    (!is_deeper_than(root, MAX_TREE_DEPTH)).then_some(root)
}

/// Whether `node` sits in a link or an image: its text belongs to that link and is never made another one.
fn in_link<'a>(node: &'a AstNode<'a>) -> bool {
    node.ancestors()
        .skip(1)
        .any(|a| matches!(a.data().value, NodeValue::Link(_) | NodeValue::Image(_)))
}

/// Documents of this size or more are sanitized on several threads.
const PARALLEL_FROM: usize = 256 * 1024;
/// Smallest piece of HTML handed to one sanitizer call.
const PIECE: usize = 32 * 1024;
const MAX_THREADS: usize = 8;

/// Sanitizes the top-level pieces of `raw` (each on its own, so a block never depends on its neighbours) and cuts
/// them into blocks. Big documents use several threads; the result is the same in the same order. `None` when a piece
/// parses into a tree deeper than [`MAX_TREE_DEPTH`]: the depth is measured on each piece as the sanitizer will parse
/// it, before the sanitizer sees it.
fn clean_parts(raw: &str, cuts: &[usize], doc: &str) -> Option<Vec<blocks::RawBlock>> {
    let pieces = blocks::pieces(raw, cuts, PIECE);
    let cleaner = sanitize::Cleaner::new(doc);
    let work = |piece: &&str| {
        (nesting::deepest(piece, MAX_TREE_DEPTH) <= MAX_TREE_DEPTH)
            .then(|| blocks::split(&cleaner.clean(piece)))
    };
    let threads = if raw.len() < PARALLEL_FROM {
        1
    } else {
        std::thread::available_parallelism()
            .map_or(1, std::num::NonZero::get)
            .min(MAX_THREADS)
    };
    if threads < 2 || pieces.len() < 2 {
        let parts = pieces.iter().map(work).collect::<Option<Vec<_>>>()?;
        return Some(parts.into_iter().flatten().collect());
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
            .map(|group| scope.spawn(|| group.iter().map(work).collect::<Option<Vec<_>>>()))
            .collect();
        let mut parts = Vec::new();
        for handle in handles {
            match handle.join() {
                Ok(Some(group)) => parts.extend(group.into_iter().flatten()),
                Ok(None) => return None,
                // a thread that panicked leaves its pieces out
                Err(_) => {}
            }
        }
        Some(parts)
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

// ---- HTML -----------------------------------------------------------------------------------------------------------

create_formatter!(PageHtml<callouts::Boxes>, {
    // GitHub alerts, Obsidian callouts and known `:::` containers draw as one box
    NodeValue::Alert(ref alert) => |context, node, entering| {
        callouts::write(context, node, entering, &callouts::Callout::of_alert(alert))?;
    },
    NodeValue::BlockQuote => |context, node, entering| {
        match callouts::found(&context.user, node) {
            Some(callout) => callouts::write(context, node, entering, &callout)?,
            None => return format_node_default(context, node, entering),
        }
    },
    // a container of no known kind keeps its content in a plain `div` (its info string is no class)
    NodeValue::BlockDirective(ref directive) => |context, node, entering| {
        match callouts::Callout::of_directive(&directive.info) {
            Some(callout) => callouts::write(context, node, entering, &callout)?,
            None if entering => {
                context.cr()?;
                context.write_str("<div")?;
                comrak::html::render_sourcepos(context, node)?;
                context.write_str(">")?;
            }
            None => {
                context.cr()?;
                context.write_str("</div>")?;
                context.lf()?;
            }
        }
    },
    // an emoji says its shortcode on hover (and the visual editor finds it by that)
    NodeValue::ShortCode(ref short) => |context, entering| {
        if entering {
            context.write_str("<span class=\"emoji\" title=\":")?;
            context.escape(&short.code)?;
            write!(context, ":\">{}</span>", short.emoji)?;
        }
    },
});

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
        if !in_link(node) {
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

/// Each heading with the id of the first block holding its line. Headings come in line order: one sweep over the blocks
/// by first line, the blocks whose lines may hold the heading in a heap by place (a block ended before a heading ends
/// before every later one). A heading out of order is searched for from the start.
fn outline_of(
    headings: &[Heading],
    parts: &[blocks::RawBlock],
    ids: &[String],
) -> Vec<OutlineItem> {
    let holds = |p: &blocks::RawBlock, line: u32| p.line_start <= line && line <= p.line_end;
    let mut by_start: Vec<usize> = (0..parts.len()).collect();
    by_start.sort_by_key(|&at| parts[at].line_start);
    let mut started = by_start.iter().peekable();
    let mut open: BinaryHeap<Reverse<usize>> = BinaryHeap::new();
    let mut last_line = 0;
    headings
        .iter()
        .map(|heading| {
            let line = heading.line;
            let found = if line < last_line {
                parts.iter().position(|p| holds(p, line))
            } else {
                last_line = line;
                while let Some(&&at) = started.peek().filter(|&&&at| parts[at].line_start <= line) {
                    open.push(Reverse(at));
                    started.next();
                }
                while open
                    .peek()
                    .is_some_and(|&Reverse(at)| parts[at].line_end < line)
                {
                    open.pop();
                }
                open.peek().map(|&Reverse(at)| at)
            };
            let block = found
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
/// already has its `id` and the UI adds its own anchor, so the extra link is dropped. Only a lone empty link is one:
/// a raw `<a href="#...">` left open in the document runs to some later anchor's `</a>`, and the text between stays.
fn strip_heading_anchors(html: &str) -> String {
    const OPEN: &str = "<a href=\"#";
    const CLOSE: &str = "</a>";
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(at) = rest.find(OPEN) {
        let after = &rest[at..];
        out.push_str(&rest[..at]);
        // its open tag ends at the first `>` (before any `<`: the search stays short), and `</a>` follows at once
        let anchor = after[1..].find(['<', '>']).map(|i| i + 1).filter(|&gt| {
            after[gt..].starts_with('>')
                && after[..gt].ends_with("class=\"anchor\"")
                && after[gt + 1..].starts_with(CLOSE)
        });
        if let Some(gt) = anchor {
            rest = &after[gt + 1 + CLOSE.len()..];
        } else {
            // not an anchor: keep its start and look on after it (a heading anchor may follow inside it)
            out.push_str(OPEN);
            rest = &after[OPEN.len()..];
        }
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
