<script lang="ts">
  // A tag as a small pill: its color and icon when the project gave it some. Plain, a button (opens the tag in the tags
  // view) or removable (the editor); a tag from a file's front matter is locked. Where the opened folder has official
  // lists: a mark says official | local | unlisted (shape + words; an unlisted tag's outline is dashed).
  import Lock from "@lucide/svelte/icons/lock";
  import X from "@lucide/svelte/icons/x";
  import type { TagStatus } from "../../api";
  import { t } from "../../core/i18n";
  import StatusMark from "../../core/projects/StatusMark.svelte";
  import { TAG_ICONS } from "./icons";
  import { isColor, isIcon } from "./model";

  type Props = {
    name: string;
    color?: string | null;
    icon?: string | null;
    /** From the front matter of the file: it cannot be removed here. */
    locked?: boolean;
    count?: number;
    active?: boolean;
    onclick?: (event: MouseEvent) => void;
    onremove?: () => void;
    title?: string;
    /** Official | local | unlisted (`null`: no official list, nothing drawn) and its words. */
    status?: TagStatus | null;
    statusWords?: string;
  };
  let {
    name,
    color = null,
    icon = null,
    locked = false,
    count,
    active = false,
    onclick,
    onremove,
    title,
    status = null,
    statusWords = "",
  }: Props = $props();

  const Icon = $derived(isIcon(icon) ? TAG_ICONS[icon] : null);
  const tint = $derived(isColor(color) ? `var(--tag-${color})` : "var(--muted)");
</script>

<span
  class="chip"
  class:active
  class:colored={isColor(color)}
  class:unlisted={status === "unlisted"}
  style:--c={tint}
  data-tag={name}
>
  {#if onclick}
    <button type="button" class="face" {onclick} {title}>
      {#if Icon}<Icon size={11} />{/if}
      <span class="name">{name}</span>
      {#if status !== null}<StatusMark {status} words={statusWords} />{/if}
      {#if count !== undefined}<span class="count">{count}</span>{/if}
    </button>
  {:else}
    <span class="face" {title}>
      {#if Icon}<Icon size={11} />{/if}
      <span class="name">{name}</span>
      {#if status !== null}<StatusMark {status} words={statusWords} />{/if}
      {#if count !== undefined}<span class="count">{count}</span>{/if}
    </span>
  {/if}
  {#if locked}
    <span class="lock" title={t("tags.fromFileHint")}><Lock size={10} aria-hidden="true" /></span>
  {/if}
  {#if onremove}
    <button
      type="button"
      class="remove"
      aria-label={t("tags.removeTag", { name })}
      title={t("tags.removeTag", { name })}
      onclick={onremove}
    >
      <X size={11} />
    </button>
  {/if}
</span>

<style>
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    max-width: 100%;
    padding: 0 6px;
    border: 1px solid color-mix(in srgb, var(--c) 28%, transparent);
    border-radius: 999px;
    background: color-mix(in srgb, var(--c) var(--tag-tint, 12%), transparent);
    color: var(--c);
    font-size: var(--fs-ui-xs);
    line-height: 18px;
    white-space: nowrap;
  }
  .chip:not(.colored) {
    background: var(--surface-2);
    border-color: var(--border);
  }
  .chip.unlisted {
    border-style: dashed;
  }
  .chip.active {
    background: color-mix(in srgb, var(--c) 28%, transparent);
    border-color: var(--c);
    font-weight: 600;
  }
  .face {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
    padding: 0;
    border: 0;
    background: transparent;
    color: inherit;
    font: inherit;
  }
  button.face {
    cursor: pointer;
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .count {
    padding-left: 2px;
    opacity: 0.75;
    font-variant-numeric: tabular-nums;
  }
  .lock {
    display: inline-flex;
    opacity: 0.6;
  }
  .remove {
    display: inline-grid;
    place-items: center;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: transparent;
    color: inherit;
    cursor: pointer;
  }
  .remove:hover {
    background: var(--hover);
  }
</style>
