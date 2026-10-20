// Plugin `fmt-toon`: TOON files as cards and tables (Edit changes values, keys and rows in place), and `toon` blocks of
// a Markdown page drawn the same way.

import { NO_REQUIREMENT } from "../../core/conditions/available";
import { definePlugin } from "../../core/plugins/registry.svelte";

export default definePlugin({
  manifest: { id: "fmt-toon", kind: "format", dependsOn: [] },
  activate(cx) {
    const offs = [
      cx.contribute({
        kind: "viewer",
        id: "toon",
        claims: (name) => /\.toon$/i.test(name),
        requires: NO_REQUIREMENT,
        view: () => import("./ToonView.svelte"),
      }),
      cx.contribute({
        kind: "embed",
        id: "toon.block",
        language: "toon",
        requires: NO_REQUIREMENT,
        view: () => import("./ToonEmbed.svelte"),
      }),
    ];
    return () => offs.forEach((off) => off());
  },
});
