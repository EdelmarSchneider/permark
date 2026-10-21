<script lang="ts">
  import { onMount } from "svelte";
  import { api, type AppInfo, type WorkspaceView } from "./api";
  import { language, t } from "./core/i18n";
  import DocumentArea from "./core/doc/DocumentArea.svelte";
  import { documents } from "./core/doc/document.svelte";
  import OutlinePanel from "./core/doc/OutlinePanel.svelte";
  import { reading } from "./core/doc/reading.svelte";
  import { look } from "./core/look.svelte";
  import ExplorerSidebar from "./core/explorer/ExplorerSidebar.svelte";
  import ImportAsk from "./core/explorer/ImportAsk.svelte";
  import ImportProgress from "./core/explorer/ImportProgress.svelte";
  import { importFlow } from "./core/explorer/importWindow.svelte";
  import DropBadge from "./core/shell/DropBadge.svelte";
  import { dropFlow } from "./core/shell/dropView.svelte";
  import OpenManyAsk from "./core/shell/OpenManyAsk.svelte";
  import FocusExit from "./core/shell/FocusExit.svelte";
  import Frame from "./core/shell/Frame.svelte";
  import PanelToggle from "./core/shell/PanelToggle.svelte";
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
  import { rightAvailable } from "./core/shell/layout";
  import { layout } from "./core/shell/layout.svelte";
  import { hasSidebar, rightSide } from "./core/shell/sides";
  import { tabs } from "./core/shell/tabs.svelte";
  import { ui } from "./core/shell/ui.svelte";
  import { conditions } from "./core/conditions/context.svelte";
  import { contributions } from "./core/plugins/instance.svelte";
  import { windowTitle } from "./core/title";
  import StatusItems from "./core/shell/StatusItems.svelte";
  import { activeRight, activeSidebar, leftPanels, rightPanels } from "./core/shell/panels.svelte";
  import { windowStore as store } from "./core/window.svelte";
  import { projects } from "./core/projects/instance.svelte";

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

  // files | folders dragged in from outside: this view's own system events (only the view on screen gets them)
  $effect(() => {
    let off: (() => void) | null = null;
    let gone = false;
    void api
      .onDrop((event) => dropFlow.event(event))
      .then((stop) => {
        if (gone) stop();
        else off = stop;
      });
    return () => {
      gone = true;
      off?.();
      dropFlow.reset();
    };
  });
  // the view left the screen (another tab came forward): the drag over it and its questions end
  $effect(() => {
    if (store.tab !== null && tabs.active !== null && tabs.active !== store.tab) dropFlow.hidden();
  });
  // a project's import: progress listened to while the project is shown; its question forgotten when it goes
  const projectShown = $derived(shown?.workspace.kind === "project" ? shown.workspace.id : null);
  $effect(() => {
    if (projectShown === null) return;
    let off: (() => void) | null = null;
    let gone = false;
    void importFlow.start().then((stop) => {
      if (gone) stop();
      else off = stop;
    });
    return () => {
      gone = true;
      off?.();
      importFlow.reset();
    };
  });

  // projects inside the shown folder: followed while it shows (`projects.md` §12)
  const projectRoot = $derived(shown?.workspace.kind === "project" ? shown.workspace.root : null);
  $effect(() => {
    const root = projectRoot;
    if (root === null) return;
    let off: (() => void) | null = null;
    let gone = false;
    void projects.start(root).then((stop) => {
      if (gone) stop();
      else off = stop;
    });
    return () => {
      gone = true;
      off?.();
    };
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
  <div class="slot left">
    {#if hasSidebar(store.view)}<PanelToggle side="sidebar" />{/if}
    <StatusItems slot="left" />
  </div>
  <div class="slot center"><StatusItems slot="center" /></div>
  <div class="slot right">
    <StatusItems slot="right" /><span>{versionText}</span>
    {#if shown !== null && rightAvailable(rightSide())}<PanelToggle side="right" />{/if}
  </div>
{/snippet}

{#if store.boot.status !== "booting"}
  <Frame
    sidebarWidth={layout.sidebar}
    rightWidth={layout.right}
    onSidebarWidth={(px) => layout.setSidebar(px)}
    onRightWidth={(px) => layout.setRight(px)}
    sidebarHidden={layout.sidebarOff}
    rightHidden={layout.rightOff}
    {top}
    sidebar={shown?.workspace.kind === "project" || shown?.workspace.kind === "notes"
      ? sidebar
      : undefined}
    panel={rightPanel !== null || (outline && reading.outline) ? panel : undefined}
    focus={ui.focus === "on"}
    moving={ui.frameMoving}
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
{:else}
  <!-- what a projects action did (an include undone, a removal kept), for screen readers -->
  <p class="sr-only" aria-live="polite">{projects.announcement}</p>
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
{#if importFlow.state.status === "asking" || (importFlow.state.status === "planning" && importFlow.state.slow)}
  {@const asked = importFlow.state}
  <ImportAsk
    at={asked.at}
    where={asked.where}
    plan={asked.status === "asking" ? asked.plan : null}
    conflicts={asked.status === "asking" ? asked.conflicts : "keepBoth"}
    setConflicts={(choice) => importFlow.setConflicts(choice)}
    confirm={(mode) => void importFlow.confirm(mode)}
    cancel={() => importFlow.cancel()}
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
{#if dropFlow.ask.status === "asking"}
  {@const many = dropFlow.ask}
  <OpenManyAsk
    at={many.at}
    count={many.paths.length}
    confirm={() => void dropFlow.confirmOpen()}
    cancel={() => dropFlow.cancelOpen()}
  />
{/if}
<Notices {store} />
{#if dropFlow.hover !== null}
  <DropBadge badge={dropFlow.hover.badge} at={dropFlow.hover.at} />
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
</style>
