// Plugin `spelling`: misspelled words marked while editing (Edit and Source), checked in the document's own language
// with dictionaries the person downloads (none comes with the app). On, the webview's own spellcheck is off in the
// editors; off, nothing of it is left and the webview's spellcheck is back.

import { definePlugin } from "../../core/plugins/registry.svelte";
import { spelling } from "./store.svelte";

export default definePlugin({
  manifest: { id: "spelling", kind: "feature", dependsOn: [] },
  activate() {
    return spelling.start();
  },
});
