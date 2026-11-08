// `pm_locked`: a block the editor keeps as written (its bytes are written back untouched). At rest it looks as View
// draws its lines: View's blocks (backend HTML, hydrated with View's hooks), View's front matter card, or nothing for
// lines View does not draw. Only what View cannot be matched with shows the source, with the reason and a way to
// Source. A click | Enter opens it in place: the front matter as its editable properties, any other block as its
// source in a text box, View's look of that text below it. What is typed goes into the document at once (the
// editor's undo, dirty state and autosave see it); leaving the box shows View's look of the new source.

import type { Node as PMNode } from "@milkdown/kit/prose/model";
import { closeHistory, redo, undo } from "@milkdown/kit/prose/history";
import { NodeSelection, Plugin, Selection } from "@milkdown/kit/prose/state";
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
  /** Name of the text box an opened block shows its source in. */
  sourceLabel: string;
  /** The short line under that box: how to finish. */
  sourceHint: string;
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
  sourceLabel: "Source",
  sourceHint: "Esc",
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

/** What the block shows: View's blocks | View's card | nothing | its source | its source opened in a text box. */
type Look = "view" | "card" | "none" | "source" | "open";

/** The source opened in place: a text box above View's look of what it holds. */
type Editing = {
  kind: "source";
  field: HTMLTextAreaElement;
  preview: HTMLElement;
  /** Undoes the hydration of what the preview shows. */
  undraw: () => void;
  timer: ReturnType<typeof setTimeout> | undefined;
  /** An IME composition is going on in the box: its text is not the person's yet. */
  composing: boolean;
  /** Time of the last edit put into the document since the box opened (null: none yet). */
  edited: number | null;
};

type State = { kind: "idle" } | { kind: "properties"; opened: string } | Editing;

/** How the box is left: Escape | focus elsewhere | an arrow key out of its first | last line. */
type Exit = "done" | "outside" | "before" | "after";

/** Pause in typing before the preview is rendered again (each render is one of the whole text). */
const PREVIEW_MS = 250;

/** Opens the block under a node selection (Enter); false when it has nothing to open. */
const openers = new WeakMap<Node, () => boolean>();

/** Hints made so far: each box points at its own hint by id. */
let hints = 0;

/** The box as tall as its text: no scrollbar inside it, the page scrolls. */
function fit(field: HTMLTextAreaElement): void {
  field.rows = Math.max(1, field.value.split("\n").length);
  field.style.height = "auto";
  // no layout (a detached box, a test): the rows above are the height
  if (field.scrollHeight > 0) {
    field.style.height = `${field.scrollHeight + field.offsetHeight - field.clientHeight}px`;
  }
}

/** `text` in the box, the caret at the end of what differs from the text it showed (an undo shows what it undid). */
function showText(field: HTMLTextAreaElement, text: string): void {
  const old = field.value;
  let head = 0;
  while (head < old.length && head < text.length && old[head] === text[head]) head += 1;
  let tail = 0;
  while (
    tail < old.length - head &&
    tail < text.length - head &&
    old[old.length - 1 - tail] === text[text.length - 1 - tail]
  ) {
    tail += 1;
  }
  field.value = text;
  field.setSelectionRange(text.length - tail, text.length - tail);
}

/** An arrow key with the caret at the very start (up) | end (down) of the box: the key leaves it. */
function leavesBox(field: HTMLTextAreaElement, key: string): boolean {
  const { selectionStart: start, selectionEnd: end, value } = field;
  if (start !== end) return false;
  return (key === "ArrowUp" && start === 0) || (key === "ArrowDown" && end === value.length);
}

