<script lang="ts">
  // One row of the list: the title, the first words after it, when it was edited, and the marks it carries. Click
  // shows the note; the pointer's second button (or the dots) opens its menu; F2 renames; Delete deletes.
  import Copy from "@lucide/svelte/icons/copy";
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import Pin from "@lucide/svelte/icons/pin";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Share2 from "@lucide/svelte/icons/share-2";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import { t } from "../../core/i18n";
  import type { NoteEntry } from "../../api";
  import { marksOf, shortFolder, shownName, timeLabel } from "./model";
  import type { NotesStore } from "./store.svelte";

  type Props = {
    store: NotesStore;
    entry: NoteEntry;
    now: number;
    /** The row's menu is asked for at this place. */
    menu: (entry: NoteEntry, x: number, y: number) => void;
    drag: { from: string | null };
  };
  let { store, entry, now, menu, drag }: Props = $props();

  const marks = $derived(marksOf(entry));
  const current = $derived(store.current === entry.path);
  const renaming = $derived(store.renaming === entry.path);
  const title = $derived(entry.kind === "file" ? entry.name : entry.title);
  const shown = $derived(title === "" ? t("notes.untitled") : title);

  let draft = $state("");
  let problem = $state("");
  let field: HTMLInputElement | undefined = $state();

  // the name field opens with the stem selected (imperative DOM: focus and selection once, when it appears)
  $effect(() => {
    if (!renaming || !field) return;
    draft = entry.name;
    problem = "";
    field.focus();
    const dot = entry.name.lastIndexOf(".");
    field.setSelectionRange(0, dot > 0 ? dot : entry.name.length);
  });

  async function commit(): Promise<void> {
    const error = await store.rename(entry.path, draft);
    if (error === null) store.renaming = null;
    else problem = error;
  }

  function onfield(e: KeyboardEvent): void {
    if (e.isComposing) return;
    if (e.key === "Enter") {
      e.preventDefault();
      void commit();
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      store.renaming = null;
    }
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.isComposing || renaming) return;
    if (e.key === "F2") {
      e.preventDefault();
      store.renaming = entry.path;
    } else if (e.key === "Delete" && entry.kind === "note") {
      e.preventDefault();
      void store.deleteNotes([entry.path]);
    } else if (e.key === "ContextMenu" || (e.key === "F10" && e.shiftKey)) {
      e.preventDefault();
      const box =
        e.currentTarget instanceof HTMLElement ? e.currentTarget.getBoundingClientRect() : null;
      menu(entry, (box?.left ?? 0) + 24, (box?.top ?? 0) + 24);
    }
  }

  const tip = $derived(
    entry.kind === "file"
      ? `${entry.folder ?? ""} — ${entry.name}`
      : `${shownName(entry)} — ${entry.name}`,
  );
</script>

