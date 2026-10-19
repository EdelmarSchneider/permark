// Plugin `tags`: tags and a one-line description for any file, without touching it. A strip under the document header,
// a row tooltip, the Tags tab with counts and the files that have the selected tags, the editor (T), and the tag filter
// of the search boxes.

import Tag from "@lucide/svelte/icons/tag";
import { NO_REQUIREMENT } from "../../core/conditions/available";
import { documents } from "../../core/doc/document.svelte";
import { definePlugin } from "../../core/plugins/registry.svelte";
import { searchTags } from "../../core/search/tags.svelte";
import { ui } from "../../core/shell/ui.svelte";
import { PANEL } from "./ids";
import { tags } from "./store.svelte";

export default definePlugin({
  manifest: { id: "tags", kind: "feature", dependsOn: [] },
  activate(cx) {
    const stop = tags.start();
    // Search everywhere offers the tags of the project and the boxes narrow to them
    searchTags.setProvider({ list: () => tags.overview?.tags ?? [] });
    const removals = [
      cx.contribute({
        kind: "treeBadge",
        id: "tags.badge",
        requires: NO_REQUIREMENT,
        badge: (path, isDir) => tags.badge(path, isDir),
      }),
      cx.contribute({
        kind: "panel",
        id: PANEL,
        side: "left",
        label: "tags.title",
        icon: Tag,
        requires: NO_REQUIREMENT,
        view: () => import("./TagsPanel.svelte"),
      }),
      cx.contribute({
        kind: "docBar",
        id: "tags.strip",
        order: 20,
        requires: NO_REQUIREMENT,
        view: () => import("./TagStrip.svelte"),
      }),
      // draws nothing in the bar: the editor opens over the window when asked
      cx.contribute({
        kind: "statusItem",
        id: "tags.editor",
        slot: "right",
        order: 90,
        requires: NO_REQUIREMENT,
        view: () => import("./TagEditorHost.svelte"),
      }),
      cx.contribute({
        kind: "searchFilter",
        id: "tags.filter",
        requires: NO_REQUIREMENT,
        view: () => import("./SearchTagFilter.svelte"),
      }),
      cx.contribute({
        kind: "command",
        id: "tags.edit",
        label: "tags.edit",
        requires: NO_REQUIREMENT,
        when: () => documents.path !== null,
        run: () => {
          const path = documents.path;
          if (path !== null) tags.openEditor(path);
        },
      }),
      cx.contribute({
        kind: "shortcut",
        keys: "t",
        command: "tags.edit",
        requires: NO_REQUIREMENT,
      }),
      cx.contribute({
        kind: "command",
        id: "tags.show",
        label: "tags.show",
        requires: NO_REQUIREMENT,
        run: () => ui.showSidebar(PANEL),
      }),
      cx.contribute({
        kind: "treeMenu",
        id: "tags.menu.edit",
        label: "tags.menu",
        requires: NO_REQUIREMENT,
        applies: (_path, isDir) => !isDir,
        run: ([path]) => {
          if (path !== undefined) tags.openEditor(path);
        },
      }),
    ];
    return () => {
      for (const remove of removals) remove();
      searchTags.setProvider(null);
      tags.closeEditor();
      if (ui.sidebar === PANEL) ui.showSidebar("files");
      stop();
    };
  },
});
