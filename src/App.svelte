<script lang="ts">
  import { onMount, untrack } from "svelte";
  import AgentPublisher from "./lib/components/AgentPublisher.svelte";
  import ImageZoom from "./lib/components/ImageZoom.svelte";
  import DropOverlay from "./lib/components/DropOverlay.svelte";
  import GitLegend from "./lib/components/GitLegend.svelte";
  import InitDialog from "./lib/components/InitDialog.svelte";
  import LinkUpdateDialog from "./lib/components/LinkUpdateDialog.svelte";
  import CommentField from "./lib/components/CommentField.svelte";
  import OutlinePopover from "./lib/components/OutlinePopover.svelte";
  import SearchEverywhere from "./lib/components/SearchEverywhere.svelte";
  import TagEditor from "./lib/components/TagEditor.svelte";
  import NotesShell from "./lib/components/NotesShell.svelte";
  import NotesVersions from "./lib/components/NotesVersions.svelte";
  import SelectionPopover from "./lib/components/SelectionPopover.svelte";
  import SettingsPopover from "./lib/components/SettingsPopover.svelte";
  import Shell from "./lib/components/Shell.svelte";
  import Welcome from "./lib/components/Welcome.svelte";
  import { language, t } from "./lib/i18n";
  import { isEditable } from "./lib/dom/domUtil";
  import { closeTopOverlay, hasKeyOverlay, hasOpenOverlay } from "./lib/overlays";
  import { setWindowTitle } from "./lib/platform";
  import {
    isMacPlatform,
    matchNotesShortcut,
    matchShortcut,
    yieldsToTyping,
  } from "./lib/shortcuts";
  import { app } from "./lib/stores/app.svelte";
  import { docs } from "./lib/stores/docs.svelte";
  import { edit } from "./lib/stores/edit.svelte";
  import { fileops } from "./lib/stores/fileops.svelte";
  import { comments } from "./lib/stores/comments.svelte";
  import { notes } from "./lib/stores/notes.svelte";
  import { session } from "./lib/stores/session.svelte";
  import { ui } from "./lib/stores/ui.svelte";
  import { unread } from "./lib/stores/unread.svelte";
  import { updates } from "./lib/stores/updates.svelte";
  import { vault } from "./lib/stores/vault.svelte";
  import { currentTitleState } from "./lib/stores/windowTitleState";
  import { windowTitle } from "./lib/windowTitle";

  const mac = typeof navigator !== "undefined" && isMacPlatform(navigator.userAgent);

  type MockPanel = typeof import("./lib/mock/MockControls.svelte").default;
  let MockControls = $state<MockPanel | null>(null);

  // follow the resolved theme (system / light / dark)
  $effect(() => {
    document.documentElement.dataset.theme = session.resolvedTheme;
  });

  // the page's language (screen readers, hyphenation), and the pieces of the open document that are drawn by code
  // (the copy button of a code block, the title of an alert) are drawn again in the new language
  let shownLocale = untrack(() => language.locale);
  $effect(() => {
    const locale = language.locale;
    document.documentElement.lang = locale;
    untrack(() => {
      if (locale === shownLocale) return;
      shownLocale = locale;
      docs.rerender();
    });
  });

  // The title of the window (the taskbar, Alt+Tab) tells windows apart: the document, the project, an unsaved mark. It is computed in
  // one place (`windowTitle.ts`); the derived string changes only when the title does, so the window is asked only then.
  const title = $derived(windowTitle(currentTitleState()));
  $effect(() => {
    void setWindowTitle(title);
  });

  // The reading rule needs the Markdown document on show and whether its reading view is up (not the editor, not
  // a tombstone); the stores below tell it when the window has the focus and when the person scrolls.
  $effect(() => {
    const view = docs.current;
    const onShow =
      app.screen === "project" && !vault.isSimple && view?.kind === "markdown" ? view.path : null;
    unread.watch(onShow, !docs.editing && !edit.active && docs.deleted === null);
  });

  // The comments are found again in a document each time it is drawn (opened, saved, changed by someone else).
  $effect(() => {
    void docs.request?.rev;
    untrack(() => comments.documentChanged());
  });

  onMount(() => {
    void app.boot();
    if (import.meta.env.VITE_MOCK === "1") {
      void import("./lib/mock/driver").then((driver) => {
        const quiet = new URLSearchParams(location.search).get("mock") === "quiet";
        driver.startMockDriver(quiet);
      });
      void import("./lib/mock/MockControls.svelte").then((m) => (MockControls = m.default));
    }
  });

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === "Escape") {
      if (closeTopOverlay()) {
        e.preventDefault();
        return;
      }
      // Esc also leaves editing, once menus and bars are closed (and not from some other text field)
      const target = e.target as Element | null;
      if (edit.active && (!isEditable(target) || target?.closest(".cm-editor, .tm-visual"))) {
        e.preventDefault();
        edit.requestExit();
      }
      return;
    }
    // the editor took the key (Ctrl+B is bold there, not the sidebar)
    if (e.defaultPrevented) return;
    if (app.screen === "notes" && notesKey(e)) return;
    const action = matchShortcut(e);
    if (!action) return;
    // "T" and "E" are plain letters, and Option+U and Option+Arrow are typing on macOS: they only mean something
    // while reading
    if (yieldsToTyping(action, mac) && (isEditable(e.target) || hasKeyOverlay())) {
      return;
    }
    e.preventDefault();
    app.run(action);
  }

  /** The keys that only the Notes window has: the pinned notes, stepping through the notes, renaming. Returns whether one was used. */
  function notesKey(e: KeyboardEvent): boolean {
    const key = matchNotesShortcut(e);
    if (key === null) return false;
    // an overlay (the search box, the versions dialog) keeps its own keys
    if (hasOpenOverlay() && key.kind !== "step") return false;
    e.preventDefault();
    if (key.kind === "pinned") void notes.openPinned(key.n);
    else if (key.kind === "step") void notes.step(key.direction);
    else if (notes.current && !notes.readOnly) notes.renaming = true;
    return true;
  }

  // mouse buttons 4 and 5 are back and forward (not in the Notes window, where a note is not a page to go back to)
  function onmouseup(e: MouseEvent): void {
    if (app.screen === "notes") return;
    if (e.button === 3) {
      e.preventDefault();
      void docs.back();
    } else if (e.button === 4) {
      e.preventDefault();
      void docs.forward();
    }
  }

  // the webview's own context menu (Reload, Inspect, ...) is not part of the app
  function oncontextmenu(e: MouseEvent): void {
    const target = e.target as HTMLElement | null;
    if (target?.closest("input, textarea")) return;
    if (!window.getSelection()?.isCollapsed) return;
    e.preventDefault();
  }
