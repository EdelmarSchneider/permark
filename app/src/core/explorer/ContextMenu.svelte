<script lang="ts">
  // A small context menu at the pointer: arrows | Home | End move the focus over the items (a disabled item is reached
  // and announced, it does nothing), Enter runs, Escape | click outside closes; an entry on | off on its own switches
  // by a click | Space and the menu stays for the next one. Entries are data; a heading names the group of the
  // entries under it. Taller than the window: it scrolls inside it, so every entry stays in reach; one that opens
  // upward is never taller than the room above its control, which stays in sight.
  import Check from "@lucide/svelte/icons/check";
  import { untrack } from "svelte";
  import { t } from "../i18n";
  import type { MenuEntry } from "./menu";

  type Props = {
    x: number;
    y: number;
    entries: readonly MenuEntry[];
    close: () => void;
    /** What the menu is called for assistive tech (default: the file actions). */
    label?: string;
    /** The button that opened it: a press on it is that button's own (it toggles the menu). */
    anchor?: HTMLElement;
    /** A submenu shown in place: ArrowLeft goes back to the menu it came from. */
    back?: () => void;
    /** The entry the keyboard starts on (default: the first item), e.g. the submenu entry when coming back. */
    start?: number;
    /** Opens upward: `y` is where its bottom goes (a control at the bottom of the window). */
    above?: boolean;
  };
  let { x, y, entries, close, label, anchor, back, start, above = false }: Props = $props();

  let menu: HTMLElement | undefined = $state();
  let picked = $state(-1);
  const uid = $props.id();

  type Loose = Exclude<MenuEntry, { kind: "heading" }>;
  /** What the menu draws, in order: an entry of its own, or a heading with the entries under it (up to the next
   *  heading | separator), drawn as one group the heading names. `at`: the place in `entries`. */
  type Part =
    | { kind: "one"; entry: Loose; at: number }
    | { kind: "group"; label: string; at: number; under: { entry: Loose; at: number }[] };
  const parts = $derived.by((): Part[] => {
    const out: Part[] = [];
    let group: Extract<Part, { kind: "group" }> | null = null;
    for (const [at, entry] of entries.entries()) {
      if (entry.kind === "heading") {
        group = { kind: "group", label: entry.label, at, under: [] };
        out.push(group);
      } else if (group !== null && entry.kind !== "separator") {
        group.under.push({ entry, at });
      } else {
        group = null;
        out.push({ kind: "one", entry, at });
      }
    }
    return out;
  });

  const items = $derived(entries.flatMap((e, at) => (e.kind === "item" ? [at] : [])));
  /** Some item has an icon: every item gets the icon column, so the labels line up. */
  const iconic = $derived(entries.some((e) => e.kind === "item" && e.icon !== undefined));
  /** The first item that can act: a disabled one before it would only start the keyboard on a dimmed row. */
  const firstLive = $derived(
    items.find((at) => {
      const entry = entries[at];
      return entry?.kind === "item" && entry.disabled !== true;
    }) ??
      items[0] ??
      -1,
  );
  /** The entry the keyboard is on: the one picked, else `start`, else the first that can act. */
  const active = $derived(
    picked >= 0 && items.includes(picked)
      ? picked
      : start !== undefined && items.includes(start)
        ? start
        : firstLive,
  );

  /** Runs the entry `at`. `stay`: an entry on | off on its own switches and the menu stays for the next one. */
  function run(at: number, shift = false, stay = false): void {
    const entry = entries[at];
    if (entry?.kind === "item" && entry.disabled === true) return;
    if (stay && entry?.kind === "item" && entry.checkbox === true) {
      entry.run(shift);
      return;
    }
    close();
    if (entry?.kind === "item") entry.run(shift);
  }

  /** The item `at` takes the keyboard focus (assistive tech follows the focus). */
  function focusItem(at: number): void {
    picked = at;
    menu?.querySelector<HTMLElement>(`[data-at="${at}"]`)?.focus();
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === "Escape") {
      e.preventDefault();
      close();
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const step = e.key === "ArrowDown" ? 1 : -1;
      const at = items.indexOf(active);
      focusItem(items[(at + step + items.length) % items.length] ?? active);
    } else if (e.key === "Home" || e.key === "End") {
      e.preventDefault();
      focusItem((e.key === "Home" ? items[0] : items[items.length - 1]) ?? active);
    } else if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      run(active, e.shiftKey, e.key === " ");
    } else if (e.key === "ArrowRight") {
      const entry = entries[active];
      if (entry?.kind === "item" && entry.opens === true) {
        e.preventDefault();
        run(active);
      }
    } else if (e.key === "ArrowLeft" && back) {
      e.preventDefault();
      back();
    } else if (e.key === "Tab") {
      e.preventDefault();
    }
  }

  // placed at the pointer, kept inside the window; focus enters the menu and returns when it closes;
  // a click elsewhere closes it (imperative listeners, torn down on close)
  $effect(() => {
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    if (menu) {
      // upward from a control at the bottom: no taller than the room above it, so the control stays in sight
      if (above) menu.style.maxHeight = `${Math.max(0, Math.min(window.innerHeight - 8, y - 4))}px`;
      const box = menu.getBoundingClientRect();
      menu.style.left = `${Math.max(4, Math.min(x, window.innerWidth - box.width - 4))}px`;
      const top = above ? y - box.height : y;
      menu.style.top = `${Math.max(4, Math.min(top, window.innerHeight - box.height - 4))}px`;
      // the first item (or `start`) takes the focus; the menu itself only when it has no item
      const first = untrack(() => active);
      const item = menu.querySelector<HTMLElement>(`[data-at="${first}"]`);
      if (item) item.focus();
      else menu.focus();
    }
    const onPointer = (e: PointerEvent): void => {
      if (!(e.target instanceof Node) || !menu || menu.contains(e.target)) return;
      if (anchor?.contains(e.target)) return;
      close();
    };
    window.addEventListener("pointerdown", onPointer, true);
    return () => {
      window.removeEventListener("pointerdown", onPointer, true);
      before?.focus();
    };
  });
