import { describe, expect, it } from "vitest";
import {
  isUnsaved,
  transition,
  type Base,
  type DocEvent,
  type DocState,
  type Effect,
  type SavePolicy,
} from "./docMachine";

const base: Base = { hash: "h1", text: "a\n" };
const next: Base = { hash: "h2", text: "b\n" };
const explicit: SavePolicy = { kind: "explicit" };
const auto: SavePolicy = { kind: "auto", delayMs: 800 };

const STATES: DocState[] = [
  { s: "loading" },
  { s: "clean", base },
  { s: "dirty", base },
  { s: "saving", base, then: null, editedSince: false },
  { s: "saving", base, then: { kind: "window" }, editedSince: true },
  { s: "conflict", base, disk: "h9" },
  { s: "conflict", base, disk: null },
  { s: "saveError", base, error: "io" },
  { s: "unsavedPrompt", back: { s: "dirty", base }, leave: { kind: "workspace" } },
  { s: "unsavedPrompt", back: { s: "conflict", base, disk: null }, leave: { kind: "view" } },
  { s: "closed" },
];

const EVENTS: DocEvent[] = [
  { e: "loaded", base: next },
  { e: "edited" },
  { e: "reverted" },
  { e: "save" },
  { e: "autosaveDue" },
  { e: "saved", base: next },
  { e: "savedAs", path: "a-mine.md", sameFile: false },
  { e: "savedAs", path: "a.md", sameFile: true },
  { e: "conflict", disk: "h9" },
  { e: "conflict", disk: null },
  { e: "saveFailed", error: "io" },
  { e: "retry" },
  { e: "diskChanged", hash: "h1" },
  { e: "diskChanged", hash: "h7" },
  { e: "diskDeleted" },
  { e: "reloadTheirs" },
  { e: "keepMine" },
  { e: "saveAsNew" },
  { e: "leave", leave: { kind: "document", path: "b.md" } },
  { e: "discard" },
  { e: "keepEditing" },
];

const types = (effects: Effect[]) => effects.map((e) => e.type);

