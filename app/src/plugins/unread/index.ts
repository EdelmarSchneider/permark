// Plugin `unread`: a dot on documents changed since the person read them, a count on folders, the Unread tab with
// what changed since the last visit, Alt+U for the next one.

import { NO_REQUIREMENT } from "../../core/conditions/available";
import { documents } from "../../core/doc/document.svelte";
import { definePlugin } from "../../core/plugins/registry.svelte";
import { ui } from "../../core/shell/ui.svelte";
import { isDocument, unread } from "./store.svelte";

const PANEL = "unread.panel";

export default definePlugin({
  manifest: { id: "unread", kind: "feature", dependsOn: [] },
  activate(cx) {
    const stop = unread.start();
    const shown = (): string | null => documents.path;
    const removals = [
      cx.contribute({
        kind: "treeBadge",
        id: "unread.badge",
        requires: NO_REQUIREMENT,
        badge: (path, isDir) => unread.badge(path, isDir),
      }),
      cx.contribute({
        kind: "panel",
        id: PANEL,
        side: "left",
        label: "unread.title",
        requires: NO_REQUIREMENT,
        view: () => import("./UnreadPanel.svelte"),
      }),
      cx.contribute({
        kind: "statusItem",
        id: "unread.status",
        slot: "right",
        order: 9,
        requires: NO_REQUIREMENT,
        view: () => import("./UnreadStatus.svelte"),
      }),
      cx.contribute({
        kind: "command",
        id: "unread.show",
        label: "unread.show",
        requires: NO_REQUIREMENT,
        run: () => ui.showSidebar(PANEL),
      }),
      cx.contribute({
        kind: "command",
        id: "unread.next",
        label: "unread.next",
        requires: NO_REQUIREMENT,
        when: () => unread.unread.length > 0,
        run: () => {
          const target = unread.next(shown());
          if (target !== null) documents.navigate(target, null);
        },
      }),
      cx.contribute({
        kind: "shortcut",
        keys: "Alt+u",
        command: "unread.next",
        requires: NO_REQUIREMENT,
      }),
      cx.contribute({
        kind: "command",
        id: "unread.markRead",
        label: "unread.markRead",
        requires: NO_REQUIREMENT,
        when: () => {
          const path = shown();
          return path !== null && unread.isUnread(path);
        },
        run: () => {
          const path = shown();
          const hash = documents.view?.hash ?? null;
          if (path !== null && hash !== null) void unread.markRead(path, hash);
        },
      }),
      cx.contribute({
        kind: "command",
        id: "unread.markUnread",
        label: "unread.markUnread",
        requires: NO_REQUIREMENT,
        when: () => {
          const path = shown();
          return path !== null && isDocument(path) && !unread.isUnread(path);
        },
        run: () => {
          const path = shown();
          if (path !== null) void unread.markUnread(path);
        },
      }),
      cx.contribute({
        kind: "command",
        id: "unread.markAllRead",
        label: "unread.markAllRead",
        requires: NO_REQUIREMENT,
        when: () => unread.unread.length > 0,
        run: () => void unread.markFolderRead(""),
      }),
      cx.contribute({
        kind: "treeMenu",
        id: "unread.menu.folder",
        label: "unread.markFolderRead",
        requires: NO_REQUIREMENT,
        applies: (path, isDir) => isDir && unread.countBelow(path) > 0,
        run: ([path]) => {
          if (path !== undefined) void unread.markFolderRead(path);
        },
      }),
      cx.contribute({
        kind: "treeMenu",
        id: "unread.menu.unread",
        label: "unread.markUnread",
        requires: NO_REQUIREMENT,
        applies: (path, isDir) => !isDir && isDocument(path) && !unread.isUnread(path),
        run: ([path]) => {
          if (path !== undefined) void unread.markUnread(path);
        },
      }),
    ];
    return () => {
      for (const remove of removals) remove();
      if (ui.sidebar === PANEL) ui.showSidebar("files");
      stop();
    };
  },
});
