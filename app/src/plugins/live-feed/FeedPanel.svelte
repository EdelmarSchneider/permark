<script lang="ts">
  // The right panel "Changes": what the disk reported, newest first. Click a row to open the file.
  import ArrowRightLeft from "@lucide/svelte/icons/arrow-right-left";
  import FilePen from "@lucide/svelte/icons/file-pen";
  import FilePlus from "@lucide/svelte/icons/file-plus";
  import FileX from "@lucide/svelte/icons/file-x";
  import Layers from "@lucide/svelte/icons/layers";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import type { FeedItem } from "../../api";
  import { documents } from "../../core/doc/document.svelte";
  import { t, tn } from "../../core/i18n";
  import { ui } from "../../core/shell/ui.svelte";
  import { ACTIVE_MS, feed } from "./store.svelte";
  import { ago, split } from "./time";

  let now = $state(Date.now());
  // "3 min ago" and the idle | active dot stay true while the panel is open
  $effect(() => {
    const timer = setInterval(() => (now = Date.now()), 5000);
    return () => clearInterval(timer);
  });

  const active = $derived(feed.lastAt > 0 && now - feed.lastAt < ACTIVE_MS);

  function open(item: FeedItem): void {
    if (item.item === "entry" && !item.isDir && item.kind !== "removed") {
      documents.navigate(item.path, null);
    }
  }

  const KINDS = {
    created: "feed.kind.created",
    modified: "feed.kind.modified",
    removed: "feed.kind.removed",
    renamed: "feed.kind.renamed",
  } as const;
</script>

<section class="feed" aria-label={t("live.label")}>
  <header>
    <span class="dot" class:active aria-hidden="true"></span>
    <h2>{t("live.label")}</h2>
    <span class="state">{active ? t("live.active") : t("live.idle")}</span>
    <span class="spacer"></span>
    <label class="docs" title={t("live.docsOnlyHint")}>
      <input type="checkbox" bind:checked={feed.docsOnly} />
      {t("live.docsOnly")}
    </label>
    <button
      type="button"
      class="icon"
      title={t("feed.clear")}
      aria-label={t("feed.clear")}
      onclick={() => void feed.clear()}
    >
      <RefreshCw size={14} />
    </button>
    <button
      type="button"
      class="icon close"
      aria-label={t("feed.close")}
      title={t("feed.close")}
      onclick={() => ui.toggleRight("live-feed.panel")}>×</button
    >
  </header>
  <ul>
    {#each feed.visible as item (item.id)}
      <li>
        {#if item.item === "entry"}
          {@const where = split(item.path)}
          <button
            type="button"
            class="row"
            class:gone={item.kind === "removed"}
            title={item.path}
            disabled={item.isDir || item.kind === "removed"}
            onclick={() => open(item)}
          >
            <span class="icon-cell kind-{item.kind}" title={t(KINDS[item.kind])}>
              {#if item.kind === "created"}<FilePlus size={15} />
              {:else if item.kind === "removed"}<FileX size={15} />
              {:else if item.kind === "renamed"}<ArrowRightLeft size={15} />
              {:else}<FilePen size={15} />{/if}
            </span>
            <span class="text">
              <span class="name">{where.name || t("feed.root")}</span>
              {#if where.folder}<span class="folder">{where.folder}</span>{/if}
              {#if item.kind === "renamed"}
                <span class="folder">{t("feed.renamedFrom", { name: split(item.from).name })}</span>
              {/if}
            </span>
            {#if item.own}<span class="tag">{t("feed.you")}</span>{/if}
            {#if item.saves > 1}<span class="tag">{tn("feed.saves", item.saves)}</span>{/if}
            <span class="when">{ago(item.atMs, now)}</span>
          </button>
        {:else if item.item === "group"}
          <div class="row group" title={item.sample.join("\n")}>
            <span class="icon-cell"><Layers size={15} /></span>
            <span class="text">
              <span class="name"
                >{t(item.reason === "git" ? "feed.bulk.git" : "feed.bulk.large", {
                  count: item.count,
                })}</span
              >
              <span class="folder"
                >{t(item.own ? "feed.bulk.own" : "feed.bulk.other")}
                {item.sample.slice(0, 3).join(", ")}{item.count > 3 ? "…" : ""}</span
              >
            </span>
            <span class="when">{ago(item.atMs, now)}</span>
          </div>
        {:else}
          <div class="row note">
            <span class="icon-cell"><RefreshCw size={15} /></span>
            <span class="text"><span class="name">{t("feed.bulk.resynced")}</span></span>
            <span class="when">{ago(item.atMs, now)}</span>
          </div>
        {/if}
      </li>
    {:else}
      <li class="empty">{t("feed.empty")}</li>
    {/each}
  </ul>
  {#if feed.hidden > 0}
    <p class="hidden">{tn("feed.hidden", feed.hidden)}</p>
  {/if}
</section>

<style>
  .feed {
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
  h2 {
    margin: 0;
    font-size: var(--fs-ui);
    font-weight: 600;
  }
  .state {
    color: var(--muted);
    font-size: var(--fs-ui-xs);
  }
  .spacer {
    flex: 1;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--muted);
    opacity: 0.5;
  }
  .dot.active {
    background: var(--accent);
    opacity: 1;
  }
  .docs {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--muted);
    font-size: var(--fs-ui-xs);
    cursor: pointer;
  }
  .icon {
    display: inline-grid;
    place-items: center;
    width: 24px;
    height: 24px;
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .icon:hover {
    background: var(--hover);
    color: var(--text);
  }
  .close {
    font-size: 16px;
    line-height: 1;
  }
  ul {
    flex: 1;
    min-height: 0;
    margin: 0;
    padding: var(--sp-1) 0;
    overflow-y: auto;
    list-style: none;
  }
  .row {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    width: 100%;
    padding: 5px var(--sp-3);
    border: 0;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: var(--fs-ui-sm);
    text-align: left;
  }
  button.row {
    cursor: pointer;
  }
  button.row:hover:not(:disabled) {
    background: var(--hover);
  }
  button.row:disabled {
    cursor: default;
  }
  .gone .name {
    text-decoration: line-through;
    color: var(--muted);
  }
  .icon-cell {
    flex: none;
    display: inline-grid;
    place-items: center;
    color: var(--muted);
  }
  .kind-created {
    color: var(--git-added-text);
  }
  .kind-modified {
    color: var(--git-modified-text);
  }
  .kind-removed {
    color: var(--git-deleted-text);
  }
  .kind-renamed {
    color: var(--git-renamed-text);
  }
  .text {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
  }
  .name,
  .folder {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .folder {
    color: var(--muted);
    font-size: var(--fs-ui-xs);
  }
  .tag {
    flex: none;
    padding: 0 6px;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--muted);
    font-size: var(--fs-ui-xs);
  }
  .when {
    flex: none;
    color: var(--muted);
    font-size: var(--fs-ui-xs);
  }
  .note .name {
    color: var(--muted);
    font-style: italic;
  }
  .empty,
  .hidden {
    margin: 0;
    padding: var(--sp-3) var(--sp-4);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
</style>
