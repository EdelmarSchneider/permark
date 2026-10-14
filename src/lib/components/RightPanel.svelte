<script lang="ts">
  import { t, type MessageKey } from "../i18n";
  import { RIGHT_TABS, stepTab, type RightTab } from "../rightPanel";
  import { app } from "../stores/app.svelte";
  import { clock } from "../stores/clock.svelte";
  import { feed } from "../stores/feed.svelte";
  import { session } from "../stores/session.svelte";
  import { ui } from "../stores/ui.svelte";
  import { ago, relativeTime } from "../time";
  import FeedPanel from "./FeedPanel.svelte";
  import IconButton from "./IconButton.svelte";
  import OutlineList from "./OutlineList.svelte";
  import Resizer from "./Resizer.svelte";
  import SinceLastVisit from "./SinceLastVisit.svelte";

  const ACTIVE_MS = 30_000;
  const last = $derived(feed.lastEventMs);
  const active = $derived(last !== null && clock.now - last < ACTIVE_MS);
  const status = $derived(
    last === null
      ? t("live.idle")
      : active
        ? t("live.active")
        : t("live.idleFor", { time: relativeTime(last, clock.now) }),
  );
  const statusHint = $derived(
    last === null ? undefined : t("live.lastChange", { ago: ago(last, clock.now) }),
  );

  const labels: Record<RightTab, { name: MessageKey; hint: MessageKey }> = {
    changes: { name: "panel.tabChanges", hint: "panel.tabChangesHint" },
    outline: { name: "panel.tabOutline", hint: "panel.tabOutlineHint" },
  };

  let tablist: HTMLElement | undefined = $state();

  /** Left and right move between the tabs, like any tab list (the tab in focus is the one showing). */
  function onTabKeys(e: KeyboardEvent): void {
    const direction = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0;
    if (direction === 0 || e.ctrlKey || e.metaKey || e.altKey) return;
    e.preventDefault();
    const next = stepTab(session.rightTab, direction);
    app.showRightTab(next);
    tablist?.querySelector<HTMLElement>(`[data-tab="${next}"]`)?.focus();
  }
</script>

<aside class="rpanel" aria-label={t("panel.label")} inert={!session.right.open}>
  <Resizer side="right" />
  <div class="panel-inner" style:width="{session.right.width}px">
    <div class="panel-header">
      <!-- the arrow keys are the tab list's own keyboard model; the tabs themselves are the buttons -->
      <!-- svelte-ignore a11y_interactive_supports_focus -->
      <div
        bind:this={tablist}
        class="segmented"
        role="tablist"
        aria-label={t("panel.tabs")}
        tabindex="-1"
        onkeydown={onTabKeys}
      >
        {#each RIGHT_TABS as tab (tab)}
          <button
            type="button"
            role="tab"
            id="rpanel-tab-{tab}"
            data-tab={tab}
            aria-selected={session.rightTab === tab}
            aria-controls="rpanel-view-{tab}"
            tabindex={session.rightTab === tab ? 0 : -1}
            title={t(labels[tab].hint)}
            onclick={() => app.showRightTab(tab)}
          >
            {t(labels[tab].name)}
            {#if tab === "changes" && ui.changesNotify && session.rightTab !== "changes"}
              <span class="tab-dot"></span>
              <span class="sr-only">{t("panel.newChanges")}</span>
            {/if}
          </button>
        {/each}
      </div>
      <span class="spacer"></span>
      <IconButton
        small
        icon="chevrons-right"
        label={t("panel.collapseRight")}
        hint={t("panel.collapseRightHint")}
        onclick={() => app.toggleRight()}
      />
    </div>
    <div
      id="rpanel-view-changes"
      class="panel-inner"
      role="tabpanel"
      aria-labelledby="rpanel-tab-changes"
      hidden={session.rightTab !== "changes"}
    >
      <div class="panel-subheader">
        <span class="live-status" title={statusHint}>
          <span class={["live-dot", !active && "live-dot--idle"]}></span>{status}
        </span>
        <span class="spacer"></span>
        <button
          type="button"
          class="chip-toggle"
          aria-pressed={feed.docsOnly}
          title={t("live.docsOnlyHint")}
          onclick={() => feed.toggleDocsOnly()}
        >
          {t("live.docsOnly")}
        </button>
      </div>
      <div class="scroll" style="flex:1">
        <FeedPanel />
        <SinceLastVisit />
      </div>
    </div>
    <div
      id="rpanel-view-outline"
      class="panel-inner"
      role="tabpanel"
      aria-labelledby="rpanel-tab-outline"
      hidden={session.rightTab !== "outline"}
    >
      <OutlineList active={session.rightTab === "outline" && session.right.open} />
    </div>
  </div>
</aside>
