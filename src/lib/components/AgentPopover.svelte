<script lang="ts">
  // The agent connection from the status bar: what is shared, what agents asked lately, and the way to turn it off.
  import { t } from "../i18n";
  import { agent } from "../stores/agent.svelte";
  import { clock } from "../stores/clock.svelte";
  import { ui } from "../stores/ui.svelte";
  import { ago } from "../time";
  import type { AgentRequest } from "../types";
  import Popover, { type Anchor } from "./Popover.svelte";

  type Props = { anchor: Anchor; onclose: () => void };
  let { anchor, onclose }: Props = $props();

  const REQUEST_WORDS = {
    get_context: "agent.request.get_context",
    list_unread: "agent.request.list_unread",
    search: "agent.request.search",
    list_notes: "agent.request.list_notes",
    resolve_note: "agent.request.resolve_note",
    open: "agent.request.open",
  } as const;

  const status = $derived(agent.status);
  const state = $derived(
    status.error !== null
      ? t("agent.state.error", { message: status.error })
      : agent.connected
        ? t("agent.state.connected")
        : t("agent.state.idle"),
  );

  /** What an agent did, in a few words ("Searched for rollout"). */
  function what(request: AgentRequest): string {
    const key = REQUEST_WORDS[request.tool as keyof typeof REQUEST_WORDS];
    return key ? t(key, { subject: request.subject ?? "" }) : request.tool;
  }

  async function turnOff(): Promise<void> {
    onclose();
    await agent.setEnabled(false);
  }

  function setup(): void {
    onclose();
    ui.settingsOpen = true;
  }
</script>

<Popover {anchor} variant="panel" wide side="top" align="end" label={t("agent.title")} {onclose}>
  <div class="agent-pop__head">
    <span class={["agent-dot", agent.connected && "is-connected"]} aria-hidden="true"></span>
    <div>
      <span class="agent-pop__title">{t("agent.title")}</span>
      <span class="field__status" role="status">{state}</span>
    </div>
  </div>
  <div class="field">
    <span class="field__label">{t("agent.shares")}</span>
    <ul class="agent-list">
      <li>{t("agent.shares.context")}</li>
      <li>{t("agent.shares.unread")}</li>
      <li>{t("agent.shares.search")}</li>
      <li>{t("agent.shares.notes")}</li>
      <li>{t("agent.shares.show")}</li>
    </ul>
    <span class="field__status">{t("agent.shares.leaves")}</span>
    <span class="field__status">{t("agent.shares.file")}</span>
  </div>
  <div class="field">
    <span class="field__label">{t("agent.recent")}</span>
    {#if status.recent.length === 0}
      <span class="field__status">{t("agent.recent.none")}</span>
    {:else}
      <ul class="agent-requests">
        {#each status.recent.slice(0, 5) as request (request.atMs + request.tool)}
          <li>
            <span class="agent-requests__what">{what(request)}</span>
            <span class="agent-requests__when">{ago(request.atMs, clock.now)}</span>
          </li>
        {/each}
      </ul>
    {/if}
  </div>
  <div class="agent-pop__actions">
    <button type="button" class="btn btn--sm" onclick={setup}>{t("agent.setup")}</button>
    <button type="button" class="btn btn--sm" disabled={agent.busy} onclick={() => void turnOff()}>
      {t("agent.turnOff")}
    </button>
  </div>
</Popover>
