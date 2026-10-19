<script lang="ts">
  // Settings row of the git plugin: which git it uses.
  import type { GitBackendPref } from "../../api";
  import { t, type MessageKey } from "../../core/i18n";
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

<label class="row" title={t("git.backend.hint")}>
  <span>{t("git.backend.label")}</span>
  <select
    value={windowStore.gitBackend}
    onchange={(e) =>
      void windowStore.changeSetting({ key: "gitBackend", value: parse(e.currentTarget.value) })}
  >
    {#each OPTIONS as option (option.value)}
      <option value={option.value}>{t(option.label)}</option>
    {/each}
  </select>
</label>

<style>
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-3);
    padding: 6px 10px;
  }
  select {
    min-width: 160px;
    height: 28px;
    padding: 0 6px;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
  }
</style>
