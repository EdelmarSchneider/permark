// Plugin `token-count`: tokens of the document, the selection and each section by the tokenizer the person chooses.
// Exact where the tokenizer is public (the vocabulary is downloaded once, when the person asks), approximate where it
// is not; an approximate number is always marked. A folder can be counted from the Explorer (a guard asks first when
// it is big; the count never blocks the app). Off by default; off, it leaves nothing on screen.

import { NO_REQUIREMENT } from "../../core/conditions/available";
import { documents } from "../../core/doc/document.svelte";
import { explorerSelectedFolder } from "../../core/explorer/current";
import { definePlugin } from "../../core/plugins/registry.svelte";
import { ui } from "../../core/shell/ui.svelte";
import { parentOf } from "./folders";
import { folders } from "./folders.svelte";
import { sections } from "./sections.svelte";
import { tokens } from "./store.svelte";

export default definePlugin({
  manifest: { id: "token-count", kind: "analysis", dependsOn: [] },
  activate(cx) {
    const stopTokens = tokens.start();
    const stopFolders = folders.start();
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
        kind: "statusItem",
        id: "token-count.folders",
        slot: "right",
        order: 99,
        requires: NO_REQUIREMENT,
        view: () => import("./FolderHost.svelte"),
      }),
      cx.contribute({
        kind: "treeBadge",
        id: "token-count.folder-badge",
        requires: NO_REQUIREMENT,
        badge: (path, isDir) => folders.badge(path, isDir),
      }),
      cx.contribute({
        kind: "treeMenu",
        id: "token-count.folder-count",
        label: "tokens.folder.menu",
        requires: NO_REQUIREMENT,
        applies: (_path, isDir) => isDir,
        run: ([path]) => {
          if (path !== undefined) void folders.count(path);
        },
      }),
      cx.contribute({
        kind: "treeMenu",
        id: "token-count.folder-summary",
        label: "tokens.folder.menuSummary",
        requires: NO_REQUIREMENT,
        applies: (path, isDir) => isDir && folders.has(path),
        run: ([path]) => {
          if (path !== undefined) void folders.showSummary(path);
        },
      }),
      cx.contribute({
        kind: "command",
        id: "tokens.countFolder",
        label: "tokens.folder.commandHere",
        requires: NO_REQUIREMENT,
        run: () => {
          const shown = documents.path;
          void folders.count(explorerSelectedFolder() ?? (shown === null ? "" : parentOf(shown)));
        },
      }),
      cx.contribute({
        kind: "command",
        id: "tokens.countProject",
        label: "tokens.folder.commandProject",
        requires: NO_REQUIREMENT,
        run: () => void folders.count(""),
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
      stopFolders();
      stopTokens();
    };
  },
});
