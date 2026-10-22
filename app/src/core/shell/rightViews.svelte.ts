// The right column of this window: the arrangement (app-wide, from the settings), which views are present (open +
// with content), the groups drawn, what is on screen. The project's open | selected choices live in the layout store
// (its session, read behind `layout.reading`); a view the project has no choice for takes its default, decided once
// per project load (`panel.defaultOpen`). Spec: shell.md §Frame (Right region).

import type { Api, AppSettings, SettingChange } from "../../api";
import { conditions } from "../conditions/context.svelte";
import { documents } from "../doc/document.svelte";
import { reading } from "../doc/reading.svelte";
import type { ContributionOf } from "../plugins/registry.svelte";
import { layout } from "./layout.svelte";
import { rightPanels } from "./panels.svelte";
import {
  defaultRightViews,
  drawnGroups,
  OUTLINE,
  parseRightViews,
  selectedIn,
  withViews,
  type DrawnGroup,
  type RightViews,
} from "./rightViews";
import { ui } from "./ui.svelte";

type Panel = ContributionOf<"panel">;

/** How long the views' defaults wait for git to answer after a project shows. */
export const DEFAULT_WAIT_MS = 500;
/** The arrangement is written this long after the last change (a divider dragged). */
const SAVE_DELAY_MS = 400;

/** The arrangement in the settings (`rightViews`; `null` = never set | unreadable: the default). */
export function arrangementOf(settings: AppSettings): RightViews {
  return parseRightViews(settings.rightViews);
}

export class RightViewsStore {
  /** As saved (app-wide). */
  arrangement = $state.raw<RightViews>(defaultRightViews());
  /** The column is too short for its groups one above the other: they are drawn as one tab group. */
  short = $state(false);
  /** What the last move said (a polite status). */
  status = $state("");
  #api: Api | null = null;
  #timer: ReturnType<typeof setTimeout> | null = null;
  /** The project load whose wait for git is over (`layout.loads`; -1: none). */
  #decided = $state(-1);
  /** Defaults taken for the load `#latchedFor`, by view id. */
  #latched = new Map<string, boolean>();
  #latchedFor = -1;

  /** The load the defaults belong to: a project shown, its session read, its context known (-1: not yet). */
  readonly #load = $derived(
    !layout.reading && conditions.current.workspace === "project" ? layout.loads : -1,
  );

  /** The right panels offered now (plugin on, `offered()`), in a fixed order. */
  readonly panels = $derived(rightPanels());

  /** The document shown has an outline. */
  readonly hasOutline = $derived.by(() => {
    const view = documents.view;
    return view?.body.kind === "markdown" && view.body.outline.length > 0;
  });

  /** The arrangement with every view known now listed (a new plugin's view joins the last group). */
  readonly layout = $derived(
    withViews(this.arrangement, [OUTLINE, ...this.panels.map((p) => p.id)]),
  );

  /** The groups with a present view, one above the other as arranged. */
  readonly stacked = $derived(drawnGroups(this.layout, (id) => this.isPresent(id)));

  /** The groups drawn: `stacked`, or all in one tab group while the column is too short. */
  readonly drawn = $derived.by((): DrawnGroup[] => {
    const groups = this.stacked;
    if (!this.short || groups.length < 2) return groups;
    return [{ at: groups[0]?.at ?? 0, views: groups.flatMap((g) => g.views), share: 1 }];
  });

  /** Follows the settings of the app (boot + every change) and decides the views' defaults per project load. */
  start(api: Api): () => void {
    this.#api = api;
    let gone = false;
    let off: (() => void) | null = null;
    void api.settingsGet().then((result) => {
      if (!gone && result.ok) this.arrangement = arrangementOf(result.value);
    });
    void api
      .on("settings-changed", (event) => {
        // our own write comes back: the arrangement shown is newer than a write still waiting
        if (this.#timer === null) this.arrangement = arrangementOf(event.settings);
      })
      .then((stop) => {
        if (gone) stop();
        else off = stop;
      });
    const stopDefaults = $effect.root(() => {
      $effect(() => {
        const load = this.#load;
        if (load < 0) return;
        const timer = setTimeout(() => (this.#decided = load), DEFAULT_WAIT_MS);
        return () => clearTimeout(timer);
      });
    });
    return () => {
      gone = true;
      off?.();
      stopDefaults();
      this.#flush();
    };
  }

  /**
   * Whether `panel` opens when the project has no choice for it: its `defaultOpen()`, taken once per project load.
   * While git may still answer (`DEFAULT_WAIT_MS`), a yes is taken at once and a no waits.
   */
  defaultOf(panel: Panel): boolean {
    const load = this.#load;
    if (load < 0 || panel.defaultOpen === undefined) return false;
    if (this.#latchedFor !== load) {
      this.#latched = new Map();
      this.#latchedFor = load;
    }
    const known = this.#latched.get(panel.id);
    if (known !== undefined) return known;
    const now = panel.defaultOpen();
    if (now || this.#decided === load) this.#latched.set(panel.id, now);
    return now;
  }

  /** The panel is open for this project (its choice, else its default). */
  isOpen(panel: Panel): boolean {
    const chosen = layout.views.open[panel.id];
    return typeof chosen === "boolean" ? chosen : this.defaultOf(panel);
  }

  /** The view is present: the outline when the document has one and it is on; a panel offered + open. */
  isPresent(id: string): boolean {
    if (id === OUTLINE) return this.hasOutline && reading.outline;
    const panel = this.panels.find((p) => p.id === id);
    return panel !== undefined && this.isOpen(panel);
  }

  /** The drawn group that holds `id` (undefined: not drawn). */
  groupWith(id: string): DrawnGroup | undefined {
    return this.drawn.find((g) => g.views.includes(id));
  }

  /** The view a drawn group shows. */
  selectedOf(group: DrawnGroup): string | null {
    return selectedIn(group.views, layout.views.selected);
  }

  /** The view is the one its group shows (a present view alone in its group, or the selected tab). */
  isSelected(id: string): boolean {
    const group = this.groupWith(id);
    return group !== undefined && this.selectedOf(group) === id;
  }

  /** The view is on screen: present, shown by its group, the region shown, not in focus mode. */
  onScreen(id: string): boolean {
    return !layout.rightOff && ui.focus !== "on" && this.isSelected(id);
  }

  /** A new arrangement: drawn at once, written a moment after the last change. */
  arrange(next: RightViews): void {
    this.arrangement = next;
    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#timer = setTimeout(() => this.#flush(), SAVE_DELAY_MS);
  }

  #flush(): void {
    if (this.#timer === null) return;
    clearTimeout(this.#timer);
    this.#timer = null;
    // the settings field `rightViews` (core.md §settings): a failed write keeps the layout for this run
    const { v, groups } = this.arrangement;
    const change: SettingChange = {
      key: "rightViews",
      value: { v, groups: groups.map((g) => ({ views: [...g.views], share: g.share })) },
    };
    void this.#api?.settingsSet(change);
  }
}

export const rightViews = new RightViewsStore();
