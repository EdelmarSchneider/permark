// `inline_source`: a piece of a line the editor keeps as written (its exact source is the node's `source`), drawn as
// View draws it, so the text around it stays editable. Kinds: inline math (`inlineMath.ts`). Read from the editor's
// mdast node `inlineSource` and written back as the same bytes. At rest it is one thing to select (an arrow key
// selects it, the next moves past it); a click | Enter opens a small field under it with the editable part of its
// source (a formula's TeX). What is typed goes into the document at once (the editor's undo, dirty state and autosave
// see it) when it still reads as one such piece; the piece is drawn again on each pause. Escape | Enter | focus
// elsewhere closes the field; emptied, the piece goes.

import type { Literal } from "mdast";
import type { Handle } from "mdast-util-to-markdown";
import { parserCtx } from "@milkdown/kit/core";
import type { Ctx } from "@milkdown/kit/ctx";
import { closeHistory, redo, undo } from "@milkdown/kit/prose/history";
import type { Node as PMNode } from "@milkdown/kit/prose/model";
import { NodeSelection, TextSelection } from "@milkdown/kit/prose/state";
import type { NodeViewConstructor } from "@milkdown/kit/prose/view";
import type { RemarkPluginRaw } from "@milkdown/kit/transformer";
import { $node, $remark, $view } from "@milkdown/kit/utils";
import { mathElement, mathFromMarkdown, mathParts, mathSyntax } from "./inlineMath";
import { lockedHostCtx, opensOnEnter, PREVIEW_MS, showText, type BlockName } from "./lockedView";

declare module "mdast" {
  /** A piece of a line kept as written: `value` is its exact source. */
  interface InlineSource extends Literal {
    type: "inlineSource";
    kind: InlineKind;
  }
  interface PhrasingContentMap {
    inlineSource: InlineSource;
  }
  interface RootContentMap {
    inlineSource: InlineSource;
  }
}

export const INLINE_SOURCE = "inline_source";

export const INLINE_KINDS = ["math"] as const;
export type InlineKind = (typeof INLINE_KINDS)[number];

/** The source split into the part the person edits and what stays around it. */
type Parts = { open: string; text: string; close: string };

/** What one kind of piece is to the editor. */
type Kind = {
  /** Its name to assistive technology. */
  name: BlockName;
  /** View's element for `source`, the source only ever as text (View's hooks hydrate it). */
  look(source: string): HTMLElement;
  /** The editable part of `source`; null: nothing to edit in place. */
  parts(source: string): Parts | null;
};

const KINDS: Record<InlineKind, Kind> = {
  math: {
    name: "formula",
    look: mathElement,
    parts: mathParts,
  },
};

function kindOf(value: unknown): InlineKind | null {
  return INLINE_KINDS.find((kind) => kind === value) ?? null;
}

/** Written as its source; what follows a piece sees the source's first character before it. */
function writeSource(node: { value: string }): string {
  return node.value;
}
writeSource.peek = (node: { value: string }): string => node.value.charAt(0);

/**
 * Milkdown's text writing (`text`) with every `$` escaped: Milkdown writes a text that ends in a space as it is,
 * without the escapes (`unsafe` below) any other text gets.
 */
export function escapingDollars(text: Handle | undefined): Handle {
  return (node: { value: string }, parent, state, info) => {
    const written = text ? text(node, parent, state, info) : state.safe(node.value, info);
    return written === node.value ? written.replace(/\$/g, "\\$") : written;
  };
}

const remarkInlineSource: RemarkPluginRaw<undefined> = function () {
  const data = this.data();
  (data.micromarkExtensions ??= []).push(mathSyntax);
  (data.fromMarkdownExtensions ??= []).push(mathFromMarkdown);
  // a `$` typed as text stays text (as `*` and `_` do): written escaped, it never starts a formula on the next read
  (data.toMarkdownExtensions ??= []).push({
    handlers: { inlineSource: writeSource },
    unsafe: [{ character: "$", inConstruct: "phrasing" }],
  });
};

