<script lang="ts">
  import { formatNumber, t, tn } from "../i18n";
  import { agent } from "../stores/agent.svelte";
  import { app } from "../stores/app.svelte";
  import { clock } from "../stores/clock.svelte";
  import { docs } from "../stores/docs.svelte";
  import { edit } from "../stores/edit.svelte";
  import { media } from "../stores/media.svelte";
  import { comments } from "../stores/comments.svelte";
  import { session } from "../stores/session.svelte";
  import { ui } from "../stores/ui.svelte";
  import { updates } from "../stores/updates.svelte";
  import { vault } from "../stores/vault.svelte";
  import { baselineLabel } from "../time";
  import { describeUpdate } from "../updates";
  import AgentPopover from "./AgentPopover.svelte";
  import Icon from "./Icon.svelte";
  import IconButton from "./IconButton.svelte";
  import Menu, { type MenuEntry } from "./Menu.svelte";

  let menu = $state<{ kind: "baseline" | "state"; anchor: DOMRect } | null>(null);

  /** A picture is on show: the zoom is the picture's. */
  const image = $derived(docs.current?.kind === "image");
  const git = $derived(vault.git);
  const head = $derived(git?.head);
  const shortOid = $derived(head?.oid ? head.oid.slice(0, 7) : null);
  const branchLabel = $derived(
    !head
      ? ""
      : head.unborn
        ? t("status.unborn")
        : head.detached
          ? t("status.detached", { oid: shortOid ?? "" })
          : (head.branch ?? ""),
  );
  const changed = $derived(
    git?.truncated
      ? t("status.changedMore", { count: formatNumber(vault.changedCount) })
      : t("status.changed", { count: formatNumber(vault.changedCount) }),
  );
  const baselineKind = $derived(git?.baseline.kind ?? "lastVisit");
  const baselineText = $derived(
    git?.gitError
      ? t("status.sinceFileTimes")
      : baselineKind === "head"
        ? t("status.sinceCommit")
        : t("status.sinceVisit"),
  );
  const rebasing = $derived(git && git.repoState !== "clean");
  const sinceMs = $derived(vault.since?.sinceMs ?? null);
  const update = $derived(updates.state);

  const baselineEntries = $derived<MenuEntry[]>([
    {
      id: "lastVisit",
      label: t("status.sinceVisit"),
      hint: sinceMs !== null ? baselineLabel(sinceMs, clock.now) : undefined,
      checked: baselineKind === "lastVisit",
      onselect: () => void app.setBaseline("lastVisit"),
    },
    {
      id: "head",
      label: t("status.sinceCommit"),
      hint: shortOid ?? undefined,
      checked: baselineKind === "head",
      onselect: () => void app.setBaseline("head"),
    },
  ]);
  const stateEntries = $derived<MenuEntry[]>([
    {
      id: "init",
      label:
        vault.stateLocation === "project" ? t("explorer.stateInProject") : t("explorer.initialize"),
      icon: "folder-cog",
      disabled: vault.stateLocation === "project",
      onselect: () => (ui.initDialog = true),
    },
  ]);

  function openMenu(kind: "baseline" | "state", e: MouseEvent): void {
    const anchor = (e.currentTarget as HTMLElement).getBoundingClientRect();
    menu = menu?.kind === kind ? null : { kind, anchor };
  }

  // the agent connection: its item in the status bar and the popover under it
  let agentAnchor = $state<DOMRect | null>(null);
  const agentTitle = $derived(
    agent.status.error !== null
      ? t("agent.item.error")
      : agent.connected
        ? t("agent.item.connected")
        : t("agent.item.idle"),
  );
  function toggleAgent(e: MouseEvent): void {
    const anchor = (e.currentTarget as HTMLElement).getBoundingClientRect();
    agentAnchor = agentAnchor === null ? anchor : null;
  }
  // turned off from elsewhere (Settings, the palette): the popover has nothing to show
  $effect(() => {
    if (!agent.enabled) agentAnchor = null;
  });
</script>

