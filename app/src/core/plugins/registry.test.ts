import type { Component } from "svelte";
import { describe, expect, it } from "vitest";
import { neutralContext } from "../conditions/context.svelte";
import {
  ContributionRegistry,
  definePlugin,
  PluginHost,
  pluginLoaders,
  type ContributionInput,
  type PluginModule,
} from "./registry.svelte";

const Stub: Component = () => ({});
const view = () => Promise.resolve({ default: Stub });

/** A fake plugin contributing one of each kind + a window listener, all removed on deactivate. */
function fakePlugin(id: string, opts: { leaky?: boolean; throwing?: boolean } = {}): PluginModule {
  return definePlugin({
    manifest: { id, kind: "feature", dependsOn: [] },
    activate(cx) {
      if (opts.throwing) throw new Error("boom");
      const none = { needs: [], prefers: [] };
      const inputs: ContributionInput[] = [
        { kind: "command", id: `${id}.run`, label: "app.name", run: () => {}, requires: none },
        {
          kind: "paletteEntry",
          id: `${id}.p`,
          label: "app.name",
          command: `${id}.run`,
          requires: none,
        },
        { kind: "shortcut", keys: "Ctrl+K", command: `${id}.run`, requires: none },
        { kind: "panel", id: `${id}.panel`, side: "left", label: "app.name", view, requires: none },
        { kind: "statusItem", id: `${id}.s`, slot: "right", order: 1, view, requires: none },
        { kind: "treeBadge", id: `${id}.b`, badge: () => null, requires: none },
        { kind: "docDecoration", id: `${id}.d`, decorate: () => () => {}, requires: none },
        { kind: "settingsSection", id: `${id}.set`, label: "app.name", view, requires: none },
        { kind: "viewer", id: `${id}.v`, claims: () => false, view, requires: none },
        { kind: "editor", id: `${id}.e`, claims: () => false, view, requires: none },
        {
          kind: "embed",
          id: `${id}.m`,
          language: "x",
          view,
          requires: { needs: [{ need: "gitRepo" }], prefers: [] },
        },
      ];
      const removals = inputs.map((c) => cx.contribute(c));
      return () => {
        const keep = opts.leaky ? 1 : 0;
        removals.slice(keep).forEach((remove) => remove());
      };
    },
  });
}

function host(modules: Record<string, PluginModule>, delay = 0): PluginHost {
  const loaders = Object.fromEntries(
    Object.entries(modules).map(([id, module]) => [
      id,
      () =>
        new Promise<{ default: PluginModule }>((resolve) =>
          setTimeout(() => resolve({ default: module }), delay),
        ),
    ]),
  );
  return new PluginHost(new ContributionRegistry(), loaders);
}

describe("plugin host", () => {
  it("enable -> disable leaves the registry empty of the plugin (teardown symmetry)", async () => {
    const h = host({ alpha: fakePlugin("alpha"), beta: fakePlugin("beta") });
    await h.enable("alpha");
    await h.enable("beta");
    expect(h.registry.ownedBy("alpha")).toHaveLength(11);
    expect(h.state("alpha")).toBe("on");
    expect(h.disable("alpha")).toBe(0);
    expect(h.registry.ownedBy("alpha")).toHaveLength(0);
    expect(h.registry.ownedBy("beta")).toHaveLength(11);
  });

  it("a leaky teardown is reported and cleaned anyway", async () => {
    const h = host({ leaky: fakePlugin("leaky", { leaky: true }) });
    await h.enable("leaky");
    expect(h.disable("leaky")).toBe(1);
    expect(h.registry.ownedBy("leaky")).toHaveLength(0);
  });

  it("a throwing activate leaves nothing behind", async () => {
    const h = host({ bad: fakePlugin("bad", { throwing: true }) });
    await h.enable("bad");
    expect(h.state("bad")).toBe("failed");
    expect(h.registry.items).toHaveLength(0);
  });

  it("toggle storm: only the last wish counts, stale loads never activate", async () => {
    const h = host({ storm: fakePlugin("storm") }, 5);
    const pending: Promise<void>[] = [];
    for (let i = 0; i < 50; i++) {
      pending.push(h.enable("storm"));
      h.disable("storm");
    }
    await Promise.all(pending);
    expect(h.state("storm")).toBe("off");
    expect(h.registry.items).toHaveLength(0);
    pending.length = 0;
    for (let i = 0; i < 20; i++) {
      h.disable("storm");
      pending.push(h.enable("storm"));
    }
    await Promise.all(pending);
    expect(h.state("storm")).toBe("on");
    expect(h.registry.ownedBy("storm")).toHaveLength(11);
  });

  it("unknown plugin ids fail quietly", async () => {
    const h = host({});
    await h.enable("ghost");
    expect(h.state("ghost")).toBe("failed");
  });

  it("visible() hides contributions whose needs are unmet", async () => {
    const h = host({ alpha: fakePlugin("alpha") });
    await h.enable("alpha");
    const context = neutralContext();
    expect(h.registry.visible("embed", context)).toHaveLength(0);
    expect(
      h.registry.visible("embed", { ...context, git: { status: "repo", state: "clean" } }),
    ).toHaveLength(1);
    expect(h.registry.of("command")).toHaveLength(1);
  });

  it("the lazy module map is keyed by folder name: the plugins that have a UI", () => {
    expect(Object.keys(pluginLoaders()).sort()).toEqual([
      "agent",
      "broken-links",
      "comments",
      "fmt-dotenv",
      "fmt-ignore",
      "fmt-ini",
      "fmt-properties",
      "fmt-toml",
      "fmt-yaml",
      "follow",
      "frontmatter-card",
      "git",
      "highlight",
      "json-view",
      "live-feed",
      "math",
      "media",
      "mermaid",
      "notes",
      "plantuml",
      "quiet",
      "review",
      "tags",
      "text-view",
      "unread",
      "word-count",
      "xml-view",
    ]);
  });
});
