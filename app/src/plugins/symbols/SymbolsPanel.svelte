<script lang="ts">
  // The view "Symbols": the favorites, then the groups of the settings payload (symbols, then emoji), searchable like
  // the toolbar's pickers. A click | Enter on a character puts it in at the caret of the open editor: a click leaves
  // the keyboard in the editor (the press never takes it), a key leaves it here. One tab stop for the whole list
  // (the stop roves over the headers and the cells); the keys follow the grid as drawn. A right click | Shift+F10 |
  // the Menu key on a cell or a group header opens the favorites menu (the view's own options stay on its header);
  // the star on a group header pins | unpins the group by pointer.
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Star from "@lucide/svelte/icons/star";
  import { onMount, tick, type Snippet } from "svelte";
  import type { CharacterEntry } from "../../api";
  import { documents } from "../../core/doc/document.svelte";
  import { editingNow } from "../../core/editing/choose";
  import { editing } from "../../core/editing/session.svelte";
  import {
    cellLabel,
    cellName,
    codePoints,
    entryName,
    groupLabel,
    isBlankQuery,
    isInvisible,
  } from "../../core/editing/visual/characters";
  import ContextMenu from "../../core/explorer/ContextMenu.svelte";
  import type { MenuEntry } from "../../core/explorer/menu";
  import { t } from "../../core/i18n";
  import { arrangement } from "../../core/shell/arrangement.svelte";
  import { adrift, closeView, focusPage } from "../../core/shell/sides";
  import { windowStore } from "../../core/window.svelte";
  import { hasChar, hasGroup } from "./favorites";
  import { cellKey, headerKey, keptStop, moveFrom, relatedStop, stopsOf, type Stop } from "./keys";
  import { canInsert, sectionsOf, type FavoritesSection, type GroupSection } from "./model";
  import { PANEL, symbols } from "./store.svelte";

  type Props = {
    /** The view options button for this header (given while the panel is alone in its group). */
    viewMenu?: Snippet;
  };
  let { viewMenu }: Props = $props();

  const uid = $props.id();
  /** The note under the search (why a click inserts nothing now): each cell points at it while it shows. */
  const noteId = `${uid}-note`;

  /** How long a cell glows after it put its character in. */
  const FLASH_MS = 400;

  const sections = $derived(
    sectionsOf(
      windowStore.characters,
      windowStore.symbolFavorites,
      symbols.query,
      symbols.collapsed,
    ),
  );
  const stops = $derived(stopsOf(sections));
  const querying = $derived(!isBlankQuery(symbols.query));
  /** A query nothing answers (the favorites section is left out then too). */
  const nothing = $derived(querying && !sections.some((s) => s.kind !== "divider"));
  /** The stop the person moved to last (and its character, for a cell); `active` is the one that holds the tab stop
   *  as the list stands. */
  let wanted = $state<string | null>(null);
  let wantedChar = $state<string | null>(null);
  const active = $derived(keptStop(stops, wanted, wantedChar));
  const insertable = $derived(
    canInsert(editingNow() ? editing.mode : "view", documents.path !== null),
  );
  /** What the peek line shows: the cell under the pointer, else the one with the focus. */
  let hovered = $state.raw<CharacterEntry | null>(null);
  let focused = $state.raw<CharacterEntry | null>(null);
  const peek = $derived(hovered ?? focused);
  let flashed = $state<string | null>(null);
  let flashTimer: ReturnType<typeof setTimeout> | null = null;

  let list = $state<HTMLElement>();
  let search = $state<HTMLInputElement>();

  // edits to the groups file show when the panel opens, without a restart
  onMount(() => {
    windowStore.refreshCharacters();
    return () => {
      if (flashTimer !== null) clearTimeout(flashTimer);
    };
  });

  /** The element with the keyboard went away | moved in a redraw: the stop related to the one wanted takes it (the
   *  same character in its own group, the same header), else the favorites title (the last favorite went), else the
   *  first stop; never the page body. */
  function refocus(): void {
    if (list === undefined) return;
    const related = relatedStop(stops, wanted, wantedChar);
    const target =
      (related !== null ? elementOf(related) : null) ??
      list.querySelector<HTMLElement>("[data-favorites-title]") ??
      (active !== null ? elementOf(active) : null);
    target?.focus();
  }

  // a redraw (the settings changed, a group closed) under the keyboard
  $effect.pre(() => {
    void stops;
    if (list === undefined || !list.contains(document.activeElement)) return;
    void tick().then(() => {
      if (list !== undefined && !list.contains(document.activeElement)) refocus();
    });
  });

  /** A favorite change from the menu: the menu gives the keyboard back to the element it was for, which the change
   *  may take away (the last favorite, a group moving down); then the stop related to it takes the keyboard. */
  async function favorite(write: () => Promise<void>): Promise<void> {
    await write();
    await tick();
    if (adrift()) refocus();
  }

  // opened from the emoji popup: its group comes into view once drawn
  $effect(() => {
    const name = symbols.reveal;
    if (name === null || list === undefined) return;
    void tick().then(() => {
      elementOf(headerKey(name))?.scrollIntoView({ block: "start" });
      symbols.reveal = null;
    });
  });

  function elementOf(key: string): HTMLElement | null {
    for (const el of list?.querySelectorAll<HTMLElement>("[data-stop]") ?? []) {
      if (el.dataset.stop === key) return el;
    }
    return null;
  }

  /** `key` becomes the stop the person wants. */
  function want(key: string, char: string | null): void {
    wanted = key;
    wantedChar = char;
  }

  async function focusStop(key: string | null): Promise<void> {
    if (key === null) return;
    const stop = stops.find((s) => s.key === key);
    want(key, stop?.kind === "cell" ? stop.char : null);
    await tick();
    const el = elementOf(key);
    el?.focus();
    el?.scrollIntoView({ block: "nearest" });
  }

  function insert(entry: CharacterEntry, key: string): void {
    want(key, entry.char);
    if (insertable !== "ok" || !editing.insert(entry.char)) return;
    flashed = key;
    if (flashTimer !== null) clearTimeout(flashTimer);
    flashTimer = setTimeout(() => (flashed = null), FLASH_MS);
    arrangement.say(t("symbols.inserted", { name: entryName(entry) }));
  }

  function toggle(section: GroupSection): void {
    want(headerKey(section.key), null);
    symbols.setExpanded(section.key, !section.expanded);
  }

  /** The favorites menu open now (`null`: none); `label` names what it is for. */
  let menu = $state.raw<{ x: number; y: number; label: string; entries: MenuEntry[] } | null>(null);

  /** What a cell's | header's menu is for: the character's name | the group's label. */
  type MenuFor = { label: string; entries: () => MenuEntry[] };

  function cellMenu(entry: CharacterEntry, section: FavoritesSection | GroupSection): MenuFor {
    return { label: entryName(entry), entries: () => cellEntries(entry, section) };
  }

  function headerMenu(section: GroupSection): MenuFor {
    return { label: groupLabel(section.name), entries: () => [groupItem(section.name)] };
  }

  function groupItem(name: string): MenuEntry {
    const group = groupLabel(name);
    const pinned = hasGroup(windowStore.symbolFavorites, name);
    return {
      kind: "item",
      label: t(pinned ? "symbols.removeGroup" : "symbols.addGroup", { group }),
      run: () => void favorite(() => symbols.toggleGroup(name)),
    };
  }

  /** A cell's menu: the character; in a group (not the favorites section), the group too. */
  function cellEntries(
    entry: CharacterEntry,
    section: FavoritesSection | GroupSection,
  ): MenuEntry[] {
    const kept = hasChar(windowStore.symbolFavorites, entry.char);
    const char: MenuEntry = {
      kind: "item",
      label: t(kept ? "symbols.removeFavorite" : "symbols.addFavorite"),
      run: () => void favorite(() => symbols.toggleChar(entry)),
    };
    return section.kind === "group"
      ? [char, { kind: "separator" }, groupItem(section.name)]
      : [char];
  }

  function openMenu(x: number, y: number, target: MenuFor): void {
    menu = { x, y, label: t("symbols.menuFor", { name: target.label }), entries: target.entries() };
  }

  /** Right click: the menu at the pointer. On a group header the header takes the keyboard first (the press never
   *  did), so closing the menu leaves the keyboard on what it was for. */
  function pointerMenu(event: MouseEvent, target: MenuFor, focus = false): void {
    event.preventDefault();
    if (focus && event.currentTarget instanceof HTMLElement) event.currentTarget.focus();
    openMenu(event.clientX, event.clientY, target);
  }

  /** Shift+F10 | the Menu key: the menu under the element with the keyboard (which gets it back on close). Taken
   *  here, so the view region never sees it as a call for the view's options. */
  function keyMenu(event: KeyboardEvent, target: MenuFor): void {
    if (!((event.key === "F10" && event.shiftKey) || event.key === "ContextMenu")) return;
    if (!(event.currentTarget instanceof HTMLElement)) return;
    event.preventDefault();
    event.stopPropagation();
    const box = event.currentTarget.getBoundingClientRect();
    openMenu(box.left, box.bottom + 2, target);
  }

  /** A press on a cell | header never takes the keyboard: the editor keeps its caret, selection and IME. */
  function keepEditor(event: MouseEvent): void {
    if (event.target instanceof Element && event.target.closest("button") !== null) {
      event.preventDefault();
    }
  }

  /** The cell straight above | below `stop` in the drawn grid (nearest by x); `null`: it is on the first | last row. */
  function verticalOf(stop: Stop, down: boolean): string | null {
    const here = elementOf(stop.key);
    if (here === null || list === undefined) return null;
    const at = here.getBoundingClientRect();
    const middle = at.left + at.width / 2;
    let best: { key: string; top: number; dx: number } | null = null;
    for (const el of list.querySelectorAll<HTMLElement>("[data-stop]")) {
      if (el.dataset.section !== stop.section || el.dataset.kind !== "cell") continue;
      const box = el.getBoundingClientRect();
      const beyond = down ? box.top > at.top + 1 : box.top < at.top - 1;
      if (!beyond) continue;
      const dx = Math.abs(box.left + box.width / 2 - middle);
      const closer =
        best === null ||
        (down ? box.top < best.top - 1 : box.top > best.top + 1) ||
        (Math.abs(box.top - best.top) <= 1 && dx < best.dx);
      if (closer) best = { key: el.dataset.stop ?? "", top: box.top, dx };
    }
    return best?.key ?? null;
  }

  /** Up | Down from `stop`: the next row of its grid; past the first row its header, past the last the next stop. */
  function vertical(stop: Stop, down: boolean): string | null {
    if (stop.kind === "header") return moveFrom(stops, stop.key, down ? "next" : "previous");
    const row = verticalOf(stop, down);
    if (row !== null) return row;
    if (!down) {
      const header = stops.find((s) => s.kind === "header" && s.section === stop.section);
      return header?.key ?? null;
    }
    const last = moveFrom(stops, stop.key, "groupEnd") ?? stop.key;
    return moveFrom(stops, last, "next");
  }

  function listKey(event: KeyboardEvent): void {
    if (event.isComposing) return;
    const stop = stops.find((s) => s.key === active);
    if (event.key === "Escape") {
      event.preventDefault();
      focusPage();
      return;
    }
    // a letter typed in the list goes to the search
    const plain = !event.ctrlKey && !event.metaKey && !event.altKey;
    if (plain && [...event.key].length === 1 && event.key !== " ") {
      event.preventDefault();
      symbols.query += event.key;
      search?.focus();
      return;
    }
    if (stop === undefined) return;
    const section = sections.find((s) => s.key === stop.section);
    const header = stop.kind === "header" && section?.kind === "group" ? section : null;
    let next: string | null | undefined;
    switch (event.key) {
      case "ArrowLeft":
        if (header?.expanded && !querying) symbols.setExpanded(header.key, false);
        else next = moveFrom(stops, stop.key, "previous");
        break;
      case "ArrowRight":
        if (header !== null && !header.expanded) symbols.setExpanded(header.key, true);
        else next = moveFrom(stops, stop.key, "next");
        break;
      case "ArrowUp":
        next = vertical(stop, false);
        break;
      case "ArrowDown":
        next = vertical(stop, true);
        break;
      case "Home":
        next = moveFrom(
          stops,
          stop.key,
          event.ctrlKey || event.metaKey ? "listStart" : "groupStart",
        );
        break;
      case "End":
        next = moveFrom(stops, stop.key, event.ctrlKey || event.metaKey ? "listEnd" : "groupEnd");
        break;
      case "PageUp":
        next = moveFrom(stops, stop.key, "previousHeader");
        break;
      case "PageDown":
        next = moveFrom(stops, stop.key, "nextHeader");
        break;
      default:
        return;
    }
    event.preventDefault();
    if (next !== undefined) void focusStop(next);
  }

  function searchKey(event: KeyboardEvent): void {
    if (event.isComposing) return;
    if (event.key === "Escape") {
      event.preventDefault();
      if (symbols.query !== "") symbols.query = "";
      else focusPage();
    } else if (event.key === "ArrowDown") {
      event.preventDefault();
      void focusStop(active);
    }
  }

  const noteKey = $derived(
    insertable === "view"
      ? "symbols.insertView"
      : insertable === "none"
        ? "symbols.insertNone"
        : null,
  );
