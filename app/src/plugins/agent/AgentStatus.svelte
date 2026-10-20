<script lang="ts">
  // The agent item of the status bar, only while the switch is on: a dot and "Agent" (waiting), the client's name
  // (connected) or a problem. A click opens what is shared, the latest requests, Turn off and the setup command.
  import Bot from "@lucide/svelte/icons/bot";
  import type { AgentRequest } from "../../api";
  import { t, type MessageKey } from "../../core/i18n";
  import { claudeCodeCommand } from "./setup";
  import { GROUPS } from "./groups";
  import { handOff } from "./handoff.svelte";
  import { agent } from "./store.svelte";

  const status = $derived(agent.status);
  const clients = $derived(agent.connected);
  const phase = $derived.by(() => {
    const server = status?.server;
    if (server === undefined || server.state === "off") return "off" as const;
    if (server.state === "error") return "error" as const;
    return clients.length > 0 ? ("connected" as const) : ("idle" as const);
  });
  const label = $derived(
    phase === "connected"
      ? (clients[0] ?? "")
      : phase === "error"
        ? t("agent.status.error")
        : t("agent.status.idle"),
  );
  const problem = $derived.by(() => {
    const server = status?.server;
    if (server?.state !== "error") return null;
    return server.reason === "inUse"
      ? t("agent.popover.inUse")
      : t("agent.popover.error", { reason: server.reason });
  });
  const shared = $derived(
    GROUPS.filter((g) => status?.groups[g.id] === true && !status.denied.includes(g.id)),
  );

  const OUTCOMES: Readonly<Record<string, MessageKey>> = {
    denied: "agent.outcome.denied",
    notFound: "agent.outcome.notFound",
    invalid: "agent.outcome.invalid",
    off: "agent.outcome.off",
    notOpen: "agent.outcome.notOpen",
    error: "agent.outcome.error",
  };

  function outcome(request: AgentRequest): string | null {
    const key = OUTCOMES[request.outcome];
    return key === undefined ? null : t(key);
  }

  function time(ms: number): string {
    return new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  }

  let box: HTMLElement | undefined = $state();
  let copied = $state(false);

  /** The status bar clips what overflows it, so the popover is fixed and put above the item. */
  function above(node: HTMLElement): void {
    if (!box) return;
    const anchor = box.getBoundingClientRect();
    const left = Math.min(anchor.left, window.innerWidth - node.offsetWidth - 8);
    node.style.left = `${String(Math.max(8, left))}px`;
    node.style.bottom = `${String(window.innerHeight - anchor.top + 6)}px`;
  }

  async function copySetup(): Promise<void> {
    if (agent.program === null) await agent.loadProgram();
    await navigator.clipboard.writeText(claudeCodeCommand(agent.program));
    copied = true;
  }

  // closes on Escape | a click outside
  $effect(() => {
    if (!agent.popover) return;
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") agent.popover = false;
    };
    const onPointer = (e: PointerEvent): void => {
      if (box && e.target instanceof Node && !box.contains(e.target)) agent.popover = false;
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("pointerdown", onPointer);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("pointerdown", onPointer);
    };
  });
</script>

{#if agent.enabled}
  <span class="agent" bind:this={box}>
    <button
      type="button"
      class="chip"
      aria-haspopup="dialog"
      aria-expanded={agent.popover}
      title={phase === "connected"
        ? t("agent.status.hintConnected", { client: label })
        : t("agent.status.hintIdle")}
      onclick={() => {
        copied = false;
        agent.popover = !agent.popover;
      }}
    >
      <span class="dot is-{phase}" aria-hidden="true"></span>
      <Bot size={13} aria-hidden="true" />
      <span class="label">{label}</span>
    </button>
    {#if agent.popover}
      <div class="popover" role="dialog" aria-label={t("agent.title")} use:above>
        <p class="title">{t("agent.title")}</p>
        <p class="line" role="status">
          {#if problem !== null}
            {problem}
          {:else if phase === "connected"}
            {t("agent.popover.connected", { clients: clients.join(", ") })}
          {:else}
            {t("agent.popover.idle")}
          {/if}
        </p>
        <p class="label-row">{t("agent.popover.shared")}</p>
        <ul class="groups">
          {#each shared as group (group.id)}
            <li>{t(group.label)}</li>
          {/each}
        </ul>
        <p class="label-row">{t("agent.popover.recent")}</p>
        {#if (status?.recent.length ?? 0) === 0}
          <p class="muted">{t("agent.popover.none")}</p>
        {:else}
          <ul class="recent">
            {#each status?.recent ?? [] as request, i (i)}
              <li>
                <span class="tool">{request.tool}</span>
                <span class="subject" title={request.subject ?? ""}>{request.subject ?? ""}</span>
                {#if outcome(request) !== null}
                  <span class="outcome">{outcome(request)}</span>
                {/if}
                <span class="time">{time(request.atMs)}</span>
              </li>
            {/each}
          </ul>
        {/if}
        <div class="actions">
          <button
            type="button"
            class="btn btn--sm btn--primary"
            onclick={() => {
              agent.popover = false;
              void handOff.open();
            }}
          >
            {t("agent.handoff.button")}
          </button>
          <button type="button" class="btn btn--sm" onclick={() => void copySetup()}>
            {copied ? t("agent.copied") : t("agent.popover.copySetup")}
          </button>
          <button type="button" class="btn btn--sm" onclick={() => void agent.setEnabled(false)}>
            {t("agent.popover.turnOff")}
          </button>
        </div>
      </div>
    {/if}
  </span>
{/if}

<style>
  .agent {
    position: relative;
    display: inline-flex;
    align-items: center;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 0 6px;
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }
  .chip:hover,
  .chip[aria-expanded="true"] {
    background: var(--hover);
    color: var(--text);
  }
  .label {
    max-width: 16ch;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--muted);
  }
  .dot.is-connected {
    background: var(--git-added-text);
  }
  .dot.is-idle {
    background: var(--accent);
  }
  .dot.is-error {
    background: var(--git-deleted-text);
  }
  .popover {
    position: fixed;
    z-index: 40;
    width: 340px;
    max-width: calc(100vw - 24px);
    padding: var(--sp-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--bg);
    color: var(--text);
    box-shadow: var(--shadow-pop);
    font-size: var(--fs-ui-sm);
    white-space: normal;
  }
  .title {
    margin: 0 0 var(--sp-2);
    font-weight: 600;
  }
  .line {
    margin: 0 0 var(--sp-3);
  }
  .label-row {
    margin: var(--sp-2) 0 4px;
    color: var(--muted);
  }
  .muted {
    margin: 0;
    color: var(--muted);
  }
  ul {
    display: grid;
    gap: 3px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .groups {
    grid-template-columns: 1fr 1fr;
    column-gap: var(--sp-3);
  }
  .groups li::before {
    content: "✓ ";
    color: var(--git-added-text);
  }
  .recent li {
    display: flex;
    align-items: baseline;
    gap: var(--sp-2);
    min-width: 0;
  }
  .tool {
    font-family: var(--font-code);
  }
  .subject {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    color: var(--muted);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .outcome {
    color: var(--git-deleted-text);
  }
  .time {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: var(--sp-2);
    margin-top: var(--sp-3);
  }
</style>
