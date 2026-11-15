// Plugin `spelling`: misspelled words marked while editing (Edit and Source), checked in the document's own language
// with dictionaries the person downloads (none comes with the app). On, the webview's own spellcheck is off in the
// editors; off, nothing of it is left and the webview's spellcheck is back. Seen while a document is edited: the
// language in the status bar (its picker, the download question), a marked word's menu, the step to the next
// misspelling; its settings in Integration.

import { NO_REQUIREMENT } from "../../core/conditions/available";
import { definePlugin } from "../../core/plugins/registry.svelte";
import { spelling } from "./store.svelte";

export default definePlugin({
  manifest: { id: "spelling", kind: "feature", dependsOn: [] },
  activate(cx) {
    const stop = spelling.start();
    const removals = [
      // draws nothing while no document is edited; the plugin's menus live here
      cx.contribute({
        kind: "statusItem",
        id: "spelling.status",
        slot: "right",
        order: 1.2,
        requires: NO_REQUIREMENT,
        view: () => import("./SpellingStatus.svelte"),
      }),
      cx.contribute({
        kind: "settingsSection",
        id: "spelling.settings",
        label: "spelling.section",
        requires: NO_REQUIREMENT,
        view: () => import("./SpellingSettings.svelte"),
      }),
      // no default key: F7 belongs to the reading view's next change and to a review's next suggestion, and a plain
      // key while the editor has the keyboard is the editor's
      cx.contribute({
        kind: "command",
        id: "spelling.next",
        label: "spelling.next.command",
        requires: NO_REQUIREMENT,
        when: () => spelling.canStep,
        run: async () => {
          await spelling.next();
        },
      }),
    ];
    return () => {
      for (const remove of removals) remove();
      stop();
    };
  },
});
