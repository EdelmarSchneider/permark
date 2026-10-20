<script lang="ts">
  // One value of a sheet: text, number, date, bool (a switch), or empty. A secret value shows as a mask until the
  // person asks for it. In Edit mode a click opens the value for typing; Enter or leaving it commits through the
  // sheet's editor, Escape puts it back.
  import Eye from "@lucide/svelte/icons/eye";
  import EyeOff from "@lucide/svelte/icons/eye-off";
  import Lock from "@lucide/svelte/icons/lock";
  import type { NewValue, Scalar } from "../../api";
  import { t } from "../i18n";
  import { reasonKey, refusalKey } from "./reasons";
  import { MASK } from "./secrets";

  type Props = {
    scalar: Scalar;
    /** What the value is called (its key), for screen readers. */
    label: string;
    editable: boolean;
    revealed: boolean;
    reveal: (path: string) => void;
    /** Commits a new value; resolves to the word of a refusal, or null when it was made. */
    commit: (path: string, value: NewValue) => Promise<string | null>;
  };
  let { scalar, label, editable, revealed, reveal, commit }: Props = $props();

  let draft = $state<string | null>(null);
  let refusal = $state<string | null>(null);
  let busy = false;

  const readOnly = $derived(scalar.edit.editable === "readOnly");
  const reason = $derived(scalar.edit.editable === "readOnly" ? scalar.edit.reason : null);
  const canEdit = $derived(editable && !readOnly);
  const hidden = $derived(scalar.secret && !revealed);

  function begin(): void {
    if (!canEdit || scalar.kind === "bool") return;
    refusal = null;
    draft = scalar.text;
  }

  function newValue(text: string): NewValue {
    return scalar.kind === "string" || scalar.kind === "null"
      ? { type: "text", text }
      : { type: "literal", text };
  }

  async function finish(text: string): Promise<void> {
    if (busy) return;
    if (text === scalar.text) {
      draft = null;
      refusal = null;
      return;
    }
    busy = true;
    const word = await commit(scalar.path, newValue(text));
    busy = false;
    if (word === null) {
      draft = null;
      refusal = null;
    } else refusal = word;
  }

  function onKey(event: KeyboardEvent): void {
    if (event.isComposing) return;
    if (event.key === "Enter") {
      event.preventDefault();
      if (draft !== null) void finish(draft);
    } else if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      draft = null;
      refusal = null;
    }
  }

  async function flip(): Promise<void> {
    if (!canEdit) return;
    refusal = await commit(scalar.path, {
      type: "literal",
      text: scalar.text === "true" ? "false" : "true",
    });
  }

  function focusOnMount(node: HTMLInputElement): void {
    node.focus();
    node.select();
  }
</script>

<span class="value" data-kind={scalar.kind}>
  {#if draft !== null}
    <input
      class="field edit"
      type={hidden ? "password" : "text"}
      autocomplete="off"
      spellcheck="false"
      aria-label={t("config.valueOf", { key: label })}
      bind:value={draft}
      onkeydown={onKey}
      onblur={() => {
        if (draft !== null) void finish(draft);
      }}
      use:focusOnMount
    />
  {:else if scalar.kind === "bool"}
    <button
      type="button"
      class="switch"
      role="switch"
      aria-checked={scalar.text === "true"}
      aria-label={label}
      disabled={!canEdit}
      onclick={() => void flip()}
    >
      <span class="knob"></span>
    </button>
    <span class="word">{scalar.text}</span>
  {:else if hidden}
    <span class="mask" aria-label={t("config.hidden")}>{MASK}</span>
  {:else if scalar.kind === "null"}
    <span class="empty">{t("config.empty")}</span>
  {:else}
    <span class="text" class:code={scalar.kind !== "string"}>{scalar.text}</span>
  {/if}
  {#if draft === null}
    {#if canEdit && scalar.kind !== "bool"}
      <button
        type="button"
        class="hit"
        aria-label={t("config.valueOf", { key: label })}
        onclick={begin}
      ></button>
    {/if}
    {#if scalar.secret}
      <button
        type="button"
        class="icon-btn icon-btn--sm reveal"
        aria-label={revealed ? t("config.hide") : t("config.reveal")}
        title={revealed ? t("config.hide") : t("config.reveal")}
        aria-pressed={revealed}
        onclick={() => reveal(scalar.path)}
      >
        {#if revealed}<EyeOff size={14} />{:else}<Eye size={14} />{/if}
      </button>
    {/if}
    {#if reason !== null}
      <span class="lock" title={t(reasonKey(reason))}>
        <Lock size={12} aria-label={t(reasonKey(reason))} />
      </span>
    {/if}
  {/if}
  {#if refusal !== null}
    <span class="refusal" role="alert">{t(refusalKey(refusal))}</span>
  {/if}
</span>

<style>
  .value {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: var(--sp-2);
    min-width: 0;
    max-width: 100%;
    flex-wrap: wrap;
  }
  .text,
  .mask,
  .empty,
  .word {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .code,
  .mask,
  .word {
    font-family: var(--font-code);
    font-size: 0.92em;
  }
  .mask {
    letter-spacing: 0.06em;
    color: var(--muted);
  }
  .empty {
    color: var(--faint);
    font-style: italic;
  }
  .word {
    color: var(--muted);
  }
  /* a click on the text opens it for typing; the target covers the whole value */
  .hit {
    position: absolute;
    inset: -2px -4px;
    padding: 0;
    border: 0;
    border-radius: 4px;
    background: none;
    cursor: text;
  }
  .hit:hover {
    background: var(--hover);
  }
  .hit:focus-visible {
    outline: 2px solid var(--accent);
  }
  .reveal {
    position: relative;
    z-index: 1;
  }
  .lock {
    display: inline-flex;
    color: var(--faint);
    cursor: help;
  }
  .edit {
    min-width: 12em;
    width: 100%;
    font-family: var(--font-code);
  }
  .refusal {
    flex-basis: 100%;
    color: var(--git-deleted-text);
    font-size: var(--fs-ui-sm);
  }
  .switch {
    position: relative;
    width: 30px;
    height: 18px;
    padding: 0;
    border: 1px solid var(--border-strong);
    border-radius: 9px;
    background: var(--surface-2);
    transition: background var(--dur-fast);
  }
  .switch[aria-checked="true"] {
    background: var(--accent);
    border-color: var(--accent);
  }
  .switch:disabled {
    opacity: 0.7;
    cursor: default;
  }
  .switch:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: var(--bg);
    box-shadow: 0 1px 2px rgb(0 0 0 / 0.25);
    transition: transform var(--dur-fast) var(--ease);
  }
  .switch[aria-checked="true"] .knob {
    transform: translateX(12px);
  }
</style>
