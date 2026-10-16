// Input methods (Pinyin, kana, Hangul) pick their candidates with Enter and move through them with the arrow keys. While
// one is composing, those keys belong to it and must not reach the shortcuts of a text field.

/**
 * Whether a key press belongs to an input method that is composing text. Chromium reports an IME key as `keyCode` 229,
 * and WebKit sets `isComposing` or sends the Enter that confirms a candidate as a 229 right after `compositionend`.
 */
export function isComposingKey(e: Pick<KeyboardEvent, "isComposing" | "keyCode">): boolean {
  return e.isComposing || e.keyCode === 229;
}
