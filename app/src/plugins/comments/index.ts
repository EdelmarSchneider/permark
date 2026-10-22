// Plugin `comments`: comments on passages, anchored so they follow the text when it changes. A bar in the margin, the
// Comments tab (open, done, replies, jump), a field under the selected passage (C), and the prompt that hands comments
// to an agent. The text of a comment is the person's and lives in the project's local state, never in project files.

import MessageSquare from "@lucide/svelte/icons/message-square";
import { NO_REQUIREMENT } from "../../core/conditions/available";
import { definePlugin } from "../../core/plugins/registry.svelte";
import { ui } from "../../core/shell/ui.svelte";
import { PANEL } from "./ids";
import { decorateMargin } from "./margin.svelte";
import { readSelection } from "./marks";
import { comments } from "./store.svelte";

export default definePlugin({
  manifest: { id: "comments", kind: "feature", dependsOn: [] },
  activate(cx) {
    const stop = comments.start();
    const removals = [
      cx.contribute({
        kind: "treeBadge",
        id: "comments.badge",
        requires: NO_REQUIREMENT,
        badge: (path, isDir) => comments.badge(path, isDir),
      }),
      cx.contribute({
        kind: "projectCount",
        id: "comments.projectCount",
        requires: NO_REQUIREMENT,
        count: (row) => comments.projectCount(row.path),
      }),
      cx.contribute({
        kind: "panel",
        id: PANEL,
        side: "left",
        label: "comments.list",
        icon: MessageSquare,
        requires: NO_REQUIREMENT,
        view: () => import("./CommentsPanel.svelte"),
      }),
      // draws nothing in the bar: the field and the button of a selection live over the window
      cx.contribute({
        kind: "statusItem",
        id: "comments.host",
        slot: "right",
        order: 91,
        requires: NO_REQUIREMENT,
        view: () => import("./CommentHost.svelte"),
      }),
      cx.contribute({
        kind: "docDecoration",
        id: "comments.margin",
        requires: NO_REQUIREMENT,
        decorate: (root) => decorateMargin(root),
      }),
      cx.contribute({
        kind: "selectionMenu",
        id: "comments.selection",
        label: "comments.addMenu",
        requires: NO_REQUIREMENT,
        run: (selection) => {
          comments.begin(selection);
        },
      }),
      cx.contribute({
        kind: "command",
        id: "comments.add",
        label: "comments.addCommand",
        requires: NO_REQUIREMENT,
        when: () => readSelection() !== null,
        run: () => {
          const selection = readSelection();
          if (selection) comments.begin(selection);
        },
      }),
      cx.contribute({
        kind: "shortcut",
        keys: "c",
        command: "comments.add",
        requires: NO_REQUIREMENT,
      }),
      cx.contribute({
        kind: "command",
        id: "comments.show",
        label: "comments.show",
        requires: NO_REQUIREMENT,
        run: () => ui.showSidebar(PANEL),
      }),
      cx.contribute({
        kind: "command",
        id: "comments.sendAll",
        label: "comments.sendAllCommand",
        requires: NO_REQUIREMENT,
        when: () => comments.count > 0,
        run: () => void comments.sendAllToAgent(),
      }),
    ];
    return () => {
      for (const remove of removals) remove();
      comments.cancel();
      if (ui.sidebar === PANEL) ui.showSidebar("files");
      stop();
    };
  },
});
