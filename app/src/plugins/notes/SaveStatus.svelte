<script lang="ts">
  // Status bar item of the Notes: whether what is typed is on disk yet. Autosave makes "Unsaved" brief.
  import { editing } from "../../core/editing/session.svelte";
  import { t } from "../../core/i18n";
  import { notes } from "./store.svelte";

  const open = $derived(notes.current !== null && editing.path === notes.current);
  const text = $derived(
    editing.state.s === "saving"
      ? t("notes.status.saving")
      : editing.state.s === "conflict" || editing.state.s === "saveError"
        ? t("notes.status.unsaved")
        : editing.unsaved
          ? t("notes.status.unsaved")
          : t("notes.status.saved"),
  );
</script>

{#if open}
  <span class="status" aria-live="polite">{text}</span>
{/if}

<style>
  .status {
    color: var(--muted);
  }
</style>
