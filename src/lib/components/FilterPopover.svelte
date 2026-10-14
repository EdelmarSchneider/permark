<script lang="ts">
  import { onMount } from "svelte";
  import { FILE_CATEGORIES, type FileCategory } from "../fileKinds";
  import { t, type MessageKey } from "../i18n";
  import { filter } from "../stores/filter.svelte";
  import { FILTER_SHOWS, parsePatterns, withShow } from "../treeFilter";
  import type { FilterShow } from "../types";
  import Popover, { type Anchor } from "./Popover.svelte";

  type Props = { anchor: Anchor; onclose: () => void };
  let { anchor, onclose }: Props = $props();

  const names: Record<FilterShow, MessageKey> = {
    documents: "filter.documents",
    "docs-and-assets": "filter.docsAndAssets",
    everything: "filter.everything",
    custom: "filter.custom",
  };
  const hints: Partial<Record<FilterShow, MessageKey>> = {
    documents: "filter.documentsHint",
    "docs-and-assets": "filter.docsAndAssetsHint",
    everything: "filter.everythingHint",
  };
  const categoryNames: Record<FileCategory, MessageKey> = {
    documents: "filter.categoryDocuments",
    images: "filter.categoryImages",
    data: "filter.categoryData",
    xml: "filter.categoryXml",
    text: "filter.categoryText",
    other: "filter.categoryOther",
  };

  let group: HTMLElement | undefined = $state();

  const current = $derived(filter.current);
  // the rule is about what the presets call assets: it has no meaning for Documents or Everything
  const assetsRule = $derived(current.show === "docs-and-assets" || current.show === "custom");
  const invalid = $derived(
    current.show === "custom" ? parsePatterns(current.patterns).invalid : [],
  );

  function toggleCategory(category: FileCategory, on: boolean): void {
    const categories = on
      ? [...current.categories, category]
      : current.categories.filter((c) => c !== category);
    filter.set({ ...current, categories });
  }

  onMount(() => {
    // the popover is positioned in an effect; focus the chosen option once it is in place
    const timer = setTimeout(() => {
      group?.querySelector<HTMLInputElement>("input:checked")?.focus();
    }, 0);
    return () => clearTimeout(timer);
  });
</script>

<Popover {anchor} align="end" variant="panel" label={t("filter.title")} {onclose}>
  <div bind:this={group} class="filter" role="radiogroup" aria-label={t("filter.title")}>
    {#each FILTER_SHOWS as show (show)}
      <label class="filter__option">
        <input
          type="radio"
          name="tree-filter"
          value={show}
          checked={current.show === show}
          onchange={() => filter.set(withShow(current, show))}
        />
        <span class="filter__text">
          <span class="filter__name">{t(names[show])}</span>
          {#if hints[show]}<span class="filter__hint">{t(hints[show])}</span>{/if}
        </span>
      </label>
    {/each}
  </div>

  {#if current.show === "custom"}
    <div class="field">
      <div class="filter__categories" role="group" aria-label={t("filter.categories")}>
        {#each FILE_CATEGORIES as category (category)}
          <label
            class="filter__check"
            title={category === "data" ? t("filter.categoryDataHint") : undefined}
          >
            <input
              type="checkbox"
              checked={current.categories.includes(category)}
              onchange={(e) => toggleCategory(category, e.currentTarget.checked)}
            />
            <span>{t(categoryNames[category])}</span>
          </label>
        {/each}
      </div>
    </div>
    <div class="field">
      <label class="field__label" for="filter-patterns">{t("filter.advanced")}</label>
      <input
        id="filter-patterns"
        class="text-input filter__patterns"
        value={current.patterns}
        spellcheck="false"
        autocomplete="off"
        aria-invalid={invalid.length > 0}
        aria-describedby="filter-patterns-note"
        oninput={(e) => filter.set({ ...current, patterns: e.currentTarget.value })}
      />
      <span id="filter-patterns-note" class="filter__note" aria-live="polite">
        {#if invalid.length > 0}
          <span class="filter__error">{t("filter.invalidGlob", { pattern: invalid[0] })}</span>
        {:else}
          {t("filter.advancedHint")}
        {/if}
      </span>
    </div>
  {/if}

  <div class="menu-sep" role="separator"></div>
  <label class={["filter__check", !assetsRule && "is-disabled"]}>
    <input
      type="checkbox"
      checked={current.assetsNearDocs}
      disabled={!assetsRule}
      aria-describedby="filter-assets-note"
      onchange={(e) => filter.set({ ...current, assetsNearDocs: e.currentTarget.checked })}
    />
    <span class="filter__text">
      <span>{t("filter.assetsNearDocs")}</span>
      <span id="filter-assets-note" class="filter__hint">{t("filter.assetsNearDocsHint")}</span>
    </span>
  </label>
  <label class="filter__check">
    <input
      type="checkbox"
      checked={current.hideEmptyFolders}
      onchange={(e) => filter.set({ ...current, hideEmptyFolders: e.currentTarget.checked })}
    />
    <span>{t("filter.hideEmptyFolders")}</span>
  </label>
  <div class="menu-sep" role="separator"></div>
  <label class="filter__check">
    <input
      type="checkbox"
      checked={current.showHidden}
      aria-describedby="filter-hidden-note"
      onchange={(e) => filter.set({ ...current, showHidden: e.currentTarget.checked })}
    />
    <span class="filter__text">
      <span>{t("filter.showHidden")}</span>
      <span id="filter-hidden-note" class="filter__hint">{t("filter.showHiddenHint")}</span>
    </span>
  </label>
  <label class="filter__check">
    <input
      type="checkbox"
      checked={current.showIgnored}
      aria-describedby="filter-ignored-note"
      onchange={(e) => filter.set({ ...current, showIgnored: e.currentTarget.checked })}
    />
    <span class="filter__text">
      <span>{t("filter.showIgnored")}</span>
      <span id="filter-ignored-note" class="filter__hint">{t("filter.showIgnoredHint")}</span>
    </span>
  </label>
</Popover>
