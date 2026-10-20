// Plugin `fmt-ignore`: ignore files (`.gitignore`, `.aiignore`, ...) as a sheet of cards of keys and values; Edit changes one value at a time.

import { NO_REQUIREMENT } from "../../core/conditions/available";
import { definePlugin } from "../../core/plugins/registry.svelte";

export default definePlugin({
  manifest: { id: "fmt-ignore", kind: "format", dependsOn: [] },
  activate(cx) {
    return cx.contribute({
      kind: "viewer",
      id: "ignore",
      claims: (name) => /^\..*ignore$/i.test(name),
      requires: NO_REQUIREMENT,
      view: () => import("../../core/config/ConfigView.svelte"),
    });
  },
});
