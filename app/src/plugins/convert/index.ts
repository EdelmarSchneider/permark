// Plugin `convert` (off by default: a specialist tool): data from JSON, JSON Lines, YAML, XML, CSV | TSV, TOML and TOON
// turned into TOON and other compact formats and back, with the tokens each choice takes. Reached from the Explorer
// menu, the palette, a selection of the page, a fenced code block and a table of the page.

import ArrowLeftRight from "@lucide/svelte/icons/arrow-left-right";
import { NO_REQUIREMENT } from "../../core/conditions/available";
import { documents } from "../../core/doc/document.svelte";
import { definePlugin } from "../../core/plugins/registry.svelte";
import { convertible, sourceOf } from "./model";
import { converter } from "./store.svelte";
import { decorateTables } from "./tables";

/** The text selected in the page, if any. */
function pageSelection(): string {
  const selection = window.getSelection();
  return selection && !selection.isCollapsed ? selection.toString() : "";
}

export default definePlugin({
  manifest: { id: "convert", kind: "feature", dependsOn: ["fmt-toon"] },
  activate(cx) {
    const offs = [
      // draws nothing in the bar: the dialog over the window while it is open
      cx.contribute({
        kind: "statusItem",
        id: "convert.host",
        slot: "right",
        order: 95,
        requires: NO_REQUIREMENT,
        view: () => import("./ConvertHost.svelte"),
      }),
      cx.contribute({
        kind: "treeMenu",
        id: "convert.menu",
        label: "convert.menu",
        requires: NO_REQUIREMENT,
        applies: (path, isDir) => !isDir && convertible(path),
        run: ([path]) => {
          if (path !== undefined) converter.open({ kind: "file", path });
        },
      }),
      cx.contribute({
        kind: "command",
        id: "convert.open",
        label: "convert.menu",
        requires: NO_REQUIREMENT,
        when: () =>
          pageSelection() !== "" || (documents.path !== null && convertible(documents.path)),
        run: () => {
          const text = pageSelection();
          if (text !== "") {
            converter.open({ kind: "text", text, language: null, beside: documents.path });
          } else if (documents.path !== null) {
            converter.open({ kind: "file", path: documents.path });
          }
        },
      }),
      cx.contribute({
        kind: "selectionMenu",
        id: "convert.selection",
        label: "convert.menu",
        requires: NO_REQUIREMENT,
        run: (selection) => {
          converter.open({
            kind: "text",
            text: selection.text,
            language: null,
            beside: documents.path,
          });
        },
      }),
      cx.contribute({
        kind: "codeAction",
        id: "convert.code",
        label: "convert.menu",
        icon: ArrowLeftRight,
        requires: NO_REQUIREMENT,
        applies: (language) => sourceOf(language) !== null,
        run: (source, language) => {
          converter.open({ kind: "text", text: source, language, beside: documents.path });
        },
      }),
      cx.contribute({
        kind: "docDecoration",
        id: "convert.tables",
        requires: NO_REQUIREMENT,
        decorate: (root) =>
          decorateTables(root, (csv) =>
            converter.open(
              { kind: "text", text: csv, language: "csv", beside: documents.path },
              "csv",
            ),
          ),
      }),
    ];
    return () => {
      offs.forEach((off) => off());
      converter.close();
    };
  },
});
