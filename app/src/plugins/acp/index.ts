// Plugin `acp` "Assistant" (experimental, off until the person turns it on): a right panel where the person talks to
// an agent of their choice. The agent works on a copy of the project kept outside it; its changes reach the real
// files only hunk by hunk, as the person accepts them. Turned on means asked for: the panel opens by itself in a
// project until the person closes it there.

import Bot from "@lucide/svelte/icons/bot";
import type { Requirement } from "../../core/conditions/available";
import { definePlugin } from "../../core/plugins/registry.svelte";
import { acp, PANEL } from "./store.svelte";

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
      cx.contribute({
        kind: "command",
        id: "acp.toggle",
        label: "acp.toggle",
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
