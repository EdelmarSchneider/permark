import { describe, expect, it } from "vitest";
import { TagQueryState } from "./tagQueryState.svelte";

const COUNTS = [
  { name: "spec", count: 12 },
  { name: "feed", count: 5 },
  { name: "watcher", count: 4 },
];

function key(name: string, extra: Partial<KeyboardEvent> = {}): KeyboardEvent {
  return { key: name, shiftKey: false, ...extra } as KeyboardEvent;
}

function state(): TagQueryState {
  return new TagQueryState(() => COUNTS);
}

describe("TagQueryState", () => {
  it("turns finished tag tokens into chips as the text changes", () => {
    const q = state();
    q.setText("live tag:spec ");
    expect(q.chips).toEqual(["spec"]);
    expect(q.text).toBe("live");
    expect(q.words).toBe("live");
    expect(q.partial).toBeNull();
  });

  it("suggests tags for the token being typed, without the chosen ones", () => {
    const q = state();
    q.setText("tag:");
    expect(q.suggestions.map((s) => s.tag.name)).toEqual(["spec", "feed", "watcher"]);
    q.setText("tag:wa");
    expect(q.suggestions.map((s) => s.tag.name)).toEqual(["watcher"]);
    q.pick("watcher");
    q.setText("tag:");
    expect(q.suggestions.map((s) => s.tag.name)).toEqual(["spec", "feed"]);
  });

  it("picking a suggestion makes a chip and removes the unfinished token", () => {
    const q = state();
    q.setText("notes tag:fe");
    q.pick("feed");
    expect(q.chips).toEqual(["feed"]);
    expect(q.text.trim()).toBe("notes");
    expect(q.partial).toBeNull();
  });

  it("moves through the suggestions with the arrows, and takes one with Enter or Tab", () => {
    const q = state();
    q.setText("tag:");
    expect(q.handleKey(key("ArrowDown"))).toBe(true);
    expect(q.active).toBe(1);
    expect(q.handleKey(key("ArrowUp"))).toBe(true);
    expect(q.handleKey(key("ArrowUp"))).toBe(true);
    expect(q.active).toBe(2);
    expect(q.handleKey(key("Enter"))).toBe(true);
    expect(q.chips).toEqual(["watcher"]);
    q.setText("tag:fe");
    expect(q.handleKey(key("Tab"))).toBe(true);
    expect(q.chips).toEqual(["watcher", "feed"]);
  });

  it("makes a chip from a tag nobody has when Enter finishes it", () => {
    const q = state();
    q.setText("tag:nope");
    expect(q.suggestions).toEqual([]);
    expect(q.handleKey(key("Enter"))).toBe(true);
    expect(q.chips).toEqual(["nope"]);
  });

  it("leaves Enter and the arrows alone when no token is being typed", () => {
    const q = state();
    q.setText("plain words");
    expect(q.handleKey(key("Enter"))).toBe(false);
    expect(q.handleKey(key("ArrowDown"))).toBe(false);
    expect(q.handleKey(key("Tab"))).toBe(false);
  });

  it("Backspace in an empty box removes the last chip", () => {
    const q = state();
    q.setText("tag:spec tag:feed ");
    expect(q.chips).toEqual(["spec", "feed"]);
    expect(q.handleKey(key("Backspace"))).toBe(true);
    expect(q.chips).toEqual(["spec"]);
    q.setText("x");
    expect(q.handleKey(key("Backspace"))).toBe(false);
  });

  it("knows when nothing is typed or chosen", () => {
    const q = state();
    expect(q.isEmpty).toBe(true);
    q.setText("tag:spec ");
    expect(q.isEmpty).toBe(false);
    q.clear();
    expect(q.isEmpty).toBe(true);
    expect(q.chips).toEqual([]);
  });
});
