// `pm_locked`: a block the editor keeps as written (its bytes are written back untouched). At rest it looks as View
// draws its lines: View's blocks (backend HTML, hydrated with View's hooks), View's front matter card, or nothing for
// lines View does not draw. Only what View cannot be matched with shows the source, with the reason and a way to
// Source. A click selects the block; a click on the front matter opens its editable properties in place.

import type { Node as PMNode } from "@milkdown/kit/prose/model";
import { NodeSelection, Plugin } from "@milkdown/kit/prose/state";
import type { EditorView, NodeViewConstructor } from "@milkdown/kit/prose/view";
import { $ctx, $node, $prose, $view } from "@milkdown/kit/utils";
import type { FrontMatter } from "../../../api/generated/FrontMatter";
import type { BlockHooks } from "../../doc/blocks";
import type { LockedRender } from "./lockedRender";

export const LOCKED = "pm_locked";

/** Why a block stays as written (backend reasons + the editor's own). */
export const LOCK_REASONS = [
  "frontMatter",
  "html",
  "math",
  "footnote",
  "diagram",
  "data",
  "definition",
  "kind",
  "parse",
  "reference",
  "differs",
] as const;

export type LockReason = (typeof LOCK_REASONS)[number];

/** The reason a locked node carries (a node made elsewhere without one: `kind`). */
export function reasonOf(node: PMNode): LockReason {
  return LOCK_REASONS.find((reason) => reason === node.attrs.reason) ?? "kind";
}

/** What a locked atom needs from the app to draw itself. */
export type LockedHost = {
  /** View's look of this node (filled before its first paint); null: none known, the source shows. */
  render(node: PMNode): LockedRender | null;
  /** Renders the document again after a locked block's source changed; `render` answers for the new nodes after. */
  refresh(): Promise<void>;
  /** View's hooks for backend blocks: structure at once, embeds and links once near the screen. */
  blocks: BlockHooks;
  /** View's front matter card in `target`; null: no card on offer. Returns its teardown. */
  card(target: HTMLElement, frontMatter: FrontMatter): (() => void) | null;
  /** The front matter as editable properties in `target` (`replace` puts a new source in the node); null: none. */
  properties(
    target: HTMLElement,
    source: string,
    replace: (source: string) => void,
  ): (() => void) | null;
  reasonText(reason: LockReason): string;
  editLabel: string;
  /** "Edit in Source" on this node. */
  edit(node: PMNode): void;
};

const noHost: LockedHost = {
  render: () => null,
  refresh: () => Promise.resolve(),
  blocks: { enhance: (element) => element, near: () => {} },
  card: () => null,
  properties: () => null,
  reasonText: (reason) => reason,
  editLabel: "Source",
  edit: () => {},
};

export const lockedHostCtx = $ctx<LockedHost, "pmLockedHost">(noHost, "pmLockedHost");

export const lockedNode = $node(LOCKED, () => ({
  group: "block",
  atom: true,
  selectable: true,
  draggable: false,
  isolating: true,
  marks: "",
  attrs: {
    // required: ProseMirror never fills a gap with an atom (an emptied document gets a paragraph)
    source: {},
    kind: { default: "other" },
    reason: { default: "kind" },
  },
  toDOM: (node) => ["div", { class: "pm-locked", "data-kind": String(node.attrs.kind) }],
  // made from the file's own text by `buildDoc`, never parsed from Markdown
  parseMarkdown: { match: () => false, runner: () => {} },
  toMarkdown: {
    match: (node) => node.type.name === LOCKED,
    runner: (state, node) => {
      state.addNode("html", undefined, String(node.attrs.source));
    },
  },
}));

/** Controls inside View's blocks (Copy, a diagram's buttons) and inside the properties: they keep their events. */
const CONTROL = "button, input, select, textarea, label";

/** `target` is a control inside `block` (the editor around it is editable, not a control). */
function isControl(block: HTMLElement, target: EventTarget | null): boolean {
  const control = target instanceof Element ? target.closest(CONTROL) : null;
  return control !== null && block.contains(control);
}

