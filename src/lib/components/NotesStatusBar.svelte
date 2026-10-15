<script lang="ts">
  // The status bar of the Notes window: whether the note is saved, how long it is, and the zoom. There is no git, no
  // "since last visit" and no agent connection here.
  import { tn, t } from "../i18n";
  import { app } from "../stores/app.svelte";
  import { edit } from "../stores/edit.svelte";
  import { notes } from "../stores/notes.svelte";
  import { session } from "../stores/session.svelte";
  import { updates } from "../stores/updates.svelte";
  import { describeUpdate } from "../updates";
  import Icon from "./Icon.svelte";
  import IconButton from "./IconButton.svelte";

  const update = $derived(updates.state);
  // "Saved" when the note is on disk; "Saving..." only when a save takes longer than a second; nothing while typing
  const saveState = $derived(
    !notes.current || notes.unborn || notes.currentMissing
      ? ""
      : notes.isFile
        ? // a loose file is written when asked: it says "Unsaved changes" until then, and "Saved" for a moment after
          edit.dirty
          ? t("notes.status.unsaved")
          : (edit.note ?? "")
        : edit.savingSlow
          ? t("notes.status.saving")
          : edit.dirty || edit.saving
            ? ""
            : t("notes.status.saved"),
  );
</script>

<footer class="statusbar">
  <div class="sb-group sb-group--start">
    <IconButton
      small
      class="sb-panel-toggle"
      icon="panel-left"
      label={t("status.toggleLeft")}
      pressed={session.left.open}
      onclick={() => app.toggleLeft()}
    />
    <span class="sb-item">{t("notes.title")}</span>
    {#if notes.readOnly}
      <span class="sb-item is-warn" title={t(`notes.readOnly.${notes.readOnly}`)}>
        <Icon name="alert" />{t("notes.status.readOnly")}
      </span>
    {/if}
    {#if saveState !== ""}
      <span class="sb-item" role="status" aria-live="polite">{saveState}</span>
    {/if}
    {#if notes.current && !notes.unborn && !notes.currentMissing}
      <span class="sb-item">{tn("notes.status.words", notes.words)}</span>
    {/if}
    {#if edit.active && edit.mode === "source"}
      <span class="sb-item" aria-live="off">
        {t("edit.position", { line: edit.caret.line, col: edit.caret.column })}
      </span>
    {/if}
  </div>
  <div class="sb-group sb-group--end">
    {#if update.phase === "available"}
      <button
        type="button"
        class="sb-item is-on"
        title={t("update.statusHint", { version: update.version })}
        onclick={() => updates.announce()}
      >
        <Icon name="download" />{t("update.statusAvailable", { version: update.version })}
      </button>
    {:else if update.phase === "downloading" || update.phase === "installing"}
      <button
        type="button"
        class="sb-item is-on"
        title={describeUpdate(update)}
        onclick={() => updates.announce()}
      >
        <Icon name="download" />{describeUpdate(update)}
      </button>
    {/if}
    <button
      type="button"
      class="sb-item"
      title={t("status.zoomHint")}
      onclick={() => session.setZoom(1)}
    >
      {Math.round(session.zoom * 100)}%
    </button>
    <button
      type="button"
      class="sb-item"
      title={t("status.widthHint", { name: t(`width.${session.width}`) })}
      onclick={() => session.cycleWidth()}
    >
      <Icon name="width" />{t(`width.${session.width}`)}
    </button>
  </div>
</footer>
