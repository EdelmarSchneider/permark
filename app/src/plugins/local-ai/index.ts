// Plugin `local-ai`: small models that run on this computer, for PerMark's own jobs. On but dormant: its settings page
// (+ a palette command to open it) is all it adds; nothing is read until the page opens, nothing downloaded until the
// person asks.

import { NO_REQUIREMENT } from "../../core/conditions/available";
import { definePlugin } from "../../core/plugins/registry.svelte";
import { ui } from "../../core/shell/ui.svelte";
import { LOCAL_AI_PAGE, localAi } from "./store.svelte";

export default definePlugin({
  manifest: { id: "local-ai", kind: "feature", dependsOn: [] },
  activate(cx) {
    const removals = [
      cx.contribute({
        kind: "settingsPage",
        id: LOCAL_AI_PAGE,
        label: "localAi.section",
        order: 35,
        requires: NO_REQUIREMENT,
        view: () => import("./LocalAiSection.svelte"),
      }),
      cx.contribute({
        kind: "command",
        id: "local-ai.openSettings",
        label: "localAi.command",
        requires: NO_REQUIREMENT,
        run: () => ui.openSettingsPage(LOCAL_AI_PAGE, "content"),
      }),
    ];
    return () => {
      for (const remove of removals) remove();
      localAi.stop();
    };
  },
});
