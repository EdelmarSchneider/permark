// The Milkdown editor of Edit mode, assembled from `@milkdown/kit` (no Crepe: toolbar, menus and styles are the
// app's own). Runs without a window too (vitest drives it in jsdom).
//
// Added to the stock CommonMark + GFM schema: `pm_locked`, an atom holding a block the editor must not rewrite
// (drawn as View draws it, `lockedView.ts`; View's prompt boxes around blocks, `promptBoxes.ts`), `inline_source`, an
// inline atom keeping a piece of a line as written (inline math, footnote references, reference-style links:
// `inlineSource.ts`), `taskItemView`, the
// real checkbox of a task item, and the reading view's wrappers of code blocks (with their language and token colors,
// `codeColors.ts`) and tables (`codeBlockView`, `tableView`), and View's inline looks (`inlineLooks.ts`). Left out: stock plugins that change a document by
// themselves (heading ids, list renumbering, empty-line preservation). A no-break space is written as `&nbsp;`; link
// marks end at the link's edge; the serializer writes the style below.

import {
  Editor,
  editorViewCtx,
  editorViewOptionsCtx,
  parserCtx,
  remarkCtx,
  remarkStringifyOptionsCtx,
  rootCtx,
  serializerCtx,
} from "@milkdown/kit/core";
import { clipboard } from "@milkdown/kit/plugin/clipboard";
import { history } from "@milkdown/kit/plugin/history";
import {
  bulletListKeymap,
  codeBlockKeymap,
  codeBlockSchema,
  commonmark,
  headingKeymap,
  inlineCodeKeymap,
  linkSchema,
  listItemSchema,
  orderedListKeymap,
  paragraphKeymap,
  remarkPreserveEmptyLinePlugin,
  syncHeadingIdPlugin,
  syncListOrderPlugin,
  textSchema,
} from "@milkdown/kit/preset/commonmark";
import { gfm, strikethroughKeymap, tableSchema } from "@milkdown/kit/preset/gfm";
import { gapCursor } from "@milkdown/kit/prose/gapcursor";
import type { Node as PMNode } from "@milkdown/kit/prose/model";
import { EditorState, Selection } from "@milkdown/kit/prose/state";
import type { EditorView, NodeViewConstructor } from "@milkdown/kit/prose/view";
import { $ctx, $node, $prose, $view } from "@milkdown/kit/utils";
import { CODE_BLOCK, TABLE_WRAP, taskLabel } from "../../doc/enhance";
import type { CodeColorer } from "../../plugins/props";
import { codeColors } from "./codeColors";
import { inlineLooks, noLooks, type LooksHost } from "./inlineLooks";
import {
  lockedHostCtx,
  lockedKeys,
  lockedNode,
  lockedRelook,
  lockedView,
  type LockedHost,
} from "./lockedView";
import { escapingText, inlineSource } from "./inlineSource";
import { promptBoxes } from "./promptBoxes";

export { INLINE_SOURCE } from "./inlineSource";
export type { LooksHost } from "./inlineLooks";
export { LOCKED, relook, type BlockName, type LockReason, type LockedHost } from "./lockedView";

const NO_BREAK_SPACE = String.fromCharCode(0xa0);

/** A code block's label as the reading view's corner shows it: the text, and the buttons right of it there. */
export type CodeLabel = { text: string; buttons: number };

/** What a code block needs from the app to look like the reading view: its label and its token colors. */
export type CodeHost = {
  label(language: string): CodeLabel | null;
  /** The reading view's token colors for code of this language (loaded on first use); absent | null: plain. */
  colors?(language: string): Promise<CodeColorer> | null;
};

const plainCode: CodeHost = {
  label: (language) => (language === "" ? null : { text: language, buttons: 1 }),
};

export const codeHostCtx = $ctx<CodeHost, "pmCodeHost">(plainCode, "pmCodeHost");

/** View's lookups behind the inline looks (missing links, code references, prompt text); none: anchors only. */
export const looksHostCtx = $ctx<LooksHost, "pmLooksHost">(noLooks, "pmLooksHost");

