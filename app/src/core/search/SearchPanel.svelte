<script lang="ts">
  // The Search panel (sidebar view): the words, the documents that hold them, the sections in each with the matched
  // words marked. All state is in the store; this draws it and turns events into store calls.
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import FileText from "@lucide/svelte/icons/file-text";
  import SearchIcon from "@lucide/svelte/icons/search";
  import X from "@lucide/svelte/icons/x";
  import { api } from "../../api";
  import { conditions } from "../conditions/context.svelte";
  import { contributions } from "../plugins/instance.svelte";
  import { documents } from "../doc/document.svelte";
  import { projects } from "../projects/instance.svelte";
  import Segments from "../shell/settings/Segments.svelte";
  import { apiErrorText } from "../errors";
  import { t, tn } from "../i18n";
  import { ui } from "../shell/ui.svelte";
  import PathText from "./PathText.svelte";
  import { SearchPanelStore } from "./panel.svelte";
  import SnippetText from "./SnippetText.svelte";
  import { searchTags } from "./tags.svelte";
  import type { SectionHit } from "../../api";

  type Props = { root: string; store?: SearchPanelStore };
  let {
    root,
    store = new SearchPanelStore(api, () => projects.ownerOf(documents.path)?.uid ?? null),
  }: Props = $props();

  let input: HTMLInputElement | undefined = $state();
  let list: HTMLElement | undefined = $state();

  const quiet = $derived(conditions.current.plugins.includes("quiet"));
  /** The scope choice shows only where the folder includes projects. */
  const scoped = $derived(projects.members.size > 0);
  /** The project the open document belongs to ("This project"). */
  const owner = $derived.by(() => {
    // `path` follows the person's navigation (also to a document that failed to show); `state` changes with it
    void documents.state;
    return projects.ownerOf(documents.path);
  });

  // "This project" follows the open document: another project's document asks again; none -> everywhere
  let seenOwner: string | null = null;
  $effect(() => {
    const uid = owner?.uid ?? null;
    if (uid === seenOwner) return;
    seenOwner = uid;
    if (store.scope !== "this") return;
    if (uid === null) store.setScope("all");
    else store.retry();
  });
  /** What the box searches, in its words (S2-5): plain folder -> as before; with projects -> the chosen scope. */
  const boxLabel = $derived(
    !scoped
      ? t("search.placeholder")
      : owner !== null && store.scope === "this"
        ? t("search.placeholderProject", { short: owner.short })
        : t("search.placeholderFolder"),
  );
  const filters = $derived(contributions.visible("searchFilter", conditions.current));

  // the tags the search is narrowed to changed: ask again
  let seenTags = searchTags.selected;
  $effect(() => {
    const now = searchTags.selected;
    if (now === seenTags) return;
    seenTags = now;
    store.retry();
  });
  const building = $derived(store.progress?.phase === "building");
  const activeKey = $derived(store.current?.key ?? null);
  const fileCount = $derived(store.groups.length);
  const hitCount = $derived(store.groups.reduce((sum, g) => sum + g.hits.length, 0));

  // listens to the index while the panel lives
  $effect(() => {
    let off: (() => void) | null = null;
    let gone = false;
    void store.start(root).then((stop) => {
      if (gone) stop();
      else off = stop;
    });
    return () => {
      gone = true;
      off?.();
    };
  });

  // Ctrl+Shift+F: the cursor goes to the box with its text selected
  $effect(() => {
    if (ui.searchFocus > 0 && ui.sidebar === "search") {
      input?.focus();
      input?.select();
    }
  });

  // the row the keyboard is on stays in view
  $effect(() => {
    const key = activeKey;
    if (key === null || !list) return;
    for (const el of list.querySelectorAll<HTMLElement>("[data-key]")) {
      if (el.dataset["key"] === key) el.scrollIntoView({ block: "nearest" });
    }
  });

  function open(hit: SectionHit): void {
    documents.navigate(hit.path, hit.anchor, hit.line);
  }

  function activate(): void {
    const row = store.current;
    if (row === null) return;
    if (row.type === "hit") open(row.hit);
    else store.toggle(row.path);
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.isComposing || e.keyCode === 229) return;
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        store.move(1);
        break;
      case "ArrowUp":
        e.preventDefault();
        store.move(-1);
        break;
      case "Enter":
        e.preventDefault();
        activate();
        break;
      case "Escape":
        if (store.query !== "") {
          e.preventDefault();
          store.clear();
        }
        break;
      default:
        break;
    }
  }
</script>

