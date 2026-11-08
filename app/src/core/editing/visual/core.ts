// The Milkdown editor of Edit mode, assembled from `@milkdown/kit` (no Crepe: toolbar, menus and styles are the
// app's own). Runs without a window too (vitest drives it in jsdom).
//
// Added to the stock CommonMark + GFM schema: `pm_locked`, an atom holding a block the editor must not rewrite
// (its source, drawn by the app), `taskItemView`, the real checkbox of a task item, and the reading view's wrappers
// of code blocks (with their language) and tables (`codeBlockView`, `tableView`). Left out: stock plugins
// that change a document by themselves (heading ids, list renumbering, empty-line preservation). A no-break space
// is written as `&nbsp;`; link marks end at the link's edge; the serializer writes the style below.

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

export const LOCKED = "pm_locked";
const NO_BREAK_SPACE = String.fromCharCode(0xa0);

/** Why a block stays as written (backend reasons + the editor's own). */
export type LockReason =
  | "frontMatter"
  | "html"
  | "math"
  | "footnote"
  | "diagram"
  | "data"
  | "definition"
  | "kind"
  | "parse"
  | "reference"
  | "differs";

/** What a locked atom needs from the app to draw itself. */
export type LockedHost = {
  reasonText(reason: LockReason): string;
  editLabel: string;
  /** "Edit in Source" on this node. */
  edit(node: PMNode): void;
  /**
   * Draws more than the source (a diagram), or takes the source's place (the front matter as editable properties);
   * returns its teardown. `replace` puts a new source in the node.
   */
  draw(
    body: HTMLElement,
    kind: string,
    source: string,
    replace: (source: string) => void,
  ): (() => void) | null;
  /** What the bar says of a block that edits itself (the properties card). */
  cardText?: string;
};

const noHost: LockedHost = {
  reasonText: (reason) => reason,
  editLabel: "Source",
  edit: () => {},
  draw: () => null,
};

export const lockedHostCtx = $ctx<LockedHost, "pmLockedHost">(noHost, "pmLockedHost");

/** A code block's label as the reading view's corner shows it: the text, and the buttons right of it there. */
export type CodeLabel = { text: string; buttons: number };

/** What a code block needs from the app to label itself like the reading view. */
export type CodeHost = { label(language: string): CodeLabel | null };

const plainCode: CodeHost = {
  label: (language) => (language === "" ? null : { text: language, buttons: 1 }),
};

export const codeHostCtx = $ctx<CodeHost, "pmCodeHost">(plainCode, "pmCodeHost");

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

const lockedView = $view(lockedNode, (ctx): NodeViewConstructor => {
  return (node, view, getPos) => {
    const host = ctx.get(lockedHostCtx.key);
    const dom = document.createElement("div");
    dom.className = "pm-locked";
    dom.contentEditable = "false";
    dom.setAttribute("role", "group");
    const reason = String(node.attrs.reason) as LockReason;
    dom.dataset.kind = String(node.attrs.kind);
    dom.dataset.reason = reason;
    const bar = document.createElement("div");
    bar.className = "pm-locked__bar";
    const why = document.createElement("span");
    why.className = "pm-locked__why";
    why.textContent = host.reasonText(reason);
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
    const drawing = document.createElement("div");
    drawing.className = "pm-locked__drawing";
    const replace = (next: string): void => {
      const at = getPos();
      if (at === undefined) return;
      view.dispatch(view.state.tr.setNodeMarkup(at, undefined, { ...node.attrs, source: next }));
    };
    const undraw = host.draw(drawing, String(node.attrs.kind), String(node.attrs.source), replace);
    if (undraw) dom.append(drawing);
    // the front matter as properties takes the source's place
    if (undraw && reason === "frontMatter") {
      dom.classList.add("pm-locked--card");
      if (host.cardText) why.textContent = host.cardText;
    }
    return {
      dom,
      stopEvent: (event) => event.target instanceof Node && button.contains(event.target),
      ignoreMutation: () => true,
      destroy: () => undraw?.(),
    };
  };
});

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
  locked?: LockedHost;
  code?: CodeHost;
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
        attributes: { class: "md-body pm-editor", spellcheck: "true" },
      }));
      ctx.update(remarkStringifyOptionsCtx, (previous) => ({ ...previous, ...STRINGIFY_OPTIONS }));
      // text typed at the edge of a link stays out of it
      ctx.update(linkSchema.key, (previous) => (c) => ({ ...previous(c), inclusive: false }));
      if (options.locked) ctx.set(lockedHostCtx.key, options.locked);
      if (options.code) ctx.set(codeHostCtx.key, options.code);
    })
    .use(lockedHostCtx)
    .use(codeHostCtx)
    .use(lockedNode)
    .use(lockedView)
    .use(taskItemView)
    .use(codeBlockView)
    .use(tableView)
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
