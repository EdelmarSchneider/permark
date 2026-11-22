<script lang="ts">
  // Settings > This folder > Token count: the tokenizers this project counts with, for everyone who works on it (kept in
  // `.permark/config.toml`). Without a choice of its own the project uses the one for every project. Without
  // `.permark/` the row shows greyed beside the offer to set PerMark up; with a shared file that does not read, greyed
  // beside a line that says so.
  import { t } from "../../core/i18n";
  import Card from "../../core/shell/settings/Card.svelte";
  import SettingRow from "../../core/shell/settings/SettingRow.svelte";
  import ShareInit from "../../core/shell/settings/ShareInit.svelte";
  import { tokens } from "./store.svelte";
  import TokenChoice from "./TokenChoice.svelte";

  const off = $derived(tokens.projectLocked);
</script>

{#if off}
  <ShareInit compact />
{/if}
<div class="shared" class:off>
  <Card>
    <SettingRow
      label={t("tokens.projectRow")}
      labelId="set-project-tokenizers"
      hint={t("tokens.projectHint")}
      scope="everyone"
      wide
    >
      <div class="choice"><TokenChoice mode="project" /></div>
    </SettingRow>
  </Card>
</div>

<style>
  /* without .permark/ (or with a shared file that does not read) the shared row only shows what it holds
     (its fields grey; its buttons grey by their own disabled look): labels and hints keep their contrast */
  .shared.off :global(input),
  .shared.off :global(select) {
    opacity: 0.6;
  }
  .choice {
    flex: 1;
    min-width: 0;
  }
</style>
