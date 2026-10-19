// Plugin `follow`: follow mode (Ctrl+Shift+L) opens the document an agent is writing; a chip says it is on and how to
// get back to where the person was.

import { NO_REQUIREMENT } from "../../core/conditions/available";
import { definePlugin } from "../../core/plugins/registry.svelte";
import { follow } from "./store.svelte";

export default definePlugin({
  manifest: { id: "follow", kind: "feature", dependsOn: ["live-feed"] },
  activate(cx) {
    const stop = follow.start();
    const removals = [
      cx.contribute({
        kind: "command",
        id: "follow.toggle",
        label: "follow.toggle",
        requires: NO_REQUIREMENT,
        run: () => follow.toggle(),
      }),
      cx.contribute({
        kind: "shortcut",
        keys: "Mod+Shift+l",
        command: "follow.toggle",
        requires: NO_REQUIREMENT,
      }),
      cx.contribute({
        kind: "statusItem",
        id: "follow.status",
        slot: "right",
        order: 7,
        requires: NO_REQUIREMENT,
        view: () => import("./FollowStatus.svelte"),
      }),
    ];
    return () => {
      for (const remove of removals) remove();
      stop();
    };
  },
});
