<script lang="ts">
  // "Models": every catalog model, the ones on this computer first (ready, then with an update), the rest by name. Each
  // row says what the model can do and how it runs here, and offers what its state allows. One live region tells
  // screen readers the download moments (start, each quarter, done, failure), never every tick.
  import { language, t } from "../../core/i18n";
  import Card from "../../core/shell/settings/Card.svelte";
  import ModelRow from "./ModelRow.svelte";
  import { localAi, type Milestone } from "./store.svelte";
  import { percentText } from "./words";
  import type { CatalogModel } from "../../api/generated/CatalogModel";
  import type { ModelStatus } from "../../api/generated/ModelStatus";

  const id = $props.id();

  /** 0: ready here, 1: here with an update (or updating), 2: the rest. */
  function rank(status: ModelStatus): number {
    if (status.state.kind === "installed") return 0;
    return status.state.kind === "updateAvailable" || status.installedVersion !== null ? 1 : 2;
  }

  const rows = $derived.by(() => {
    const collator = new Intl.Collator(language.locale);
    const found = (localAi.state?.models ?? []).flatMap((status) => {
      const model = localAi.modelOf(status.model);
      return model === null ? [] : [{ model, status, rank: rank(status) }];
    });
    return found.sort((a, b) => a.rank - b.rank || collator.compare(a.model.name, b.model.name));
  });

  function liveText(milestone: Milestone | null, model: CatalogModel | null): string {
    if (milestone === null) return "";
    const name = model?.name ?? milestone.model;
    switch (milestone.step) {
      case "started":
        return t("localAi.dl.live.started", { name });
      case 25:
      case 50:
      case 75:
        return t("localAi.dl.live.percent", { name, percent: percentText(milestone.step, 100) });
      case "done":
        return t("localAi.dl.live.done", { name });
      case "failed":
        return t("localAi.dl.live.failed", { name });
      default:
        return milestone.step satisfies never;
    }
  }

  const live = $derived(
    liveText(
      localAi.milestone,
      localAi.milestone === null ? null : localAi.modelOf(localAi.milestone.model),
    ),
  );

  // the fit hints need this computer: asked once the card shows
  localAi.needMachine();
</script>

<section class="models" aria-labelledby={`${id}-title`}>
  <h3 id={`${id}-title`}>{t("localAi.models.title")}</h3>
  <Card>
    <ul class="list" aria-label={t("localAi.models.title")}>
      {#each rows as row (row.model.id)}
        <ModelRow model={row.model} status={row.status} />
      {/each}
    </ul>
  </Card>
  <p class="sr-only live" role="status" aria-live="polite">{live}</p>
</section>

<style>
  .models {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
  }
  h3 {
    margin: 0;
    padding-left: var(--sp-1);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    font-weight: 600;
  }
  .list {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .list > :global(li + li) {
    border-top: 1px solid var(--border);
  }
</style>
