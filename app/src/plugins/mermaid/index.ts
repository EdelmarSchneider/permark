// Plugin `mermaid`: ```` ```mermaid ```` blocks drawn in the page.

import { NO_REQUIREMENT } from "../../core/conditions/available";
import { definePlugin } from "../../core/plugins/registry.svelte";

export default definePlugin({
  manifest: { id: "mermaid", kind: "embed", dependsOn: [] },
  activate(cx) {
    return cx.contribute({
      kind: "embed",
      id: "mermaid.diagram",
      language: "mermaid",
      requires: NO_REQUIREMENT,
      view: () => import("./Diagram.svelte"),
    });
  },
});
