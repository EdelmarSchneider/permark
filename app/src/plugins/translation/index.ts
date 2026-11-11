// Plugin `translation`: translates text with the local model assigned to job "translation" (plugin `local-ai`) into a
// language the person picks. Dormant until a model is assigned: no button, no palette entry, no key acts. Switched
// on: the review of this window follows the job's model (`translation.available`) and reads the languages chosen
// before. Two uses, both while editing.
// In the text, every translated part reviewed before it lands: the entries of a piece review
// (`local-ai/pieces/entries.ts`): a button with the mode switch (selection | this paragraph | whole document, then
// the language translated into), Mod+Alt+R (selection, else the paragraph; in review: the keyboard to the bar; not
// Mod+Alt+T: Linux desktops open a terminal on it before the app sees it) and the review bar under the header; next |
// previous translation in review are `local-ai`'s keys for whichever review is open. A run with no language chosen
// yet opens the language dialog first and starts on its confirm: nothing is translated into a language nobody picked.
// "Translate to…" and the menu's language entry open the dialog to save the choice alone.
// Into a new file ("Translate to a new file…", in the button's menu and the palette, on a Markdown document): the
// whole document, not reviewed part by part, written beside the original (`copy.svelte.ts`). The dialog always
// opens first and names the file its language makes; the run shows in the status bar with its Cancel.
// The plugin's backend takes one run at a time (a new one stops the one going): a copy does not start while this
// window's translation review is open, and the review's entries are off while a copy runs.
// Switched off: all of it is let go, a copy on its way stopped with nothing written.

import Languages from "@lucide/svelte/icons/languages";
import { NO_REQUIREMENT } from "../../core/conditions/available";
import { editing } from "../../core/editing/session.svelte";
import type { MenuEntry } from "../../core/explorer/menu";
import { nameOf } from "../../core/explorer/list";
import { t } from "../../core/i18n";
import { definePlugin } from "../../core/plugins/registry.svelte";
import { contributeReview, edited, startRun, type RunTarget } from "../local-ai/pieces/entries";
import { copy, copyName } from "./copy.svelte";
import { languageDialog } from "./dialog.svelte";
import { namesIn } from "./languages";
import { translation } from "./store.svelte";

/** Why no review run may start now, in words: a copy has the plugin's backend (`null`: one may). */
function reviewHeld(): string | null {
  return copy.running ? t("translation.copy.busy") : null;
}

/** Starts the run the person asked for; with no language to translate into yet, once one is confirmed. */
function start(target: RunTarget): void {
  // asked again when the language is confirmed: a copy may have started meanwhile
  const run = (): void => {
    if (reviewHeld() === null) startRun(translation, target);
  };
  if (reviewHeld() !== null) return;
  if (translation.languages.target === null)
    languageDialog.ask({ confirm: "translation.action", then: run });
  else run();
}

/** A copy into a new file is offered: a model is assigned and the open document is Markdown, being edited. */
function copyOffered(): boolean {
  return translation.available && edited() && editing.kind === "markdown";
}

/** Why a copy cannot start now, in words: one is on its way, or this window's translation review is open (`null`:
 *  it can). */
function copyHeld(): string | null {
  if (copy.running) return t("translation.copy.busy");
  return translation.review === null ? null : t("localAi.review.busy");
}

/** "Translate to a new file…": asks for the language, whatever was chosen before (the dialog says the name the file
 *  gets for the language picked), and starts the copy on its confirm. */
function startCopy(): void {
  const path = editing.path;
  if (path === null || !copyOffered() || copyHeld() !== null) return;
  const file = nameOf(path);
  languageDialog.ask({
    confirm: "translation.action",
    note: (tag) => t("translation.copy.creates", { name: copyName(file, tag) }),
    then: () => {
      // the document the dialog named, still: the copy takes the one being edited when it starts
      if (editing.path === path && copyHeld() === null) void copy.run();
    },
  });
}

/** The menu's last entry: the language translated into, by name; picking it opens the dialog. */
function languageEntry(): MenuEntry {
  const target = translation.languages.target;
  return {
    kind: "item",
    label:
      target === null
        ? t("translation.menu.choose")
        : t("translation.menu.language", { language: namesIn()(target) }),
    run: () => languageDialog.ask(),
  };
}

/** The menu's entries after the scopes: the copy into a new file where it is offered, then the language. */
function ownEntries(): MenuEntry[] {
  if (!copyOffered()) return [languageEntry()];
  const why = copyHeld();
  return [
    {
      kind: "item",
      label: t("translation.copy.action"),
      disabled: why !== null,
      // said with the entry: a disabled one is still reached and read
      ...(why === null ? {} : { detail: why }),
      run: startCopy,
    },
    { kind: "separator" },
    languageEntry(),
  ];
}

export default definePlugin({
  manifest: { id: "translation", kind: "feature", dependsOn: ["local-ai"] },
  activate(cx) {
    const stop = translation.start();
    const stopCopy = copy.listen();
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
      // there only while a document is translated into a new file
      cx.contribute({
        kind: "statusItem",
        id: "translation.copy",
        slot: "right",
        order: 0.5,
        requires: NO_REQUIREMENT,
        view: () => import("./CopyStatus.svelte"),
      }),
      cx.contribute({
        kind: "command",
        id: "translation.language",
        label: "translation.language.command",
        requires: NO_REQUIREMENT,
        when: () => translation.available,
        run: () => languageDialog.ask(),
      }),
      cx.contribute({
        kind: "command",
        id: "translation.newFile",
        label: "translation.copy.action",
        requires: NO_REQUIREMENT,
        when: () => copyOffered() && copyHeld() === null,
        run: startCopy,
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
        more: ownEntries,
        busy: reviewHeld,
      }),
    ];
    return () => {
      for (const remove of removals) remove();
      languageDialog.close();
      stopCopy();
      stop();
      translation.languages.reset();
    };
  },
});
