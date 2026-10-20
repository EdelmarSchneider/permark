import { describe, expect, it } from "vitest";
import { headingChord } from "./plugins";

const press = (
  code: string,
  mods: { ctrl?: boolean; meta?: boolean; shift?: boolean; alt?: boolean; altGr?: boolean },
) => ({
  code,
  ctrlKey: mods.ctrl === true,
  metaKey: mods.meta === true,
  shiftKey: mods.shift === true,
  altKey: mods.alt === true,
  getModifierState: (name: string) => name === "AltGraph" && mods.altGr === true,
});

describe("headingChord", () => {
  it("reads Ctrl+Shift+0-6 by the key's place, whatever character the layout types", () => {
    expect(headingChord(press("Digit1", { ctrl: true, shift: true }))).toBe(1);
    expect(headingChord(press("Digit6", { ctrl: true, shift: true }))).toBe(6);
    expect(headingChord(press("Digit0", { ctrl: true, shift: true }))).toBe(0);
    expect(headingChord(press("Numpad3", { ctrl: true, shift: true }))).toBe(3);
    expect(headingChord(press("Digit2", { meta: true, shift: true }))).toBe(2);
  });

  it("reads Ctrl+Alt+0-6, but never when the Alt is AltGr", () => {
    expect(headingChord(press("Digit4", { ctrl: true, alt: true }))).toBe(4);
    expect(headingChord(press("Digit4", { ctrl: true, alt: true, altGr: true }))).toBeNull();
  });

  it("leaves other keys and other modifiers alone", () => {
    expect(headingChord(press("Digit7", { ctrl: true, shift: true }))).toBeNull();
    expect(headingChord(press("Digit8", { ctrl: true, shift: true }))).toBeNull();
    expect(headingChord(press("KeyA", { ctrl: true, shift: true }))).toBeNull();
    expect(headingChord(press("Digit1", { ctrl: true }))).toBeNull();
    expect(headingChord(press("Digit1", {}))).toBeNull();
    expect(headingChord(press("Digit1", { shift: true }))).toBeNull();
    expect(headingChord(press("Digit1", { ctrl: true, shift: true, alt: true }))).toBeNull();
  });
});
