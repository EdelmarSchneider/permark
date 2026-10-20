// Plugin `updates`: signed self-updates of the direct builds. The backend checks a few seconds after the start and then
// once a day (quietly); this window shows a chip when something waits for the person, the settings card, and the
// command "Check for updates". A Store build has neither: the store updates it.

import type { Requirement } from "../../core/conditions/available";
import { definePlugin } from "../../core/plugins/registry.svelte";
import { updates } from "./store.svelte";

const DIRECT: Requirement = {
  needs: [{ need: "distribution", distribution: "direct" }],
  prefers: [],
};

export default definePlugin({
  manifest: { id: "updates", kind: "feature", dependsOn: [] },
  activate(cx) {
    const stop = updates.start();
    const removals = [
      cx.contribute({
        kind: "statusItem",
        id: "updates.chip",
        slot: "right",
        order: 2,
        requires: DIRECT,
        view: () => import("./UpdateChip.svelte"),
      }),
      cx.contribute({
        kind: "settingsSection",
        id: "updates.settings",
        label: "updates.section",
        requires: DIRECT,
        view: () => import("./UpdateSettings.svelte"),
      }),
      cx.contribute({
        kind: "command",
        id: "updates.check",
        label: "updates.command",
        requires: DIRECT,
        run: () => void updates.checkAndTell(),
      }),
    ];
    return () => {
      for (const remove of removals) remove();
      stop();
    };
  },
});
