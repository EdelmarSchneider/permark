<script lang="ts">
  // The Notes button of the top bar, left of Settings: one click opens the Notes tab, or brings it forward where it is
  // (another tab, another window). Marked current (`aria-current`) in the Notes tab's own top bar: it says where the
  // person is, it is no switch.
  import StickyNote from "@lucide/svelte/icons/sticky-note";
  import { keyHint } from "../../core/commands/hint";
  import { t } from "../../core/i18n";
  import { windowStore } from "../../core/window.svelte";
  import { inNotes } from "./open";

  const hint = $derived(keyHint("notes.open"));
  const here = $derived(inNotes(windowStore.view));
</script>

<button
  type="button"
  class="icon-btn notes"
  aria-label={t("notes.title")}
  aria-current={here ? "page" : undefined}
  title={hint === null ? t("notes.title") : `${t("notes.title")} (${hint})`}
  onclick={() => void windowStore.openNotes("tab")}
>
  <StickyNote size={16} strokeWidth={1.9} aria-hidden="true" />
</button>

<style>
  /* the top bar's own icon look (Settings beside it) */
  .notes {
    flex: none;
    width: 30px;
    height: 30px;
    border-radius: 9px;
  }
  .notes[aria-current="page"] {
    background: var(--accent-soft);
    color: var(--accent-text);
  }
</style>
