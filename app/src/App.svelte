<script lang="ts">
  import { onMount } from "svelte";
  import { api, type AppInfo, type WorkspaceView } from "./api";
  import { language, t, tn } from "./core/i18n";
  import DocumentArea from "./core/doc/DocumentArea.svelte";
  import { documents } from "./core/doc/document.svelte";
  import OutlinePanel from "./core/doc/OutlinePanel.svelte";
  import { reading } from "./core/doc/reading.svelte";
  import { look } from "./core/look.svelte";
  import ExplorerSidebar from "./core/explorer/ExplorerSidebar.svelte";
  import ImportDialog from "./core/explorer/ImportDialog.svelte";
  import ImportProgress from "./core/explorer/ImportProgress.svelte";
  import { folderLabel } from "./core/explorer/importModel";
  import { importFlow } from "./core/explorer/importWindow.svelte";
  import FocusExit from "./core/shell/FocusExit.svelte";
  import Frame from "./core/shell/Frame.svelte";
  import Notices from "./core/shell/Notices.svelte";
  import DefaultAppDialog from "./core/shell/DefaultAppDialog.svelte";
  import { defaultApp } from "./core/shell/defaultApp.svelte";
  import InitDialog from "./core/shell/InitDialog.svelte";
  import SettingsPage from "./core/shell/settings/SettingsPage.svelte";
  import SettingsPopover from "./core/shell/SettingsPopover.svelte";
  import TopBar from "./core/shell/TopBar.svelte";
  import { closeHere } from "./core/shell/closeWorkspace";
  import Welcome from "./core/shell/Welcome.svelte";
  import CloseWindowDialog from "./core/shell/CloseWindowDialog.svelte";
  import { editing } from "./core/editing/session.svelte";
  import { installWindowKeys } from "./core/commands/install";
  import Finder from "./core/search/Finder.svelte";
  import SearchPanel from "./core/search/SearchPanel.svelte";
  import SidebarTabs from "./core/search/SidebarTabs.svelte";
  import KeymapDialog from "./core/shell/KeymapDialog.svelte";
  import { confirmInit } from "./core/shell/initProject";
  import { layout } from "./core/shell/layout.svelte";
  import { tabs } from "./core/shell/tabs.svelte";
  import { ui } from "./core/shell/ui.svelte";
  import { conditions } from "./core/conditions/context.svelte";
  import { contributions } from "./core/plugins/instance.svelte";
  import { windowTitle } from "./core/title";
  import StatusItems from "./core/shell/StatusItems.svelte";
  import { activeRight, activeSidebar, leftPanels, rightPanels } from "./core/shell/panels.svelte";
  import { windowStore as store } from "./core/window.svelte";

  type InfoState =
    { status: "loading" } | { status: "ready"; info: AppInfo } | { status: "failed" };

  let info = $state<InfoState>({ status: "loading" });

  /** What the window shows now; while opening, what stays on screen. */
  const shown = $derived.by((): { workspace: WorkspaceView; document: string | null } | null => {
    const view = store.view;
    switch (view.state) {
      case "showing":
        return { workspace: view.workspace, document: view.document };
      case "opening":
        return view.showing ? { workspace: view.showing, document: null } : null;
      case "welcome":
      case "closing":
        return null;
      default:
        return view satisfies never;
    }
  });

  const openingName = $derived.by(() => {
    const view = store.view;
    if (view.state !== "opening") return null;
    switch (view.target.kind) {
      case "project":
      case "singleFile":
        return (
          view.target.path
            .split(/[\\/]/)
            .filter((p) => p !== "")
            .at(-1) ?? view.target.path
        );
      case "notes":
        return t("notes.title");
      default:
        return view.target satisfies never;
    }
  });

  const versionText = $derived.by(() => {
    switch (info.status) {
      case "loading":
        return t("app.loading");
      case "ready":
        return t("status.version", { version: info.info.version });
      case "failed":
        return t("status.versionUnavailable");
      default:
        return info satisfies never;
    }
  });

  const title = $derived(windowTitle(store.view, editing.unsaved));

  /** What the notes plugin shows for the Notes workspace (none while the plugin is off). */
  const notesView = $derived(
    contributions.visible("workspaceView", conditions.current).find((v) => v.workspace === "notes"),
  );
  const notesOn = $derived(conditions.current.plugins.includes("notes"));
  const sidebarView = $derived(activeSidebar());
  const rightPanel = $derived(activeRight());

  /** The outline of the document shown, when it has one and the panel is on. */
  const outline = $derived.by(() => {
    const view = documents.view;
    return view?.body.kind === "markdown" && view.body.outline.length > 0
      ? view.body.outline
      : null;
  });

  onMount(() => {
    void api.appInfo().then((result) => {
      info = result.ok ? { status: "ready", info: result.value } : { status: "failed" };
    });
  });

  // the core's commands and keys live in the registry while the window does; one key listener runs them
  $effect(() => installWindowKeys());

  // external I/O only: window title, the native title bar's theme, system theme + language watchers
  $effect(() => {
    void api.setTitle(title);
  });
  $effect(() => {
    void api.setWindowTheme(look.preference === "system" ? null : look.preference);
  });
  // this window's tabs, for the top bar chips and their keys
  $effect(() => tabs.start(store.window));
  $effect(() => look.watchSystem());
  // the backend asks this view (events for other views are theirs): the window close prompt (tabs with unsaved
  // edits), a save before the window closes (Save all), the unsaved prompt before this tab closes
  $effect(() => {
    const offs: (() => void)[] = [];
    let gone = false;
    const keep = (stop: () => void): void => {
      if (gone) stop();
      else offs.push(stop);
    };
    void api
      .on("close-requested", (event) => {
        if (event.window === store.label) ui.closePrompt = event.tabs;
      })
      .then(keep);
    void api
      .on("save-requested", (event) => {
        if (event.window !== store.label) return;
        void editing.saveForClose().then((saved) => api.viewSaved(saved));
      })
      .then(keep);
    void api
      .on("tab-close-requested", (event) => {
        if (event.window === store.label) void closeHere();
      })
      .then(keep);
    // this tab moved to another window: its chips are that window's tabs now
    void api
      .on("view-moved", (event) => {
        if (event.window !== store.label) return;
        store.window = event.to;
        tabs.window = event.to;
      })
      .then(keep);
    return () => {
      gone = true;
      offs.forEach((off) => off());
    };
  });

  function answerClose(answer: "saveAll" | "discard" | "cancel"): void {
    ui.closePrompt = null;
    void api.windowCloseAnswer(answer);
  }
  $effect(() => language.watchSystem());

  // a project window takes what is dropped on it into the project (or into its editor); the flow listens to the
  // import's progress while the project is shown and forgets its question when another workspace comes
  const projectShown = $derived(shown?.workspace.kind === "project" ? shown.workspace.id : null);
  $effect(() => {
    if (projectShown === null) return;
    store.projectDrop = {
      over: (x, y, paths, count) => importFlow.over(x, y, paths, count),
      leave: () => importFlow.leave(),
      drop: (paths, x, y) => void importFlow.drop(paths, x, y),
    };
    let off: (() => void) | null = null;
    let gone = false;
    void importFlow.start().then((stop) => {
      if (gone) stop();
      else off = stop;
    });
    return () => {
      gone = true;
      off?.();
      store.projectDrop = null;
      importFlow.reset();
    };
  });

  /** What the dashed overlay says while a drag is over the window. */
  const dropText = $derived.by(() => {
    const hover = importFlow.hover;
    if (store.projectDrop === null || hover === null) {
      return store.drag.status === "over" && store.drag.count === 1
        ? t("welcome.dropActive")
        : t("welcome.dropOne");
    }
    if (hover.pictures) return t("import.overlayPictures");
    return tn("import.overlay", hover.count, {
      folder: folderLabel(hover.folder, shown?.workspace.name ?? ""),
    });
  });
  // first run: "Open these files with PerMark?" in the first window, once (the backend says when it is due)
  $effect(() => {
    if (store.label !== "") void defaultApp.askIfDue(store.label);
  });