</script>

{#snippet one(entry: Loose, at: number)}
  {#if entry.kind === "separator"}
    <div class="sep" role="separator"></div>
  {:else if entry.kind === "note"}
    <div class="note" role="presentation">{entry.label}</div>
  {:else}
    <button
      type="button"
      role={entry.checked === undefined
        ? "menuitem"
        : entry.checkbox === true
          ? "menuitemcheckbox"
          : "menuitemradio"}
      data-at={at}
      tabindex={active === at ? 0 : -1}
      title={entry.why}
      aria-checked={entry.checked}
      aria-haspopup={entry.opens === true ? "menu" : undefined}
      aria-disabled={entry.disabled === true ? "true" : undefined}
      class={[
        "item",
        entry.danger && "danger",
        entry.disabled && "is-disabled",
        active === at && "is-active",
      ]}
      onpointerenter={() => focusItem(at)}
      onclick={(e) => run(at, e.shiftKey, true)}
    >
      {#if iconic}
        <span class="icon" aria-hidden="true">
          {#if entry.icon}
            {@const Icon = entry.icon}
            <Icon size={15} strokeWidth={1.8} />
          {/if}
        </span>
      {/if}
      {#if entry.detail}
        <span class="two" title={entry.detail}>
          <span class="label">{entry.label}</span>
          <span class="detail">{entry.detail}</span>
        </span>
      {:else if entry.checked !== undefined}
        <span class="choice">
          <span class="check" aria-hidden="true"
            >{#if entry.checked}<Check size={14} />{/if}</span
          >
          <span>{entry.label}</span>
        </span>
      {:else}
        <span class="text">{entry.label}</span>
      {/if}
      {#if entry.hint}<kbd>{entry.hint}</kbd>{/if}
      {#if entry.opens}<span class="opens" aria-hidden="true">▸</span>{/if}
    </button>
  {/if}
{/snippet}

<div
  bind:this={menu}
  class="menu"
  role="menu"
  tabindex="-1"
  aria-label={label ?? t("menu.fileActions")}
  style:left="{x}px"
  style:top="{y}px"
  {onkeydown}
>
  {#each parts as part (part.at)}
    {#if part.kind === "group"}
      <div role="group" aria-labelledby="{uid}-{part.at}">
        <div class="menu-label heading" role="presentation" id="{uid}-{part.at}">{part.label}</div>
        {#each part.under as { entry, at } (at)}{@render one(entry, at)}{/each}
      </div>
    {:else}
      {@render one(part.entry, part.at)}
    {/if}
  {/each}
</div>

<style>
  .menu {
    position: fixed;
    z-index: 55;
    min-width: 200px;
    max-width: min(380px, calc(100vw - 8px));
    /* kept inside the window (4 px each side, as placed): a long menu scrolls, its ends stay in reach */
    max-height: calc(100vh - 8px);
    overflow-y: auto;
    padding: 6px;
    border: 1px solid var(--border);
    border-radius: var(--radius-menu);
    background: var(--pop);
    box-shadow: var(--shadow-pop);
    outline: none;
    animation: pm-fade var(--dur-fast) var(--ease);
  }
  .item {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    width: 100%;
    padding: 6px 10px;
    border: 0;
    border-radius: var(--radius-control);
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
  /* can't act now: quieter (still AA), still reached by the arrows */
  .item.is-disabled {
    color: var(--muted);
    cursor: default;
  }
  .item.danger {
    color: var(--git-deleted-text);
  }
  .icon {
    display: grid;
    flex: none;
    place-items: center;
    width: 15px;
    height: 15px;
    color: var(--muted);
  }
  .text {
    flex: 1;
    min-width: 0;
  }
  /* the label takes the room; the hint keeps a clear gap after it */
  kbd,
  .opens {
    margin-left: var(--sp-2);
    color: var(--faint);
    font: inherit;
    font-size: var(--fs-ui-xs);
  }
  .two {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
  }
  .label,
  .detail {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .detail {
    color: var(--faint);
    font-size: var(--fs-ui-xs);
  }
  .heading {
    padding-top: var(--sp-2);
  }
  /* nothing to act on: quiet, in the items' indent */
  .note {
    padding: 2px 10px 6px;
    color: var(--muted);
    font-size: var(--fs-ui);
  }
  /* a choice: the mark's room is kept when unchecked, so the labels of the set line up */
  .choice {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
  }
  .check {
    flex: none;
    display: inline-grid;
    place-items: center;
    width: 14px;
  }
  .sep {
    height: 1px;
    margin: var(--sp-1) 0;
    background: var(--border);
  }
</style>
