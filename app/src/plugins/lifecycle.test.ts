// @vitest-environment jsdom
import "../core/testing/media";
import { afterEach, describe, expect, it } from "vitest";
import type { PluginId } from "../api/generated/PluginId";
import { ContributionRegistry, PluginHost, pluginLoaders } from "../core/plugins/registry.svelte";
import { searchTags } from "../core/search/tags.svelte";
import { ui } from "../core/shell/ui.svelte";
import { silence } from "../core/silence/silence.svelte";

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
    expect(await kinds("live-feed")).toEqual(["changeStep", "command", "panel", "statusItem"]);
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
      "bodyOverride",
      "changeStep",
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

const ORGANIZING: PluginId[] = ["tags", "comments", "quiet", "broken-links"];

describe("the organizing plugins", () => {
  it("each one adds its parts when switched on and leaves nothing when switched off", async () => {
    const loaders = pluginLoaders();
    for (const id of ORGANIZING) {
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
    expect(await kinds("tags")).toEqual([
      "command",
      "docBar",
      "panel",
      "searchFilter",
      "shortcut",
      "statusItem",
      "treeBadge",
      "treeMenu",
    ]);
    expect(await kinds("comments")).toEqual([
      "command",
      "docDecoration",
      "panel",
      "selectionMenu",
      "shortcut",
      "statusItem",
      "treeBadge",
    ]);
    expect(await kinds("quiet")).toEqual(["command", "treeBadge", "treeMenu"]);
    expect(await kinds("broken-links")).toEqual(["command", "panel"]);
  });

  it("switching off puts back the tab that was open and the search the tags narrowed", async () => {
    const registry = new ContributionRegistry();
    const host = new PluginHost(registry, pluginLoaders());
    for (const id of ORGANIZING) await host.enable(id);
    for (const panel of ["tags.panel", "comments.panel", "broken-links.panel"]) {
      ui.showSidebar(panel);
    }
    searchTags.add("alpha");
    expect(searchTags.available).toBe(true);
    for (const id of ORGANIZING) host.disable(id);
    expect(ui.sidebar).toBe("files");
    expect(searchTags.available).toBe(false);
    expect(searchTags.selected).toEqual([]);
    expect(silence.isSilenced("anything")).toBe(false);
    expect(registry.items).toHaveLength(0);
  });

  it("every command and key they add has a free id and a label in the interface", async () => {
    const registry = new ContributionRegistry();
    const host = new PluginHost(registry, pluginLoaders());
    for (const id of [...FOLLOWING, ...ORGANIZING]) await host.enable(id);
    const ids = registry.of("command").map((c) => c.id);
    expect(new Set(ids).size).toBe(ids.length);
    const known = new Set(ids);
    for (const shortcut of registry.of("shortcut")) {
      expect(known.has(shortcut.command), shortcut.command).toBe(true);
    }
    const keys = registry.of("shortcut").map((s) => s.keys);
    expect(new Set(keys).size, "no two commands share a default key").toBe(keys.length);
  });
});

describe("the notes plugin", () => {
  it("adds the Notes page, the welcome block, its settings and commands, and leaves nothing when off", async () => {
    const registry = new ContributionRegistry();
    const host = new PluginHost(registry, pluginLoaders());
    await host.enable("notes");
    expect(host.state("notes")).toBe("on");
    const found = [...new Set(registry.ownedBy("notes").map((c) => c.kind))].sort();
    expect(found).toEqual([
      "command",
      "settingsSection",
      "shortcut",
      "statusItem",
      "topBarItem",
      "welcomeSection",
      "workspaceView",
    ]);
    expect(registry.of("workspaceView").map((v) => v.workspace)).toEqual(["notes"]);
    expect(host.disable("notes"), "contributions left after deactivate").toBe(0);
    expect(registry.items).toHaveLength(0);
  });

  it("every key it adds names a command it added, and the ids are free", async () => {
    const registry = new ContributionRegistry();
    const host = new PluginHost(registry, pluginLoaders());
    await host.enable("notes");
    const ids = registry.of("command").map((c) => c.id);
    expect(new Set(ids).size).toBe(ids.length);
    const keys = registry.of("shortcut");
    for (const shortcut of keys) expect(ids, shortcut.command).toContain(shortcut.command);
    // Ctrl+1..9 open the pinned notes
    expect(keys.filter((k) => /^Mod\+[1-9]$/.test(k.keys))).toHaveLength(9);
    host.disable("notes");
  });
});

describe("the config format plugins", () => {
  const FORMATS: [PluginId, string, string[], string[]][] = [
    ["fmt-toml", "toml", ["Cargo.toml", "pyproject.toml"], ["a.json", "toml"]],
    ["fmt-yaml", "yaml", ["ci.yml", "docker-compose.yaml"], ["a.yaml.bak", "yml"]],
    [
      "fmt-ini",
      "ini",
      [".editorconfig", "setup.cfg", "php.ini", ".gitconfig", ".npmrc"],
      ["editorconfig", "a.toml"],
    ],
    [
      "fmt-dotenv",
      "dotenv",
      [".env", ".env.example", ".env.local", "prod.env"],
      [".envrc", "env.txt"],
    ],
    ["fmt-properties", "properties", ["application.properties"], ["a.prop"]],
    [
      "fmt-ignore",
      "ignore",
      [".gitignore", ".aiignore", ".cursorignore", ".dockerignore"],
      ["gitignore", "notes.txt"],
    ],
  ];

  it("each one adds a viewer for its format and leaves nothing when off", async () => {
    const loaders = pluginLoaders();
    for (const [id, format, yes, no] of FORMATS) {
      const registry = new ContributionRegistry();
      const host = new PluginHost(registry, loaders);
      await host.enable(id);
      expect(host.state(id), id).toBe("on");
      const viewers = registry.of("viewer").filter((v) => v.owner === id);
      expect(
        viewers.map((v) => v.id),
        id,
      ).toEqual([format]);
      const claims = viewers[0]?.claims;
      for (const name of yes) expect(claims?.(name), `${id} claims ${name}`).toBe(true);
      for (const name of no) expect(claims?.(name), `${id} leaves ${name}`).toBe(false);
      expect(host.disable(id), `${id}: contributions left after deactivate`).toBe(0);
      expect(registry.items, id).toHaveLength(0);
    }
  });
});
