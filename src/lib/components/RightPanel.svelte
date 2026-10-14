<script lang="ts">
  import { t } from "../i18n";
  import { app } from "../stores/app.svelte";
  import { clock } from "../stores/clock.svelte";
  import { feed } from "../stores/feed.svelte";
  import { session } from "../stores/session.svelte";
  import { ago, relativeTime } from "../time";
  import FeedPanel from "./FeedPanel.svelte";
  import IconButton from "./IconButton.svelte";
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
</script>

<aside class="rpanel" aria-label={t("live.label")} inert={!session.right.open}>
  <Resizer side="right" />
  <div class="panel-inner" style:width="{session.right.width}px">
    <div class="panel-header">
      <span class="panel-title">{t("live.title")}</span>
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
      <IconButton
        small
        icon="chevrons-right"
        label={t("panel.collapseRight")}
        hint={t("panel.collapseRightHint")}
        onclick={() => app.toggleRight()}
      />
    </div>
    <div class="scroll" style="flex:1">
      <FeedPanel />
      <SinceLastVisit />
    </div>
  </div>
</aside>
