<script lang="ts">
  // The Comments tab of the sidebar: the open comments grouped by document, then the ones that are done. A comment
  // shown from the margin scrolls into view and is marked for a moment.
  import Send from "@lucide/svelte/icons/send";
  import { t, tn } from "../../core/i18n";
  import CommentRow from "./CommentRow.svelte";
  import { documentTitle } from "./model";
  import { comments } from "./store.svelte";

  const MARK_MS = 1800;
  let list: HTMLElement | undefined = $state();
  let marked = $state<string | null>(null);
  let doneOpen = $state(false);

  // a comment was asked for (a click in the margin): scroll to it, mark it for a moment
  $effect(() => {
    const request = comments.focus;
    if (!request || !list) return;
    const target = comments.all.find((c) => c.id === request.id);
    if (target?.done) doneOpen = true;
    marked = request.id;
    const timer = setTimeout(() => (marked = null), MARK_MS);
    const frame = requestAnimationFrame(() => {
      for (const el of list?.querySelectorAll<HTMLElement>("[data-comment]") ?? []) {
        if (el.dataset["comment"] === request.id) el.scrollIntoView({ block: "nearest" });
      }
    });
    return () => {
      clearTimeout(timer);
      cancelAnimationFrame(frame);
    };
  });
</script>

<section class="comments" aria-label={t("comments.list")}>
  <header>
    <h2>{t("comments.list")}</h2>
    {#if comments.count > 0}<span class="count">{comments.count}</span>{/if}
    <span class="spacer"></span>
    {#if comments.count > 0}
      <button
        type="button"
        class="send"
        title={t("comments.sendAllHint")}
        onclick={() => void comments.sendAllToAgent()}
      >
        <Send size={13} aria-hidden="true" />
        {t("comments.sendAll", { n: comments.count })}
      </button>
    {/if}
  </header>

  <div class="body" bind:this={list}>
    {#if !comments.ready}
      <p class="note">{t("comments.loading")}</p>
    {:else if comments.all.length === 0}
      <p class="note strong">{t("comments.empty.title")}</p>
      <p class="note">{t("comments.empty.text")}</p>
    {:else}
      {#if comments.count === 0}
        <p class="note">{t("comments.allDone")}</p>
      {/if}
      {#each comments.groups as group (group.path)}
        {@const title = documentTitle(group.path)}
        <h3 title={group.path} aria-label={t("comments.groupLabel", { path: group.path })}>
          <span class="name">{title.name}</span>
          {#if title.folder !== ""}<span class="folder">{title.folder}</span>{/if}
          <span class="n">{group.comments.length}</span>
        </h3>
        {#each group.comments as comment (comment.id)}
          <CommentRow {comment} marked={marked === comment.id} />
        {/each}
      {/each}

      {#if comments.done.length > 0}
        <details bind:open={doneOpen}>
          <summary>{tn("comments.done", comments.done.length)}</summary>
          {#each comments.done as comment (comment.id)}
            <CommentRow {comment} marked={marked === comment.id} />
          {/each}
        </details>
      {/if}
    {/if}
  </div>
</section>

<style>
  .comments {
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
  h2 {
    margin: 0;
    font-size: var(--fs-ui);
    font-weight: 600;
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
  .send {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 2px var(--sp-2);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: var(--fs-ui-xs);
    cursor: pointer;
  }
  .send:hover {
    background: var(--hover);
  }
  .body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding-bottom: var(--sp-3);
  }
  .note {
    margin: var(--sp-2) var(--sp-3);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .strong {
    color: var(--text);
    font-weight: 600;
  }
  h3 {
    display: flex;
    align-items: baseline;
    gap: var(--sp-2);
    margin: 0;
    padding: var(--sp-2) var(--sp-3) var(--sp-1);
    background: var(--surface-2);
    font-size: var(--fs-ui-sm);
    font-weight: 600;
  }
  .name {
    flex: none;
  }
  .folder {
    min-width: 0;
    overflow: hidden;
    color: var(--faint);
    font-size: var(--fs-ui-xs);
    font-weight: 400;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .n {
    margin-left: auto;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  details {
    margin-top: var(--sp-3);
    border-top: 1px solid var(--border);
  }
  summary {
    padding: var(--sp-2) var(--sp-3);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    cursor: pointer;
  }
</style>
