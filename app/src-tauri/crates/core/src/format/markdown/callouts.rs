//! Callouts: GitHub alerts, Obsidian callouts (`> [!kind]`, an optional `+` | `-` fold mark and a title on the
//! marker's line) and `:::kind` containers (as Docusaurus, `VitePress` and `MyST` write them), all drawn as one kind of box. Many kind names
//! share a few looks; the box carries its look and, when it differs, its kind as classes. Its title is the author's or
//! the kind's English name (the UI translates that one). A box that folds is a `<details>`: folding it in the reading
//! view changes nothing in the file.

use std::collections::HashMap;
use std::fmt::{self, Write};

use comrak::html::{render_sourcepos, Context};
use comrak::nodes::{AlertType, AstNode, NodeAlert, NodeValue};

/// The boxes found in a tree, by the start (line, column) of their block quote.
pub(super) type Boxes = HashMap<(usize, usize), Callout>;

/// The looks a box can have (the UI's colors and default icons).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Look {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
    Quote,
}

impl Look {
    fn name(self) -> &'static str {
        match self {
            Self::Note => "note",
            Self::Tip => "tip",
            Self::Important => "important",
            Self::Warning => "warning",
            Self::Caution => "caution",
            Self::Quote => "quote",
        }
    }
}

/// A kind with a name of its own: the name its title and icon go by, its English title, its look.
struct Kind {
    name: &'static str,
    title: &'static str,
    look: Look,
}

const fn kind(name: &'static str, title: &'static str, look: Look) -> Kind {
    Kind { name, title, look }
}

const KINDS: [Kind; 16] = [
    kind("note", "Note", Look::Note),
    kind("tip", "Tip", Look::Tip),
    kind("important", "Important", Look::Important),
    kind("warning", "Warning", Look::Warning),
    kind("caution", "Caution", Look::Caution),
    kind("info", "Info", Look::Note),
    kind("todo", "To do", Look::Note),
    kind("abstract", "Abstract", Look::Note),
    kind("success", "Success", Look::Tip),
    kind("question", "Question", Look::Warning),
    kind("example", "Example", Look::Important),
    kind("failure", "Failure", Look::Caution),
    kind("danger", "Danger", Look::Caution),
    kind("bug", "Bug", Look::Caution),
    kind("quote", "Quote", Look::Quote),
    kind("details", "Details", Look::Quote),
];

/// Other names of known kinds (Obsidian's aliases).
const ALIASES: [(&str, &str); 12] = [
    ("summary", "abstract"),
    ("tldr", "abstract"),
    ("hint", "tip"),
    ("check", "success"),
    ("done", "success"),
    ("help", "question"),
    ("faq", "question"),
    ("attention", "warning"),
    ("fail", "failure"),
    ("missing", "failure"),
    ("error", "danger"),
    ("cite", "quote"),
];

fn known(word: &str) -> Option<&'static Kind> {
    let name = ALIASES
        .iter()
        .find(|(alias, _)| alias.eq_ignore_ascii_case(word))
        .map_or(word, |(_, name)| name);
    KINDS.iter().find(|k| k.name.eq_ignore_ascii_case(name))
}

/// Longest kind name read (a longer `[!...]` is text).
const MAX_KIND: usize = 40;

fn is_kind_word(word: &str) -> bool {
    !word.is_empty()
        && word.len() <= MAX_KIND
        && word
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Whether and how a box folds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Fold {
    No,
    Open,
    Closed,
}

/// One box as drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Callout {
    look: Look,
    /// The kind's class name when it is not the look's.
    kind: Option<&'static str>,
    title: String,
    fold: Fold,
}

impl Callout {
    /// A box of the kind named `word` (any case); an unknown kind looks like a note, titled by its own name.
    fn new(word: &str, title: Option<&str>, fold: Fold) -> Self {
        let title = title.map(str::trim).filter(|t| !t.is_empty());
        match known(word) {
            Some(kind) => Self {
                look: kind.look,
                kind: (kind.name != kind.look.name()).then_some(kind.name),
                title: title.unwrap_or(kind.title).to_owned(),
                fold,
            },
            None => Self {
                look: Look::Note,
                kind: None,
                title: title.map_or_else(|| capitalized(word), str::to_owned),
                fold,
            },
        }
    }