</script>

<svelte:window {onkeydown} {onmouseup} {oncontextmenu} />

{#if app.screen === "welcome"}
  <Welcome />
{:else if app.screen === "project"}
  <Shell />
  {#if ui.searchBox}<SearchEverywhere />{/if}
  {#if ui.settingsOpen}<SettingsPopover />{/if}
  {#if ui.initDialog}<InitDialog />{/if}
  <GitLegend />
  {#if fileops.prompt}<LinkUpdateDialog prompt={fileops.prompt} />{/if}
  {#if ui.tagEditor}<TagEditor />{/if}
  {#if ui.outlinePopover}<OutlinePopover />{/if}
  <SelectionPopover />
  {#if comments.draft}<CommentField />{/if}
  <AgentPublisher />
{:else if app.screen === "notes"}
  <NotesShell />
  {#if ui.searchBox}<SearchEverywhere />{/if}
  {#if ui.settingsOpen}<SettingsPopover />{/if}
  {#if ui.outlinePopover}<OutlinePopover />{/if}
  {#if notes.versions}<NotesVersions />{/if}
{:else}
  <div class="welcome" aria-busy="true" aria-label={t("app.loading")}></div>
{/if}

{#if ui.zoomImage}<ImageZoom src={ui.zoomImage} />{/if}
<DropOverlay />
{#if MockControls}<MockControls />{/if}
