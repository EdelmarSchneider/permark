// The Panels menu (the status bar's Panels button, the palette's "Panels…"): every view this window can show now,
// under the region that lists it, each on | off; a pick opens | closes it. And the palette's "Show <name>" entries:
// one per view the person can close, there while that view is offered and not on screen (a plugin off, a dormant
// panel: none); the view shown takes the keyboard.
// Both read the window's views (`arrangement.views`) and their one presence rule (`isPresent`), never a project's
// stored choices: a choice left by a plugin that is off lists nothing.

import { untrack } from "svelte";
import { NO_REQUIREMENT } from "../conditions/available";
import { conditions } from "../conditions/context.svelte";
import type { MenuEntry } from "../explorer/menu";
import { t, type MessageKey } from "../i18n";
import { CORE_OWNER, type ContributionRegistry } from "../plugins/registry.svelte";
import { REGIONS, type Region } from "./arrangement";
import { arrangement, type KnownView } from "./arrangement.svelte";
import { bringView, closeView, focusShownView } from "./sides";
import { ui } from "./ui.svelte";

/** The heading of each region. */
const REGION_NAME: Readonly<Record<Region, MessageKey>> = {
  left: "panel.sidebar",
  right: "panel.right",
  bottom: "panel.bottom",
};

/** The palette entry that shows the view `id`. */
const SHOW_PREFIX = "view.show.";

/** The view can be in this window: a view of a project while one is shown, the others while a workspace is. */
function inWindow(view: KnownView): boolean {
  const workspace = conditions.current.workspace;
  return view.projectOnly ? workspace === "project" : workspace !== "welcome";
}

/** Why the person cannot open | close the view now (`null`: they can). */
function blocked(view: KnownView): string | null {
  if (!view.closable) return t("panel.alwaysShown");
  // the outline lists the headings of the document shown: without them it has nothing to show
  if (view.panel === null && !arrangement.hasOutline) return t("panel.noOutline");
  return null;
}

/** A pick in the menu: a present view is closed (said, with its Undo), any other shown. */
function toggleView(id: string): void {
  const view = arrangement.viewOf(id);
  if (view === undefined || blocked(view) !== null) return;
  if (arrangement.isPresent(id)) closeView(id);
  else bringView(id);
}

/** The entries about the whole arrangement (a view's menu ends with them too): the Layouts dialog, the reset. */
export function wholeEntries(afterReset: () => void = () => {}): MenuEntry[] {
  return [
    { kind: "item", label: t("layouts.open"), run: () => ui.openLayouts() },
    {
      kind: "item",
      label: t("panel.resetLayout"),
      disabled: arrangement.atDefault,
      run: () => {
        arrangement.reset();
        afterReset();
      },
    },
  ];
}

function entryOf(view: KnownView): MenuEntry {
  const why = blocked(view);
  return {
    kind: "item",
    label: t(view.label),
    checkbox: true,
    checked: arrangement.isPresent(view.id),
    disabled: why !== null,
    ...(why === null ? {} : { why }),
    run: () => toggleView(view.id),
  };
}

/** The Panels menu: per region its heading and its views (none: a quiet line saying so), then the whole arrangement. */
export function viewsEntries(): MenuEntry[] {
  const views = arrangement.views.filter(inWindow);
  const regions = REGIONS.flatMap((region): MenuEntry[] => {
    const here = views.filter((view) => (arrangement.regionOf(view.id) ?? view.region) === region);
    return [
      { kind: "heading", label: t(REGION_NAME[region]) },
      ...(here.length === 0
        ? [{ kind: "note", label: t("panel.none") } satisfies MenuEntry]
        : here.map(entryOf)),
    ];
  });
  return [...regions, { kind: "separator" }, ...wholeEntries()];
}

/**
 * Keeps one palette entry "Show <name>" per view the person can close, in `registry`, while the window offers that
 * view: a view offered later gets its entry, one no longer offered loses it. Returns the removal of all of them.
 */
export function registerViewCommands(registry: ContributionRegistry): () => void {
  const added = new Map<string, () => void>();
  const register = (id: string): (() => void) =>
    registry.register({
      kind: "command",
      owner: CORE_OWNER,
      id: `${SHOW_PREFIX}${id}`,
      label: "panel.show",
      labelParams: () => ({ name: arrangement.nameOf(id) }),
      requires: NO_REQUIREMENT,
      when: () => {
        const view = arrangement.viewOf(id);
        return (
          view !== undefined &&
          inWindow(view) &&
          blocked(view) === null &&
          !arrangement.onScreen(id)
        );
      },
      run: async () => {
        bringView(id);
        await focusShownView(id);
      },
    });
  // the registry is not reactive state of the views: the entries follow the views as a subscription would
  const stop = $effect.root(() => {
    $effect(() => {
      const wanted = new Set(arrangement.views.flatMap((v) => (v.closable ? [v.id] : [])));
      untrack(() => {
        for (const [id, remove] of added) {
          if (wanted.has(id)) continue;
          remove();
          added.delete(id);
        }
        for (const id of wanted) if (!added.has(id)) added.set(id, register(id));
      });
    });
  });
  return () => {
    stop();
    for (const remove of added.values()) remove();
    added.clear();
  };
}
