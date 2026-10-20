// jsdom has no layout. ProseMirror measures the caret to scroll it into view, which needs `Range.getClientRects` and
// `getBoundingClientRect`: empty answers are enough. Imported by the tests that run the editor.

if (typeof Range !== "undefined") {
  const empty = Object.assign([] as DOMRect[], { item: () => null });
  Range.prototype.getClientRects ??= () => empty as unknown as DOMRectList;
  Range.prototype.getBoundingClientRect ??= () => new DOMRect(0, 0, 0, 0);
}
if (typeof document !== "undefined" && !("elementFromPoint" in document)) {
  Object.assign(document, { elementFromPoint: () => null });
}

export {};