    /// A GitHub alert: comrak keeps a fold mark (`[!NOTE]-`) at the start of the title.
    pub(super) fn of_alert(alert: &NodeAlert) -> Self {
        let word = match alert.alert_type {
            AlertType::Note => "note",
            AlertType::Tip => "tip",
            AlertType::Important => "important",
            AlertType::Warning => "warning",
            AlertType::Caution => "caution",
        };
        let (fold, title) = alert
            .title
            .as_deref()
            .map_or((Fold::No, None), |t| fold_mark(t.trim_start()));
        Self::new(word, title, fold)
    }

    /// An Obsidian callout from the text of its first line: `[!kind]` (an `|` and what follows inside the brackets
    /// ignored), an optional `+` | `-`, then the title; `None` when the line does not open so.
    pub(super) fn of_marker(line: &str) -> Option<Self> {
        let inner = line.strip_prefix("[!")?;
        let end = inner.find(']')?;
        let word = inner[..end].split('|').next().unwrap_or_default().trim();
        if !is_kind_word(word) {
            return None;
        }
        let (fold, title) = fold_mark(&inner[end + 1..]);
        Some(Self::new(word, title, fold))
    }

    /// A `:::` container from its info string: `kind`, `{kind}`, then a title as `[title]` or plain words. Only a
    /// known kind is a box (`details` folds, closed).
    pub(super) fn of_directive(info: &str) -> Option<Self> {
        let info = info.trim();
        let (word, rest) = if let Some(braced) = info.strip_prefix('{') {
            let end = braced.find('}')?;
            (braced[..end].trim(), &braced[end + 1..])
        } else {
            let end = info
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
                .unwrap_or(info.len());
            (&info[..end], &info[end..])
        };
        let kind = known(word)?;
        let rest = rest.trim();
        let title = match rest.strip_prefix('[') {
            Some(bracketed) => bracketed.rfind(']').map(|end| &bracketed[..end]),
            None => Some(rest),
        };
        let fold = if kind.name == "details" {
            Fold::Closed
        } else {
            Fold::No
        };
        Some(Self::new(kind.name, title, fold))
    }
}

/// `+` | `-` right at the start (then a space or nothing) as the fold, and the title after it.
fn fold_mark(rest: &str) -> (Fold, Option<&str>) {
    let fold = match rest.chars().next() {
        Some('+') => Fold::Open,
        Some('-') => Fold::Closed,
        _ => return (Fold::No, Some(rest)),
    };
    let after = &rest[1..];
    if after.is_empty() || after.starts_with(char::is_whitespace) {
        (fold, Some(after))
    } else {
        (Fold::No, Some(rest))
    }
}

fn capitalized(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars.as_str().chars()).collect()
    })
}

fn key<'a>(node: &'a AstNode<'a>) -> (usize, usize) {
    let start = node.data().sourcepos.start;
    (start.line, start.column)
}

/// The callout a block quote opens, with the nodes of its marker line (its first paragraph's inline nodes up to the
/// first line break, and that break); `None` when it opens none.
fn marker_of<'a>(quote: &'a AstNode<'a>) -> Option<(Callout, Vec<&'a AstNode<'a>>)> {
    let paragraph = quote.first_child()?;
    if !matches!(paragraph.data().value, NodeValue::Paragraph) {
        return None;
    }
    let mut line = Vec::new();
    let mut text = String::new();
    for child in paragraph.children() {
        let is_break = matches!(
            child.data().value,
            NodeValue::SoftBreak | NodeValue::LineBreak
        );
        line.push(child);
        if is_break {
            break;
        }
        // an inline node over two lines (emphasis, a link) makes the marker line no line of its own
        if child
            .descendants()
            .any(|d| matches!(d.data().value, NodeValue::SoftBreak | NodeValue::LineBreak))
        {
            return None;
        }
        child.collect_text_append(&mut text);
    }
    Callout::of_marker(&text).map(|callout| (callout, line))
}

