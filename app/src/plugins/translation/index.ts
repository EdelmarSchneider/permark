// Plugin `translation`: translates text with the local model assigned to job "translation" (plugin `local-ai`) into a
// language the person picks, every translated piece reviewed before it lands. Dormant until a model is assigned: no
// palette entry. Switched on: the review of this window follows the job's model (`translation.available`) and reads
// the languages chosen before; "Translate to…" opens the language dialog (the choice is saved, nothing is translated).
// Switched off: all of it is let go.

import { NO_REQUIREMENT } from "../../core/conditions/available";
import { definePlugin } from "../../core/plugins/registry.svelte";
import { languageDialog } from "./dialog.svelte";
import { translation } from "./store.svelte";

export default definePlugin({
  manifest: { id: "translation", kind: "feature", dependsOn: ["local-ai"] },
  activate(cx) {
    const stop = translation.start();
    void translation.languages.load();
    const removals = [
      // draws nothing in the bar: the language dialog over the window while it is open
      cx.contribute({
        kind: "statusItem",
        id: "translation.host",
        slot: "right",
        order: 96,
        requires: NO_REQUIREMENT,
        view: () => import("./TranslationHost.svelte"),
      }),
      cx.contribute({
        kind: "command",
        id: "translation.language",
        label: "translation.language.command",
        requires: NO_REQUIREMENT,
        when: () => translation.available,
        run: () => languageDialog.ask(),
      }),
    ];
    return () => {
      for (const remove of removals) remove();
      languageDialog.close();
      stop();
      translation.languages.reset();
    };
  },
});
