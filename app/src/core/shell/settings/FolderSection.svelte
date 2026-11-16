<script lang="ts">
  // This folder's overview: who each kind of folder setting is for (Everyone = its `.permark/config.toml`, Only you =
  // this computer), the folder's own card where it holds projects, and the offer to set PerMark up while `.permark/`
  // is missing. Nothing is written into the folder until the person answers the Initialize question.
  import { conditions } from "../../conditions/context.svelte";
  import { t } from "../../i18n";
  import { projects } from "../../projects/instance.svelte";
  import { askInitHere } from "../initProject";
  import Card from "./Card.svelte";
  import SettingRow from "./SettingRow.svelte";

  const initialized = $derived(conditions.current.permark === "initialized");

  /** The card fields load only where the folder holds projects (a plain folder never needs them). */
  const cardFields = (): Promise<typeof import("../../projects/CardFields.svelte")> =>
    import("../../projects/CardFields.svelte");
</script>

<div class="overview">
  <p class="scopes">{t("project.scopes")}</p>
  {#if !initialized}
    <Card>
      <SettingRow label={t("project.notInitialized")} hint={t("project.notInitializedHint")}>
        <button type="button" class="btn btn--primary btn--sm" onclick={() => askInitHere()}>
          {t("project.initButton")}
        </button>
      </SettingRow>
    </Card>
  {/if}
  {#if projects.present && projects.view}
    {@const card = projects.view.rootCard}
    <!-- the opened folder's own card (it holds projects: its name shows where they are listed) -->
    <section class="root-card" aria-labelledby="proj-card">
      <h3 id="proj-card">{t("projects.card.title")}</h3>
      <Card>
        <div class="card-body">
          {#await cardFields() then fields}
            <fields.default
              id="root-card"
              target={{ kind: "root" }}
              place={card.place}
              categoryStatus={projects.view.rootCategoryStatus}
              short={card.short}
              values={{
                name: card.name,
                short: card.short,
                description: card.description ?? "",
                category: card.category ?? "",
              }}
            />
          {/await}
        </div>
      </Card>
    </section>
  {/if}
</div>

<style>
  .overview {
    display: flex;
    flex-direction: column;
    gap: var(--sp-5);
  }
  .scopes {
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .card-body {
    padding: var(--sp-3) var(--sp-4);
  }
  h3 {
    margin: 0 0 var(--sp-2);
    padding-left: var(--sp-1);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    font-weight: 600;
  }
</style>
