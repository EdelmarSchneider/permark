// Plugin `correction`: fixes spelling, grammar and punctuation with the local model assigned to job "correction"
// (plugin `local-ai`), every change reviewed before it lands. Dormant until a model is assigned: no button, no
// palette entry, no key acts. Then, while editing, the entries of a piece review (`local-ai/pieces/entries.ts`): a
// button with the mode switch (selection | this paragraph | whole document), Mod+Alt+K (selection, else the
// paragraph; in review: the keyboard to the bar) and the review bar under the header. None of them starts a run while
// another plugin's review is open. F7 | Shift+F7 in review are `local-ai`'s: one pair for whichever review is open.

import SpellCheck from "@lucide/svelte/icons/spell-check";
import { definePlugin } from "../../core/plugins/registry.svelte";
import { contributeReview } from "../local-ai/pieces/entries";
import { correction } from "./store.svelte";

export default definePlugin({
  manifest: { id: "correction", kind: "feature", dependsOn: ["local-ai"] },
  activate(cx) {
    const stop = correction.start();
    const remove = contributeReview(cx, {
      review: correction,
      action: { id: "correction.menu", label: "correction.action", icon: SpellCheck },
      bar: { id: "correction.bar", order: 15, view: () => import("./CorrectionBar.svelte") },
      scopes: {
        selection: "correction.scope.selection",
        paragraph: "correction.scope.paragraph",
        document: "correction.scope.document",
      },
      here: { id: "correction.correct", label: "correction.command", keys: "Mod+Alt+k" },
      document: { id: "correction.correctDocument", label: "correction.commandDocument" },
    });
    return () => {
      remove();
      stop();
    };
  },
});
