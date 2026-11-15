<script lang="ts">
  // The Notes button of the top bar, left of Settings: one click opens the Notes tab, or brings it forward where it is
  // (another tab, another window). Marked current (`aria-current`) in the Notes tab's own top bar: it says where the
  // person is, it is no switch. A notebook plus the word, in the Notes' own color (their tab chip's), so the way to the
  // Notes reads from the screen; a narrow window keeps the icon alone (the name stays the label and the tooltip).
  import NotebookPen from "@lucide/svelte/icons/notebook-pen";
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
  <NotebookPen size={16} strokeWidth={here ? 2 : 1.8} aria-hidden="true" />
  <span class="word">{t("notes.title")}</span>
</button>

<style>
  /* the top bar's own icon size (Settings beside it); the tint of the Notes tab chip, stronger on hover and where they
     are, with the color drawn toward the text color there so the word keeps 4.5:1 on the stronger tint (every tone) */
  .notes {
    --ink: color-mix(in srgb, var(--tag-yellow) 75%, var(--text));
    display: inline-flex;
    flex: none;
    align-items: center;
    gap: 6px;
    width: auto;
    min-width: 30px;
    height: 30px;
    padding: 0 7px;
    border-radius: var(--radius-surface);
    background: color-mix(in srgb, var(--tag-yellow) var(--tag-tint), transparent);
    color: var(--tag-yellow);
  }
  .notes:hover:not(:disabled) {
    background: color-mix(in srgb, var(--tag-yellow) 20%, transparent);
    color: var(--ink);
  }
  .notes[aria-current="page"] {
    background: color-mix(in srgb, var(--tag-yellow) 24%, transparent);
    color: var(--ink);
  }
  .word {
    display: none;
    font-size: 12.5px;
    font-weight: 560;
    white-space: nowrap;
  }
  @media (min-width: 900px) {
    .notes {
      padding: 0 11px 0 9px;
    }
    .word {
      display: inline;
    }
  }
</style>
