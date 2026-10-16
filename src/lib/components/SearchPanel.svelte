<script lang="ts">
  import { tick, untrack } from "svelte";
  import { formatNumber, t, tn } from "../i18n";
  import { isComposingKey } from "../ime";
  import { crumbOf, sanitizeSnippet } from "../searchModel";
  import { knownTagsHint } from "../tagModel";
  import { app } from "../stores/app.svelte";
  import { search } from "../stores/search.svelte";
  import { tags } from "../stores/tags.svelte";
  import { ui } from "../stores/ui.svelte";
  import { unread } from "../stores/unread.svelte";
  import { vault } from "../stores/vault.svelte";
  import Icon from "./Icon.svelte";
  import IconButton from "./IconButton.svelte";
  import TagChip from "./TagChip.svelte";
  import TagSuggestions from "./TagSuggestions.svelte";
  import UnreadDot from "./UnreadDot.svelte";

  let input: HTMLInputElement | undefined = $state();
  let list: HTMLElement | undefined = $state();

  const box = search.box;
  const suggesting = $derived(box.partial !== null && box.suggestions.length > 0);
  const progress = $derived(vault.indexProgress);
  const indexing = $derived(progress !== null && progress.indexed < progress.total);
  const percent = $derived(
    progress && progress.total > 0 ? Math.round((progress.indexed / progress.total) * 100) : 0,
  );
  const summary = $derived(
    search.status === "ready" && search.hits.length > 0
      ? t("search.summary", {
          hits: search.hits.length,
          files: tn("search.files", search.groups.length),
        })
      : search.status === "loading"
        ? t("search.searching")
        : "",
  );

  // Ctrl+Shift+F: focus the query and select its text
  $effect(() => {
    void ui.searchFocusTick;
    untrack(() => {
      if (ui.searchFocusTick > 0) {
        input?.focus();
        input?.select();
      }
    });
  });

  // keep the active row visible while moving with the keyboard
  $effect(() => {
    const key = search.active;
    if (!key) return;
    untrack(() => {
      void tick().then(() => {
        list
          ?.querySelector(`[data-key="${CSS.escape(key)}"]`)
          ?.scrollIntoView({ block: "nearest" });
      });
    });
  });

  function focusDocument(): void {
    document.querySelector<HTMLElement>(".doc-scroll")?.focus();
  }

  function escape(): void {
    if (!box.isEmpty) {
      search.reset();
      input?.focus();
    } else {
      focusDocument();
    }
  }

  function onQueryKey(e: KeyboardEvent): void {
    if (isComposingKey(e)) return;
    const chips = box.chips;
    if (box.handleKey(e)) {
      e.preventDefault();
      if (box.chips !== chips) search.changedChips();
      return;
    }
    if (e.key === "ArrowDown") {
      e.preventDefault();
      list?.focus();
      if (search.active === null && search.rows.length > 0) search.move(0);
    } else if (e.key === "Enter") {
      e.preventDefault();
      search.runNow();
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      escape();
    }
  }

  function activeRow() {
    return search.rows.find((r) => search.rowKey(r) === search.active) ?? null;
  }

  function openActive(): void {
    const row = activeRow();
    if (!row) return;
    if (row.type === "group") search.toggleGroup(row.group.path);
    else void app.openHit(row.hit);
  }

  function onListKey(e: KeyboardEvent): void {
    if (isComposingKey(e)) return;
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        search.move(1);
        break;
      case "ArrowUp":
        e.preventDefault();
        if (search.activeIndex <= 0) input?.focus();
        else search.move(-1);
        break;
      case "ArrowLeft":
      case "ArrowRight": {
        const row = activeRow();
        if (!row) break;
        e.preventDefault();
        const collapse = e.key === "ArrowLeft";
        search.toggleGroup(row.group.path, collapse);
        search.active = `group:${row.group.path}`;
        break;
      }
      case "Enter":
        e.preventDefault();
        openActive();
        break;
      case "Escape":
        e.preventDefault();
        e.stopPropagation();
        escape();
        break;
    }
  }
</script>

<div class="panel-header">
  <span class="panel-title">{t("search.title")}</span>
  <span class="spacer"></span>
  <IconButton
    small
    icon="chevrons-left"
    label={t("panel.collapseLeft")}
    hint={t("panel.collapseLeftHint")}
    onclick={() => app.toggleLeft()}
  />
