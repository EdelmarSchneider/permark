<script lang="ts">
  // The choice of the agent: a top row that stays in view (the title, Cancel once an agent is chosen), one sentence of
  // what an agent is (the rest, what happens to the person's words and account, behind a disclosure), the person's
  // own command ("Other command…" near the top, never below a long list), then the popular agents, then all the others
  // A-Z, found by a word of the name | description. An agent that can start has Choose (the one in use says so); one
  // that cannot says why and where to get what it needs, never a Choose that does nothing. The command each runs waits
  // under Details. Escape goes back like Cancel. Inside a Snap nothing can start: one line says so instead of the list.
  // The list is read again each time the picker shows, so an install made meanwhile counts.
  import { onMount, tick } from "svelte";
  import type { AgentEntry } from "../../api";
  import { t } from "../../core/i18n";
  import Note from "./Note.svelte";
  import { acp, canStart, entryAbout, entryName, howKey, pickerList, whyNot } from "./store.svelte";

  type Props = {
    /** The person opened the picker: the search field takes the keyboard. */
    focus: boolean;
    /** The picker goes away (a choice, Cancel): the panel moves the keyboard on. */
    onclose: () => void;
  };
  let { focus, onclose }: Props = $props();

  const uid = $props.id();
  let query = $state("");
  /** The own-command form is open. */
  let other = $state(false);
  let typed = $state("");
  let search = $state<HTMLInputElement>();
  let field = $state<HTMLInputElement>();
  let otherButton = $state<HTMLButtonElement>();

  const agents = $derived(acp.catalog.status === "ready" ? acp.catalog.agents : []);
  const shown = $derived(pickerList(agents, query));
  const locked = $derived(!acp.canChange || acp.choosing);
  /** Inside a Snap no program can start: no list to choose from. */
  const snap = $derived(acp.status?.needs === "snap");
  /** An agent is chosen: the picker can be left without a new choice. */
  const leavable = $derived(acp.status?.chosen === true && !snap);

  onMount(() => {
    if (focus) search?.focus({ preventScroll: true });
    if (!snap) void acp.loadAgents();
  });

  /** The focused button was disabled while the call ran (the page itself took the keyboard). */
  function lost(): boolean {
    return document.activeElement === null || document.activeElement === document.body;
  }

  async function choose(entry: AgentEntry): Promise<void> {
    // an own command is kept by its text: the list's `custom` id is no registry id
    if (await acp.choose(entry.how === "custom" ? { command: entry.command } : { agent: entry.id }))
      onclose();
    else if (lost()) search?.focus();
  }

  async function openOther(): Promise<void> {
    typed = agents.find((a) => a.how === "custom")?.command ?? "";
    other = true;
    await tick();
    field?.focus();
  }

  async function closeOther(): Promise<void> {
    other = false;
    await tick();
    otherButton?.focus();
  }

  async function useCommand(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (await acp.choose({ command: typed })) {
      other = false;
      onclose();
    } else if (lost()) field?.focus();
  }

  function cancel(): void {
    acp.closePicker();
    onclose();
  }

  function keydown(event: KeyboardEvent): void {
    if (event.key !== "Escape" || event.isComposing) return;
    if (other) void closeOther();
    else if (leavable) cancel();
    else return;
    event.preventDefault();
    event.stopPropagation();
  }
</script>

