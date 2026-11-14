// The views of this window and where they sit: the arrangement (app-wide, from the settings: three regions), which
// views are present (open + with content), the groups each region draws, what is on screen. The project's open |
// selected choices live in the layout store (its session, read behind `layout.reading`); a view the project has no
// choice for takes its default, decided once per project load (`panel.defaultOpen`). Spec: shell.md §Frame.

import FolderTree from "@lucide/svelte/icons/folder-tree";
import ListTree from "@lucide/svelte/icons/list-tree";
import SearchIcon from "@lucide/svelte/icons/search";
import type { Component } from "svelte";
import type { Api, AppSettings, SettingChange } from "../../api";
import { conditions } from "../conditions/context.svelte";
import { documents } from "../doc/document.svelte";
import { reading } from "../doc/reading.svelte";
import { t, type MessageKey } from "../i18n";
import type { ContributionOf } from "../plugins/registry.svelte";
import { layout } from "./layout.svelte";
import { offeredPanels } from "./panels.svelte";
import {
  axisOf,
  defaultArrangement,
  drawnGroups,
  FILES,
  isDefault,
  OUTLINE,
  parseArrangement,
  regionOf,
  SEARCH,
  selectedIn,
  tooShort,
  withViews,
  type Arrangement,
  type DrawnGroup,
  type Region,
  type ViewGroup,
} from "./arrangement";
import { ui } from "./ui.svelte";

type Panel = ContributionOf<"panel">;

/** How long the views' defaults wait for git to answer after a project shows. */
export const DEFAULT_WAIT_MS = 500;
/** The arrangement is written this long after the last change (a divider dragged). */
const SAVE_DELAY_MS = 400;

/** The arrangement in the settings: `panels`, else (never set | unreadable) the right column 0.11 saved. */
export function arrangementOf(settings: AppSettings): Arrangement {
  return parseArrangement(settings.panels, settings.rightViews);
}

/** A view the window knows now: a core one, or a panel its plugin offers. */
export type KnownView = {
  readonly id: string;
  /** Where it goes while no region lists it. */
  readonly region: Region;
  /** The person can close it (the outline, a panel of the right side); the others are always open in a project. */
  readonly closable: boolean;
  /** It draws a header of its own (its title, its options button): alone in its group it needs no tab strip. */
  readonly ownHeader: boolean;
  /** Its name, as a tab says it. */
  readonly label: MessageKey;
  /** A small icon: a crowded tab strip shows it alone. */
  readonly icon: Component<{ size?: number }> | undefined;
  /** The plugin's panel (`null`: a view of the core). */
  readonly panel: Panel | null;
};

/** The region is off screen: hidden by the person, or its project's session not read yet. */
function regionOff(region: Region): boolean {
  switch (region) {
    case "left":
      return layout.sidebarOff;
    case "right":
      return layout.rightOff;
    case "bottom":
      return layout.bottomOff;
    default:
      return region satisfies never;
  }
}

/** What a region reads of the window's views. */
type Views = {
  readonly layout: Arrangement;
  isPresent(id: string): boolean;
  hasStrip(group: DrawnGroup): boolean;
};

/** One region of this window: the groups it would draw, the room it measured, the groups it draws. */
class RegionViews {
  /** The region's extent along its stack as last measured (px; 0: not drawn | not measured yet). */
  size = $state(0);
  readonly #region: Region;
  readonly #views: Views;

  constructor(region: Region, views: Views) {
    this.#region = region;
    this.#views = views;
  }

