<script lang="ts">
  import { t } from "../i18n";
  import { app } from "../stores/app.svelte";
  import { clock } from "../stores/clock.svelte";
  import { session } from "../stores/session.svelte";
  import { ui } from "../stores/ui.svelte";
  import { vault } from "../stores/vault.svelte";
  import { baselineLabel } from "../time";
  import Icon from "./Icon.svelte";
  import IconButton from "./IconButton.svelte";
  import Menu, { type MenuEntry } from "./Menu.svelte";

  let menu = $state<{ kind: "baseline" | "state"; anchor: DOMRect } | null>(null);

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
      ? t("status.changedMore", { count: vault.changedCount.toLocaleString() })
      : t("status.changed", { count: vault.changedCount.toLocaleString() }),
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
</script>

<footer class="statusbar">
  <IconButton
    small
    class="sb-panel-toggle"
    icon="panel-left"
    label={t("status.toggleLeft")}
    pressed={session.left.open}
    onclick={() => app.toggleLeft()}
  />
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
  <span class="spacer"></span>
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
    title={t("status.zoomHint")}
    onclick={() => session.setZoom(1)}
  >
    {Math.round(session.zoom * 100)}%
  </button>
  <button
    type="button"
    class="sb-item"
    title={t("status.widthHint")}
    onclick={() => session.toggleWide()}
  >
    <Icon name="width" />{session.wide ? t("status.wide") : t("status.normalWidth")}
  </button>
  <IconButton
    small
    class="sb-panel-toggle"
    icon="panel-right"
    label={t("status.toggleRight")}
    pressed={session.right.open}
    onclick={() => app.toggleRight()}
  />
</footer>

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
