import { describe, expect, it } from "vitest";
import { matchShortcut, type KeyInfo, type ShortcutAction } from "./shortcuts";

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
    ["Ctrl+Shift+F", ctrl("F", { shiftKey: true }), "searchAll"],
    ["Ctrl+Shift+X", ctrl("X", { shiftKey: true }), "explorer"],
    ["Ctrl+Shift+T", ctrl("T", { shiftKey: true }), "tagsView"],
    ["T (reading)", press("t"), "editTags"],
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
    ["Ctrl+,", ctrl(",", {}, "Comma"), "settings"],
  ];

  it.each(table)("%s", (_name, event, action) => {
    expect(matchShortcut(event)).toBe(action);
  });

  it("uses the physical key when Alt changes the character (macOS)", () => {
    expect(matchShortcut(ctrl("ç", { altKey: true }, "KeyC"))).toBe("copyReference");
  });

  it("treats Cmd like Ctrl", () => {
    expect(matchShortcut(press("p", { metaKey: true }))).toBe("quickOpen");
  });

  it("leaves the keys reserved for v0.2 and plain typing alone", () => {
    expect(matchShortcut(ctrl("k"))).toBeNull();
    expect(matchShortcut(ctrl("e"))).toBeNull();
    expect(matchShortcut(ctrl("s"))).toBeNull();
    expect(matchShortcut(press("f"))).toBeNull();
    expect(matchShortcut(press("ArrowLeft"))).toBeNull();
    expect(matchShortcut(ctrl("c"))).toBeNull();
  });

  it("T needs no modifier, and the other letters stay free", () => {
    expect(matchShortcut(press("T", { shiftKey: true }))).toBeNull();
    expect(matchShortcut(press("t", { altKey: true }))).toBeNull();
    expect(matchShortcut(ctrl("t"))).toBeNull();
    expect(matchShortcut(press("e"))).toBeNull();
  });
});
