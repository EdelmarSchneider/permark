// @vitest-environment jsdom
// Plugin `translation` switched on | off: the review follows the model of job "translation" and the remembered
// languages, and nothing is left after; dormant while no model is assigned to the job.
import "../../core/testing/media";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});

const { api } = await import("../../api");
const { ContributionRegistry, PluginHost, pluginLoaders } =
  await import("../../core/plugins/registry.svelte");
const { translation } = await import("./store.svelte");
const { languageDialog } = await import("./dialog.svelte");
const { t } = await import("../../core/i18n");
const mock = api as unknown as ReturnType<typeof import("../../api/mock").createMockApi>;

afterEach(() => {
  mock.probe.translation.available = null;
  mock.probe.translation.prefs = { target: null, recent: [] };
});

async function on() {
  const registry = new ContributionRegistry();
  const host = new PluginHost(registry, pluginLoaders());
  await host.enable("translation");
  return { registry, host };
}

describe("plugin translation", () => {
  it("switched on it follows the job's model and the remembered languages; switched off nothing is left", async () => {
    mock.probe.translation.prefs = { target: "pt-BR", recent: ["pt-BR", "de"] };
    const { registry, host } = await on();
    expect(host.state("translation")).toBe("on");
    await vi.waitFor(() => expect(translation.available).toBe(true));
    await vi.waitFor(() => expect(translation.languages.target).toBe("pt-BR"));
    expect(translation.languages.recent).toEqual(["pt-BR", "de"]);
    expect(host.disable("translation"), "contributions left after deactivate").toBe(0);
    expect(registry.items).toHaveLength(0);
    expect(translation.available).toBe(false);
    expect(translation.languages).toMatchObject({ target: null, source: null, recent: [] });
  });

  it("is dormant until a model is assigned to the job", async () => {
    mock.probe.translation.available = false;
    const { host } = await on();
    await vi.waitFor(() => expect(mock.probe.translation.calls).toContain("status"));
    expect(translation.available).toBe(false);
    host.disable("translation");
  });

  it("adds the command that opens the language dialog and the dialog's place; off, both go and the dialog closes", async () => {
    const { registry, host } = await on();
    await vi.waitFor(() => expect(translation.available).toBe(true));
    expect(registry.ownedBy("translation").length).toBeGreaterThan(0);
    expect(registry.of("statusItem").map((c) => c.id)).toContain("translation.host");
    const command = registry.of("command").find((c) => c.id === "translation.language");
    expect(command?.when?.()).toBe(true);
    expect(t(command?.label ?? "app.name")).toBe("Translate to…");
    expect(languageDialog.state.status).toBe("closed");
    void command?.run();
    // the default: the choice is saved, nothing else follows
    expect(languageDialog.state).toMatchObject({
      status: "open",
      confirm: "translation.language.save",
    });
    expect(host.disable("translation"), "contributions left after deactivate").toBe(0);
    expect(registry.items).toHaveLength(0);
    expect(languageDialog.state.status).toBe("closed");
  });

  it("the command is not offered until a model is assigned to the job", async () => {
    mock.probe.translation.available = false;
    const { registry, host } = await on();
    await vi.waitFor(() => expect(mock.probe.translation.calls).toContain("status"));
    const commands = registry.ownedBy("translation").filter((c) => c.kind === "command");
    expect(commands.map((c) => c.id)).toContain("translation.language");
    for (const command of commands) expect(command.when?.()).toBe(false);
    host.disable("translation");
  });

  it("the dialog's place draws the dialog over the window while something asks for it", async () => {
    const { registry, host } = await on();
    const place = registry.of("statusItem").find((c) => c.id === "translation.host");
    const { default: Place } = (await place?.view()) ?? { default: null };
    if (Place === null) throw new Error("no place");
    const target = document.createElement("div");
    document.body.append(target);
    const app = mount(Place, { target });
    flushSync();
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    languageDialog.ask();
    flushSync();
    expect(document.querySelector('[role="dialog"]')).not.toBeNull();
    // the plugin goes: so does its dialog
    host.disable("translation");
    flushSync();
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    await unmount(app);
    target.remove();
  });

  it("asks again when the Local AI assignments change", async () => {
    mock.probe.translation.available = false;
    const { host } = await on();
    await vi.waitFor(() => expect(mock.probe.translation.calls).toContain("status"));
    mock.probe.translation.available = null;
    await mock.pluginCall("local-ai.assign", { task: "translation", model: "gemma-4-e2b-it-q4_0" });
    await vi.waitFor(() => expect(translation.available).toBe(true));
    host.disable("translation");
  });
});