const textNode = $node("text", () => ({
  group: "inline",
  parseMarkdown: {
    match: ({ type }) => type === "text",
    runner: (state, node) => {
      state.addText(String(node.value ?? ""));
    },
  },
  toMarkdown: {
    match: (node) => node.type.name === "text",
    runner: (state, node) => {
      (node.text ?? "").split(NO_BREAK_SPACE).forEach((part, i) => {
        if (i > 0) state.addNode("html", undefined, "&nbsp;");
        if (part !== "") state.addNode("text", undefined, part);
      });
    },
  },
}));

/** One task item box flips (`pos` is the node's position): the path `taskItemView`'s click takes.
 *  False when `pos` is not a task item, so a stray click does not mark the file changed. */
export function flipTask(view: EditorView, pos: number, next: boolean): boolean {
  const item = view.state.doc.nodeAt(pos);
  if (item === null || item.type.name !== "list_item") return false;
  const checked = item.attrs.checked;
  if (typeof checked !== "boolean") return false;
  view.dispatch(view.state.tr.setNodeMarkup(pos, undefined, { ...item.attrs, checked: next }));
  return true;
}

/**
 * The box of a task item in Edit mode. gfm draws a task item as `li[data-item-type=task]` with no
 * `<input>`, so the list shows the browser's bullet dots and nothing can be clicked. This view adds the real
 * checkbox, names it by its task (`taskName`, the same name view mode gives), writes the classes the reading
 * stylesheet already styles (`task-list-item`, `is-done`), and keeps gfm's `data-*` attributes so a copy inside
 * the editor reads back the same item. A plain list item gets the stock shape and no box.
 */
const taskItemView = $view(listItemSchema.node, (): NodeViewConstructor => (node, view, getPos) => {
  const dom = document.createElement("li");
  const box = document.createElement("input");
  box.type = "checkbox";
  const content = document.createElement("div");
  dom.append(box, content);
  const isTask = (): boolean => typeof node.attrs.checked === "boolean";
  // the name is read from the node, not from the drawn text: the view paints before the content is inserted
  const nameOf = (): string => {
    const pieces: string[] = [];
    node.forEach((child) => {
      if (child.type.name === "bullet_list" || child.type.name === "ordered_list") return;
      pieces.push(child.textContent);
    });
    return taskLabel(pieces);
  };
  const paint = (): void => {
    if (isTask()) {
      dom.dataset.itemType = "task";
      dom.dataset.label = String(node.attrs.label ?? "");
      dom.dataset.listType = String(node.attrs.listType ?? "");
      dom.dataset.spread = String(node.attrs.spread ?? "true");
      dom.dataset.checked = String(node.attrs.checked);
      if (box.parentElement !== dom) dom.prepend(box);
      box.checked = node.attrs.checked === true;
      const name = nameOf();
      if (name === "") box.removeAttribute("aria-label");
      else box.setAttribute("aria-label", name);
    } else {
      for (const key of ["itemType", "label", "listType", "spread", "checked"]) {
        delete dom.dataset[key];
      }
      box.remove();
      box.removeAttribute("aria-label");
    }
    dom.classList.toggle("task-list-item", isTask());
    dom.classList.toggle("is-done", node.attrs.checked === true);
  };
  paint();
  box.addEventListener("click", () => {
    const at = getPos();
    if (at === undefined) return;
    flipTask(view, at, !(node.attrs.checked === true));
  });
  return {
    dom,
    contentDOM: content,
    // the box is the app's control, not the editor's text
    stopEvent: (event) => event.target instanceof Node && box.contains(event.target),
    ignoreMutation: (mutation) => box.contains(mutation.target),
    update: (next) => {
      node = next;
      paint();
      return true;
    },
  };
});

/**
 * A code block in the reading view's shape: the `.code-block` wrapper (the block's outer space and the label's
 * frame) around `pre > code`, and its language label where that view's corner has it (`CodeHost`). The label is
 * drawn, not text: it stays out of the document and is gone when the block gets none.
 */
