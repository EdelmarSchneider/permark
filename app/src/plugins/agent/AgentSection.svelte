<script lang="ts">
  // Settings > AI agents (project windows): the state of this project's sharing and one button to turn it on; while on,
  // the steps that connect an agent right below (Claude Code first); what agents may do, folded. Nothing turns on by
  // itself: the person sees what is shared, then decides.
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import { tick, untrack } from "svelte";
  import { conditions } from "../../core/conditions/context.svelte";
  import { t, type MessageKey } from "../../core/i18n";
  import Card from "../../core/shell/settings/Card.svelte";
  import { ui } from "../../core/shell/ui.svelte";
  import { GROUPS } from "./groups";
  import { AGENT_PAGE } from "./offer.svelte";
  import { claudeCodeCommand, hookJson, mcpJson } from "./setup";
  import { agent } from "./store.svelte";

  const status = $derived(agent.status);
  const phase = $derived.by(() => {
    if (status === null || !status.enabled) return "off" as const;
    if (status.server.state === "error") return "error" as const;
    return agent.connected.length > 0 ? ("connected" as const) : ("listening" as const);
  });
  const stateText = $derived.by(() => {
    switch (phase) {
      case "off":
        return t("agent.state.off");
      case "listening":
        return t("agent.state.listening");
      case "connected":
        return t("agent.state.connected", { clients: agent.connected.join(", ") });
      case "error":
        return t("agent.state.error");
      default:
        return phase satisfies never;
    }
  });
  const problem = $derived.by(() => {
    const server = status?.server;
    if (server?.state !== "error") return null;
    return server.reason === "inUse"
      ? t("agent.popover.inUse")
      : t("agent.popover.error", { reason: server.reason });
  });
  const allowed = $derived(
    GROUPS.filter((g) => status?.groups[g.id] === true && !status.denied.includes(g.id)).length,
  );
  const initialized = $derived(conditions.current.permark === "initialized");

  const MORE: { id: string; label: MessageKey; text: (program: string | null) => string }[] = [
    { id: "json", label: "agent.setup.json", text: mcpJson },
    { id: "hook", label: "agent.setup.hook", text: hookJson },
  ];

  let primary: HTMLButtonElement | undefined = $state();
  let firstCopy: HTMLButtonElement | undefined = $state();
  let copied = $state<string | null>(null);
  let failed = $state(false);
  let busy = $state(false);
  let announcement = $state("");
  let moreOpen = $state(false);
  let groupsOpen = $state(false);
  const moreId = $props.id();
  const groupsId = `${moreId}-groups`;

  // the setup lines name the program by its path: asked once, when they show
  $effect(() => {
    if (agent.enabled && agent.program === null) void agent.loadProgram();
  });

  // opened from "Set up…" (offer, gear row, palette): focus on the first control, once
  $effect(() => {
    // the control exists once its element is bound (reads it: runs again then)
    const first = status === null ? undefined : status.enabled ? firstCopy : primary;
    if (first === undefined) return;
    untrack(() => {
      if (ui.takeContentFocus(AGENT_PAGE)) first.focus();
    });
  });

  async function turn(on: boolean): Promise<void> {
    if (busy) return;
    busy = true;
    const ok = await agent.setEnabled(on, true);
    busy = false;
    failed = !ok;
    if (!ok) return;
    copied = null;
    announcement = on ? t("agent.turnedOn") : t("agent.state.off");
    await tick();
    (on ? firstCopy : primary)?.focus();
  }

  async function copy(id: string, text: string): Promise<void> {
    await navigator.clipboard.writeText(text);
    copied = id;
  }
</script>

