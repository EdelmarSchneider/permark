// Plugin `live-feed`: the Changes panel (every file the disk reported, grouped when many change at once), the status
// chip, the quiet toast for documents that changed behind the person's back, the glow on changed passages and the
// marks on their sections in the outline. Chip + panel stay dormant until the project calls for them.

import Activity from "@lucide/svelte/icons/activity";
import { NO_REQUIREMENT } from "../../core/conditions/available";
import { t } from "../../core/i18n";
import { definePlugin } from "../../core/plugins/registry.svelte";
import { ui } from "../../core/shell/ui.svelte";
import { startGlow, stepGlow } from "./glow";
import { countText, feed, PANEL } from "./store.svelte";

export default definePlugin({
  manifest: { id: "live-feed", kind: "feature", dependsOn: [] },
  activate(cx) {
    const stop = feed.start();
    const stopGlow = startGlow((path, ids) => feed.glow(path, ids));
    const removals = [
      cx.contribute({
        kind: "panel",
        id: PANEL,
        side: "right",
        label: "live.label",
        icon: Activity,
        requires: NO_REQUIREMENT,
        view: () => import("./FeedPanel.svelte"),
        offered: () => feed.offered,
        badge: () => (feed.unseen > 0 ? countText(feed.unseen) : null),
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
        kind: "outlineMark",
        id: "live-feed.changed",
        requires: NO_REQUIREMENT,
        mark: (index) =>
          feed.changed.has(index)
            ? { text: t("outline.changed"), title: t("outline.changedHint") }
            : null,
      }),
      cx.contribute({
        kind: "command",
        id: "feed.toggle",
        label: "feed.toggle",
        requires: NO_REQUIREMENT,
        run: () => feed.toggle(),
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
