<script lang="ts">
  // The two file manager entries, each a switch. What is there is read from the registry each time the card opens
  // (another program or the installer may have changed it), and again after a change the backend refused, so each
  // switch shows what the registry holds, never the click that did not take. A first read that failed offers Try again.
  import { onMount } from "svelte";
  import type { ApiError, ShellEntry, ShellState } from "../../api";
  import { apiErrorText } from "../../core/errors";
  import { t } from "../../core/i18n";
  import { pluginClient } from "../../core/plugins/client";
  import SettingRow from "../../core/shell/settings/SettingRow.svelte";

  let entries = $state<ShellState | null>(null);
  let problem = $state<string | null>(null);

  const failed = (error: ApiError): string => t("shell.failed", { message: apiErrorText(error) });
  const unread = (error: ApiError): string =>
    t("shell.readFailed", { message: apiErrorText(error) });

  /** `again`: the person's Try again after a first read that failed: its answer replaces the line. */
  async function read(again = false): Promise<void> {
    const result = await pluginClient.call("shell-integration.state", {});
    if (result.ok) {
      entries = result.value;
      if (again) problem = null;
    } else if (again) {
      problem = unread(result.error);
    } else {
      // after a refused change, that refusal stays the line shown
      problem ??= unread(result.error);
    }
  }

  onMount(() => {
    void read();
  });

  async function set(entry: ShellEntry, on: boolean, input: HTMLInputElement): Promise<void> {
    const result = await pluginClient.call("shell-integration.set", { entry, on });
    if (result.ok) {
      entries = result.value;
      problem = null;
      return;
    }
    problem = failed(result.error);
    await read();
    // the switch drew the click itself: an unchanged state redraws nothing, so it is set back here
    if (entries !== null) input.checked = entries[entry];
  }
</script>

{#if entries !== null}
  <SettingRow label={t("shell.menu.row")} labelFor="set-shell-menu" hint={t("shell.menu.hint")}>
    <input
      id="set-shell-menu"
      type="checkbox"
      role="switch"
      checked={entries.menu}
      onchange={(e) => void set("menu", e.currentTarget.checked, e.currentTarget)}
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
      onchange={(e) => void set("newFile", e.currentTarget.checked, e.currentTarget)}
    />
  </SettingRow>
{/if}
{#if problem !== null}
  <div class="problem">
    <p role="alert">{problem}</p>
    <!-- nothing read yet: the card has no switches, only the way to read again -->
    {#if entries === null}
      <button type="button" class="btn btn--sm" onclick={() => void read(true)}
        >{t("shell.retry")}</button
      >
    {/if}
  </div>
{/if}

<style>
  .problem {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-2) var(--sp-3);
    padding: var(--sp-3) var(--sp-4);
  }
  .problem p {
    flex: 1 1 240px;
    min-width: 0;
    margin: 0;
    color: var(--git-deleted-text);
    font-size: var(--fs-ui-sm);
    overflow-wrap: anywhere;
  }
</style>
