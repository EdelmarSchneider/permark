// @vitest-environment jsdom
import { flushSync } from "svelte";
import { describe, expect, it } from "vitest";
import { observe } from "../testing/effect.svelte";
import { ContributionRegistry, type ContributionInput } from "./registry.svelte";

const none = { needs: [], prefers: [] };
const command = (id: string): ContributionInput => ({
  kind: "command",
  id,
  label: "app.name",
  run: () => {},
  requires: none,
});

describe("the registry and effects", () => {
  it("registering inside an effect never makes the effect depend on the registry", () => {
    // the window registers the core's commands in an effect; plugins register later, from anywhere
    const registry = new ContributionRegistry();
    const effect = observe(() => {
      registry.register({ ...command("core.a"), owner: "core" });
      registry.register({ ...command("core.b"), owner: "core" });
    });
    flushSync();
    expect(effect.runs()).toBe(1);
    registry.register({ ...command("later"), owner: "git" });
    registry.removeOwner("git");
    flushSync();
    expect(effect.runs()).toBe(1);
    expect(registry.ownedBy("core")).toHaveLength(2);
    effect.stop();
  });
});