/// Whether the block quote `node` is an Obsidian callout.
pub(super) fn is_callout<'a>(node: &'a AstNode<'a>) -> bool {
    matches!(node.data().value, NodeValue::BlockQuote) && marker_of(node).is_some()
}

/// The Obsidian callouts under `root`, their marker lines taken out of the tree (a paragraph left empty goes too).
pub(super) fn take<'a>(root: &'a AstNode<'a>) -> Boxes {
    let quotes: Vec<&AstNode<'_>> = root
        .descendants()
        .filter(|n| matches!(n.data().value, NodeValue::BlockQuote))
        .collect();
    let mut boxes = Boxes::new();
    for quote in quotes {
        let Some((callout, line)) = marker_of(quote) else {
            continue;
        };
        let paragraph = line.first().and_then(|n| n.parent());
        for node in line {
            node.detach();
        }
        if let Some(paragraph) = paragraph.filter(|p| p.first_child().is_none()) {
            paragraph.detach();
        }
        boxes.insert(key(quote), callout);
    }
    boxes
}

/// The box `quote` opens in `boxes`, if any.
pub(super) fn found<'a>(boxes: &Boxes, quote: &'a AstNode<'a>) -> Option<Callout> {
    boxes.get(&key(quote)).cloned()
}

/// The opening (`entering`) or closing HTML of `callout` around the children of `node`.
pub(super) fn write<'a, T>(
    context: &mut Context<'_, '_, T>,
    node: &'a AstNode<'a>,
    entering: bool,
    callout: &Callout,
) -> fmt::Result {
    let (tag, title_tag) = match callout.fold {
        Fold::No => ("div", "p"),
        Fold::Open | Fold::Closed => ("details", "summary"),
    };
    if !entering {
        context.cr()?;
        write!(context, "</{tag}>")?;
        return context.lf();
    }
    context.cr()?;
    write!(
        context,
        "<{tag} class=\"markdown-alert markdown-alert-{}",
        callout.look.name()
    )?;
    if let Some(kind) = callout.kind {
        write!(context, " markdown-alert-kind-{kind}")?;
    }
    context.write_str("\"")?;
    render_sourcepos(context, node)?;
    if callout.fold == Fold::Open {
        context.write_str(" open")?;
    }
    write!(context, ">\n<{title_tag} class=\"markdown-alert-title\">")?;
    context.escape(&callout.title)?;
    write!(context, "</{title_tag}>")?;
    context.lf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_read_kind_fold_and_title() {
        let marker = |line| Callout::of_marker(line);
        assert_eq!(
            marker("[!tip]+ A title"),
            Some(Callout {
                look: Look::Tip,
                kind: None,
                title: "A title".into(),
                fold: Fold::Open
            })
        );
        assert_eq!(
            marker("[!TLDR|wide]-").map(|c| (c.kind, c.title, c.fold)),
            Some((Some("abstract"), "Abstract".into(), Fold::Closed))
        );
        // a sign glued to a word is part of the title
        assert_eq!(
            marker("[!note]-x").map(|c| (c.title, c.fold)),
            Some(("-x".into(), Fold::No))
        );
        for text in ["[!]", "[! ]", "[!a b]", "[!x", "text [!note]", "[!<b>]"] {
            assert_eq!(marker(text), None, "{text}");
        }
        assert!(marker(&format!("[!{}]", "k".repeat(MAX_KIND + 1))).is_none());
    }

    #[test]
    fn directives_need_a_known_kind() {
        let titled = |info| Callout::of_directive(info).map(|c| c.title);
        assert_eq!(titled("tip"), Some("Tip".into()));
        assert_eq!(titled("  warning  Mind it "), Some("Mind it".into()));
        assert_eq!(
            titled("note[Custom [x] title]"),
            Some("Custom [x] title".into())
        );
        assert_eq!(titled("{caution} Hot"), Some("Hot".into()));
        assert_eq!(titled("hint"), Some("Tip".into()));
        for info in ["", "columns", "{.class}", "{note", "danger-zone"] {
            assert_eq!(titled(info), None, "{info:?}");
        }
    }
}