const inlineSourceRemark = $remark("pmInlineSource", () => remarkInlineSource);

/** `source` read by the editor of `ctx` is one piece of `kind` with this very source (a paste, a field's text). */
function readsAsOne(ctx: Ctx, kind: InlineKind, source: string): boolean {
  try {
    const block = ctx.get(parserCtx)(source).firstChild;
    const piece = block?.childCount === 1 ? block.firstChild : null;
    return (
      piece?.type.name === INLINE_SOURCE &&
      piece.attrs.kind === kind &&
      piece.attrs.source === source
    );
  } catch {
    return false;
  }
}

export const inlineSourceNode = $node(INLINE_SOURCE, (ctx) => {
  return {
    group: "inline",
    inline: true,
    atom: true,
    selectable: true,
    draggable: false,
    attrs: { kind: { default: "math" }, source: {} },
    // a copy inside the editor pastes back as the piece; any other such span only as its text
    parseDOM: [
      {
        tag: "span.pm-inline[data-source]",
        getAttrs: (dom) => {
          const kind = kindOf(dom.dataset.kind);
          const source = dom.dataset.source ?? "";
          return kind !== null && readsAsOne(ctx, kind, source) ? { kind, source } : false;
        },
      },
    ],
    toDOM: (node) => [
      "span",
      {
        class: "pm-inline",
        "data-kind": String(node.attrs.kind),
        "data-source": String(node.attrs.source),
      },
      String(node.attrs.source),
    ],
    parseMarkdown: {
      match: ({ type }) => type === "inlineSource",
      runner: (state, node, type) => {
        state.addNode(type, {
          kind: kindOf(node.kind) ?? "math",
          source: String(node.value ?? ""),
        });
      },
    },
    toMarkdown: {
      match: (node) => node.type.name === INLINE_SOURCE,
      runner: (state, node) => {
        state.addNode("inlineSource", undefined, String(node.attrs.source), {
          kind: String(node.attrs.kind),
        });
      },
    },
  };
});

/** A piece of no known kind: its source as text. */
function plainLook(source: string): HTMLElement {
  const shown = document.createElement("span");
  shown.textContent = source;
  return shown;
}

/** The field opened under a piece. */
type Editing = {
  field: HTMLInputElement;
  box: HTMLElement;
  open: string;
  close: string;
  /** An IME composition is going on: its text is not the person's yet. */
  composing: boolean;
  /** Time of the last edit put into the document since the field opened (null: none yet). */
  edited: number | null;
};

/** How the field is left: Escape | Enter, focus elsewhere, an arrow key out of its start | end. */
type Exit = "done" | "outside" | "before" | "after";

/** Hints made so far: each field points at its own by id. */
let hints = 0;

function leavesField(field: HTMLInputElement, key: string): "before" | "after" | null {
  const { selectionStart: start, selectionEnd: end, value } = field;
  if (start !== end) return null;
  if (key === "ArrowLeft" && start === 0) return "before";
  if (key === "ArrowRight" && end === value.length) return "after";
  return null;
}

