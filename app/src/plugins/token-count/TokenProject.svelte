<script lang="ts">
  // Settings -> Project -> Token count: the tokenizers this project counts with, for everyone who works on it (kept in
  // `.permark/config.toml`). Without a choice of its own the project uses the one for every project.
  import { t } from "../../core/i18n";
  import SettingRow from "../../core/shell/settings/SettingRow.svelte";
  import TokenChoice from "./TokenChoice.svelte";
</script>

<SettingRow
  label={t("tokens.projectRow")}
  labelId="set-project-tokenizers"
  hint={t("tokens.projectHint")}
  wide
>
  <div class="choice"><TokenChoice mode="project" /></div>
</SettingRow>

<style>
  .choice {
    flex: 1;
    min-width: 0;
  }
</style>