const codeBlockView = $view(codeBlockSchema.node, (ctx): NodeViewConstructor => (node) => {
  const host = ctx.get(codeHostCtx.key);
  const dom = document.createElement("div");
  dom.className = CODE_BLOCK;
  const pre = document.createElement("pre");
  const code = document.createElement("code");
  pre.append(code);
  const label = document.createElement("span");
  label.className = "lang";
  label.contentEditable = "false";
  dom.append(pre);
  // painted when the node is made | updated only: nothing re-reads the host when plugins change, so a plugin toggled
  // while editing relabels a block on its next change (cheap; the label is cosmetic)
  const paint = (): void => {
    const language = String(node.attrs.language ?? "");
    dom.dataset.info = language;
    if (language === "") delete pre.dataset.language;
    else pre.dataset.language = language;
    const shown = host.label(language);
    if (shown === null) {
      label.remove();
      return;
    }
    label.textContent = shown.text;
    label.style.setProperty("--corner-buttons", String(shown.buttons));
    if (label.parentElement !== dom) dom.append(label);
  };
  paint();
  return {
    dom,
    contentDOM: code,
    ignoreMutation: (mutation) => label.contains(mutation.target),
    update: (next) => {
      if (next.type !== node.type) return false;
      node = next;
      paint();
      return true;
    },
  };
});

/** A table in the reading view's `.table-wrap`, so its frame, corners and outer space come from the same rules. */
const tableView = $view(tableSchema.node, (): NodeViewConstructor => (node) => {
  const dom = document.createElement("div");
  dom.className = TABLE_WRAP;
  const table = document.createElement("table");
  const body = document.createElement("tbody");
  table.append(body);
  dom.append(table);
  return { dom, contentDOM: body, update: (next) => next.type === node.type };
});

/** Where the caret starts: the first text, so a leading locked block (the front matter) is not selected. */
function startOf(doc: PMNode): Selection {
  return Selection.findFrom(doc.resolve(0), 1, true) ?? Selection.atStart(doc);
}

/**
 * Stock plugins left out: they change a document by themselves, or are replaced. The Mod-Alt keymaps go too: AltGr
 * is Ctrl+Alt on Windows and types characters (`{` `[` `}` on many layouts); `keys.ts` binds them safely. Mod-e is
 * the mode key.
 */
const LEFT_OUT = new Set<unknown>([
  ...[headingKeymap, paragraphKeymap, orderedListKeymap, bulletListKeymap].flat(),
  ...[codeBlockKeymap, inlineCodeKeymap, strikethroughKeymap].flat(),
  ...[syncHeadingIdPlugin].flat(),
  ...[syncListOrderPlugin].flat(),
  ...[remarkPreserveEmptyLinePlugin].flat(),
  ...[textSchema].flat(),
]);

/** How the editor writes Markdown. */
export const STRINGIFY_OPTIONS = {
  bullet: "-",
  bulletOther: "*",
  emphasis: "*",
  strong: "*",
  fence: "`",
  rule: "-",
  ruleSpaces: false,
  listItemIndent: "one",
  incrementListMarker: true,
  setext: false,
  closeAtx: false,
} as const;

export type MdastNode = {
  type: string;
  children?: MdastNode[];
  position?: { start: { offset?: number }; end: { offset?: number } };
  [key: string]: unknown;
};

export type Core = {
  editor: Editor;
  view: EditorView;
  /** Markdown -> document. Throws on text the editor cannot map. */
  parse(text: string): PMNode;
  /** Document -> Markdown (with a final line break). */
  serialize(doc: PMNode): string;
  /** Markdown of one top-level node (no final break). Cached per node object. */
  write(node: PMNode): string;
  /** mdast after Milkdown's transformers, without positions and marker characters. */
  mdast(text: string): MdastNode;
  /** mdast as parsed (positions, references still references). */
  parseRaw(text: string): MdastNode;
  /** Replaces the whole document (history starts over). */
  setDoc(doc: PMNode): void;
  destroy(): Promise<void>;
};

