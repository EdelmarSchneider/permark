<script lang="ts">
  // The right panel "Changes": what the disk reported and what the person did in PerMark (with the hand-off marks),
  // newest first; a filter shows mine | outside | all. Click a row to open the file. Paths inside an included project
  // say which one ("{short} · {folder}"); the project list's own news (appeared, moved, gone) are rows too.
  import ArrowRightLeft from "@lucide/svelte/icons/arrow-right-left";
  import FilePen from "@lucide/svelte/icons/file-pen";
  import FilePlus from "@lucide/svelte/icons/file-plus";
  import FileX from "@lucide/svelte/icons/file-x";
  import FolderDot from "@lucide/svelte/icons/folder-dot";
  import FolderGit2 from "@lucide/svelte/icons/folder-git-2";
  import Layers from "@lucide/svelte/icons/layers";
  import MessageSquare from "@lucide/svelte/icons/message-square";
  import Radio from "@lucide/svelte/icons/radio";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Save from "@lucide/svelte/icons/save";
  import Send from "@lucide/svelte/icons/send";
  import Tag from "@lucide/svelte/icons/tag";
  import type { ActivityEntry, FeedItem, ProjectEvent } from "../../api";
  import { keyHint } from "../../core/commands/hint";
  import { documents } from "../../core/doc/document.svelte";
  import { revealInTree } from "../../core/explorer/current";
  import { t, tn } from "../../core/i18n";
  import { projects } from "../../core/projects/instance.svelte";
  import { middleCut, relativeTo } from "../../core/projects/model";
  import { ui } from "../../core/shell/ui.svelte";
  import { followStore } from "./followLink.svelte";
  import { mineLabel, pathOf, type FeedShow } from "./mine";
  import { feed, PANEL } from "./store.svelte";
  import { ago, split } from "./time";

  let now = $state(Date.now());
  // "3 min ago" and "Last change ..." stay true while the panel is open
  $effect(() => {
    const timer = setInterval(() => (now = Date.now()), 5000);
    return () => clearInterval(timer);
  });

  const follow = $derived(followStore());
  const followHint = $derived(keyHint("follow.toggle"));

  function open(item: FeedItem): void {
    if (item.item === "entry" && !item.isDir && item.kind !== "removed") {
      documents.navigate(item.path, null);
    }
  }

  function openMine(entry: ActivityEntry): void {
    const path = pathOf(entry.fact);
    if (path !== null && !gone(entry)) documents.navigate(path, null);
  }

  /** The action left nothing to open (a delete, the undo of a new file). */
  function gone(entry: ActivityEntry): boolean {
    return entry.fact.kind === "file" && entry.fact.op === "delete";
  }

  /** The folder line of a path: inside an included project "{short} · {folder in it}", else the folder as is. */
  function folderLine(path: string, folder: string): string {
    const owner = projects.ownerOf(path);
    if (owner === null) return folder;
    const inner = split(relativeTo(owner, path)).folder;
    return inner === ""
      ? owner.short
      : t("projects.feed.label", { short: owner.short, folder: inner });
  }

  /** The words of a project row; `cut`: long paths cut in the middle (the row), else whole (its tooltip). */
  function projectText(event: ProjectEvent, cut = false): string {
    const path = (p: string): string => (cut ? middleCut(p, 40) : p);
    switch (event.kind) {
      case "appeared":
        return t("projects.feed.appeared", {
          name: event.name,
          folder: event.path === "" ? (projects.view?.rootCard.name ?? "") : path(event.path),
        });
      case "moved":
        return t("projects.feed.moved", { name: event.name, path: path(event.path) });
      case "gone":
        return t("projects.feed.gone", { name: event.name });
      case "back":
        return t("projects.feed.back", { name: event.name, path: path(event.path) });
      case "listChanged":
        return t("projects.feed.listChanged");
      case "keptOut":
        return t("projects.feed.keptOut", { name: event.name });
      default:
        return event satisfies never;
    }
  }

  /** What a project row does (`null`: nothing): a project not found now -> its row in Settings (Locate… | Remove from
   *  list) | kept out by the person's no (Include there); the list changed -> Settings > Projects inside; else the
   *  folder shown in the Explorer. */
  function projectAction(event: ProjectEvent): (() => void) | null {
    if (event.kind === "listChanged") return () => projects.openSettings();
    const row = projects.rowAt(event.path);
    if (row?.state === "missing" || event.kind === "keptOut")
      return () => projects.openSettings(row);
    if (event.kind === "gone" && row === null) return null;
    return () => revealInTree(event.path);
  }

  function projectIsRepo(event: ProjectEvent): boolean {
    if (event.kind === "listChanged") return true;
    const row = projects.view?.rows.find((r) => r.path === event.path);
    return row?.marker !== "permark";
  }

  const SHOWS: readonly {
    id: FeedShow;
    label: "feed.show.all" | "feed.show.mine" | "feed.show.outside";
  }[] = [
    { id: "all", label: "feed.show.all" },
    { id: "mine", label: "feed.show.mine" },
    { id: "outside", label: "feed.show.outside" },
  ];

  const KINDS = {
    created: "feed.kind.created",
    modified: "feed.kind.modified",
    removed: "feed.kind.removed",
    renamed: "feed.kind.renamed",
  } as const;
