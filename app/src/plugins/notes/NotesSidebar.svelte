<script lang="ts">
  // The list of the Notes window: New note, the filter, the notes grouped by when they were edited (pinned first), the
  // loose files under Files, and the banner when the folder is missing or cannot be written.
  import Ellipsis from "@lucide/svelte/icons/ellipsis";
  import FilePlus from "@lucide/svelte/icons/file-plus";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import type { NoteEntry } from "../../api";
  import ContextMenu from "../../core/explorer/ContextMenu.svelte";
  import type { MenuEntry } from "../../core/explorer/menu";
  import { apiErrorText } from "../../core/errors";
  import { t, tn, type MessageKey } from "../../core/i18n";
  import NoteRow from "./NoteRow.svelte";
  import { drawnOrder, shownName, type GroupId } from "./model";
  import { notes as store } from "./store.svelte";

  const GROUP_LABEL: Record<GroupId, MessageKey> = {
    today: "notes.group.today",
    yesterday: "notes.group.yesterday",
    week: "notes.group.week",
    month: "notes.group.month",
    older: "notes.group.older",
  };

  /** The clock the groups are drawn by; read again whenever the list changes. */
  const now = $derived.by(() => {
    void store.list;
    return Date.now();
  });
  const arranged = $derived(store.arranged(now));
  const rows = $derived(drawnOrder(arranged));
  const folder = $derived(store.folder);
  const writable = $derived(folder !== null && folder.status.status !== "missing");
  const copied = $derived(store.copiedCount);

  let menu = $state.raw<{ x: number; y: number; entries: readonly MenuEntry[] } | null>(null);
  const drag = $state<{ from: string | null }>({ from: null });
  let list: HTMLElement | undefined = $state();

  function entriesFor(entry: NoteEntry): MenuEntry[] {
    const item = (label: string, run: () => void, danger = false): MenuEntry => ({
      kind: "item",
      label,
      danger,
      run,
    });
    const out: MenuEntry[] = [
      item(
        entry.pinned ? t("notes.unpin") : t("notes.pin"),
        () => void store.togglePin(entry.path),
      ),
    ];
    if (entry.pinned && arranged.pinned.length > 1) {
      out.push(
        item(t("notes.moveUp"), () => void store.movePin(entry.path, -1)),
        item(t("notes.moveDown"), () => void store.movePin(entry.path, 1)),
      );
    }
    out.push(item(t("notes.rename"), () => (store.renaming = entry.path)));
    if (entry.kind === "note") {
      out.push(
        item(t("notes.versionsMenu"), () => void store.openVersions(entry.path)),
        item(
          entry.shared ? t("notes.unshare") : t("notes.share"),
          () => void store.toggleShare(entry.path),
        ),
        item(t("notes.copy"), () => void store.copyNote(entry.path)),
        item(t("notes.moveToProject"), () => void store.openMove(entry.path)),
      );
    } else {
      out.push(item(t("notes.file.openInProject"), () => void store.openInProject(entry.path)));
    }
    out.push(
      { kind: "separator" },
      item(t("menu.showInFileManager"), () => void store.reveal(entry.path)),
      item(t("menu.copyPath"), () => void store.copyPath(entry.path)),
      { kind: "separator" },
      entry.kind === "note"
        ? item(t("notes.delete"), () => void store.deleteNotes([entry.path]), true)
        : item(t("notes.file.remove"), () => void store.removeFile(entry.path), true),
    );
    return out;
  }

  function openMenu(entry: NoteEntry, x: number, y: number): void {
    menu = { x, y, entries: entriesFor(entry) };
  }

  function openNotesMenu(e: MouseEvent): void {
    const box =
      e.currentTarget instanceof HTMLElement ? e.currentTarget.getBoundingClientRect() : null;
    const entries: MenuEntry[] = [
      {
        kind: "item",
        label: t("notes.openFolder"),
        run: () => void store.reveal(""),
      },
    ];
    if (copied > 0) {
      entries.push({
        kind: "item",
        label: t("notes.deleteCopied", { count: copied }),
        danger: true,
        run: () => void store.deleteCopied(),
      });
    }
    menu = { x: box?.left ?? 0, y: (box?.bottom ?? 0) + 2, entries };
  }

  // arrows move through the rows; Enter on a row (a button) shows it
  function onlistkeys(e: KeyboardEvent): void {
    if (e.isComposing || (e.key !== "ArrowDown" && e.key !== "ArrowUp")) return;
    const buttons = Array.from(list?.querySelectorAll<HTMLElement>("button.open") ?? []);
    const at = buttons.indexOf(document.activeElement as HTMLElement);
    if (at < 0) return;
    e.preventDefault();
    buttons[(at + (e.key === "ArrowDown" ? 1 : -1) + buttons.length) % buttons.length]?.focus();
  }

  function onfilter(e: KeyboardEvent): void {
    if (e.isComposing) return;
    if (e.key === "Escape" && store.filter !== "") {
      e.preventDefault();
      store.filter = "";
    } else if (e.key === "Enter") {
      const first = rows[0];
      if (first) void store.select(first.path);
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      list?.querySelector<HTMLElement>("button.open")?.focus();
    }
  }

  const empty = $derived(store.list.status === "ready" && store.entries.length === 0);
