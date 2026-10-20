import { describe, expect, it } from "vitest";
import { restartPlugins, syncPlugins } from "./instance.svelte";
import {
  ContributionRegistry,
  definePlugin,
  PluginHost,
  type PluginModule,
} from "./registry.svelte";

/** A plugin that counts how often it was switched on and off. */
function counting(id: string, log: string[]): PluginModule {
  return definePlugin({
    manifest: { id, kind: "feature", dependsOn: [] },
    activate() {
      log.push(`on:${id}`);
      return () => log.push(`off:${id}`);
    },
  });
}

async function settle(): Promise<void> {
  for (let i = 0; i < 5; i += 1) await Promise.resolve();
}

describe("restarting the plugin modules (the project state moved)", () => {
  it("every module that is on goes off and on again, each once, and the ones that were off stay off", async () => {
    const log: string[] = [];
    const host = new PluginHost(new ContributionRegistry(), {
      git: () => Promise.resolve({ default: counting("git", log) }),
      tags: () => Promise.resolve({ default: counting("tags", log) }),
      math: () => Promise.resolve({ default: counting("math", log) }),
    });
    syncPlugins(["git", "tags", "no-ui-module"], host);
    await settle();
    expect(log.sort()).toEqual(["on:git", "on:tags"]);
    log.length = 0;
    restartPlugins(host);
    await settle();
    expect(log.sort()).toEqual(["off:git", "off:tags", "on:git", "on:tags"]);
    expect(host.state("git")).toBe("on");
    expect(host.state("math")).not.toBe("on");
    // and again: a storm of restarts ends with everything on, once
    log.length = 0;
    for (let i = 0; i < 20; i += 1) restartPlugins(host);
    await settle();
    expect(host.state("git")).toBe("on");
    expect(host.state("tags")).toBe("on");
    syncPlugins([], host);
    await settle();
    expect(host.state("git")).not.toBe("on");
  });
});
