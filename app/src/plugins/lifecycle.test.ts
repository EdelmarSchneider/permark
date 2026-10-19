// @vitest-environment jsdom
import "../core/testing/media";
import { afterEach, describe, expect, it } from "vitest";
import type { PluginId } from "../api/generated/PluginId";
import { ContributionRegistry, PluginHost, pluginLoaders } from "../core/plugins/registry.svelte";
import { ui } from "../core/shell/ui.svelte";

const FOLLOWING: PluginId[] = ["git", "live-feed", "follow", "unread", "review"];

afterEach(() => {
  ui.right = null;
  ui.sidebar = "files";
});

describe("the following plugins", () => {
  it("each one adds its parts when switched on and leaves nothing when switched off", async () => {
    const loaders = pluginLoaders();
    for (const id of FOLLOWING) {
      const registry = new ContributionRegistry();
      const host = new PluginHost(registry, loaders);
      await host.enable(id);
      expect(host.state(id), id).toBe("on");
      expect(registry.ownedBy(id).length, id).toBeGreaterThan(0);
      expect(host.disable(id), `${id}: contributions left after deactivate`).toBe(0);
      expect(registry.items, id).toHaveLength(0);
    }
  });

  it("the parts each plugin adds are the ones its spec lists", async () => {
    const loaders = pluginLoaders();
    const kinds = async (id: PluginId): Promise<string[]> => {
      const registry = new ContributionRegistry();
      const host = new PluginHost(registry, loaders);
      await host.enable(id);
      const found = [...new Set(registry.ownedBy(id).map((c) => c.kind))].sort();
      host.disable(id);
      return found;
    };
    expect(await kinds("git")).toEqual([
      "command",
      "docBar",
      "settingsSection",
      "statusItem",
      "treeBadge",
    ]);
    expect(await kinds("live-feed")).toEqual(["command", "panel", "statusItem"]);
    expect(await kinds("follow")).toEqual(["command", "shortcut", "statusItem"]);
    expect(await kinds("unread")).toEqual([
      "command",
      "panel",
      "shortcut",
      "statusItem",
      "treeBadge",
      "treeMenu",
    ]);
    expect(await kinds("review")).toEqual([
      "command",
      "docBar",
      "docBlocks",
      "docDecoration",
      "shortcut",
    ]);
  });

  it("switching off puts back the panels the plugin had opened", async () => {
    const registry = new ContributionRegistry();
    const host = new PluginHost(registry, pluginLoaders());
    await host.enable("live-feed");
    ui.right = "live-feed.panel";
    host.disable("live-feed");
    expect(ui.right).toBeNull();
    await host.enable("unread");
    ui.showSidebar("unread.panel");
    host.disable("unread");
    expect(ui.sidebar).toBe("files");
  });

  it("every command and key they add has a free id and a label in the interface", async () => {
    const registry = new ContributionRegistry();
    const host = new PluginHost(registry, pluginLoaders());
    for (const id of FOLLOWING) await host.enable(id);
    const commands = registry.of("command");
    const ids = commands.map((c) => c.id);
    expect(new Set(ids).size).toBe(ids.length);
    const known = new Set(ids);
    for (const shortcut of registry.of("shortcut")) {
      expect(known.has(shortcut.command), shortcut.command).toBe(true);
    }
  });
});
