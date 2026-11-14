// @vitest-environment jsdom
import "../core/testing/media";
import { afterEach, describe, expect, it } from "vitest";
import type { PluginId } from "../api/generated/PluginId";
import { conditions, neutralContext } from "../core/conditions/context.svelte";
import { contributions } from "../core/plugins/instance.svelte";
import { ContributionRegistry, PluginHost, pluginLoaders } from "../core/plugins/registry.svelte";
import { searchTags } from "../core/search/tags.svelte";
import { defaultArrangement, FILES } from "../core/shell/arrangement";
import { arrangement } from "../core/shell/arrangement.svelte";
import { layout } from "../core/shell/layout.svelte";
import { closeView, showView } from "../core/shell/sides";
import { silence } from "../core/silence/silence.svelte";

const FOLLOWING: PluginId[] = ["git", "live-feed", "follow", "unread", "review"];

afterEach(() => {
  conditions.set(neutralContext());
  layout.setSelected([]);
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
      "projectFacts",
      "settingsSection",
      "statusItem",
      "treeBadge",
    ]);
    expect(await kinds("live-feed")).toEqual([
      "changeStep",
      "command",
      "outlineMark",
      "panel",
      "statusItem",
    ]);
    expect(await kinds("follow")).toEqual(["command", "shortcut", "statusItem"]);
    expect(await kinds("unread")).toEqual([
      "command",
      "panel",
      "projectCount",
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
      "docMenu",
      "outlineMark",
      "shortcut",
      "statusItem",
      "treeMenu",
    ]);
  });

  it("switching off puts back the panels the plugin had opened", async () => {
    const registry = new ContributionRegistry();
    const host = new PluginHost(registry, pluginLoaders());
    // the right column lists only the panels registered now: Live changes goes with its plugin
    await host.enable("live-feed");
    host.disable("live-feed");
    expect(registry.of("panel").filter((p) => p.side === "right")).toEqual([]);
    // the sidebar tab that showed goes with its plugin: the first view of the group shows again
    conditions.set({ ...neutralContext(), workspace: "project" });
    const window = new PluginHost(contributions, pluginLoaders());
    await window.enable("unread");
    showView("unread.panel");
    expect(arrangement.isSelected("unread.panel")).toBe(true);
    expect(window.disable("unread")).toBe(0);
    expect(arrangement.isSelected(FILES)).toBe(true);
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
      "projectCount",
      "selectionMenu",
      "shortcut",
      "statusItem",
      "treeBadge",
    ]);
    expect(await kinds("quiet")).toEqual(["command", "treeBadge", "treeMenu"]);
    expect(await kinds("broken-links")).toEqual(["command", "panel", "shortcut"]);
  });

  it("switching off puts back the tab that was open and the search the tags narrowed", async () => {
    // in the window's own registry: the views of the window are the panels it lists
    const registry = contributions;
    const host = new PluginHost(registry, pluginLoaders());
    conditions.set({ ...neutralContext(), workspace: "project" });
    for (const id of ORGANIZING) await host.enable(id);
    for (const panel of ["tags.panel", "comments.panel", "broken-links.panel"]) {
      showView(panel);
      expect(arrangement.isSelected(panel), panel).toBe(true);
    }
    searchTags.add("alpha");
    expect(searchTags.available).toBe(true);
    for (const id of ORGANIZING) host.disable(id);
    expect(arrangement.isSelected(FILES)).toBe(true);
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
    expect(registry.of("shortcut").find((s) => s.command === "brokenLinks.show")?.keys).toBe(
      "Mod+Shift+b",
    );
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
    // Mod+1..9 open the pinned notes
    expect(keys.filter((k) => /^Mod\+[1-9]$/.test(k.keys))).toHaveLength(9);
    host.disable("notes");
  });
});

describe("the symbols plugin", () => {
  // the panel's open | closed choice is the project's: no test leaves its choice to the next
  afterEach(() => layout.setOpen("symbols.panel", false));

  it("adds its panel, its command and the popup's button, dormant, and leaves nothing when off", async () => {
    const registry = new ContributionRegistry();
    const host = new PluginHost(registry, pluginLoaders());
    await host.enable("symbols");
    const found = [...new Set(registry.ownedBy("symbols").map((c) => c.kind))].sort();
    expect(found).toEqual(["command", "panel", "pickerAction"]);
    // a fresh profile shows no panel: it opens only when the person asks
    const panel = registry.of("panel").find((p) => p.id === "symbols.panel");
    expect(panel?.side).toBe("right");
    expect(panel?.defaultOpen).toBeUndefined();
    expect(registry.of("shortcut")).toEqual([]);
    expect(host.disable("symbols"), "contributions left after deactivate").toBe(0);
    expect(registry.items).toHaveLength(0);
  });

  it("the popup's button shows the panel; switching off takes it away", async () => {
    const window = new PluginHost(contributions, pluginLoaders());
    await window.enable("symbols");
    // not a view of the window until the person asks for it
    expect(arrangement.viewOf("symbols.panel")).toBeUndefined();
    const [keepOpen] = contributions.of("pickerAction");
    keepOpen?.run("symbols");
    expect(arrangement.isSelected("symbols.panel")).toBe(true);
    expect(window.disable("symbols")).toBe(0);
    expect(arrangement.viewOf("symbols.panel")).toBeUndefined();
  });

  it("the palette command shows the panel, then hides it", async () => {
    const window = new PluginHost(contributions, pluginLoaders());
    await window.enable("symbols");
    const toggle = contributions.of("command").find((c) => c.id === "symbols.toggle");
    toggle?.run();
    expect(arrangement.isSelected("symbols.panel")).toBe(true);
    toggle?.run();
    expect(arrangement.isPresent("symbols.panel")).toBe(false);
    expect(window.disable("symbols")).toBe(0);
  });

  it("a saved arrangement that lists the panel shows nothing until the person asks, then it opens there", async () => {
    arrangement.saved = {
      ...defaultArrangement(),
      bottom: [{ views: ["symbols.panel"], share: 1 }],
    };
    try {
      const window = new PluginHost(contributions, pluginLoaders());
      await window.enable("symbols");
      // the layout names it, yet a window where nobody asked for it shows no trace of it
      expect(arrangement.viewOf("symbols.panel")).toBeUndefined();
      expect(arrangement.isPresent("symbols.panel")).toBe(false);
      expect(arrangement.drawn("bottom")).toEqual([]);
      contributions
        .of("command")
        .find((c) => c.id === "symbols.toggle")
        ?.run();
      expect(arrangement.regionOf("symbols.panel")).toBe("bottom");
      expect(arrangement.isSelected("symbols.panel")).toBe(true);
      closeView("symbols.panel");
      expect(window.disable("symbols")).toBe(0);
    } finally {
      arrangement.saved = defaultArrangement();
    }
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
