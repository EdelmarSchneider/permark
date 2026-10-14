// The Milkdown editor behind Visual mode, assembled from `@milkdown/kit` (no Crepe, so the toolbar, menus and
// styles are the app's own). This module is the part that runs without a window: vitest drives it in jsdom.
//
// Two things are added to the stock Markdown schema:
//  - `tmBlock` on every node that can sit at the top of the document: the index of the block of the original
//    file it was made from (null for a new block). Saving writes the original bytes of every block that is still
//    equal to its original.
//  - `tm_locked`, an atom for a block the editor must not rewrite. It keeps the block's text and the app draws it.

import {
  Editor,
  editorViewCtx,
  parserCtx,
  remarkCtx,
  remarkStringifyOptionsCtx,
  rootCtx,
  serializerCtx,
} from "@milkdown/kit/core";
import { history } from "@milkdown/kit/plugin/history";
import {
  blockquoteSchema,
  bulletListSchema,
  codeBlockSchema,
  commonmark,
  headingSchema,
  hrSchema,
  orderedListSchema,
  paragraphSchema,
  remarkPreserveEmptyLinePlugin,
  syncHeadingIdPlugin,
} from "@milkdown/kit/preset/commonmark";
import { footnoteDefinitionSchema, gfm, tableSchema } from "@milkdown/kit/preset/gfm";
import { gapCursor } from "@milkdown/kit/prose/gapcursor";
import { Fragment, Slice, type Node as PMNode } from "@milkdown/kit/prose/model";
import { Plugin, PluginKey, type EditorState } from "@milkdown/kit/prose/state";
import type { EditorView, NodeViewConstructor } from "@milkdown/kit/prose/view";
import { $ctx, $node, $prose, $view } from "@milkdown/kit/utils";
import type { BlockKind } from "../../types";

/** The name of the attribute that ties a top-level node to a block of the original file. */
export const TM_BLOCK = "tmBlock";
export const LOCKED = "tm_locked";

/** Why a block is a locked atom (shown as the tooltip of the atom, and counted by the tests). */
export type LockReason =
  /** its kind: front matter, raw HTML, math, footnote definitions or something the editor has no node for */
  | "kind"
  /** a Mermaid diagram, which is drawn, not edited as text */
  | "diagram"
  /** the editor could not read the block, or read it as something other than one block */
  | "parse"
  /** the editor would write the block as different Markdown */
  | "differs"
  /** it uses a reference defined elsewhere in the document (a link reference or a footnote) */
  | "reference"
  /** it holds inline math, which the editor would not understand */
  | "math";

/** What a locked atom needs from the app to draw itself. */
export type LockedHost = {
  /** Fills the body of an atom (its rendered HTML, or its source when there is none). */
  render(body: HTMLElement, atom: { kind: BlockKind; source: string; reason: LockReason }): void;
  /** The button of an atom: open Source at this block. */
  edit(tmBlock: number | null, source: string): void;
  labels: {
    edit: string;
    editHint: string;
    /** The name of the atom for a screen reader. */
    region(kind: string): string;
    /** Why the atom is locked (its tooltip). */
    reason(reason: string): string;
  };
};

const noHost: LockedHost = {
  render: (body, atom) => {
    body.textContent = atom.source;
  },
  edit: () => {},
  labels: { edit: "Edit in Source", editHint: "", region: (kind) => kind, reason: () => "" },
};

/** The slot the app puts its `LockedHost` in before the editor is created. */
export const lockedHostCtx = $ctx<LockedHost, "tmLockedHost">(noHost, "tmLockedHost");

export const lockedNode = $node(LOCKED, () => ({
  group: "block",
  atom: true,
  selectable: true,
  draggable: false,
  isolating: true,
  marks: "",
  attrs: {
    // required, so that ProseMirror never fills a gap with an atom (an emptied document gets a paragraph)
    source: {},
    kind: { default: "other" },
    reason: { default: "kind" },
    [TM_BLOCK]: { default: null },
  },
  toDOM: (node) => ["div", { class: "tm-locked", "data-kind": node.attrs.kind as string }],
  // blocks are made from the file's own slices by `buildDoc`, never from Markdown text
  parseMarkdown: { match: () => false, runner: () => {} },
  toMarkdown: {
    match: (node) => node.type.name === LOCKED,
    runner: (state, node) => {
      state.addNode("html", undefined, node.attrs.source as string);
    },
  },
}));

/** The atom's rendered body and its "Edit in Source" button. */
export const lockedView = $view(lockedNode, (ctx): NodeViewConstructor => {
  return (node, view, getPos) => {
    const host = ctx.get(lockedHostCtx.key);
    const dom = document.createElement("div");
    dom.className = "tm-locked";
    dom.contentEditable = "false";
    dom.setAttribute("role", "group");
    dom.dataset.kind = node.attrs.kind as string;
    dom.dataset.reason = node.attrs.reason as string;
    dom.setAttribute("aria-label", host.labels.region(node.attrs.kind as string));
    dom.title = host.labels.reason(node.attrs.reason as string);
    const body = document.createElement("div");
    body.className = "tm-locked__body md-body";
    const button = document.createElement("button");
    button.type = "button";
    button.className = "btn btn--sm tm-locked__edit";
    button.textContent = host.labels.edit;
    button.title = host.labels.editHint;
    const open = (): void => {
      const pos = getPos();
      const attrs =
        pos === undefined ? node.attrs : (view.state.doc.nodeAt(pos)?.attrs ?? node.attrs);
      host.edit((attrs[TM_BLOCK] as number | null) ?? null, attrs.source as string);
    };
    button.addEventListener("click", open);
    dom.append(body, button);
    host.render(body, {
      kind: node.attrs.kind as BlockKind,
      source: node.attrs.source as string,
      reason: node.attrs.reason as LockReason,
    });
    return {
      dom,
      // the body is the app's own markup: the editor neither reads nor reacts to it
      stopEvent: (event) => event.target instanceof Node && button.contains(event.target),
      ignoreMutation: () => true,
      update: (next) => next.type === node.type && next.attrs.source === node.attrs.source,
    };
  };
});

