// Each line of Source reads its own direction: an Arabic | Hebrew line runs right to left, as the same paragraph does in
// View and Edit. `dir="auto"` on every drawn line, and CodeMirror reads each line's direction for caret motion and
// selection (`perLineTextDirection`).

import { RangeSetBuilder, type Extension } from "@codemirror/state";
import {
  Decoration,
  EditorView,
  ViewPlugin,
  type DecorationSet,
  type ViewUpdate,
} from "@codemirror/view";

const auto = Decoration.line({ attributes: { dir: "auto" } });

/** `dir="auto"` on the lines in view (a long document is not decorated whole). */
function drawnLines(view: EditorView): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  let last = -1;
  for (const { from, to } of view.visibleRanges) {
    for (let at = from; at <= to;) {
      const line = view.state.doc.lineAt(at);
      if (line.from > last) builder.add(line.from, line.from, auto);
      last = line.from;
      at = line.to + 1;
    }
  }
  return builder.finish();
}

export const lineDirection: Extension = [
  EditorView.perLineTextDirection.of(true),
  ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;
      constructor(view: EditorView) {
        this.decorations = drawnLines(view);
      }
      update(update: ViewUpdate): void {
        if (update.docChanged || update.viewportChanged) this.decorations = drawnLines(update.view);
      }
    },
    { decorations: (plugin) => plugin.decorations },
  ),
];
