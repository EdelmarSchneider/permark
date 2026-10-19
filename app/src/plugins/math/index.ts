// Plugin `math`: formulas (`$x$`, `$$x$$`, ```` ```math ````) drawn by KaTeX.

import { NO_REQUIREMENT } from "../../core/conditions/available";
import { definePlugin } from "../../core/plugins/registry.svelte";

export default definePlugin({
  manifest: { id: "math", kind: "embed", dependsOn: [] },
  activate(cx) {
    return cx.contribute({
      kind: "embed",
      id: "math.formula",
      language: "@math",
      requires: NO_REQUIREMENT,
      view: () => import("./Math.svelte"),
    });
  },
});
