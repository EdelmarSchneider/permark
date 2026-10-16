<script lang="ts">
  import { tick, untrack } from "svelte";
  import { gitBadgeTitle } from "../gitLabels";
  import { t, tn } from "../i18n";
  import { isComposingKey } from "../ime";
  import { fileIconOf } from "../fileIcons";
  import { basename, folderLabel } from "../paths";
  import {
    filterTags,
    isEmptySelection,
    selectionLabel,
    selectOnly,
    toggleTag,
    toggleUntagged,
  } from "../tagModel";
  import { app } from "../stores/app.svelte";
  import { docs } from "../stores/docs.svelte";
  import { tags } from "../stores/tags.svelte";
  import { ui } from "../stores/ui.svelte";
  import { unread } from "../stores/unread.svelte";
  import { vault } from "../stores/vault.svelte";
  import { badgeFor } from "../tree";
  import type { TagCount } from "../types";
  import Icon from "./Icon.svelte";
  import IconButton from "./IconButton.svelte";
  import UnreadDot from "./UnreadDot.svelte";

  type Row = { kind: "tag"; tag: TagCount } | { kind: "untagged"; count: number };

  const META_FILE = ".trailmark/meta.toml";

  let filterInput: HTMLInputElement | undefined = $state();
  let tagList: HTMLElement | undefined = $state();
  let fileList: HTMLElement | undefined = $state();
  let tagFocus = $state(0);
  let fileFocus = $state(0);
  // the lists keep the focus and point at a row (aria-activedescendant): the row shows the focus ring
  let tagsFocused = $state(false);
  let filesFocused = $state(false);

  const overview = $derived(tags.overview);
  const problem = $derived(overview?.status.problem ?? null);
  const selection = $derived(tags.selection);
  const shown = $derived(filterTags(overview?.tags ?? [], tags.filter));
  const rows = $derived<Row[]>([
    ...shown.map((tag): Row => ({ kind: "tag", tag })),
    ...(overview && (overview.untagged > 0 || selection.untagged) && tags.filter.trim() === ""
      ? [{ kind: "untagged", count: overview.untagged } satisfies Row]
      : []),
  ]);
  const hasTags = $derived((overview?.tags.length ?? 0) > 0);
  const noMatch = $derived(hasTags && shown.length === 0 && tags.filter.trim() !== "");
  const files = $derived(tags.files);
  const label = $derived(selectionLabel(selection, t("tags.untagged")));

  // Ctrl+Shift+T: focus the filter box
  $effect(() => {
    void ui.tagsFocusTick;
    untrack(() => {
      if (ui.tagsFocusTick > 0) {
        filterInput?.focus();
        filterInput?.select();
      }
    });
  });

  // keep the focus row inside the list when it shrinks
  $effect(() => {
    const max = Math.max(0, rows.length - 1);
    untrack(() => {
      if (tagFocus > max) tagFocus = max;
    });
  });
  $effect(() => {
    const max = Math.max(0, files.length - 1);
    untrack(() => {
      if (fileFocus > max) fileFocus = max;
    });
  });

  function isSelected(row: Row): boolean {
    return row.kind === "untagged" ? selection.untagged : selection.tags.includes(row.tag.name);
  }

  function chooseRow(row: Row, additive: boolean): void {
    if (row.kind === "untagged") tags.select(toggleUntagged(selection, !additive));
    else tags.select(additive ? toggleTag(selection, row.tag.name) : selectOnly(row.tag.name));
  }

  function rowKey(row: Row): string {
    return row.kind === "untagged" ? "untagged" : `tag:${row.tag.name}`;
  }

  async function moveTo(list: "tags" | "files", index: number): Promise<void> {
    if (list === "tags") tagFocus = index;
    else fileFocus = index;
    await tick();
    (list === "tags" ? tagList : fileList)
      ?.querySelector(`[data-i="${index}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }

  function onTagsKey(e: KeyboardEvent): void {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    const last = rows.length - 1;
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        void moveTo("tags", Math.min(last, tagFocus + 1));
        break;
      case "ArrowUp":
        e.preventDefault();
        if (tagFocus === 0) filterInput?.focus();
        else void moveTo("tags", tagFocus - 1);
        break;
      case "Home":
        e.preventDefault();
        void moveTo("tags", 0);
        break;
      case "End":
        e.preventDefault();
        void moveTo("tags", Math.max(0, last));
        break;
      case " ":
        e.preventDefault();
        if (rows[tagFocus]) chooseRow(rows[tagFocus], true);
        break;
      case "Enter":
        e.preventDefault();
        if (rows[tagFocus]) chooseRow(rows[tagFocus], false);
        break;
      case "Escape":
        if (!isEmptySelection(selection)) {
          e.preventDefault();
          e.stopPropagation();
          tags.select({ tags: [], untagged: false });
        }
        break;
    }
  }

  function onFilesKey(e: KeyboardEvent): void {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    const last = files.length - 1;
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        void moveTo("files", Math.min(last, fileFocus + 1));
        break;
      case "ArrowUp":
        e.preventDefault();
        void moveTo("files", Math.max(0, fileFocus - 1));
        break;
      case "Home":
        e.preventDefault();
        void moveTo("files", 0);
        break;
      case "End":
        e.preventDefault();
        void moveTo("files", Math.max(0, last));
        break;
      case "Enter":
        e.preventDefault();
        if (files[fileFocus]) void app.openDoc(files[fileFocus].path);
        break;
    }
  }

  function onFilterKey(e: KeyboardEvent): void {
    if (isComposingKey(e)) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      tagList?.focus();
      void moveTo("tags", Math.min(tagFocus, Math.max(0, rows.length - 1)));
    } else if (e.key === "Escape" && tags.filter !== "") {
      e.preventDefault();
      e.stopPropagation();
      tags.filter = "";
    }
  }
</script>

<div class="panel-header">
  <span class="panel-title">{t("tags.title")}</span>
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
  <input
    bind:this={filterInput}
    bind:value={tags.filter}
    onkeydown={onFilterKey}
    placeholder={t("tags.filter")}
    aria-label={t("tags.filter")}
    spellcheck="false"
    autocomplete="off"
  />
  {#if tags.filter !== ""}
    <IconButton
      small
      icon="x"
      label={t("tags.clearFilter")}
      onclick={() => {
        tags.filter = "";
        filterInput?.focus();
      }}
    />
  {/if}
</label>

{#if problem}
  <div class="banner banner--error tags-banner" role="alert">
    <Icon name="circle-alert" />
    <div>
      {t("tags.metaUnreadable", {
        where: problem.line === null ? "" : t("tags.metaUnreadableLine", { line: problem.line }),
      })}
      {#if (overview?.status.pendingChanges ?? 0) > 0}
        {tn("tags.pendingChanges", overview?.status.pendingChanges ?? 0)}
      {/if}
    </div>
    {#if vault.stateLocation === "project"}
      <span class="spacer"></span>
      <button type="button" class="btn btn--sm" onclick={() => void app.openExternally(META_FILE)}>
        {t("tags.openMetaFile")}
      </button>
    {/if}
  </div>
{/if}

{#if !overview}
  <div class="empty" aria-busy="true"></div>
{:else if !hasTags && overview.untagged === 0}
  <div class="empty">
    <strong>{t("tags.empty")}</strong>
    {t("tags.emptyHint")}
  </div>
{:else}
  {#if noMatch}
    <div class="empty">{t("tags.noMatch", { query: tags.filter.trim() })}</div>
  {:else if !hasTags}
    <div class="empty">
      <strong>{t("tags.empty")}</strong>
      {t("tags.emptyHint")}
    </div>
  {/if}
  {#if rows.length > 0}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div
      bind:this={tagList}
      class="tags-list scroll"
      role="listbox"
      tabindex="0"
      aria-multiselectable="true"
      aria-label={t("tags.label")}
      aria-activedescendant={rows.length > 0 ? `tag-row-${tagFocus}` : undefined}
      onfocus={() => (tagsFocused = true)}
      onblur={() => (tagsFocused = false)}
      onkeydown={onTagsKey}
    >
      {#each rows as row, i (rowKey(row))}
        <!-- the list handles the keys; rows are picked with the pointer or with Enter and Space on the list -->
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div
          id="tag-row-{i}"
          class={[
            "tag-row",
            isSelected(row) && "is-selected",
            tagsFocused && tagFocus === i && "is-focused",
          ]}
          role="option"
          tabindex="-1"
          aria-selected={isSelected(row)}
          data-i={i}
          title={row.kind === "tag" && row.tag.onlyInFiles ? t("tags.onlyInFilesHint") : undefined}
          onclick={(e) => {
            tagFocus = i;
            chooseRow(row, e.ctrlKey || e.metaKey);
            tagList?.focus();
          }}
        >
          <span class="tag-row__check" aria-hidden="true">
            {#if isSelected(row)}<Icon name="check" size="sm" />{/if}
          </span>
          {#if row.kind === "untagged"}
            <span class="tag-row__name tag-row__name--muted">{t("tags.untagged")}</span>
            <span class="count-pill">{row.count}</span>
          {:else}
            <span class="tag-row__name">{row.tag.name}</span>
            {#if row.tag.onlyInFiles}
              <span class="tag-row__file"
                ><Icon name="file-text" size="xs" />{t("tags.fromFile")}</span
              >
            {/if}
            <span class="count-pill" title={tn("tags.countHint", row.tag.count)}
              >{row.tag.count}</span
            >
          {/if}
        </div>
      {/each}
    </div>
  {/if}

  <div class="tags-files">
    {#if isEmptySelection(selection)}
      {#if hasTags}<div class="panel-note">{t("tags.pickHint")}</div>{/if}
    {:else}
      <div class="tags-files__head" aria-live="polite">
        <span class="tags-files__count">{tn("tags.filesCount", files.length)}</span>
        <span class="tags-files__label">{label}</span>
      </div>
      {#if files.length === 0}
        <div class="empty">
          {selection.untagged ? t("tags.noFilesUntagged") : t("tags.noFilesAll")}
        </div>
      {:else}
        <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
        <div
          bind:this={fileList}
          class="tags-files__list scroll"
          role="listbox"
          tabindex="0"
          aria-label={t("tags.files")}
          aria-activedescendant={`tag-file-${fileFocus}`}
          onfocus={() => (filesFocused = true)}
          onblur={() => (filesFocused = false)}
          onkeydown={onFilesKey}
        >
          {#each files as file, i (file.path)}
            {@const badge = badgeFor(vault.git?.files[file.path]?.state)}
            {@const folder = folderLabel(file.path)}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <div
              id="tag-file-{i}"
              class={[
                "tag-file",
                docs.path === file.path && "is-current",
                filesFocused && fileFocus === i && "is-focused",
              ]}
              role="option"
              tabindex="-1"
              aria-selected={docs.path === file.path}
              data-i={i}
              title={file.path}
              onclick={() => {
                fileFocus = i;
                fileList?.focus();
                void app.openDoc(file.path);
              }}
            >
              <div class="tag-file__line">
                <Icon name={fileIconOf(file.path)} size="sm" />
                {#if unread.isUnread(file.path)}<UnreadDot />{/if}
                <span class="tag-file__name">{basename(file.path)}</span>
                <span class="tag-file__folder">{folder === "" ? t("feed.root") : folder}</span>
                {#if badge}
                  <span
                    class={["badge", badge.className]}
                    title={gitBadgeTitle(badge, vault.git?.baseline.kind)}>{badge.letter}</span
                  >
                {/if}
              </div>
              {#if file.description !== ""}
                <div class="tag-file__desc">{file.description}</div>
              {/if}
            </div>
          {/each}
        </div>
      {/if}
    {/if}
  </div>

  {#if overview.missing > 0}
    <div class="tags-foot">
      <span>{tn("tags.missingEntries", overview.missing)}</span>
      <span class="spacer"></span>
      <button
        type="button"
        class="btn btn--sm"
        title={t("tags.cleanUpHint")}
        disabled={problem !== null}
        onclick={() => void tags.cleanUp()}
      >
        {t("tags.cleanUp")}
      </button>
    </div>
  {/if}
{/if}
