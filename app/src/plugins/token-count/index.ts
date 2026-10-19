// Plugin `token-count`: tokens of the document, the selection and each section by the tokenizer the person chooses.
// Exact where the tokenizer is public (the vocabulary is downloaded once, when the person asks), approximate where it
// is not; an approximate number is always marked. Off by default; off, it leaves nothing on screen.

import { NO_REQUIREMENT } from "../../core/conditions/available";
import { definePlugin } from "../../core/plugins/registry.svelte";
import { ui } from "../../core/shell/ui.svelte";
import { sections } from "./sections.svelte";
import { tokens } from "./store.svelte";

export default definePlugin({
  manifest: { id: "token-count", kind: "analysis", dependsOn: [] },
  activate(cx) {
    const stop = tokens.start();
    const removals = [
      cx.contribute({
        kind: "statusItem",
        id: "token-count.status",
        slot: "right",
        order: 1.5,
        requires: NO_REQUIREMENT,
        view: () => import("./TokenStatus.svelte"),
      }),
      cx.contribute({
        kind: "settingsSection",
        id: "token-count.settings",
        label: "tokens.section",
        requires: NO_REQUIREMENT,
        view: () => import("./TokenSettings.svelte"),
      }),
      cx.contribute({
        kind: "projectSection",
        id: "token-count.project",
        label: "tokens.section",
        requires: NO_REQUIREMENT,
        view: () => import("./TokenProject.svelte"),
      }),
      cx.contribute({
        kind: "outlineColumn",
        id: "token-count.outline",
        requires: NO_REQUIREMENT,
        header: () => sections.header(),
        cell: (index) => sections.cell(index),
        watcher: () => import("./OutlineTokens.svelte"),
      }),
      cx.contribute({
        kind: "command",
        id: "tokens.choose",
        label: "tokens.command",
        requires: NO_REQUIREMENT,
        run: () => ui.openSettingsPage("integration"),
      }),
    ];
    return () => {
      for (const remove of removals) remove();
      sections.clear();
      stop();
    };
  },
});
