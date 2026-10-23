<script lang="ts">
  // A small mark a plugin puts on a path: a git letter, an unread dot or count. The tone picks the color (tokens).
  // Screen readers hear its meaning in words ("Unread", "Modified: ..."), never the raw "M" | "•": the visible mark
  // is hidden from them and the title is spoken instead (every row that draws marks: Explorer, header, search).
  import type { TreeBadge } from "../plugins/registry.svelte";

  type Props = { badge: TreeBadge };
  let { badge }: Props = $props();
</script>

{#if badge.text !== ""}
  <span class="badge tone-{badge.tone ?? 'muted'}" title={badge.title}
    >{#if badge.title}<span class="mark" aria-hidden="true">{badge.text}</span><span class="sr-only"
        >, {badge.title}</span
      >{:else}{badge.text}{/if}</span
  >
{/if}

<style>
  .badge {
    flex: none;
    min-width: 16px;
    padding: 0 5px;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--muted);
    font-size: var(--fs-ui-xs);
    font-weight: 600;
    line-height: 16px;
    text-align: center;
  }
  .tone-modified {
    background: color-mix(in srgb, var(--git-modified) 16%, transparent);
    color: var(--git-modified-text);
  }
  .tone-added,
  .tone-untracked {
    background: color-mix(in srgb, var(--git-added) 16%, transparent);
    color: var(--git-added-text);
  }
  .tone-deleted,
  .tone-conflict {
    background: color-mix(in srgb, var(--git-deleted) 16%, transparent);
    color: var(--git-deleted-text);
  }
  .tone-renamed {
    background: color-mix(in srgb, var(--git-renamed) 16%, transparent);
    color: var(--git-renamed-text);
  }
  .tone-unread {
    background: color-mix(in srgb, var(--accent) 16%, transparent);
    color: var(--accent);
  }
</style>