<section class="panel" aria-label={t("search.label")}>
  <div class="box">
    <SearchIcon size={14} aria-hidden="true" />
    <input
      bind:this={input}
      type="search"
      class="field"
      autocomplete="off"
      autocapitalize="off"
      spellcheck="false"
      placeholder={boxLabel}
      aria-label={scoped ? boxLabel : t("search.label")}
      value={store.query}
      oninput={(e) => store.setQuery(e.currentTarget.value)}
      {onkeydown}
    />
    {#if store.query !== ""}
      <button
        type="button"
        class="clear"
        title={t("search.clearHint")}
        aria-label={t("search.clear")}
        onclick={() => {
          store.clear();
          input?.focus();
        }}><X size={13} /></button
      >
    {/if}
  </div>

  {#each filters as filter (filter.owner + filter.id)}
    {#await filter.view() then loaded}
      {@const Filter = loaded.default}
      <Filter />
    {/await}
  {/each}

  {#if scoped}
    <div class="scope">
      <Segments
        label={t("search.scope.label")}
        value={owner === null ? "all" : store.scope}
        options={[
          { value: "all", label: t("search.scope.allProjects") },
          {
            value: "this",
            label: t("search.scope.thisProject", { short: owner?.short ?? "…" }),
            // why it is off, as things are now: no document open | the open one is in no project
            ...(owner === null
              ? {
                  why:
                    documents.path === null
                      ? t("search.scope.noDocument")
                      : t("search.scope.notInProject"),
                }
              : {}),
          },
        ]}
        disabledValues={owner === null ? ["this"] : []}
        onchange={(scope) => store.setScope(scope)}
      />
    </div>
  {/if}

  {#if quiet}
    <label class="option" title={t("search.includeQuietHint")}>
      <input
        type="checkbox"
        checked={store.includeSilenced}
        onchange={(e) => store.setIncludeSilenced(e.currentTarget.checked)}
      />
      {t("search.includeQuiet")}
    </label>
  {/if}

  {#if building && store.progress}
    <p class="note" role="status">
      {t("search.indexing", { indexed: store.progress.done, total: store.progress.total })}
    </p>
  {/if}

  <div class="results" role="region" aria-label={t("search.results")} bind:this={list}>
    {#if store.answer.status === "idle"}
      <p class="note">{t("search.empty")}</p>
    {:else if store.answer.status === "loading"}
      <p class="note" role="status">{t("search.searching")}</p>
    {:else if store.answer.status === "failed"}
      <div class="note" role="alert">
        <p>{t("search.error", { message: apiErrorText(store.answer.error) })}</p>
        <button type="button" class="btn btn--sm" onclick={() => store.retry()}>
          {t("search.retry")}
        </button>
      </div>
    {:else if hitCount === 0}
      <p class="note">
        {building ? t("search.noneYet") : t("search.none", { query: store.query.trim() })}
        {#if !building}<span class="hint">{t("search.noneHint")}</span>{/if}
      </p>
    {:else}
      <p class="summary" role="status">
        {t("search.summary", { hits: hitCount, files: tn("search.files", fileCount) })}
      </p>
      {#each store.rows as row (row.key)}
        {#if row.type === "file"}
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
          <div
            class="file"
            class:active={activeKey === row.key}
            data-key={row.key}
            role="button"
            tabindex="-1"
            aria-expanded={!store.collapsed.has(row.path)}
            onclick={() => store.toggle(row.path)}
          >
            {#if store.collapsed.has(row.path)}<ChevronRight size={13} />{:else}<ChevronDown
                size={13}
              />{/if}
            <FileText size={14} aria-hidden="true" />
            <span class="path" title={row.path}><PathText path={row.path} /></span>
            <span class="count">{row.count}</span>
          </div>
        {:else}
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
          <div
            class="hit"
            class:active={activeKey === row.key}
            data-key={row.key}
            role="button"
            tabindex="-1"
            onclick={() => open(row.hit)}
          >
            {#if row.hit.headingPath.length > 0}
              <span class="crumb">{row.hit.headingPath.join(" › ")}</span>
            {/if}
            <span class="snippet"><SnippetText parts={row.hit.snippet} /></span>
          </div>
        {/if}
      {/each}
    {/if}
  </div>
</section>

<style>
  .panel {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
  }
  .box {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    margin: var(--sp-2);
    padding: 0 var(--sp-2);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--muted);
  }
  .box:focus-within {
    border-color: var(--accent);
    box-shadow: 0 0 0 2px var(--accent-soft);
  }
  .field {
    flex: 1;
    min-width: 0;
    padding: 6px 0;
    border: 0;
    background: transparent;
    color: var(--text);
    font: inherit;
    outline: none;
  }
  .field::-webkit-search-cancel-button {
    display: none;
  }
  .clear {
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    padding: 0;
    border: 0;
    border-radius: 4px;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .clear:hover {
    background: var(--hover);
  }
  .scope {
    padding: 0 var(--sp-2) var(--sp-2);
  }
  .option {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    padding: 0 var(--sp-3) var(--sp-2);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .note {
    margin: var(--sp-2) var(--sp-3);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .hint {
    display: block;
    margin-top: var(--sp-1);
    color: var(--faint);
  }
  .summary {
    margin: 0 var(--sp-3) var(--sp-1);
    color: var(--faint);
    font-size: var(--fs-ui-xs);
  }
  .results {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding-bottom: var(--sp-3);
  }
  .file {
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    padding: 3px var(--sp-2);
    color: var(--text);
    cursor: pointer;
  }
  .file :global(svg) {
    flex: none;
    color: var(--muted);
  }
  .path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .count {
    flex: none;
    padding: 0 6px;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--muted);
    font-size: var(--fs-ui-xs);
  }
  .hit {
    display: block;
    padding: 3px var(--sp-3) 5px calc(var(--sp-5) + var(--sp-2));
    cursor: pointer;
  }
  .file:hover,
  .hit:hover {
    background: var(--hover);
  }
  .file.active,
  .hit.active {
    background: var(--accent-soft);
  }
  .crumb {
    display: block;
    color: var(--accent-text);
    font-size: var(--fs-ui-xs);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .snippet {
    display: -webkit-box;
    -webkit-line-clamp: 3;
    line-clamp: 3;
    -webkit-box-orient: vertical;
    overflow: hidden;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    word-break: break-word;
  }
</style>
