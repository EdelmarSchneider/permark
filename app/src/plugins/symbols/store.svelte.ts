// The symbols panel's state for this window: the search and the groups the person collapsed (kept while the view
// moves between regions; not stored: a new window starts afresh), and the group to bring into view once, when the
// emoji popup handed over to the panel. The panel is offered (a view of the arrangement, a thing the right region's
// toggle can open) only once the person asked for it in this window, or while it is open for the project: a window
// that never used it shows nothing new.

import type { CharacterEntry, SymbolFavorites } from "../../api";
import {
  groupsFor,
  groupLabel,
  entryName,
  type PickerKind,
} from "../../core/editing/visual/characters";
import { apiErrorText } from "../../core/errors";
import { t } from "../../core/i18n";
import { arrangement } from "../../core/shell/arrangement.svelte";
import { layout } from "../../core/shell/layout.svelte";
import { reasonCode } from "../../core/shell/settings/problems";
import { showView, togglePanel } from "../../core/shell/sides";
import { windowStore } from "../../core/window.svelte";
import { hasChar, hasGroup, withChar, withGroup, withoutChar, withoutGroup } from "./favorites";
import { DEFAULT_COLLAPSED } from "./model";

/** The id of the symbols panel. */
export const PANEL = "symbols.panel";

export class SymbolsStore {
  query = $state("");
  /** Section keys of the groups drawn collapsed. */
  collapsed = $state.raw<ReadonlySet<string>>(DEFAULT_COLLAPSED);
  /** A group the panel scrolls to once it draws it (`null`: none asked). */
  reveal = $state<string | null>(null);
  /** The person asked for the panel in this window (the command, the popup's button). */
  called = $state(false);

  /** The panel is a view of this window now (reactive). */
  get offered(): boolean {
    return this.called || layout.views.open[PANEL] === true;
  }

  /** Expands | collapses one group. */
  setExpanded(key: string, expanded: boolean): void {
    if (expanded !== this.collapsed.has(key)) return;
    const next = new Set(this.collapsed);
    if (expanded) next.delete(key);
    else next.add(key);
    this.collapsed = next;
  }

  /** The palette command: the panel shows | closes. */
  toggle(): void {
    this.called = true;
    togglePanel(PANEL);
  }

  /** The popup's "Keep open in a panel": the panel shows (the keyboard stays in the editor); from the emoji popup
   *  its group opens and comes into view. */
  keepOpen(picker: PickerKind): void {
    this.called = true;
    if (picker === "emoji") {
      const emoji = groupsFor("emoji", windowStore.characters)[0]?.name;
      if (emoji !== undefined) {
        this.setExpanded(emoji, true);
        this.reveal = emoji;
      }
    }
    showView(PANEL);
  }

  /** Adds the character to the favorites, or removes it when it is one; said in the status line. */
  toggleChar(entry: CharacterEntry): Promise<void> {
    return this.#step((current) => {
      const had = hasChar(current, entry.char);
      const name = entryName(entry);
      return {
        next: had ? withoutChar(current, entry.char) : withChar(current, entry.char),
        done: t(had ? "symbols.removedFavorite" : "symbols.addedFavorite", { name }),
      };
    });
  }

  /** Pins the group (by its name in the groups file), or unpins it; said in the status line. */
  toggleGroup(name: string): Promise<void> {
    return this.#step((current) => {
      const had = hasGroup(current, name);
      const group = groupLabel(name);
      return {
        next: had ? withoutGroup(current, name) : withGroup(current, name),
        done: t(had ? "symbols.removedGroup" : "symbols.addedGroup", { group }),
      };
    });
  }

  /** The writes in flight, one after the other. */
  #queue: Promise<void> = Promise.resolve();
  /** The last write that landed: the window's settings read count when it started, and the value it wrote. */
  #wrote: { reads: number; value: SymbolFavorites | null } | null = null;

  /** The value a step starts from: what the last write wrote while no settings news came since it started (its
   *  own `settings-changed` is still on the way), else the window's (that news, or another window's change). */
  #latest(): SymbolFavorites | null {
    const wrote = this.#wrote;
    return wrote !== null && windowStore.settingsReads === wrote.reads
      ? wrote.value
      : windowStore.symbolFavorites;
  }

  /** Queues one change: each step reads what the one before it wrote, so quick toggles never lose one. */
  #step(
    change: (current: SymbolFavorites | null) => { next: SymbolFavorites | null; done: string },
  ): Promise<void> {
    const run = this.#queue.then(async () => {
      const current = this.#latest();
      const { next, done } = change(current);
      await this.#write(current, next, done);
    });
    this.#queue = run.catch(() => {});
    return run;
  }

  /** One settings write; the window's value follows from `settings-changed`. A step that changed nothing was one
   *  the settings would refuse (a toggle never repeats an add). */
  async #write(
    current: SymbolFavorites | null,
    next: SymbolFavorites | null,
    done: string,
  ): Promise<void> {
    if (next === current) {
      arrangement.say(t("symbols.favoriteRefused"));
      return;
    }
    const reads = windowStore.settingsReads;
    const refused = await windowStore.trySetting({ key: "symbolFavorites", value: next });
    if (refused === null) {
      this.#wrote = { reads, value: next };
      arrangement.say(done);
    } else if (reasonCode(refused) === "symbolFavorites")
      arrangement.say(t("symbols.favoriteRefused"));
    else arrangement.say(t("settings.notSaved", { message: apiErrorText(refused) }));
  }

  /** The plugin is switched off: nothing of this window's panel state is left. */
  reset(): void {
    this.query = "";
    this.collapsed = DEFAULT_COLLAPSED;
    this.reveal = null;
    this.called = false;
    this.#wrote = null;
  }
}

export const symbols = new SymbolsStore();
