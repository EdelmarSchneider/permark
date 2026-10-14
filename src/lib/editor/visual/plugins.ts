// The ProseMirror plugins of Visual mode that belong to this app and not to the Markdown presets: keeping each top-level
// node tied to its block, the unsaved-block bar, task checkboxes, pasted and dropped images, and the keys that open the
// link popover and the slash menu.

import { keymap } from "@milkdown/kit/prose/keymap";
import type { Node as PMNode } from "@milkdown/kit/prose/model";
import { NodeSelection, Plugin, PluginKey, TextSelection } from "@milkdown/kit/prose/state";
import { Mapping } from "@milkdown/kit/prose/transform";
import { Decoration, DecorationSet, type EditorView } from "@milkdown/kit/prose/view";
import { $prose } from "@milkdown/kit/utils";
import { LOCKED, TM_BLOCK } from "./core";
import { originOf, type Analysis } from "./doc";

/** What the plugins ask of the editor object that owns them. */
export type PluginHost = {
  analysisFor(doc: PMNode): Analysis | null;
  /** The view or selection changed (after the state was applied). */
  updated(view: EditorView): void;
  files(files: File[], source: "paste" | "drop"): void;
  link(): void;
  editInSource(block: number | null): void;
  /** The slash menu is open and wants this key. Resolves to whether it took it. */
  slashKey(key: string): boolean;
};

const isEmptyParagraph = (node: PMNode): boolean =>
  node.type.name === "paragraph" && node.content.size === 0;

/**
 * Gives a top-level node that took a block's place the block's index: turning a paragraph into a heading, wrapping it in a list or
 * lifting it out creates a new node where the old one was, and the block should stay the same block (so the blank lines around it
 * are kept). A block that no node claims any more was deleted or merged away.
 */
function adoptOrigins(
  transactions: readonly import("@milkdown/kit/prose/state").Transaction[],
  before: import("@milkdown/kit/prose/state").EditorState,
  after: import("@milkdown/kit/prose/state").EditorState,
) {
  if (!transactions.some((tr) => tr.docChanged)) return null;
  const claimed = new Set<number>();
  after.doc.forEach((node) => {
    const origin = originOf(node);
    if (origin !== null) claimed.add(origin);
  });
  const orphans: { pos: number; origin: number }[] = [];
  let pos = 0;
  before.doc.forEach((node) => {
    const origin = originOf(node);
    if (origin !== null && !claimed.has(origin)) orphans.push({ pos, origin });
    pos += node.nodeSize;
  });
  if (orphans.length === 0) return null;
  const mapping = new Mapping();
  for (const tr of transactions) mapping.appendMapping(tr.mapping);
  const tr = after.tr;
  for (const { pos: from, origin } of orphans) {
    const at = mapping.map(from, -1);
    if (at > after.doc.content.size || after.doc.resolve(at).depth !== 0) continue;
    const node = after.doc.nodeAt(at);
    if (!node || originOf(node) !== null || isEmptyParagraph(node)) continue;
    tr.setNodeAttribute(at, TM_BLOCK, origin);
  }
  return tr.docChanged ? tr.setMeta("addToHistory", false) : null;
}

const imagesOf = (data: DataTransfer | null, onlyImages: boolean): File[] =>
  Array.from(data?.files ?? []).filter((file) => !onlyImages || file.type.startsWith("image/"));

export const unsavedKey = new PluginKey("TM_UNSAVED");

/** The plugins, in the order the editor should try them (before the Markdown presets'). */
export function appPlugins(host: PluginHost) {
  const tracking = $prose(
    () =>
      new Plugin({
        key: unsavedKey,
        appendTransaction: adoptOrigins,
        props: {
          decorations(state) {
            const analysis = host.analysisFor(state.doc);
            if (!analysis || analysis.changed === 0) return null;
            const decorations: Decoration[] = [];
            let pos = 0;
            analysis.nodes.forEach((info) => {
              if (info.unsaved) {
                decorations.push(
                  Decoration.node(pos, pos + info.node.nodeSize, { class: "tm-unsaved" }),
                );
              }
              pos += info.node.nodeSize;
            });
            return DecorationSet.create(state.doc, decorations);
          },
        },
        view: () => ({
          update: (view) => host.updated(view),
        }),
      }),
  );

  const interaction = $prose(
    () =>
      new Plugin({
        key: new PluginKey("TM_INTERACTION"),
        props: {
          // a task's box is drawn in the margin of its list item: a click there flips it, and nothing else changes
          handleClick(view, _pos, event) {
            const target = event.target;
            if (!(target instanceof HTMLElement)) return false;
            const item = target.closest<HTMLElement>('li[data-item-type="task"]');
            if (!item || !view.dom.contains(item)) return false;
            const box = item.getBoundingClientRect();
            const at = event.clientX - box.left;
            // the box is 15px wide at the left edge of the item (see editor.css)
            if (at < 0 || at > 22 || event.clientY - box.top > 30) return false;
            const inside = view.posAtDOM(item, 0);
            const $pos = view.state.doc.resolve(inside);
            for (let depth = $pos.depth; depth > 0; depth--) {
              const node = $pos.node(depth);
              if (node.type.name !== "list_item") continue;
              view.dispatch(
                view.state.tr.setNodeMarkup($pos.before(depth), undefined, {
                  ...node.attrs,
                  checked: !node.attrs.checked,
                }),
              );
              return true;
            }
            return false;
          },
          handlePaste(_view, event) {
            const images = imagesOf(event.clipboardData, true);
            if (images.length === 0) return false;
            event.preventDefault();
            host.files(images, "paste");
            return true;
          },
          handleDrop(view, event) {
            const files = imagesOf(event.dataTransfer, false);
            if (files.length === 0) return false;
            event.preventDefault();
            const at = view.posAtCoords({ left: event.clientX, top: event.clientY });
            if (at) {
              try {
                view.dispatch(
                  view.state.tr.setSelection(TextSelection.near(view.state.doc.resolve(at.pos))),
                );
              } catch {
                // the caret stays where it was
              }
            }
            host.files(files, "drop");
            return true;
          },
          handleKeyDown(_view, event) {
            if (!["ArrowUp", "ArrowDown", "Enter"].includes(event.key)) return false;
            if (event.ctrlKey || event.metaKey || event.altKey || event.shiftKey) return false;
            return host.slashKey(event.key);
          },
        },
      }),
  );

  const keys = $prose(() =>
    keymap({
      "Mod-k": () => {
        host.link();
        return true;
      },
      // Enter on a selected atom opens it in Source: the keyboard way to the atom's button
      Enter: (state) => {
        const selection = state.selection;
        if (!(selection instanceof NodeSelection) || selection.node.type.name !== LOCKED)
          return false;
        host.editInSource(originOf(selection.node));
        return true;
      },
    }),
  );

  return [tracking, interaction, keys];
}
