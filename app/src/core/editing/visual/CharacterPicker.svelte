<script lang="ts" module>
  /** A button of the footer, before Cancel (the plugins' picker actions). */
  export type PickerFootAction = { id: string; label: string; run: () => void };
</script>

<script lang="ts">
  // The popup behind the toolbar's emoji + symbols buttons: the groups the settings payload ships, searchable by
  // name, by the character itself or by its code point. The window's settings store holds the groups (no
  // subscription of its own here), the filtering is `characters.ts` over `core/search/fuzzy.ts`, and a click hands
  // the character to the editor through the `run` path.
  import { onMount } from "svelte";
  import { t } from "../../i18n";
  import { windowStore } from "../../window.svelte";
  import {
    cellLabel,
    cellName,
    codePoints,
    entryName,
    groupLabel,
    isInvisible,
    scanGroups,
    type PickerKind,
  } from "./characters";

  type Props = {
    /** The payload's emoji group, or the symbol groups. */
    kind: PickerKind;
    /** Insert the picked character at the caret and hand the keyboard back to the editor. */
    apply: (character: string) => void;
    /** Close the popup without inserting. */
    close: () => void;
    /** The editor's popover key handling (Escape closes, focus returns). */
    popoverKey: (event: KeyboardEvent) => void;
    /** Buttons the footer shows before Cancel. */
    actions?: readonly PickerFootAction[];
  };
  let { kind, apply, close, popoverKey, actions = [] }: Props = $props();

  let query = $state("");

  const shown = $derived(scanGroups(kind, windowStore.characters, query));

  // edits to the groups file show when a picker opens, without a restart
  onMount(() => windowStore.refreshCharacters());
</script>

<div class="picker">
  <!-- svelte-ignore a11y_autofocus -->
  <input
    class="picker__search"
    type="search"
    placeholder={t("picker.search")}
    aria-label={t("picker.search")}
    autofocus
    value={query}
    onkeydown={popoverKey}
    oninput={(e) => (query = e.currentTarget.value)}
  />
  {#if shown.length === 0}
    <p class="picker__empty">{t("picker.empty")}</p>
  {:else}
    {#each shown as group, at (at)}
      {@const label = groupLabel(group.name)}
      <div class="picker__group" role="group" aria-label={label}>
        <h3 class="picker__title">{label}</h3>
        <div class="picker__grid">
          <!-- keyed by place: a groups file may list one character in several groups and a query lists both -->
          {#each group.entries as entry, index (index)}
            {@const name = entryName(entry)}
            <button
              type="button"
              class="picker__char"
              class:picker__char--invisible={isInvisible(entry.char)}
              aria-label={cellName(entry)}
              title={t("picker.tip", { name, code: codePoints(entry.char) })}
              onclick={() => apply(entry.char)}>{cellLabel(entry)}</button
            >
          {/each}
        </div>
      </div>
    {/each}
  {/if}
  <div class="picker__foot">
    {#each actions as action (action.id)}
      <button type="button" class="btn btn--sm btn--ghost" onclick={action.run}
        >{action.label}</button
      >
    {/each}
    <button type="button" class="btn btn--sm btn--ghost" onclick={close}
      >{t("common.cancel")}</button
    >
  </div>
</div>

<style>
  .picker {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    padding: var(--sp-3);
    max-height: 22rem;
    overflow-y: auto;
  }
  .picker__search {
    height: 2rem;
    padding: 0 var(--sp-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    font-size: var(--fs-ui);
  }
  .picker__group {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
  }
  .picker__title {
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    font-weight: 600;
  }
  .picker__grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(2rem, 1fr));
    gap: var(--sp-1);
  }
  .picker__char {
    height: 2rem;
    padding: 0;
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--text);
    font-size: 1.125rem;
    line-height: 1;
    cursor: pointer;
  }
  .picker__char:hover {
    background: var(--hover);
  }
  /* an invisible character draws as a small name in a dotted box, so the cell does not look empty */
  .picker__char--invisible {
    border: 1px dotted var(--muted);
    color: var(--muted);
    /* five capitals fit a 2rem cell */
    font-size: calc(var(--fs-ui-xs) - 1px);
    letter-spacing: -0.04em;
    overflow: hidden;
  }
  .picker__empty {
    margin: 0;
    color: var(--faint);
    font-size: var(--fs-ui-sm);
  }
  .picker__foot {
    display: flex;
    justify-content: flex-end;
    gap: var(--sp-2);
  }
</style>
