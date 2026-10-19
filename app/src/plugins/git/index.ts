// Plugin `git`: change letters on files, a dot on folders, branch + HEAD + state in the status bar, the legend, and
// the way out when git cannot be used. Read-only: PerMark never commits, stages or creates repositories.

import type { Requirement } from "../../core/conditions/available";
import { NO_REQUIREMENT } from "../../core/conditions/available";
import { definePlugin } from "../../core/plugins/registry.svelte";
import { git } from "./store.svelte";

const REPO: Requirement = { needs: [{ need: "gitRepo" }], prefers: [] };

export default definePlugin({
  manifest: { id: "git", kind: "feature", dependsOn: [] },
  activate(cx) {
    const stop = git.start();
    const removals = [
      cx.contribute({
        kind: "treeBadge",
        id: "git.badge",
        requires: REPO,
        badge: (path, isDir) => git.badge(path, isDir),
      }),
      cx.contribute({
        kind: "statusItem",
        id: "git.status",
        slot: "left",
        order: 5,
        requires: REPO,
        view: () => import("./GitStatus.svelte"),
      }),
      // the problem has no repository to need: it shows by itself when git cannot be used
      cx.contribute({
        kind: "statusItem",
        id: "git.problem",
        slot: "left",
        order: 6,
        requires: NO_REQUIREMENT,
        view: () => import("./GitProblem.svelte"),
      }),
      cx.contribute({
        kind: "docBar",
        id: "git.banner",
        order: 10,
        requires: REPO,
        view: () => import("./GitBanner.svelte"),
      }),
      cx.contribute({
        kind: "settingsSection",
        id: "git.settings",
        label: "git.backend.label",
        requires: NO_REQUIREMENT,
        view: () => import("./GitSettings.svelte"),
      }),
      cx.contribute({
        kind: "command",
        id: "git.legend",
        label: "git.legend.menu",
        requires: REPO,
        run: () => {
          git.legend = true;
        },
      }),
    ];
    return () => {
      for (const remove of removals) remove();
      git.legend = false;
      stop();
    };
  },
});
