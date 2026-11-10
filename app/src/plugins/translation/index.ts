// Plugin `translation`: translates text with the local model assigned to job "translation" (plugin `local-ai`) into a
// language the person picks, every translated piece reviewed before it lands. Dormant until a model is assigned.
// Switched on: the review of this window follows the job's model (`translation.available`) and reads the languages
// chosen before; switched off: both are let go.

import { definePlugin } from "../../core/plugins/registry.svelte";
import { translation } from "./store.svelte";

export default definePlugin({
  manifest: { id: "translation", kind: "feature", dependsOn: ["local-ai"] },
  activate() {
    const stop = translation.start();
    void translation.languages.load();
    return () => {
      stop();
      translation.languages.reset();
    };
  },
});
