// @vitest-environment jsdom
// Local AI against the mock backend: what the plugin adds and removes, the page opening (reads, never downloads), the
// "What each job uses" card (installed compatible models only, none -> disabled, assign, budget warning), the words.
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../api", async () => {
  const { createMockApi } = await import("../../api/mock");
  return { api: createMockApi() };
});

const { api } = await import("../../api");
const { ContributionRegistry, PluginHost, pluginLoaders } =
  await import("../../core/plugins/registry.svelte");
const { ui } = await import("../../core/shell/ui.svelte");
const { language, sizeText } = await import("../../core/i18n");
const { MOCK_MACHINE } = await import("../../api/mockLocalAi");
const { localAi, LOCAL_AI_PAGE } = await import("./store.svelte");
const { languagesText, errorText } = await import("./words");
const { default: LocalAiSection } = await import("./LocalAiSection.svelte");

const mock = api as unknown as ReturnType<typeof import("../../api/mock").createMockApi>;
const probe = mock.probe.localAi;
const GIB = 1024 ** 3;

let target: HTMLElement;
let component: Record<string, unknown> | null = null;

async function until(check: () => boolean, what: string): Promise<void> {
  await vi.waitFor(() => expect(check(), what).toBe(true), { timeout: 10_000, interval: 10 });
}

function select(task: string): HTMLSelectElement {
  const found = target.querySelector<HTMLSelectElement>(`select[data-task="${task}"]`);
  if (!found) throw new Error(`no select for ${task}: ${target.textContent ?? ""}`);
  return found;
}

const options = (task: string): string[] =>
  Array.from(select(task).options).map((o) => o.textContent?.trim() ?? "");

async function openPage(): Promise<void> {
  component = mount(LocalAiSection, { target });
  flushSync();
  await until(() => target.querySelector("select") !== null, "the jobs card shows");
}

beforeEach(() => {
  document.body.replaceChildren();
  target = document.createElement("div");
  document.body.append(target);
  probe.machine = structuredClone(MOCK_MACHINE);
  probe.calls.length = 0;
  ui.closeSettingsPage();
});

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
  localAi.stop();
  probe.stopAll();
  vi.restoreAllMocks();
});

describe("what the plugin adds", () => {
  it("adds its page and command when on, leaves nothing when off", async () => {
    const registry = new ContributionRegistry();
    const host = new PluginHost(registry, pluginLoaders());
    const call = vi.spyOn(api, "pluginCall");
    await host.enable("local-ai");
    expect(host.state("local-ai")).toBe("on");
    const owned = registry.ownedBy("local-ai");
    expect(owned.map((c) => c.kind).sort()).toEqual(["command", "settingsPage"]);
    const page = owned.find((c) => c.kind === "settingsPage");
    expect(page).toMatchObject({ id: LOCAL_AI_PAGE, label: "localAi.section", order: 35 });
    // on but dormant: switching on reads nothing
    expect(call).not.toHaveBeenCalled();
    const command = owned.find((c) => c.kind === "command");
    if (command?.kind !== "command") throw new Error("no command");
    expect(command.label).toBe("localAi.command");
    void command.run();
    expect(ui.settingsPage).toEqual({ status: "open", section: LOCAL_AI_PAGE });
    expect(host.disable("local-ai"), "contributions left after deactivate").toBe(0);
    expect(registry.items).toHaveLength(0);
    expect(localAi.state).toBeNull();
    expect(localAi.load.status).toBe("idle");
  });
});

describe("opening the page", () => {
  it("reads the state (and this computer for the jobs card), never starts a download", async () => {
    const call = vi.spyOn(api, "pluginCall");
    await openPage();
    const methods = call.mock.calls.map(([method]) => method);
    expect(methods).toContain("local-ai.state");
    expect(methods).toContain("local-ai.machine");
    expect(methods.filter((m) => m === "local-ai.machine")).toHaveLength(1);
    expect(methods).not.toContain("local-ai.get");
    expect(methods).not.toContain("local-ai.update");
    expect(probe.calls).toEqual([]);
    expect(target.textContent).toContain("What each job uses");
  });

  it("says when the state cannot be read, and tries again", async () => {
    const real = api.pluginCall.bind(api);
    const call = vi.spyOn(api, "pluginCall");
    call.mockResolvedValueOnce({
      ok: false,
      error: { kind: "command", command: "plugin_call", message: "gone" },
    });
    component = mount(LocalAiSection, { target });
    flushSync();
    await until(() => target.querySelector('[role="alert"]') !== null, "the failure shows");
    call.mockImplementation(real);
    const retry = target.querySelector<HTMLButtonElement>('[role="alert"] button');
    retry?.click();
    await until(() => target.querySelector("select") !== null, "the page shows after a retry");
  });
});

