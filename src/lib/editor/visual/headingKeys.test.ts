import { describe, expect, it } from "vitest";
import { matchHeadingShortcut, takeHeadingShortcut, type HeadingKeyEvent } from "./headingKeys";

const key = (
  patch: Partial<HeadingKeyEvent> & Pick<HeadingKeyEvent, "key" | "code">,
): HeadingKeyEvent => ({
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  ...patch,
});

describe("matchHeadingShortcut", () => {
  it("sets heading 1 from the physical digit when the layout types a superscript", () => {
    const event = key({ key: "¹", code: "Digit1", ctrlKey: true, shiftKey: true });
    const applied: string[] = [];
    expect(takeHeadingShortcut(event, (block) => applied.push(block))).toBe(true);
    expect(applied).toEqual(["h1"]);
    expect(matchHeadingShortcut(event)).toBe("h1");
  });

  it("sets a paragraph from Digit0 and heading 6 from Digit6", () => {
    expect(
      matchHeadingShortcut(key({ key: ")", code: "Digit0", ctrlKey: true, shiftKey: true })),
    ).toBe("paragraph");
    expect(
      matchHeadingShortcut(key({ key: "^", code: "Digit6", ctrlKey: true, shiftKey: true })),
    ).toBe("h6");
  });

  it("accepts the numeric keypad the same way", () => {
    expect(
      matchHeadingShortcut(key({ key: "1", code: "Numpad1", ctrlKey: true, shiftKey: true })),
    ).toBe("h1");
    expect(
      matchHeadingShortcut(key({ key: "0", code: "Numpad0", ctrlKey: true, shiftKey: true })),
    ).toBe("paragraph");
  });

  it("treats Command+Shift+digit as the same chord", () => {
    expect(
      matchHeadingShortcut(key({ key: "@", code: "Digit2", metaKey: true, shiftKey: true })),
    ).toBe("h2");
  });

  it("does not treat Ctrl+Alt+1 as a heading shortcut", () => {
    const applied: string[] = [];
    const event = key({ key: "¹", code: "Digit1", ctrlKey: true, altKey: true });
    expect(takeHeadingShortcut(event, (block) => applied.push(block))).toBe(false);
    expect(applied).toEqual([]);
    expect(matchHeadingShortcut(event)).toBeNull();
  });

  it("leaves Ctrl+0, Ctrl+Alt+letter and other digits alone", () => {
    expect(matchHeadingShortcut(key({ key: "0", code: "Digit0", ctrlKey: true }))).toBeNull();
    expect(
      matchHeadingShortcut(key({ key: "c", code: "KeyC", ctrlKey: true, altKey: true })),
    ).toBeNull();
    expect(
      matchHeadingShortcut(key({ key: "&", code: "Digit7", ctrlKey: true, shiftKey: true })),
    ).toBeNull();
    expect(
      matchHeadingShortcut(
        key({ key: "1", code: "Digit1", ctrlKey: true, shiftKey: true, altKey: true }),
      ),
    ).toBeNull();
  });
});
