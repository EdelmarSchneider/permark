<script lang="ts">
  // The offer to set PerMark up while `.permark/` is missing: the folder's shared rows only show what they would hold
  // until the person answers the Initialize question (nothing is written into the folder before). The full card sits
  // on the folder's Overview; every other folder page shows it as one quiet line (`compact`), so it is not repeated
  // as a band on each page.
  import { t } from "../../i18n";
  import { askInitHere } from "../initProject";
  import Card from "./Card.svelte";
  import SettingRow from "./SettingRow.svelte";

  type Props = { compact?: boolean };
  let { compact = false }: Props = $props();
</script>

{#if compact}
  <p class="share-init compact">
    <span>{t("project.shareInit")}</span>
    <button type="button" class="link" onclick={() => askInitHere()}>
      {t("project.initButton")}
    </button>
  </p>
{:else}
  <div class="share-init">
    <Card>
      <SettingRow label={t("project.notInitialized")} hint={t("project.notInitializedHint")}>
        <button type="button" class="btn btn--primary btn--sm" onclick={() => askInitHere()}>
          {t("project.initButton")}
        </button>
      </SettingRow>
    </Card>
  </div>
{/if}

<style>
  .compact {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 0 var(--sp-2);
    margin: 0 0 var(--sp-4);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent-text);
    font: inherit;
    text-decoration: underline;
    cursor: pointer;
  }
</style>
