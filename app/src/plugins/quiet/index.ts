// Plugin `quiet`: silence a folder or a file. It stays in the tree (quieter, with a word in its tooltip) and can be
// opened and searched, but its changes stop calling for attention elsewhere (unread, feed, follow, git letters).

import { NO_REQUIREMENT } from "../../core/conditions/available";
import { documents } from "../../core/doc/document.svelte";
import { definePlugin } from "../../core/plugins/registry.svelte";
import { silence } from "../../core/silence/silence.svelte";
import { quiet } from "./store.svelte";

export default definePlugin({
  manifest: { id: "quiet", kind: "feature", dependsOn: [] },
  activate(cx) {
    const stop = quiet.start();
    const removals = [
      cx.contribute({
        kind: "treeBadge",
        id: "quiet.badge",
        requires: NO_REQUIREMENT,
        badge: (path) => quiet.badge(path),
      }),
      cx.contribute({
        kind: "treeMenu",
        id: "quiet.menu.silence",
        label: "quiet.silence",
        requires: NO_REQUIREMENT,
        applies: (path) => !silence.isSilenced(path),
        run: ([path]) => {
          if (path !== undefined) void quiet.set([path], "quiet");
        },
      }),
      cx.contribute({
        kind: "treeMenu",
        id: "quiet.menu.listen",
        label: "quiet.listen",
        requires: NO_REQUIREMENT,
        applies: (path) => silence.isSilenced(path),
        run: ([path]) => {
          if (path !== undefined) void quiet.set([path], "speaking");
        },
      }),
      cx.contribute({
        kind: "command",
        id: "quiet.silenceCurrent",
        label: "quiet.silenceCurrent",
        requires: NO_REQUIREMENT,
        when: () => documents.path !== null && !silence.isSilenced(documents.path),
        run: () => {
          const path = documents.path;
          if (path !== null) void quiet.set([path], "quiet");
        },
      }),
      cx.contribute({
        kind: "command",
        id: "quiet.listenCurrent",
        label: "quiet.listenCurrent",
        requires: NO_REQUIREMENT,
        when: () => documents.path !== null && silence.isSilenced(documents.path),
        run: () => {
          const path = documents.path;
          if (path !== null) void quiet.set([path], "speaking");
        },
      }),
    ];
    return () => {
      for (const remove of removals) remove();
      stop();
    };
  },
});
