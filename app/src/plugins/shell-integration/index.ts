// Plugin `shell-integration`: "Open with PerMark" and "New Markdown file" in the Windows right-click menu (direct
// builds; a Store package declares its own). Its settings card switches each entry; switched off, the plugin removes
// every entry it made (the backend does it when the plugin goes off).

import type { Requirement } from "../../core/conditions/available";
import { definePlugin } from "../../core/plugins/registry.svelte";

const WINDOWS_DIRECT: Requirement = {
  needs: [
    { need: "platform", platforms: ["windows"] },
    { need: "distribution", distribution: "direct" },
  ],
  prefers: [],
};

export default definePlugin({
  manifest: { id: "shell-integration", kind: "feature", dependsOn: [] },
  activate(cx) {
    const remove = cx.contribute({
      kind: "settingsSection",
      id: "shell.settings",
      label: "shell.section",
      requires: WINDOWS_DIRECT,
      view: () => import("./ShellSettings.svelte"),
    });
    return () => remove();
  },
});
