// Plugin `graphviz`: ```` ```dot ```` (and `graphviz`) blocks drawn in the page, offline.

import { NO_REQUIREMENT } from "../../core/conditions/available";
import { definePlugin } from "../../core/plugins/registry.svelte";

export default definePlugin({
  manifest: { id: "graphviz", kind: "embed", dependsOn: [] },
  activate(cx) {
    const view = () => import("./Diagram.svelte");
    const offs = ["dot", "graphviz"].map((language) =>
      cx.contribute({
        kind: "embed",
        id: `graphviz.${language}`,
        language,
        requires: NO_REQUIREMENT,
        view,
      }),
    );
    return () => offs.forEach((off) => off());
  },
});
