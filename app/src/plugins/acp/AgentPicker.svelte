<script lang="ts">
  // The choice of the agent: every agent this computer lists (those that can start first), found by a word of its
  // name | description, each with how it would start or what it needs first ("How to install" opens its website in
  // the browser), and the person's own command for an agent the list does not have. The list is read again each time
  // the picker shows, so an install made meanwhile counts.
  import Check from "@lucide/svelte/icons/check";
  import { onMount, tick } from "svelte";
  import type { AgentEntry } from "../../api";
  import { t } from "../../core/i18n";
  import { acp, canStart, howKey, pickerList } from "./store.svelte";

  type Props = {
    /** The person opened the picker: the search field takes the keyboard. */
    focus: boolean;
  };
  let { focus }: Props = $props();

  const uid = $props.id();
  let query = $state("");
  /** The own-command form is open. */
  let other = $state(false);
  let typed = $state("");
  let search = $state<HTMLInputElement>();
  let field = $state<HTMLInputElement>();

  const agents = $derived(acp.catalog.status === "ready" ? acp.catalog.agents : []);
  const shown = $derived(pickerList(agents, query));
  const locked = $derived(!acp.canChange || acp.choosing);

  onMount(() => {
    if (focus) search?.focus();
    void acp.loadAgents();
  });

  function choose(entry: AgentEntry): void {
    // an own command is kept by its text: the list's `custom` id is no registry id
    void acp.choose(entry.how === "custom" ? { command: entry.command } : { agent: entry.id });
  }

  async function openOther(): Promise<void> {
    typed = agents.find((a) => a.how === "custom")?.command ?? "";
    other = true;
    await tick();
    field?.focus();
  }

  async function useCommand(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    if (await acp.choose({ command: typed })) other = false;
  }
</script>

<section class="picker" aria-labelledby="{uid}-title">
  <h3 id="{uid}-title">{t("acp.pick.title")}</h3>
  <p class="intro">{t("acp.pick.intro")}</p>
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
    <p class="quiet">{t("acp.error.busy")}</p>
  {/if}
  {#if acp.choiceRefusal !== null}
    <p class="refusal" role="alert">{acp.choiceRefusal}</p>
  {/if}

  {#if acp.catalog.status === "failed"}
    <p class="refusal">{acp.catalog.text}</p>
    <button type="button" class="btn btn--sm" onclick={() => void acp.loadAgents()}
      >{t("acp.retry")}</button
    >
  {:else if acp.catalog.status !== "ready"}
    <p class="quiet">{t("acp.pick.loading")}</p>
  {:else if shown.length === 0}
    <p class="quiet">{t("acp.pick.none")}</p>
  {:else}
    <ul class="agents" aria-label={t("acp.pick.list")}>
      {#each shown as entry (entry.id)}
        {@const ready = canStart(entry)}
        <li class={["agent", entry.chosen && "chosen"]}>
          <div class="head">
            <span class="name">{entry.name}</span>
            {#if entry.chosen}
              <span class="current">
                <Check size={12} aria-hidden="true" />
                {t("acp.pick.current")}
              </span>
            {/if}
            <span class="spacer"></span>
            <span class={["how", ready ? "ready" : "missing"]}>{t(howKey(entry))}</span>
          </div>
          {#if entry.description !== ""}
            <p class="description" title={entry.description}>{entry.description}</p>
          {/if}
          {#if entry.command !== ""}
            <code class="command" title={entry.command}>{entry.command}</code>
          {/if}
          <div class="actions">
            {#if !ready && entry.website !== null}
              {@const website = entry.website}
              <button
                type="button"
                class="btn btn--sm btn--ghost"
                aria-label={t("acp.pick.howToInstallNamed", { name: entry.name })}
                onclick={() => acp.openWebsite(website)}>{t("acp.pick.howToInstall")}</button
              >
            {/if}
            <button
              type="button"
              class={["btn", "btn--sm", ready && !entry.chosen && "btn--primary"]}
              aria-label={t("acp.pick.chooseNamed", { name: entry.name })}
              disabled={!ready || locked}
              onclick={() => choose(entry)}>{t("acp.pick.choose")}</button
            >
          </div>
        </li>
      {/each}
    </ul>
  {/if}

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
      <p class="warning" id="{uid}-warning">{t("acp.pick.commandWarning")}</p>
      <div class="actions">
        <button type="button" class="btn btn--sm" onclick={() => (other = false)}
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
    <div class="foot">
      <button type="button" class="btn btn--sm btn--ghost" onclick={() => void openOther()}
        >{t("acp.pick.other")}</button
      >
      {#if acp.status?.chosen === true}
        <span class="spacer"></span>
        <button type="button" class="btn btn--sm" onclick={() => acp.closePicker()}
          >{t("common.cancel")}</button
        >
      {/if}
    </div>
  {/if}
</section>

<style>
  .picker {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
  }
  h3 {
    margin: 0;
    font-size: var(--fs-ui-sm);
    font-weight: 600;
  }
  .intro {
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-ui-xs);
    line-height: 1.5;
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
    font-size: var(--fs-ui-xs);
  }
  .quiet {
    margin: 0;
    color: var(--faint);
    font-size: var(--fs-ui-xs);
  }
  .refusal {
    margin: 0;
    color: var(--git-deleted-text);
    font-size: var(--fs-ui-xs);
    overflow-wrap: anywhere;
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
    align-items: baseline;
    gap: var(--sp-1);
    min-width: 0;
  }
  .name {
    min-width: 0;
    overflow: hidden;
    font-size: var(--fs-ui-sm);
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .current {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    flex: none;
    color: var(--accent-text);
    font-size: var(--fs-ui-xs);
  }
  .spacer {
    flex: 1 1 0;
  }
  .how {
    flex: none;
    padding: 0 var(--sp-1);
    border-radius: var(--radius-sm);
    font-size: var(--fs-ui-xs);
    white-space: nowrap;
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
    font-size: var(--fs-ui-xs);
    line-height: 1.4;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
  }
  .command {
    overflow: hidden;
    color: var(--faint);
    font-family: var(--font-code);
    font-size: var(--fs-ui-xs);
    text-overflow: ellipsis;
    white-space: nowrap;
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
    padding-top: var(--sp-2);
    border-top: 1px solid var(--border);
  }
  label {
    color: var(--muted);
    font-size: var(--fs-ui-xs);
    font-weight: 600;
  }
  .warning {
    margin: 0;
    color: var(--git-modified-text);
    font-size: var(--fs-ui-xs);
  }
  .foot {
    display: flex;
    align-items: center;
    gap: var(--sp-1);
  }
</style>
