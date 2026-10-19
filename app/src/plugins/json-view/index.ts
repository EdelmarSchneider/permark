// Plugin `json-view`: JSON files as a read-only tree, the text one click away.

import { NO_REQUIREMENT } from "../../core/conditions/available";
import { definePlugin } from "../../core/plugins/registry.svelte";

export default definePlugin({
  manifest: { id: "json-view", kind: "format", dependsOn: [] },
  activate(cx) {
    return cx.contribute({
      kind: "viewer",
      id: "json",
      claims: (name) => /\.(json|jsonc|geojson|webmanifest)$/i.test(name),
      requires: NO_REQUIREMENT,
      view: () => import("./JsonView.svelte"),
    });
  },
});
