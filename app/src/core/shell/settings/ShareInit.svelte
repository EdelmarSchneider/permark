<script lang="ts">
  // The line above a folder's shared rows while they can't be changed. While `.permark/` is missing: the offer to set
  // PerMark up (the rows only show what they would hold until the person answers the Initialize question; nothing is
  // written into the folder before). The full card sits on the folder's Overview; every other folder page shows it as
  // one quiet line (`compact`), so it is not repeated as a band on each page. Once set up, a shared file that does not
  // read keeps the rows locked too: the compact line says so and why (the Overview's reasons), and leads there.
  import type { ConfigUnreadableWhy } from "../../../api/generated/ConfigUnreadableWhy";
  import type { MessageKey } from "../../i18n";
  import { t } from "../../i18n";
  import { askInitHere } from "../initProject";
  import { ui } from "../ui.svelte";
  import Card from "./Card.svelte";
  import SettingRow from "./SettingRow.svelte";
  import { projectSettings } from "./stores.svelte";

  type Props = { compact?: boolean };
  let { compact = false }: Props = $props();

  /** Why the shared rows stay locked while the shared file does not read: where it stops being TOML, or why it is no
   *  text PerMark can read. */
  const lockedText = $derived.by((): string => {
    if (projectSettings.state.status !== "ready") return t("project.sharedLocked");
    const { unreadableLine, unreadableWhy } = projectSettings.state.settings;
    if (unreadableLine !== null) return t("project.sharedLocked.line", { line: unreadableLine });
    return t(unreadableWhy === null ? "project.sharedLocked" : whyKey(unreadableWhy));
  });

  function whyKey(why: ConfigUnreadableWhy): MessageKey {
    switch (why) {
      case "notText":
        return "project.sharedLocked.notText";
      case "tooLarge":
        return "project.sharedLocked.tooLarge";
      case "locked":
        return "project.sharedLocked.locked";
      default:
        return why satisfies never;
    }
  }
</script>

{#if compact && projectSettings.unreadable}
  <p class="share-init compact">
    <span>{lockedText}</span>
    <button type="button" class="link" onclick={() => ui.showSettingsSection("project")}>
      {t("settings.section.project")}
    </button>
  </p>
{:else if compact}
  <p class="share-init compact">
    <span>{t("project.shareInit")}</span>
    <button type="button" class="link" onclick={() => askInitHere()}>
      {t("project.initButton")}
    </button>
  </p>
{:else}
  <div class="share-init">
    <Card>
      <SettingRow label={t("project.notInitialized")} hint={t("project.notInitializedHint")}>
        <button type="button" class="btn btn--primary btn--sm" onclick={() => askInitHere()}>
          {t("project.initButton")}
        </button>
      </SettingRow>
    </Card>
  </div>
{/if}

<style>
  .compact {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 0 var(--sp-2);
    margin: 0 0 var(--sp-4);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent-text);
    font: inherit;
    text-decoration: underline;
    cursor: pointer;
  }
</style>
