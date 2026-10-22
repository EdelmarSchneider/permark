<script lang="ts">
  // The Tags tab of the sidebar: every tag with the number of files that have it, the documents without any, and the
  // files that have all the selected tags (Ctrl | Cmd + click adds a tag to the selection).
  import Palette from "@lucide/svelte/icons/palette";
  import X from "@lucide/svelte/icons/x";
  import { documents } from "../../core/doc/document.svelte";
  import { apiErrorText } from "../../core/errors";
  import { t, tn } from "../../core/i18n";
  import TagChip from "./TagChip.svelte";
  import TagStylePopover from "./TagStylePopover.svelte";
  import { tags } from "./store.svelte";

  let styling = $state<string | null>(null);
  let confirming = $state(false);

  const overview = $derived(tags.overview);
  const status = $derived(tags.status);
  const orphans = $derived(overview?.orphans ?? 0);
  const filtering = $derived(tags.selected.length > 0 || tags.untaggedOnly);
  const empty = $derived((overview?.tags.length ?? 0) === 0 && (overview?.untagged ?? 0) === 0);

  function split(path: string): { name: string; folder: string } {
    const at = path.lastIndexOf("/");
    return at < 0
      ? { name: path, folder: "" }
      : { name: path.slice(at + 1), folder: path.slice(0, at) };
  }

  async function cleanUp(): Promise<void> {
    confirming = false;
    await tags.cleanUp();
    await tags.load();
  }
</script>

