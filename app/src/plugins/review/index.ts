// Plugin `review`: what an agent changed since the person last read a document, shown in the page; Keep or Revert each
// change (K | R), all of them, Undo, previous | next (P | N), Esc closes it. Needs `unread` (it follows what was read).
// The earlier versions of a document (a list to compare with one of them) are asked for from the palette, the Explorer
// menu of a file and the document's menu.

import { NO_REQUIREMENT } from "../../core/conditions/available";
import { documents } from "../../core/doc/document.svelte";
import { overlayShown } from "../../core/doc/escape";
import { editing } from "../../core/editing/session.svelte";
import { t, type MessageKey } from "../../core/i18n";
import { definePlugin } from "../../core/plugins/registry.svelte";
import { ui } from "../../core/shell/ui.svelte";
import { reviewBlocks } from "./blocks";
import { closesReview } from "./keys";
import { listen, startMarks } from "./marks";
import { reviewSections } from "./sections.svelte";
import { review } from "./store.svelte";
import { followVersionsOf, versionsOf } from "./versionsOf.svelte";
import "./review.css";

const reviewing = (): boolean => review.state.status === "reviewing";
const withChanges = (): boolean => review.reviewing !== null && review.reviewing.count > 0;

export default definePlugin({
  manifest: { id: "review", kind: "feature", dependsOn: ["unread"] },
  activate(cx) {
    const stop = review.start();
    const stopMarks = startMarks();
    const stopVersionsOf = followVersionsOf();
    const command = (
      id: string,
      label: MessageKey,
      when: () => boolean,
      run: () => void,
      keys?: string,
    ): (() => void)[] => [
      cx.contribute({ kind: "command", id, label, requires: NO_REQUIREMENT, when, run }),
      ...(keys === undefined
        ? []
        : [cx.contribute({ kind: "shortcut", keys, command: id, requires: NO_REQUIREMENT })]),
    ];
    const onEscape = (event: KeyboardEvent): void => {
      const env = {
        reviewing: reviewing(),
        editing: editing.mode !== "view" && editing.path === documents.path,
        modal: ui.modal,
        zoomed: documents.zoomed !== null,
        finding: ui.finding,
        overlay: event.key === "Escape" && overlayShown(),
      };
      if (!closesReview(event, env)) return;
      event.preventDefault();
      void review.done();
    };
    window.addEventListener("keydown", onEscape);
    const removals = [
      () => window.removeEventListener("keydown", onEscape),
      cx.contribute({
        kind: "outlineMark",
        id: "review.changed",
        requires: NO_REQUIREMENT,
        mark: (index) =>
          reviewSections.changed.has(index)
            ? { text: t("outline.changed"), title: t("outline.changedHint") }
            : null,
      }),
      cx.contribute({
        kind: "docBar",
        id: "review.bar",
        order: 20,
        requires: NO_REQUIREMENT,
        view: () => import("./ReviewBar.svelte"),
      }),
      cx.contribute({
        kind: "docBlocks",
        id: "review.blocks",
        requires: NO_REQUIREMENT,
        transform: (blocks, view) => {
          const state = review.state;
          // only the version the review was drawn from; a newer one waits for the review to look again
          if (state.status !== "reviewing" || state.view.unit !== "blocks") return blocks;
          if (state.view.hash !== view.hash) return blocks;
          return reviewBlocks(blocks, state.view, state.at);
        },
      }),
      cx.contribute({
        kind: "bodyOverride",
        id: "review.code",
        requires: NO_REQUIREMENT,
        // code and plain text in review: the code view instead of the text viewer
        when: (view) => {
          const state = review.state;
          return (
            state.status === "reviewing" &&
            state.view.unit === "lines" &&
            state.view.path === view.path
          );
        },
        view: () => import("./CodeReview.svelte"),
      }),
      cx.contribute({
        kind: "changeStep",
        id: "review.step",
        order: 10,
        requires: NO_REQUIREMENT,
        step: (direction) => review.step(direction),
      }),
      cx.contribute({
        kind: "docDecoration",
        id: "review.clicks",
        requires: NO_REQUIREMENT,
        decorate: (root) => listen(root),
      }),
      ...command(
        "review.toggle",
        "review.toggle",
        () => documents.path !== null,
        () => void (reviewing() ? review.done() : review.begin()),
      ),
      ...command("review.keep", "review.keepCommand", withChanges, () => void review.keep(), "k"),
      ...command(
        "review.revert",
        "review.revertCommand",
        withChanges,
        () => void review.revert(),
        "r",
      ),
      ...command("review.next", "review.next", withChanges, () => review.go(1), "n"),
      ...command("review.prev", "review.prev", withChanges, () => review.go(-1), "p"),
      ...command(
        "review.keepAll",
        "review.keepAll",
        withChanges,
        () => void review.keepAll(),
        "Shift+k",
      ),
      ...command(
        "review.revertAll",
        "review.revertAll",
        withChanges,
        () => void review.revertAll(),
        "Shift+r",
      ),
      ...command("review.undo", "review.undo", reviewing, () => void review.undo()),
      // draws nothing in the bar: the list of earlier versions over the window while it is open
      cx.contribute({
        kind: "statusItem",
        id: "review.versions.host",
        slot: "right",
        order: 96,
        requires: NO_REQUIREMENT,
        view: () => import("./VersionsHost.svelte"),
      }),
      ...command(
        "review.versions",
        "review.versions.command",
        () => documents.path !== null,
        () => void review.openVersions(),
      ),
      cx.contribute({
        kind: "treeMenu",
        id: "review.versions.tree",
        label: "review.versions.menu",
        requires: NO_REQUIREMENT,
        applies: (_path, isDir) => !isDir,
        run: ([path]) => {
          if (path !== undefined) void versionsOf(path);
        },
      }),
      cx.contribute({
        kind: "docMenu",
        id: "review.versions.doc",
        label: "review.versions.menu",
        requires: NO_REQUIREMENT,
        run: () => void review.openVersions(),
      }),
    ];
    return () => {
      for (const remove of removals) remove();
      stopVersionsOf();
      stopMarks();
      stop();
    };
  },
});
