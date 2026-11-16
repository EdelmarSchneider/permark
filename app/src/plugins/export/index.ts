// Plugin `export` (on by default, dormant): the open Markdown document saved as a PDF | a standalone HTML page, or
// printed, with the reading view's look on light paper. Reached from the document's "…" menu; Mod+Alt+p prints
// (Mod+p stays Quick open). Nothing loads until the person asks: the builder comes with the first run.

import { NO_REQUIREMENT } from "../../core/conditions/available";
import { documents } from "../../core/doc/document.svelte";
import { definePlugin } from "../../core/plugins/registry.svelte";
import { exporter, type Entry } from "./store.svelte";

/** Only the Markdown document the window shows: its saved file is what the page is built from. */
function shownMarkdown(path: string): boolean {
  return documents.path === path && documents.view?.body.kind === "markdown";
}

export default definePlugin({
  manifest: { id: "export", kind: "feature", dependsOn: [] },
  activate(cx) {
    const menu = (
      id: string,
      label: "export.menu.pdf" | "export.menu.html" | "export.menu.print",
      what: Entry,
    ) =>
      cx.contribute({
        kind: "docMenu",
        id,
        label,
        requires: NO_REQUIREMENT,
        applies: shownMarkdown,
        run: (path) => void exporter.start(what, path),
      });
    const removals = [
      // draws nothing in the bar: the page setup dialog over the window while it is open
      cx.contribute({
        kind: "statusItem",
        id: "export.host",
        slot: "right",
        order: 97,
        requires: NO_REQUIREMENT,
        view: () => import("./ExportHost.svelte"),
      }),
      // listed by id: PDF, HTML, Print
      menu("export.1pdf", "export.menu.pdf", "pdf"),
      menu("export.2html", "export.menu.html", "html"),
      menu("export.3print", "export.menu.print", "print"),
      cx.contribute({
        kind: "command",
        id: "export.print",
        label: "export.menu.print",
        requires: NO_REQUIREMENT,
        when: () => documents.path !== null && shownMarkdown(documents.path),
        run: () => {
          if (documents.path !== null) void exporter.start("print", documents.path);
        },
      }),
      cx.contribute({
        kind: "shortcut",
        keys: "Mod+Alt+p",
        command: "export.print",
        requires: NO_REQUIREMENT,
      }),
    ];
    return () => {
      for (const remove of removals) remove();
      exporter.close();
    };
  },
});
