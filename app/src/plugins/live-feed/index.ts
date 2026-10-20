// Plugin `live-feed`: the Changes panel (every file the disk reported, grouped when many change at once), the status
// chip, the quiet toast for documents that changed behind the person's back, and the glow on changed passages.

import { NO_REQUIREMENT } from "../../core/conditions/available";
import { definePlugin } from "../../core/plugins/registry.svelte";
import { ui } from "../../core/shell/ui.svelte";
import { startGlow, stepGlow } from "./glow";
import { feed } from "./store.svelte";

const PANEL = "live-feed.panel";

export default definePlugin({
  manifest: { id: "live-feed", kind: "feature", dependsOn: [] },
  activate(cx) {
    const stop = feed.start();
    const stopGlow = startGlow();
    const removals = [
      cx.contribute({
        kind: "panel",
        id: PANEL,
        side: "right",
        label: "live.label",
        requires: NO_REQUIREMENT,
        view: () => import("./FeedPanel.svelte"),
      }),
      cx.contribute({
        kind: "statusItem",
        id: "live-feed.status",
        slot: "left",
        order: 8,
        requires: NO_REQUIREMENT,
        view: () => import("./FeedStatus.svelte"),
      }),
      cx.contribute({
        kind: "changeStep",
        id: "live-feed.step",
        order: 20,
        requires: NO_REQUIREMENT,
        step: stepGlow,
      }),
      cx.contribute({
        kind: "command",
        id: "feed.toggle",
        label: "feed.toggle",
        requires: NO_REQUIREMENT,
        run: () => ui.toggleRight(PANEL),
      }),
    ];
    return () => {
      for (const remove of removals) remove();
      if (ui.right === PANEL) ui.right = null;
      stopGlow();
      stop();
    };
  },
});
