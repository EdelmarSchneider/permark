<script lang="ts">
  // The Unread tab of the sidebar: what changed since the person read it, the way to the next one, and what changed
  // since the project was last closed.
  import CheckCheck from "@lucide/svelte/icons/check-check";
  import ChevronsDown from "@lucide/svelte/icons/chevrons-down";
  import { documents } from "../../core/doc/document.svelte";
  import { language, t, tn } from "../../core/i18n";
  import OnlyProject from "../../core/projects/OnlyProject.svelte";
  import { nextUnread, unread } from "./store.svelte";

  const since = $derived(unread.since);
  /** The documents listed: all | one project's (a count on its detail card led here). */
  const list = $derived(unread.shown);
  const prefix = $derived(unread.only === null ? null : `${unread.only.folder}/`);
  const sinceItems = $derived(
    since === null || prefix === null
      ? (since?.items ?? [])
      : since.items.filter((i) => i.path.startsWith(prefix)),
  );

  // one project's documents last while the tab shows: back later, it lists all again
  $effect(() => () => unread.showOnly(null));
  const when = $derived(
    since && since.atMs > 0
      ? new Date(since.atMs).toLocaleString(language.locale, {
          dateStyle: "medium",
          timeStyle: "short",
        })
      : "",
  );

  function split(path: string): { name: string; folder: string } {
    const at = path.lastIndexOf("/");
    return at < 0
      ? { name: path, folder: "" }
      : { name: path.slice(at + 1), folder: path.slice(0, at) };
  }

  function open(path: string): void {
    documents.navigate(path, null);
  }

  function goNext(): void {
    const target = nextUnread(list, documents.path);
    if (target !== null) open(target);
  }
</script>

<section class="unread" aria-label={t("unread.title")}>
  <header>
    <h2>{t("unread.title")}</h2>
    {#if list.length > 0}<span class="count">{list.length}</span>{/if}
    <span class="spacer"></span>
    <button
      type="button"
      class="icon"
      title={t("unread.next")}
      aria-label={t("unread.next")}
      disabled={list.length === 0}
      onclick={goNext}
    >
      <ChevronsDown size={15} />
    </button>
    <button
      type="button"
      class="icon"
      title={t("unread.markAllRead")}
      aria-label={t("unread.markAllRead")}
      disabled={list.length === 0}
      onclick={() => void unread.markFolderRead(unread.only?.folder ?? "")}
    >
      <CheckCheck size={15} />
    </button>
  </header>
  {#if unread.only !== null}
    <OnlyProject name={unread.only.name} clear={() => unread.showOnly(null)} />
  {/if}
  <div class="body">
    {#if !unread.ready}
      <p class="note">{t("since.loading")}</p>
    {:else if list.length === 0}
      <p class="note caught-up">{t("unread.allCaughtUp")}</p>
    {:else}
      {#if list.length > 1}<p class="note order" id="unread-order">{t("unread.order")}</p>{/if}
      <ul aria-describedby={list.length > 1 ? "unread-order" : undefined}>
        {#each list as path (path)}
          {@const where = split(path)}
          <li>
            <button type="button" class="row" title={path} onclick={() => open(path)}>
              <span class="dot" aria-hidden="true"></span>
              <span class="name">{where.name}</span>
              {#if where.folder}<span class="folder">{where.folder}</span>{/if}
            </button>
            <button
              type="button"
              class="mark"
              title={t("unread.markRead")}
              aria-label={`${t("unread.markRead")}: ${where.name}`}
              onclick={() => void unread.markFolderRead(path)}>✓</button
            >
          </li>
        {/each}
      </ul>
    {/if}

    <h3>{t("since.title")}</h3>
    {#if since === null || !since.ready}
      <p class="note">{t("since.loading")}</p>
    {:else if since.first}
      <p class="note">{t("since.firstVisit")}</p>
    {:else if sinceItems.length === 0}
      <p class="note">{t("since.nothing", { when })}</p>
    {:else}
      <p class="note">{t("since.baseline", { when })}</p>
      <ul>
        {#each sinceItems as item (item.path)}
          {@const where = split(item.path)}
          <li>
            <button
              type="button"
              class="row"
              title={item.path}
              disabled={item.kind === "removed"}
              onclick={() => open(item.path)}
            >
              <span class="kind kind-{item.kind}">{t(`since.kind.${item.kind}`)}</span>
              <span class="name">{where.name}</span>
              {#if where.folder}<span class="folder">{where.folder}</span>{/if}
            </button>
          </li>
        {/each}
      </ul>
      {#if since.more > 0 && prefix === null}<p class="note">{tn("since.more", since.more)}</p>{/if}
    {/if}
  </div>
</section>

<style>
  .unread {
    display: flex;
    flex-direction: column;
    min-height: 0;
    flex: 1;
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
    padding: var(--sp-3) var(--sp-3) var(--sp-1);
    border-top: 1px solid var(--border);
    margin-top: var(--sp-3);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .count {
    padding: 0 6px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent) 16%, transparent);
    color: var(--accent);
    font-size: var(--fs-ui-xs);
    font-weight: 600;
  }
  .spacer {
    flex: 1;
  }
  .icon {
    display: inline-grid;
    place-items: center;
    width: 26px;
    height: 26px;
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .icon:hover:not(:disabled) {
    background: var(--hover);
    color: var(--text);
  }
  .icon:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding-bottom: var(--sp-3);
  }
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: flex;
    align-items: center;
  }
  li:hover .mark,
  .mark:focus-visible {
    opacity: 1;
  }
  .row {
    display: flex;
    flex: 1;
    align-items: center;
    gap: var(--sp-2);
    min-width: 0;
    padding: 5px var(--sp-3);
    border: 0;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: var(--fs-ui-sm);
    text-align: left;
    cursor: pointer;
  }
  .row:hover:not(:disabled) {
    background: var(--hover);
  }
  .row:disabled {
    cursor: default;
    color: var(--muted);
  }
  .dot {
    flex: none;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--accent);
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .folder {
    min-width: 0;
    overflow: hidden;
    color: var(--muted);
    font-size: var(--fs-ui-xs);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mark {
    flex: none;
    width: 24px;
    height: 24px;
    margin-right: var(--sp-2);
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--muted);
    opacity: 0;
    cursor: pointer;
  }
  .mark:hover {
    background: var(--hover);
    color: var(--text);
  }
  .kind {
    flex: none;
    padding: 0 6px;
    border-radius: 999px;
    font-size: var(--fs-ui-xs);
    font-weight: 600;
  }
  .kind-changed {
    background: color-mix(in srgb, var(--git-modified) 16%, transparent);
    color: var(--git-modified-text);
  }
  .kind-new {
    background: color-mix(in srgb, var(--git-added) 16%, transparent);
    color: var(--git-added-text);
  }
  .kind-removed {
    background: color-mix(in srgb, var(--git-deleted) 16%, transparent);
    color: var(--git-deleted-text);
  }
  .note {
    margin: 0;
    padding: var(--sp-2) var(--sp-3);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  /* the list's order, said once and quietly */
  .order {
    padding-bottom: 0;
  }
  .caught-up {
    color: var(--git-added-text);
  }
</style>
