// The review on the page itself: the changed blocks of the document are tinted (classes on the elements the page
// keeps by id), the current change is marked and scrolled to, and the buttons of the bars are heard on the page root.

import { documents } from "../../core/doc/document.svelte";
import { review, type ReviewStore } from "./store.svelte";

const MARKS = ["rv-added", "rv-edited", "rv-current"] as const;

/** Tints the blocks of the changes (and clears the tint of the ones that are not any more). */
export function paintMarks(store: ReviewStore = review): void {
  const container = documents.host.container;
  if (container === null) return;
  for (const el of container.querySelectorAll(".rv-added, .rv-edited, .rv-current")) {
    el.classList.remove(...MARKS);
  }
  const view = store.reviewing;
  const doc = documents.view;
  if (
    view === null ||
    view.unit !== "blocks" ||
    doc === null ||
    doc.body.kind !== "markdown" ||
    doc.hash !== view.hash
  )
    return;
  const blocks = doc.body.blocks;
  for (const change of view.changes) {
    const kind = change.kind === "added" ? "rv-added" : "rv-edited";
    for (const block of blocks.slice(change.start, change.start + change.len)) {
      const el = documents.host.element(block.id);
      el?.classList.add(kind);
      if (change.index === store.current) el?.classList.add("rv-current");
    }
  }
}

/** Brings the current change into view. */
export function scrollToCurrent(store: ReviewStore = review): void {
  const container = documents.host.container;
  if (container === null || store.current < 0) return;
  const target = container.querySelector(`[data-rv-change="${store.current}"]`);
  if (target === null) return;
  const calm =
    typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;
  target.scrollIntoView({ block: "center", behavior: calm ? "auto" : "smooth" });
}

/** Listens to the buttons of the bars on the page root; returns what stops it. */
export function listen(root: HTMLElement, store: ReviewStore = review): () => void {
  const onclick = (event: MouseEvent): void => {
    if (!(event.target instanceof Element)) return;
    const button = event.target.closest("[data-rv-act]");
    if (button === null || !root.contains(button)) return;
    const index = Number(button.getAttribute("data-rv-index"));
    if (!Number.isInteger(index)) return;
    event.preventDefault();
    if (button.getAttribute("data-rv-act") === "keep") void store.keep(index);
    else void store.revert(index);
  };
  root.addEventListener("click", onclick);
  return () => root.removeEventListener("click", onclick);
}

/** Paints after every paint of the page; returns what stops it. */
export function startMarks(store: ReviewStore = review): () => void {
  return documents.onPainted(() => {
    paintMarks(store);
    scrollToCurrent(store);
  });
}
