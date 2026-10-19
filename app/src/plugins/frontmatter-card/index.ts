// Plugin `frontmatter-card`: the front matter of a Markdown document as a properties card above the page.

import { NO_REQUIREMENT } from "../../core/conditions/available";
import { definePlugin } from "../../core/plugins/registry.svelte";

export default definePlugin({
  manifest: { id: "frontmatter-card", kind: "embed", dependsOn: [] },
  activate(cx) {
    return cx.contribute({
      kind: "frontMatterCard",
      id: "frontmatter-card.card",
      requires: NO_REQUIREMENT,
      view: () => import("./Card.svelte"),
    });
  },
});