/**
 * Backend HTML into `target` as View builds a block: every element is one of View's blocks, made by View's hooks.
 * The `data-b` marks go: the editor's DOM must not pass for View's blocks (features find those by them).
 */
function drawBlocks(target: HTMLElement, html: string, hooks: BlockHooks): () => void {
  const template = document.createElement("template");
  template.innerHTML = html;
  for (const marked of template.content.querySelectorAll("[data-b]")) {
    marked.removeAttribute("data-b");
  }
  const undo: (() => void)[] = [];
  for (const element of [...template.content.children]) {
    if (!(element instanceof HTMLElement)) continue;
    const root = hooks.enhance(element);
    target.append(root);
    const done = hooks.near(root);
    if (done) undo.push(done);
  }
  return () => {
    for (const done of undo) done();
  };
}

/** What the block shows: View's blocks | View's card | nothing | its source. */
type Look = "view" | "card" | "none" | "source";

type State = { kind: "idle" } | { kind: "properties"; opened: string };

/** Opens the block under a node selection (Enter); false when it has nothing to open. */
const openers = new WeakMap<Node, () => boolean>();

export const lockedView = $view(lockedNode, (ctx): NodeViewConstructor => {
  return (initial, view, getPos) => {
    const host = ctx.get(lockedHostCtx.key);
    let node = initial;
    let render = host.render(node);
    let state: State = { kind: "idle" };
    let selected = false;
    let gone = false;
    let teardown: (() => void) | null = null;

    const dom = document.createElement("div");
    dom.className = "pm-locked";
    dom.contentEditable = "false";
    dom.dataset.kind = String(node.attrs.kind);
    dom.dataset.reason = reasonOf(node);

    const sourceLook = (): void => {
      const bar = document.createElement("div");
      bar.className = "pm-locked__bar";
      const why = document.createElement("span");
      why.className = "pm-locked__why";
      why.textContent = host.reasonText(reasonOf(node));
      const button = document.createElement("button");
      button.type = "button";
      button.className = "btn btn--sm btn--ghost pm-locked__edit";
      button.textContent = host.editLabel;
      button.addEventListener("click", () => host.edit(node));
      bar.append(why, button);
      const source = document.createElement("pre");
      source.className = "pm-locked__source";
      source.textContent = String(node.attrs.source);
      dom.append(bar, source);
    };

    const idleLook = (): Look => {
      if (render === null) return "source";
      switch (render.kind) {
        case "html":
          teardown = drawBlocks(dom, render.html, host.blocks);
          return "view";
        case "footnote":
          teardown = drawBlocks(dom, render.html, host.blocks);
          dom.dataset.notes = render.first ? "first" : "next";
          return "view";
        case "frontMatter":
          teardown = host.card(dom, render.frontMatter);
          return teardown === null ? "source" : "card";
        case "none":
          return "none";
        case "promptTag":
        case "source":
          return "source";
        default:
          return render satisfies never;
      }
    };

    const paint = (): void => {
      teardown?.();
      teardown = null;
      dom.replaceChildren();
      delete dom.dataset.notes;
      dom.removeAttribute("tabindex");
      let look: Look;
      if (state.kind === "properties") {
        teardown = host.properties(dom, String(node.attrs.source), replace);
        dom.tabIndex = -1;
        look = "card";
      } else {
        look = idleLook();
        // nothing drawn, but the keyboard selected it: it shows what it holds
        if (look === "none" && selected) look = "source";
      }
      if (look === "source") sourceLook();
      dom.dataset.look = look;
    };

    const replace = (next: string): void => {
      const at = getPos();
      if (at === undefined) return;
      view.dispatch(view.state.tr.setNodeMarkup(at, undefined, { ...node.attrs, source: next }));
    };

    const select = (): void => {
      const at = getPos();
      if (at === undefined) return;
      view.dispatch(view.state.tr.setSelection(NodeSelection.create(view.state.doc, at)));
    };

    /** The source changed (an edit of the properties, undo): the look is rendered again. */
    const rerender = (): void => {
      void host.refresh().then(() => {
        if (gone || state.kind !== "idle") return;
        render = host.render(node);
        paint();
      });
    };

    const outside = (event: Event): void => {
      if (event.target instanceof Node && dom.contains(event.target)) return;
      close(false);
    };

    const open = (): boolean => {
      if (state.kind !== "idle" || render?.kind !== "frontMatter") return false;
      state = { kind: "properties", opened: String(node.attrs.source) };
      paint();
      if (teardown === null) {
        // no plugin reads this front matter as properties
        state = { kind: "idle" };
        paint();
        return false;
      }
      document.addEventListener("pointerdown", outside, true);
      dom.focus({ preventScroll: true });
      return true;
    };

    const close = (refocus: boolean): void => {
      if (state.kind !== "properties") return;
      const changed = state.opened !== String(node.attrs.source);
      state = { kind: "idle" };
      document.removeEventListener("pointerdown", outside, true);
      paint();
      if (refocus) {
        select();
        view.focus();
      }
      if (changed) rerender();
    };

    dom.addEventListener("mousedown", (event) => {
      if (state.kind !== "idle" || event.button !== 0 || isControl(dom, event.target)) return;
      // the block is one thing to select: no caret | text selection inside what View drew
      event.preventDefault();
      select();
      view.focus();
    });
    dom.addEventListener("click", (event) => {
      if (state.kind !== "idle" || event.button !== 0 || isControl(dom, event.target)) return;
      if (event.ctrlKey || event.metaKey) return;
      open();
    });
    dom.addEventListener("keydown", (event) => {
      if (state.kind !== "properties" || event.key !== "Escape") return;
      // a field of the properties takes its own Escape (it puts its value back) and stops it
      if (event.isComposing || event.defaultPrevented) return;
      event.preventDefault();
      event.stopPropagation();
      close(true);
    });

    openers.set(dom, open);
    paint();

    return {
      dom,
      update: (next) => {
        if (next.type !== node.type) return false;
        const changed = String(next.attrs.source) !== String(node.attrs.source);
        node = next;
        if (!changed) return true;
        if (state.kind === "properties") {
          // the properties edit the new source (a value set, the next one starts from it)
          paint();
          dom.focus({ preventScroll: true });
          return true;
        }
        const known = host.render(next);
        if (known !== null) {
          render = known;
          paint();
        } else rerender();
        return true;
      },
      selectNode: () => {
        selected = true;
        dom.classList.add("ProseMirror-selectednode");
        if (dom.dataset.look === "none") paint();
      },
      deselectNode: () => {
        selected = false;
        dom.classList.remove("ProseMirror-selectednode");
        if (state.kind === "idle" && render?.kind === "none") paint();
      },
      stopEvent: (event) => {
        if (state.kind !== "idle") return true;
        if (isControl(dom, event.target)) return true;
        // a click on a link goes on to the editor, which follows it on Ctrl | Cmd + click as in its own text
        return event.type !== "click" && !event.type.startsWith("key");
      },
      // View's hydration (embeds, a details toggled) and the properties change this DOM, never the document
      ignoreMutation: () => true,
      destroy: () => {
        gone = true;
        document.removeEventListener("pointerdown", outside, true);
        teardown?.();
        teardown = null;
      },
    };
  };
});

/** Enter on a selected locked block opens what it offers in place (the front matter's properties). */
export const lockedKeys = $prose(
  () =>
    new Plugin({
      props: {
        handleKeyDown: (view: EditorView, event: KeyboardEvent) => {
          if (event.key !== "Enter" || event.isComposing) return false;
          if (event.ctrlKey || event.metaKey || event.altKey || event.shiftKey) return false;
          const selection = view.state.selection;
          if (!(selection instanceof NodeSelection) || selection.node.type.name !== LOCKED) {
            return false;
          }
          const dom = view.nodeDOM(selection.from);
          return dom !== null && (openers.get(dom)?.() ?? false);
        },
      },
    }),
);