describe("document machine", () => {
  it("answers every state x event pair with a state, under both policies", () => {
    for (const policy of [explicit, auto]) {
      for (const state of STATES) {
        for (const event of EVENTS) {
          const step = transition(state, event, policy);
          expect(step.state.s).toBeTypeOf("string");
          // a save never starts from a state without edits
          if (!isUnsaved(state) && state.s !== "loading") {
            expect(types(step.effects)).not.toContain("save");
          }
        }
      }
    }
  });

  it("goes clean -> dirty -> saving -> clean and tells the backend both ways", () => {
    let step = transition({ s: "clean", base }, { e: "edited" }, explicit);
    expect(step.state.s).toBe("dirty");
    expect(step.effects).toContainEqual({ type: "setUnsaved", unsaved: true });
    step = transition(step.state, { e: "save" }, explicit);
    expect(step.state.s).toBe("saving");
    expect(step.effects).toContainEqual({ type: "save", against: "base" });
    step = transition(step.state, { e: "saved", base: next }, explicit);
    expect(step.state).toEqual({ s: "clean", base: next });
    expect(step.effects).toContainEqual({ type: "setUnsaved", unsaved: false });
  });

  it("stays dirty when the person typed while the save ran", () => {
    let state: DocState = { s: "saving", base, then: null, editedSince: false };
    state = transition(state, { e: "edited" }, explicit).state;
    const step = transition(state, { e: "saved", base: next }, explicit);
    expect(step.state).toEqual({ s: "dirty", base: next });
  });

  it("never overwrites a changed file: a disk change while dirty is a conflict", () => {
    const step = transition({ s: "dirty", base }, { e: "diskChanged", hash: "h7" }, explicit);
    expect(step.state).toEqual({ s: "conflict", base, disk: "h7" });
    expect(types(step.effects)).toEqual(["cancelAutosave", "fetchDisk"]);
    // the same version again (our own save echoed) is no news
    expect(
      transition({ s: "dirty", base }, { e: "diskChanged", hash: "h1" }, explicit).state.s,
    ).toBe("dirty");
  });

  it("reloads a clean document in place and leaves a deleted clean one to the view", () => {
    const changed = transition({ s: "clean", base }, { e: "diskChanged", hash: "h7" }, explicit);
    expect(changed.state.s).toBe("loading");
    expect(types(changed.effects)).toEqual(["load"]);
    const deleted = transition({ s: "clean", base }, { e: "diskDeleted" }, explicit);
    expect(deleted.effects).toEqual([{ type: "leave", leave: { kind: "view" } }]);
  });

  it("resolves a conflict three ways", () => {
    const conflict: DocState = { s: "conflict", base, disk: "h9" };
    expect(types(transition(conflict, { e: "reloadTheirs" }, explicit).effects)).toEqual([
      "setUnsaved",
      "load",
    ]);
    expect(transition(conflict, { e: "keepMine" }, explicit).effects).toEqual([
      { type: "save", against: "disk" },
    ]);
    expect(transition(conflict, { e: "saveAsNew" }, explicit).effects).toEqual([
      { type: "saveAs", exact: false },
    ]);
    const gone: DocState = { s: "conflict", base, disk: null };
    expect(transition(gone, { e: "keepMine" }, explicit).effects).toEqual([
      { type: "saveAs", exact: true },
    ]);
    const saving = transition(conflict, { e: "saveAsNew" }, explicit).state;
    const opened = transition(
      saving,
      { e: "savedAs", path: "x-mine.md", sameFile: false },
      explicit,
    );
    expect(opened.effects).toContainEqual({
      type: "leave",
      leave: { kind: "open", path: "x-mine.md" },
    });
  });

  it("asks before leaving unsaved edits; discard, keep editing and save-then-leave", () => {
    const dirty: DocState = { s: "dirty", base };
    const ask = transition(dirty, { e: "leave", leave: { kind: "window" } }, explicit);
    expect(ask.state.s).toBe("unsavedPrompt");
    expect(transition(ask.state, { e: "keepEditing" }, explicit).state).toEqual(dirty);
    const discard = transition(ask.state, { e: "discard" }, explicit);
    expect(discard.state.s).toBe("closed");
    expect(discard.effects).toContainEqual({ type: "leave", leave: { kind: "window" } });
    const save = transition(ask.state, { e: "save" }, explicit);
    expect(save.state).toMatchObject({ s: "saving", then: { kind: "window" } });
    const done = transition(save.state, { e: "saved", base: next }, explicit);
    expect(done.effects).toContainEqual({ type: "leave", leave: { kind: "window" } });
  });

  it("autosaves: edits schedule, leaving saves first instead of asking", () => {
    const step = transition({ s: "clean", base }, { e: "edited" }, auto);
    expect(step.effects).toContainEqual({ type: "scheduleAutosave", delayMs: 800 });
    const due = transition(step.state, { e: "autosaveDue" }, auto);
    expect(due.state.s).toBe("saving");
    const leaving = transition({ s: "dirty", base }, { e: "leave", leave: { kind: "view" } }, auto);
    expect(leaving.state).toMatchObject({ s: "saving", then: { kind: "view" } });
    // a conflict is never autosaved
    const conflict: DocState = { s: "conflict", base, disk: "h9" };
    expect(transition(conflict, { e: "autosaveDue" }, auto).state).toEqual(conflict);
  });

  it("keeps the unsaved flag right through random walks", () => {
    let seed = 42;
    const pick = (n: number) => {
      seed = (seed * 1103515245 + 12345) & 0x7fffffff;
      return seed % n;
    };
    for (const policy of [explicit, auto]) {
      for (let walk = 0; walk < 200; walk++) {
        let state: DocState = { s: "loading" };
        let told = false;
        for (let i = 0; i < 60; i++) {
          const event = EVENTS[pick(EVENTS.length)] as DocEvent;
          const step = transition(state, event, policy);
          for (const effect of step.effects) {
            if (effect.type === "setUnsaved") told = effect.unsaved;
          }
          state = step.state;
          if (state.s === "clean") expect(told).toBe(false);
          if (state.s === "dirty") expect(told).toBe(true);
        }
      }
    }
  });
});
