//! How deep the tree is that the sanitizer's HTML parser builds from a piece of HTML, found with that parser itself.
//!
//! The sanitizer (ammonia) parses with html5ever, whose tree builder can make far more, and far deeper, elements than
//! the input has tags: formatting elements left open are cloned into later blocks and moved again on each misnested end
//! tag. A deep tree makes the parse grow with the square of its input and endangers everything after it. Here the same
//! parser, with the same options and the same `div` context as the sanitizer, builds into a sink that keeps only each
//! node's depth and links (no attributes, no text), so the depth found is the depth of the sanitizer's tree. The work
//! is bounded, and the input goes in by chunks so that a parse past a bound stops early. The work counted includes what
//! the sanitizer's own tree pays on top of the tree's shape (sibling scans, attribute copies and compares), and the
//! attributes of each tag are counted before the parser reads it.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};

use html5ever::interface::{Attribute, ElementFlags, NodeOrText, QualName, QuirksMode, TreeSink};
use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::{local_name, ns, parse_fragment, ParseOpts};

/// Input handed to the parser at once: past a bound, the parse ends after the chunk it reads. Small, so that the work
/// one chunk may still cause after a bound is passed stays small.
pub(super) const CHUNK: usize = 1024;

/// Attributes one tag may hold. The tokenizer checks each new attribute name of a tag against every earlier one before
/// the tree builder sees the tag (work that grows with the square of the count, out of reach of any bound kept by the
/// sink), and the builder copies and sorts the attributes of formatting elements again and again. Hand-written and
/// tool-made tags stay far below this.
const MAX_ATTRS: usize = 256;

/// Work allowed on top of one unit per input byte. A node made from a tag needs at least three input bytes (`<b>`), an
/// attribute two (` a`), and the elements the builder adds by itself for a tag are few (a `<td>` straight in a table:
/// `tbody` and `tr`), so a tree from input the builder does not multiply stays under one unit per byte; moving a subtree
/// visits each node in it once. More work than bytes means the builder multiplies the input (formatting elements cloned
/// again and again). The slack keeps tiny inputs far from the bound: room for one chain of formatting elements nested to
/// the depth limit, each compared with every one before it (256 x 255 / 2 compares, an attribute on each side).
const SLACK: usize = 100_000;

/// The deepest element nesting in the tree the sanitizer's parser builds from `html` (a top-level element is 1), or
/// `usize::MAX` once that passes `limit` or the parse does more work than its input can explain.
pub(super) fn deepest(html: &str, limit: usize) -> usize {
    // the fragment's tree sits under the document node and an `html` root: two levels more than the depth asked for
    let sink = Depths::new(html.len().saturating_add(SLACK), limit.saturating_add(1));
    let context = QualName::new(None, ns!(html), local_name!("div"));
    let mut parser = parse_fragment(sink, ParseOpts::default(), context, Vec::new(), false);
    let mut attrs = AttrCount::default();
    let mut rest = html;
    while !rest.is_empty() {
        if parser.tokenizer.sink.sink.stopped() {
            return usize::MAX;
        }
        let (chunk, tail) = rest.split_at(rest.ceil_char_boundary(CHUNK));
        if !attrs.read(chunk) {
            return usize::MAX;
        }
        parser.tokenizer.sink.sink.formatting_tags(attrs.formatting);
        parser.process(StrTendril::from_slice(chunk));
        rest = tail;
    }
    parser.tokenizer.end();
    parser.tokenizer.sink.sink.verdict()
}

/// A place inside a tag, in the tokenizer's tag states (spec "tag open" to "self-closing start tag").
#[derive(Clone, Copy, PartialEq, Eq)]
enum InTag {
    Open,
    EndOpen,
    Name,
    BeforeAttr,
    AttrName,
    AfterAttr,
    BeforeValue,
    Quoted(u8),
    Unquoted,
    AfterQuoted,
    SelfClosing,
}

