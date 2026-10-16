import { describe, expect, it } from "vitest";
import { isComposingKey } from "./ime";

describe("isComposingKey", () => {
  it("is true while an input method is composing", () => {
    expect(isComposingKey({ isComposing: true, keyCode: 13 })).toBe(true);
  });

  it("is true for the key code an IME key gets, even after composition has ended", () => {
    expect(isComposingKey({ isComposing: false, keyCode: 229 })).toBe(true);
  });

  it("is false for an ordinary Enter", () => {
    expect(isComposingKey({ isComposing: false, keyCode: 13 })).toBe(false);
  });
});
