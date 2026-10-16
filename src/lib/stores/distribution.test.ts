import { beforeEach, describe, expect, it, vi } from "vitest";

// The backend is replaced: these tests are about what the store makes of its answer.
const fake = vi.hoisted(() => ({ appDistribution: vi.fn() }));
vi.mock("../api", () => ({ api: { ...fake, on: async () => () => {} } }));

/** The store as a fresh start of the app has it: it keeps the first answer, so each test needs its own. */
async function freshStore() {
  vi.resetModules();
  return (await import("./distribution.svelte")).distribution;
}

describe("the distribution the app was built as", () => {
  // Braces: a function that `beforeEach` returns is run after the test as its clean-up, and `mockReset` returns the mock.
  beforeEach(() => {
    fake.appDistribution.mockReset();
  });

  it("is the standard build until the backend has answered", async () => {
    const distribution = await freshStore();
    expect(distribution.value).toBe("standard");
    expect(distribution.isStore).toBe(false);
  });

  it("takes the build the backend names, and asks it once", async () => {
    fake.appDistribution.mockResolvedValue("store");
    const distribution = await freshStore();
    await Promise.all([distribution.load(), distribution.load()]);
    await distribution.load();
    expect(distribution.value).toBe("store");
    expect(distribution.isStore).toBe(true);
    expect(fake.appDistribution).toHaveBeenCalledOnce();
  });

  it("stays the standard build when the backend cannot say, and does not reject", async () => {
    fake.appDistribution.mockRejectedValue(new Error("no"));
    const distribution = await freshStore();
    await expect(distribution.load()).resolves.toBeUndefined();
    expect(distribution.isStore).toBe(false);
  });
});
