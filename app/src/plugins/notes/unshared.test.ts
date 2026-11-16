// @vitest-environment jsdom
import { describe, expect, it, vi } from "vitest";
import { createMockApi } from "../../api/mock";
import { PluginClient } from "../../core/plugins/client";
import { WindowStore } from "../../core/window.svelte";
import { tellUnshared } from "./unshared";

function setup() {
  const api = createMockApi();
  const window = new WindowStore(api);
  const client = new PluginClient(api);
  return { api, window, client };
}

const keyed = (window: WindowStore) =>
  window.notices.filter((n) => n.kind === "action" && n.text.startsWith("Notes are now shared"));

describe("the notice about sharing turned off by the update", () => {
  it("names the notes once, by title, with Got it and Open agent panel in a project window", async () => {
    const { api, window, client } = setup();
    api.probe.plugins.notes.unshared(["2026-10-16 1031.md", "gone.md"]);
    const open = vi.fn();
    await tellUnshared(client, window, () => open);
    const [notice] = keyed(window);
    expect(notice?.kind === "action" && notice.text).toBe(
      "Notes are now shared with agents one project at a time, so earlier sharing was turned off: Reading list, gone. Share them again from a project's agent panel.",
    );
    const labels = notice?.kind === "action" ? notice.actions.map((a) => a.label) : [];
    expect(labels).toEqual(["Got it", "Open agent panel"]);
    // told once: it waits for the person instead of closing by itself
    expect(notice?.kind === "action" && notice.lasting).toBe(true);
    if (notice?.kind === "action") await notice.actions[1]?.run();
    expect(open).toHaveBeenCalledOnce();
    // the next start (another window, the app again) says nothing
    await tellUnshared(client, window, () => open);
    expect(keyed(window)).toHaveLength(1);
  });

  it("shortens a long list and offers only Got it outside a project", async () => {
    const { api, window, client } = setup();
    api.probe.plugins.notes.unshared(["a.md", "b.md", "c.md", "d.md", "e.md"]);
    await tellUnshared(client, window, () => null);
    const [notice] = keyed(window);
    // no agent panel to point to: the text does not send the person there
    expect(notice?.kind === "action" && notice.text).toBe(
      "Notes are now shared with agents one project at a time, so earlier sharing was turned off: a, b, c and 2 more.",
    );
    expect(notice?.kind === "action" && notice.lasting).toBe(true);
    expect(notice?.kind === "action" && notice.actions.map((a) => a.label)).toEqual(["Got it"]);
  });

  it("says nothing when nothing was shared", async () => {
    const { window, client } = setup();
    await tellUnshared(client, window, () => null);
    expect(keyed(window)).toHaveLength(0);
  });
});
