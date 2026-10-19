// Plugin `broken-links`: the Markdown links of the project whose target file is missing, by document and line. Click
// opens the document at the line. The list is kept true by the backend as the project changes.

import Unlink from "@lucide/svelte/icons/unlink";
import { NO_REQUIREMENT } from "../../core/conditions/available";
import { definePlugin } from "../../core/plugins/registry.svelte";
import { ui } from "../../core/shell/ui.svelte";
import { PANEL } from "./ids";
import { brokenLinks } from "./store.svelte";

export default definePlugin({
  manifest: { id: "broken-links", kind: "checker", dependsOn: [] },
  activate(cx) {
    const stop = brokenLinks.start();
    const removals = [
      cx.contribute({
        kind: "panel",
        id: PANEL,
        side: "left",
        label: "brokenLinks.title",
        icon: Unlink,
        requires: NO_REQUIREMENT,
        view: () => import("./BrokenLinksPanel.svelte"),
      }),
      cx.contribute({
        kind: "command",
        id: "brokenLinks.show",
        label: "brokenLinks.show",
        requires: NO_REQUIREMENT,
        run: () => ui.showSidebar(PANEL),
      }),
    ];
    return () => {
      for (const remove of removals) remove();
      if (ui.sidebar === PANEL) ui.showSidebar("files");
      stop();
    };
  },
});
