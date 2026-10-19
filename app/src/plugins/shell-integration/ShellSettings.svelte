<script lang="ts">
  // The two file manager entries, each a switch. What is there is read from the registry each time the card opens
  // (another program or the installer may have changed it).
  import { onMount } from "svelte";
  import type { ShellEntry, ShellState } from "../../api";
  import { apiErrorText } from "../../core/errors";
  import { t } from "../../core/i18n";
  import { pluginClient } from "../../core/plugins/client";
  import SettingRow from "../../core/shell/settings/SettingRow.svelte";

  let entries = $state<ShellState | null>(null);
  let problem = $state<string | null>(null);

  onMount(() => {
    void pluginClient.call("shell-integration.state", {}).then((result) => {
      if (result.ok) entries = result.value;
    });
  });

  async function set(entry: ShellEntry, on: boolean): Promise<void> {
    const result = await pluginClient.call("shell-integration.set", { entry, on });
    if (result.ok) {
      entries = result.value;
      problem = null;
    } else {
      problem = t("shell.failed", { message: apiErrorText(result.error) });
    }
  }
</script>

{#if entries !== null}
  <SettingRow label={t("shell.menu.row")} labelFor="set-shell-menu" hint={t("shell.menu.hint")}>
    <input
      id="set-shell-menu"
      type="checkbox"
      role="switch"
      checked={entries.menu}
      onchange={(e) => void set("menu", e.currentTarget.checked)}
    />
  </SettingRow>
  <SettingRow
    label={t("shell.newFile.row")}
    labelFor="set-shell-new"
    hint={t("shell.newFile.hint")}
  >
    <input
      id="set-shell-new"
      type="checkbox"
      role="switch"
      checked={entries.newFile}
      onchange={(e) => void set("newFile", e.currentTarget.checked)}
    />
  </SettingRow>
  {#if problem !== null}<p class="problem" role="alert">{problem}</p>{/if}
{/if}

<style>
  .problem {
    margin: 0;
    padding: var(--sp-3) var(--sp-4);
    color: var(--git-deleted-text);
    font-size: var(--fs-ui-sm);
  }
</style>