</script>

{#snippet top()}
  <TopBar {store} notes={notesOn} />
{/snippet}

{#snippet sidebar()}
  {#if shown?.workspace.kind === "project"}
    {#key shown.workspace.id}
      <SidebarTabs />
      <!-- both views stay alive: the Explorer keeps its list and place while the Search shows -->
      <div class="view" class:off={sidebarView !== "files"} inert={sidebarView !== "files"}>
        <ExplorerSidebar {store} document={shown.document} />
      </div>
      <div class="view" class:off={sidebarView !== "search"} inert={sidebarView !== "search"}>
        <SearchPanel root={shown.workspace.root} />
      </div>
      {#each leftPanels() as plugin (plugin.owner + plugin.id)}
        <div class="view" class:off={sidebarView !== plugin.id} inert={sidebarView !== plugin.id}>
          {#await plugin.view() then loaded}
            {@const PanelView = loaded.default}
            <PanelView />
          {/await}
        </div>
      {/each}
    {/key}
  {:else if shown?.workspace.kind === "notes" && notesView}
    {#await notesView.sidebar() then loaded}
      {@const Sidebar = loaded.default}
      <Sidebar />
    {/await}
  {/if}
{/snippet}

{#snippet panel()}
  {#if rightPanel !== null}
    {#each rightPanels() as plugin (plugin.owner + plugin.id)}
      {#if plugin.id === rightPanel}
        {#await plugin.view() then loaded}
          {@const PanelView = loaded.default}
          <PanelView />
        {/await}
      {/if}
    {/each}
  {:else if outline}
    <OutlinePanel items={outline} />
  {/if}
{/snippet}

{#snippet main()}
  {#if shown?.workspace.kind === "project" || shown?.workspace.kind === "singleFile"}
    <DocumentArea workspace={shown.workspace} document={shown.document} {store} />
  {:else if shown?.workspace.kind === "notes"}
    {@const notes = shown.workspace}
    {#if notesView}
      {#await notesView.main() then loaded}
        {@const Main = loaded.default}
        <Main workspace={notes} />
      {/await}
    {:else}
      <div class="off"><p>{t("notes.off")}</p></div>
    {/if}
  {:else}
    <Welcome {store} />
  {/if}
  {#if openingName !== null}
    <div class="opening" role="status" aria-live="polite">
      {t("app.opening", { name: openingName })}
    </div>
  {/if}
{/snippet}

{#snippet status()}
  <div class="slot left"><StatusItems slot="left" /></div>
  <div class="slot center"><StatusItems slot="center" /></div>
  <div class="slot right"><StatusItems slot="right" /><span>{versionText}</span></div>
{/snippet}

{#if store.boot.status !== "booting"}
  <Frame
    sidebarWidth={layout.sidebar}
    rightWidth={layout.right}
    onSidebarWidth={(px) => layout.setSidebar(px)}
    onRightWidth={(px) => layout.setRight(px)}
    {top}
    sidebar={shown?.workspace.kind === "project" || shown?.workspace.kind === "notes"
      ? sidebar
      : undefined}
    panel={rightPanel !== null || (outline && reading.outline) ? panel : undefined}
    focus={ui.focus === "on"}
    moving={ui.focusMoving}
    {main}
    {status}
  />
  {#if ui.focus === "on"}
    <FocusExit />
  {/if}
{/if}

{#if ui.closePrompt !== null}
  <CloseWindowDialog tabs={ui.closePrompt} answer={answerClose} />
{/if}
{#if ui.settings}
  <SettingsPopover {store} close={() => ui.closeSettingsPopover()} />
{/if}
{#if ui.settingsPage.status === "open"}
  <SettingsPage {store} />
{/if}
{#if ui.initDialog.status === "asking"}
  {@const asked = ui.initDialog}
  <InitDialog
    name={asked.name}
    path={asked.path}
    answer={(yes) => (yes ? void confirmInit(asked.path) : ui.closeInit())}
  />
{/if}
{#if ui.finder.status === "open"}
  {#key ui.finder.seed}
    <Finder preset={ui.finder.preset} />
  {/key}
{/if}
{#if ui.keyboard}
  <KeymapDialog {store} />
{/if}
<DefaultAppDialog />
{#if importFlow.state.status === "asking"}
  {@const asked = importFlow.state}
  <ImportDialog
    plan={asked.plan}
    where={asked.where}
    mode={asked.mode}
    conflicts={asked.conflicts}
    setMode={(mode) => importFlow.setMode(mode)}
    setConflicts={(choice) => importFlow.setConflicts(choice)}
    confirm={() => void importFlow.confirm()}
    cancel={() => importFlow.cancelAsking()}
  />
{:else if importFlow.state.status === "running"}
  {@const running = importFlow.state}
  <ImportProgress
    where={running.where}
    mode={running.mode}
    progress={running.progress}
    cancelling={running.cancelling}
    stop={() => void importFlow.stop()}
  />
{/if}
<Notices {store} />
{#if store.drag.status === "over"}
  <div class="drop" aria-hidden="true">
    {dropText}
  </div>
{/if}

<style>
  .slot {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    min-width: 0;
  }
  .slot.left {
    flex: 0 1 auto;
    overflow: hidden;
  }
  .slot.center {
    flex: 1 1 0;
    justify-content: center;
    overflow: hidden;
  }
  .slot.right {
    flex: none;
    margin-left: auto;
  }
  .view {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
  }
  .off {
    display: grid;
    place-items: center;
    flex: 1;
    color: var(--muted);
  }
  .view.off {
    flex: none;
    height: 0;
    overflow: hidden;
    visibility: hidden;
  }
  .opening {
    position: absolute;
    top: var(--sp-3);
    left: 50%;
    transform: translateX(-50%);
    padding: 4px 12px;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--surface);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    animation: pm-fade var(--dur) var(--ease);
  }
  .drop {
    position: fixed;
    inset: 6px;
    z-index: 60;
    display: grid;
    place-items: center;
    border: 2px dashed var(--accent);
    border-radius: var(--radius-card);
    background: color-mix(in srgb, var(--bg) 82%, transparent);
    color: var(--accent);
    font-size: 15px;
    font-weight: 500;
    pointer-events: none;
    animation: pm-fade var(--dur) var(--ease);
  }
</style>
