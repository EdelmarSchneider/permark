<script lang="ts">
  // A list of short texts (glob patterns, group names) the person edits one by one: each stays a line with a remove
  // button, a field at the end adds another. Every change reports the whole new list; the owner keeps it.
  import X from "@lucide/svelte/icons/x";
  import { t } from "../../i18n";

  type Props = {
    items: readonly string[];
    label: string;
    placeholder: string;
    onchange: (items: string[]) => void;
    disabled?: boolean;
  };
  let { items, label, placeholder, onchange, disabled = false }: Props = $props();

  let draft = $state("");

  function add(): void {
    const text = draft.trim();
    if (text === "" || items.includes(text)) {
      draft = "";
      return;
    }
    onchange([...items, text]);
    draft = "";
  }

  function remove(at: number): void {
    onchange(items.filter((_, i) => i !== at));
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === "Enter" && !e.isComposing) {
      e.preventDefault();
      add();
    }
  }
</script>

<div class="list" role="group" aria-label={label}>
  {#if items.length > 0}
    <ul>
      {#each items as item, at (item)}
        <li>
          <code>{item}</code>
          <button
            type="button"
            class="icon-btn icon-btn--sm"
            aria-label={t("list.remove", { item })}
            title={t("list.remove", { item })}
            {disabled}
            onclick={() => remove(at)}
          >
            <X size={13} aria-hidden="true" />
          </button>
        </li>
      {/each}
    </ul>
  {/if}
  <div class="add">
    <input
      class="field-input"
      type="text"
      spellcheck="false"
      autocomplete="off"
      aria-label={label}
      {placeholder}
      {disabled}
      bind:value={draft}
      {onkeydown}
    />
    <button
      type="button"
      class="btn btn--sm"
      disabled={disabled || draft.trim() === ""}
      onclick={add}
    >
      {t("list.add")}
    </button>
  </div>
</div>

<style>
  .list {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    width: 100%;
    min-width: 0;
  }
  ul {
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-1) var(--sp-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    max-width: 100%;
    padding: 1px 2px 1px 8px;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--surface);
  }
  code {
    overflow: hidden;
    font-family: var(--font-code);
    font-size: var(--fs-ui-sm);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .add {
    display: flex;
    gap: var(--sp-2);
  }
  .add input {
    flex: 1;
    min-width: 0;
  }
</style>
