// The margin of the page: keeps the bars true while the page and the comments change, and turns a click on a bar into
// "show that comment in the tab". Runs while the page exists (a `docDecoration`).

import { documents } from "../../core/doc/document.svelte";
import { ui } from "../../core/shell/ui.svelte";
import { applyMarks, clearMarks, commentAt } from "./marks";
import { PANEL } from "./ids";
import { comments } from "./store.svelte";

/** Marks the page and follows it; returns what takes the marks off again. */
export function decorateMargin(root: HTMLElement): () => void {
  const sync = (): void => applyMarks(root, comments.bars(documents.shown));
  // the page painted again (blocks made | replaced)
  const unpaint = documents.onPainted(sync);
  // the comments (or the document, or its blocks) changed
  const stop = $effect.root(() => {
    $effect(() => {
      void comments.all;
      sync();
    });
  });
  const onclick = (event: MouseEvent): void => {
    const id = commentAt(event);
    if (id === null) return;
    comments.show(id);
    ui.showSidebar(PANEL);
  };
  root.addEventListener("click", onclick);
  return () => {
    unpaint();
    stop();
    root.removeEventListener("click", onclick);
    clearMarks(root);
  };
}
