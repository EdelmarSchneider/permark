// `pm_locked`: a block the editor keeps as written (its bytes are written back untouched). At rest it looks as View
// draws its lines: View's blocks (backend HTML, hydrated with View's hooks), View's front matter card, an edge of
// View's prompt box (the box itself: `promptBoxes.ts`), or nothing for lines View does not draw. Only what View
// cannot be matched with shows the source, with the reason and a way to Source. A click | Enter opens it in place:
// the front matter as its editable properties, any other block as its source in a text box, View's look of that
// text below it. What is typed goes into the document at once (the editor's undo, dirty state and autosave see it);
// leaving the box shows View's look of the new source.

import type { Node as PMNode } from "@milkdown/kit/prose/model";
import { closeHistory, redo, undo } from "@milkdown/kit/prose/history";
import {
  NodeSelection,
  Plugin,
  PluginKey,
  Selection,
  type Transaction,
} from "@milkdown/kit/prose/state";
import type { EditorView, NodeViewConstructor } from "@milkdown/kit/prose/view";
import { $ctx, $node, $prose, $view } from "@milkdown/kit/utils";
import type { FrontMatter } from "../../../api/generated/FrontMatter";
import type { LockReason as BackendLockReason } from "../../../api/generated/LockReason";
import type { BlockHooks } from "../../doc/blocks";
import { sameLook, type LockedRender } from "./lockedRender";

export const LOCKED = "pm_locked";

/** Meta of a transaction made when View's looks came back from a render (what depends on them is found again). */
export const LOOKS_META = "pmLockedLooks";

/** Meta of a transaction made from an opened block: the block renders its text again on its own pause. */
const TYPED_META = "pmLockedTyped";

/** The frame that paints a prompt box: under its open edge, stretched to its close edge (`promptBoxes.ts`). */
const PROMPT_FRAME = "pm-prompt-frame";

/** Why the backend keeps a block as written (mirror of the generated `LockReason`; `satisfies` + the check below keep
 *  it exhaustive, so a reason the backend adds fails to compile until it is listed here). */
const BACKEND_REASONS = [
  "frontMatter",
  "html",
  "math",
  "footnote",
  "diagram",
  "data",
  "definition",
  "kind",
  "parse",
] as const satisfies readonly BackendLockReason[];

type MissingReasons = Exclude<BackendLockReason, (typeof BACKEND_REASONS)[number]>;
export const BACKEND_REASONS_COMPLETE: MissingReasons extends never ? true : false = true;

/** Why a block stays as written: the backend's reasons, then the editor's own. */
export const LOCK_REASONS = [...BACKEND_REASONS, "reference", "differs"] as const;

export type LockReason = (typeof LOCK_REASONS)[number];

/** The reason a locked node carries (a node made elsewhere without one: `kind`). */
export function reasonOf(node: PMNode): LockReason {
  return LOCK_REASONS.find((reason) => reason === node.attrs.reason) ?? "kind";
}

/** What a locked block is, by the name assistive technology gives it. */
export type BlockName =
  "properties" | "html" | "formula" | "footnote" | "link" | "diagram" | "alert" | "other";

