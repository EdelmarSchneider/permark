// Plugin `word-count`: words, characters and reading time of the document or the selection, in the status bar.

import { NO_REQUIREMENT } from "../../core/conditions/available";
import { definePlugin } from "../../core/plugins/registry.svelte";

export default definePlugin({
  manifest: { id: "word-count", kind: "analysis", dependsOn: [] },
  activate(cx) {
    return cx.contribute({
      kind: "statusItem",
      id: "word-count.status",
      slot: "right",
      order: 1,
      requires: NO_REQUIREMENT,
      view: () => import("./WordCount.svelte"),
    });
  },
});
