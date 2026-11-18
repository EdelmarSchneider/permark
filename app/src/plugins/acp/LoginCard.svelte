<script lang="ts">
  // The card shown while the agent waits for a sign-in: each way it offers, as that way works (the agent signs in
  // itself | a command the person runs in a terminal, ready to copy | variables to set), then Try again. The agent's
  // own way comes first; while it has one, the ways for developers (a command, a key) wait folded under "Advanced".
  // PerMark opens no terminal and keeps no key: the person signs in outside, then the agent is asked again.
  import Copy from "@lucide/svelte/icons/copy";
  import type { LoginMethod } from "../../api";
  import { t } from "../../core/i18n";
  import Note from "./Note.svelte";
  import { acp } from "./store.svelte";

  type Props = {
    methods: readonly LoginMethod[];
    /** The agent's name (`null`: an own command, which has none). */
    name: string | null;
    /** A sign-in was asked for: its button waits meanwhile and the card goes once signed in, so the keyboard moves
     *  on at once. */
    onsignin: () => void;
  };
  let { methods, name, onsignin }: Props = $props();

  const COPIED_MS = 2000;
  const uid = $props.id();
  /** The method whose command was copied last (`null`: none, or long enough ago). */
  let copied = $state<string | null>(null);
  /** The method whose command the system did not let copy (`null`: none). */
  let uncopied = $state<string | null>(null);
  let title = $state<HTMLElement>();

  const agentWays = $derived(methods.filter((m) => m.kind === "agent"));
  const otherWays = $derived(methods.filter((m) => m.kind !== "agent"));

  /** The card's title takes the keyboard (the panel shows the card the moment a sign-in is needed). */
  export function focusTitle(): HTMLElement | null {
    title?.focus({ preventScroll: true });
    return title ?? null;
  }

  function signIn(method: LoginMethod | null): void {
    onsignin();
    void acp.login(method);
  }

  async function copy(method: LoginMethod, text: string): Promise<void> {
    const done = await acp.copy(text);
    copied = done ? method.id : null;
    uncopied = done ? null : method.id;
  }

  /** The words above a way for developers. */
  function wayTitle(method: LoginMethod): string {
    return method.kind === "terminal" ? t("acp.login.terminalWay") : t("acp.login.envWay");
  }

  // "Copied" goes back to "Copy" after a moment, so the next copy confirms again
  $effect(() => {
    if (copied === null) return;
    const timer = setTimeout(() => (copied = null), COPIED_MS);
    return () => clearTimeout(timer);
  });
</script>

{#snippet way(method: LoginMethod)}
  <p class="method-name">{method.name}</p>
  {#if method.description !== null && method.description !== ""}
    <Note>{method.description}</Note>
  {/if}
  {#if method.kind === "terminal" && method.hint !== null}
    {@const command = method.hint}
    <Note tone="muted">{t("acp.login.terminalSteps")}</Note>
    <div class="command-row">
      <!-- one line that scrolls sideways (with the keyboard too): a command broken mid-flag is easy to copy wrong -->
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <pre
        class="command"
        tabindex="0"
        role="region"
        aria-label={t("acp.login.commandLabel")}>{command}</pre>
      <button
        type="button"
        class="btn btn--sm"
        title={t("acp.login.copyCommand")}
        onclick={() => void copy(method, command)}
      >
        <Copy size={12} aria-hidden="true" />
        {copied === method.id ? t("acp.login.copied") : t("acp.login.copy")}
      </button>
    </div>
    {#if uncopied === method.id}
      <Note tone="refusal" alert>{t("acp.login.copyFailed")}</Note>
    {/if}
  {:else if method.kind === "env" && method.hint !== null}
    <Note tone="muted">{t("acp.login.env", { names: method.hint })}</Note>
  {/if}
{/snippet}

<section class="login" aria-labelledby="{uid}-title">
  <h3 id="{uid}-title" tabindex="-1" bind:this={title}>
    {name === null ? t("acp.login.titleAgent") : t("acp.login.title", { name })}
  </h3>
  <Note tone="muted">{t("acp.login.intro")}</Note>
  {#if agentWays.length > 0}
    <ul class="methods">
      {#each agentWays as method (method.id)}
        <li class="method">
          <button
            type="button"
            class="btn btn--sm btn--primary"
            disabled={acp.signingIn}
            onclick={() => signIn(method)}>{method.name}</button
          >
          {#if method.description !== null && method.description !== ""}
            <Note>{method.description}</Note>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
  {#if otherWays.length > 0}
    <ul class="methods">
      {#each otherWays as method (method.id)}
        <li class="method">
          {#if agentWays.length > 0}
            <details class="advanced">
              <summary>{wayTitle(method)}</summary>
              <div class="folded">{@render way(method)}</div>
            </details>
          {:else}
            {@render way(method)}
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
  <div class="actions">
    <button type="button" class="btn btn--sm" disabled={acp.signingIn} onclick={() => signIn(null)}
      >{t("acp.retry")}</button
    >
  </div>
</section>

<style>
  .login {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    padding: var(--sp-2);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-card);
    background: var(--surface);
  }
  h3 {
    margin: 0;
    font-size: var(--fs-ui-sm);
    font-weight: 600;
  }
  .methods {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .method,
  .folded {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    min-width: 0;
  }
  .folded {
    padding-top: var(--sp-1);
  }
  .advanced {
    align-self: stretch;
    min-width: 0;
  }
  summary {
    min-height: var(--icon-btn-sm);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    line-height: var(--icon-btn-sm);
    cursor: pointer;
  }
  /* agents name their methods freely: long names wrap inside the card */
  .method .btn {
    max-width: 100%;
    height: auto;
    min-height: 24px;
    white-space: normal;
    text-align: start;
  }
  .method-name {
    margin: 0;
    font-size: var(--fs-ui-sm);
    font-weight: 600;
  }
  .command-row {
    display: flex;
    align-items: flex-start;
    gap: var(--sp-1);
    align-self: stretch;
    min-width: 0;
  }
  .command {
    flex: 1;
    min-width: 0;
    margin: 0;
    padding: var(--sp-1) var(--sp-2);
    overflow-x: auto;
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    font-family: var(--font-code);
    font-size: var(--fs-ui-sm);
    white-space: pre;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
  }
</style>
