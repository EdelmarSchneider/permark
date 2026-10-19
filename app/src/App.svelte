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
  import Frame from "./core/shell/Frame.svelte";
  import Notices from "./core/shell/Notices.svelte";
  import SettingsPopover from "./core/shell/SettingsPopover.svelte";
  import Welcome from "./core/shell/Welcome.svelte";
  import WindowChoice from "./core/shell/WindowChoice.svelte";
  import { confirmLeave } from "./core/editing/guard";
  import { editing } from "./core/editing/session.svelte";
  import { installWindowKeys } from "./core/commands/install";
  import Finder from "./core/search/Finder.svelte";
  import SearchPanel from "./core/search/SearchPanel.svelte";
  import SidebarTabs from "./core/search/SidebarTabs.svelte";
  import KeymapDialog from "./core/shell/KeymapDialog.svelte";
  import { ui } from "./core/shell/ui.svelte";
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
        return t("app.name");
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

  // external I/O only: window title, system theme + language watchers
  $effect(() => {
    void api.setTitle(title);
  });
  $effect(() => look.watchSystem());
  // the window was asked to close with unsaved edits: the person answers, then it closes for real
  $effect(() => {
    let off: (() => void) | null = null;
    let gone = false;
    void api
      .on("close-requested", (event) => {
        if (event.window !== store.label) return;
        void confirmLeave({ kind: "window" }).then((ok) => {
          if (ok) void api.windowClose();
        });
      })
      .then((stop) => {
        if (gone) stop();
        else off = stop;
      });
    return () => {
      gone = true;
      off?.();
    };
  });

  async function closeWorkspace(): Promise<void> {
    if (await confirmLeave({ kind: "workspace" })) await store.closeWorkspace();
  }
  $effect(() => language.watchSystem());
</script>

{#snippet header()}
  {#if shown}
    <span
      class="title"
      title={shown.workspace.kind === "project" ? shown.workspace.root : shown.workspace.path}
    >
      {shown.workspace.name}
    </span>
  {:else}
    <span class="title">{t("app.name")}</span>
  {/if}
  <span class="spacer"></span>
  {#if shown}
    <button
      type="button"
      class="btn btn--ghost btn--sm"
      title={t("header.closeHint")}
      onclick={() => void closeWorkspace()}
    >
      {t("header.close")}
    </button>
  {/if}
  <button
    type="button"
    class="btn btn--ghost btn--sm"
    data-settings-trigger
    aria-expanded={ui.settings}
    onclick={() => ui.toggleSettings()}
  >
    {t("header.settings")}
  </button>
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
  <span class="spacer"></span>
  <StatusItems />
  <span>{versionText}</span>
{/snippet}

{#if store.boot.status !== "booting"}
  <Frame
    {header}
    sidebar={shown?.workspace.kind === "project" ? sidebar : undefined}
    panel={rightPanel !== null || (outline && reading.outline) ? panel : undefined}
    {main}
    {status}
  />
{/if}

{#if store.choice.status === "asking"}
  <WindowChoice answer={(choice) => void store.answerChoice(choice)} />
{/if}
{#if ui.settings}
  <SettingsPopover {store} close={() => ui.closeSettingsPopover()} />
{/if}
{#if ui.finder.status === "open"}
  {#key ui.finder.seed}
    <Finder preset={ui.finder.preset} />
  {/key}
{/if}
{#if ui.keyboard}
  <KeymapDialog {store} />
{/if}
<Notices {store} />
{#if store.drag.status === "over"}
  <div class="drop" aria-hidden="true">
    {store.drag.count === 1 ? t("welcome.dropActive") : t("welcome.dropOne")}
  </div>
{/if}

<style>
  .title {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 600;
  }
  .view {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
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
