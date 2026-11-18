<script lang="ts">
  // The card shown while the agent waits for a sign-in: each way it offers, as that way works (the agent signs in
  // itself | a command the person runs in a terminal, ready to copy | variables to set), then Try again. PerMark opens
  // no terminal and keeps no key: the person signs in outside, then the agent is asked again.
  import Copy from "@lucide/svelte/icons/copy";
  import type { LoginMethod } from "../../api";
  import { t } from "../../core/i18n";
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

  function signIn(method: LoginMethod | null): void {
    onsignin();
    void acp.login(method);
  }

  async function copy(method: LoginMethod, text: string): Promise<void> {
    if (await acp.copy(text)) copied = method.id;
  }

  // "Copied" goes back to "Copy" after a moment, so the next copy confirms again
  $effect(() => {
    if (copied === null) return;
    const timer = setTimeout(() => (copied = null), COPIED_MS);
    return () => clearTimeout(timer);
  });
</script>

<section class="login" aria-labelledby="{uid}-title">
  <h3 id="{uid}-title">
    {name === null ? t("acp.login.titleAgent") : t("acp.login.title", { name })}
  </h3>
  <p class="intro">{t("acp.login.intro")}</p>
  {#if methods.length > 0}
    <ul class="methods">
      {#each methods as method (method.id)}
        <li class="method">
          {#if method.kind === "agent"}
            <button
              type="button"
              class="btn btn--sm btn--primary"
              disabled={acp.signingIn}
              onclick={() => signIn(method)}>{method.name}</button
            >
          {:else}
            <p class="method-name">{method.name}</p>
          {/if}
          {#if method.description !== null && method.description !== ""}
            <p class="quiet">{method.description}</p>
          {/if}
          {#if method.kind === "terminal" && method.hint !== null}
            {@const command = method.hint}
            <p class="how">{t("acp.login.terminal")}</p>
            <div class="command-row">
              <pre class="command">{command}</pre>
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
          {:else if method.kind === "env" && method.hint !== null}
            <p class="how">{t("acp.login.env", { names: method.hint })}</p>
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
  .intro,
  .how {
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-ui-xs);
    line-height: 1.5;
  }
  .methods {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .method {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    min-width: 0;
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
  .quiet {
    margin: 0;
    color: var(--faint);
    font-size: var(--fs-ui-xs);
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
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    font-family: var(--font-code);
    font-size: var(--fs-ui-xs);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
  }
</style>
