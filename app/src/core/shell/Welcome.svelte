<script lang="ts">
  // Start screen: open folder | file, new project, recent folders (missing marked, removable; arrows + Enter +
  // Delete), blocks plugins add (the Notes) beside them, and where to drop a folder or a file.
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import FileText from "@lucide/svelte/icons/file-text";
  import FolderPlus from "@lucide/svelte/icons/folder-plus";
  import Import from "@lucide/svelte/icons/import";
  import type { RecentFolder } from "../../api";
  import { keyHint } from "../commands/hint";
  import { conditions } from "../conditions/context.svelte";
  import { language, t } from "../i18n";
  import { contributions } from "../plugins/instance.svelte";
  import { relativeTime } from "../time";
  import type { WindowStore } from "../window.svelte";

  type Props = { store: WindowStore };
  let { store }: Props = $props();

  let active = $state(0);
  let list: HTMLElement | undefined = $state();
  const now = Date.now();

  const recents = $derived(store.recent);
  /** Blocks plugins add beside the recent folders (the latest notes). */
  const sections = $derived(
    [...contributions.visible("welcomeSection", conditions.current)].sort(
      (a, b) => a.order - b.order,
    ),
  );
  const opening = $derived(store.view.state === "opening");

  /** `newWindow` (Shift+click | Shift+Enter): in a window of its own. */
  function open(recent: RecentFolder | undefined, newWindow = false): void {
    if (recent && !recent.missing)
      void store.open(recent.path, newWindow ? "newWindow" : undefined);
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.isComposing || recents.length === 0) return;
    switch (e.key) {
      case "ArrowDown":
      case "ArrowUp":
        e.preventDefault();
        active = (active + (e.key === "ArrowDown" ? 1 : -1) + recents.length) % recents.length;
        return;
      case "Enter":
        e.preventDefault();
        open(recents[active], e.shiftKey);
        return;
      case "Delete": {
        const recent = recents[active];
        if (!recent) return;
        e.preventDefault();
        void store.removeRecent(recent.path);
        active = Math.max(0, Math.min(active, recents.length - 2));
        return;
      }
      default:
        return;
    }
  }

  // keyboard works as soon as the screen shows (imperative DOM: focus once on mount)
  $effect(() => {
    list?.focus({ preventScroll: true });
  });

  const folderKeys = $derived(keyHint("app.openFolder"));
  const fileKeys = $derived(keyHint("app.openFile"));
</script>

