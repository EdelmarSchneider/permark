<script lang="ts">
  // What a loose file that is on screen needs the person to decide (docs/design/v0.5.md section 5). It changed on disk: what is
  // shown stays until they choose, Reload or Keep mine, whether or not they typed into it. Or it is not there any more: the
  // text typed into it is kept, to save as a note or to write the file again. An inline bar, never a modal.
  import { t } from "../i18n";
  import { edit } from "../stores/edit.svelte";
  import { notes } from "../stores/notes.svelte";
  import Icon from "./Icon.svelte";

  const notice = $derived(notes.looseNotice);
  const name = $derived(notes.currentEntry?.title ?? "");
  // there is text to keep: what is being edited, or what was kept for a file that was not there when it was opened
  const hasText = $derived((edit.active && edit.dirty) || notes.missingKept !== null);
</script>

{#if notice?.kind === "changed"}
  <div class="banner banner--error" role="alert">
    <Icon name="circle-alert" />
    <div>
      <strong>{t("notes.file.changed")}</strong>
      {#if edit.active && edit.dirty}{t("notes.file.changedKept")}{/if}
    </div>
    <span class="spacer"></span>
    <div class="banner__actions">
      <button
        type="button"
        class="btn btn--sm"
        disabled={notes.readOnly !== null}
        onclick={() => void notes.reloadFile()}
      >
        {t("notes.file.reload")}
      </button>
      <button type="button" class="btn btn--sm" onclick={() => void notes.keepMine()}>
        {t("notes.file.keepMine")}
      </button>
    </div>
  </div>
{:else if notice?.kind === "missing"}
  <div class="banner banner--error" role="alert">
    <Icon name="circle-alert" />
    <div>
      <strong>{t("notes.file.gone", { name })}</strong>
      {#if hasText}{t("notes.file.goneKept")}{/if}
    </div>
    <span class="spacer"></span>
    <div class="banner__actions">
      {#if hasText}
        <button
          type="button"
          class="btn btn--sm btn--primary"
          disabled={notes.readOnly !== null}
          onclick={() => void notes.saveMissingAsNote()}
        >
          {t("notes.file.saveAsNote")}
        </button>
        <button
          type="button"
          class="btn btn--sm"
          disabled={notes.readOnly !== null}
          onclick={() => void notes.saveMissingAgain()}
        >
          {t("notes.file.saveAgain")}
        </button>
      {/if}
      <button
        type="button"
        class={["btn", "btn--sm", !hasText && "btn--primary"]}
        disabled={notes.readOnly !== null}
        onclick={() => notes.current && void notes.locateFile(notes.current)}
      >
        {t("notes.file.locate")}
      </button>
      {#if !hasText}
        <button
          type="button"
          class="btn btn--sm"
          disabled={notes.readOnly !== null}
          onclick={() => notes.current && void notes.removeFile(notes.current)}
        >
          {t("notes.file.remove")}
        </button>
      {/if}
    </div>
  </div>
{/if}
