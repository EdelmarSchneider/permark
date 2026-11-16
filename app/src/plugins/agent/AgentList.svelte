<script lang="ts">
  // The agents connected to this project, one row each (name, start, working folder, latest request) with
  // Disconnect. One list for the status popover and the settings section. Disconnect asks nothing: the notice it
  // leaves offers Undo; focus goes to the next row's button, else the previous one, else the empty text.
  import { tick } from "svelte";
  import { formatLocale, t } from "../../core/i18n";
  import { exactTime, relativeTime } from "../../core/time";
  import { sessionRows, type SessionRow } from "./sessions";
  import { agent } from "./store.svelte";

  const rows = $derived(sessionRows(agent.sessions, agent.status?.recent ?? []));
  // times are read when the rows change (a new status), like the other relative times of the app
  const now = $derived.by(() => {
    void rows;
    return Date.now();
  });

  let list: HTMLUListElement | null = $state(null);
  let empty: HTMLParagraphElement | null = $state(null);
  let said = $state("");

  function when(ms: number): string {
    return relativeTime(ms, now, formatLocale());
  }

  function lastLine(row: SessionRow): string {
    const last = row.last;
    if (last === null) return t("agent.sessions.noRequests");
    return last.tool === null
      ? t("agent.sessions.lastAt", { when: when(last.atMs) })
      : t("agent.sessions.last", { tool: last.tool, when: when(last.atMs) });
  }

  async function disconnect(row: SessionRow, index: number): Promise<void> {
    if (!(await agent.disconnect(row.session.id, row.label))) return;
    said = t("agent.sessions.disconnected", { name: row.label });
    await tick();
    const buttons = Array.from(
      list?.querySelectorAll<HTMLButtonElement>("button[data-disconnect]") ?? [],
    );
    (buttons[index] ?? buttons[index - 1] ?? empty)?.focus();
  }
</script>

<div class="agents">
  {#if rows.length === 0}
    <p class="muted" tabindex="-1" bind:this={empty}>{t("agent.sessions.empty")}</p>
  {:else}
    <!-- every row holds a button, so Tab reaches each row of a long list and scrolls it into view -->
    <div class="scroll" role="region" aria-label={t("agent.sessions.title")}>
      <ul bind:this={list}>
        {#each rows as row, i (row.session.id)}
          {@const folder =
            row.session.folder === "" ? t("agent.sessions.root") : row.session.folder}
          <li>
            <div class="text">
              <span class="name" title={row.label}>{row.label}</span>
              <span class="line">
                <span class="started" title={exactTime(row.session.startedMs, formatLocale())}>
                  {t("agent.sessions.started", { when: when(row.session.startedMs) })}
                </span>
                <span aria-hidden="true">·</span>
                <span class="folder" title={folder}><bdi>{folder}</bdi></span>
              </span>
              <span
                class="line"
                title={row.last === null ? undefined : exactTime(row.last.atMs, formatLocale())}
                >{lastLine(row)}</span
              >
            </div>
            <button
              type="button"
              class="btn btn--sm"
              data-disconnect
              aria-label={t("agent.sessions.disconnectAria", { name: row.label })}
              title={t("agent.sessions.disconnectHint")}
              onclick={() => void disconnect(row, i)}
            >
              {t("agent.sessions.disconnect")}
            </button>
          </li>
        {/each}
      </ul>
    </div>
  {/if}
  <p class="sr-only" role="status">{said}</p>
</div>

<style>
  .agents {
    min-width: 0;
  }
  .muted {
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  /* about four rows, then the list scrolls */
  .scroll {
    max-height: 18.5em;
    overflow-y: auto;
  }
  ul {
    display: grid;
    gap: var(--sp-2);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    min-width: 0;
  }
  .text {
    display: grid;
    flex: 1;
    min-width: 0;
    font-size: var(--fs-ui-sm);
  }
  .name {
    overflow: hidden;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .line {
    display: flex;
    gap: 4px;
    min-width: 0;
    overflow: hidden;
    color: var(--muted);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .started {
    flex: none;
  }
  /* a long folder keeps its end (the part that tells folders apart) */
  .folder {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    direction: rtl;
    text-align: left;
  }
  .btn {
    flex: none;
  }
</style>
