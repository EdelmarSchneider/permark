<script lang="ts">
  // The activity bar of the Notes window: the list of notes (its icon shows or hides it), the theme and the settings.
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
    icon="sticky-note"
    label={t("notes.title")}
    pressed={session.left.open}
    onclick={() => app.toggleLeft()}
  />
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
