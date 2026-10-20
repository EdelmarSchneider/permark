// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { createMockApi } from "../../api/mock";
import { DefaultAppStore } from "./defaultApp.svelte";

function fresh(): { api: ReturnType<typeof createMockApi>; store: DefaultAppStore } {
  const api = createMockApi();
  return { api, store: new DefaultAppStore(api) };
}

describe("the first-run question", () => {
  it("is asked once, in the first window only, when the backend says it is due", async () => {
    const { api, store } = fresh();
    api.probe.distribution.setDefaultApp({ mode: "direct" }, true);
    await store.askIfDue("w-2");
    expect(store.question.status).toBe("closed");
    await store.askIfDue("main");
    expect(store.question).toMatchObject({ status: "asking", chosen: ["markdown"] });
    await store.answer(false);
    await store.askIfDue("main");
    expect(store.question.status).toBe("closed");
  });

  it("is not asked when it is not due (an installer asked, an older PerMark, a confined copy)", async () => {
    const { api, store } = fresh();
    api.probe.distribution.setDefaultApp({ mode: "unsupported", reason: "confined" }, false);
    await store.askIfDue("main");
    expect(store.question.status).toBe("closed");
  });

  it("Use PerMark sets the chosen groups where PerMark can", async () => {
    const { api, store } = fresh();
    api.probe.distribution.setDefaultApp({ mode: "direct" }, true);
    await store.askIfDue("main");
    store.toggleChosen("json", true);
    store.toggleChosen("json", false);
    store.toggleChosen("yaml", true);
    await store.answer(true);
    const ours = store.view?.groups.filter((g) => g.ours).map((g) => g.group);
    expect(ours).toEqual(["markdown", "yaml"]);
    expect(store.view?.ask).toBe(false);
  });

  it("on Windows the answer opens Windows' own page (never a takeover)", async () => {
    const { api, store } = fresh();
    api.probe.distribution.setDefaultApp({ mode: "handoff" }, true);
    await store.askIfDue("main");
    await store.answer(true);
    expect(api.probe.distribution.handoffs).toBe(1);
  });

  it("nothing chosen counts as Not now", async () => {
    const { api, store } = fresh();
    api.probe.distribution.setDefaultApp({ mode: "handoff" }, true);
    await store.askIfDue("main");
    store.toggleChosen("markdown", false);
    await store.answer(true);
    expect(api.probe.distribution.handoffs).toBe(0);
    await store.load();
    expect(store.view?.ask).toBe(false);
  });
});

describe("the settings rows", () => {
  it("a refused change says why", async () => {
    const { api, store } = fresh();
    api.probe.distribution.setDefaultApp({ mode: "handoff" }, false);
    await store.set("markdown", true);
    expect(store.problem).toMatch(/Couldn't change the default app/);
  });

  it("toggles in Direct mode are the person's, both ways", async () => {
    const { api, store } = fresh();
    api.probe.distribution.setDefaultApp({ mode: "direct" }, false);
    await store.set("toml", true);
    expect(store.view?.groups.find((g) => g.group === "toml")?.ours).toBe(true);
    await store.set("toml", false);
    await store.set("markdown", false);
    expect(store.view?.groups.some((g) => g.ours)).toBe(false);
    expect(store.problem).toBeNull();
  });
});
