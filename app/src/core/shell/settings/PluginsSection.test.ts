// @vitest-environment jsdom
// The Plugins list of Settings holds app-wide choices only: one switch per plugin, no per-folder select even in a
// project window (a folder's own plugin choices live with that folder's settings).
import "../../testing/media";
import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api, type PluginInfo } from "../../../api";
import { MOCK_MANIFESTS } from "../../../api/mockShell";
import { conditions, neutralContext } from "../../conditions/context.svelte";
import { t } from "../../i18n";
import en from "../../i18n/locales/en";
import { windowStore } from "../../window.svelte";
import PluginsSection from "./PluginsSection.svelte";

let component: Record<string, unknown> | null = null;
let root: HTMLElement;

function info(id: string, appOn: boolean): PluginInfo {
  const manifest = MOCK_MANIFESTS.find((m) => m.id === id);
  if (manifest === undefined) throw new Error(`no mock manifest ${id}`);
  return {
    manifest,
    appOn,
    state: appOn ? { state: "on" } : { state: "off", why: { reason: "setting" } },
    projectChoice: null,
    projectConfigOff: false,
  };
}

const projectScoped = (): string => {
  const found = MOCK_MANIFESTS.find((m) => m.scope === "project");
  if (found === undefined) throw new Error("no project-scoped mock plugin");
  return found.id;
};

beforeEach(() => {
  root = document.body.appendChild(document.createElement("div"));
  conditions.set({ ...neutralContext(), workspace: "project" });
});

afterEach(() => {
  if (component) void unmount(component);
  component = null;
  document.body.replaceChildren();
  conditions.set(neutralContext());
  vi.restoreAllMocks();
});

async function show(infos: PluginInfo[]): Promise<void> {
  vi.spyOn(api, "pluginsList").mockResolvedValue({ ok: true, value: infos });
  vi.spyOn(api, "on").mockResolvedValue(() => {});
  component = mount(PluginsSection, { target: root, props: { store: windowStore } });
  flushSync();
  await Promise.resolve();
  await Promise.resolve();
  await tick();
  flushSync();
}

describe("the Settings plugins list", () => {
  it("shows no per-folder choice for a project plugin in a project window", async () => {
    const id = projectScoped();
    await show([info(id, true)]);
    expect(root.querySelector(`#plugin-${id}`)).not.toBeNull();
    expect(root.querySelector("select")).toBeNull();
    expect(root.textContent).not.toContain("In this project");
  });

  it("turns a plugin off for the whole app with its switch", async () => {
    const id = projectScoped();
    const change = vi.spyOn(windowStore, "changeSetting").mockResolvedValue();
    await show([info(id, true)]);
    const toggle = root.querySelector<HTMLInputElement>(`input[aria-labelledby="plugin-${id}"]`);
    expect(toggle?.checked).toBe(true);
    toggle?.click();
    expect(change).toHaveBeenCalledWith({
      key: "pluginEnabled",
      value: { plugin: id, enabled: false },
    });
  });

  it("asks before removing data and says which folders it cannot reach now", async () => {
    const id = projectScoped();
    vi.spyOn(api, "pluginDataBytes").mockResolvedValue({ ok: true, value: 0 });
    await show([info(id, false)]);
    const remove = Array.from(root.querySelectorAll("button")).find(
      (b) => b.textContent?.trim() === t("plugins.removeData"),
    );
    remove?.click();
    flushSync();
    await tick();
    const confirm = root.querySelector('.confirm[role="group"]');
    expect(confirm?.textContent).toContain(t("plugins.removeScope"));
    expect(en["plugins.removeScope"]).toBe(
      "Folders that keep their data in their own .permark folder are cleaned only while they are open.",
    );
  });

  it("the question takes the keyboard on its Cancel; Cancel gives it back to Remove data", async () => {
    const id = projectScoped();
    vi.spyOn(api, "pluginDataBytes").mockResolvedValue({ ok: true, value: 0 });
    await show([info(id, false)]);
    const removeButton = (): HTMLButtonElement | undefined =>
      Array.from(root.querySelectorAll("button")).find(
        (b) => b.textContent?.trim() === t("plugins.removeData"),
      );
    removeButton()?.focus();
    removeButton()?.click();
    flushSync();
    await tick();
    // the button that had the keyboard went with the question: never the page body
    const cancel = Array.from(root.querySelectorAll<HTMLButtonElement>(".confirm button")).find(
      (b) => b.textContent?.trim() === t("common.cancel"),
    );
    expect(cancel).toBeDefined();
    expect(document.activeElement).toBe(cancel);
    cancel?.click();
    flushSync();
    await tick();
    expect(root.querySelector(".confirm")).toBeNull();
    expect(document.activeElement).toBe(removeButton());
  });
});