impl InTag {
    /// The place after `byte` and whether `byte` starts an attribute; `None` once the tag ended or was none.
    fn after(self, byte: u8) -> Option<(Self, bool)> {
        use InTag::{
            AfterAttr, AfterQuoted, AttrName, BeforeAttr, BeforeValue, EndOpen, Name, Open, Quoted,
            SelfClosing, Unquoted,
        };
        // a CR reaches the tokenizer as a line feed
        let space = matches!(byte, b'\t' | b'\n' | b'\x0C' | b'\r' | b' ');
        let next = match self {
            Open | EndOpen if byte.is_ascii_alphabetic() => Name,
            Open if byte == b'/' => EndOpen,
            Open | EndOpen => return None,
            Quoted(quote) if byte == quote => AfterQuoted,
            Quoted(_) => self,
            _ if byte == b'>' => return None,
            Name | BeforeAttr | SelfClosing | AfterQuoted | Unquoted if space => BeforeAttr,
            AttrName | AfterAttr if space => AfterAttr,
            BeforeValue if space => BeforeValue,
            Name | BeforeAttr | AttrName | AfterAttr | AfterQuoted | SelfClosing
                if byte == b'/' =>
            {
                SelfClosing
            }
            AttrName | AfterAttr if byte == b'=' => BeforeValue,
            Name | AttrName | Unquoted => self,
            BeforeValue if byte == b'"' || byte == b'\'' => Quoted(byte),
            BeforeValue => Unquoted,
            BeforeAttr | AfterAttr | AfterQuoted | SelfClosing => return Some((AttrName, true)),
        };
        Some((next, false))
    }
}

/// Counts the attributes of each tag before the tokenizer reads it, up to [`MAX_ATTRS`]. The tag states are followed
/// as the tokenizer runs them; its other states (comments, raw text, CDATA: the tree builder picks some of them) are
/// not, so a tag may start at any `<`. Every reading that may be the tokenizer's is followed at once, and readings at
/// the same place keep the larger count: the count can only come out above the tokenizer's.
#[derive(Default)]
struct AttrCount {
    /// The tags that may be open, each place once, with its attribute count.
    open: Vec<(InTag, usize)>,
    next: Vec<(InTag, usize)>,
    /// Start tags read so far that may make a formatting element (at least as many as the tokenizer reads).
    formatting: usize,
}

impl AttrCount {
    /// Reads the next piece of the input; `false` once a tag may hold more than [`MAX_ATTRS`] attributes.
    fn read(&mut self, chunk: &str) -> bool {
        let bytes = chunk.as_bytes();
        let mut at = 0;
        while at < bytes.len() {
            match self.open.as_slice() {
                // in text no tag is open until the next `<`
                [] => {
                    while !chunk.is_char_boundary(at) {
                        at += 1;
                    }
                    match chunk[at..].find('<') {
                        Some(lt) => at += lt,
                        None => return true,
                    }
                }
                // a quoted value runs to its quote; a `<` in it may still start a tag
                &[(InTag::Quoted(quote), _)] => {
                    while at < bytes.len() && bytes[at] != quote && bytes[at] != b'<' {
                        at += 1;
                    }
                    if at == bytes.len() {
                        return true;
                    }
                }
                _ => {}
            }
            let byte = bytes[at];
            at += 1;
            self.next.clear();
            for &(place, count) in &self.open {
                let Some((after, starts)) = place.after(byte) else {
                    continue;
                };
                let count = count + usize::from(starts);
                if count > MAX_ATTRS {
                    return false;
                }
                keep_larger(&mut self.next, after, count);
            }
            if byte == b'<' {
                keep_larger(&mut self.next, InTag::Open, 0);
                self.formatting += usize::from(may_open_formatting(&bytes[at..]));
            }
            std::mem::swap(&mut self.open, &mut self.next);
        }
        true
    }
}

