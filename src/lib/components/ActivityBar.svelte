<script lang="ts">
  import { t } from "../i18n";
  import { app } from "../stores/app.svelte";
  import { session } from "../stores/session.svelte";
  import { ui } from "../stores/ui.svelte";
  import IconButton from "./IconButton.svelte";

  const themeIcon = $derived(
    session.theme === "system" ? "monitor" : session.theme === "light" ? "sun" : "moon",
  );
  const themeName = $derived(t(`theme.${session.theme}`));
</script>

<nav class="activitybar" aria-label={t("activity.label")}>
  <IconButton
    class="ab-btn"
    icon="files"
    label={t("activity.explorer")}
    pressed={ui.sidebarView === "explorer" && session.left.open}
    onclick={() => app.clickView("explorer")}
  />
  <IconButton
    class="ab-btn"
    icon="search"
    label={t("activity.search")}
    pressed={ui.sidebarView === "search" && session.left.open}
    onclick={() => app.clickView("search")}
  />
  <IconButton
    class="ab-btn ab-btn--toggle"
    icon="activity"
    label={t("activity.live")}
    pressed={session.right.open}
    onclick={() => app.toggleRight()}
  >
    {#if ui.rightNotify && !session.right.open}<span class="notify-dot"></span>{/if}
  </IconButton>
  <span class="spacer"></span>
  <IconButton
    class="ab-btn"
    icon={themeIcon}
    label={t("activity.theme", { theme: themeName })}
    onclick={() => session.cycleTheme()}
  />
  <IconButton
    class="ab-btn"
    icon="settings"
    label={t("activity.settings")}
    data-settings-anchor
    data-popover-trigger
    onclick={() => (ui.settingsOpen = !ui.settingsOpen)}
  />
</nav>
