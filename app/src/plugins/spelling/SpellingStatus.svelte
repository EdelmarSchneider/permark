<script lang="ts">
  // Status bar item while a document is edited: the language its words are checked in (short name; where it comes
  // from in the tooltip), "Not available" for a writing system no dictionary covers, a download mark while the
  // language's dictionary is not on this computer. A click opens the language picker. Also the place of the plugin's
  // menus and of the download question (drawn over the window, whatever the item shows).
  import BookDown from "@lucide/svelte/icons/book-down";
  import SpellCheck from "@lucide/svelte/icons/spell-check";
  import { t } from "../../core/i18n";
  import { stateText } from "./menus";
  import { languageLabel, shortLabel } from "./names";
  import SpellingMenus from "./SpellingMenus.svelte";
  import { spelling, type SpellingStore } from "./store.svelte";

  type Props = { store?: SpellingStore };
  let { store = spelling }: Props = $props();

  let button: HTMLButtonElement | undefined = $state();

  const status = $derived(store.status);
  /** The dictionary of the document's language, while it is on its way. */
  const fetching = $derived.by(() => {
    if (status.kind !== "language" || !status.missing) return null;
    const language = store.languageOf(status.lang);
    return language?.state.kind === "downloading" ? language : null;
  });

  const label = $derived.by((): string => {
    switch (status.kind) {
      case "none":
        return "";
      case "unsupported":
        return t("spelling.status.unavailable");
      case "language":
        return fetching === null
          ? shortLabel(status.lang)
          : `${shortLabel(status.lang)} · ${stateText(fetching) ?? ""}`;
      default:
        return status satisfies never;
    }
  });

  const tooltip = $derived.by((): string => {
    switch (status.kind) {
      case "none":
        return "";
      case "unsupported":
        return t("spelling.status.unavailableHint");
      case "language": {
        const language = languageLabel(status.lang);
        const where = t(
          status.source === "declared"
            ? "spelling.status.declared"
            : status.source === "detected"
              ? "spelling.status.detected"
              : "spelling.status.default",
          { language },
        );
        return status.missing ? `${where}. ${t("spelling.status.missing")}` : where;
      }
      default:
        return status satisfies never;
    }
  });

  function toggle(): void {
    if (store.picker !== null) {
      store.closePicker();
      return;
    }
    const box = button?.getBoundingClientRect();
    store.openPicker(box?.left ?? 0, box?.top ?? 0);
  }
</script>

{#if status.kind !== "none"}
  <button
    bind:this={button}
    type="button"
    class="item"
    class:is-missing={status.kind === "language" && status.missing}
    aria-haspopup="menu"
    aria-expanded={store.picker !== null}
    title={tooltip}
    aria-label={tooltip}
    data-spelling-status
    onclick={toggle}
  >
    {#if status.kind === "language" && status.missing}
      <BookDown size={13} aria-hidden="true" />
    {:else}
      <SpellCheck size={13} aria-hidden="true" />
    {/if}
    <span class="label">{label}</span>
  </button>
{/if}
<SpellingMenus {store} />

<style>
  .item {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    margin-right: var(--sp-3);
    padding: 0 6px;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }
  .item:hover,
  .item[aria-expanded="true"] {
    background: var(--hover);
  }
  /* small text on the status bar: the text accent (AA in every tone), not the accent itself */
  .item.is-missing {
    color: var(--accent-text);
  }
  .label {
    white-space: nowrap;
  }
</style>
