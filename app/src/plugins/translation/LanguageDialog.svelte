<script lang="ts">
  // The language dialog: the language a text is translated into (To) and the one it is in (From: read off the text
  // unless named; a named one is for the run that starts next alone, and shows here again while it waits for that
  // run), each picked from one searchable list. The two picks are a draft: taken only on the confirm; Esc | Cancel
  // leave everything as it was. Keys: Up | Down | PageUp | PageDown move through the list from the field and from
  // the list (there Home | End too), Enter picks the row and, on the row that is the pick already, confirms (a new
  // press: never a key held down), Ctrl+Enter confirms.
  import Check from "@lucide/svelte/icons/check";
  import { isComposition } from "../../core/commands/combo";
  import { language, t, tn, type MessageKey } from "../../core/i18n";
  import { moveActive } from "../../core/search/finderModel";
  import { markRuns } from "../../core/search/fuzzy";
  import Marked from "../../core/search/Marked.svelte";
  import { dialogFocus } from "../../core/shell/dialogFocus";
  import {
    groupsOf,
    listed,
    type Group,
    type LanguageDialogStore,
    type Row,
    type Side,
  } from "./dialog.svelte";
  import { sameTag } from "./languages";

  type Props = { dialog: LanguageDialogStore };
  let { dialog }: Props = $props();

  /** Prefix of the ids the parts name each other by. */
  const ID = "translation-language";
  const SIDES: readonly Side[] = ["to", "from"];
  const GROUP_LABEL: Record<NonNullable<Group["title"]>, MessageKey> = {
    recent: "translation.language.recent",
    all: "translation.language.all",
  };
  /** Rows PageUp | PageDown move by. */
  const PAGE = 10;
  /** The count is said once the list stood still this long, ms (a screen reader is not told at every key). */
  const SAY_AFTER_MS = 300;

  const request = $derived(dialog.state.status === "open" ? dialog.state : null);

  // one draft per open (the host makes this component anew for every open)
  // svelte-ignore state_referenced_locally
  let target = $state(dialog.languages.target);
  /** The language named for the next run that no run took yet; else "detect". */
  // svelte-ignore state_referenced_locally
  let source = $state(dialog.languages.source);
  let side = $state<Side>("to");
  let query = $state("");
  /** What the live line says: the count of the list as it last settled. */
  let said = $state("");
  let input: HTMLInputElement | undefined = $state();

  const list = $derived(listed(language.locale));
  const groups = $derived(
    groupsOf({
      side,
      query,
      list,
      recent: dialog.languages.recent,
      target,
      detect: t("translation.language.detect"),
    }),
  );
  const rows = $derived(groups.flatMap((group) => group.rows));
  /** Languages listed (a recently used one counts once). */
  const count = $derived(groups.find((group) => group.title === "all")?.rows.length ?? 0);

  /** The row is the pick of the side shown. */
  function isChosen(row: Row): boolean {
    const chosen = side === "to" ? target : source;
    return row.kind === "detect"
      ? chosen === null
      : chosen !== null && sameTag(row.language.tag, chosen);
  }

  /** Where the keyboard starts in an untyped list: on the pick, else on the first row. */
  function start(): number {
    return Math.max(rows.findIndex(isChosen), 0);
  }

  /** Index in `rows` of the row the keyboard is on. */
  let active = $state(start());
  const current = $derived(rows[active] ?? null);

  const rowId = (row: Row): string => `${ID}-${row.key}`;
  const nameOf = (tag: string): string =>
    list.find((entry) => sameTag(entry.tag, tag))?.name ?? tag;
  const labels = $derived<Record<Side, string>>({
    to: t("translation.language.to", {
      language: target === null ? t("translation.language.choose") : nameOf(target),
    }),
    from: t("translation.language.from", {
      language: source === null ? t("translation.language.detect") : nameOf(source),
    }),
  });
  const note = $derived.by(() => {
    const given = request?.note ?? null;
    if (typeof given !== "function") return given;
    return target === null ? null : given(target);
  });

  let saying: ReturnType<typeof setTimeout> | undefined;
  $effect(() => () => clearTimeout(saying));

  function listChanged(): void {
    clearTimeout(saying);
    saying = setTimeout(() => {
      said = tn("translation.language.count", count);
    }, SAY_AFTER_MS);
  }

  function show(next: Side): void {
    if (next === side) return;
    side = next;
    // the text typed was a search of the other side's list
    query = "";
    active = start();
    listChanged();
  }

  function search(text: string): void {
    query = text;
    active = text.trim() === "" ? start() : 0;
    listChanged();
  }

  function pick(row: Row | null): void {
    if (row === null) return;
    if (row.kind === "detect") {
      source = null;
    } else if (row.off) {
      return;
    } else if (side === "from") {
      source = row.language.tag;
    } else {
      target = row.language.tag;
      // a text is not translated into its own language
      if (source !== null && sameTag(source, target)) source = null;
    }
  }

  function confirm(): void {
    if (target !== null) dialog.confirm(target, source);
  }

  function hover(row: Row): void {
    const at = rows.findIndex((other) => other.key === row.key);
    if (at >= 0 && at !== active) active = at;
  }

  // keys of the field and the list
  function onListKey(event: KeyboardEvent): void {
    if (isComposition(event) || event.altKey || event.ctrlKey || event.metaKey) return;
    const typing = event.target === input;
    const last = rows.length - 1;
    let to: number;
    switch (event.key) {
      case "ArrowDown":
        to = moveActive(active, 1, rows.length);
        break;
      case "ArrowUp":
        to = moveActive(active, -1, rows.length);
        break;
      case "PageDown":
        to = Math.min(active + PAGE, last);
        break;
      case "PageUp":
        to = Math.max(active - PAGE, 0);
        break;
      case "Home":
      case "End":
        // in the field they move through the text
        if (typing) return;
        to = event.key === "Home" ? 0 : last;
        break;
      case "Enter":
      case " ":
        if (typing && event.key === " ") return;
        event.preventDefault();
        // the row is the pick already: nothing left to pick, so Enter says "this one" (nothing without a language
        // to translate into)
        if (event.key === "Enter" && !event.repeat && current !== null && isChosen(current))
          confirm();
        else pick(current);
        return;
      default:
        return;
    }
    event.preventDefault();
    active = Math.max(to, 0);
  }

  // keys of a tab: the arrows go to the other one, which shows its list at once
  function onTabKey(event: KeyboardEvent): void {
    if (isComposition(event) || event.altKey || event.ctrlKey || event.metaKey) return;
    let next: Side;
    switch (event.key) {
      case "ArrowRight":
      case "ArrowLeft":
        next = side === "to" ? "from" : "to";
        break;
      case "Home":
        next = "to";
        break;
      case "End":
        next = "from";
        break;
      default:
        return;
    }
    event.preventDefault();
    show(next);
    document.getElementById(`${ID}-tab-${next}`)?.focus();
  }

  function onkeydown(event: KeyboardEvent): void {
    if (isComposition(event) || event.key !== "Enter" || !(event.ctrlKey || event.metaKey)) return;
    event.preventDefault();
    confirm();
  }

  // the row the keyboard is on stays in view
  $effect(() => {
    if (current !== null) {
      document.getElementById(rowId(current))?.scrollIntoView({ block: "nearest" });
    }
  });
