// Plugin `correction`: fixes spelling, grammar and punctuation with the local model assigned to job "correction"
// (plugin `local-ai`), every change reviewed before it lands. Dormant until a model is assigned: no button, no
// palette entry, no key acts. Then, while editing: a button with the mode switch (selection | this paragraph | whole
// document), Mod+Alt+K (selection, else the paragraph; in review: the keyboard to the bar), F7 | Shift+F7 in review,
// and the review bar under the header.

import SpellCheck from "@lucide/svelte/icons/spell-check";
import { NO_REQUIREMENT } from "../../core/conditions/available";
import { documents } from "../../core/doc/document.svelte";
import { editing } from "../../core/editing/session.svelte";
import { t, type MessageKey } from "../../core/i18n";
import { definePlugin } from "../../core/plugins/registry.svelte";
import { correction } from "./store.svelte";

/** A model is assigned and the open document is edited. */
const offered = (): boolean =>
  correction.available && editing.mode !== "view" && editing.path === documents.path;
const reviewing = (): boolean => offered() && (correction.review?.suggestions.length ?? 0) > 0;

export default definePlugin({
  manifest: { id: "correction", kind: "feature", dependsOn: ["local-ai"] },
  activate(cx) {
    const stop = correction.start();
    const command = (
      id: string,
      label: MessageKey,
      when: () => boolean,
      run: () => void,
      keys?: string,
    ): (() => void)[] => [
      cx.contribute({ kind: "command", id, label, requires: NO_REQUIREMENT, when, run }),
      ...(keys === undefined
        ? []
        : [cx.contribute({ kind: "shortcut", keys, command: id, requires: NO_REQUIREMENT })]),
    ];
    const removals = [
      cx.contribute({
        kind: "editAction",
        id: "correction.menu",
        label: "correction.action",
        icon: SpellCheck,
        requires: NO_REQUIREMENT,
        when: offered,
        command: "correction.correct",
        entries: () => [
          {
            kind: "item",
            label: t("correction.scope.selection"),
            disabled: editing.scope("selection") === null,
            run: () => void correction.correct("selection"),
          },
          {
            kind: "item",
            label: t("correction.scope.paragraph"),
            run: () => void correction.correct("paragraph"),
          },
          {
            kind: "item",
            label: t("correction.scope.document"),
            run: () => void correction.correct("document"),
          },
        ],
      }),
      cx.contribute({
        kind: "docBar",
        id: "correction.bar",
        order: 15,
        requires: NO_REQUIREMENT,
        view: () => import("./CorrectionBar.svelte"),
      }),
      ...command(
        "correction.correct",
        "correction.command",
        offered,
        () => void correction.correctHere(),
        "Mod+Alt+k",
      ),
      ...command(
        "correction.correctDocument",
        "correction.commandDocument",
        offered,
        () => void correction.correct("document"),
      ),
      ...command("correction.next", "correction.next", reviewing, () => correction.go(1), "F7"),
      ...command(
        "correction.prev",
        "correction.prev",
        reviewing,
        () => correction.go(-1),
        "Shift+F7",
      ),
    ];
    return () => {
      for (const remove of removals) remove();
      stop();
    };
  },
});