/** Hands every top-level node of a pasted slice a null `tmBlock`: a paste is new text, never an original block. */
const pasteAsNew = $prose(
  () =>
    new Plugin({
      key: new PluginKey("TM_PASTE_AS_NEW"),
      props: {
        transformPasted(slice) {
          if (slice.openStart > 0) return slice;
          const nodes: PMNode[] = [];
          slice.content.forEach((child) => {
            const carries = child.type.spec.attrs?.[TM_BLOCK] !== undefined;
            nodes.push(
              carries
                ? child.type.create(
                    { ...child.attrs, [TM_BLOCK]: null },
                    child.content,
                    child.marks,
                  )
                : child,
            );
          });
          return new Slice(Fragment.fromArray(nodes), slice.openStart, slice.openEnd);
        },
      },
    }),
);

/** Top-level node types that carry `tmBlock`. */
const TOP_LEVEL_SCHEMAS = [
  paragraphSchema,
  headingSchema,
  blockquoteSchema,
  bulletListSchema,
  orderedListSchema,
  codeBlockSchema,
  hrSchema,
  tableSchema,
  footnoteDefinitionSchema,
];

/** Plugins of the stock presets that would change the document by themselves, or that Visual mode does not want. */
const LEFT_OUT = new Set<unknown>([
  // writes an `id` into every heading as soon as the view exists: every heading would look edited
  ...[syncHeadingIdPlugin].flat(),
  // writes a blank paragraph as `<br />`; here an empty paragraph is simply not written
  ...[remarkPreserveEmptyLinePlugin].flat(),
]);

/** How the editor writes Markdown. The same choices the files of this product's audience use most. */
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

/** The tools the rest of Visual mode needs, taken out of the Milkdown context once the editor exists. */
export type Core = {
  editor: Editor;
  view: EditorView;
  /** Markdown text to a ProseMirror document. Throws for text the editor cannot map. */
  parse(text: string): PMNode;
  /** A document to Markdown text (with a final line break). */
  serialize(doc: PMNode): string;
  /** The mdast of a text as Milkdown's own parser sees it (its transformers have run: references are links), without positions and markers. */
  mdast(text: string): MdastNode;
  /** The mdast as parsed, before any transformer: with positions, and links by reference still references. */
  parseRaw(text: string): MdastNode;
  state(): EditorState;
  /** Run something with the Milkdown context (commands, other slices). */
  action: Editor["action"];
  destroy(): Promise<void>;
};

export type MdastNode = {
  type: string;
  children?: MdastNode[];
  position?: { start: { offset?: number }; end: { offset?: number } };
  [key: string]: unknown;
};

export type CoreOptions = {
  host: HTMLElement;
  locked?: LockedHost;
  /** Added to the editor after the stock plugins, in this order (keymaps, popovers, paste and drop handling...). */
  plugins?: Parameters<Editor["use"]>[0][];
  /** Added before the stock plugins, so that they see keys and pastes first. */
  firstPlugins?: Parameters<Editor["use"]>[0][];
  config?: Parameters<Editor["config"]>[0];
  label?: string;
};

/** Creates the editor in `host` with an empty document. */
export async function createCore(options: CoreOptions): Promise<Core> {
  const editor = Editor.make()
    .config((ctx) => {
      ctx.set(rootCtx, options.host);
      ctx.update(remarkStringifyOptionsCtx, (previous) => ({ ...previous, ...STRINGIFY_OPTIONS }));
      for (const schema of TOP_LEVEL_SCHEMAS) {
        ctx.update(schema.key, (previous) => (c) => {
          const spec = previous(c);
          return { ...spec, attrs: { ...spec.attrs, [TM_BLOCK]: { default: null } } };
        });
      }
      if (options.locked) ctx.set(lockedHostCtx.key, options.locked);
    })
    .config(options.config ?? (() => {}))
    .use(lockedHostCtx)
    .use(lockedNode)
    .use(lockedView)
    .use((options.firstPlugins ?? []).flat())
    .use([...commonmark, ...gfm].filter((plugin) => !LEFT_OUT.has(plugin)))
    .use(history)
    .use(pasteAsNew)
    .use($prose(() => gapCursor()))
    .use((options.plugins ?? []).flat());
  await editor.create();

  return editor.action((ctx) => {
    const view = ctx.get(editorViewCtx);
    const parser = ctx.get(parserCtx);
    const serializer = ctx.get(serializerCtx);
    const remark = ctx.get(remarkCtx);
    return {
      editor,
      view,
      parse: (text) => parser(text),
      serialize: (doc) => serializer(doc),
      mdast: (text) =>
        stripPositions(remark.runSync(remark.parse(text), { value: text }) as MdastNode),
      parseRaw: (text) => remark.parse(text) as MdastNode,
      state: () => view.state,
      action: (fn) => editor.action(fn),
      destroy: async () => {
        await editor.destroy();
      },
    } satisfies Core;
  });
}

/** What mdast nodes carry that does not change what a text says: where they were, and the marker character. */
const IGNORED_KEYS = new Set(["position", "marker"]);

function stripPositions(node: MdastNode): MdastNode {
  return JSON.parse(
    JSON.stringify(node, (key, value: unknown) => (IGNORED_KEYS.has(key) ? undefined : value)),
  );
}
