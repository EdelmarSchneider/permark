<script lang="ts">
  // The header of the Notes window: the title of the note (its first line), the Read | Visual | Source switch, Copy and a menu.
  import { tick } from "svelte";
  import { bridge } from "../bridge";
  import { keys, t } from "../i18n";
  import { docs } from "../stores/docs.svelte";
  import { edit } from "../stores/edit.svelte";
  import { fullPlace } from "../notesModel";
  import { notes } from "../stores/notes.svelte";
  import Icon from "./Icon.svelte";
  import IconButton from "./IconButton.svelte";
  import Menu, { type MenuEntry } from "./Menu.svelte";
  import ModeSwitch from "./ModeSwitch.svelte";

  let menuAnchor = $state<DOMRect | null>(null);
  let nameInput: HTMLInputElement | undefined = $state();
  let draft = $state("");
  let problem = $state<string | null>(null);

  const path = $derived(notes.current);
  const fresh = $derived(notes.unborn);
  const entry = $derived(notes.currentEntry);
  const title = $derived(entry && entry.title !== "" ? entry.title : t("notes.untitled"));
  const stem = $derived((path ?? "").replace(/\.(md|markdown)$/i, ""));
  const pinned = $derived(entry?.pinned === true);
  const shared = $derived(entry?.shared === true);
  const readOnly = $derived(notes.readOnly !== null);

  // asking for a name: the field opens with the current file name selected
  let asked = false;
  $effect(() => {
    if (notes.renaming && !asked && path) {
      asked = true;
      draft = stem;
      problem = null;
      void tick().then(() => {
        nameInput?.focus();
        nameInput?.select();
      });
    }
    if (!notes.renaming) asked = false;
  });

  async function commit(): Promise<void> {
    if (!path) return;
    if (draft.trim() === stem) {
      notes.renaming = false;
      return;
    }
    const failed = await notes.rename(path, draft);
    if (failed === null) notes.renaming = false;
    else problem = failed;
  }

  function onnamekeydown(e: KeyboardEvent): void {
    if (e.key === "Enter") {
      e.preventDefault();
      void commit();
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      notes.renaming = false;
      if (edit.active) bridge.editor?.focus();
    }
  }

  const isFile = $derived(entry?.kind === "file");
  const missing = $derived(entry?.missing === true);
  const unsaved = $derived(isFile && (entry?.unsaved === true || (edit.loose && edit.dirty)));

  // A loose file keeps its name, has no earlier versions here, and is taken out of the list rather than deleted: the file is
  // not ours.
  const entries = $derived<MenuEntry[]>(
    isFile
      ? [
          {
            id: "pin",
            label: pinned ? t("notes.unpin") : t("notes.pin"),
            icon: pinned ? "pin-off" : "pin",
            disabled: !path || readOnly,
            onselect: () => path && void notes.togglePin(path),
          },
          { type: "separator" },
          {
            id: "show",
            label: t("notes.file.showInFolder"),
            icon: "folder-open",
            disabled: !path || missing,
            onselect: () => path && void notes.revealFile(path),
          },
          {
            id: "locate",
            label: t("notes.file.locate"),
            icon: "folder-search",
            disabled: !path || readOnly,
            onselect: () => path && void notes.locateFile(path),
          },
          { type: "separator" },
          {
            id: "remove",
            label: t("notes.file.remove"),
            icon: "x",
            disabled: !path || readOnly,
            onselect: () => path && void notes.removeFile(path),
          },
          { type: "separator" },
          {
            id: "folder",
            label: t("notes.openFolder"),
            icon: "folder-open",
            onselect: () => void notes.openFolder(),
          },
        ]
      : [
          {
            id: "pin",
            label: pinned ? t("notes.unpin") : t("notes.pin"),
            icon: pinned ? "pin-off" : "pin",
            disabled: !path || fresh || readOnly,
            onselect: () => path && void notes.togglePin(path),
          },
          {
            id: "rename",
            label: t("notes.rename"),
            icon: "pencil",
            hint: "F2",
            disabled: !path || fresh || readOnly,
            onselect: () => (notes.renaming = true),
          },
          { type: "separator" },
          {
            id: "move",
            label: t("notes.moveToProject"),
            icon: "corner-down-right",
            hint: "Ctrl+Shift+S",
            disabled: !notes.movable,
            onselect: () => void notes.moveToProject(),
          },
          {
            id: "share",
            label: shared ? t("notes.unshare") : t("notes.share"),
            icon: "bot",
            disabled: !notes.movable,
            onselect: () => void notes.toggleShare(),
          },
          { type: "separator" },
          {
            id: "versions",
            label: t("notes.versionsMenu"),
            icon: "restore",
            disabled: !path || fresh,
            onselect: () => void notes.showVersions(),
          },
          { type: "separator" },
          {
            id: "delete",
            label: t("notes.delete"),
            icon: "trash",
            disabled: !path || fresh || readOnly,
            onselect: () => path && void notes.deleteNotes([path]),
          },
          { type: "separator" },
          {
            id: "folder",
            label: t("notes.openFolder"),
            icon: "folder-open",
            onselect: () => void notes.openFolder(),
          },
        ],
  );

  function toggleMenu(e: MouseEvent): void {
    menuAnchor = menuAnchor ? null : (e.currentTarget as HTMLElement).getBoundingClientRect();
  }