/** The backend HTML the preview shows for `look` (`""`: View draws nothing); null: none known, the last one stays. */
function previewOf(look: LockedRender): string | null {
  switch (look.kind) {
    case "html":
    case "footnote":
      return look.html;
    case "none":
      return "";
    case "frontMatter":
    case "promptTag":
    case "source":
      return null;
    default:
      return look satisfies never;
  }
}

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

    const editorLook = (editing: Editing): void => {
      const hint = document.createElement("div");
      hint.className = "pm-locked__hint";
      hints += 1;
      hint.id = `pm-locked-hint-${hints}`;
      hint.textContent = host.sourceHint;
      editing.field.setAttribute("aria-describedby", hint.id);
      dom.append(editing.field, hint, editing.preview);
      fit(editing.field);
      teardown = () => {
        clearTimeout(editing.timer);
        editing.undraw();
        editing.undraw = () => {};
      };
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
      } else if (state.kind === "source") {
        editorLook(state);
        look = "open";
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

    const openProperties = (): boolean => {
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

    // ---- the source in a text box ------------------------------------------------------------------------------

    /** View's blocks for `look` in the preview, hydrated as View's; a look without them keeps the last one shown. */
    const showPreview = (editing: Editing, look: LockedRender | null): void => {
      const html = look === null ? null : previewOf(look);
      if (html === null) return;
      editing.undraw();
      editing.preview.replaceChildren();
      editing.undraw = drawBlocks(editing.preview, html, host.blocks);
    };

    /** Once typing pauses: the whole text rendered as View does (references resolve), this block's part shown. */
    const schedulePreview = (editing: Editing): void => {
      clearTimeout(editing.timer);
      editing.timer = setTimeout(() => {
        const asked = node;
        void host.refresh().then(
          () => {
            // typed on meanwhile: the next pause renders again
            if (gone || state !== editing || node !== asked) return;
            showPreview(editing, host.render(node));
          },
          () => {},
        );
      }, PREVIEW_MS);
    };

    /** The box's text into the document: one transaction per input (quick ones in a row make one undo step). */
    const commit = (editing: Editing): void => {
      const next = editing.field.value;
      const at = getPos();
      if (state !== editing || at === undefined || next === String(node.attrs.source)) return;
      const tr = view.state.tr.setNodeMarkup(at, undefined, { ...node.attrs, source: next });
      // the block stays the selection: what is typed in the box is its own
      tr.setSelection(NodeSelection.create(tr.doc, at));
      // the first edit starts an undo step of its own: undo never joins it to an edit made before the box opened
      if (editing.edited === null) closeHistory(tr);
      editing.edited = tr.time;
      view.dispatch(tr);
      schedulePreview(editing);
    };

    /** Where the keyboard goes once the box is gone (focus moved elsewhere: where the person put it). */
    const leave = (exit: Exit): void => {
      const at = getPos();
      if (exit === "outside" || at === undefined) return;
      const doc = view.state.doc;
      const next =
        exit === "before"
          ? Selection.findFrom(doc.resolve(at), -1)
          : exit === "after"
            ? Selection.findFrom(doc.resolve(at + node.nodeSize), 1)
            : null;
      view.dispatch(
        view.state.tr.setSelection(next ?? NodeSelection.create(doc, at)).scrollIntoView(),
      );
      view.focus();
    };

    /** An emptied block goes, in the last edit's undo step: one undo brings its text back. */
    const remove = (at: number, editing: Editing, exit: Exit): void => {
      const tr = view.state.tr.delete(at, at + node.nodeSize);
      if (editing.edited !== null) tr.setTime(editing.edited);
      if (exit !== "outside") {
        const $at = tr.doc.resolve(Math.min(at, tr.doc.content.size));
        tr.setSelection(Selection.near($at, exit === "after" ? 1 : -1)).scrollIntoView();
      }
      view.dispatch(tr);
      if (exit !== "outside") view.focus();
    };

    const closeSource = (editing: Editing, exit: Exit): void => {
      if (gone || state !== editing) return;
      // a composition cut short by leaving: what the box shows is what the person typed
      commit(editing);
      state = { kind: "idle" };
      const at = getPos();
      if (at !== undefined && String(node.attrs.source).trim() === "") {
        remove(at, editing, exit);
        return;
      }
      // View's look of the new source when the preview has it already; else it is rendered again
      const known = host.render(node);
      if (known !== null) render = known;
      paint();
      leave(exit);
      if (known === null) rerender();
    };

    const keyed = (editing: Editing, event: KeyboardEvent): void => {
      if (event.isComposing || editing.composing) return;
      const mod = event.ctrlKey || event.metaKey;
      const key = event.key.toLowerCase();
      if (event.key === "Escape") closeSource(editing, "done");
      else if (mod && !event.altKey && (key === "z" || key === "y")) {
        // the editor's undo, not the box's own: the box shows the document's text at every step
        (key === "y" || event.shiftKey ? redo : undo)(view.state, view.dispatch);
      } else if (!mod && !event.altKey && !event.shiftKey && leavesBox(editing.field, event.key)) {
        closeSource(editing, event.key === "ArrowUp" ? "before" : "after");
      } else return;
      event.preventDefault();
      event.stopPropagation();
    };

    const openSource = (): boolean => {
      const field = document.createElement("textarea");
      field.className = "pm-locked__editor";
      field.spellcheck = false;
      field.setAttribute("aria-label", host.sourceLabel);
      field.value = String(node.attrs.source);
      const preview = document.createElement("div");
      preview.className = "pm-locked__preview";
      // a picture of the text: nothing in it is clicked, followed or focused (the keys stay in the box)
      preview.setAttribute("inert", "");
      const editing: Editing = {
        kind: "source",
        field,
        preview,
        undraw: () => {},
        timer: undefined,
        composing: false,
        edited: null,
      };
      field.addEventListener("input", () => {
        fit(field);
        if (!editing.composing) commit(editing);
      });
      // an IME composition is never cut: its text goes into the document once it is chosen
      field.addEventListener("compositionstart", () => {
        editing.composing = true;
      });
      field.addEventListener("compositionend", () => {
        editing.composing = false;
        commit(editing);
      });
      field.addEventListener("keydown", (event) => keyed(editing, event));
      field.addEventListener("blur", () => {
        // the window lost focus, the box keeps it: still open when the person comes back
        if (document.activeElement === field) return;
        closeSource(editing, "outside");
      });
      state = editing;
      paint();
      showPreview(editing, render);
      field.focus({ preventScroll: true });
      field.setSelectionRange(field.value.length, field.value.length);
      return true;
    };

    const open = (): boolean => {
      if (state.kind !== "idle") return false;
      if (render?.kind === "frontMatter" && openProperties()) return true;
      // a prompt box's tag line is one of a pair: it is not opened alone
      if (render?.kind === "promptTag") return false;
      return openSource();
    };

    dom.addEventListener("mousedown", (event) => {
      if (state.kind === "source") {
        // the hint and the preview take no focus: the keys stay in the box
        if (event.target !== state.field) event.preventDefault();
        return;
      }
      if (state.kind !== "idle" || event.button !== 0 || isControl(dom, event.target)) return;
      // the block is one thing to select: no caret | text selection inside what View drew
      event.preventDefault();
      select();
      view.focus();
    });
    dom.addEventListener("click", (event) => {
      if (state.kind !== "idle" || event.button !== 0 || isControl(dom, event.target)) return;
      if (event.ctrlKey || event.metaKey) return;
      // the block opens; a link in it is not followed
      if (open()) event.preventDefault();
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
        if (state.kind === "source") {
          // the box stays (caret, composition): only a text it does not show yet goes in (undo, redo)
          if (state.field.value !== String(next.attrs.source)) {
            showText(state.field, String(next.attrs.source));
            fit(state.field);
            schedulePreview(state);
          }
          return true;
        }
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
      // View's hydration (embeds, a details toggled), the properties and the source box change this DOM, never the
      // document
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

/** Enter on a selected locked block opens it in place (its source | the front matter's properties). */
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
