import { describe, expect, it } from "vitest";
import type { PluginInfo } from "../../../api";
import { MOCK_MANIFESTS } from "../../../api/mockShell";
import { neutralContext } from "../../conditions/context.svelte";
import en from "../../i18n/locales/en";
import { pluginLoaders } from "../../plugins/registry.svelte";
import {
  aboutKey,
  canRemoveData,
  choiceOf,
  choiceValue,
  grouped,
  KNOWN_PLUGINS,
  nameKey,
  statusOf,
} from "./pluginsModel";

function info(patch: Partial<PluginInfo> & { id?: string }): PluginInfo {
  const manifest = MOCK_MANIFESTS.find((m) => m.id === (patch.id ?? "git"));
  if (manifest === undefined) throw new Error("no such manifest");
  const { id: _id, ...rest } = patch;
  return {
    manifest,
    appOn: true,
    state: { state: "on" },
    projectChoice: null,
    projectConfigOff: false,
    ...rest,
  };
}

describe("the plugin list", () => {
  it("every plugin of the build has a name and a line in the language files", () => {
    for (const manifest of MOCK_MANIFESTS) {
      const name = nameKey(manifest.id);
      const about = aboutKey(manifest.id);
      expect(name, manifest.id).not.toBeNull();
      expect(about, manifest.id).not.toBeNull();
      if (name !== null) expect(en[name], manifest.id).not.toBe("");
      if (about !== null) expect(en[about], manifest.id).not.toBe("");
    }
    expect(new Set(MOCK_MANIFESTS.map((m) => m.id))).toEqual(new Set(KNOWN_PLUGINS));
  });

  it("a plugin with a page of its own is a plugin of the list", () => {
    const withModule = Object.keys(pluginLoaders());
    for (const id of withModule)
      expect(
        KNOWN_PLUGINS.some((k) => k === id),
        id,
      ).toBe(true);
  });

  it("an unknown plugin id has no key (the page shows the id)", () => {
    expect(nameKey("from-the-future")).toBeNull();
    expect(aboutKey("")).toBeNull();
  });

  it("groups by kind in the page's order, in manifest order, and skips empty groups", () => {
    const groups = grouped(MOCK_MANIFESTS.map((manifest) => info({ id: manifest.id })));
    expect(groups.map((g) => g.kind)).toEqual([
      "feature",
      "format",
      "role",
      "analysis",
      "checker",
      "embed",
    ]);
    expect(groups[0]?.plugins.map((p) => p.manifest.id).slice(0, 3)).toEqual([
      "git",
      "live-feed",
      "follow",
    ]);
    expect(grouped([info({ id: "math" })]).map((g) => g.kind)).toEqual(["embed"]);
    expect(grouped([])).toEqual([]);
  });
});

describe("what a row says", () => {
  it("nothing when the plugin works here", () => {
    const context = { ...neutralContext(), workspace: "project" as const };
    expect(statusOf(info({}), context)).toEqual({ kind: "none" });
  });

  it("a plugin that is on but needs a project says so, with the same words as the palette", () => {
    const status = statusOf(info({}), neutralContext());
    expect(status.kind).toBe("needs");
    if (status.kind === "needs") {
      expect(status.reason).toMatchObject({ need: "workspace", kinds: ["project"] });
    }
    // a plugin of the app needs nothing
    expect(statusOf(info({ id: "math" }), neutralContext())).toEqual({ kind: "none" });
  });

  it("off because of a dependency, off in this project, off by the setting", () => {
    expect(
      statusOf(
        info({
          id: "review",
          state: { state: "off", why: { reason: "dependency", plugin: "unread" } },
        }),
        neutralContext(),
      ),
    ).toEqual({ kind: "dependency", plugin: "unread" });
    expect(
      statusOf(info({ state: { state: "off", why: { reason: "project" } } }), neutralContext()),
    ).toEqual({ kind: "project" });
    expect(
      statusOf(
        info({ appOn: false, state: { state: "off", why: { reason: "setting" } } }),
        neutralContext(),
      ),
    ).toEqual({ kind: "none" });
  });

  it("data can be removed only while the app has the plugin off", () => {
    expect(canRemoveData(info({}))).toBe(false);
    expect(
      canRemoveData(info({ appOn: false, state: { state: "off", why: { reason: "setting" } } })),
    ).toBe(true);
    // off only in this project: the app still runs it elsewhere
    expect(canRemoveData(info({ state: { state: "off", why: { reason: "project" } } }))).toBe(
      false,
    );
    // the app turned it off but this project turns it on: it runs
    expect(canRemoveData(info({ appOn: false, projectChoice: true }))).toBe(false);
  });

  it("the per-project choice reads and writes both ways", () => {
    expect(choiceOf(info({ projectChoice: null }))).toBe("app");
    expect(choiceOf(info({ projectChoice: true }))).toBe("on");
    expect(choiceOf(info({ projectChoice: false }))).toBe("off");
    expect([choiceValue("app"), choiceValue("on"), choiceValue("off")]).toEqual([
      null,
      true,
      false,
    ]);
  });
});
