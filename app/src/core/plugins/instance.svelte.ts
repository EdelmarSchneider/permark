// The registry the core UI renders contributions from (one per window), and the host that activates plugin modules
// into it. The backend decides which plugins are on (`context.plugins`); `syncPlugins` makes the UI follow.

import type { PluginId } from "../../api/generated/PluginId";
import { ContributionRegistry, PluginHost, pluginLoaders } from "./registry.svelte";

export const contributions = new ContributionRegistry();
export const host = new PluginHost(contributions, pluginLoaders());

let asked = new Set<PluginId>();

/** Activates the plugins that came on, deactivates the ones that went off (ids without a UI module are skipped). */
export function syncPlugins(on: readonly PluginId[], plugins: PluginHost = host): void {
  const wanted = new Set(on.filter((id) => plugins.has(id)));
  for (const id of asked) {
    if (!wanted.has(id)) plugins.disable(id);
  }
  for (const id of wanted) {
    if (!asked.has(id)) void plugins.enable(id);
  }
  asked = wanted;
}
