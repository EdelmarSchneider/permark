<script lang="ts">
  import { gitBadgeTitle } from "../gitLabels";
  import { t } from "../i18n";
  import { groupSince, isDubiousOwnership, safeDirectoryCommand } from "../since";
  import { app } from "../stores/app.svelte";
  import { clock } from "../stores/clock.svelte";
  import { docs } from "../stores/docs.svelte";
  import { session } from "../stores/session.svelte";
  import { ui } from "../stores/ui.svelte";
  import { quiet } from "../stores/quiet.svelte";
  import { unread } from "../stores/unread.svelte";
  import { vault } from "../stores/vault.svelte";
  import { baselineLabel } from "../time";
  import { badgeFor } from "../tree";
  import Icon from "./Icon.svelte";
  import IconButton from "./IconButton.svelte";
  import UnreadDot from "./UnreadDot.svelte";

  const SECTION = "since";
  const since = $derived(vault.since);
  const git = $derived(vault.git);
  const open = $derived(!ui.collapsedSections.has(SECTION));
  const visible = $derived(
    since ? since.files.filter((file) => !quiet.isQuiet(file.path)) : [],
  );
  const groups = $derived(groupSince(visible));
  const when = $derived(since?.sinceMs != null ? baselineLabel(since.sinceMs, clock.now) : null);
  const byFileTimes = $derived(!git?.isRepo || git.gitError !== null);
  const showHint = $derived(
    vault.stateLocation === "appData" && !ui.hintDismissed && !session.initHintNever,
  );
  const fixCommand = $derived(
    git?.gitError && isDubiousOwnership(git.gitError) && vault.info
      ? safeDirectoryCommand(vault.info.root)
      : null,
  );
</script>

<section class="panel-section">
  <button
    type="button"
    class="section-header"
    style="padding-left:10px"
    aria-expanded={open}
    onclick={() => ui.toggleSection(SECTION)}
  >
    <Icon name="chevron-down" size="sm" class="twisty" />{t("since.title")}
    {#if visible.length > 0}<span class="count-pill" style="margin-left:4px">{visible.length}</span
      >{/if}
    {#if when}
      <span
        class="section-header__aside"
        title={git?.baseline.oid
          ? t("since.baseline", { oid: git.baseline.oid.slice(0, 7) })
          : undefined}>{when}</span
      >
    {/if}
  </button>

  {#if open}
    {#if git?.gitError}
      <div class="git-error" role="alert">
        <strong>{t("git.error.title")}</strong>
        {git.gitError}
        <br />
        {t("git.error.fallback")}
        {#if fixCommand}
          <code>{fixCommand}</code>
          <button
            type="button"
            class="link-btn"
            onclick={() => void app.copy(fixCommand, "gitfix")}
          >
            {ui.flashed === "gitfix" ? t("common.copied") : t("common.copy")}
          </button>
        {/if}
      </div>
    {/if}

    {#if !vault.sinceLoaded}
      <p class="panel-note">{t("since.loading")}</p>
    {:else if since && since.sinceMs === null && since.files.length === 0}
      <p class="panel-note">{t("since.firstVisit")}</p>
    {:else if since && visible.length === 0}
      <p class="panel-note">{t("since.nothing", { when: when ?? "" })}</p>
    {:else}
      {#if byFileTimes && when}
        <p class="feed-note" style="padding-bottom:0">{t("since.byFileTimes", { when })}</p>
      {/if}
      {#each groups as group (group.folder)}
        <div class="since-folder">{group.folder === "" ? t("feed.root") : group.folder}</div>
        {#each group.files as file (file.path)}
          {@const badge = badgeFor(file.state)}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div
            class={["tree-row", "since-row", docs.path === file.path && "is-selected"]}
            role="button"
            tabindex="0"
            onclick={() => void app.openDoc(file.path)}
            onkeydown={(e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                void app.openDoc(file.path);
              }
            }}
          >
            <Icon name="file-text" class="file-icon" />
            {#if unread.isUnread(file.path)}<UnreadDot />{/if}
            <span class="tree-name">{file.name}</span>
            {#if badge}
              <span class={["badge", badge.className]} title={gitBadgeTitle(badge, "lastVisit")}
                >{badge.letter}</span
              >
            {/if}
          </div>
        {/each}
      {/each}
    {/if}

    {#if showHint}
      <div class="hint">
        <Icon name="lightbulb" size="sm" class="" />
        <span>
          {t("hint.keepSettings")}
          <button
            type="button"
            class="link-btn"
            title={t("initHint.detail")}
            onclick={() => (ui.initDialog = true)}
          >
            {t("hint.initialize")}
          </button>
        </span>
        <IconButton
          small
          icon="x"
          label={t("hint.dismiss")}
          hint={t("hint.dismissHint")}
          onclick={() => ui.dismissHint()}
        />
      </div>
    {/if}
  {/if}
</section>
