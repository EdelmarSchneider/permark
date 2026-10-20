// Plugin `fmt-ini`: INI-style files (`.ini`, `.cfg`, `.editorconfig`, `.gitconfig`) as a sheet of cards of keys and values; Edit changes one value at a time.

import { NO_REQUIREMENT } from "../../core/conditions/available";
import { definePlugin } from "../../core/plugins/registry.svelte";

export default definePlugin({
  manifest: { id: "fmt-ini", kind: "format", dependsOn: [] },
  activate(cx) {
    return cx.contribute({
      kind: "viewer",
      id: "ini",
      claims: (name) =>
        /(\.(ini|cfg)|^\.(editorconfig|gitconfig|gitmodules|npmrc|pypirc|flake8|pylintrc))$/i.test(
          name,
        ),
      requires: NO_REQUIREMENT,
      view: () => import("../../core/config/ConfigView.svelte"),
    });
  },
});