</script>

<header
  class={[
    "doc-header",
    "notes-header",
    docs.scrolled && "is-scrolled",
    edit.active && "is-editing",
  ]}
>
  {#if path}
    {#if notes.renaming && !fresh}
      <div class="notes-rename">
        <input
          bind:this={nameInput}
          class="text-input"
          type="text"
          bind:value={draft}
          aria-label={t("notes.renameLabel")}
          aria-invalid={problem !== null}
          placeholder={t("notes.renamePlaceholder")}
          spellcheck="false"
          autocomplete="off"
          onkeydown={onnamekeydown}
          onblur={() => void commit()}
        />
        <span class="notes-rename__ext">.md</span>
        {#if problem}<span class="notes-rename__problem" role="alert">{problem}</span>{/if}
      </div>
    {:else}
      <div
        class="notes-header__title"
        title={fresh
          ? t("notes.untitled")
          : isFile
            ? fullPlace(entry?.folder ?? null, title)
            : path}
      >
        <span class="notes-header__text">{title}</span>
        {#if pinned}<Icon name="pin" size="xs" class="notes-header__pin" />{/if}
        {#if shared && !isFile}
          <span class="notes-header__shared" title={t("notes.sharedMark")}>
            <Icon name="bot" size="xs" /><span class="sr-only">{t("notes.sharedMark")}</span>
          </span>
        {/if}
        {#if unsaved}
          <span class="notes-header__unsaved" title={t("notes.file.unsaved")}>
            <span class="note-row__dot" aria-hidden="true"></span>
            <span class="sr-only">{t("notes.file.unsaved")}</span>
          </span>
        {/if}
      </div>
    {/if}
  {/if}

  <span class="spacer"></span>

  <div class="actions">
    {#if path && isFile && edit.active && edit.dirty}
      <!-- a loose file is written when asked: this is how, with the key beside it -->
      <button
        type="button"
        class="btn btn--primary btn--sm"
        title={t("edit.saveHint")}
        onclick={() => void edit.save()}
      >
        {t("edit.save")}
      </button>
    {/if}
    {#if path && !missing}
      <button
        type="button"
        class="btn btn--ghost btn--sm"
        title={t("mode.withKeys", { hint: t("notes.copyHint"), keys: keys("Ctrl+Shift+C") })}
        onclick={() => void notes.copyNote()}
      >
        <Icon name="copy" size="sm" />
        <span>{t("notes.copy")}</span>
      </button>
      {#if docs.current || edit.active}<ModeSwitch />{/if}
    {/if}
    <IconButton
      icon="more"
      label={t("header.more")}
      hint={t("header.moreHint")}
      data-popover-trigger
      onclick={toggleMenu}
    />
  </div>
  {#if docs.loadingVisible}<div
      class="doc-header__loading"
      role="progressbar"
      aria-label={t("doc.loading")}
    ></div>{/if}
</header>

{#if menuAnchor}
  <Menu
    label={t("header.more")}
    anchor={menuAnchor}
    align="end"
    {entries}
    onclose={() => (menuAnchor = null)}
  />
{/if}