</script>

<aside class="notes" aria-label={t("notes.title")}>
  <div class="top">
    <button
      type="button"
      class="btn btn--primary new"
      title={t("notes.newHint")}
      disabled={folder?.status.status === "missing"}
      onclick={() => void store.newNote()}
    >
      <FilePlus size={14} aria-hidden="true" />
      {t("notes.new")}
    </button>
    <button
      type="button"
      class="btn btn--ghost"
      title={t("notes.openFileHint")}
      aria-label={t("notes.openFile")}
      onclick={() => void store.chooseFile()}
    >
      <FolderOpen size={14} aria-hidden="true" />
    </button>
    <button
      type="button"
      class="btn btn--ghost"
      title={t("notes.menuHint")}
      aria-label={t("notes.menu")}
      onclick={openNotesMenu}
    >
      <Ellipsis size={14} aria-hidden="true" />
    </button>
  </div>
  <div class="filter">
    <input
      type="search"
      value={store.filter}
      placeholder={t("notes.filter")}
      aria-label={t("notes.filterLabel")}
      spellcheck="false"
      autocomplete="off"
      oninput={(e) => (store.filter = e.currentTarget.value)}
      onkeydown={onfilter}
    />
  </div>

  {#if folder?.status.status === "missing"}
    <div class="banner" role="alert" data-banner="missing">
      <p><strong>{t("notes.folderMissing", { path: folder.path })}</strong></p>
      <p class="faint">{t("notes.folderMissingHint")}</p>
      <div class="actions">
        <button type="button" class="btn btn--sm" onclick={() => void store.locateFolder()}>
          {t("notes.locate")}
        </button>
        <button type="button" class="btn btn--sm" onclick={() => void store.createFolder()}>
          {t("notes.createFolder")}
        </button>
      </div>
    </div>
  {:else if folder?.status.status === "readOnly"}
    <div class="banner" role="status" data-banner="readOnly">
      <p>{t("notes.readOnly.unwritable")}</p>
    </div>
  {/if}

  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="list"
    role="list"
    aria-label={t("notes.listLabel")}
    bind:this={list}
    onkeydown={onlistkeys}
  >
    {#if store.list.status === "loading"}
      <p class="faint pad">{t("app.loading")}</p>
    {:else if store.list.status === "failed"}
      <div class="pad" role="alert">
        <p>{t("notes.failed", { message: apiErrorText(store.list.error) })}</p>
        <button type="button" class="btn btn--sm" onclick={() => void store.reset()}>
          {t("notes.retry")}
        </button>
      </div>
    {:else if empty && writable}
      <p class="faint pad">{t("notes.empty")}</p>
    {:else if rows.length === 0 && store.filter !== ""}
      <p class="faint pad">{t("notes.noMatch")}</p>
    {:else}
      {#if arranged.pinned.length > 0}
        <div class="group-label">{t("notes.group.pinned")}</div>
        {#each arranged.pinned as entry (entry.path)}
          <NoteRow {store} {entry} {now} menu={openMenu} {drag} />
        {/each}
      {/if}
      {#each arranged.groups as group (group.id)}
        <div class="group-label">{t(GROUP_LABEL[group.id])}</div>
        {#each group.notes as entry (entry.path)}
          <NoteRow {store} {entry} {now} menu={openMenu} {drag} />
        {/each}
      {/each}
      {#if arranged.files.length > 0}
        <div class="group-label">{t("notes.group.files")}</div>
        {#each arranged.files as entry (entry.path)}
          <NoteRow {store} {entry} {now} menu={openMenu} {drag} />
        {/each}
      {/if}
    {/if}
  </div>
  <div class="count faint" aria-live="polite">
    {tn("notes.count", store.entries.filter((e) => e.kind === "note").length)}
  </div>
</aside>

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} entries={menu.entries} close={() => (menu = null)} />
{/if}

<style>
  .notes {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
  }
  .top {
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    padding: var(--sp-2);
  }
  .new {
    flex: 1;
    justify-content: center;
  }
  .filter {
    padding: 0 var(--sp-2) var(--sp-2);
  }
  .filter input {
    width: 100%;
    height: 26px;
    padding: 0 8px;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-ui);
  }
  .filter input:focus {
    outline: 2px solid var(--accent-muted);
    outline-offset: -1px;
  }
  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0 var(--sp-2) var(--sp-2);
  }
  .group-label {
    padding: 10px 8px 3px;
    color: var(--faint);
    font-size: var(--fs-ui-xs);
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
  }
  .pad {
    padding: var(--sp-3) var(--sp-2);
  }
  .faint {
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .banner {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    margin: 0 var(--sp-2) var(--sp-2);
    padding: var(--sp-2) var(--sp-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: color-mix(in srgb, var(--git-modified) 9%, var(--bg));
    font-size: var(--fs-ui-sm);
  }
  .banner p {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .actions {
    display: flex;
    gap: var(--sp-2);
  }
  .count {
    padding: var(--sp-1) var(--sp-3);
    border-top: 1px solid var(--border);
  }
</style>
