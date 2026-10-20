<script lang="ts">
  // Settings -> Integration -> Token count: which tokenizers count (for every project), the vocabularies the exact
  // ones need (each downloaded once, on a press of Download; Remove deletes it again), and whether small folders show
  // their total as they are opened.
  import { t } from "../../core/i18n";
  import SettingRow from "../../core/shell/settings/SettingRow.svelte";
  import { tokens } from "./store.svelte";
  import TokenChoice from "./TokenChoice.svelte";
</script>

<SettingRow label={t("tokens.row")} labelId="set-tokenizers" hint={t("tokens.rowHint")} wide>
  <div class="choice"><TokenChoice mode="app" removable /></div>
</SettingRow>
<SettingRow
  label={t("tokens.folder.totalsRow")}
  labelFor="set-folder-totals"
  hint={t("tokens.folder.totalsHint")}
>
  <input
    id="set-folder-totals"
    type="checkbox"
    role="switch"
    checked={tokens.state?.folderTotals ?? false}
    onchange={(e) => void tokens.setFolderTotals(e.currentTarget.checked)}
  />
</SettingRow>

<style>
  .choice {
    flex: 1;
    min-width: 0;
  }
</style>
