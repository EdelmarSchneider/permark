// Plugin `text-view`: any text file, with line numbers; highlighted when the `highlight` plugin is on.

import { NO_REQUIREMENT } from "../../core/conditions/available";
import { definePlugin } from "../../core/plugins/registry.svelte";

export default definePlugin({
  manifest: { id: "text-view", kind: "format", dependsOn: [] },
  activate(cx) {
    return cx.contribute({
      kind: "viewer",
      id: "text",
      claims: () => true,
      requires: NO_REQUIREMENT,
      view: () => import("./TextView.svelte"),
    });
  },
});