</div>
<!-- svelte-ignore a11y_label_has_associated_control -->
<label class="search-box">
  <Icon name="search" size="sm" />
  {#if box.chips.length > 0}
    <span class="search-box__chips">
      {#each box.chips as chip (chip)}
        <TagChip
          name={chip}
          removable
          unknown={!tags.known.has(chip)}
          title={t("tags.queryChip", { name: chip })}
          removeLabel={t("tags.removeFilter", { name: chip })}
          onremove={() => {
            box.remove(chip);
            search.changedChips();
            input?.focus();
          }}
        />
      {/each}
    </span>
  {/if}
  <input
    bind:this={input}
    value={search.query}
    oninput={(e) => search.setQuery(e.currentTarget.value)}
    onkeydown={onQueryKey}
    placeholder={box.chips.length > 0 ? "" : t("search.placeholder")}
    aria-label={t("search.label")}
    role="combobox"
    aria-expanded={suggesting}
    aria-controls="search-tags"
    aria-activedescendant={suggesting ? `search-tags-${box.active}` : undefined}
    spellcheck="false"
    autocomplete="off"
  />
  {#if !box.isEmpty}
    <IconButton
      small
      icon="x"
      label={t("search.clear")}
      hint={t("search.clearHint")}
      onclick={() => {
        search.reset();
        input?.focus();
      }}
    />
  {/if}
</label>
<div class="search-sub">
  <span class="switch is-disabled" title={t("search.semanticHint")}>
    <span
      class="switch__track"
      role="switch"
      aria-checked="false"
      aria-disabled="true"
      aria-label={t("search.semanticLabel")}
    ></span>
    {t("search.semantic")} <span class="soon-tag">{t("search.soon")}</span>
  </span>
  <span aria-live="polite">{summary}</span>
</div>
{#if indexing && progress}
  <div class="index-progress">
    {t("search.indexing", {
      indexed: formatNumber(progress.indexed),
      total: formatNumber(progress.total),
    })}
    <div
      class="bar"
      role="progressbar"
      aria-valuenow={percent}
      aria-valuemin={0}
      aria-valuemax={100}
    >
      <span style:width="{percent}%"></span>
    </div>
  </div>
{/if}

{#if suggesting}
  <div class="search-results scroll">
    <TagSuggestions
      id="search-tags"
      rows={box.suggestions.map((match) => ({ type: "tag" as const, match }))}
      active={box.active}
      onpick={(row) => {
        if (row.type !== "tag") return;
        box.pick(row.match.tag.name);
        search.changedChips();
        input?.focus();
      }}
      onhover={(i) => (box.active = i)}
    />
  </div>
{:else if search.unknown.length > 0}
  <div class="empty">
    {#if tags.counts.length === 0}
      <strong>{t("tags.noTagsAtAll", { name: search.unknown[0] })}</strong>
    {:else}
      <strong
        >{t("tags.unknownTag", {
          name: search.unknown[0],
          tags: knownTagsHint(tags.counts),
        })}</strong
      >
    {/if}
  </div>
{:else if search.status === "error"}
  <div class="empty">
    <strong>{t("search.error", { message: search.error ?? "" })}</strong>
    <button class="btn btn--sm" onclick={() => search.runNow()}>{t("search.retry")}</button>
  </div>
{:else if search.tagsOnly}
  <div class="empty">{t("search.tagsOnly")}</div>
{:else if box.isEmpty}
  <div class="empty">{t("search.empty")}</div>
{:else if search.status === "ready" && search.rows.length === 0}
  <div class="empty">
    {#if indexing}
      <strong>{t("search.noneYet")}</strong>
    {:else}
      <strong>{t("search.none", { query: search.answered })}</strong>
      {t("search.noneHint")}
    {/if}
  </div>
{:else}
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div
    bind:this={list}
    class="search-results scroll"
    role="tree"
    tabindex="0"
    aria-label={t("search.results")}
    onkeydown={onListKey}
  >
    {#each search.rows as row (search.rowKey(row))}
      {@const key = search.rowKey(row)}
      {#if row.type === "group"}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div
          class={["hit-group", search.active === key && "is-active"]}
          role="treeitem"
          tabindex="-1"
          aria-selected={search.active === key}
          aria-expanded={!search.collapsed.has(row.group.path)}
          data-key={key}
          onclick={() => {
            search.active = key;
            search.toggleGroup(row.group.path);
            list?.focus();
          }}
        >
          <Icon name="chevron-down" size="sm" class="twisty" />
          {#if unread.isUnread(row.group.path)}<UnreadDot />{/if}
          <span class="hit-group__name">{row.group.name}</span>
          <span class="count-pill">{row.group.hits.length}</span>
          <span class="hit-group__path">{row.group.where}</span>
          {#if row.group.description !== ""}
            <span class="hit-group__desc">{row.group.description}</span>
          {/if}
        </div>
      {:else}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div
          class={["hit", search.active === key && "is-active"]}
          role="treeitem"
          tabindex="-1"
          aria-selected={search.active === key}
          data-key={key}
          onclick={() => {
            search.active = key;
            list?.focus();
            void app.openHit(row.hit);
          }}
        >
          <div class="hit__crumb">
            {#each crumbOf(row.hit) as part, i (i)}{#if i > 0}<span class="sep">›</span
                >{/if}{part}{/each}
          </div>
          <div class="hit__snippet">{@html sanitizeSnippet(row.hit.snippet)}</div>
        </div>
      {/if}
    {/each}
  </div>
{/if}