<!-- `heading` false: a group without a heading (the own command), named for assistive technology only -->
{#snippet group(entries: AgentEntry[], key: string, label: string, heading: boolean)}
  {#if heading}
    <h4 class="group" id="{uid}-{key}">{label}</h4>
  {/if}
  <ul
    class="agents"
    aria-labelledby={heading ? `${uid}-${key}` : undefined}
    aria-label={heading ? undefined : label}
  >
    {#each entries as entry (entry.id)}
      {@const ready = canStart(entry)}
      {@const name = entryName(entry)}
      {@const how = howKey(entry)}
      {@const about = entryAbout(entry)}
      <li class={["agent", entry.chosen && "chosen"]}>
        <div class="head">
          <span class="name">{name}</span>
          <span class="spacer"></span>
          <!-- an own command's name already says it is the person's command: only a problem earns a chip -->
          {#if how !== "acp.how.custom"}
            <span class={["how", ready ? "ready" : "missing"]}>{t(how)}</span>
          {/if}
        </div>
        {#if about !== ""}
          <p class="description" title={about}>{about}</p>
        {/if}
        {#if entry.how === "custom"}
          <!-- the person's own words: what they typed is what identifies it -->
          <code class="command own" title={entry.command}>{entry.command}</code>
        {:else if entry.command !== ""}
          <details class="details">
            <summary>{t("acp.details")}</summary>
            <code class="command">{entry.command}</code>
          </details>
        {/if}
        {#if ready}
          <div class="actions">
            {#if entry.chosen}
              <button
                type="button"
                class="btn btn--sm"
                aria-label={t("acp.pick.inUseNamed", { name })}
                disabled>{t("acp.pick.inUse")}</button
              >
            {:else}
              <button
                type="button"
                class="btn btn--sm btn--primary"
                aria-label={t("acp.pick.chooseNamed", { name })}
                disabled={locked}
                onclick={() => void choose(entry)}>{t("acp.pick.choose")}</button
              >
            {/if}
          </div>
        {:else}
          {@const help = whyNot(entry)}
          {@const url = help.url ?? entry.website}
          <Note tone="muted">{t(help.why)}</Note>
          {#if url !== null}
            <div class="actions">
              <button
                type="button"
                class="btn btn--sm"
                aria-label={help.url === null
                  ? t("acp.pick.howToInstallNamed", { name })
                  : t(help.help)}
                onclick={() => acp.openWebsite(url)}>{t(help.help)}</button
              >
            </div>
          {/if}
        {/if}
      </li>
    {/each}
  </ul>
{/snippet}

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<section class="picker" aria-labelledby="{uid}-title" onkeydown={keydown}>
  <div class="title-row">
    <h3 id="{uid}-title">{t("acp.pick.title")}</h3>
    <span class="spacer"></span>
    {#if leavable && !other}
      <button type="button" class="btn btn--sm" onclick={cancel}>{t("common.cancel")}</button>
    {/if}
  </div>

  {#if snap}
    <Note tone="muted">{t("acp.snap")}</Note>
  {:else}
    <Note tone="muted">{t("acp.pick.introShort")}</Note>
    <details class="intro">
      <summary>{t("acp.pick.more")}</summary>
      <div class="intro-text">
        <Note tone="muted">{t("acp.pick.intro")}</Note>
        <Note tone="muted">{t("acp.pick.signInHint")}</Note>
      </div>
    </details>

    {#if other}
      <form class="other" onsubmit={(event) => void useCommand(event)}>
        <label for="{uid}-command">{t("acp.pick.command")}</label>
        <input
          bind:this={field}
          bind:value={typed}
          id="{uid}-command"
          class="field mono"
          spellcheck="false"
          autocomplete="off"
          placeholder={t("acp.pick.commandPlaceholder")}
          aria-describedby="{uid}-warning"
        />
        <Note tone="warning" id="{uid}-warning">{t("acp.pick.commandWarning")}</Note>
        <div class="actions">
          <button type="button" class="btn btn--sm" onclick={() => void closeOther()}
            >{t("common.cancel")}</button
          >
          <button
            type="submit"
            class="btn btn--sm btn--primary"
            disabled={typed.trim() === "" || locked}>{t("acp.pick.useCommand")}</button
          >
        </div>
      </form>
    {:else}
      <button
        bind:this={otherButton}
        type="button"
        class="btn btn--sm btn--ghost other-button"
        onclick={() => void openOther()}>{t("acp.pick.other")}</button
      >
    {/if}

    <input
      bind:this={search}
      bind:value={query}
      class="field"
      type="search"
      spellcheck="false"
      autocomplete="off"
      placeholder={t("acp.pick.search")}
      aria-label={t("acp.pick.search")}
    />
    {#if !acp.canChange}
      <Note>{t("acp.error.busy")}</Note>
    {/if}
    {#if acp.choiceRefusal !== null}
      <Note tone="refusal" alert>{acp.choiceRefusal}</Note>
    {/if}

    {#if acp.catalog.status === "failed"}
      <Note tone="refusal">{acp.catalog.text}</Note>
      <button type="button" class="btn btn--sm retry" onclick={() => void acp.loadAgents()}
        >{t("acp.retry")}</button
      >
    {:else if acp.catalog.status !== "ready"}
      <Note>{t("acp.pick.loading")}</Note>
    {:else if shown.own.length + shown.popular.length + shown.all.length === 0}
      <Note>{t("acp.pick.none")}</Note>
    {:else}
      {#if shown.own.length > 0}
        {@render group(shown.own, "own", t("acp.ownCommand"), false)}
      {/if}
      {#if shown.popular.length > 0}
        {@render group(shown.popular, "popular", t("acp.pick.popular"), true)}
      {/if}
      {#if shown.all.length > 0}
        {@render group(shown.all, "all", t("acp.pick.all"), true)}
      {/if}
    {/if}
  {/if}
</section>

<style>
  .picker {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
  }
  /* stays in view over the long list, so Cancel is always one step away; it covers the scroller's top padding */
  .title-row {
    position: sticky;
    top: calc(-1 * var(--sp-3));
    z-index: 1;
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    min-width: 0;
    margin: calc(-1 * var(--sp-3)) calc(-1 * var(--sp-3)) 0;
    padding: var(--sp-2) var(--sp-3);
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }
  h3 {
    margin: 0;
    font-size: var(--fs-ui-sm);
    font-weight: 600;
  }
  .other-button,
  .retry {
    align-self: flex-start;
  }
  /* same group title style as the panel's other section titles */
  .group {
    margin: var(--sp-1) 0 0;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
  }
  .field {
    width: 100%;
    box-sizing: border-box;
    height: 28px;
    padding: 0 var(--sp-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-ui-sm);
  }
  .field:hover {
    border-color: var(--border-strong);
  }
  .mono {
    font-family: var(--font-code);
  }
  .agents {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .agent {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    padding: var(--sp-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--surface);
  }
  .agent.chosen {
    border-color: var(--accent);
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 2px var(--sp-1);
    min-width: 0;
  }
  .name {
    min-width: 0;
    font-size: var(--fs-ui-sm);
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .spacer {
    flex: 1 1 0;
  }
  /* long words (German, Russian) wrap inside a narrow panel rather than push the list sideways */
  .how {
    flex: none;
    max-width: 100%;
    padding: 0 var(--sp-1);
    border-radius: var(--radius-sm);
    font-size: var(--fs-ui-sm);
    overflow-wrap: anywhere;
  }
  .how.ready {
    background: var(--accent-soft);
    color: var(--accent-text);
  }
  .how.missing {
    background: var(--surface-2);
    color: var(--muted);
  }
  /* registry descriptions run long: two lines here, the whole text on hover */
  .description {
    display: -webkit-box;
    margin: 0;
    overflow: hidden;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    line-height: 1.4;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
  }
  .intro-text {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
  }
  .intro summary {
    min-height: var(--icon-btn-sm);
    color: var(--accent-text);
    font-size: var(--fs-ui-sm);
    line-height: var(--icon-btn-sm);
    cursor: pointer;
  }
  .details summary {
    min-height: var(--icon-btn-sm);
    color: var(--faint);
    font-size: var(--fs-ui-sm);
    line-height: var(--icon-btn-sm);
    cursor: pointer;
  }
  .command {
    display: block;
    overflow-x: auto;
    color: var(--muted);
    font-family: var(--font-code);
    font-size: var(--fs-ui-sm);
    white-space: pre;
  }
  .command.own {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--sp-1);
  }
  .other {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    padding: var(--sp-2) 0;
    border-block: 1px solid var(--border);
  }
  label {
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    font-weight: 600;
  }
</style>
