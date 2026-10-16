<script lang="ts">
  // The language of a code block (the toolbar's language button): a search over every language highlight.js knows, by name or by
  // any alias, and free text for anything else. Whatever is picked goes into the fence's info string; typed text goes in exactly as
  // typed. Enter picks the highlighted row, Up and Down move, Esc closes.
  import { onMount, untrack } from "svelte";
  import { t } from "../i18n";
  import { pickerRows, resolveLanguage, type PickerRow } from "../code/languages";
  import Icon from "./Icon.svelte";
  import Popover, { type Anchor } from "./Popover.svelte";

  type Props = {
    anchor: Anchor;
    /** The info string the block has now ("" for none). */
    current: string;
    /** The text to write after the backticks ("" for plain text). */
    onpick(language: string): void;
    onclose(): void;
  };
  let { anchor, current, onpick, onclose }: Props = $props();

  let query = $state("");
  let index = $state(0);
  let input: HTMLInputElement | undefined = $state();
  let list: HTMLElement | undefined = $state();

  const rows = $derived(pickerRows(query));
  const currentId = $derived(resolveLanguage(current)?.id ?? null);

  // a new search starts at the top; the list that opens starts at the language the block has
  $effect(() => {
    void query;
    index = 0;
    // the list was scrolled to the language the block had when it opened: a search starts at its top
    const box = untrack(() => list);
    if (box) box.scrollTop = 0;
  });

  function checked(row: PickerRow): boolean {
    if (row.kind === "plain") return current.trim() === "";
    if (row.kind === "language") return currentId === row.language.id;
    return false;
  }

  function scrollToActive(): void {
    list?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: "nearest" });
  }

  onMount(() => {
    const at = rows.findIndex(checked);
    if (at > 0) index = at;
    const timer = setTimeout(() => {
      input?.focus();
      scrollToActive();
    }, 0);
    return () => clearTimeout(timer);
  });

  function pick(row: PickerRow): void {
    onpick(row.kind === "plain" ? "" : row.write);
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (rows.length === 0) return;
      index = (index + (e.key === "ArrowDown" ? 1 : -1) + rows.length) % rows.length;
      queueMicrotask(scrollToActive);
    } else if (e.key === "Enter") {
      e.preventDefault();
      const row = rows[index];
      if (row) pick(row);
    }
  }
</script>

<Popover {anchor} variant="panel" label={t("toolbar.language")} {onclose}>
  <div class="lang-picker" role="presentation" {onkeydown}>
    <input
      bind:this={input}
      bind:value={query}
      class="text-input"
      type="text"
      role="combobox"
      aria-expanded="true"
      aria-controls="lang-picker-list"
      aria-activedescendant={`lang-picker-${index}`}
      aria-label={t("toolbar.languageSearch")}
      placeholder={t("toolbar.languageSearch")}
      autocomplete="off"
      autocapitalize="off"
      spellcheck="false"
    />
    <div
      bind:this={list}
      id="lang-picker-list"
      class="lang-picker__list"
      role="listbox"
      aria-label={t("toolbar.language")}
    >
      {#each rows as row, i (i)}
        <button
          type="button"
          id={`lang-picker-${i}`}
          class={["menu-item", "lang-picker__row", i === index && "is-active"]}
          role="option"
          tabindex="-1"
          aria-selected={i === index}
          onmousemove={() => (index = i)}
          onclick={() => pick(row)}
        >
          <span class="menu-item__check">
            {#if checked(row)}<Icon name="check" size="sm" />{/if}
          </span>
          {#if row.kind === "plain"}
            <span class="menu-item__label">{t("toolbar.plainText")}</span>
          {:else if row.kind === "language"}
            <span class="menu-item__label">{row.language.name}</span>
            <span class="menu-item__hint lang-picker__code">{row.write}</span>
          {:else}
            <span class="menu-item__label">{t("toolbar.languageUse", { text: row.write })}</span>
          {/if}
        </button>
      {/each}
    </div>
  </div>
</Popover>
