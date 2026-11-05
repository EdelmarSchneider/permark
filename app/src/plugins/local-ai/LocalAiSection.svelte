<script lang="ts">
  // Settings > Local AI: what each job uses, the models, this computer, the storage, the Hugging Face token (only while
  // a listed model needs one).
  // Opening it reads the state; it never starts a download. Opened from the palette: the keyboard lands on the first
  // control.
  import { tick, untrack } from "svelte";
  import { t } from "../../core/i18n";
  import { ui } from "../../core/shell/ui.svelte";
  import JobsCard from "./JobsCard.svelte";
  import MachineCard from "./MachineCard.svelte";
  import ModelsCard from "./ModelsCard.svelte";
  import StorageCard from "./StorageCard.svelte";
  import { LOCAL_AI_PAGE, localAi } from "./store.svelte";
  import TokenCard from "./TokenCard.svelte";
  import { errorText } from "./words";

  const load = $derived(localAi.load);
  /** The token card shows only while a listed model needs a Hugging Face token. */
  const gated = $derived(localAi.catalog?.models.some((m) => m.gated) ?? false);
  let section: HTMLElement | undefined = $state();

  // follows the backend while the page shows
  $effect(() => untrack(() => localAi.open()));

  // opened from the palette: focus on the first control, once it is drawn
  $effect(() => {
    if (load.status === "idle" || load.status === "loading" || section === undefined) return;
    const root = section;
    let gone = false;
    void tick().then(() => {
      if (gone) return;
      untrack(() => {
        if (ui.takeContentFocus(LOCAL_AI_PAGE))
          root.querySelector<HTMLElement>("[data-autofocus]:not(:disabled)")?.focus();
      });
    });
    return () => {
      gone = true;
    };
  });
</script>

<div class="local-ai" bind:this={section}>
  {#if load.status === "failed"}
    <p class="failed" role="alert">
      <span>{t("localAi.loadFailed")} {errorText(load.error)}</span>
      <button type="button" class="btn btn--sm" data-autofocus onclick={() => void localAi.retry()}
        >{t("localAi.retry")}</button
      >
    </p>
  {:else if load.status === "ready"}
    <p class="intro">{t("localAi.intro")}</p>
    <JobsCard />
    <ModelsCard />
    <MachineCard />
    <StorageCard />
    {#if gated}
      <TokenCard />
    {/if}
  {:else}
    <p class="quiet">{t("app.loading")}</p>
  {/if}
</div>

<style>
  .local-ai {
    display: flex;
    flex-direction: column;
    gap: var(--sp-5);
  }
  .intro,
  .quiet {
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .failed {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-2);
    margin: 0;
    color: var(--git-deleted-text);
    font-size: var(--fs-ui-sm);
  }
</style>
