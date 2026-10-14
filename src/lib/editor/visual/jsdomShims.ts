// jsdom has no layout. ProseMirror measures the caret to scroll it into view after commands, which needs `Range.getClientRects`
// and `getBoundingClientRect`: empty answers are enough, there is nothing to scroll. Imported by the tests that run the editor.

if (typeof Range !== "undefined") {
  const empty = Object.assign([] as DOMRect[], { item: () => null }) as unknown as DOMRectList;
  Range.prototype.getClientRects ??= () => empty;
  Range.prototype.getBoundingClientRect ??= () => new DOMRect(0, 0, 0, 0);
}
