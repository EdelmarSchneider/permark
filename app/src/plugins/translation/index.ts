// Plugin `translation`: translates text with the local model assigned to job "translation" (plugin `local-ai`) into a
// language the person picks, every translated part reviewed before it lands. Dormant until a model is assigned: no
// button, no palette entry, no key acts. Switched on: the review of this window follows the job's model
// (`translation.available`) and reads the languages chosen before. Then, while editing, the entries of a piece review
// (`local-ai/pieces/entries.ts`): a button with the mode switch (selection | this paragraph | whole document, then
// the language translated into), Mod+Alt+R (selection, else the paragraph; in review: the keyboard to the bar; not
// Mod+Alt+T: Linux desktops open a terminal on it before the app sees it) and the review bar under the header; next |
// previous translation in review are `local-ai`'s keys for whichever review is open. A run with no language chosen
// yet opens the language dialog first and starts on its confirm: nothing is translated into a language nobody picked.
// "Translate to…" and the menu's language entry open the dialog to save the choice alone. Switched off: all of it is
// let go.

import Languages from "@lucide/svelte/icons/languages";
import { NO_REQUIREMENT } from "../../core/conditions/available";
import { t } from "../../core/i18n";
import { definePlugin } from "../../core/plugins/registry.svelte";
import { contributeReview, startRun, type RunTarget } from "../local-ai/pieces/entries";
import { languageDialog } from "./dialog.svelte";
import { namesIn } from "./languages";
import { translation } from "./store.svelte";

/** Starts the run the person asked for; with no language to translate into yet, once one is confirmed. */
function start(target: RunTarget): void {
  const run = (): void => startRun(translation, target);
  if (translation.languages.target === null)
    languageDialog.ask({ confirm: "translation.action", then: run });
  else run();
}

/** The menu's last entry: the language translated into, by name; picking it opens the dialog. */
function languageEntry(): string {
  const target = translation.languages.target;
  return target === null
    ? t("translation.menu.choose")
    : t("translation.menu.language", { language: namesIn()(target) });
}

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
      contributeReview(cx, {
        review: translation,
        // after correction's button, right under correction's bar
        action: { id: "translation.menu", label: "translation.action", icon: Languages, order: 1 },
        bar: { id: "translation.bar", order: 16, view: () => import("./TranslationBar.svelte") },
        scopes: {
          selection: "translation.scope.selection",
          paragraph: "translation.scope.paragraph",
          document: "translation.scope.document",
        },
        here: { id: "translation.here", label: "translation.command", keys: "Mod+Alt+r" },
        document: { id: "translation.document", label: "translation.commandDocument" },
        start,
        more: () => [{ kind: "item", label: languageEntry(), run: () => languageDialog.ask() }],
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
