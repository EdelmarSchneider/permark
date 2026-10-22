<script lang="ts">
  // The top of every window, one calm row: this window's tabs as chips, the Open button (folder, file, new project,
  // Notes, recent folders), the buttons plugins add (the Notes) and the settings. A window without tabs shows the app's name instead of the chips and the
  // Welcome page holds the actions.
  import Plus from "@lucide/svelte/icons/plus";
  import Settings from "@lucide/svelte/icons/settings";
  import { keyHint } from "../commands/hint";
  import { conditions } from "../conditions/context.svelte";
  import { contributions } from "../plugins/instance.svelte";
  import ContextMenu from "../explorer/ContextMenu.svelte";
  import type { MenuEntry } from "../explorer/menu";
  import { t } from "../i18n";
  import type { WindowStore } from "../window.svelte";
  import { newProject } from "./initProject";
  import Switcher from "./Switcher.svelte";
  import { tabs } from "./tabs.svelte";
  import { ui } from "./ui.svelte";

  type Props = {
    store: WindowStore;
    /** The notes plugin is on (the Open menu offers the Notes). */
    notes: boolean;
  };
  let { store, notes }: Props = $props();

  /** At most this many recent folders in the Open menu (the Welcome lists them all). */
  const RECENT_IN_MENU = 6;

  let openButton: HTMLButtonElement | undefined = $state();
  let menu = $state.raw<{ x: number; y: number } | null>(null);

  const settingsKeys = $derived(keyHint("app.settings"));
  /** Buttons plugins put left of Settings (the Notes); a plugin off: gone. */
  const items = $derived(
    contributions.visible("topBarItem", conditions.current).sort((a, b) => a.order - b.order),
  );

  const entries = $derived.by((): MenuEntry[] => {
    const hint = (id: string): string | undefined => keyHint(id) ?? undefined;
    const list: MenuEntry[] = [
      {
        kind: "item",
        label: t("welcome.openFolder"),
        hint: hint("app.openFolder"),
        run: () => void store.openFolder(),
      },
      {
        kind: "item",
        label: t("welcome.openFile"),
        hint: hint("app.openFile"),
        run: () => void store.openFile(),
      },
      { kind: "item", label: t("welcome.newProject"), run: () => void newProject(store) },
    ];
    if (notes) {
      list.push({
        kind: "item",
        label: t("notes.title"),
        // where they are already, else by the setting
        run: () => void store.openNotes(),
      });
    }
    const recent = store.recent.filter((r) => !r.missing).slice(0, RECENT_IN_MENU);
    if (recent.length > 0) {
      list.push({ kind: "separator" }, { kind: "heading", label: t("welcome.recent") });
      for (const folder of recent) {
        list.push({
          kind: "item",
          label: folder.name,
          detail: folder.path,
          // where it is already, else by the setting; Shift -> a new window
          run: (shift) => void store.open(folder.path, shift ? "newWindow" : undefined),
        });
      }
    }
    return list;
  });

  function toggleMenu(): void {
    if (menu) {
      menu = null;
      return;
    }
    // the recent folders first: the menu is placed (kept inside the window) with its final size
    void store.refreshRecent().then(() => {
      const box = openButton?.getBoundingClientRect();
      menu = { x: box?.left ?? 0, y: (box?.bottom ?? 0) + 4 };
    });
  }
</script>

<div class="top">
  {#if tabs.list.length > 0}
    <Switcher />
    <button
      bind:this={openButton}
      type="button"
      class="icon open"
      aria-label={t("top.open")}
      title={t("top.openHint")}
      aria-haspopup="menu"
      aria-expanded={menu !== null}
      onclick={toggleMenu}
    >
      <Plus size={16} strokeWidth={2} aria-hidden="true" />
    </button>
  {:else}
    <span class="brand">{t("app.name")}</span>
  {/if}
  <span class="spacer"></span>
  {#each items as item (item.owner + item.id)}
    {#await item.view() then loaded}
      {@const Item = loaded.default}
      <Item />
    {/await}
  {/each}
  <button
    type="button"
    class="icon"
    data-settings-trigger
    aria-label={t("header.settings")}
    title={settingsKeys === null
      ? t("header.settings")
      : `${t("header.settings")} (${settingsKeys})`}
    aria-expanded={ui.settings}
    onclick={() => ui.toggleSettings()}
  >
    <Settings size={16} strokeWidth={1.9} aria-hidden="true" />
  </button>
</div>
{#if menu}
  <ContextMenu
    x={menu.x}
    y={menu.y}
    {entries}
    close={() => (menu = null)}
    label={t("top.open")}
    anchor={openButton}
  />
{/if}

<style>
  .top {
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    width: 100%;
    min-width: 0;
    height: 100%;
    padding: 0 var(--sp-2);
  }
  .brand {
    padding: 0 var(--sp-2);
    color: var(--muted);
    font-weight: 600;
    letter-spacing: 0.01em;
  }
  .icon {
    display: grid;
    flex: none;
    place-items: center;
    width: 30px;
    height: 30px;
    padding: 0;
    border: 1px solid transparent;
    border-radius: 9px;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    transition:
      background-color var(--dur-fast) var(--ease),
      color var(--dur-fast) var(--ease);
  }
  .icon:hover,
  .icon[aria-expanded="true"] {
    background: var(--hover);
    color: var(--text);
  }
  .open {
    border: 1px dashed var(--border-strong);
  }
  .open:hover,
  .open[aria-expanded="true"] {
    border-style: solid;
  }
  @media (prefers-reduced-motion: reduce) {
    .icon {
      transition: none;
    }
  }
</style>
