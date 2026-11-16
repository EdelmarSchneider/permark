<script lang="ts">
  // The offer shown above a folder's shared rows while `.permark/` is missing: those rows only show what they would
  // hold until the person answers the Initialize question (nothing is written into the folder before).
  import { t } from "../../i18n";
  import { askInitHere } from "../initProject";
  import Card from "./Card.svelte";
  import SettingRow from "./SettingRow.svelte";
</script>

<div class="share-init">
  <Card>
    <SettingRow label={t("project.shareInit")}>
      <button type="button" class="btn btn--primary btn--sm" onclick={() => askInitHere()}>
        {t("project.initButton")}
      </button>
    </SettingRow>
  </Card>
</div>

<style>
  .share-init {
    margin-bottom: var(--sp-5);
  }
</style>
