// Syntax highlighting inside the code blocks of Visual mode: ProseMirror decorations drawn from highlight.js's result for the block's
// text, with the classes the reading view uses, so a block has the same colours in both. Only a block that names a language is
// highlighted (a guess would change under the caret as the person types). The grammar loads on first use; until it has, the block
// is plain, and a transaction that carries no change of the document asks for the decorations again once it is there.

import type { Node as PMNode } from "@milkdown/kit/prose/model";
import { Plugin, PluginKey, type EditorState } from "@milkdown/kit/prose/state";
import { Decoration, DecorationSet, type EditorView } from "@milkdown/kit/prose/view";
import { $prose } from "@milkdown/kit/utils";
import { ensureLanguage, highlightLoaded } from "../../code/highlighter";
import {
  fenceLanguage,
  isPlainName,
  languageDisplayName,
  resolveLanguage,
} from "../../code/languages";
import { tokenRanges, type TokenRange } from "../../code/tokens";

export const codeHighlightKey = new PluginKey<DecorationSet>("TM_CODE_HIGHLIGHT");

/** Longer than this a block is left plain while it is edited (the work is done again at every key). */
const MAX_EDITED_CHARS = 30_000;

type Highlighted = { language: string; ranges: TokenRange[] };

/** The ranges of every block already highlighted, by node: a node is replaced when its text changes, so the others are kept. */
const highlighted = new WeakMap<PMNode, Highlighted>();

/** The decorations of a document's code blocks. `request` is told about the grammars that are not loaded yet. */
export function codeDecorations(doc: PMNode, request: (id: string) => void): DecorationSet {
  const decorations: Decoration[] = [];
  doc.descendants((node, pos) => {
    if (node.type.name !== "code_block") return true;
    const info = String(node.attrs.language ?? "");
    const name = fenceLanguage(info);
    if (name === "" || isPlainName(name)) return false;
    // the name the corner label shows: the language's own, or the info string as written
    decorations.push(
      Decoration.node(pos, pos + node.nodeSize, { "data-code-name": languageDisplayName(info) }),
    );
    const resolved = resolveLanguage(name);
    if (!resolved || node.content.size > MAX_EDITED_CHARS) return false;
    let found = highlighted.get(node);
    if (!found || found.language !== resolved.id) {
      const html = highlightLoaded(node.textContent, resolved.id);
      if (html === null) {
        request(resolved.id);
        return false;
      }
      found = { language: resolved.id, ranges: tokenRanges(html) };
      highlighted.set(node, found);
    }
    // the text of a code block starts one position after the node
    for (const range of found.ranges) {
      decorations.push(
        Decoration.inline(pos + 1 + range.from, pos + 1 + range.to, { class: range.className }),
      );
    }
    return false;
  });
  return DecorationSet.create(doc, decorations);
}

/** The Milkdown plugin. */
export const codeHighlight = $prose(() => {
  let view: EditorView | null = null;
  const asked = new Set<string>();
  const request = (id: string): void => {
    if (asked.has(id)) return;
    asked.add(id);
    void ensureLanguage(id).then((ok) => {
      // a grammar that would not load can be asked for again the next time a block needs it
      if (!ok) asked.delete(id);
      if (!ok || !view || view.isDestroyed) return;
      view.dispatch(
        view.state.tr.setMeta(codeHighlightKey, "loaded").setMeta("addToHistory", false),
      );
    });
  };
  return new Plugin<DecorationSet>({
    key: codeHighlightKey,
    state: {
      init: (_config, state: EditorState) => codeDecorations(state.doc, request),
      apply: (tr, previous, _old, state) =>
        tr.docChanged || tr.getMeta(codeHighlightKey) !== undefined
          ? codeDecorations(state.doc, request)
          : previous,
    },
    props: {
      decorations: (state) => codeHighlightKey.getState(state) ?? null,
    },
    view: (editorView) => {
      view = editorView;
      return {
        destroy: () => {
          view = null;
        },
      };
    },
  });
});