{#if renaming}
  <div class="row renaming">
    <input
      bind:this={field}
      bind:value={draft}
      type="text"
      aria-label={t("notes.renameLabel")}
      placeholder={t("notes.renamePlaceholder")}
      spellcheck="false"
      autocomplete="off"
      onkeydown={onfield}
      onblur={() => {
        if (problem === "") store.renaming = null;
      }}
    />
    {#if problem}<p class="problem" role="alert">{problem}</p>{/if}
  </div>
{:else}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="row"
    class:current
    class:gone={entry.state === "gone"}
    role="listitem"
    draggable={entry.pinned}
    ondragstart={(e) => {
      drag.from = entry.path;
      e.dataTransfer?.setData("text/plain", entry.path);
    }}
    ondragover={(e) => {
      if (drag.from !== null && entry.pinned) e.preventDefault();
    }}
    ondrop={(e) => {
      e.preventDefault();
      const from = drag.from;
      drag.from = null;
      if (from !== null && entry.pinned) void store.dropPin(from, entry.path);
    }}
    ondragend={() => (drag.from = null)}
    oncontextmenu={(e) => {
      e.preventDefault();
      menu(entry, e.clientX, e.clientY);
    }}
  >
    <button
      type="button"
      class="open"
      data-path={entry.path}
      aria-current={current ? "true" : undefined}
      title={tip}
      {onkeydown}
      onclick={() => void store.select(entry.path)}
    >
      <span class="line">
        <span class="title" class:untitled={title === ""}>{shown}</span>
        <span class="marks">
          {#if entry.pinned}<span title={t("notes.pinnedMark")}
              ><Pin size={11} aria-label={t("notes.pinnedMark")} /></span
            >{/if}
          {#if marks.shared}<span title={t("notes.sharedMark")}
              ><Share2 size={11} aria-label={t("notes.sharedMark")} /></span
            >{/if}
          {#if marks.copied}<span title={t("notes.copiedMark")}
              ><Copy size={11} aria-label={t("notes.copiedMark")} /></span
            >{/if}
          {#if marks.recovered}<span class="warn" title={t("notes.recoveredMark")}
              ><TriangleAlert size={11} aria-label={t("notes.recoveredMark")} /></span
            >{/if}
          {#if marks.unsaved}<span class="warn" title={t("notes.file.unsaved")}
              ><TriangleAlert size={11} aria-label={t("notes.file.unsaved")} /></span
            >{/if}
          {#if marks.changed}<span title={t("notes.file.changed")}
              ><RefreshCw size={11} aria-label={t("notes.file.changed")} /></span
            >{/if}
          {#if marks.gone}<span class="warn" title={t("notes.file.notFound")}
              ><TriangleAlert size={11} aria-label={t("notes.file.notFound")} /></span
            >{/if}
        </span>
        <span class="time"
          >{entry.state === "gone"
            ? t("notes.file.notFound")
            : timeLabel(entry.modifiedMs, now)}</span
        >
      </span>
      {#if entry.kind === "file"}
        <span class="snippet">{shortFolder(entry.folder ?? "")}</span>
      {:else if entry.snippet !== ""}
        <span class="snippet">{entry.snippet}</span>
      {/if}
    </button>
    <button
      type="button"
      class="dots"
      aria-label={t("notes.menu")}
      title={t("notes.menuHint")}
      onclick={(e) => {
        const box = e.currentTarget.getBoundingClientRect();
        menu(entry, box.left, box.bottom + 2);
      }}
    >
      <Ellipsis size={14} aria-hidden="true" />
    </button>
  </div>
{/if}

<style>
  .row {
    position: relative;
    display: flex;
    align-items: stretch;
    border-radius: var(--radius-control);
  }
  .row:hover {
    background: var(--hover);
  }
  .row.current {
    background: var(--accent-soft);
  }
  .open {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
    padding: 6px 8px;
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .open:focus-visible {
    outline: 2px solid var(--accent-muted);
    outline-offset: -2px;
  }
  .line {
    display: flex;
    align-items: baseline;
    gap: var(--sp-2);
    min-width: 0;
  }
  .title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    font-size: var(--fs-ui);
    font-weight: 500;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .title.untitled {
    color: var(--muted);
    font-style: italic;
    font-weight: 400;
  }
  .marks {
    display: inline-flex;
    gap: 3px;
    color: var(--muted);
  }
  .marks :global(svg) {
    display: block;
  }
  .warn {
    color: var(--git-modified-text);
  }
  .gone .title {
    color: var(--muted);
  }
  .time {
    flex: none;
    color: var(--faint);
    font-size: var(--fs-ui-xs);
  }
  .snippet {
    overflow: hidden;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dots {
    position: absolute;
    top: 4px;
    right: 2px;
    display: none;
    padding: 2px 4px;
    border: 0;
    border-radius: 4px;
    background: var(--surface-2);
    color: var(--muted);
    cursor: pointer;
  }
  .row:hover .dots,
  .dots:focus-visible {
    display: inline-flex;
  }
  .row:hover .time {
    visibility: hidden;
  }
  .renaming {
    flex-direction: column;
    gap: 2px;
    padding: 4px 6px;
  }
  .renaming input {
    height: 26px;
    padding: 0 6px;
    border: 1px solid var(--accent);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-ui);
  }
  .problem {
    margin: 0;
    color: var(--git-deleted-text);
    font-size: var(--fs-ui-xs);
  }
</style>
