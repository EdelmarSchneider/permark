<script lang="ts">
  // The Explorer filter: a preset, the file types of a custom filter, `+pattern` | `-pattern` entries, and switches.
  // Every change applies at once; the backend runs the filter.
  import type { Category, FilterShow, TreeFilter } from "../../api";
  import { t, type MessageKey } from "../i18n";
  import { CATEGORIES, FILTER_SHOWS, withShow } from "./filter";
  import type { ExplorerStore } from "./store.svelte";

  type Props = { store: ExplorerStore; close: () => void };
  let { store, close }: Props = $props();

  const LABELS: Record<FilterShow, { label: MessageKey; hint: MessageKey }> = {
    documents: { label: "filter.documents", hint: "filter.documentsHint" },
    "docs-and-assets": { label: "filter.docsAndAssets", hint: "filter.docsAndAssetsHint" },
    everything: { label: "filter.everything", hint: "filter.everythingHint" },
    custom: { label: "filter.custom", hint: "filter.customHint" },
  };

  const CATEGORY_LABELS: Record<Category, MessageKey> = {
    markdown: "filter.category.markdown",
    text: "filter.category.text",
    config: "filter.category.config",
    data: "filter.category.data",
    image: "filter.category.image",
    audio: "filter.category.audio",
    video: "filter.category.video",
    office: "filter.category.office",
    code: "filter.category.code",
    other: "filter.category.other",
  };

  const filter = $derived(store.filter);
  let panel: HTMLElement | undefined = $state();

  function change(next: Partial<TreeFilter>): void {
    void store.setFilter({ ...filter, ...next });
  }

  function toggleCategory(category: Category, on: boolean): void {
    const rest = filter.categories.filter((c) => c !== category);
    change({ categories: on ? [...rest, category] : rest });
  }

  // closes on Escape | click outside (imperative listeners, torn down on close)
  $effect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") close();
    };
    const onPointer = (e: PointerEvent): void => {
      const target = e.target;
      if (
        panel &&
        target instanceof Node &&
        !panel.contains(target) &&
        !(target instanceof Element && target.closest("[data-filter-trigger]"))
      )
        close();
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("pointerdown", onPointer);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("pointerdown", onPointer);
    };
  });
</script>

<div class="popover" role="dialog" aria-label={t("filter.title")} bind:this={panel}>
  <div class="menu-label">{t("filter.title")}</div>
  <div class="segments" role="radiogroup" aria-label={t("filter.hint")}>
    {#each FILTER_SHOWS as show (show)}
      <button
        type="button"
        role="radio"
        aria-checked={filter.show === show}
        class={["segment", filter.show === show && "is-on"]}
        onclick={() => change(withShow(filter, show))}
      >
        {t(LABELS[show].label)}
      </button>
    {/each}
  </div>
  <p class="hint">{t(LABELS[filter.show].hint)}</p>

  {#if filter.show === "custom"}
    <fieldset class="types">
      <legend>{t("filter.categories")}</legend>
      {#each CATEGORIES as category (category)}
        <label class="check">
          <input
            type="checkbox"
            checked={filter.categories.includes(category)}
            onchange={(e) => toggleCategory(category, e.currentTarget.checked)}
          />
          <span>{t(CATEGORY_LABELS[category])}</span>
        </label>
      {/each}
    </fieldset>
    <label class="field">
      <span>{t("filter.advanced")}</span>
      <input
        type="text"
        spellcheck="false"
        autocomplete="off"
        value={filter.patterns}
        onchange={(e) => change({ patterns: e.currentTarget.value })}
      />
      <span class="hint">{t("filter.advancedHint")}</span>
    </label>
    {#each store.invalidPatterns as pattern (pattern)}
      <p class="problem">{t("filter.invalidGlob", { pattern })}</p>
    {/each}
  {/if}

  {#if filter.show !== "documents" && filter.show !== "everything"}
    <label class="check">
      <input
        type="checkbox"
        checked={filter.assetsNearDocsOnly}
        onchange={(e) => change({ assetsNearDocsOnly: e.currentTarget.checked })}
      />
      <span>
        {t("filter.assetsNearDocs")}
        <span class="hint">{t("filter.assetsNearDocsHint")}</span>
      </span>
    </label>
  {/if}
  <label class="check">
    <input
      type="checkbox"
      checked={filter.hideEmptyDirs}
      onchange={(e) => change({ hideEmptyDirs: e.currentTarget.checked })}
    />
    <span>{t("filter.hideEmptyFolders")}</span>
  </label>
  <label class="check">
    <input
      type="checkbox"
      checked={filter.showHidden}
      onchange={(e) => change({ showHidden: e.currentTarget.checked })}
    />
    <span>
      {t("filter.showHidden")}
      <span class="hint">{t("filter.showHiddenHint")}</span>
    </span>
  </label>
  <label class="check">
    <input
      type="checkbox"
      checked={filter.showIgnored}
      onchange={(e) => change({ showIgnored: e.currentTarget.checked })}
    />
    <span>
      {t("filter.showIgnored")}
      <span class="hint">{t("filter.showIgnoredHint")}</span>
    </span>
  </label>

  <footer>
    <button type="button" class="btn btn--sm" onclick={() => void store.resetFilter()}>
      {t("filter.useProjectDefault")}
    </button>
  </footer>
</div>

<style>
  .popover {
    position: absolute;
    top: calc(var(--h-header) - 8px);
    left: var(--sp-2);
    z-index: 40;
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    width: calc(var(--w-sidebar) + 60px);
    max-height: calc(100vh - 120px);
    overflow: auto;
    padding: var(--sp-2) var(--sp-3) var(--sp-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--bg);
    box-shadow: var(--shadow-pop);
    animation: pm-fade var(--dur-fast) var(--ease);
  }
  .menu-label {
    padding: 4px 0 0;
  }
  .segments {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 2px;
    padding: 2px;
    border-radius: var(--radius-control);
    background: var(--surface-2);
  }
  .segment {
    padding: 4px 6px;
    border: 0;
    border-radius: 4px;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-ui-sm);
    cursor: pointer;
  }
  .segment.is-on {
    background: var(--bg);
    color: var(--text);
    box-shadow: 0 0 0 1px var(--border);
  }
  .hint {
    display: block;
    margin: 0;
    color: var(--faint);
    font-size: var(--fs-ui-xs);
  }
  .types {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 2px var(--sp-3);
    margin: 0;
    padding: 0;
    border: 0;
  }
  legend {
    padding: 0 0 2px;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .check {
    display: flex;
    align-items: flex-start;
    gap: var(--sp-2);
    font-size: var(--fs-ui);
  }
  .check input {
    margin-top: 3px;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: var(--fs-ui-sm);
    color: var(--muted);
  }
  .field input {
    height: 28px;
    padding: 0 8px;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-family: var(--font-code);
    font-size: var(--fs-ui-sm);
  }
  .problem {
    margin: 0;
    color: var(--git-deleted-text);
    font-size: var(--fs-ui-xs);
  }
  footer {
    display: flex;
    justify-content: flex-end;
  }
</style>
