// Plugin `symbols`: a panel of symbols and emoji that stays beside the document; a click | Enter inserts one at the
// caret of the open editor. Dormant: nothing is on screen until the person opens the panel (the palette command, or
// "Keep open in a panel" in the Edit toolbar's emoji | symbols popup); from then on it is a view like any other.

import Sigma from "@lucide/svelte/icons/sigma";
import { NO_REQUIREMENT } from "../../core/conditions/available";
import { definePlugin } from "../../core/plugins/registry.svelte";
import { PANEL, symbols } from "./store.svelte";

export default definePlugin({
  manifest: { id: "symbols", kind: "feature", dependsOn: [] },
  activate(cx) {
    const removals = [
      cx.contribute({
        kind: "panel",
        id: PANEL,
        side: "right",
        label: "symbols.label",
        icon: Sigma,
        requires: NO_REQUIREMENT,
        view: () => import("./SymbolsPanel.svelte"),
        // dormant: nothing of it (no view, no right-region toggle that would open it) until the person asks
        offered: () => symbols.offered,
      }),
      cx.contribute({
        kind: "command",
        id: "symbols.toggle",
        label: "symbols.toggle",
        requires: NO_REQUIREMENT,
        run: () => symbols.toggle(),
      }),
      cx.contribute({
        kind: "pickerAction",
        id: "symbols.keepOpen",
        label: "picker.keepOpen",
        requires: NO_REQUIREMENT,
        run: (picker) => symbols.keepOpen(picker),
      }),
    ];
    return () => {
      for (const remove of removals) remove();
      symbols.reset();
    };
  },
});