</script>

{#snippet peekLine()}
  {#if peek !== null}
    <span class="peek-glyph" class:invisible={isInvisible(peek.char)}>{cellLabel(peek)}</span>
    <span class="peek-name">{entryName(peek)}</span>
    <span class="peek-code">{codePoints(peek.char)}</span>
  {/if}
{/snippet}

{#snippet cells(section: FavoritesSection | GroupSection, labelledBy: string)}
  <div class="grid" role="group" aria-labelledby={labelledBy}>
    {#each section.entries as entry, index (index)}
      {@const key = cellKey(section.key, index)}
      {@const name = entryName(entry)}
      <button
        type="button"
        class="cell"
        class:invisible={isInvisible(entry.char)}
        class:flash={flashed === key}
        tabindex={key === active ? 0 : -1}
        data-stop={key}
        data-kind="cell"
        data-section={section.key}
        aria-label={cellName(entry)}
        aria-disabled={insertable === "ok" ? undefined : "true"}
        aria-describedby={noteKey === null ? undefined : noteId}
        title={t("picker.tip", { name, code: codePoints(entry.char) })}
        onclick={() => insert(entry, key)}
        oncontextmenu={(event) => pointerMenu(event, cellMenu(entry, section))}
        onkeydown={(event) => keyMenu(event, cellMenu(entry, section))}
        onfocus={() => {
          want(key, entry.char);
          focused = entry;
        }}
        onblur={() => (focused = null)}
        onmouseenter={() => (hovered = entry)}
        onmouseleave={() => (hovered = null)}>{cellLabel(entry)}</button
      >
    {/each}
  </div>
{/snippet}

<section class="symbols" aria-label={t("symbols.label")}>
  <!-- narrow, the view's buttons sit on the title's line and the search on the next: they come first in the DOM so
       the tab order follows the lines; wide (the bottom region) the CSS order puts the search beside the title -->
  <header data-view-header>
    <h2 title={t("symbols.label")}>{t("symbols.label")}</h2>
    <span class="spacer"></span>
    <div class="tools">
      {@render viewMenu?.()}
      <button
        type="button"
        class="icon close"
        aria-label={t("symbols.close")}
        title={t("symbols.close")}
        onclick={() => closeView(PANEL)}>×</button
      >
    </div>
    <input
      bind:this={search}
      class="search"
      type="search"
      placeholder={t("picker.search")}
      aria-label={t("picker.search")}
      bind:value={symbols.query}
      onkeydown={searchKey}
    />
    <p class="peek head-peek" aria-hidden="true">{@render peekLine()}</p>
  </header>
  {#if noteKey !== null}
    <p class="note" id={noteId}>{t(noteKey)}</p>
  {/if}
  <!-- the keys of the list (arrows, Home | End, a letter to the search) are on the group that holds the cells -->
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    bind:this={list}
    class="list"
    role="group"
    aria-label={t("symbols.list")}
    onmousedown={keepEditor}
    onkeydown={listKey}
  >
    {#each sections as section, at (section.key)}
      {@const id = `${uid}-${at}`}
      {#if section.kind === "divider"}
        <hr class="divider" />
      {:else if section.kind === "favorites"}
        <div class="section">
          <!-- reached by script only: the keyboard lands here when the last favorite it was on went away -->
          <h3 class="title fav-title" {id} tabindex="-1" data-favorites-title>
            {t("symbols.favorites")}
          </h3>
          {#if section.entries.length === 0}
            <p class="hint">{t("symbols.favoritesEmpty")}</p>
          {:else}
            {@render cells(section, id)}
          {/if}
        </div>
      {:else}
        {@const key = headerKey(section.key)}
        {@const group = groupLabel(section.name)}
        <div class="section">
          <div class="head-row">
            <button
              type="button"
              class="head"
              {id}
              aria-expanded={section.expanded}
              tabindex={key === active ? 0 : -1}
              data-stop={key}
              data-kind="header"
              data-section={section.key}
              onclick={() => toggle(section)}
              onfocus={() => want(key, null)}
              oncontextmenu={(event) => pointerMenu(event, headerMenu(section), true)}
              onkeydown={(event) => keyMenu(event, headerMenu(section))}
            >
              <ChevronRight size={14} class="chevron" aria-hidden="true" />
              <span class="title">{group}</span>
              <span class="count">{section.total}</span>
            </button>
            <!-- by pointer only: the keyboard reaches the same choice through the header's menu -->
            <button
              type="button"
              class="icon star"
              tabindex="-1"
              aria-pressed={section.pinned}
              aria-label={t("symbols.keepGroup", { group })}
              title={t("symbols.keepGroup", { group })}
              onclick={() => void favorite(() => symbols.toggleGroup(section.name))}
            >
              <Star size={14} aria-hidden="true" fill={section.pinned ? "currentColor" : "none"} />
            </button>
          </div>
          {#if section.expanded}
            {@render cells(section, id)}
          {/if}
        </div>
      {/if}
    {/each}
    {#if nothing}
      <p class="hint">{t("picker.empty")}</p>
    {/if}
  </div>
  <p class="peek foot-peek" aria-hidden="true">{@render peekLine()}</p>
  {#if menu !== null}
    <!-- outside the list: the menu's own keys never reach the list's -->
    {#key menu}
      <ContextMenu
        x={menu.x}
        y={menu.y}
        entries={menu.entries}
        label={menu.label}
        close={() => (menu = null)}
      />
    {/key}
  {/if}
</section>

<style>
  .symbols {
    container-type: inline-size;
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
  }
  header {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-1) var(--sp-2);
    padding: var(--sp-2) var(--sp-3);
    border-bottom: 1px solid var(--border);
  }
  header > :global(*) {
    flex: none;
  }
  /* same title style as the other panels of the column */
  h2 {
    flex: 0 1 auto;
    min-width: 0;
    margin: 0;
    overflow: hidden;
    color: var(--muted);
    font-size: var(--fs-ui-xs);
    font-weight: 600;
    letter-spacing: 0.05em;
    text-overflow: ellipsis;
    text-transform: uppercase;
    white-space: nowrap;
  }
  header > .spacer {
    flex: 1 1 0;
  }
  .tools {
    display: flex;
    align-items: center;
    gap: var(--sp-1);
  }
  /* narrow: the search on a line of its own under the title */
  header > .search {
    flex: 1 1 100%;
    min-width: 0;
    height: 2rem;
    padding: 0 var(--sp-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-ui-sm);
  }
  .icon {
    display: inline-grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .icon:hover {
    background: var(--hover);
    color: var(--text);
  }
  .close {
    font-size: 16px;
    line-height: 1;
  }
  .note {
    margin: 0;
    padding: var(--sp-2) var(--sp-3) 0;
    color: var(--muted);
    font-size: var(--fs-ui-xs);
  }
  .list {
    flex: 1;
    min-height: 0;
    padding: var(--sp-2) var(--sp-3);
    overflow-y: auto;
  }
  .section {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    margin-bottom: var(--sp-2);
  }
  .title {
    overflow: hidden;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .fav-title {
    margin: 0;
    padding: 2px 0;
  }
  .head-row {
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    min-width: 0;
  }
  .head {
    display: flex;
    flex: 1 1 auto;
    align-items: center;
    gap: var(--sp-1);
    min-width: 0;
    padding: 2px var(--sp-1) 2px 0;
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .head:hover {
    background: var(--hover);
  }
  .head :global(.chevron) {
    flex: none;
    color: var(--muted);
    transition: transform var(--dur-fast) var(--ease);
  }
  .head[aria-expanded="true"] :global(.chevron) {
    transform: rotate(90deg);
  }
  /* the star shows on the header's hover | focus, and always on a pinned group */
  .star {
    flex: none;
    opacity: 0;
  }
  .head-row:hover .star,
  .head-row:focus-within .star,
  .star[aria-pressed="true"] {
    opacity: 1;
  }
  .star[aria-pressed="true"] {
    color: var(--accent);
  }
  .count {
    flex: none;
    color: var(--muted);
    font-size: var(--fs-ui-xs);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(2rem, 1fr));
    gap: var(--sp-1);
  }
  .cell {
    height: 2rem;
    padding: 0;
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--text);
    font-size: 1.125rem;
    line-height: 1;
    cursor: pointer;
    transition: background var(--dur-fast) var(--ease);
  }
  .cell:hover {
    background: var(--hover);
  }
  .cell.flash {
    background: var(--accent-soft);
  }
  /* nothing to put in where the keyboard would go: the glyphs dim, a click does nothing */
  .cell[aria-disabled="true"] {
    cursor: default;
    opacity: 0.5;
  }
  /* an invisible character draws as a small name in a dotted box, so the cell does not look empty */
  .invisible {
    border: 1px dotted var(--muted);
    color: var(--muted);
    font-size: calc(var(--fs-ui-xs) - 1px);
    letter-spacing: -0.04em;
    overflow: hidden;
  }
  .hint {
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-ui-xs);
  }
  .divider {
    margin: var(--sp-1) 0 var(--sp-3);
    border: 0;
    border-top: 1px solid var(--border);
  }
  .peek {
    display: flex;
    align-items: baseline;
    gap: var(--sp-2);
    min-width: 0;
    margin: 0;
    overflow: hidden;
    color: var(--muted);
    font-size: var(--fs-ui-xs);
    white-space: nowrap;
  }
  .foot-peek {
    min-height: calc(var(--fs-ui-xs) * 1.6 + 2 * var(--sp-1));
    padding: var(--sp-1) var(--sp-3);
    border-top: 1px solid var(--border);
  }
  .head-peek {
    display: none;
  }
  .peek-glyph {
    color: var(--text);
    font-size: var(--fs-ui);
  }
  .peek-glyph.invisible {
    padding: 0 2px;
    font-size: var(--fs-ui-xs);
  }
  .peek-name {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .peek-code {
    flex: none;
    color: var(--muted);
  }
  @media (prefers-reduced-motion: reduce) {
    .cell,
    .head :global(.chevron) {
      transition: none;
    }
    .cell.flash {
      background: transparent;
      outline: 2px solid var(--accent);
      outline-offset: -2px;
    }
  }
  /* wide (the bottom region): the search and the peek line in the header row, each group a row [title | grid] */
  @container (min-width: 520px) {
    header > .search {
      order: 1;
      flex: 0 1 16rem;
    }
    header > .head-peek {
      order: 2;
      display: flex;
      flex: 1 1 0;
    }
    header > .tools {
      order: 3;
    }
    header > .spacer,
    .foot-peek {
      display: none;
    }
    .section {
      display: grid;
      grid-template-columns: 8rem minmax(0, 1fr);
      align-items: start;
      column-gap: var(--sp-3);
    }
    .section > .grid,
    .section > .hint {
      grid-column: 2;
    }
    .section > .hint {
      align-self: center;
    }
  }
</style>
