<script lang="ts">
  // Settings > Local AI: what each job uses, then (later units) the models, this computer, the storage, the token.
  // Opening it reads the state; it never starts a download. Opened from the palette: the keyboard lands on the first
  // control.
  import { tick, untrack } from "svelte";
  import { t } from "../../core/i18n";
  import { ui } from "../../core/shell/ui.svelte";
  import JobsCard from "./JobsCard.svelte";
  import { LOCAL_AI_PAGE, localAi } from "./store.svelte";
  import { errorText } from "./words";

  const load = $derived(localAi.load);
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
    <!-- Models, This computer, Storage, Token cards: later units, in this order -->
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
