<script lang="ts">
  // Where a tag | category stands (`projects.md` §7.2): official (a badge with a check), local to a project (the
  // project's folder mark), unlisted (a dashed circle). A shape per status, never the color alone; its words in the
  // tooltip and after the name for screen readers (inside the chip's button, so the button's name says it). 14 px: the
  // shapes must read apart at a glance (UX round 4 F2). `silent`: the words are written next to it (legend, status
  // lines), so the mark itself says nothing.
  import BadgeCheck from "@lucide/svelte/icons/badge-check";
  import CircleDashed from "@lucide/svelte/icons/circle-dashed";
  import FolderDot from "@lucide/svelte/icons/folder-dot";
  import type { TagStatus } from "../../api";

  type Props = { status: TagStatus; words: string; size?: number; silent?: boolean };
  let { status, words, size = 14, silent = false }: Props = $props();
</script>

<span class="mark {status}" title={words} data-status={status}>
  {#if status === "official"}<BadgeCheck
      {size}
      aria-hidden="true"
    />{:else if status === "local"}<FolderDot {size} aria-hidden="true" />{:else}<CircleDashed
      {size}
      aria-hidden="true"
    />{/if}{#if !silent}<span class="sr-only">, {words}</span>{/if}
</span>

<style>
  .mark {
    display: inline-flex;
    flex: none;
    align-items: center;
  }
  .official {
    color: var(--accent-text);
  }
  .local,
  .unlisted {
    color: var(--muted);
  }
</style>
