<script lang="ts">
  // A small context menu at the pointer: arrows move, Enter runs, Escape | click outside closes. Entries are data.
  import { t } from "../i18n";
  import type { MenuEntry } from "./menu";

  type Props = { x: number; y: number; entries: readonly MenuEntry[]; close: () => void };
  let { x, y, entries, close }: Props = $props();

  let menu: HTMLElement | undefined = $state();
  let picked = $state(-1);

  const items = $derived(entries.flatMap((e, at) => (e.kind === "item" ? [at] : [])));
  /** The entry the keyboard is on: the one picked, else the first. */
  const active = $derived(picked >= 0 && items.includes(picked) ? picked : (items[0] ?? -1));

  function run(at: number): void {
    const entry = entries[at];
    close();
    if (entry?.kind === "item") entry.run();
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === "Escape") {
      e.preventDefault();
      close();
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const step = e.key === "ArrowDown" ? 1 : -1;
      const at = items.indexOf(active);
      picked = items[(at + step + items.length) % items.length] ?? active;
    } else if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      run(active);
    } else if (e.key === "Tab") {
      e.preventDefault();
    }
  }

  // placed at the pointer, kept inside the window; focus enters the menu and returns when it closes;
  // a click elsewhere closes it (imperative listeners, torn down on close)
  $effect(() => {
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    if (menu) {
      const box = menu.getBoundingClientRect();
      menu.style.left = `${Math.max(4, Math.min(x, window.innerWidth - box.width - 4))}px`;
      menu.style.top = `${Math.max(4, Math.min(y, window.innerHeight - box.height - 4))}px`;
      menu.focus();
    }
    const onPointer = (e: PointerEvent): void => {
      if (menu && e.target instanceof Node && !menu.contains(e.target)) close();
    };
    window.addEventListener("pointerdown", onPointer, true);
    return () => {
      window.removeEventListener("pointerdown", onPointer, true);
      before?.focus();
    };
  });
</script>

<div
  bind:this={menu}
  class="menu"
  role="menu"
  tabindex="-1"
  aria-label={t("menu.fileActions")}
  style:left="{x}px"
  style:top="{y}px"
  {onkeydown}
>
  {#each entries as entry, at (at)}
    {#if entry.kind === "separator"}
      <div class="sep" role="separator"></div>
    {:else}
      <button
        type="button"
        role="menuitem"
        class={["item", entry.danger && "danger", active === at && "is-active"]}
        onpointerenter={() => (picked = at)}
        onclick={() => run(at)}
      >
        <span>{entry.label}</span>
        {#if entry.hint}<kbd>{entry.hint}</kbd>{/if}
      </button>
    {/if}
  {/each}
</div>

<style>
  .menu {
    position: fixed;
    z-index: 55;
    min-width: 200px;
    padding: var(--sp-1);
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--bg);
    box-shadow: var(--shadow-pop);
    outline: none;
    animation: pm-fade var(--dur-fast) var(--ease);
  }
  .item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-4);
    width: 100%;
    padding: 5px 10px;
    border: 0;
    border-radius: 4px;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: var(--fs-ui);
    text-align: left;
    cursor: pointer;
  }
  .item.is-active {
    background: var(--hover);
  }
  .item.danger {
    color: var(--git-deleted-text);
  }
  kbd {
    color: var(--faint);
    font: inherit;
    font-size: var(--fs-ui-xs);
  }
  .sep {
    height: 1px;
    margin: var(--sp-1) 0;
    background: var(--border);
  }
</style>
