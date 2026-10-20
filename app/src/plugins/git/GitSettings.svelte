<script lang="ts">
  // Settings row of the git plugin: which git it uses.
  import type { GitBackendPref } from "../../api";
  import { t, type MessageKey } from "../../core/i18n";
  import SettingRow from "../../core/shell/settings/SettingRow.svelte";
  import { windowStore } from "../../core/window.svelte";

  const OPTIONS: { value: GitBackendPref; label: MessageKey }[] = [
    { value: "automatic", label: "git.backend.automatic" },
    { value: "system", label: "git.backend.system" },
    { value: "builtin", label: "git.backend.builtin" },
  ];

  function parse(value: string): GitBackendPref {
    return OPTIONS.find((o) => o.value === value)?.value ?? "automatic";
  }
</script>

<SettingRow label={t("git.backend.row")} labelFor="set-git" hint={t("git.backend.hint")}>
  <select
    id="set-git"
    class="field-select"
    value={windowStore.gitBackend}
    onchange={(e) =>
      void windowStore.changeSetting({ key: "gitBackend", value: parse(e.currentTarget.value) })}
  >
    {#each OPTIONS as option (option.value)}
      <option value={option.value}>{t(option.label)}</option>
    {/each}
  </select>
</SettingRow>