<div class="welcome">
  <div class="top">
    <svg
      class="mark"
      viewBox="0 0 256 256"
      width="60"
      height="60"
      aria-hidden="true"
      focusable="false"
    >
      <path
        d="M70 22h116a22 22 0 0 1 22 22v190l-80-50-80 50V44a22 22 0 0 1 22-22z"
        fill="#f2542d"
      />
      <circle cx="104" cy="92" r="21" fill="#fff" />
      <circle cx="108" cy="97" r="11" fill="#1b1520" />
      <circle cx="112" cy="93" r="3.4" fill="#fff" />
      <circle cx="152" cy="92" r="21" fill="#fff" />
      <circle cx="156" cy="97" r="11" fill="#1b1520" />
      <circle cx="160" cy="93" r="3.4" fill="#fff" />
      <rect x="96" y="128" width="64" height="20" rx="10" fill="#ffe066" />
    </svg>
    <h1>{t("app.name")}</h1>
    <p class="tagline">{t("welcome.tagline")}</p>

    <div class="actions">
      <button
        type="button"
        class="btn btn--primary"
        disabled={opening}
        onclick={() => void store.openFolder()}
      >
        <FolderOpen size={14} aria-hidden="true" />
        {t("welcome.openFolder")}
        {#if folderKeys}<kbd>{folderKeys}</kbd>{/if}
      </button>
      <button type="button" class="btn" disabled={opening} onclick={() => void store.openFile()}>
        <FileText size={14} aria-hidden="true" />
        {t("welcome.openFile")}
        {#if fileKeys}<kbd>{fileKeys}</kbd>{/if}
      </button>
      <button
        type="button"
        class="btn"
        disabled={opening}
        title={t("welcome.newProjectHint")}
        onclick={() => void store.newProject()}
      >
        <FolderPlus size={14} aria-hidden="true" />
        {t("welcome.newProject")}
      </button>
    </div>
  </div>

  <div class="content" class:single={sections.length === 0}>
    <div class="columns" class:single={sections.length === 0}>
      <section class="column" aria-labelledby="recent-title">
        <h2 id="recent-title" class="menu-label">{t("welcome.recent")}</h2>
        <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
        <div
          class="recents"
          tabindex="0"
          role="listbox"
          aria-label={t("welcome.recent")}
          aria-activedescendant={recents[active] ? `recent-${active}` : undefined}
          bind:this={list}
          {onkeydown}
        >
          {#if recents.length === 0}
            <p class="empty">{t("welcome.noRecents")}</p>
          {:else}
            <ul>
              {#each recents as recent, i (recent.path)}
                <li
                  id={`recent-${i}`}
                  role="option"
                  aria-selected={i === active}
                  aria-disabled={recent.missing}
                  class={["row", i === active && "is-active", recent.missing && "is-missing"]}
                  title={recent.path}
                  onclick={(e) => {
                    active = i;
                    open(recent, e.shiftKey);
                  }}
                  onkeydown={() => {}}
                >
                  <FolderOpen size={15} aria-hidden="true" />
                  <span class="text">
                    <span class="name">{recent.name}</span>
                    <span class="path"
                      >{recent.path}{recent.missing ? ` ${t("welcome.notFound")}` : ""}</span
                    >
                  </span>
                  {#if recent.missing}
                    <button
                      type="button"
                      class="btn btn--ghost btn--sm"
                      onclick={(e) => {
                        e.stopPropagation();
                        void store.removeRecent(recent.path);
                      }}
                    >
                      {t("welcome.remove")}
                    </button>
                  {:else}
                    <span class="age">{relativeTime(recent.openedMs, now, language.locale)}</span>
                  {/if}
                </li>
              {/each}
            </ul>
          {/if}
        </div>
      </section>
      {#if sections.length > 0}
        <div class="column aside">
          {#each sections as section (section.owner + section.id)}
            {#await section.view() then loaded}
              {@const Section = loaded.default}
              <Section />
            {/await}
          {/each}
        </div>
      {/if}
    </div>

    <div class="drop-zone">
      <Import size={16} aria-hidden="true" />
      <span>{t("welcome.dropHint")}</span>
    </div>
  </div>
</div>

<style>
  .welcome {
    height: 100%;
    overflow: auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: clamp(var(--sp-5), 8vh, 96px) var(--sp-5) var(--sp-6);
  }
  .top {
    display: flex;
    flex-direction: column;
    align-items: center;
    width: 100%;
  }
  .mark {
    margin-bottom: var(--sp-3);
  }
  h1 {
    margin: 0;
    font-size: 28px;
    font-weight: 700;
    letter-spacing: -0.02em;
  }
  .tagline {
    margin: 6px 0 var(--sp-5);
    max-width: 560px;
    color: var(--muted);
    font-size: 14px;
    text-align: center;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 10px;
  }
  .actions .btn {
    height: 34px;
    padding: 0 14px;
    font-size: 13px;
  }
  kbd {
    font: 500 10.5px var(--font-ui);
    opacity: 0.7;
  }
  .content {
    width: min(940px, 100%);
    margin-top: var(--sp-6);
  }
  .content.single {
    width: min(600px, 100%);
  }
  .columns {
    display: grid;
    grid-template-columns: minmax(0, 1.5fr) minmax(0, 1fr);
    gap: var(--sp-6);
  }
  .columns.single {
    grid-template-columns: minmax(0, 1fr);
  }
  @media (max-width: 760px) {
    .columns {
      grid-template-columns: minmax(0, 1fr);
      gap: var(--sp-5);
    }
  }
  .column {
    min-width: 0;
  }
  .menu-label {
    margin: 0;
    padding-left: var(--sp-3);
  }
  .recents {
    margin-top: 4px;
    border-radius: var(--radius-control);
    outline: none;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .empty {
    margin: 0;
    padding: 6px var(--sp-3);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .row {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    align-items: center;
    gap: var(--sp-3);
    min-height: 40px;
    padding: 4px var(--sp-3);
    border-radius: var(--radius-control);
    color: var(--muted);
    cursor: pointer;
  }
  .row:hover {
    background: var(--hover);
  }
  .row.is-active {
    background: var(--accent-soft);
  }
  .row.is-missing {
    cursor: default;
  }
  .row.is-missing .text {
    opacity: 0.6;
  }
  .row :global(svg) {
    flex: none;
  }
  .text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .name {
    overflow: hidden;
    color: var(--text);
    font-weight: 500;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .path {
    overflow: hidden;
    color: var(--faint);
    font-size: var(--fs-ui-xs);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .age {
    color: var(--faint);
    font-size: var(--fs-ui-xs);
    white-space: nowrap;
  }
  .drop-zone {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--sp-2);
    margin-top: var(--sp-6);
    padding: var(--sp-4);
    border: 1.5px dashed var(--border-strong);
    border-radius: var(--radius-card);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    text-align: center;
  }
</style>