<footer class="statusbar">
  <div class="sb-group sb-group--start">
    <IconButton
      small
      class="sb-panel-toggle"
      icon="panel-left"
      label={t("status.toggleLeft")}
      pressed={session.left.open}
      onclick={() => app.toggleLeft()}
    />
    {#if vault.info && !vault.info.liveUpdates}
      <span class="sb-item is-warn" title={t("status.liveOffHint")}>
        <Icon name="alert" />{t("status.liveOff")}
      </span>
    {/if}
    {#if git?.gitError}
      <span class="sb-item is-warn" title={git.gitError}>
        <Icon name="alert" />{t("status.gitUnavailable")}
      </span>
    {:else if git && !git.isRepo}
      <span class="sb-item" title={t("status.noRepoHint")}>{t("status.noRepo")}</span>
    {:else if git}
      <span
        class="sb-item"
        title={t("status.branchHint", { branch: branchLabel, oid: shortOid ?? "" })}
      >
        <Icon name="branch" />{branchLabel}
      </span>
      <span class="sb-item" title={t("status.changedHint")}>
        <span class="rollup" style="margin:0"></span>{changed}
      </span>
    {/if}
    {#if git?.isRepo || git?.gitError}
      <button
        type="button"
        class="sb-item"
        title={t("status.baselineHint")}
        data-popover-trigger
        disabled={!!git?.gitError}
        onclick={(e) => openMenu("baseline", e)}
      >
        {baselineText}<Icon name="chevron-down" class="chev" />
      </button>
    {/if}
    {#if rebasing && git}
      <span class="sb-chip">{t(`status.chip.${git.repoState}`)}</span>
    {/if}
    {#if edit.active}
      <span class="sb-item">{t("edit.statusMode", { mode: t(`edit.${edit.mode}`) })}</span>
      {#if edit.mode === "visual" && edit.unsavedBlocks > 0}
        <span class="sb-item">{tn("edit.unsavedBlocks", edit.unsavedBlocks)}</span>
      {/if}
      {#if edit.mode === "source"}
        <span class="sb-item" aria-live="off">
          {t("edit.position", { line: edit.caret.line, col: edit.caret.column })}
        </span>
      {/if}
      {#if session.follow}<span class="sb-chip">{t("edit.followPaused")}</span>{/if}
    {/if}
  </div>
  <div class="sb-group sb-group--end">
    {#if update.phase === "available"}
      <button
        type="button"
        class="sb-item is-on"
        title={t("update.statusHint", { version: update.version })}
        onclick={() => updates.announce()}
      >
        <Icon name="download" />{t("update.statusAvailable", { version: update.version })}
      </button>
    {:else if update.phase === "downloading" || update.phase === "installing"}
      <button
        type="button"
        class="sb-item is-on"
        title={describeUpdate(update)}
        onclick={() => updates.announce()}
      >
        <Icon name="download" />{describeUpdate(update)}
      </button>
    {/if}
    {#if comments.count > 0 && !vault.isSimple}
      <button
        type="button"
        class="sb-item"
        title={t("comments.statusHint")}
        onclick={() => app.showComments()}
      >
        <Icon name="comment" />{tn("comments.count", comments.count)}
      </button>
    {/if}
    {#if agent.enabled}
      <button
        type="button"
        class={[
          "sb-item",
          "agent-item",
          agent.connected && "is-on",
          agent.pulsing && "is-pulsing",
          agent.status.error !== null && "is-warn",
        ]}
        title={agentTitle}
        aria-label={agentTitle}
        aria-haspopup="dialog"
        aria-expanded={agentAnchor !== null}
        data-popover-trigger
        onclick={toggleAgent}
      >
        <span class={["agent-dot", agent.connected && "is-connected"]} aria-hidden="true"></span>
        {t("agent.item")}
      </button>
    {/if}
    <button
      type="button"
      class="sb-item"
      title={vault.stateLocation === "project"
        ? t("status.stateProjectHint")
        : t("status.stateLocalHint")}
      data-popover-trigger
      onclick={(e) => openMenu("state", e)}
    >
      <Icon name="folder-cog" />{vault.stateLocation === "project"
        ? t("status.stateProject")
        : t("status.stateLocal")}
    </button>
    <button
      type="button"
      class={["sb-item", session.follow && "is-on"]}
      aria-pressed={session.follow}
      title={t("status.followHint")}
      onclick={() => app.toggleFollow()}
    >
      <Icon name="eye" />{session.follow ? t("status.following") : t("status.follow")}
    </button>
    <button
      type="button"
      class="sb-item"
      title={image ? t("status.imageZoomHint") : t("status.zoomHint")}
      onclick={() => app.resetZoom()}
    >
      {image ? media.label : `${Math.round(session.zoom * 100)}%`}
    </button>
    <button
      type="button"
      class="sb-item"
      title={t("status.widthHint", { name: t(`width.${session.width}`) })}
      onclick={() => session.cycleWidth()}
    >
      <Icon name="width" />{t(`width.${session.width}`)}
    </button>
    <IconButton
      small
      class="sb-panel-toggle"
      icon="panel-right"
      label={t("status.toggleRight")}
      pressed={session.right.open}
      onclick={() => app.toggleRight()}
    />
  </div>
</footer>

{#if agentAnchor}
  <AgentPopover anchor={agentAnchor} onclose={() => (agentAnchor = null)} />
{/if}

{#if menu}
  <Menu
    label={menu.kind === "baseline" ? t("status.baselineMenu") : t("status.stateMenu")}
    anchor={menu.anchor}
    side="top"
    align={menu.kind === "state" ? "end" : "start"}
    entries={menu.kind === "baseline" ? baselineEntries : stateEntries}
    onclose={() => (menu = null)}
  />
{/if}
