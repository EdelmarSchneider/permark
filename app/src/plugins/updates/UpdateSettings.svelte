<script lang="ts">
  // Settings -> Integration -> Updates: where the updater stands, and the action that moves it on (Check now,
  // Download and install, Restart to update).
  import { t } from "../../core/i18n";
  import SettingRow from "../../core/shell/settings/SettingRow.svelte";
  import { actionOf, statusLine, type Action } from "./model";
  import { updates } from "./store.svelte";

  const LABELS: Readonly<
    Record<Action, "updates.checkNow" | "updates.download" | "updates.restart">
  > = {
    check: "updates.checkNow",
    download: "updates.download",
    restart: "updates.restart",
  };

  const action = $derived(actionOf(updates.state));
</script>

<SettingRow label={t("updates.row")} labelId="set-updates" hint={statusLine(updates.state)}>
  <button
    type="button"
    class="btn"
    disabled={action === null}
    aria-describedby="set-updates"
    onclick={() => {
      if (action !== null) void updates.run(action);
    }}
  >
    {t(LABELS[action ?? "check"])}
  </button>
</SettingRow>