/// Whether the bytes after a `<` may be the start tag of a formatting element: its name (any case) then the end of the
/// name, or the end of the chunk anywhere in a name.
fn may_open_formatting(after: &[u8]) -> bool {
    const NAMES: [&[u8]; 14] = [
        b"a", b"b", b"big", b"code", b"em", b"font", b"i", b"nobr", b"s", b"small", b"strike",
        b"strong", b"tt", b"u",
    ];
    NAMES.iter().any(|name| match after.get(..name.len()) {
        Some(head) => {
            head.eq_ignore_ascii_case(name)
                && after.get(name.len()).is_none_or(|&end| {
                    matches!(end, b'\t' | b'\n' | b'\x0C' | b'\r' | b' ' | b'/' | b'>')
                })
        }
        None => after.eq_ignore_ascii_case(&name[..after.len()]),
    })
}

/// Adds a reading at `place`, or raises the count of the one already there.
fn keep_larger(readings: &mut Vec<(InTag, usize)>, place: InTag, count: usize) {
    match readings.iter_mut().find(|(p, _)| *p == place) {
        Some((_, kept)) => *kept = (*kept).max(count),
        None => readings.push((place, count)),
    }
}

/// A node handle: its index in [`Tree::nodes`] and what the builder asks back about it.
#[derive(Clone)]
struct Handle {
    id: usize,
    name: QualName,
    html_point: bool,
}

/// Where a node sits. The order of siblings does not matter for depth: a new child goes first.
#[derive(Clone, Copy, Default)]
struct Node {
    depth: usize,
    /// Levels the node adds below its parent: 1 for an element, 0 for a comment or a template's contents.
    weight: usize,
    /// For a formatting element (`b`, `a`, `code`...): 1 plus its attribute count, what a later formatting element pays
    /// to be compared with it. 0 for any other node.
    format: usize,
    /// Nodes linked below it: the sanitizer's tree scans them to find a child's place.
    children: usize,
    parent: Option<usize>,
    first: Option<usize>,
    next: Option<usize>,
    prev: Option<usize>,
}

struct Tree {
    nodes: Vec<Node>,
    deepest: usize,
    work: usize,
    budget: usize,
    most: usize,
    over: bool,
    /// Attributes given to the `html` and `body` elements, at creation or by stray tags later.
    root_attrs: usize,
    /// The formatting element just made, compared with the open ones once it is placed.
    fresh: Option<usize>,
    /// Start tags in the input so far that may make a formatting element, and the elements charged for one. Only those
    /// are compared: the builder also makes formatting elements again by itself (cloned, counted as new nodes), and
    /// those are never compared.
    tags: usize,
    compared: usize,
}

impl Tree {
    fn spend(&mut self, units: usize) {
        self.work = self.work.saturating_add(units);
        self.over |= self.work > self.budget;
    }

    fn unlink(&mut self, id: usize) {
        let Node {
            parent, next, prev, ..
        } = self.nodes[id];
        match (prev, parent) {
            (Some(prev), _) => self.nodes[prev].next = next,
            (None, Some(parent)) => self.nodes[parent].first = next,
            (None, None) => {}
        }
        if let Some(next) = next {
            self.nodes[next].prev = prev;
        }
        if let Some(parent) = parent {
            let children = &mut self.nodes[parent].children;
            *children = children.saturating_sub(1);
        }
        let node = &mut self.nodes[id];
        (node.parent, node.next, node.prev) = (None, None, None);
    }

    fn link(&mut self, id: usize, parent: usize) {
        self.unlink(id);
        let first = self.nodes[parent].first;
        if let Some(first) = first {
            self.nodes[first].prev = Some(id);
        }
        let node = &mut self.nodes[id];
        (node.parent, node.next) = (Some(parent), first);
        self.nodes[parent].first = Some(id);
        self.nodes[parent].children += 1;
        self.settle(id);
        if self.fresh == Some(id) {
            self.fresh = None;
            if self.compared < self.tags {
                self.compared += 1;
                self.compare(id);
            }
        }
    }