<section class="tags" aria-label={t("tags.title")}>
  <header>
    <h2>{t("tags.title")}</h2>
    <span class="spacer"></span>
    {#if filtering}
      <button type="button" class="link" onclick={() => tags.clearSelection()}>
        {t("tags.clearSelection")}
      </button>
    {/if}
  </header>

  {#if status?.problem}
    <p class="notice warn" role="alert">
      {t("tags.metaUnreadable", { message: status.problem.message })}
      {status.problem.line !== null
        ? t("tags.metaUnreadableLine", { line: status.problem.line })
        : ""}
      {#if status.parked > 0}
        <br />{tn("tags.pendingChanges", status.parked)}
      {/if}
    </p>
  {:else if status?.writeError}
    <p class="notice warn" role="alert">{t("tags.writeError", { message: status.writeError })}</p>
  {/if}

  <div class="find">
    <input
      type="search"
      value={tags.filter}
      placeholder={t("tags.filter")}
      aria-label={t("tags.filter")}
      oninput={(e) => (tags.filter = e.currentTarget.value)}
    />
  </div>

  <div class="body">
    {#if overview === null}
      <p class="note">{t("tags.loading")}</p>
    {:else if empty}
      <p class="note">{t("tags.empty")}</p>
      <p class="note faint">{t("tags.emptyHint")}</p>
    {:else}
      {#if overview.untagged > 0}
        <button
          type="button"
          class="untagged"
          aria-pressed={tags.untaggedOnly}
          onclick={() => tags.pickUntagged()}
        >
          <span>{t("tags.untagged")}</span>
          <span class="n">{overview.untagged}</span>
        </button>
      {/if}
      <ul class="list">
        {#each tags.listed as item (item.name)}
          {@const style = tags.styleOf(item.name)}
          {@const mark = overview.marks ? tags.markOf(item) : null}
          <li>
            <TagChip
              name={item.name}
              color={style?.color}
              icon={style?.icon}
              status={mark?.status ?? null}
              statusWords={mark?.words ?? ""}
              count={item.count}
              active={tags.selected.includes(item.name)}
              onclick={(event) => tags.pick(item.name, event.ctrlKey || event.metaKey)}
              title={item.onlyInFiles
                ? `${tn("tags.countHint", item.count)} · ${t("tags.onlyInFilesHint")}`
                : tn("tags.countHint", item.count)}
            />
            <button
              type="button"
              class="icon"
              title={t("tags.style.title", { name: item.name })}
              aria-label={t("tags.style.title", { name: item.name })}
              aria-expanded={styling === item.name}
              onclick={() => (styling = styling === item.name ? null : item.name)}
            >
              <Palette size={13} />
            </button>
            {#if styling === item.name}
              <TagStylePopover
                tag={item.name}
                status={mark?.status ?? null}
                close={() => (styling = null)}
              />
            {/if}
          </li>
        {:else}
          <li class="note">{t("tags.noMatch", { query: tags.filter })}</li>
        {/each}
      </ul>
    {/if}

    {#if filtering}
      <h3>{t("tags.files")}</h3>
      {#if tags.listing.status === "loading"}
        <p class="note">{t("tags.loading")}</p>
      {:else if tags.listing.status === "failed"}
        <p class="note warn" role="alert">{apiErrorText(tags.listing.error)}</p>
      {:else if tags.listing.status === "ready"}
        {#if tags.listing.files.length === 0}
          <p class="note">{tags.untaggedOnly ? t("tags.noFilesUntagged") : t("tags.noFilesAll")}</p>
        {:else}
          <ul class="files">
            {#each tags.listing.files as file (file.path)}
              {@const parts = split(file.path)}
              <li>
                <button
                  type="button"
                  class="file"
                  title={file.path}
                  onclick={() => documents.navigate(file.path, null)}
                >
                  <span class="fname">{parts.name}</span>
                  {#if parts.folder !== ""}<span class="folder">{parts.folder}</span>{/if}
                  {#if file.description !== ""}<span class="desc">{file.description}</span>{/if}
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      {/if}
    {/if}
  </div>

  {#if orphans > 0}
    <footer>
      {#if confirming}
        <span class="ask">{tn("tags.missingEntries", orphans)}</span>
        <button type="button" class="btn btn--sm btn--primary" onclick={() => void cleanUp()}>
          {t("tags.cleanUp")}
        </button>
        <button type="button" class="btn btn--sm" onclick={() => (confirming = false)}>
          <X size={12} aria-hidden="true" />
          {t("common.cancel")}
        </button>
      {:else}
        <button
          type="button"
          class="link"
          title={t("tags.cleanUpHint")}
          onclick={() => (confirming = true)}
        >
          {t("tags.cleanUp")} ({orphans})
        </button>
      {/if}
    </footer>
  {/if}
</section>

<style>
  .tags {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
  }
  header {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    padding: var(--sp-2) var(--sp-3);
    border-bottom: 1px solid var(--border);
  }
  h2,
  h3 {
    margin: 0;
    font-size: var(--fs-ui);
    font-weight: 600;
  }
  h3 {
    margin-top: var(--sp-3);
    padding: var(--sp-3) var(--sp-3) var(--sp-1);
    border-top: 1px solid var(--border);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .spacer {
    flex: 1;
  }
  .link {
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--accent-text);
    font: inherit;
    font-size: var(--fs-ui-sm);
    cursor: pointer;
  }
  .link:hover {
    text-decoration: underline;
  }
  .find {
    padding: var(--sp-2) var(--sp-3);
  }
  .find input {
    width: 100%;
    padding: 4px var(--sp-2);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-ui-sm);
  }
  .find input:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
  }
  .body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding-bottom: var(--sp-3);
  }
  .note,
  .notice {
    margin: var(--sp-2) var(--sp-3);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .faint {
    color: var(--faint);
  }
  .warn {
    color: var(--git-deleted-text);
  }
  .untagged {
    display: flex;
    justify-content: space-between;
    width: calc(100% - 2 * var(--sp-3));
    margin: 0 var(--sp-3) var(--sp-1);
    padding: 3px var(--sp-2);
    border: 1px dashed var(--border-strong);
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-ui-sm);
    cursor: pointer;
  }
  .untagged[aria-pressed="true"] {
    border-style: solid;
    border-color: var(--accent);
    color: var(--text);
  }
  .n {
    font-variant-numeric: tabular-nums;
  }
  .list,
  .files {
    margin: 0;
    padding: 0 var(--sp-3);
    list-style: none;
  }
  .list li {
    position: relative;
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    padding: 2px 0;
  }
  .icon {
    display: inline-grid;
    place-items: center;
    width: 22px;
    height: 22px;
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--faint);
    opacity: 0;
    cursor: pointer;
  }
  .list li:hover .icon,
  .icon:focus-visible,
  .icon[aria-expanded="true"] {
    opacity: 1;
  }
  .icon:hover {
    background: var(--hover);
    color: var(--text);
  }
  .file {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    width: 100%;
    padding: 3px var(--sp-2);
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .file:hover {
    background: var(--hover);
  }
  .fname {
    font-size: var(--fs-ui);
  }
  .folder,
  .desc {
    max-width: 100%;
    overflow: hidden;
    color: var(--faint);
    font-size: var(--fs-ui-xs);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .desc {
    color: var(--muted);
  }
  footer {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    padding: var(--sp-2) var(--sp-3);
    border-top: 1px solid var(--border);
  }
  .ask {
    flex: 1;
    font-size: var(--fs-ui-sm);
  }
</style>
