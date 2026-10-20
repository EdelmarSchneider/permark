<script lang="ts">
  // A group of settings rows: one bordered card, rows separated by lines (`SettingRow` draws the lines).
  import type { Snippet } from "svelte";

  type Props = { children: Snippet };
  let { children }: Props = $props();
</script>

<div class="card">{@render children()}</div>

<style>
  .card {
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--bg);
  }
  .card > :global(* + *) {
    border-top: 1px solid var(--border);
  }
</style>
