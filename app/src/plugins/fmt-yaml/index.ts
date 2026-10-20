// Plugin `fmt-yaml`: YAML files as a sheet of cards of keys and values; Edit changes one value at a time.

import { NO_REQUIREMENT } from "../../core/conditions/available";
import { definePlugin } from "../../core/plugins/registry.svelte";

export default definePlugin({
  manifest: { id: "fmt-yaml", kind: "format", dependsOn: [] },
  activate(cx) {
    return cx.contribute({
      kind: "viewer",
      id: "yaml",
      claims: (name) => /\.ya?ml$/i.test(name),
      requires: NO_REQUIREMENT,
      view: () => import("../../core/config/ConfigView.svelte"),
    });
  },
});
