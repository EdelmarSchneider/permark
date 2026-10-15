import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Distribution } from "../types";

// The backend and the update server are replaced: these tests are about when the store asks the server.
const fake = vi.hoisted(() => ({ appDistribution: vi.fn(), checkForUpdate: vi.fn() }));
vi.mock("../api", () => ({
  api: { appDistribution: fake.appDistribution, on: async () => () => {} },
}));
vi.mock("../platform", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../platform")>()),
  isMock: false,
  checkForUpdate: fake.checkForUpdate,
}));

/** The updates store of an app that was built as `distribution`. The stores keep their state, so each test starts fresh. */
async function updatesOf(distribution: Distribution) {
  vi.resetModules();
  fake.appDistribution.mockResolvedValue(distribution);
  return (await import("./updates.svelte")).updates;
}

describe("the app updater", () => {
  beforeEach(() => {
    fake.appDistribution.mockReset();
    fake.checkForUpdate.mockReset();
    fake.checkForUpdate.mockResolvedValue(null);
  });

  it("asks the update server when the person checks", async () => {
    const updates = await updatesOf("standard");
    await updates.check(true);
    expect(fake.checkForUpdate).toHaveBeenCalledOnce();
    expect(updates.state).toEqual({ phase: "upToDate" });
  });

  it("asks nothing in the Microsoft Store build: the Store updates the app", async () => {
    const updates = await updatesOf("store");
    await updates.check(true);
    await updates.check(false);
    expect(fake.checkForUpdate).not.toHaveBeenCalled();
    expect(updates.state).toEqual({ phase: "idle" });
  });

  describe("the check at start", () => {
    beforeEach(() => {
      vi.stubEnv("PROD", true);
      vi.useFakeTimers();
    });
    afterEach(() => {
      vi.useRealTimers();
      vi.unstubAllEnvs();
    });

    it("asks the update server a few seconds after launch", async () => {
      const updates = await updatesOf("standard");
      await updates.scheduleStartupCheck();
      expect(fake.checkForUpdate).not.toHaveBeenCalled();
      await vi.advanceTimersByTimeAsync(60_000);
      expect(fake.checkForUpdate).toHaveBeenCalledOnce();
    });

    it("never happens in the Microsoft Store build", async () => {
      const updates = await updatesOf("store");
      await updates.scheduleStartupCheck();
      await vi.advanceTimersByTimeAsync(60_000);
      expect(fake.checkForUpdate).not.toHaveBeenCalled();
    });
  });
});