{#if status !== null}
  <Card>
    <div class="state-card">
      <p class="state">
        {#if phase === "error"}
          <TriangleAlert size={14} class="alert" aria-hidden="true" />
        {:else}
          <span class={["dot", phase !== "off" && "on"]} aria-hidden="true"></span>
        {/if}
        <span>{stateText}</span>
      </p>
      {#if phase === "off"}
        <p class="about">{t("agent.switchHint")}</p>
        <div class="actions">
          <button
            type="button"
            class="btn btn--primary wrap"
            bind:this={primary}
            disabled={busy}
            onclick={() => void turn(true)}
          >
            {t("agent.switch")}
          </button>
        </div>
        <p class="muted">{t("agent.offAnyTime")}</p>
      {:else}
        {#if problem !== null}
          <p class="about">{problem}</p>
        {/if}
        <div class="actions">
          <button type="button" class="btn" disabled={busy} onclick={() => void turn(false)}>
            {t("agent.popover.turnOff")}
          </button>
        </div>
      {/if}
      {#if failed}
        <p class="failed" role="alert">{t("agent.notSaved")}</p>
      {/if}
    </div>
  </Card>

  {#if phase !== "off"}
    <h4 class="group-title">{t("agent.connect")}</h4>
    <Card>
      <div class="setup">
        <div class="setup-head">
          <span>{t("agent.setup.claudeStep")}</span>
          <button
            type="button"
            class="btn btn--sm"
            bind:this={firstCopy}
            onclick={() => void copy("claude", claudeCodeCommand(agent.program))}
          >
            {copied === "claude" ? t("agent.copied") : t("agent.copy")}
          </button>
        </div>
        <pre>{claudeCodeCommand(agent.program)}</pre>
        <button
          type="button"
          class="disclosure"
          aria-expanded={moreOpen}
          aria-controls={moreId}
          onclick={() => (moreOpen = !moreOpen)}
        >
          {#if moreOpen}<ChevronDown size={14} aria-hidden="true" />{:else}<ChevronRight
              size={14}
              aria-hidden="true"
            />{/if}
          {t("agent.setup.more")}
        </button>
        {#if moreOpen}
          <div class="more" id={moreId} role="region" aria-label={t("agent.setup.more")}>
            {#each MORE as line (line.id)}
              {@const text = line.text(agent.program)}
              <div class="setup-head">
                <span>{t(line.label)}</span>
                <button type="button" class="btn btn--sm" onclick={() => void copy(line.id, text)}>
                  {copied === line.id ? t("agent.copied") : t("agent.copy")}
                </button>
              </div>
              <pre>{text}</pre>
            {/each}
          </div>
        {/if}
      </div>
    </Card>
  {/if}

  <button
    type="button"
    class="disclosure outer"
    aria-expanded={groupsOpen}
    aria-controls={groupsId}
    onclick={() => (groupsOpen = !groupsOpen)}
  >
    {#if groupsOpen}<ChevronDown size={14} aria-hidden="true" />{:else}<ChevronRight
        size={14}
        aria-hidden="true"
      />{/if}
    {t("agent.groupsCount", { n: allowed, total: GROUPS.length })}
  </button>
  {#if groupsOpen}
    <div id={groupsId} role="region" aria-label={t("agent.groups")}>
      <Card>
        <div class="groups-card">
          <fieldset class="groups">
            <legend class="sr-only">{t("agent.groups")}</legend>
            {#each GROUPS as group (group.id)}
              {@const denied = status.denied.includes(group.id)}
              <label class="group" title={denied ? t("agent.denied") : ""}>
                <input
                  type="checkbox"
                  checked={status.groups[group.id] === true && !denied}
                  disabled={denied}
                  onchange={(e) => void agent.setGroup(group.id, e.currentTarget.checked)}
                />
                <span class:denied>{t(group.label)}</span>
                {#if denied}<span class="sr-only">{t("agent.denied")}</span>{/if}
              </label>
            {/each}
          </fieldset>
          <button type="button" class="link" onclick={() => void agent.setDefaults()}>
            {t("agent.defaults")}
          </button>
        </div>
      </Card>
    </div>
  {/if}
  {#if initialized}
    <button type="button" class="link limits" onclick={() => ui.showSettingsSection("project")}>
      {t("agent.limitsLink")}
    </button>
  {/if}
  <p class="sr-only" role="status">{announcement}</p>
{/if}

<style>
  .state-card {
    display: grid;
    gap: var(--sp-2);
    padding: var(--sp-4);
  }
  .state {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    margin: 0;
    font-weight: 600;
  }
  .dot {
    flex: none;
    width: 10px;
    height: 10px;
    border: 2px solid var(--muted);
    border-radius: 50%;
  }
  .dot.on {
    border-color: var(--accent);
    background: var(--accent);
  }
  .state :global(.alert) {
    flex: none;
    color: var(--git-deleted-text);
  }
  .about {
    margin: 0;
    font-size: var(--fs-ui-sm);
    overflow-wrap: anywhere;
  }
  .muted {
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .failed {
    margin: 0;
    color: var(--git-deleted-text);
    font-size: var(--fs-ui-sm);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-2);
    margin-top: var(--sp-1);
  }
  /* long labels (German) wrap instead of being cut */
  .btn.wrap {
    height: auto;
    min-height: 28px;
    padding-block: 4px;
    white-space: normal;
    text-align: left;
  }
  .group-title {
    margin: var(--sp-5) 0 var(--sp-2);
    padding-left: var(--sp-1);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    font-weight: 600;
  }
  .setup {
    display: grid;
    gap: var(--sp-2);
    min-width: 0;
    padding: var(--sp-3) var(--sp-4) var(--sp-4);
  }
  .more {
    display: grid;
    gap: var(--sp-2);
    min-width: 0;
  }
  .setup-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: var(--sp-2);
    font-size: var(--fs-ui-sm);
  }
  pre {
    margin: 0;
    padding: var(--sp-2);
    overflow-x: auto;
    border-radius: var(--radius-control);
    background: var(--surface);
    font-family: var(--font-code);
    font-size: var(--fs-ui-xs);
    white-space: pre;
  }
  .disclosure {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    justify-self: start;
    padding: 2px 0;
    border: 0;
    background: none;
    color: var(--text);
    font: inherit;
    font-size: var(--fs-ui-sm);
    cursor: pointer;
  }
  .disclosure.outer {
    margin: var(--sp-5) 0 var(--sp-2);
    color: var(--muted);
    font-weight: 600;
  }
  .disclosure:hover {
    color: var(--text);
  }
  .groups-card {
    display: grid;
    gap: var(--sp-3);
    padding: var(--sp-3) var(--sp-4);
  }
  .groups {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: var(--sp-1) var(--sp-4);
    min-width: 0;
    margin: 0;
    padding: 0;
    border: 0;
  }
  .group {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    font-size: var(--fs-ui-sm);
  }
  .denied {
    color: var(--muted);
    text-decoration: line-through;
  }
  .link {
    justify-self: start;
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent-text);
    font: inherit;
    font-size: var(--fs-ui-sm);
    text-align: left;
    text-decoration: underline;
    cursor: pointer;
  }
  .limits {
    display: block;
    margin-top: var(--sp-3);
  }
</style>