</script>

{#snippet options(items: readonly Row[])}
  {#each items as row (row.key)}
    {@const chosen = isChosen(row)}
    {@const off = row.kind === "language" && row.off}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <div
      id={rowId(row)}
      class="row"
      class:active={current?.key === row.key}
      class:dim={off}
      role="option"
      tabindex="-1"
      aria-selected={chosen}
      aria-disabled={off ? "true" : undefined}
      title={row.kind === "detect"
        ? undefined
        : row.language.own === null
          ? row.language.name
          : `${row.language.name} · ${row.language.own}`}
      onpointermove={() => hover(row)}
      onclick={() => pick(row)}
    >
      <span class="text">
        {#if row.kind === "detect"}
          <span class="strong" data-name>{t("translation.language.detect")}</span>
        {:else}
          <span class="strong" data-name dir="auto"
            ><Marked runs={markRuns(row.language.name, row.nameMarks)} /></span
          >
          {#if row.language.own !== null}
            {" "}<span class="faint" lang={row.language.tag} dir="auto"
              ><Marked runs={markRuns(row.language.own, row.ownMarks)} /></span
            >
          {/if}
        {/if}
      </span>
      {#if chosen}<Check size={15} aria-hidden="true" />{/if}
    </div>
  {/each}
{/snippet}

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div
  class="backdrop"
  onclick={(e) => {
    if (e.target === e.currentTarget) dialog.close();
  }}
>
  <div
    class="box"
    role="dialog"
    aria-modal="true"
    aria-labelledby="{ID}-title"
    aria-describedby="{ID}-about"
    tabindex="-1"
    use:dialogFocus={{ initial: "input", onEscape: () => dialog.close() }}
    {onkeydown}
  >
    <h2 id="{ID}-title">{t("translation.language.title")}</h2>
    <div class="scopes" role="tablist" aria-label={t("translation.language.list")}>
      {#each SIDES as each (each)}
        <button
          type="button"
          role="tab"
          id="{ID}-tab-{each}"
          class="scope"
          aria-selected={side === each}
          aria-controls="{ID}-panel"
          tabindex={side === each ? 0 : -1}
          title={labels[each]}
          onclick={() => {
            show(each);
            input?.focus();
          }}
          onkeydown={onTabKey}>{labels[each]}</button
        >
      {/each}
    </div>
    <div class="panel" role="tabpanel" id="{ID}-panel" aria-labelledby="{ID}-tab-{side}">
      <input
        bind:this={input}
        class="field"
        type="text"
        role="combobox"
        aria-expanded="true"
        aria-controls="{ID}-list"
        aria-activedescendant={current ? rowId(current) : undefined}
        aria-label={t("translation.language.search")}
        autocomplete="off"
        autocapitalize="off"
        spellcheck="false"
        placeholder={t("translation.language.search")}
        value={query}
        oninput={(e) => search(e.currentTarget.value)}
        onkeydown={onListKey}
      />
      <div
        class="list"
        id="{ID}-list"
        role="listbox"
        aria-label={t("translation.language.list")}
        aria-activedescendant={current ? rowId(current) : undefined}
        tabindex="0"
        onkeydown={onListKey}
      >
        {#each groups as group (group.title)}
          {#if group.title === null}
            {@render options(group.rows)}
          {:else}
            <div role="group" aria-labelledby="{ID}-group-{group.title}">
              <div class="group" id="{ID}-group-{group.title}" role="presentation">
                {t(GROUP_LABEL[group.title])}
              </div>
              {@render options(group.rows)}
            </div>
          {/if}
        {/each}
      </div>
      {#if rows.length === 0}
        <p class="none">{t("translation.language.none", { query: query.trim() })}</p>
      {/if}
    </div>
    <p class="sr-only" role="status" aria-live="polite">{said}</p>

    <footer class="foot">
      <div class="words" id="{ID}-about">
        <p class="quality">{t("translation.language.quality")}</p>
        {#if note !== null}<p class="note">{note}</p>{/if}
      </div>
      <div class="buttons">
        <button
          type="button"
          class="btn btn--sm"
          data-action="cancel"
          onclick={() => dialog.close()}
        >
          {t("common.cancel")}
        </button>
        <button
          type="button"
          class="btn btn--sm btn--primary"
          data-action="confirm"
          disabled={target === null}
          onclick={confirm}
        >
          {t(request?.confirm ?? "translation.language.save")}
        </button>
      </div>
    </footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    padding: 10vh var(--sp-4) var(--sp-4);
    background: var(--backdrop);
    animation: pm-fade var(--dur) var(--ease);
  }
  .box {
    display: flex;
    flex-direction: column;
    width: min(440px, 100%);
    max-height: 100%;
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--bg);
    box-shadow: var(--shadow-pop);
    overflow: hidden;
    outline: none;
    font-size: var(--fs-ui);
  }
  h2 {
    margin: 0;
    padding: var(--sp-3) var(--sp-3) 0;
    font-size: 15px;
    font-weight: 600;
  }
  .scopes {
    display: flex;
    gap: 2px;
    padding: var(--sp-2) var(--sp-2) 0;
  }
  .scope {
    min-width: 0;
    padding: 3px var(--sp-3);
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-ui-sm);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    cursor: pointer;
  }
  .scope:hover {
    background: var(--hover);
  }
  .scope[aria-selected="true"] {
    background: var(--accent-soft);
    color: var(--accent-text);
    font-weight: 600;
  }
  .scope:focus-visible,
  .list:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  .panel {
    display: flex;
    flex-direction: column;
    flex: 1 1 auto;
    min-height: 0;
  }
  .field {
    margin: var(--sp-2);
    padding: 8px var(--sp-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: 14px;
    outline: none;
  }
  .field:focus {
    border-color: var(--accent);
    box-shadow: 0 0 0 2px var(--accent-soft);
  }
  .list {
    flex: 1 1 auto;
    min-height: 0;
    max-height: 60vh;
    overflow-y: auto;
    padding: 0 var(--sp-2) var(--sp-2);
  }
  .group {
    padding: var(--sp-2) var(--sp-2) 2px;
    color: var(--muted);
    font-size: var(--fs-ui-xs);
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    padding: 5px var(--sp-2);
    border-radius: var(--radius-control);
    color: var(--text);
    cursor: pointer;
  }
  .row :global(svg) {
    flex: none;
    color: var(--accent-text);
  }
  .row.active {
    background: var(--accent-soft);
  }
  .row.dim {
    cursor: default;
  }
  .row.dim .strong,
  .row.dim .faint {
    color: var(--faint);
  }
  .text {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .strong {
    font-weight: 500;
  }
  .faint {
    margin-inline-start: var(--sp-1);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  /* the matched part of the quieter name: muted text on the mark color is hard to read in the dark theme */
  .faint :global(.hit) {
    color: var(--text);
  }
  .none {
    margin: var(--sp-3) var(--sp-4) var(--sp-4);
    color: var(--muted);
  }
  .foot {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: flex-end;
    gap: var(--sp-2) var(--sp-3);
    padding: var(--sp-2) var(--sp-3);
    border-top: 1px solid var(--border);
    background: var(--surface);
  }
  .words {
    flex: 1 1 12em;
    min-width: 0;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .words p {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .note {
    color: var(--text);
  }
  .buttons {
    display: flex;
    gap: var(--sp-2);
  }
</style>
