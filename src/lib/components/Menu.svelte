<script lang="ts" module>
  import type { IconName } from "./Icon.svelte";

  export type MenuEntry =
    | {
        type?: "item";
        id: string;
        label: string;
        icon?: IconName;
        /** Right-aligned secondary text: a shortcut or a time. */
        hint?: string;
        /** Right-aligned secondary text that is not a key combination (the folder of a file): shown as it is. */
        detail?: string;
        checked?: boolean;
        disabled?: boolean;
        /** The tooltip: for an entry that is off, why. */
        title?: string;
        /** The keyboard starts here instead of on the first entry that is on. */
        initial?: boolean;
        onselect: () => void;
      }
    | { type: "separator" }
    | { type: "label"; label: string }
    /** A sentence of explanation between the entries (a heading, a label, is shorter and in capitals). */
    | { type: "note"; text: string };
</script>

<script lang="ts">
  import { onMount } from "svelte";
  import { keys } from "../i18n";
  import Icon from "./Icon.svelte";
  import Popover, { type Anchor } from "./Popover.svelte";

  type Props = {
    entries: MenuEntry[];
    anchor: Anchor;
    side?: "bottom" | "top" | "right";
    align?: "start" | "end";
    /** A long list scrolls instead of running off the screen. */
    scroll?: boolean;
    label: string;
    onclose: () => void;
  };
  let {
    entries,
    anchor,
    side = "bottom",
    align = "start",
    scroll = false,
    label,
    onclose,
  }: Props = $props();

  let list: HTMLElement | undefined = $state();

  function buttons(): HTMLButtonElement[] {
    return Array.from(list?.querySelectorAll<HTMLButtonElement>("button:not(:disabled)") ?? []);
  }

  onMount(() => {
    // the popover is positioned in an effect; focus once it is in place
    const timer = setTimeout(() => {
      const first = list?.querySelector<HTMLButtonElement>("button[data-initial]:not(:disabled)");
      (first ?? buttons()[0])?.focus();
    }, 0);
    return () => clearTimeout(timer);
  });

  function onkeydown(e: KeyboardEvent): void {
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    const items = buttons();
    if (items.length === 0) return;
    const i = items.indexOf(document.activeElement as HTMLButtonElement);
    const next =
      e.key === "ArrowDown" ? (i + 1) % items.length : (i - 1 + items.length) % items.length;
    items[next].focus();
  }

  function choose(entry: Extract<MenuEntry, { onselect: () => void }>): void {
    onclose();
    entry.onselect();
  }
</script>

<Popover {anchor} {side} {align} {scroll} {label} {onclose} variant="menu">
  <div bind:this={list} role="presentation" {onkeydown} style="display:contents">
    {#each entries as entry, i (i)}
      {#if entry.type === "separator"}
        <div class="menu-sep" role="separator"></div>
      {:else if entry.type === "label"}
        <div class="menu-label">{entry.label}</div>
      {:else if entry.type === "note"}
        <div class="menu-note">{entry.text}</div>
      {:else}
        <button
          type="button"
          class="menu-item"
          role={entry.checked !== undefined ? "menuitemcheckbox" : "menuitem"}
          aria-checked={entry.checked}
          disabled={entry.disabled}
          title={entry.title}
          data-initial={entry.initial ? "" : undefined}
          onclick={() => choose(entry)}
        >
          {#if entry.checked !== undefined}
            <span class="menu-item__check">
              {#if entry.checked}<Icon name="check" size="sm" />{/if}
            </span>
          {:else if entry.icon}
            <Icon name={entry.icon} size="sm" />
          {/if}
          <span class="menu-item__label">{entry.label}</span>
          {#if entry.detail}<span class="menu-item__detail">{entry.detail}</span>{/if}
          {#if entry.hint}<span class="menu-item__hint">{keys(entry.hint)}</span>{/if}
        </button>
      {/if}
    {/each}
  </div>
</Popover>