export type CoreOptions = {
  host: HTMLElement;
  /** The accessible name of the editor's text. */
  label?: string;
  locked?: LockedHost;
  code?: CodeHost;
  looks?: LooksHost;
  /** Before the stock plugins (keys and pastes seen first). */
  first?: Parameters<Editor["use"]>[0][];
  /** After the stock plugins. */
  plugins?: Parameters<Editor["use"]>[0][];
};

const IGNORED_KEYS = new Set(["position", "marker"]);

function stripPositions(node: MdastNode): MdastNode {
  return JSON.parse(
    JSON.stringify(node, (key, value: unknown) => (IGNORED_KEYS.has(key) ? undefined : value)),
  ) as MdastNode;
}

/** Creates the editor in `host` with an empty document. */
export async function createCore(options: CoreOptions): Promise<Core> {
  const editor = Editor.make()
    .config((ctx) => {
      ctx.set(rootCtx, options.host);
      // the blocks are the children of `.md-body`: the reading styles apply as they are
      ctx.update(editorViewOptionsCtx, (previous) => ({
        ...previous,
        attributes: {
          class: "md-body pm-editor",
          spellcheck: "true",
          ...(options.label === undefined ? {} : { "aria-label": options.label }),
        },
      }));
      ctx.update(remarkStringifyOptionsCtx, (previous) => ({
        ...previous,
        ...STRINGIFY_OPTIONS,
        handlers: { ...previous.handlers, text: escapingText(previous.handlers?.text) },
      }));
      // text typed at the edge of a link stays out of it
      ctx.update(linkSchema.key, (previous) => (c) => ({ ...previous(c), inclusive: false }));
      if (options.locked) ctx.set(lockedHostCtx.key, options.locked);
      if (options.code) ctx.set(codeHostCtx.key, options.code);
      if (options.looks) ctx.set(looksHostCtx.key, options.looks);
    })
    .use(lockedHostCtx)
    .use(codeHostCtx)
    .use(looksHostCtx)
    .use(lockedNode)
    .use(lockedView)
    .use(lockedKeys)
    .use(lockedRelook)
    .use(inlineSource)
    .use(promptBoxes)
    .use(taskItemView)
    .use(codeBlockView)
    .use($prose((ctx) => codeColors(() => ctx.get(codeHostCtx.key))))
    .use(tableView)
    .use($prose((ctx) => inlineLooks(() => ctx.get(looksHostCtx.key))))
    .use((options.first ?? []).flat())
    .use([...commonmark, ...gfm].filter((plugin) => !LEFT_OUT.has(plugin)))
    .use(textNode)
    .use(history)
    .use(clipboard)
    .use($prose(() => gapCursor()))
    .use((options.plugins ?? []).flat());
  await editor.create();

  return editor.action((ctx) => {
    const view = ctx.get(editorViewCtx);
    const parser = ctx.get(parserCtx);
    const serializer = ctx.get(serializerCtx);
    const remark = ctx.get(remarkCtx);
    const written = new WeakMap<PMNode, string>();
    const core: Core = {
      editor,
      view,
      parse: (text) => parser(text),
      serialize: (doc) => serializer(doc),
      write: (node) => {
        let text = written.get(node);
        if (text === undefined) {
          text = serializer(node.type.schema.topNodeType.create(null, node)).replace(/\n+$/, "");
          if (node.type.name === "paragraph" || node.type.name === "heading") {
            // a space at an edge (a sentence split in the middle) is not written as `&#x20;`
            text = text.replace(/^(?:&#x20;)+/, "").replace(/(?:&#x20;)+$/, "");
          }
          written.set(node, text);
        }
        return text;
      },
      mdast: (text) =>
        stripPositions(remark.runSync(remark.parse(text), { value: text }) as MdastNode),
      parseRaw: (text) => remark.parse(text) as MdastNode,
      setDoc: (doc) => {
        const state = EditorState.create({
          schema: view.state.schema,
          doc,
          selection: startOf(doc),
          plugins: view.state.plugins,
        });
        view.updateState(state);
      },
      destroy: async () => {
        await editor.destroy();
      },
    };
    return core;
  });
}