describe("what each job uses", () => {
  it("offers only installed models that can do the job, and Not assigned", async () => {
    await openPage();
    expect(options("correction")).toEqual(["Not assigned", "Gemma 4 E2B"]);
    const label = target.querySelector(`label[for="${select("correction").id}"]`);
    expect(label?.textContent).toBe("Correction");
  });

  it("disables the job when no model runs it here yet, and says Coming soon", async () => {
    await openPage();
    const readAloud = select("readAloud");
    expect(readAloud.disabled).toBe(true);
    expect(options("readAloud")).toEqual(["Not assigned"]);
    expect(target.textContent).toContain(
      "Coming soon. No model for this job works with this version of PerMark yet.",
    );
  });

  it("assigns through the backend and shows the new choice", async () => {
    await openPage();
    const translation = select("translation");
    translation.value = "gemma-4-e2b-it-q4_0";
    translation.dispatchEvent(new Event("change", { bubbles: true }));
    await until(
      () => localAi.assigned("translation") === "gemma-4-e2b-it-q4_0",
      "the assignment lands",
    );
    expect(probe.calls).toContain("assign translation gemma-4-e2b-it-q4_0");
    flushSync();
    expect(select("translation").value).toBe("gemma-4-e2b-it-q4_0");
    translation.value = "";
    translation.dispatchEvent(new Event("change", { bubbles: true }));
    await until(() => localAi.assigned("translation") === null, "unassigned");
    expect(probe.calls).toContain("assign translation -");
  });

  it("warns, in words and with an icon, when the assigned models need more memory than this computer has", async () => {
    await openPage();
    expect(target.querySelector('.budget[role="status"]')).toBeNull();
    if (component) await unmount(component);
    component = null;
    localAi.stop();
    probe.machine = { ...structuredClone(MOCK_MACHINE), ramTotal: 1 * GIB, gpus: [] };
    target.replaceChildren();
    await openPage();
    await until(() => target.querySelector('.budget[role="status"]') !== null, "the warning shows");
    const warning = target.querySelector('.budget[role="status"]');
    expect(warning?.textContent).toMatch(/memory/);
    expect(warning?.querySelector("svg")).not.toBeNull();
  });

  it("puts the keyboard on the first job when opened from a command", async () => {
    ui.openSettingsPage(LOCAL_AI_PAGE, "content");
    await openPage();
    await tick();
    await until(() => document.activeElement === select("correction"), "focus on the first job");
  });

  // last: it unassigns the mock's fixture
  it("keeps a choice the AI engine cannot run, says so, and lets the person unassign it", async () => {
    await openPage();
    const semantic = select("semanticIndex");
    expect(semantic.value).toBe("embeddinggemma-2-q8_0");
    expect(semantic.disabled).toBe(false);
    expect(options("semanticIndex")).toEqual(["Not assigned", "EmbeddingGemma 2 (can't run yet)"]);
    expect(target.textContent).toContain(
      "EmbeddingGemma 2 can't run with this version of PerMark yet.",
    );
    expect(probe.calls.some((c) => c.startsWith("assign"))).toBe(false);
    semantic.value = "";
    semantic.dispatchEvent(new Event("change", { bubbles: true }));
    await until(() => localAi.assigned("semanticIndex") === null, "unassigned");
    flushSync();
    expect(select("semanticIndex").disabled).toBe(true);
    expect(options("semanticIndex")).toEqual(["Not assigned"]);
  });
});

describe("plain words", () => {
  it("names languages in the current language, many as such, at most four", () => {
    expect(language.locale).toBe("en");
    expect(languagesText(["mul"])).toBe("many languages");
    expect(languagesText(["de", "ja"])).toBe("German and Japanese");
    expect(languagesText(["en", "ar", "zh", "fr", "de", "it"])).toBe(
      "English, Arabic, Chinese, French, and 2 more",
    );
  });

  it("sizes in megabytes or gigabytes, refusals in words", () => {
    expect(sizeText(309_855_456)).toBe("310 MB");
    expect(sizeText(3_398_849_248)).toBe("3.4 GB");
    expect(
      errorText({
        kind: "backend",
        command: "plugin_call",
        error: { kind: "unsupported", subject: "x", detail: "incompatible" },
      }),
    ).toBe("This model can't do this job.");
    expect(
      errorText({
        kind: "backend",
        command: "plugin_call",
        error: { kind: "unsupported", subject: "x", detail: "newerEngine" },
      }),
    ).toBe("Needs a newer version of the AI engine.");
  });
});
