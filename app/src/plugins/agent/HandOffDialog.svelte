<script lang="ts">
  // The hand-off preview: the sessions of the person's work since the last hand-off (day, hours, age, actions,
  // approximate tokens; stale ones unchecked), the changes made outside PerMark, the notes of the case, the total.
  // Confirm sets the checkpoint and copies one line for the prompt (or the full list). Cancel changes nothing.
  import X from "@lucide/svelte/icons/x";
  import { language, t, tn } from "../../core/i18n";
  import { relativeTime } from "../../core/time";
  import { approx, canConfirm, included, notes, when } from "./handoff";
  import { handOff } from "./handoff.svelte";

  const s = $derived(handOff.state);
  const open = $derived(s.status === "open" ? s : null);
  const preview = $derived(open?.preview ?? null);
  const inside = $derived(preview === null ? [] : included(preview));
  const actions = $derived(inside.reduce((n, x) => n + x.rows, 0) + (preview?.outside.files ?? 0));
  const now = Date.now();

  let dialog: HTMLElement | undefined = $state();

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      handOff.close();
    }
  }

  // focus enters the dialog and returns where it was when the dialog goes
  $effect(() => {
    if (open === null) return;
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog?.focus();
    return () => before?.focus();
  });
</script>

{#if open !== null && preview !== null}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div
    class="backdrop"
    onclick={(e) => {
      if (e.target === e.currentTarget) handOff.close();
    }}
  >
    <div
      bind:this={dialog}
      class="dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="handoff-title"
      aria-describedby="handoff-intro"
      tabindex="-1"
      {onkeydown}
    >
      <header>
        <h2 id="handoff-title">{t("agent.handoff.title")}</h2>
        <button
          type="button"
          class="icon-btn icon-btn--sm"
          aria-label={t("common.close")}
          title={t("common.close")}
          onclick={() => handOff.close()}
        >
          <X size={16} />
        </button>
      </header>
      <p id="handoff-intro" class="intro">{t("agent.handoff.intro")}</p>

      {#if preview.sessions.length > 0}
        <p class="label">{t("agent.handoff.sessions")}</p>
        <ul class="sessions">
          {#each preview.sessions as session (session.id)}
            {@const at = when(session, language.locale)}
            <li class:off={!session.checked || session.dropped}>
              <label>
                <input
                  type="checkbox"
                  checked={session.checked}
                  disabled={open.phase === "busy"}
                  onchange={() => void handOff.toggle(session.id)}
                />
                <span class="when">{t("agent.handoff.session", at)}</span>
                <span class="age">{relativeTime(session.end, now, language.locale)}</span>
                <span class="count">{tn("agent.handoff.actions", session.rows)}</span>
                <span class="size"
                  >{t("agent.handoff.tokens", { n: approx(session.tokens, language.locale) })}</span
                >
                {#if session.stale}<span class="badge">{t("agent.handoff.stale")}</span>{/if}
                {#if session.dropped}<span class="badge warn">{t("agent.handoff.dropped")}</span
                  >{/if}
              </label>
            </li>
          {/each}
        </ul>
      {/if}

      <p class="outside">
        {#if preview.outside.files > 0}
          {tn("agent.handoff.outside", preview.outside.files)}
          {#if preview.outside.byFolder}<span class="badge">{t("agent.handoff.byFolder")}</span
            >{/if}
          {#if preview.outside.git}<code>{preview.outside.git}</code>{/if}
        {:else}
          {t("agent.handoff.outsideNone")}
        {/if}
      </p>

      {#each notes(preview) as note (note)}
        <p class="note">{t(note)}</p>
      {/each}

      <p class="total" role="status">
        {t("agent.handoff.total", {
          actions: tn("agent.handoff.actions", actions),
          tokens: t("agent.handoff.tokens", { n: approx(preview.tokens, language.locale) }),
        })}
      </p>
      {#if !canConfirm(preview)}
        <p class="note">{t("agent.handoff.pick")}</p>
      {/if}
      {#if open.phase === "failed"}
        <p class="note warn" role="alert">{t("agent.handoff.failed")}</p>
      {/if}

      <label class="full">
        <input
          type="checkbox"
          checked={open.full}
          onchange={(e) => handOff.setFull(e.currentTarget.checked)}
        />
        {t("agent.handoff.full")}
      </label>

      <footer>
        <button type="button" class="btn" onclick={() => handOff.close()}>
          {t("common.cancel")}
        </button>
        <button
          type="button"
          class="btn btn--primary"
          disabled={open.phase === "busy" || !canConfirm(preview)}
          onclick={() => void handOff.confirm()}
        >
          {t("agent.handoff.confirm")}
        </button>
      </footer>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: grid;
    place-items: center;
    padding: var(--sp-5);
    background: var(--backdrop);
    animation: pm-fade var(--dur) var(--ease);
  }
  .dialog {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    width: min(620px, 100%);
    max-height: min(640px, 100%);
    overflow: auto;
    padding: var(--sp-4) var(--sp-5);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--bg);
    color: var(--text);
    box-shadow: var(--shadow-pop);
    outline: none;
    font-size: var(--fs-ui);
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-3);
  }
  h2 {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
  }
  p {
    margin: 0;
  }
  .intro,
  .label {
    color: var(--muted);
  }
  .label {
    margin-top: var(--sp-2);
    font-size: var(--fs-ui-sm);
  }
  .sessions {
    display: grid;
    gap: 2px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .sessions label {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px var(--sp-3);
    padding: 6px var(--sp-2);
    border-radius: var(--radius-control);
    cursor: pointer;
  }
  .sessions label:hover {
    background: var(--hover);
  }
  .sessions li.off .when,
  .sessions li.off .count,
  .sessions li.off .size {
    color: var(--muted);
  }
  .when {
    font-weight: 500;
  }
  .age,
  .size {
    color: var(--muted);
  }
  .count,
  .size {
    font-variant-numeric: tabular-nums;
  }
  .badge {
    padding: 0 6px;
    border-radius: 8px;
    background: var(--surface-2);
    color: var(--muted);
    font-size: var(--fs-ui-xs);
  }
  .badge.warn,
  .note.warn {
    color: var(--git-deleted-text);
  }
  .outside {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-2);
    margin-top: var(--sp-2);
  }
  code {
    font-family: var(--font-code);
    font-size: var(--fs-ui-sm);
    color: var(--muted);
  }
  .note {
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .total {
    margin-top: var(--sp-2);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .full {
    display: flex;
    align-items: flex-start;
    gap: var(--sp-2);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  footer {
    display: flex;
    justify-content: flex-end;
    gap: var(--sp-2);
    margin-top: var(--sp-2);
  }
</style>
