<script lang="ts">
  // Settings -> Integration -> Token count: which tokenizers count (for every project), and the vocabularies the exact
  // ones need (each downloaded once, on a press of Download; Remove deletes it again).
  import { t } from "../../core/i18n";
  import SettingRow from "../../core/shell/settings/SettingRow.svelte";
  import TokenChoice from "./TokenChoice.svelte";
</script>

<SettingRow label={t("tokens.row")} labelId="set-tokenizers" hint={t("tokens.rowHint")} wide>
  <div class="choice"><TokenChoice mode="app" removable /></div>
</SettingRow>

<style>
  .choice {
    flex: 1;
    min-width: 0;
  }
</style>
