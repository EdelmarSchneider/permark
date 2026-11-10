// The ways into a plugin's piece review, the same for every plugin that has one. Offered while a model is on the
// plugin's job and the open document is edited: a button with the mode switch whose menu starts a run (selection |
// this paragraph | whole document, then the plugin's own entries), the review bar under the header, and the commands:
// "here" (the selection, else the paragraph; with suggestions to go through: the keyboard to the bar), the whole
// document, next | previous suggestion while there are some. One review at a time in an editor: while another
// plugin's is open the runs of this one are off (menu entries that say why, commands not offered, so their keys pass).

import type { Component } from "svelte";
import { NO_REQUIREMENT } from "../../../core/conditions/available";
import { documents } from "../../../core/doc/document.svelte";
import { editing, type EditScope } from "../../../core/editing/session.svelte";
import type { MenuEntry } from "../../../core/explorer/menu";
import { t, type MessageKey } from "../../../core/i18n";
import type { LazyComponent, PluginContext } from "../../../core/plugins/registry.svelte";
import type { PieceReview } from "./review.svelte";

/** What a run is asked over: a scope of the text, or `here` (the selection, else the paragraph: the plugin's key). */
export type RunTarget = EditScope | "here";

/** A command of the review: its id, its name in the palette and its default key, when it has one. */
type CommandEntry = { id: string; label: MessageKey; keys?: string };

/** What a plugin gives for the entries of its review. */
export type ReviewEntries = {
  review: PieceReview;
  /** The button with the mode switch (`order`: its place among such buttons, lowest first). */
  action: { id: string; label: MessageKey; icon: Component<{ size?: number }>; order?: number };
  /** The review bar and its place among the strips under the header. */
  bar: { id: string; order: number; view: LazyComponent };
  /** The menu's words for the three scopes. */
  scopes: Readonly<Record<EditScope, MessageKey>>;
  /** Runs where the caret is; its keys stand in the button's tooltip. */
  here: CommandEntry;
  /** Runs over the whole document. */
  document: CommandEntry;
  /** Step to the next | previous suggestion. */
  next: CommandEntry;
  prev: CommandEntry;
  /** Starts a run the person asked for (default: at once, `startRun`); a plugin with something to ask first (a
   *  language) asks, then starts it. */
  start?: (target: RunTarget) => void;
  /** The plugin's own menu entries, after the scopes. */
  more?: () => MenuEntry[];
};

const SEPARATOR: MenuEntry = { kind: "separator" };

/** Starts the run of `review` over `target`. */
export function startRun(review: PieceReview, target: RunTarget): void {
  void (target === "here" ? review.runHere() : review.runOver(target));
}

/** Adds the entries of a plugin's review; returns the removal of all of them. */
export function contributeReview(cx: PluginContext, entries: ReviewEntries): () => void {
  const { review } = entries;
  /** A model is assigned and the open document is edited. */
  const offered = (): boolean =>
    review.available && editing.mode !== "view" && editing.path === documents.path;
  /** A run may start: no other plugin's review holds the editor. */
  const free = (): boolean => offered() && !review.blocked;
  const reviewing = (): boolean => offered() && (review.review?.suggestions.length ?? 0) > 0;
  const start = entries.start ?? ((target: RunTarget) => startRun(review, target));

  const command = (
    { id, label, keys }: CommandEntry,
    when: () => boolean,
    run: () => void,
  ): (() => void)[] => [
    cx.contribute({ kind: "command", id, label, requires: NO_REQUIREMENT, when, run }),
    ...(keys === undefined
      ? []
      : [cx.contribute({ kind: "shortcut", keys, command: id, requires: NO_REQUIREMENT })]),
  ];
  const scope = (kind: EditScope): MenuEntry => {
    const busy = review.blocked;
    return {
      kind: "item",
      label: t(entries.scopes[kind]),
      disabled: busy || (kind === "selection" && editing.scope("selection") === null),
      // said with the entry: a disabled one is still reached and read
      ...(busy ? { detail: t("localAi.review.busy") } : {}),
      run: () => start(kind),
    };
  };

  const removals = [
    cx.contribute({
      kind: "editAction",
      id: entries.action.id,
      label: entries.action.label,
      icon: entries.action.icon,
      ...(entries.action.order === undefined ? {} : { order: entries.action.order }),
      requires: NO_REQUIREMENT,
      when: offered,
      command: entries.here.id,
      entries: () => {
        const own = entries.more?.() ?? [];
        return [
          scope("selection"),
          scope("paragraph"),
          scope("document"),
          ...(own.length === 0 ? [] : [SEPARATOR, ...own]),
        ];
      },
    }),
    cx.contribute({
      kind: "docBar",
      id: entries.bar.id,
      order: entries.bar.order,
      requires: NO_REQUIREMENT,
      view: entries.bar.view,
    }),
    ...command(entries.here, free, () => start("here")),
    ...command(entries.document, free, () => start("document")),
    ...command(entries.next, reviewing, () => review.go(1)),
    ...command(entries.prev, reviewing, () => review.go(-1)),
  ];
  return () => {
    for (const remove of removals) remove();
  };
}
