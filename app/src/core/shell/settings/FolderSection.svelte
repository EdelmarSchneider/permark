<script lang="ts">
  // This folder's overview: the offer to set PerMark up while `.permark/` is missing (the full card lives here only);
  // once set up, who each kind of folder setting is for (Everyone = its `.permark/config.toml`, Only you = this
  // computer: before, nothing is marked Everyone yet) and the shared file, with a way to see it in the file manager;
  // where the person's data for the folder is kept while it is not in the folder's `.permark/` (Move stays here after
  // Keep); the folder's own card where it holds projects. Nothing is written into the folder until the person answers
  // the Initialize question | presses Move.
  import { conditions } from "../../conditions/context.svelte";
  import { documents } from "../../doc/document.svelte";
  import { t, type MessageKey } from "../../i18n";
  import { projects } from "../../projects/instance.svelte";
  import { windowStore, type ProjectView } from "../../window.svelte";
  import Card from "./Card.svelte";
  import SettingRow from "./SettingRow.svelte";
  import ShareInit from "./ShareInit.svelte";

  const initialized = $derived(conditions.current.permark === "initialized");
  type Offer = Exclude<ProjectView["dataOffer"], "none">;
  /** The project on screen and what its window offers about the place of its data (`null`: nothing). */
  const offering = $derived.by((): { project: ProjectView; offer: Offer } | null => {
    const workspace = windowStore.screen?.workspace;
    if (workspace?.kind !== "project" || workspace.dataOffer === "none") return null;
    return { project: workspace, offer: workspace.dataOffer };
  });

  /** Where the data is now, in words, for each offer. */
  function placeHint(offer: Offer): MessageKey {
    switch (offer) {
      case "ask":
      case "movable":
        return "project.dataPlaceProfile";
      case "missing":
        return "project.dataMissing";
      case "both":
        return "project.dataMoveHeld";
      default:
        return offer satisfies never;
    }
  }
  /** The folder's shared settings file (what Everyone means), relative to the folder. */
  const SHARED_FILE = ".permark/config.toml";

  /** The card fields load only where the folder holds projects (a plain folder never needs them). */
  const cardFields = (): Promise<typeof import("../../projects/CardFields.svelte")> =>
    import("../../projects/CardFields.svelte");
</script>

<div class="overview">
  {#if !initialized}
    <ShareInit />
  {:else}
    <p class="scopes">{t("project.scopes")}</p>
    <Card>
      <SettingRow label={SHARED_FILE} hint={t("project.sharedFileHint")}>
        <button
          type="button"
          class="btn btn--sm"
          onclick={() => void documents.openExternal(SHARED_FILE, "reveal")}
        >
          {t("menu.showInFileManager")}
        </button>
      </SettingRow>
    </Card>
  {/if}
  {#if offering}
    {@const { project, offer } = offering}
    <Card>
      <SettingRow label={t("project.dataPlace")} hint={t(placeHint(offer))} scope="you">
        {#if offer === "ask" || offer === "movable"}
          <button
            type="button"
            class="btn btn--sm"
            onclick={() => void windowStore.moveData(project)}
          >
            {t("project.dataMove")}
          </button>
        {:else if offer === "missing"}
          <button
            type="button"
            class="btn btn--sm"
            onclick={() => void windowStore.keepData(project)}
          >
            {t("project.dataKeepCopy")}
          </button>
        {:else}
          <button
            type="button"
            class="btn btn--sm"
            onclick={() => void windowStore.revealFolderData()}
          >
            {t("menu.showInFileManager")}
          </button>
        {/if}
      </SettingRow>
    </Card>
  {/if}
  {#if projects.present && projects.view}
    {@const card = projects.view.rootCard}
    <!-- the opened folder's own card (it holds projects: its name shows where they are listed) -->
    <section class="root-card" aria-labelledby="proj-card">
      <h4 id="proj-card">{t("projects.card.title")}</h4>
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
  h4 {
    margin: 0 0 var(--sp-2);
    padding-left: var(--sp-1);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    font-weight: 600;
  }
</style>
