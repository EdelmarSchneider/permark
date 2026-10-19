// The Milkdown editor of Edit mode, assembled from `@milkdown/kit` (no Crepe: toolbar, menus and styles are the
// app's own). Runs without a window too (vitest drives it in jsdom).
//
// Added to the stock CommonMark + GFM schema: `pm_locked`, an atom holding a block the editor must not rewrite
// (its source, drawn by the app). Left out: stock plugins that change a document by themselves (heading ids,
// list renumbering, empty-line preservation). A no-break space is written as `&nbsp;`; link marks end at the
// link's edge; the serializer writes the style below.

import {
  Editor,
  editorViewCtx,
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
  commonmark,
  headingKeymap,
  inlineCodeKeymap,
  linkSchema,
  orderedListKeymap,
  paragraphKeymap,
  remarkPreserveEmptyLinePlugin,
  syncHeadingIdPlugin,
  syncListOrderPlugin,
  textSchema,
} from "@milkdown/kit/preset/commonmark";
import { gfm, strikethroughKeymap } from "@milkdown/kit/preset/gfm";
import { gapCursor } from "@milkdown/kit/prose/gapcursor";
import type { Node as PMNode } from "@milkdown/kit/prose/model";
import { EditorState } from "@milkdown/kit/prose/state";
import type { EditorView, NodeViewConstructor } from "@milkdown/kit/prose/view";
import { $ctx, $node, $prose, $view } from "@milkdown/kit/utils";

export const LOCKED = "pm_locked";
const NO_BREAK_SPACE = String.fromCharCode(0xa0);

/** Why a block stays as written (backend reasons + the editor's own). */
export type LockReason =
  | "frontMatter"
  | "html"
  | "math"
  | "footnote"
  | "diagram"
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
  /** Draws more than the source (a diagram); returns its teardown. */
  draw(body: HTMLElement, kind: string, source: string): (() => void) | null;
};

const noHost: LockedHost = {
  reasonText: (reason) => reason,
  editLabel: "Source",
  edit: () => {},
  draw: () => null,
};

export const lockedHostCtx = $ctx<LockedHost, "pmLockedHost">(noHost, "pmLockedHost");

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
  return (node) => {
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
    const undraw = host.draw(drawing, String(node.attrs.kind), String(node.attrs.source));
    if (undraw) dom.append(drawing);
    return {
      dom,
      stopEvent: (event) => event.target instanceof Node && button.contains(event.target),
      ignoreMutation: () => true,
      destroy: () => undraw?.(),
    };
  };
});

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
      ctx.update(remarkStringifyOptionsCtx, (previous) => ({ ...previous, ...STRINGIFY_OPTIONS }));
      // text typed at the edge of a link stays out of it
      ctx.update(linkSchema.key, (previous) => (c) => ({ ...previous(c), inclusive: false }));
      if (options.locked) ctx.set(lockedHostCtx.key, options.locked);
    })
    .use(lockedHostCtx)
    .use(lockedNode)
    .use(lockedView)
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