</script>

<section class="feed" aria-label={t("live.label")}>
  <header>
    <h2>{t("live.label")}</h2>
    <span class="spacer"></span>
    {#if follow !== null}
      <button
        type="button"
        class="follow"
        aria-pressed={follow.on}
        title={followHint === null
          ? t("follow.toggle")
          : t("follow.buttonHint", { key: followHint })}
        onclick={() => follow?.toggle()}
      >
        <Radio size={13} aria-hidden="true" />
        {t("follow.button")}
      </button>
    {/if}
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
      onclick={() => ui.toggleRight(PANEL)}>×</button
    >
    <p class="last">
      {feed.lastAt > 0 ? t("live.last", { ago: ago(feed.lastAt, now) }) : t("live.none")}
    </p>
    <div class="show" role="radiogroup" aria-label={t("feed.show.label")}>
      {#each SHOWS as show (show.id)}
        <button
          type="button"
          role="radio"
          aria-checked={feed.show === show.id}
          class:on={feed.show === show.id}
          onclick={() => (feed.show = show.id)}>{t(show.label)}</button
        >
      {/each}
    </div>
    <label class="docs" title={t("live.docsOnlyHint")}>
      <input type="checkbox" bind:checked={feed.docsOnly} />
      {t("live.docsOnly")}
    </label>
  </header>
  <ul>
    {#each feed.rows as row (row.key)}
      <li>
        {#if row.kind === "mine"}
          {@const entry = row.entry}
          {#if entry.fact.kind === "handOff"}
            <div class="row handoff" role="separator">
              <span class="icon-cell"><Send size={15} /></span>
              <span class="text"
                ><span class="name">{tn("feed.handoff", entry.fact.rows)}</span></span
              >
              <span class="when">{ago(entry.at, now)}</span>
            </div>
          {:else}
            {@const label = mineLabel(entry.fact)}
            {@const where = split(pathOf(entry.fact) ?? "")}
            <button
              type="button"
              class="row mine"
              class:gone={gone(entry)}
              title={pathOf(entry.fact) ?? ""}
              disabled={gone(entry)}
              onclick={() => openMine(entry)}
            >
              <span class="icon-cell kind-mine">
                {#if entry.fact.kind === "save"}<Save size={15} />
                {:else if entry.fact.kind === "comment"}<MessageSquare size={15} />
                {:else if entry.fact.kind === "tag"}<Tag size={15} />
                {:else if entry.fact.kind === "convert" || entry.fact.kind === "review"}<ArrowRightLeft
                    size={15}
                  />
                {:else if entry.fact.kind === "file" && entry.fact.op === "delete"}<FileX
                    size={15}
                  />
                {:else}<FilePlus size={15} />{/if}
              </span>
              <span class="text">
                <span class="name">{where.name || t("feed.root")}</span>
                {#if label !== null}
                  <span class="folder"
                    >{t(label.key, label.params)}{label.detail
                      ? ` · ${label.detail}`
                      : ""}{label.undone ? ` (${t("feed.mine.undone")})` : ""}</span
                  >
                {/if}
              </span>
              <span class="tag">{t("feed.you")}</span>
              <span class="when">{ago(entry.at, now)}</span>
            </button>
          {/if}
        {:else if row.item.item === "entry"}
          {@const item = row.item}
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
              {#if where.folder || projects.ownerOf(item.path) !== null}<span class="folder"
                  >{folderLine(item.path, where.folder)}</span
                >{/if}
              {#if item.kind === "renamed"}
                <span class="folder">{t("feed.renamedFrom", { name: split(item.from).name })}</span>
              {/if}
            </span>
            {#if item.own}<span class="tag">{t("feed.you")}</span>{/if}
            {#if item.saves > 1}<span class="tag">{tn("feed.saves", item.saves)}</span>{/if}
            <span class="when">{ago(item.atMs, now)}</span>
          </button>
        {:else if row.item.item === "group"}
          {@const item = row.item}
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
        {:else if row.item.item === "project"}
          {@const event = row.item.event}
          {@const act = projectAction(event)}
          <button
            type="button"
            class="row"
            title={projectText(event)}
            disabled={act === null}
            onclick={() => act?.()}
          >
            <span class="icon-cell">
              {#if projectIsRepo(event)}<FolderGit2 size={15} />{:else}<FolderDot size={15} />{/if}
            </span>
            <!-- the whole sentence stays readable (the path cut in its middle, the line wraps) -->
            <span class="text"><span class="name wraps">{projectText(event, true)}</span></span>
            <span class="when">{ago(row.at, now)}</span>
          </button>
        {:else}
          <div class="row note">
            <span class="icon-cell"><RefreshCw size={15} /></span>
            <span class="text"><span class="name">{t("feed.bulk.resynced")}</span></span>
            <span class="when">{ago(row.at, now)}</span>
          </div>
        {/if}
      </li>
    {:else}
      <li class="empty">{t(feed.show === "mine" ? "feed.mine.empty" : "feed.empty")}</li>
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
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-1) var(--sp-2);
    padding: var(--sp-2) var(--sp-3);
    border-bottom: 1px solid var(--border);
  }
  h2 {
    margin: 0;
    white-space: nowrap;
    font-size: var(--fs-ui);
    font-weight: 600;
  }
  .spacer {
    flex: 1;
  }
  .follow {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 24px;
    padding: 0 var(--sp-2);
    border: 1px solid var(--border);
    border-radius: 999px;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-ui-xs);
    font-weight: 600;
    cursor: pointer;
  }
  .follow:hover {
    background: var(--hover);
    color: var(--text);
  }
  /* pressed = the look of the "Following" pill */
  .follow[aria-pressed="true"] {
    border-color: transparent;
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    color: var(--accent);
  }
  .last {
    flex-basis: 100%;
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-ui-xs);
  }
  .show {
    display: inline-flex;
    flex-basis: 100%;
    gap: 2px;
    padding: 2px;
    border-radius: var(--radius-control);
    background: var(--surface-2);
  }
  .show button {
    flex: 1;
    padding: 2px var(--sp-2);
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-ui-xs);
    cursor: pointer;
  }
  .show button.on {
    background: var(--bg);
    color: var(--text);
    box-shadow: 0 0 0 1px var(--border);
  }
  .docs {
    flex-basis: 100%;
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
  .name.wraps {
    white-space: normal;
    overflow-wrap: anywhere;
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
  .kind-mine {
    color: var(--accent);
  }
  .handoff {
    margin: var(--sp-1) 0;
    border-top: 1px dashed var(--border);
    border-bottom: 1px dashed var(--border);
    color: var(--muted);
  }
  .handoff .icon-cell {
    color: var(--accent);
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
