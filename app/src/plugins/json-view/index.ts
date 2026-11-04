// Plugin `json-view`: JSON as a read-only tree, and JSON Lines as one tree per record. The text is one click away.

import { NO_REQUIREMENT } from "../../core/conditions/available";
import { definePlugin } from "../../core/plugins/registry.svelte";

export default definePlugin({
  manifest: { id: "json-view", kind: "format", dependsOn: [] },
  activate(cx) {
    const offs = [
      cx.contribute({
        kind: "viewer",
        id: "json",
        claims: (name) => /\.(json|jsonc|geojson|webmanifest)$/i.test(name),
        requires: NO_REQUIREMENT,
        view: () => import("./JsonView.svelte"),
      }),
      cx.contribute({
        kind: "viewer",
        id: "jsonl",
        claims: (name) => /\.(jsonl|ndjson|jsonlines)$/i.test(name),
        requires: NO_REQUIREMENT,
        view: () => import("./JsonlView.svelte"),
      }),
    ];
    return () => {
      for (const off of offs) off();
    };
  },
});
