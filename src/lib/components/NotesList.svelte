<script lang="ts">
  // The list of the Notes window (docs/design/v0.5.md section 2): a filter box, the pinned notes in the person's order, then
  // the others grouped by when they were last edited. A note moves up the list when it is left, not while it is typed in.
  import { onMount, tick } from "svelte";
  import { trackPointerDrag } from "../dom/treeDrag";
  import { keys, t } from "../i18n";
  import { isComposingKey } from "../ime";
  import { GROUP_ORDER, drawnOrder, fullPlace, shortFolder, type GroupId } from "../notesModel";
  import { clock } from "../stores/clock.svelte";
  import { notes } from "../stores/notes.svelte";
  import type { NoteEntry } from "../types";
  import { relativeTime } from "../time";
  import Icon from "./Icon.svelte";
  import IconButton from "./IconButton.svelte";
  import Menu, { type MenuEntry } from "./Menu.svelte";

  let menuAnchor = $state<DOMRect | null>(null);
  let rowMenu = $state<{
    anchor: { left: number; right: number; top: number; bottom: number };
    path: string;
  } | null>(null);
  let filterBox: HTMLInputElement | undefined = $state();
  let scroller: HTMLElement | undefined = $state();
  /** A pinned row is being dragged: the row it started on and the one it is over. */
  let drag = $state<{ from: string; over: string | null } | null>(null);
  let afterDrag = false;

  const arranged = $derived(notes.arrangement(clock.now));
  const paths = $derived(drawnOrder(arranged).map((n) => n.path));
  const empty = $derived(notes.entries.length === 0);
  const nothingMatches = $derived(!empty && paths.length === 0);

  function groupLabel(id: GroupId): string {
    return t(`notes.group.${id}`);
  }

  function title(note: NoteEntry): string {
    return note.title === "" ? t("notes.untitled") : note.title;
  }

  // ---- keyboard ---------------------------------------------------------------------------------

  function rowElements(): HTMLElement[] {
    return Array.from(scroller?.querySelectorAll<HTMLElement>("[data-note]") ?? []);
  }

  function focusRow(path: string): void {
    void tick().then(() =>
      rowElements()
        .find((el) => el.dataset.note === path)
        ?.focus(),
    );
  }

  function onrowkeydown(e: KeyboardEvent, note: NoteEntry): void {
    if (isComposingKey(e)) return;
    if (e.target instanceof HTMLElement && e.target.closest("input")) return;
    if (e.altKey && !e.ctrlKey && !e.metaKey && (e.key === "ArrowUp" || e.key === "ArrowDown")) {
      // Alt+Up and Alt+Down change the order of the pinned notes
      e.preventDefault();
      if (note.pinned) {
        void notes.movePin(note.path, e.key === "ArrowUp" ? -1 : 1).then(() => focusRow(note.path));
      }
      return;
    }
    if (e.ctrlKey || e.metaKey || e.altKey || e.shiftKey) return;
    switch (e.key) {
      case "ArrowDown":
      case "ArrowUp": {
        e.preventDefault();
        const rows = rowElements();
        const at = rows.indexOf(e.currentTarget as HTMLElement);
        rows[
          Math.min(rows.length - 1, Math.max(0, at + (e.key === "ArrowDown" ? 1 : -1)))
        ]?.focus();
        break;
      }
      case "Enter":
      case " ":
        e.preventDefault();
        void notes.openNote(note.path);
        break;
      case "Delete":
        e.preventDefault();
        void notes.deleteNotes([note.path]);
        break;
      case "F2":
        e.preventDefault();
        // a loose file keeps the name it has: it is not ours to rename
        if (note.kind === "file") break;
        if (note.path === notes.current) notes.renaming = true;
        else void notes.openNote(note.path).then(() => (notes.renaming = true));
        break;
      case "Escape":
        if (notes.filter !== "") {
          e.preventDefault();
          notes.clearFilter();
        }
        break;
    }
  }

  function onfilterkeydown(e: KeyboardEvent): void {
    if (isComposingKey(e)) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      rowElements()[0]?.focus();
    } else if (e.key === "Enter") {
      e.preventDefault();
      const first = paths[0];
      if (first) void notes.openNote(first);
    } else if (e.key === "Escape" && notes.filter !== "") {
      e.preventDefault();
      e.stopPropagation();
      notes.clearFilter();
    }
  }

  // ---- reordering the pinned notes with the pointer ----------------------------------------------

  function onpinpointerdown(e: PointerEvent, note: NoteEntry): void {
    if (e.button !== 0 || e.ctrlKey || e.metaKey || e.shiftKey || e.altKey || notes.readOnly)
      return;
    const pinnedHere = (el: Element | null): string | null =>
      el?.closest<HTMLElement>("[data-pin]")?.dataset.note ?? null;
    trackPointerDrag(e, {
      begin: () => {
        drag = { from: note.path, over: null };
        afterDrag = true;
      },
      over: ({ folder }) => {
        if (drag && drag.over !== folder) drag = { ...drag, over: folder };
      },
      expand: () => {},
      end: (target) => {
        const from = drag?.from;
        drag = null;
        if (from && target !== null && target !== from) void notes.dropPin(from, target);
        // the click that follows a release comes at once; one that never comes must not swallow the next
        setTimeout(() => (afterDrag = false), 0);
      },
      folderAt: pinnedHere,
      scroller: () => scroller ?? null,
    });
  }

  function onrowclick(note: NoteEntry): void {
    if (afterDrag) return;
    void notes.openNote(note.path);
  }

  // ---- menus -------------------------------------------------------------------------------------

  function rowEntries(path: string): MenuEntry[] {
    const note = notes.entries.find((n) => n.path === path);
    if (!note) return [];
    if (note.kind === "file") return fileEntries(note);
    return [
      {
        id: "pin",
        label: note.pinned ? t("notes.unpin") : t("notes.pin"),
        icon: note.pinned ? "pin-off" : "pin",
        onselect: () => void notes.togglePin(path),
      },
      {
        id: "rename",
        label: t("notes.rename"),
        icon: "pencil",
        hint: "F2",
        onselect: () => {
          if (path === notes.current) notes.renaming = true;
          else void notes.openNote(path).then(() => (notes.renaming = true));
        },
      },
      { type: "separator" },
      {
        id: "copy",
        label: t("notes.copy"),
        icon: "copy",
        onselect: () => void notes.copyNote(path),
      },
      {
        id: "move",
        label: t("notes.moveToProject"),
        icon: "corner-down-right",
        disabled: notes.readOnly !== null,
        onselect: () => void notes.moveToProject(path),
      },
      {
        id: "share",
        label: note.shared ? t("notes.unshare") : t("notes.share"),
        icon: "bot",
        disabled: notes.readOnly !== null,
        onselect: () => void notes.toggleShare(path),
      },
      {
        id: "delete",
        label: t("notes.delete"),
        icon: "trash",
        hint: "Del",
        onselect: () => void notes.deleteNotes([path]),
      },
    ];
  }

  /** What can be done with a loose file in the list: it can be pinned, found again, shown, and taken out of the list. */
  function fileEntries(file: NoteEntry): MenuEntry[] {
    return [
      {
        id: "pin",
        label: file.pinned ? t("notes.unpin") : t("notes.pin"),
        icon: file.pinned ? "pin-off" : "pin",
        onselect: () => void notes.togglePin(file.path),
      },
      { type: "separator" },
      {
        id: "copy",
        label: t("notes.copy"),
        icon: "copy",
        disabled: file.missing,
        onselect: () => void notes.copyNote(file.path),
      },
      {
        id: "show",
        label: t("notes.file.showInFolder"),
        icon: "folder-open",
        disabled: file.missing,
        onselect: () => void notes.revealFile(file.path),
      },
      {
        id: "locate",
        label: t("notes.file.locate"),
        icon: "folder-search",
        onselect: () => void notes.locateFile(file.path),
      },
      { type: "separator" },
      {
        id: "remove",
        label: t("notes.file.remove"),
        icon: "x",
        hint: "Del",
        onselect: () => void notes.removeFile(file.path),
      },
    ];
  }

  const listEntries = $derived<MenuEntry[]>([
    {
      id: "deleteCopied",
      label: t("notes.deleteCopied", { count: notes.copiedCount }),
      icon: "trash",
      disabled: notes.copiedCount === 0 || notes.readOnly !== null,
      onselect: () => void notes.deleteCopied(),
    },
    { type: "separator" },
    {
      id: "folder",
      label: t("notes.openFolder"),
      icon: "folder-open",
      onselect: () => void notes.openFolder(),
    },
  ]);

  function oncontextmenu(e: MouseEvent, note: NoteEntry): void {
    e.preventDefault();
    rowMenu = {
      anchor: { left: e.clientX, right: e.clientX, top: e.clientY, bottom: e.clientY },
      path: note.path,
    };
  }

  function toggleMenu(e: MouseEvent): void {
    menuAnchor = menuAnchor ? null : (e.currentTarget as HTMLElement).getBoundingClientRect();
  }

  // Ctrl+Shift+D and Ctrl+F in the list move the keyboard here
  let focusedList = 0;
  $effect(() => {
    const tick_ = notes.listFocusTick;
    if (tick_ === focusedList) return;
    focusedList = tick_;
    const current = notes.current;
    const row = rowElements().find((el) => el.dataset.note === current) ?? rowElements()[0];
    (row ?? filterBox)?.focus();
  });
  let focusedFilter = 0;
  $effect(() => {
    const tick_ = notes.filterFocusTick;
    if (tick_ === focusedFilter) return;
    focusedFilter = tick_;
    filterBox?.focus();
    filterBox?.select();
  });

  onMount(() => {
    focusedList = notes.listFocusTick;
    focusedFilter = notes.filterFocusTick;
  });
