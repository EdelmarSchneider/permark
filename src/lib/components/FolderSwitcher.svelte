<script lang="ts">
  // The Projects menu: recent folders (this window's one marked), New project, Open folder, and Close folder when a
  // project is open. A folder's entry can be removed from the list (the folder itself is never touched). Opened from the
  // Projects button and from "Switch folder..." (see `app.openFolderSwitcher`); mounted while `ui.folderSwitcher` is set.
  import { onMount, tick } from "svelte";
  import { switcherRows, type SwitcherRow } from "../folderSwitcher";
  import { keys, t } from "../i18n";
  import { app } from "../stores/app.svelte";
  import { ui } from "../stores/ui.svelte";
  import { vault } from "../stores/vault.svelte";
  import Icon from "./Icon.svelte";
  import Popover, { type Anchor } from "./Popover.svelte";

  let { anchor }: { anchor: Anchor } = $props();

  let list: HTMLElement | undefined = $state();

  const rows = $derived(
    switcherRows({
      recents: app.recents,
      hidden: app.hiddenRecents,
      missing: app.missingRecents,
      current: vault.info ? { root: vault.info.root, name: vault.info.name } : null,
    }),
  );
  const hasOthers = $derived(rows.some((row) => !row.current));

  /** Every entry the arrow keys visit: the folders and the two actions (not the remove buttons, which Delete stands in for). */
  function items(): HTMLButtonElement[] {
    return Array.from(list?.querySelectorAll<HTMLButtonElement>("button[data-item]") ?? []);
  }

  onMount(() => {
    // the popover is positioned in an effect; focus once it is in place. The first folder to switch to is where the keyboard
    // starts, so Enter goes back to the folder that was open before this one.
    const timer = setTimeout(() => {
      const first =
        list?.querySelector<HTMLButtonElement>("button[data-item][data-switchable]") ??
        list?.querySelector<HTMLButtonElement>("button[data-item]");
      first?.focus();
    }, 0);
    return () => clearTimeout(timer);
  });

  function close(): void {
    ui.folderSwitcher = null;
  }

  function choose(row: SwitcherRow): void {
    // a folder that was not found cannot be opened: only its entry can be removed
    if (row.missing) return;
    close();
    void app.placeProject(row.root);
  }

  /** The entries that are folders (the rest are the two actions). */
  function folderItems(): HTMLButtonElement[] {
    return items().filter((item) => item.dataset.root !== undefined);
  }

  /**
   * Forgets the entry. The one it had is gone, with the focus: the keyboard carries on from the folder that takes its place, or
   * from the one above when it was the last.
   */
  async function remove(row: SwitcherRow): Promise<void> {
    const at = Math.max(
      0,
      folderItems().findIndex((item) => item.dataset.root === row.root),
    );
    app.removeRecent(row.root);
    await tick();
    const after = folderItems();
    (after[Math.min(at, after.length - 1)] ?? items()[0])?.focus();
  }

  function onkeydown(e: KeyboardEvent): void {
    const all = items();
    if (all.length === 0) return;
    const focused = document.activeElement as HTMLButtonElement | null;
    const i = focused ? all.indexOf(focused) : -1;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const step = e.key === "ArrowDown" ? 1 : -1;
      const next = i < 0 ? (step > 0 ? 0 : all.length - 1) : (i + step + all.length) % all.length;
      all[next].focus();
    } else if (e.key === "Home" || e.key === "End") {
      e.preventDefault();
      all[e.key === "Home" ? 0 : all.length - 1].focus();
    } else if (e.key === "Delete" && focused?.dataset.removable !== undefined) {
      const row = rows.find((r) => r.root === focused.dataset.root);
      if (row) {
        e.preventDefault();
        void remove(row);
      }
    }
  }
</script>

<Popover {anchor} variant="menu" label={t("switcher.menu")} onclose={close}>
  <div bind:this={list} class="switcher" role="presentation" {onkeydown}>
    <div class="menu-label">{t("welcome.recent")}</div>
    <div class="switcher__rows" role="presentation">
      {#each rows as row (row.root)}
        <div class={["switcher-row", row.missing && "is-missing"]} role="presentation">
          <button
            type="button"
            class="menu-item"
            role="menuitemradio"
            aria-checked={row.current}
            aria-disabled={row.missing ? true : undefined}
            title={row.root}
            data-item
            data-root={row.root}
            data-removable={row.current ? undefined : ""}
            data-switchable={row.current || row.missing ? undefined : ""}
            onclick={() => choose(row)}
          >
            <span class="menu-item__check">
              {#if row.current}<Icon name="check" size="sm" />{/if}
            </span>
            <span class="menu-item__label">
              {row.name}{row.missing ? ` ${t("welcome.notFound")}` : ""}
            </span>
            {#if row.parent !== ""}<span class="switcher-row__parent">{row.parent}</span>{/if}
          </button>
          {#if !row.current}
            <button
              type="button"
              class="switcher-row__remove"
              role="menuitem"
              aria-label={t("switcher.remove", { name: row.name })}
              title={t("switcher.removeHint")}
              onclick={() => void remove(row)}
            >
              <Icon name="x" size="xs" />
            </button>
          {/if}
        </div>
      {/each}
    </div>
    {#if !hasOthers}<div class="menu-note">{t("switcher.noOthers")}</div>{/if}
    <div class="menu-sep" role="separator"></div>
    <button
      type="button"
      class="menu-item"
      role="menuitem"
      data-item
      onclick={() => {
        close();
        void app.newProject();
      }}
    >
      <Icon name="folder-plus" size="sm" />
      <span class="menu-item__label">{t("cmd.newProject")}</span>
    </button>
    <button
      type="button"
      class="menu-item"
      role="menuitem"
      data-item
      onclick={() => {
        close();
        app.startNewFile(undefined, "markdown");
      }}
    >
      <Icon name="file-plus" size="sm" />
      <span class="menu-item__label">{t("cmd.newFile")}</span>
      <span class="menu-item__hint">{keys("Ctrl+N")}</span>
    </button>
    <button
      type="button"
      class="menu-item"
      role="menuitem"
      data-item
      onclick={() => {
        close();
        app.startNewFile(undefined, "text");
      }}
    >
      <Icon name="file-plus" size="sm" />
      <span class="menu-item__label">{t("cmd.newTextFile")}</span>
    </button>
    <button
      type="button"
      class="menu-item"
      role="menuitem"
      data-item
      onclick={() => {
        close();
        void app.chooseFolder();
      }}
    >
      <Icon name="folder-open" size="sm" />
      <span class="menu-item__label">{t("cmd.openFolder")}</span>
      <span class="menu-item__hint">{keys("Ctrl+O")}</span>
    </button>
    {#if app.screen === "project"}
      <button
        type="button"
        class="menu-item"
        role="menuitem"
        data-item
        onclick={() => {
          close();
          void app.closeFolder();
        }}
      >
        <Icon name="x" size="sm" />
        <span class="menu-item__label">{t("cmd.closeFolder")}</span>
      </button>
    {/if}
  </div>
</Popover>