function nameOf(node: PMNode): BlockName {
  const reason = reasonOf(node);
  if (reason === "frontMatter") return "properties";
  if (reason === "diagram" || reason === "footnote" || reason === "html") return reason;
  if (reason === "math" || node.attrs.kind === "math") return "formula";
  if (node.attrs.kind === "alert") return "alert";
  return "other";
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
  /** The name of a block at rest. */
  blockLabel(name: BlockName): string;
  /** How a block at rest is opened. */
  openHint: string;
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
  blockLabel: (name) => name,
  openHint: "Enter",
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

/** Positions of the document, `from` to `to`. */
export type Span = { from: number; to: number };

/** A locked block among the top-level blocks `from` to `to` of `doc` (the editor keeps them at the top level). */
function lockedIn(doc: PMNode, from: number, to: number): boolean {
  let found = false;
  doc.nodesBetween(from, Math.min(to, doc.content.size), (node) => {
    if (node.type.name === LOCKED) found = true;
    return false;
  });
  return found;
}

/**
 * `tr` makes, removes or changes a locked block, or changes something that meets one of `spans` (positions in the
 * document `tr` makes).
 */
export function touchesLocked(tr: Transaction, spans: readonly Span[] = []): boolean {
  const maps = tr.mapping.maps;
  for (let i = 0; i < maps.length; i++) {
    const before = tr.docs[i];
    const after = tr.docs[i + 1] ?? tr.doc;
    const rest = tr.mapping.slice(i + 1);
    let touched = false;
    maps[i]?.forEach((oldStart, oldEnd, newStart, newEnd) => {
      if (touched) return;
      if ((before && lockedIn(before, oldStart, oldEnd)) || lockedIn(after, newStart, newEnd)) {
        touched = true;
        return;
      }
      const from = rest.map(newStart, -1);
      const to = rest.map(newEnd, 1);
      touched = spans.some((span) => from <= span.to && to >= span.from);
    });
    if (touched) return true;
  }
  return false;
}

/**
 * The locked blocks of one editor (each repaints at rest when View's look of its node changed); `asked`: a render is
 * asked for once the changes of this turn are in.
 */
type Painters = { repaint: Set<() => void>; asked: boolean };

const paintersOf = new WeakMap<EditorView, Painters>();

function painters(view: EditorView): Painters {
  let found = paintersOf.get(view);
  if (found === undefined) {
    found = { repaint: new Set(), asked: false };
    paintersOf.set(view, found);
  }
  return found;
}

/**
 * View's looks came back from a render (or were taken from one that came late): every block at rest whose look changed
 * repaints (a footnote that became the first gets the rule, a paragraph its new reference), and what depends on the
 * looks (prompt boxes) is found again.
 */
export function relook(view: EditorView): void {
  if (view.isDestroyed) return;
  for (const repaint of painters(view).repaint) repaint();
  view.dispatch(view.state.tr.setMeta(LOOKS_META, true));
}

/** The whole text rendered again, then the blocks repainted. */
function rerender(view: EditorView, host: LockedHost): void {
  void host.refresh().then(
    () => relook(view),
    () => {},
  );
}

/** As `rerender`, once the changes of this turn are in (several in a row ask once). */
function rerenderSoon(view: EditorView, host: LockedHost): void {
  const at = painters(view);
  if (at.asked) return;
  at.asked = true;
  queueMicrotask(() => {
    at.asked = false;
    if (!view.isDestroyed) rerender(view, host);
  });
}

/**
 * After a change of the locked blocks outside an opened one (a block made, removed, given back by undo | redo), the
 * whole text is rendered again: View's look of a block can depend on the others (footnote order, references).
 */
export const lockedRelook = $prose((ctx) => {
  const host = ctx.get(lockedHostCtx.key);
  const key = new PluginKey<number>("pmLockedRelook");
  return new Plugin<number>({
    key,
    state: {
      init: () => 0,
      apply: (tr, changes) =>
        tr.docChanged && tr.getMeta(TYPED_META) !== true && touchesLocked(tr)
          ? changes + 1
          : changes,
    },
    view: () => ({
      update: (view, previous) => {
        if (key.getState(view.state) !== key.getState(previous)) rerenderSoon(view, host);
      },
    }),
  });
});

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

/**
 * An edge of View's prompt box into `target`: the open tag line as View's summary row (in a `details` of View's box
 * classes that paints nothing: the frame does), the close tag line as the box's bottom edge. The tag is text only.
 */
function drawEdge(target: HTMLElement, tag: string, edge: "open" | "close"): void {
  if (edge === "close") return;
  const box = document.createElement("details");
  box.className = "prompt-block";
  box.dataset.tag = tag;
  box.open = true;
  const summary = document.createElement("summary");
  summary.textContent = tag;
  // a click opens the tag line's source: the box never folds in the editor, nor takes the keyboard
  summary.tabIndex = -1;
  summary.addEventListener("click", (event) => event.preventDefault());
  box.append(summary);
  target.append(box, frameOf(tag));
}

function frameOf(tag: string): HTMLElement {
  const frame = document.createElement("div");
  frame.className = PROMPT_FRAME;
  frame.dataset.tag = tag;
  frame.setAttribute("aria-hidden", "true");
  return frame;
}

/**
 * What the block shows: View's blocks | View's card | a prompt box's edge | nothing | its source | its open source |
 * the front matter's editable properties.
 */
type Look = "view" | "card" | "prompt" | "none" | "source" | "open" | "properties";

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
  /** The node the last render that came back was of (null: none since the box opened). */
  rendered: PMNode | null;
};

type State = { kind: "idle" } | { kind: "properties"; opened: string } | Editing;

/** How the box is left: Escape | focus elsewhere | an arrow key out of its first | last line. */
type Exit = "done" | "outside" | "before" | "after";

/** Pause in typing before the preview is rendered again (each render is one of the whole text). */
export const PREVIEW_MS = 250;

/** Opens the node under a node selection (Enter); false when it has nothing to open. */
const openers = new WeakMap<Node, () => boolean>();