</script>

<div class="notes-list">
  <div class="panel-header">
    <span class="panel-title">{t("notes.title")}</span>
    <span class="spacer"></span>
    <IconButton
      small
      icon="plus"
      label={t("notes.new")}
      hint={t("mode.withKeys", { hint: t("notes.newHint"), keys: keys("Ctrl+Shift+N") })}
      disabled={notes.readOnly !== null}
      onclick={() => void notes.newNote()}
    />
    <IconButton
      small
      icon="more"
      label={t("notes.menu")}
      hint={t("notes.menuHint")}
      data-popover-trigger
      onclick={toggleMenu}
    />
  </div>
  <div class="notes-filter">
    <Icon name="search" size="sm" class="notes-filter__icon" />
    <input
      bind:this={filterBox}
      class="text-input notes-filter__input"
      type="text"
      value={notes.filter}
      placeholder={t("notes.filter")}
      aria-label={t("notes.filterLabel")}
      spellcheck="false"
      autocomplete="off"
      oninput={(e) => notes.setFilter(e.currentTarget.value)}
      onkeydown={onfilterkeydown}
    />
    {#if notes.filter !== ""}
      <button
        type="button"
        class="notes-filter__clear"
        aria-label={t("notes.clearFilter")}
        title={t("notes.clearFilter")}
        onclick={() => {
          notes.clearFilter();
          filterBox?.focus();
        }}
      >
        <Icon name="x" size="xs" />
      </button>
    {/if}
  </div>

  <div
    bind:this={scroller}
    class="scroll notes-scroll"
    role="list"
    aria-label={t("notes.listLabel")}
  >
    {#if empty}
      <p class="panel-note">{t("notes.empty")}</p>
    {:else if nothingMatches}
      <p class="panel-note">{t("notes.noMatch")}</p>
    {/if}

    {#if arranged.pinned.length > 0}
      <div class="notes-group">
        <div class="notes-group__title">
          <Icon name="pin" size="xs" />{t("notes.group.pinned")}
        </div>
        {#each arranged.pinned as note (note.path)}
          {@render row(note, true)}
        {/each}
      </div>
    {/if}
    {#each GROUP_ORDER as id (id)}
      {@const group = arranged.groups.find((g) => g.id === id)}
      {#if group}
        <div class="notes-group">
          <div class="notes-group__title">{groupLabel(id)}</div>
          {#each group.notes as note (note.path)}
            {@render row(note, false)}
          {/each}
        </div>
      {/if}
    {/each}
    {#if arranged.files.length > 0}
      <div class="notes-group">
        <div class="notes-group__title">{t("notes.group.files")}</div>
        {#each arranged.files as note (note.path)}
          {@render row(note, false)}
        {/each}
      </div>
    {/if}
  </div>
</div>

{#snippet row(note: NoteEntry, pinned: boolean)}
  {@const current = note.path === notes.current}
  {@const file = note.kind === "file"}
  <div
    class={[
      "note-row",
      current && "is-current",
      file && "is-file",
      file && note.missing && "is-missing",
      drag?.over === note.path && drag.from !== note.path && "is-drop",
      drag?.from === note.path && "is-dragged",
    ]}
    role="listitem"
  >
    <button
      type="button"
      class="note-row__button"
      data-note={note.path}
      data-pin={pinned ? "" : undefined}
      aria-current={current ? "true" : undefined}
      title={file ? fullPlace(note.folder, note.title) : note.path}
      onclick={() => onrowclick(note)}
      onkeydown={(e) => onrowkeydown(e, note)}
      onpointerdown={pinned ? (e) => onpinpointerdown(e, note) : undefined}
      oncontextmenu={(e) => oncontextmenu(e, note)}
    >
      <span class="note-row__head">
        <span class={["note-row__title", note.title === "" && "is-untitled"]}>{title(note)}</span>
        {#if file && note.unsaved}
          <span class="note-row__unsaved" title={t("notes.file.unsaved")}>
            <span class="note-row__dot" aria-hidden="true"></span>
            <span class="sr-only">{t("notes.file.unsaved")}</span>
          </span>
        {/if}
        {#if note.shared && !file}
          <span class="note-row__shared" title={t("notes.sharedMark")}>
            <Icon name="bot" size="xs" /><span class="sr-only">{t("notes.sharedMark")}</span>
          </span>
        {/if}
        {#if note.copied}
          <span class="note-row__copied" title={t("notes.copiedMark")}>
            <Icon name="check" size="xs" /><span class="sr-only">{t("notes.copiedMark")}</span>
          </span>
        {/if}
        {#if !file}
          <span class="note-row__time">{relativeTime(note.modifiedMs, clock.now)}</span>
        {/if}
      </span>
      {#if file}
        <span class="note-row__snippet note-row__folder">
          {#if note.missing}<Icon name="alert" size="xs" />{t(
              "notes.file.notFound",
            )}{:else}{shortFolder(note.folder ?? "")}{/if}
        </span>
      {:else if note.snippet !== ""}
        <span class="note-row__snippet">{note.snippet}</span>
      {/if}
    </button>
  </div>
{/snippet}

{#if menuAnchor}
  <Menu
    label={t("notes.menu")}
    anchor={menuAnchor}
    align="end"
    entries={listEntries}
    onclose={() => (menuAnchor = null)}
  />
{/if}
{#if rowMenu}
  <Menu
    label={t("notes.menu")}
    anchor={rowMenu.anchor}
    entries={rowEntries(rowMenu.path)}
    onclose={() => (rowMenu = null)}
  />
{/if}
