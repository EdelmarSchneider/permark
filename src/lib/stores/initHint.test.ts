import { beforeEach, describe, expect, it, vi } from "vitest";

// The backend is replaced: these tests are about what the hint remembers, and for which folder.
const fake = vi.hoisted(() => ({ saveSession: vi.fn(async () => {}) }));
vi.mock("../api", () => ({ api: { ...fake, on: async () => () => {} } }));

import { session } from "./session.svelte";
import { ui } from "./ui.svelte";

describe("the hint to keep settings with the project", () => {
  beforeEach(() => {
    fake.saveSession.mockClear();
    session.load(null);
    ui.initHintSnoozed.clear();
  });

  it("remembers Never for this folder in the folder's own saved session", async () => {
    expect(session.initHintNever).toBe(false);
    session.neverHintInit();
    expect(session.initHintNever).toBe(true);
    expect(session.snapshot().initHintNever).toBe(true);
    await session.flush();
    expect(fake.saveSession).toHaveBeenCalledWith(expect.objectContaining({ initHintNever: true }));
  });

  it("does not carry Never over to the next folder that is opened", () => {
    session.neverHintInit();
    session.load({ ...session.snapshot(), initHintNever: false });
    expect(session.initHintNever).toBe(false);
    session.load(null);
    expect(session.initHintNever).toBe(false);
  });

  it("restores Never when the folder is opened again", () => {
    session.load({ ...session.snapshot(), initHintNever: true });
    expect(session.initHintNever).toBe(true);
  });

  it("forgets Not now per folder, and only for as long as the app runs", () => {
    ui.initHintSnoozed.add("F:/work/one");
    expect(ui.initHintSnoozed.has("F:/work/one")).toBe(true);
    expect(ui.initHintSnoozed.has("F:/work/two")).toBe(false);
    // nothing about it is saved
    expect(session.snapshot()).not.toHaveProperty("initHintSnoozed");
    expect(fake.saveSession).not.toHaveBeenCalled();
  });
});