    /// The parser compares a formatting element made for a start tag with each open one after the last marker, copying
    /// and sorting the attributes of both when the names match. The open ones are ancestors of the place it goes to:
    /// one unit for each formatting ancestor plus the attributes of both. The walk up is bounded by the depth limit.
    fn compare(&mut self, id: usize) {
        if self.over {
            return;
        }
        let own = self.nodes[id].format.saturating_sub(1);
        let mut cost = 0_usize;
        let mut at = self.nodes[id].parent;
        while let Some(node) = at {
            let Node { format, parent, .. } = self.nodes[node];
            if format > 0 {
                cost = cost.saturating_add(format + own);
            }
            at = parent;
        }
        self.spend(cost);
    }

    /// The sanitizer's tree finds a child's place by a scan of its parent's children: a removal or an insertion before
    /// a sibling costs up to their number.
    fn scan_children(&mut self, parent: usize) {
        self.spend(self.nodes[parent].children);
    }

    /// Gives `top` the depth its parent asks for and carries a change down its subtree, one unit of work per node.
    fn settle(&mut self, top: usize) {
        let mut at = top;
        while !self.over {
            let node = self.nodes[at];
            let depth = node.parent.map_or(0, |p| self.nodes[p].depth) + node.weight;
            if at == top && depth == node.depth {
                // every node below keeps its depth relative to its parent: nothing else changes
                return;
            }
            self.nodes[at].depth = depth;
            self.deepest = self.deepest.max(depth);
            self.over |= depth > self.most;
            self.spend(1);
            if let Some(first) = node.first {
                at = first;
                continue;
            }
            // the next node of the subtree: a sibling of this node or of the nearest ancestor that has one
            loop {
                if at == top {
                    return;
                }
                match (self.nodes[at].next, self.nodes[at].parent) {
                    (Some(next), _) => {
                        at = next;
                        break;
                    }
                    (None, Some(parent)) => at = parent,
                    (None, None) => return,
                }
            }
        }
    }
}

/// The sink: [`Tree`] behind a `RefCell` (the builder calls with `&self`; no call reenters another).
struct Depths {
    tree: RefCell<Tree>,
    /// A borrow failed: the tree may miss a change, so the answer is "too deep".
    broken: Cell<bool>,
    blank: QualName,
}

impl Depths {
    fn new(budget: usize, most: usize) -> Self {
        let tree = Tree {
            nodes: vec![Node::default()],
            deepest: 0,
            work: 0,
            budget,
            most,
            over: false,
            root_attrs: 0,
            fresh: None,
            tags: 0,
            compared: 0,
        };
        Self {
            tree: RefCell::new(tree),
            broken: Cell::new(false),
            blank: QualName::new(None, ns!(), local_name!("")),
        }
    }

    /// Start tags read so far that may make a formatting element, before the parser reads them.
    fn formatting_tags(&self, total: usize) {
        self.edit(|tree| tree.tags = total);
    }

    fn stopped(&self) -> bool {
        self.broken.get() || self.tree.try_borrow().map_or(true, |tree| tree.over)
    }

    fn verdict(&self) -> usize {
        match self.tree.try_borrow() {
            Ok(tree) if !tree.over && !self.broken.get() => tree.deepest.saturating_sub(1),
            _ => usize::MAX,
        }
    }

    /// Runs a change on the tree, unless the answer is already known: a bound passed (no more work), or a borrow failed
    /// (the handles made since may not name real nodes).
    fn edit(&self, change: impl FnOnce(&mut Tree)) {
        match self.tree.try_borrow_mut() {
            Ok(mut tree) if !tree.over && !self.broken.get() => change(&mut tree),
            Ok(_) => {}
            Err(_) => self.broken.set(true),
        }
    }

    /// A new node: always made (the builder compares handles by id), counted as one unit of work plus `copied` (the
    /// attributes the parser hands over with it).
    fn add(&self, node: Node, copied: usize) -> usize {
        let Ok(mut tree) = self.tree.try_borrow_mut() else {
            self.broken.set(true);
            return 0;
        };
        tree.nodes.push(node);
        tree.spend(copied.saturating_add(1));
        tree.nodes.len() - 1
    }

