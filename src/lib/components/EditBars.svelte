<script lang="ts">
  // The notices under the header while editing: unsaved changes (on leaving), the file changed or was deleted on
  // disk, a save that failed, and the one-time hint about block-preserving saves. Inline bars, never a modal.
  import { untrack } from "svelte";
  import { t, tn } from "../i18n";
  import { basename } from "../paths";
  import { edit } from "../stores/edit.svelte";
  import { ui } from "../stores/ui.svelte";
  import { clock } from "../time";
  import Icon from "./Icon.svelte";

  const name = $derived(basename(edit.path ?? ""));
  const conflict = $derived(edit.conflict);

  let promptSave: HTMLButtonElement | undefined = $state();
  let conflictFirst: HTMLButtonElement | undefined = $state();

  // Enter saves: the bar opens with the Save button focused
  $effect(() => {
    if (edit.prompt && promptSave) untrack(() => promptSave?.focus());
  });

  // Ctrl+S while the other version is waiting moves the focus to the bar instead of writing
  let focused = 0;
  $effect(() => {
    const tick = edit.conflictFocus;
    if (tick === focused || !conflictFirst) return;
    focused = tick;
    untrack(() => conflictFirst?.focus());
  });

  async function copyAll(): Promise<void> {
    if (await edit.copyAll()) ui.flash("saveError");
  }
</script>

{#if edit.prompt}
  <div class="banner" role="alert" aria-label={t("edit.bars")}>
    <Icon name="alert" />
    <div>{t("edit.unsavedPrompt", { name })}</div>
    <span class="spacer"></span>
    <div class="banner__actions">
      <button
        bind:this={promptSave}
        type="button"
        class="btn btn--sm btn--primary"
        onclick={() => void edit.promptSave()}
      >
        {t("edit.save")}
      </button>
      <button type="button" class="btn btn--sm" onclick={() => edit.promptDiscard()}>
        {t("edit.discard")}
      </button>
      <button type="button" class="btn btn--sm btn--ghost" onclick={() => edit.keepEditing()}>
        {t("edit.keepEditing")}
      </button>
    </div>
  </div>
{/if}

{#if conflict}
  <div class="banner banner--error" role="alert">
    <Icon name="circle-alert" />
    <div>
      <strong>
        {t(conflict.deleted ? "conflict.deletedOnDisk" : "conflict.changedOnDisk", {
          name,
          time: clock(conflict.atMs),
        })}
      </strong>
      {#if conflict.confirmReload !== null}
        {tn("conflict.reloadConfirm", conflict.confirmReload)}
      {:else}
        {t("conflict.kept")}
      {/if}
    </div>
    <span class="spacer"></span>
    <div class="banner__actions">
      {#if conflict.confirmReload !== null}
        <button
          bind:this={conflictFirst}
          type="button"
          class="btn btn--sm btn--primary"
          onclick={() => void edit.reloadTheirs(true)}
        >
          {t("conflict.discardAndReload")}
        </button>
        <button type="button" class="btn btn--sm btn--ghost" onclick={() => edit.cancelReload()}>
          {t("common.cancel")}
        </button>
      {:else if conflict.deleted}
        <button
          bind:this={conflictFirst}
          type="button"
          class="btn btn--sm btn--primary"
          onclick={() => void edit.saveAsNew()}
        >
          {t("conflict.saveAsNew")}
        </button>
        <button type="button" class="btn btn--sm" onclick={() => edit.discardDeleted()}>
          {t("edit.discard")}
        </button>
      {:else}
        <button
          bind:this={conflictFirst}
          type="button"
          class="btn btn--sm"
          onclick={() => void edit.reloadTheirs()}
        >
          {t("conflict.reloadTheirs")}
        </button>
        <button type="button" class="btn btn--sm" onclick={() => void edit.keepMine()}>
          {t("conflict.keepMine")}
        </button>
      {/if}
    </div>
  </div>
{/if}

{#if edit.saveError}
  <div class="banner banner--error" role="alert">
    <Icon name="circle-alert" />
    <div>{t("edit.saveError", { name, message: edit.saveError })}</div>
    <span class="spacer"></span>
    <div class="banner__actions">
      <button type="button" class="btn btn--sm" onclick={() => edit.retry()}>
        {t("edit.retry")}
      </button>
      <button type="button" class="btn btn--sm" onclick={() => void copyAll()}>
        <Icon name={ui.flashed === "saveError" ? "check" : "copy"} size="sm" />
        {ui.flashed === "saveError" ? t("common.copied") : t("edit.copyAll")}
      </button>
      <button type="button" class="btn btn--sm btn--ghost" onclick={() => edit.dismissSaveError()}>
        {t("common.dismiss")}
      </button>
    </div>
  </div>
{/if}

{#if edit.hint}
  <div class="banner banner--info" role="status">
    <Icon name="info" />
    <div>{t("edit.firstSaveHint")}</div>
    <span class="spacer"></span>
    <div class="banner__actions">
      <button type="button" class="btn btn--sm btn--ghost" onclick={() => edit.dismissHint()}>
        {t("edit.gotIt")}
      </button>
    </div>
  </div>
{/if}
