<script lang="ts">
  import { onMount } from "svelte";
  import ImageZoom from "./lib/components/ImageZoom.svelte";
  import DropOverlay from "./lib/components/DropOverlay.svelte";
  import InitDialog from "./lib/components/InitDialog.svelte";
  import OutlinePopover from "./lib/components/OutlinePopover.svelte";
  import QuickOpen from "./lib/components/QuickOpen.svelte";
  import TagEditor from "./lib/components/TagEditor.svelte";
  import SelectionPopover from "./lib/components/SelectionPopover.svelte";
  import SettingsPopover from "./lib/components/SettingsPopover.svelte";
  import Shell from "./lib/components/Shell.svelte";
  import Welcome from "./lib/components/Welcome.svelte";
  import { t } from "./lib/i18n";
  import { isEditable } from "./lib/dom/domUtil";
  import { closeTopOverlay, hasOpenOverlay } from "./lib/overlays";
  import { matchShortcut } from "./lib/shortcuts";
  import { app } from "./lib/stores/app.svelte";
  import { docs } from "./lib/stores/docs.svelte";
  import { edit } from "./lib/stores/edit.svelte";
  import { session } from "./lib/stores/session.svelte";
  import { ui } from "./lib/stores/ui.svelte";
  import { updates } from "./lib/stores/updates.svelte";

  type MockPanel = typeof import("./lib/mock/MockControls.svelte").default;
  let MockControls = $state<MockPanel | null>(null);

  // follow the resolved theme (system / light / dark)
  $effect(() => {
    document.documentElement.dataset.theme = session.resolvedTheme;
  });

  onMount(() => {
    void app.boot();
    updates.scheduleStartupCheck();
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
      if (edit.active && (!isEditable(target) || target?.closest(".cm-editor"))) {
        e.preventDefault();
        edit.requestExit();
      }
      return;
    }
    // the editor took the key (Ctrl+B is bold there, not the sidebar)
    if (e.defaultPrevented) return;
    const action = matchShortcut(e);
    if (!action) return;
    // "T" and "E" are plain letters: they only mean something while reading
    const plainLetter = action === "editTags" || action === "edit";
    if (plainLetter && (isEditable(e.target) || hasOpenOverlay() || ui.find.open)) return;
    e.preventDefault();
    app.run(action);
  }

  // mouse buttons 4 and 5 are back and forward
  function onmouseup(e: MouseEvent): void {
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
  {#if ui.quickOpen}<QuickOpen />{/if}
  {#if ui.settingsOpen}<SettingsPopover />{/if}
  {#if ui.initDialog}<InitDialog />{/if}
  {#if ui.tagEditor}<TagEditor />{/if}
  {#if ui.outlinePopover}<OutlinePopover />{/if}
  <SelectionPopover />
{:else}
  <div class="welcome" aria-busy="true" aria-label={t("app.loading")}></div>
{/if}

{#if ui.zoomImage}<ImageZoom src={ui.zoomImage} />{/if}
<DropOverlay />
{#if MockControls}<MockControls />{/if}