    fn add_plain(&self, weight: usize) -> usize {
        self.add(
            Node {
                weight,
                ..Node::default()
            },
            0,
        )
    }

    fn other(&self, id: usize) -> Handle {
        Handle {
            id,
            name: self.blank.clone(),
            html_point: false,
        }
    }
}

impl TreeSink for Depths {
    type Handle = Handle;
    type Output = usize;
    type ElemName<'a> = &'a QualName;

    fn finish(self) -> usize {
        self.verdict()
    }

    fn parse_error(&self, _msg: Cow<'static, str>) {}

    fn get_document(&self) -> Handle {
        self.other(0)
    }

    fn elem_name<'a>(&'a self, target: &'a Handle) -> &'a QualName {
        &target.name
    }

    fn create_element(&self, name: QualName, attrs: Vec<Attribute>, flags: ElementFlags) -> Handle {
        let format = if is_formatting(&name) {
            attrs.len().saturating_add(1)
        } else {
            0
        };
        let node = Node {
            weight: 1,
            format,
            ..Node::default()
        };
        let id = self.add(node, attrs.len());
        let root =
            name.ns == ns!(html) && matches!(name.local, local_name!("html") | local_name!("body"));
        self.edit(|tree| {
            if format > 0 {
                tree.fresh = Some(id);
            }
            if root {
                tree.root_attrs = tree.root_attrs.saturating_add(attrs.len());
            }
        });
        if flags.template {
            // its contents come right after it (see `get_template_contents`), one level below it like children
            let contents = self.add_plain(0);
            self.edit(|tree| tree.link(contents, id));
        }
        Handle {
            id,
            name,
            html_point: flags.mathml_annotation_xml_integration_point,
        }
    }

    fn create_comment(&self, _text: StrTendril) -> Handle {
        self.other(self.add_plain(0))
    }

    fn create_pi(&self, _target: StrTendril, _data: StrTendril) -> Handle {
        self.other(self.add_plain(0))
    }

    fn append(&self, parent: &Handle, child: NodeOrText<Handle>) {
        if let NodeOrText::AppendNode(child) = child {
            self.edit(|tree| tree.link(child.id, parent.id));
        }
    }

    fn append_based_on_parent_node(
        &self,
        element: &Handle,
        prev: &Handle,
        child: NodeOrText<Handle>,
    ) {
        let placed = self.tree.try_borrow().map(|tree| {
            tree.nodes
                .get(element.id)
                .is_some_and(|n| n.parent.is_some())
        });
        match placed {
            Ok(true) => self.append_before_sibling(element, child),
            Ok(false) => self.append(prev, child),
            Err(_) => self.broken.set(true),
        }
    }

    fn append_doctype_to_document(
        &self,
        _name: StrTendril,
        _public: StrTendril,
        _system: StrTendril,
    ) {
    }

    fn get_template_contents(&self, target: &Handle) -> Handle {
        self.other(target.id + 1)
    }

    fn same_node(&self, x: &Handle, y: &Handle) -> bool {
        x.id == y.id
    }

    fn set_quirks_mode(&self, _mode: QuirksMode) {}

    fn append_before_sibling(&self, sibling: &Handle, child: NodeOrText<Handle>) {
        self.edit(|tree| {
            let Some(parent) = tree.nodes[sibling.id].parent else {
                // the builder never asks this of a node without a parent; were it to, the depth would be unknown
                tree.over = true;
                return;
            };
            // text too: it is merged into the node before the sibling, found by the same scan
            tree.scan_children(parent);
            if let NodeOrText::AppendNode(child) = child {
                tree.link(child.id, parent);
            }
        });
    }

    fn add_attrs_if_missing(&self, _target: &Handle, attrs: Vec<Attribute>) {
        // the sanitizer's tree gathers the names of every attribute the element holds before it adds the missing ones
        self.edit(|tree| {
            tree.root_attrs = tree.root_attrs.saturating_add(attrs.len());
            tree.spend(tree.root_attrs);
        });
    }

    fn remove_from_parent(&self, target: &Handle) {
        self.edit(|tree| {
            if let Some(parent) = tree.nodes[target.id].parent {
                tree.scan_children(parent);
            }
            tree.unlink(target.id);
        });
    }

    fn reparent_children(&self, node: &Handle, new_parent: &Handle) {
        if node.id == new_parent.id {
            return;
        }
        self.edit(|tree| {
            while let Some(child) = tree.nodes[node.id].first {
                if tree.over {
                    return;
                }
                tree.spend(1);
                tree.link(child, new_parent.id);
            }
        });
    }

    fn is_mathml_annotation_xml_integration_point(&self, handle: &Handle) -> bool {
        handle.html_point
    }
}

/// Elements the parser keeps in its list of active formatting elements (spec "formatting" tags).
fn is_formatting(name: &QualName) -> bool {
    name.ns == ns!(html)
        && matches!(
            name.local,
            local_name!("a")
                | local_name!("b")
                | local_name!("big")
                | local_name!("code")
                | local_name!("em")
                | local_name!("font")
                | local_name!("i")
                | local_name!("nobr")
                | local_name!("s")
                | local_name!("small")
                | local_name!("strike")
                | local_name!("strong")
                | local_name!("tt")
                | local_name!("u")
        )
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;

    use super::*;

    fn depth(html: &str) -> usize {
        deepest(html, usize::MAX - 1)
    }

    #[test]
    fn depth_counts_the_elements_the_parser_nests() {
        assert_eq!(depth(""), 0);
        assert_eq!(depth("a < b and <3"), 0);
        assert_eq!(depth("<p>x</p>"), 1);
        assert_eq!(depth("<div><div><p>x</p></div></div>"), 3);
        // an element without content is still one level below its parent; comments are none
        assert_eq!(depth("<p><br></p><!-- <div><div> -->"), 2);
        assert_eq!(depth(&"<div>".repeat(1000)), 1000);
        // what the parser closes by itself
        assert_eq!(depth(&"<p>a<p>b".repeat(500)), 1);
        assert_eq!(depth(&"<div/>".repeat(50)), 50);
        assert_eq!(depth(&format!("<svg>{}</svg>", "<path/>".repeat(1000))), 2);
        assert_eq!(depth("<textarea></div></textarea><div>"), 1);
        assert_eq!(depth("<style><div><div></style>"), 1);
        // cells left open close with their table; a `td` straight in a table gets its `tbody` and `tr`
        assert_eq!(depth(&"<table><td>x</table>".repeat(100)), 4);
        // a template's contents count as its children
        assert_eq!(depth("<template><p><b>x</b></p></template>"), 3);
    }

    #[test]
    fn past_the_limit_or_the_work_bound_the_answer_is_too_deep() {
        assert_eq!(deepest("<div><div>", 2), 2);
        assert_eq!(deepest("<div><div><div>", 2), usize::MAX);
        assert_eq!(deepest(&"<div>".repeat(100_000), 256), usize::MAX);
        // formatting elements cloned into each paragraph: depth and clones grow with the paragraphs
        let distinct = (0..2000).fold(String::new(), |mut all, i| {
            let _ = write!(all, "<p><b x={i}>a</p>");
            all
        });
        assert_eq!(deepest(&distinct, 256), usize::MAX);
        // 200 formatting elements cloned into each later paragraph: shallow enough, but far more nodes than bytes
        let open = (0..200).fold(String::from("<p>"), |mut all, i| {
            let _ = write!(all, "<b x={i}>");
            all
        });
        let shallow = format!("{open}</p>{}", "<p>x</p>".repeat(20));
        assert_eq!(depth(&shallow), 201);
        let many = format!("{open}</p>{}", "<p>x</p>".repeat(500));
        assert_eq!(depth(&many), usize::MAX);
    }

    /// `count` pieces of `each(i)`, one after the other.
    fn run(count: usize, each: impl Fn(usize) -> String) -> String {
        (0..count).map(each).collect()
    }

    #[test]
    fn nodes_put_before_a_table_count_the_siblings_passed() {
        // the sanitizer's tree finds a node's place among its siblings by a scan: each node put before the table
        // passes every one put there before it
        assert_eq!(
            deepest(&format!("<table>{}", "<br>".repeat(20_000)), 256),
            usize::MAX
        );
        // a stray node or two in a table after many siblings is fine
        let page = format!(
            "{}<table><br>x<br><tr><td>y</table>",
            "<p>x</p>".repeat(1000)
        );
        assert_eq!(depth(&page), 4);
    }

    #[test]
    fn stray_html_tags_count_the_attributes_already_on_the_root() {
        // each stray `<html>` adds its attributes to the root after a look at every attribute the root holds
        assert_eq!(depth(&run(5000, |i| format!("<html a{i}>"))), usize::MAX);
        assert_eq!(depth(&run(20, |i| format!("<html a{i}><p>x</p>"))), 1);
    }

    #[test]
    fn attributes_copied_into_each_rebuilt_element_count() {
        let attrs = run(200, |i| format!(" a{i}"));
        let rebuilt = format!("<p><b{attrs}></p>{}", "<p>x</p>".repeat(5000));
        assert_eq!(depth(&rebuilt), usize::MAX);
        assert_eq!(depth(&format!("<p><b{attrs}>x</b></p>")), 2);
    }

    #[test]
    fn a_new_formatting_element_counts_the_open_ones_it_is_compared_with() {
        // each new `<b>` is compared with every open formatting element, their attributes copied and sorted
        let open = run(250, |i| format!("<b x={i}>"));
        assert_eq!(depth(&format!("{open}x")), 250);
        assert_eq!(
            depth(&format!("{open}{}", "<b></b>".repeat(5000))),
            usize::MAX
        );
        // inline formatting deep in plain blocks is cheap: nothing open to compare with
        let deep = format!(
            "{}{}",
            "<div>".repeat(200),
            "<p><code>a</code> <em>b</em> <a href=c>d</a></p>".repeat(500)
        );
        assert_eq!(depth(&deep), 202);
    }

    fn tag_with(attrs: usize) -> String {
        format!("<div{}>x</div>", run(attrs, |i| format!(" a{i}")))
    }

    #[test]
    fn a_tag_with_too_many_attributes_is_too_deep_before_it_is_parsed() {
        assert_eq!(depth(&tag_with(MAX_ATTRS)), 1);
        assert_eq!(depth(&tag_with(MAX_ATTRS + 1)), usize::MAX);
        // an end tag's attributes are read the same way
        let end = format!("<div></div{}>", run(MAX_ATTRS + 1, |i| format!(" a{i}")));
        assert_eq!(depth(&end), usize::MAX);
        // attributes need no space after a quoted value
        let packed = format!("<div {}>", run(MAX_ATTRS + 1, |i| format!("a{i}=\"v\"")));
        assert_eq!(depth(&packed), usize::MAX);
        // a `>` in a quoted value does not end the tag
        let quoted = tag_with(MAX_ATTRS + 1).replacen(" a0", " a0=\"x > y\"", 1);
        assert_eq!(depth(&quoted), usize::MAX);
        // a quote in a comment, in raw text or in an unquoted value does not hide the tag after it
        for before in [
            "<!-- <x a=\" -->",
            "<style><x a=\"</style>",
            "<p title=x\"y>",
        ] {
            let hidden = format!("{before}{}", tag_with(MAX_ATTRS + 1));
            assert_eq!(depth(&hidden), usize::MAX, "{before}");
        }
        // a tag read across two chunks
        let across = format!("{}{}", "x".repeat(CHUNK - 40), tag_with(MAX_ATTRS + 1));
        assert_eq!(depth(&across), usize::MAX);
        // spaces in a quoted value are no attributes: a long drawing path is fine
        let path = format!("<svg><path d=\"{}\"/></svg>", "M0 0 L1 1 ".repeat(2000));
        assert_eq!(depth(&path), 2);
    }
}
