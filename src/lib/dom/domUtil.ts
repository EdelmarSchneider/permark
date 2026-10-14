// Small DOM helpers shared by the document view, the selection popover and Follow.

/** True while the user has text selected (Follow must not yank the page away then). */
export function isTextSelected(): boolean {
  const selection = window.getSelection();
  return !!selection && !selection.isCollapsed && selection.toString().trim() !== "";
}

/** Text of a heading without the injected "#" anchor. */
export function headingText(heading: Element): string {
  const clone = heading.cloneNode(true) as Element;
  clone.querySelectorAll(".anchor").forEach((a) => a.remove());
  return (clone.textContent ?? "").replace(/^#/, "").trim();
}

const HEADING = /^H[1-6]$/;

/** The nearest heading above (or at) the block that contains `node`. */
export function sectionOf(node: Node, body: HTMLElement): string | null {
  let el: Element | null =
    node.nodeType === Node.ELEMENT_NODE ? (node as Element) : node.parentElement;
  while (el && el.parentElement !== body) el = el.parentElement;
  while (el) {
    if (HEADING.test(el.tagName)) return headingText(el);
    el = el.previousElementSibling;
  }
  return null;
}

export type SelectionInfo = { text: string; section: string | null; rect: DOMRect };

/** The selection if it lies inside `body`. */
export function selectionIn(body: HTMLElement): SelectionInfo | null {
  const selection = window.getSelection();
  if (!selection || selection.isCollapsed || selection.rangeCount === 0) return null;
  const range = selection.getRangeAt(0);
  if (!body.contains(range.commonAncestorContainer)) return null;
  const text = selection.toString().trim();
  if (text === "") return null;
  return {
    text,
    section: sectionOf(selection.anchorNode ?? range.startContainer, body),
    rect: range.getBoundingClientRect(),
  };
}

/** Finds an element by id inside `root` without colliding with the app's own ids. */
export function findById(root: HTMLElement, id: string): HTMLElement | null {
  const candidates = [id];
  try {
    const decoded = decodeURIComponent(id);
    if (decoded !== id) candidates.push(decoded);
  } catch {
    // not percent-encoded
  }
  for (const candidate of candidates) {
    const hit = root.querySelector<HTMLElement>(`[id="${CSS.escape(candidate)}"]`);
    if (hit) return hit;
  }
  return null;
}

/** The block-level element to scroll to for an anchor target (an `a.anchor` inside a heading, a footnote item, ...). */
export function anchorTarget(el: HTMLElement): HTMLElement {
  const heading = el.closest<HTMLElement>("h1, h2, h3, h4, h5, h6");
  return heading ?? el;
}

/** True inside a field the user types into: plain-letter shortcuts must leave those keys alone. */
export function isEditable(target: EventTarget | null): boolean {
  if (typeof Element === "undefined" || !(target instanceof Element)) return false;
  return (
    target.closest("input, textarea, select, [contenteditable]:not([contenteditable=false])") !==
    null
  );
}
