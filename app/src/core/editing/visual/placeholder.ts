// The hint of an empty document in Edit: a decoration on its one empty paragraph (CSS draws `data-placeholder`), so
// it is never part of the text and goes with the first letter typed. Look: `doc/markdown.css`.

import { Plugin, PluginKey } from "@milkdown/kit/prose/state";
import { Decoration, DecorationSet } from "@milkdown/kit/prose/view";
import { $prose } from "@milkdown/kit/utils";

const hintKey = new PluginKey("PM_EMPTY_HINT");

/** Shows `text` in a document that holds nothing but one empty paragraph. */
export function emptyHint(text: string) {
  return $prose(
    () =>
      new Plugin({
        key: hintKey,
        props: {
          decorations: (state) => {
            const doc = state.doc;
            const only = doc.childCount === 1 ? doc.firstChild : null;
            if (only === null || only.type.name !== "paragraph" || only.content.size > 0)
              return null;
            return DecorationSet.create(doc, [
              Decoration.node(0, only.nodeSize, { class: "pm-empty", "data-placeholder": text }),
            ]);
          },
        },
      }),
  );
}
