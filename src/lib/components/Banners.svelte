<script lang="ts">
  import { t } from "../i18n";
  import { showInitHint } from "../initHint";
  import { app } from "../stores/app.svelte";
  import { docs } from "../stores/docs.svelte";
  import { review } from "../stores/review.svelte";
  import { session } from "../stores/session.svelte";
  import { ui } from "../stores/ui.svelte";
  import { vault } from "../stores/vault.svelte";
  import { clock } from "../time";
  import { basename } from "../paths";
  import Icon from "./Icon.svelte";

  const repoState = $derived(vault.git?.repoState ?? "clean");
  const showRepo = $derived(repoState !== "clean" && ui.dismissedRepoState !== repoState);
  const deleted = $derived(docs.deleted);
  const root = $derived(vault.info?.root ?? "");
  const showInit = $derived(
    showInitHint({
      simple: vault.isSimple,
      isRepo: vault.git?.isRepo === true,
      stateLocation: vault.stateLocation,
      never: session.initHintNever,
      snoozed: ui.initHintSnoozed.has(root),
      dialogOpen: ui.initDialog,
    }),
  );

  // the banner comes back the next time the state changes
  $effect(() => {
    if (repoState === "clean" && ui.dismissedRepoState !== null) ui.dismissedRepoState = null;
  });
</script>

{#if showRepo}
  <div class="banner" role="status">
    <Icon name="alert" />
    <div>{t(`repo.banner.${repoState}`)}</div>
    <span class="spacer"></span>
    <button
      type="button"
      class="btn btn--ghost btn--sm"
      onclick={() => (ui.dismissedRepoState = repoState)}
    >
      {t("common.dismiss")}
    </button>
  </div>
{/if}

{#if showInit}
  <div class="banner banner--info" role="status">
    <Icon name="lightbulb" />
    <div>{t("initHint.text")} <span class="banner__detail">{t("initHint.detail")}</span></div>
    <span class="spacer"></span>
    <div class="banner__actions">
      <button
        type="button"
        class="btn btn--sm"
        title={t("initHint.detail")}
        onclick={() => (ui.initDialog = true)}
      >
        {t("initHint.initialize")}
      </button>
      <button
        type="button"
        class="btn btn--ghost btn--sm"
        onclick={() => ui.initHintSnoozed.add(root)}
      >
        {t("initHint.notNow")}
      </button>
      <button type="button" class="btn btn--ghost btn--sm" onclick={() => session.neverHintInit()}>
        {t("initHint.never")}
      </button>
    </div>
  </div>
{/if}

{#if deleted}
  <div class="banner banner--error" role="alert">
    <Icon name="circle-alert" />
    <div>
      <strong>{t("doc.deleted.title", { time: clock(deleted.atMs) })}</strong>
      {t("doc.deleted.text")}
      {#if deleted.movedTo}
        <br />{t("doc.deleted.moved", { path: deleted.movedTo })}
      {/if}
    </div>
    <span class="spacer"></span>
    <div class="banner__actions">
      {#if deleted.movedTo}
        {@const target = deleted.movedTo}
        <button type="button" class="btn btn--sm" onclick={() => void app.openDoc(target)}>
          {t("common.open")}
        </button>
      {/if}
      {#if review.view?.status === "deleted" && review.view.restorable && review.view.path === docs.path}
        <button type="button" class="btn btn--sm" onclick={() => void review.restoreDeleted()}>
          {t("review.restore")}
        </button>
      {/if}
      <button type="button" class="btn btn--ghost btn--sm" onclick={() => void docs.closeDeleted()}>
        {t("common.close")}
      </button>
    </div>
  </div>
{/if}

{#if ui.followChip && session.follow}
  {@const chip = ui.followChip}
  <div class="banner banner--info follow-chip" role="status">
    <span class="live-dot"></span>
    <div>{t("follow.chip", { name: basename(chip.from) })}</div>
    <span class="spacer"></span>
    <div class="banner__actions">
      <button
        type="button"
        class="btn btn--ghost btn--sm"
        disabled={!docs.canBack}
        onclick={() => void docs.back()}
      >
        {t("follow.back")}
      </button>
      <button type="button" class="btn btn--ghost btn--sm" onclick={() => app.toggleFollow()}>
        {t("follow.stop")}
      </button>
    </div>
  </div>
{/if}
