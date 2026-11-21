//! How deep the tree is that the sanitizer's HTML parser builds from a piece of HTML, found with that parser itself.
//!
//! The sanitizer (ammonia) parses with html5ever, whose tree builder can make far more, and far deeper, elements than
//! the input has tags: formatting elements left open are cloned into later blocks and moved again on each misnested end
//! tag. A deep tree makes the parse grow with the square of its input and endangers everything after it. Here the same
//! parser, with the same options and the same `div` context as the sanitizer, builds into a sink that keeps only each
//! node's depth and links (no attributes, no text), so the depth found is the depth of the sanitizer's tree. The work
//! is bounded, and the input goes in by chunks so that a parse past a bound stops early.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};

use html5ever::interface::{Attribute, ElementFlags, NodeOrText, QualName, QuirksMode, TreeSink};
use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::{local_name, ns, parse_fragment, ParseOpts};

/// Input handed to the parser at once: past a bound, the parse ends after the chunk it reads. Small, so that the work
/// one chunk may still cause after a bound is passed stays small.
const CHUNK: usize = 1024;

/// Work allowed on top of one unit per input byte. A node made from a tag needs at least three input bytes (`<b>`), and
/// the elements the builder adds by itself for a tag are few (a `<td>` straight in a table: `tbody` and `tr`), so a
/// tree from input the builder does not multiply stays under one node per byte; moving a subtree visits each node in it
/// once. More work than bytes means the builder multiplies the input (formatting elements cloned again and again). The
/// slack keeps tiny inputs, where an implied element or two weigh more than their bytes, far from the bound.
const SLACK: usize = 10_000;

/// The deepest element nesting in the tree the sanitizer's parser builds from `html` (a top-level element is 1), or
/// `usize::MAX` once that passes `limit` or the parse does more work than its input can explain.
pub(super) fn deepest(html: &str, limit: usize) -> usize {
    // the fragment's tree sits under the document node and an `html` root: two levels more than the depth asked for
    let sink = Depths::new(html.len().saturating_add(SLACK), limit.saturating_add(1));
    let context = QualName::new(None, ns!(html), local_name!("div"));
    let mut parser = parse_fragment(sink, ParseOpts::default(), context, Vec::new(), false);
    let mut rest = html;
    while !rest.is_empty() {
        if parser.tokenizer.sink.sink.stopped() {
            return usize::MAX;
        }
        let (chunk, tail) = rest.split_at(rest.ceil_char_boundary(CHUNK));
        parser.process(StrTendril::from_slice(chunk));
        rest = tail;
    }
    parser.tokenizer.end();
    parser.tokenizer.sink.sink.verdict()
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
}

impl Tree {
    fn spend(&mut self) {
        self.work += 1;
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
        self.settle(id);
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
            self.spend();
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
        };
        Self {
            tree: RefCell::new(tree),
            broken: Cell::new(false),
            blank: QualName::new(None, ns!(), local_name!("")),
        }
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

    /// A new node: always made (the builder compares handles by id), counted as work.
    fn add(&self, weight: usize) -> usize {
        let Ok(mut tree) = self.tree.try_borrow_mut() else {
            self.broken.set(true);
            return 0;
        };
        tree.nodes.push(Node {
            weight,
            ..Node::default()
        });
        tree.spend();
        tree.nodes.len() - 1
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

    fn create_element(
        &self,
        name: QualName,
        _attrs: Vec<Attribute>,
        flags: ElementFlags,
    ) -> Handle {
        let id = self.add(1);
        if flags.template {
            // its contents come right after it (see `get_template_contents`), one level below it like children
            let contents = self.add(0);
            self.edit(|tree| tree.link(contents, id));
        }
        Handle {
            id,
            name,
            html_point: flags.mathml_annotation_xml_integration_point,
        }
    }

    fn create_comment(&self, _text: StrTendril) -> Handle {
        self.other(self.add(0))
    }

    fn create_pi(&self, _target: StrTendril, _data: StrTendril) -> Handle {
        self.other(self.add(0))
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
        if let NodeOrText::AppendNode(child) = child {
            self.edit(|tree| match tree.nodes[sibling.id].parent {
                Some(parent) => tree.link(child.id, parent),
                // the builder never asks this of a node without a parent; were it to, the depth would be unknown
                None => tree.over = true,
            });
        }
    }

    fn add_attrs_if_missing(&self, _target: &Handle, _attrs: Vec<Attribute>) {}

    fn remove_from_parent(&self, target: &Handle) {
        self.edit(|tree| tree.unlink(target.id));
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
                tree.spend();
                tree.link(child, new_parent.id);
            }
        });
    }

    fn is_mathml_annotation_xml_integration_point(&self, handle: &Handle) -> bool {
        handle.html_point
    }
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
}
