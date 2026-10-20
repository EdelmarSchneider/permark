// Plugin `highlight`: syntax highlighting, the language label and the copy button of code blocks (`@code` embed).

import { NO_REQUIREMENT } from "../../core/conditions/available";
import { definePlugin } from "../../core/plugins/registry.svelte";

export default definePlugin({
  manifest: { id: "highlight", kind: "embed", dependsOn: [] },
  activate(cx) {
    return cx.contribute({
      kind: "embed",
      id: "highlight.code",
      language: "@code",
      requires: NO_REQUIREMENT,
      view: () => import("./CodeBlock.svelte"),
    });
  },
});
