import { describe, expect, it } from "vitest";
import {
  comboFromEvent,
  comboLabel,
  isSingleKey,
  normalizeCombo,
  parseCombo,
  type KeyEventLike,
} from "./combo";

function key(init: Partial<KeyEventLike> & { key: string }): KeyEventLike {
  return { ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, ...init };
}

describe("combos", () => {
  it("parses the grammar and writes one spelling", () => {
    expect(parseCombo("Mod+Shift+p")).toEqual({
      mod: true,
      ctrl: false,
      alt: false,
      shift: true,
      key: "p",
    });
    expect(normalizeCombo("Shift+Mod+p")).toBe("Mod+Shift+p");
    expect(normalizeCombo("Alt+Mod+Shift+ArrowLeft")).toBe("Mod+Alt+Shift+ArrowLeft");
    expect(normalizeCombo("Mod+=")).toBe("Mod+=");
    expect(normalizeCombo("Mod+-")).toBe("Mod+-");
    expect(normalizeCombo("Mod+,")).toBe("Mod+,");
    expect(normalizeCombo("F7")).toBe("F7");
    expect(normalizeCombo("Mod+F24")).toBe("Mod+F24");
  });

  it("Ctrl is the Control key: its own modifier on macOS, Mod on Windows | Linux (one combo there)", () => {
    expect(parseCombo("Ctrl+Tab")).toEqual({
      mod: false,
      ctrl: true,
      alt: false,
      shift: false,
      key: "Tab",
    });
    expect(normalizeCombo("Ctrl+Tab")).toBe("Mod+Tab");
    expect(normalizeCombo("Shift+Ctrl+Tab")).toBe("Mod+Shift+Tab");
    expect(normalizeCombo("Ctrl+Mod+p")).toBe("Mod+p");
    expect(normalizeCombo("Ctrl+Tab", true)).toBe("Ctrl+Tab");
    expect(normalizeCombo("Alt+Ctrl+Mod+Shift+Tab", true)).toBe("Mod+Ctrl+Alt+Shift+Tab");
    expect(comboLabel("Ctrl+Shift+Tab", false)).toBe("Ctrl+Shift+Tab");
    expect(comboLabel("Ctrl+Shift+Tab", true)).toBe("⌃⇧⇥");
    expect(comboLabel("Mod+Ctrl+Alt+p", true)).toBe("⌃⌥⌘P");
    expect(isSingleKey("Ctrl+Tab")).toBe(false);
  });

  it("refuses what is not a combo", () => {
    for (const text of [
      "",
      "Mod+",
      "Mod++",
      "Ctrl+Ctrl+p",
      "Cmd+p",
      "Mod+Mod+p",
      "Mod+P",
      "Mod+pp",
      "Mod+F25",
      "Mod+ArrowSideways",
      "Mod+ p",
      "x".repeat(41),
      "Mod+é",
      "Mod+\u0001",
    ]) {
      expect(parseCombo(text), JSON.stringify(text)).toBeNull();
    }
  });

  it("tells a single key (it types) from a chord", () => {
    expect(isSingleKey("t")).toBe(true);
    expect(isSingleKey("Shift+t")).toBe(true);
    expect(isSingleKey("F7")).toBe(true);
    expect(isSingleKey("Alt+ArrowLeft")).toBe(false);
    expect(isSingleKey("Mod+p")).toBe(false);
    expect(isSingleKey("nonsense+")).toBe(false);
  });
});