/** Enter on the node drawn by `dom`, selected, calls `open` (`lockedKeys`). */
export function opensOnEnter(dom: Node, open: () => boolean): void {
  openers.set(dom, open);
}

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
export function showText(field: HTMLTextAreaElement | HTMLInputElement, text: string): void {
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
          drawEdge(dom, render.tag, render.edge);
          return "prompt";
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
      // the open tag line of a prompt box still paints the box around its blocks
      if (render?.kind === "promptTag" && render.edge === "open") dom.append(frameOf(render.tag));
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
      // which edge of a prompt box it is, opened or not (the box is laid out by it)
      if (render?.kind === "promptTag") dom.dataset.edge = render.edge;
      else delete dom.dataset.edge;
      // one named thing to assistive technology; opened, its box | fields say how to finish
      dom.setAttribute("role", "group");
      dom.setAttribute("aria-label", host.blockLabel(nameOf(node)));
      if (state.kind === "idle") dom.setAttribute("aria-description", host.openHint);
      else dom.removeAttribute("aria-description");
      let look: Look;
      if (state.kind === "properties") {
        teardown = host.properties(dom, String(node.attrs.source), replace);
        dom.tabIndex = -1;
        look = "properties";
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
      const tr = view.state.tr.setNodeMarkup(at, undefined, { ...node.attrs, source: next });
      // rendered again once the properties close
      view.dispatch(tr.setMeta(TYPED_META, true));
    };

    const select = (): void => {
      const at = getPos();
      if (at === undefined) return;
      view.dispatch(view.state.tr.setSelection(NodeSelection.create(view.state.doc, at)));
    };

    /** At rest: View's look of the node as known now, drawn when it differs from the one shown. */
    const repaint = (): void => {
      if (state.kind !== "idle") return;
      const next = host.render(node);
      if (next === render || (next !== null && render !== null && sameLook(next, render))) return;
      render = next;
      paint();
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
      if (changed) rerender(view, host);
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
            editing.rendered = node;
            showPreview(editing, host.render(node));
            relook(view);
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
      view.dispatch(tr.setMeta(TYPED_META, true));
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
      // View's look of the new source when it is known already (the preview, a source seen before)
      const known = host.render(node);
      if (known !== null) render = known;
      paint();
      leave(exit);
      // the text the last render was of is not this one: rendered again (the others may look different with it)
      if (editing.edited !== null && editing.rendered !== node) rerender(view, host);
    };

    const keyed = (editing: Editing, event: KeyboardEvent): void => {
      // 229: a key the IME takes (some browsers say so before the composition starts)
      if (event.isComposing || event.keyCode === 229 || editing.composing) return;
      const mod = event.ctrlKey || event.metaKey;
      const key = event.key.toLowerCase();
      if (event.key === "Escape") closeSource(editing, "done");
      else if (mod && !event.altKey && (key === "z" || key === "y")) {
        // the editor's undo, not the box's own: the box shows the document's text at every step
        (key === "y" || event.shiftKey ? redo : undo)(view.state, (tr) =>
          view.dispatch(tr.setMeta(TYPED_META, true)),
        );
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
        rendered: null,
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
    dom.addEventListener(
      "click",
      (event) => {
        // opened: a link in the preview is never followed (it is inert, but not every webview knows that)
        const link = event.target instanceof Element ? event.target.closest("a[href]") : null;
        if (state.kind !== "idle" && link !== null && dom.contains(link)) event.preventDefault();
      },
      true,
    );
    dom.addEventListener("keydown", (event) => {
      if (state.kind !== "properties" || event.key !== "Escape") return;
      // a field of the properties takes its own Escape (it puts its value back) and stops it
      if (event.isComposing || event.defaultPrevented) return;
      event.preventDefault();
      event.stopPropagation();
      close(true);
    });

    opensOnEnter(dom, open);
    painters(view).repaint.add(repaint);
    paint();

    return {
      dom,
      update: (next) => {
        if (next.type !== node.type) return false;
        const changed = String(next.attrs.source) !== String(node.attrs.source);
        node = next;
        if (!changed) return true;
        if (state.kind === "source") {
          // the box stays (caret, composition): only a text it does not show yet goes in (undo, redo); during a
          // composition the box keeps the person's text, and its end puts that text in (`compositionend`)
          if (!state.composing && state.field.value !== String(next.attrs.source)) {
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
        // View's look of a source seen before at once (undo, redo); the render this change asks for brings the rest
        const known = host.render(next);
        if (known !== null) {
          render = known;
          paint();
        } else rerenderSoon(view, host);
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
        painters(view).repaint.delete(repaint);
        document.removeEventListener("pointerdown", outside, true);
        teardown?.();
        teardown = null;
      },
    };
  };
});

/**
 * Enter on a selected locked block opens it in place (its source | the front matter's properties); on any other
 * selected node that registered an opener (`opensOnEnter`), that opener runs.
 */
export const lockedKeys = $prose(
  () =>
    new Plugin({
      props: {
        handleKeyDown: (view: EditorView, event: KeyboardEvent) => {
          if (event.key !== "Enter" || event.isComposing) return false;
          if (event.ctrlKey || event.metaKey || event.altKey || event.shiftKey) return false;
          const selection = view.state.selection;
          if (!(selection instanceof NodeSelection)) return false;
          const dom = view.nodeDOM(selection.from);
          return dom !== null && (openers.get(dom)?.() ?? false);
        },
      },
    }),
);
