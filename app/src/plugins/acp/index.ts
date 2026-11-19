// Plugin `acp` "Assistant" (experimental, off until the person turns it on): a right panel where the person talks to
// an agent of their choice. The agent works on a copy of the project kept outside it; its changes reach the real
// files only hunk by hunk, as the person accepts them. Turned on means asked for: the panel opens by itself in a
// project until the person closes it there.

import Bot from "@lucide/svelte/icons/bot";
import type { Requirement } from "../../core/conditions/available";
import { definePlugin } from "../../core/plugins/registry.svelte";
import { ACP_PAGE, acp, PANEL } from "./store.svelte";

const PROJECT: Requirement = { needs: [{ need: "workspace", kinds: ["project"] }], prefers: [] };

export default definePlugin({
  manifest: { id: "acp", kind: "feature", dependsOn: [] },
  activate(cx) {
    const stop = acp.start();
    const removals = [
      cx.contribute({
        kind: "panel",
        id: PANEL,
        side: "right",
        label: "acp.label",
        icon: Bot,
        requires: PROJECT,
        view: () => import("./AssistantPanel.svelte"),
        defaultOpen: () => true,
      }),
      // an app setting (the person's agent, for every folder), shown only with a folder open: the plugin answers there
      cx.contribute({
        kind: "settingsPage",
        id: ACP_PAGE,
        label: "acp.label",
        icon: Bot,
        group: "app",
        // right after Local AI (35)
        order: 36,
        requires: PROJECT,
        view: () => import("./AssistantSettings.svelte"),
      }),
      cx.contribute({
        kind: "command",
        id: "acp.toggle",
        label: "acp.toggle",
        // the Panels menu and its "Show" palette entry cover it; the id stays for a person's own key
        inPalette: false,
        requires: PROJECT,
        run: () => acp.toggle(),
      }),
      cx.contribute({
        kind: "command",
        id: "acp.chooseAgent",
        label: "acp.chooseAgent",
        requires: PROJECT,
        // the backend refuses a choice while the agent starts | works
        when: () => acp.canChange,
        run: () => acp.openPicker(),
      }),
    ];
    return () => {
      for (const remove of removals) remove();
      stop();
      acp.clear();
    };
  },
});