  /** The groups with a present view, along the stack as arranged. */
  readonly stacked = $derived.by(() =>
    drawnGroups(this.#views.layout[this.#region], (id) => this.#views.isPresent(id)),
  );

  /** How many views of the region are present. */
  readonly count = $derived(this.stacked.reduce((sum, group) => sum + group.views.length, 0));

  /** Too short for those groups along the stack. */
  readonly short = $derived.by(() =>
    tooShort(this.size, this.stacked, axisOf(this.#region), (group) => this.#views.hasStrip(group)),
  );

  /** The groups drawn: `stacked`, or all in one tab group while the region is too short. */
  readonly drawn = $derived.by((): DrawnGroup[] => {
    const groups = this.stacked;
    if (!this.short) return groups;
    return [{ at: groups[0]?.at ?? 0, views: groups.flatMap((g) => g.views), share: 1 }];
  });
}

export class ArrangementStore {
  /** As saved (app-wide). */
  saved = $state.raw<Arrangement>(defaultArrangement());
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

  /** The panels offered now (plugin on, `offered()`), of both sides, in a fixed order. */
  readonly #panels = $derived(offeredPanels());

  /** The document shown has an outline. */
  readonly hasOutline = $derived.by(() => {
    const view = documents.view;
    return view?.body.kind === "markdown" && view.body.outline.length > 0;
  });

  /**
   * Every view the window knows now, in the order a fresh arrangement lists them: Files, Search and the plugins'
   * tabs (left), the outline and the plugins' panels (right). A panel's side is only its default region.
   */
  readonly views = $derived.by((): KnownView[] => {
    // a sidebar tab is a body alone, always open in a project; a right panel has its header and can be closed
    const panelsOf = (side: Panel["side"]): KnownView[] =>
      this.#panels
        .filter((panel) => panel.side === side)
        .map((panel) => ({
          id: panel.id,
          region: side,
          closable: side === "right",
          ownHeader: side === "right",
          label: panel.label,
          icon: panel.icon,
          panel,
        }));
    const core = { closable: false, ownHeader: false, panel: null } as const;
    return [
      { ...core, id: FILES, region: "left", label: "sidebar.files", icon: FolderTree },
      { ...core, id: SEARCH, region: "left", label: "sidebar.search", icon: SearchIcon },
      ...panelsOf("left"),
      {
        id: OUTLINE,
        region: "right",
        closable: true,
        ownHeader: true,
        label: "outline.tab",
        icon: ListTree,
        panel: null,
      },
      ...panelsOf("right"),
    ];
  });

  /** The arrangement with every view known now listed (a new plugin's view joins its default region). */
  readonly layout = $derived(withViews(this.saved, this.views));

  /** The arrangement is the default one: a reset would change nothing. */
  readonly atDefault = $derived(isDefault(this.layout, this.views));

  readonly #regions: Readonly<Record<Region, RegionViews>> = {
    left: new RegionViews("left", this),
    right: new RegionViews("right", this),
    bottom: new RegionViews("bottom", this),
  };

  /** The views present in the window, whatever their region, in the order of `views`. */
  readonly present = $derived(this.views.filter((view) => this.isPresent(view.id)));

  /** How many views are present in the window. */
  readonly presentCount = $derived(this.present.length);

  /** Follows the settings of the app (boot + every change) and decides the views' defaults per project load. */
  start(api: Api): () => void {
    this.#api = api;
    let gone = false;
    let off: (() => void) | null = null;
    void api.settingsGet().then((result) => {
      if (!gone && result.ok) this.saved = arrangementOf(result.value);
    });
    void api
      .on("settings-changed", (event) => {
        // our own write comes back: the arrangement shown is newer than a write still waiting
        if (this.#timer === null) this.saved = arrangementOf(event.settings);
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

  /** The view `id` as the window knows it (undefined: none now, e.g. its plugin is off). */
  viewOf(id: string): KnownView | undefined {
    return this.views.find((view) => view.id === id);
  }

  /** The name of the view `id`, as its tab says it (a view nobody offers now: its id). */
  nameOf(id: string): string {
    const view = this.viewOf(id);
    return view === undefined ? id : t(view.label);
  }

  /** The region that lists `id` now (`null`: none). */
  regionOf(id: string): Region | null {
    return regionOf(this.layout, id);
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

  /**
   * The view is present. One that cannot be closed (Files, Search, a plugin's sidebar tab): in a project window,
   * always. The outline: when the document has one and it is on. A panel that can be closed: when open.
   */
  isPresent(id: string): boolean {
    const view = this.viewOf(id);
    if (view === undefined) return false;
    if (!view.closable) return conditions.current.workspace === "project";
    return view.panel === null ? this.hasOutline && reading.outline : this.isOpen(view.panel);
  }

  /** The groups of `region` with a present view, along its stack as arranged. */
  stacked(region: Region): readonly DrawnGroup[] {
    return this.#regions[region].stacked;
  }

  /** `region` is too short for its groups along the stack: they are drawn as one tab group. */
  short(region: Region): boolean {
    return this.#regions[region].short;
  }

  /** The groups `region` draws: `stacked`, or all in one tab group while it is too short. */
  drawn(region: Region): readonly DrawnGroup[] {
    return this.#regions[region].drawn;
  }

  /** How many views of `region` are present. */
  presentIn(region: Region): number {
    return this.#regions[region].count;
  }

  /** A drawn group shows a tab strip: several views, or one that has no header of its own to name it. */
  hasStrip(group: DrawnGroup): boolean {
    const [only] = group.views;
    return group.views.length > 1 || (only !== undefined && this.viewOf(only)?.ownHeader === false);
  }

  /** The element of `region` measured its extent along the stack (px; 0: it is gone). */
  setSize(region: Region, px: number): void {
    this.#regions[region].size = px;
  }

  /** The drawn group that holds `id`, in whatever region (undefined: not drawn). */
  groupWith(id: string): DrawnGroup | undefined {
    const region = this.regionOf(id);
    return region === null ? undefined : this.drawn(region).find((g) => g.views.includes(id));
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

  /** The view is on screen: present, shown by its group, its region shown, not in focus mode. */
  onScreen(id: string): boolean {
    const region = this.regionOf(id);
    return region !== null && !regionOff(region) && ui.focus !== "on" && this.isSelected(id);
  }

  /** A new arrangement: drawn at once, written a moment after the last change. */
  arrange(next: Arrangement): void {
    this.saved = next;
    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#timer = setTimeout(() => this.#flush(), SAVE_DELAY_MS);
  }

  /**
   * The whole arrangement back to the default (the groups, their order and shares, in the three regions), and said.
   * The regions' sizes, the hidden regions and the open views are the project's: untouched.
   */
  reset(): void {
    this.arrange(defaultArrangement());
    this.status = t("panel.layoutReset");
  }

  #flush(): void {
    if (this.#timer === null) return;
    clearTimeout(this.#timer);
    this.#timer = null;
    // the settings field `panels`: a failed write keeps the arrangement for this run. The field 0.11 wrote
    // (`rightViews`) is left as it is: an older build on the same profile still finds its column.
    const copy = (groups: readonly ViewGroup[]): { views: string[]; share: number }[] =>
      groups.map((g) => ({ views: [...g.views], share: g.share }));
    const { left, right, bottom } = this.saved;
    const change: SettingChange = {
      key: "panels",
      value: { v: 2, left: copy(left), right: copy(right), bottom: copy(bottom) },
    };
    void this.#api?.settingsSet(change);
  }
}

export const arrangement = new ArrangementStore();