describe("the combo of a key event", () => {
  it("uses Ctrl on Windows | Linux and Cmd on macOS", () => {
    expect(
      comboFromEvent(key({ key: "P", ctrlKey: true, shiftKey: true, code: "KeyP" }), false),
    ).toBe("Mod+Shift+p");
    expect(comboFromEvent(key({ key: "p", metaKey: true, code: "KeyP" }), true)).toBe("Mod+p");
    // Windows | Linux: the Windows | Super key never matches; macOS: the Control key is `Ctrl`
    expect(comboFromEvent(key({ key: "p", metaKey: true }), false)).toBeNull();
    expect(comboFromEvent(key({ key: "p", ctrlKey: true, code: "KeyP" }), true)).toBe("Ctrl+p");
    expect(comboFromEvent(key({ key: "Tab", ctrlKey: true, shiftKey: true }), true)).toBe(
      "Ctrl+Shift+Tab",
    );
    expect(comboFromEvent(key({ key: "Tab", ctrlKey: true, shiftKey: true }), false)).toBe(
      "Mod+Shift+Tab",
    );
  });

  it("reads plus | underscore as the keys they are shifted from, and drops the Shift the character carries", () => {
    expect(comboFromEvent(key({ key: "+", ctrlKey: true, shiftKey: true }), false)).toBe("Mod+=");
    expect(comboFromEvent(key({ key: "=", ctrlKey: true }), false)).toBe("Mod+=");
    expect(comboFromEvent(key({ key: "_", ctrlKey: true, shiftKey: true }), false)).toBe("Mod+-");
    expect(comboFromEvent(key({ key: ",", ctrlKey: true }), false)).toBe("Mod+,");
  });

  it("keeps Shift on letters, digits and named keys", () => {
    expect(comboFromEvent(key({ key: "F7", shiftKey: true }), false)).toBe("Shift+F7");
    expect(comboFromEvent(key({ key: "ArrowLeft", altKey: true }), false)).toBe("Alt+ArrowLeft");
    // Shift+1 types `!` on a US layout; the physical digit is the key
    expect(
      comboFromEvent(key({ key: "!", ctrlKey: true, shiftKey: true, code: "Digit1" }), false),
    ).toBe("Mod+Shift+1");
    expect(comboFromEvent(key({ key: " ", ctrlKey: true }), false)).toBe("Mod+Space");
  });

  it("falls back to the physical letter on other layouts and for macOS Option characters", () => {
    expect(
      comboFromEvent(key({ key: "з", ctrlKey: true, shiftKey: true, code: "KeyP" }), false),
    ).toBe("Mod+Shift+p");
    expect(comboFromEvent(key({ key: "ç", metaKey: true, altKey: true, code: "KeyC" }), true)).toBe(
      "Mod+Alt+c",
    );
    expect(comboFromEvent(key({ key: "ç", ctrlKey: true }), false)).toBeNull();
  });

  it("names nothing for modifiers alone, compositions, dead keys and AltGr", () => {
    expect(comboFromEvent(key({ key: "Control", ctrlKey: true }), false)).toBeNull();
    expect(comboFromEvent(key({ key: "Shift", shiftKey: true }), false)).toBeNull();
    expect(comboFromEvent(key({ key: "p", ctrlKey: true, isComposing: true }), false)).toBeNull();
    expect(comboFromEvent(key({ key: "p", ctrlKey: true, keyCode: 229 }), false)).toBeNull();
    expect(comboFromEvent(key({ key: "Dead", ctrlKey: true }), false)).toBeNull();
    expect(comboFromEvent(key({ key: "Process" }), false)).toBeNull();
    expect(
      comboFromEvent(
        key({
          key: "e",
          ctrlKey: true,
          altKey: true,
          getModifierState: (name) => name === "AltGraph",
        }),
        false,
      ),
    ).toBeNull();
  });
});

describe("how a combo is written", () => {
  it("Windows | Linux: names; macOS: symbols in the order Option, Shift, Command, key", () => {
    expect(comboLabel("Mod+Shift+p", false)).toBe("Ctrl+Shift+P");
    expect(comboLabel("Mod+Shift+p", true)).toBe("⇧⌘P");
    expect(comboLabel("Mod+Alt+Shift+c", true)).toBe("⌥⇧⌘C");
    expect(comboLabel("Alt+ArrowLeft", false)).toBe("Alt+Left");
    expect(comboLabel("Alt+ArrowLeft", true)).toBe("⌥←");
    expect(comboLabel("Mod+=", false)).toBe("Ctrl+=");
    expect(comboLabel("F7", false)).toBe("F7");
    expect(comboLabel("garbage+", false)).toBe("garbage+");
  });
});
