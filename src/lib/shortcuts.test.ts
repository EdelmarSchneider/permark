import { describe, expect, it } from "vitest";
import {
  isMacPlatform,
  matchShortcut,
  parseCombo,
  shortcutLabel,
  type KeyInfo,
  type ShortcutAction,
} from "./shortcuts";

const press = (key: string, mods: Partial<KeyInfo> = {}, code?: string): KeyInfo => ({
  key,
  code,
  ctrlKey: false,
  metaKey: false,
  altKey: false,
  shiftKey: false,
  ...mods,
});
const ctrl = (key: string, extra: Partial<KeyInfo> = {}, code?: string): KeyInfo =>
  press(key, { ctrlKey: true, ...extra }, code);

describe("matchShortcut", () => {
  const table: [string, KeyInfo, ShortcutAction][] = [
    ["Ctrl+O", ctrl("o"), "openFolder"],
    ["Ctrl+Shift+O", ctrl("O", { shiftKey: true }), "openFile"],
    ["Ctrl+P", ctrl("p"), "quickOpen"],
    ["Ctrl+T", ctrl("t"), "searchEverywhere"],
    ["Ctrl+Shift+P", ctrl("P", { shiftKey: true }), "commands"],
    ["Ctrl+Shift+F", ctrl("F", { shiftKey: true }), "searchAll"],
    ["Ctrl+Shift+X", ctrl("X", { shiftKey: true }), "explorer"],
    ["Ctrl+Shift+T", ctrl("T", { shiftKey: true }), "tagsView"],
    ["T (reading)", press("t"), "editTags"],
    ["E (reading)", press("e"), "edit"],
    ["Ctrl+E", ctrl("e"), "toggleEdit"],
    ["Ctrl+S", ctrl("s"), "save"],
    ["Ctrl+N", ctrl("n"), "newFile"],
    ["Ctrl+F", ctrl("f"), "find"],
    ["Alt+Left", press("ArrowLeft", { altKey: true }), "back"],
    ["Alt+Right", press("ArrowRight", { altKey: true }), "forward"],
    ["F7", press("F7"), "nextChange"],
    ["Shift+F7", press("F7", { shiftKey: true }), "prevChange"],
    ["Ctrl+Shift+L", ctrl("L", { shiftKey: true }), "toggleFollow"],
    ["Ctrl+Alt+C", ctrl("c", { altKey: true }), "copyReference"],
    ["Shift+Alt+C", press("C", { altKey: true, shiftKey: true }), "copyPath"],
    ["Ctrl+Shift+E", ctrl("E", { shiftKey: true }), "openExternal"],
    ["Ctrl+B", ctrl("b"), "toggleLeft"],
    ["Ctrl+Alt+B", ctrl("b", { altKey: true }), "toggleRight"],
    ["Ctrl+=", ctrl("=", {}, "Equal"), "zoomIn"],
    ["Ctrl++", ctrl("+", { shiftKey: true }, "Equal"), "zoomIn"],
    ["Ctrl+-", ctrl("-", {}, "Minus"), "zoomOut"],
    ["Ctrl+0", ctrl("0", {}, "Digit0"), "zoomReset"],
    ["Ctrl+Alt+W", ctrl("w", { altKey: true }), "cycleWidth"],
    ["Ctrl+Alt+O", ctrl("o", { altKey: true }), "toggleOutline"],
    ["Ctrl+Alt+E", ctrl("e", { altKey: true }), "toggleEditMode"],
    ["Ctrl+,", ctrl(",", {}, "Comma"), "settings"],
  ];

  it.each(table)("%s", (_name, event, action) => {
    expect(matchShortcut(event)).toBe(action);
  });

  it("uses the physical key when Alt changes the character (macOS)", () => {
    expect(matchShortcut(ctrl("ç", { altKey: true }, "KeyC"))).toBe("copyReference");
    expect(matchShortcut(ctrl("ø", { altKey: true }, "KeyO"))).toBe("toggleOutline");
  });

  it("treats Cmd like Ctrl", () => {
    expect(matchShortcut(press("p", { metaKey: true }))).toBe("quickOpen");
  });

  it("leaves Ctrl+K (the editor's link) and plain typing alone", () => {
    expect(matchShortcut(ctrl("k"))).toBeNull();
    expect(matchShortcut(ctrl("i"))).toBeNull();
    expect(matchShortcut(press("f"))).toBeNull();
    expect(matchShortcut(press("ArrowLeft"))).toBeNull();
    expect(matchShortcut(ctrl("c"))).toBeNull();
  });

  it("T needs no modifier, and the other letters stay free", () => {
    expect(matchShortcut(press("T", { shiftKey: true }))).toBeNull();
    expect(matchShortcut(press("t", { altKey: true }))).toBeNull();
    expect(matchShortcut(ctrl("t", { altKey: true }))).toBeNull();
    expect(matchShortcut(press("E", { shiftKey: true }))).toBeNull();
    expect(matchShortcut(press("e", { altKey: true }))).toBeNull();
    expect(matchShortcut(press("x"))).toBeNull();
  });
});

describe("parseCombo", () => {
  it("reads the modifiers and the key of a combo", () => {
    expect(parseCombo("Ctrl+Shift+P")).toEqual({ ctrl: true, alt: false, shift: true, key: "P" });
    expect(parseCombo("Shift+Alt+C")).toEqual({ ctrl: false, alt: true, shift: true, key: "C" });
    expect(parseCombo("F7")).toEqual({ ctrl: false, alt: false, shift: false, key: "F7" });
    expect(parseCombo("T")).toEqual({ ctrl: false, alt: false, shift: false, key: "T" });
  });

  it("keeps a key that is a plus or a comma", () => {
    expect(parseCombo("Ctrl++")).toEqual({ ctrl: true, alt: false, shift: false, key: "+" });
    expect(parseCombo("Ctrl+,")).toEqual({ ctrl: true, alt: false, shift: false, key: "," });
  });
});

describe("shortcutLabel", () => {
  const plain = (combo: string): string => combo;

  it("hands the combo to the language's key names off macOS", () => {
    expect(shortcutLabel("Ctrl+Shift+P", false, (c) => c.replace("Ctrl", "Strg"))).toBe(
      "Strg+Shift+P",
    );
    expect(shortcutLabel("F7", false, plain)).toBe("F7");
  });

  it("writes Command, Option and Shift symbols on macOS", () => {
    expect(shortcutLabel("Ctrl+T", true, plain)).toBe("⌘T");
    expect(shortcutLabel("Ctrl+Shift+P", true, plain)).toBe("⇧⌘P");
    expect(shortcutLabel("Ctrl+Alt+O", true, plain)).toBe("⌥⌘O");
    expect(shortcutLabel("Shift+Alt+C", true, plain)).toBe("⌥⇧C");
    expect(shortcutLabel("Shift+F7", true, plain)).toBe("⇧F7");
    expect(shortcutLabel("T", true, plain)).toBe("T");
  });
});

describe("isMacPlatform", () => {
  it("recognises the macOS webview and nothing else", () => {
    expect(
      isMacPlatform("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15"),
    ).toBe(true);
    expect(
      isMacPlatform("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Edg/120"),
    ).toBe(false);
    expect(isMacPlatform("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15")).toBe(false);
  });
});