export const inlineSourceView = $view(inlineSourceNode, (ctx): NodeViewConstructor => {
  return (initial, view, getPos) => {
    const host = ctx.get(lockedHostCtx.key);
    let node = initial;
    let editing: Editing | null = null;
    let gone = false;
    let look: HTMLElement | null = null;
    let undraw: () => void = () => {};
    let timer: ReturnType<typeof setTimeout> | undefined;

    const kind = (): Kind | null => {
      const name = kindOf(node.attrs.kind);
      return name === null ? null : KINDS[name];
    };
    const source = (of: PMNode = node): string => String(of.attrs.source);

    const dom = document.createElement("span");
    dom.className = "pm-inline";
    dom.contentEditable = "false";
    dom.dataset.kind = String(node.attrs.kind);
    dom.setAttribute("role", "group");
    dom.setAttribute("aria-label", host.blockLabel(kind()?.name ?? "other"));

    /** View's look of the source, hydrated as View's. */
    const draw = (): void => {
      clearTimeout(timer);
      timer = undefined;
      undraw();
      const shown = kind()?.look(source()) ?? plainLook(source());
      if (look) look.replaceWith(shown);
      else dom.prepend(shown);
      look = shown;
      const done = host.blocks.near(shown);
      undraw = done ?? (() => {});
    };

    const paintState = (): void => {
      dom.dataset.look = editing === null ? "view" : "open";
      if (editing === null) dom.setAttribute("aria-description", host.openHint);
      else dom.removeAttribute("aria-description");
    };

    /** The source the field's text makes (spaces at its edges left out when they would not read); null: none. */
    const sourceFor = (open: Editing, text: string): string | null => {
      const name = kindOf(node.attrs.kind);
      if (name === null) return null;
      for (const part of [text, text.trim()]) {
        const next = open.open + part + open.close;
        if (part !== "" && readsAsOne(ctx, name, next)) return next;
      }
      return null;
    };

    const select = (): void => {
      const at = getPos();
      if (at === undefined) return;
      view.dispatch(view.state.tr.setSelection(NodeSelection.create(view.state.doc, at)));
    };

    /** The field's text into the document: one transaction per input (quick ones in a row make one undo step). */
    const commit = (open: Editing): void => {
      const text = open.field.value;
      const next = sourceFor(open, text);
      if (next === null && text.trim() !== "") open.field.setAttribute("aria-invalid", "true");
      else open.field.removeAttribute("aria-invalid");
      const at = getPos();
      if (editing !== open || at === undefined || next === null || next === source()) return;
      const tr = view.state.tr.setNodeMarkup(at, undefined, { ...node.attrs, source: next });
      tr.setSelection(NodeSelection.create(tr.doc, at));
      // the first edit starts an undo step of its own: undo never joins it to an edit made before the field opened
      if (open.edited === null) closeHistory(tr);
      open.edited = tr.time;
      view.dispatch(tr);
    };

    const leave = (exit: Exit): void => {
      const at = getPos();
      if (exit === "outside" || at === undefined) return;
      const to = exit === "before" ? at : at + node.nodeSize;
      view.dispatch(
        view.state.tr.setSelection(TextSelection.create(view.state.doc, to)).scrollIntoView(),
      );
      view.focus();
    };

    /** An emptied piece goes, in the last edit's undo step: one undo brings its source back. */
    const remove = (at: number, open: Editing, exit: Exit): void => {
      const tr = view.state.tr.delete(at, at + node.nodeSize);
      if (open.edited !== null) tr.setTime(open.edited);
      if (exit !== "outside") tr.setSelection(TextSelection.create(tr.doc, at)).scrollIntoView();
      view.dispatch(tr);
      if (exit !== "outside") view.focus();
    };

    const close = (open: Editing, exit: Exit): void => {
      if (gone || editing !== open) return;
      // a composition cut short by leaving: what the field shows is what the person typed
      commit(open);
      editing = null;
      open.box.remove();
      paintState();
      const at = getPos();
      if (at !== undefined && open.field.value.trim() === "") {
        remove(at, open, exit);
        return;
      }
      if (timer !== undefined) draw();
      leave(exit);
    };

    const keyed = (open: Editing, event: KeyboardEvent): void => {
      // 229: a key the IME takes (some browsers say so before the composition starts)
      if (event.isComposing || event.keyCode === 229 || open.composing) return;
      const mod = event.ctrlKey || event.metaKey;
      const key = event.key.toLowerCase();
      const plain = !mod && !event.altKey && !event.shiftKey;
      const out = plain ? leavesField(open.field, event.key) : null;
      if (event.key === "Escape" || (event.key === "Enter" && plain)) close(open, "done");
      else if (mod && !event.altKey && (key === "z" || key === "y")) {
        // the editor's undo, not the field's own: the field shows the document's source at every step
        (key === "y" || event.shiftKey ? redo : undo)(view.state, (tr) => view.dispatch(tr));
      } else if (out !== null) close(open, out);
      else return;
      event.preventDefault();
      event.stopPropagation();
    };

    const openField = (): boolean => {
      const parts = editing === null ? kind()?.parts(source()) : null;
      if (!parts) return false;
      const box = document.createElement("span");
      box.className = "pm-inline__box";
      const field = document.createElement("input");
      field.type = "text";
      field.className = "pm-inline__field";
      field.spellcheck = false;
      field.autocomplete = "off";
      field.setAttribute("aria-label", host.blockLabel(kind()?.name ?? "other"));
      field.value = parts.text;
      const hint = document.createElement("span");
      hint.className = "pm-inline__hint";
      hints += 1;
      hint.id = `pm-inline-hint-${hints}`;
      hint.textContent = host.sourceHint;
      field.setAttribute("aria-describedby", hint.id);
      box.append(field, hint);
      const open: Editing = {
        field,
        box,
        open: parts.open,
        close: parts.close,
        composing: false,
        edited: null,
      };
      field.addEventListener("input", () => {
        if (!open.composing) commit(open);
      });
      // an IME composition is never cut: its text goes into the document once it is chosen
      field.addEventListener("compositionstart", () => {
        open.composing = true;
      });
      field.addEventListener("compositionend", () => {
        open.composing = false;
        commit(open);
      });
      field.addEventListener("keydown", (event) => keyed(open, event));
      field.addEventListener("blur", () => {
        // the window lost focus, the field keeps it: still open when the person comes back
        if (document.activeElement === field) return;
        close(open, "outside");
      });
      editing = open;
      dom.append(box);
      paintState();
      field.focus({ preventScroll: true });
      field.setSelectionRange(field.value.length, field.value.length);
      return true;
    };

    dom.addEventListener("mousedown", (event) => {
      if (editing !== null) {
        // the hint and the look take no focus: the keys stay in the field
        if (event.target !== editing.field) event.preventDefault();
        return;
      }
      if (event.button !== 0) return;
      // one thing to select: no caret inside what View drew
      event.preventDefault();
      select();
      view.focus();
    });
    dom.addEventListener("click", (event) => {
      if (editing !== null || event.button !== 0 || event.ctrlKey || event.metaKey) return;
      // a link around it is not followed
      if (openField()) event.preventDefault();
    });

    opensOnEnter(dom, openField);
    draw();
    paintState();

    return {
      dom,
      update: (next) => {
        if (next.type !== node.type || next.attrs.kind !== node.attrs.kind) return false;
        const changed = source(next) !== source();
        node = next;
        if (!changed) return true;
        if (editing === null) {
          draw();
          return true;
        }
        // the field stays (caret, composition): only a source it does not show yet goes in (undo, redo)
        const parts = kind()?.parts(source());
        if (!editing.composing && parts && sourceFor(editing, editing.field.value) !== source()) {
          showText(editing.field, parts.text);
          editing.field.removeAttribute("aria-invalid");
        }
        clearTimeout(timer);
        timer = setTimeout(draw, PREVIEW_MS);
        return true;
      },
      selectNode: () => dom.classList.add("ProseMirror-selectednode"),
      deselectNode: () => dom.classList.remove("ProseMirror-selectednode"),
      stopEvent: (event) => {
        if (editing !== null) return true;
        // a click goes on to the editor, which follows a link around the piece on Ctrl | Cmd + click
        return event.type !== "click" && !event.type.startsWith("key");
      },
      // View's hydration and the field change this DOM, never the document
      ignoreMutation: () => true,
      destroy: () => {
        gone = true;
        clearTimeout(timer);
        undraw();
        undraw = () => {};
      },
    };
  };
});

export const inlineSource = [inlineSourceRemark, inlineSourceNode, inlineSourceView].flat();
