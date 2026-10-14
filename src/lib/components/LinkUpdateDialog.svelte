<script lang="ts">
  // "Update 7 links in 4 files?" (docs/design/v0.2.md section 17): what a move would change in the documents, before
  // anything is written. Apply moves and rewrites the links, Move without updating only moves, Cancel does nothing.
  import { onMount } from "svelte";
  import { t, tn } from "../i18n";
  import { registerOverlay, rememberFocus } from "../overlays";
  import { basename } from "../paths";
  import { fileops, type LinkPrompt } from "../stores/fileops.svelte";
  import { skipNotes } from "../fileOps";

  type Props = { prompt: LinkPrompt };
  let { prompt }: Props = $props();

  const preview = $derived(prompt.preview);
  const title = $derived(
    tn("fileOps.updateLinks", preview.linkCount, {
      files: tn("fileOps.files", preview.fileCount),
    }),
  );
  const notes = $derived(skipNotes(preview.skipped));

  let apply: HTMLButtonElement | undefined = $state();
  let dialog: HTMLElement | undefined = $state();

  // keep Tab inside the dialog
  function onkeydown(e: KeyboardEvent): void {
    if (e.key !== "Tab" || !dialog) return;
    const focusable = Array.from(
      dialog.querySelectorAll<HTMLElement>("button:not(:disabled), summary"),
    );
    if (focusable.length === 0) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  }

  onMount(() => {
    const restore = rememberFocus();
    apply?.focus();
    const unregister = registerOverlay(() => fileops.answer("cancel"));
    return () => {
      unregister();
      restore();
    };
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div
  class="backdrop"
  onclick={(e) => {
    if (e.target === e.currentTarget) fileops.answer("cancel");
  }}
>
  <div
    bind:this={dialog}
    class="dialog dialog--links"
    role="dialog"
    aria-modal="true"
    aria-labelledby="links-title"
    aria-describedby="links-intro"
    tabindex="-1"
    {onkeydown}
  >
    <h2 id="links-title">{title}</h2>
    <ul class="link-list__moves" aria-label={t("fileOps.movesLabel")}>
      {#each preview.moves.slice(0, 3) as move (move.from)}
        <li><code>{move.from}</code> <span aria-hidden="true">→</span> <code>{move.to}</code></li>
      {/each}
      {#if preview.moves.length > 3}
        <li>{tn("fileOps.moreMoves", preview.moves.length - 3)}</li>
      {/if}
    </ul>
    <p id="links-intro">{t("fileOps.updateIntro")}</p>
    <div class="link-list" role="group" aria-label={t("fileOps.dialogLabel")}>
      {#each preview.files as file, i (file.path)}
        <details open={i < 3}>
          <summary>
            <span class="link-list__file">{basename(file.path)}</span>
            <span class="link-list__where">
              {#if file.movedTo}{t("fileOps.movesTo", {
                  path: file.movedTo,
                })}{:else}{file.path}{/if}
            </span>
            <span class="count-pill">{file.changes.length}</span>
          </summary>
          <ul>
            {#each file.changes as change, j (j)}
              <li>
                <span class="link-list__line">{t("fileOps.atLine", { line: change.line })}</span>
                <code>{change.old}</code>
                <span aria-hidden="true">→</span>
                <code>{change.new}</code>
              </li>
            {/each}
          </ul>
        </details>
      {/each}
      {#if preview.truncated}
        <p class="link-list__note">{t("fileOps.moreLinks")}</p>
      {/if}
      {#if notes.length > 0}
        <p class="link-list__note"><strong>{t("fileOps.skippedTitle")}</strong></p>
        <ul class="link-list__skipped">
          {#each notes as note, n (n)}<li>{note}</li>{/each}
        </ul>
      {/if}
    </div>
    <footer>
      <button type="button" class="btn" onclick={() => fileops.answer("cancel")}>
        {t("common.cancel")}
      </button>
      <button type="button" class="btn" onclick={() => fileops.answer("moveOnly")}>
        {t("fileOps.moveOnly")}
      </button>
      <button
        bind:this={apply}
        type="button"
        class="btn btn--primary"
        onclick={() => fileops.answer("apply")}
      >
        {t("fileOps.apply")}
      </button>
    </footer>
  </div>
</div>
