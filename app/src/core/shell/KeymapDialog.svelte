<script lang="ts">
  // Keyboard shortcuts: every command with its keys, change one by pressing the new keys, reset one or all.
  // A key that another command holds is said before it is saved; saving it anyway is allowed.
  import { comboFromEvent, comboLabel, isComposition } from "../commands/combo";
  import { listCommands } from "../commands/commands";
  import { buildKeymap, conflicts, heldBy } from "../commands/keymap";
  import { keys } from "../commands/state.svelte";
  import { conditions } from "../conditions/context.svelte";
  import { t } from "../i18n";
  import { fuzzyMatch } from "../search/fuzzy";
  import { contributions } from "../plugins/instance.svelte";
  import type { WindowStore } from "../window.svelte";
  import { ui } from "./ui.svelte";

  type Props = { store: WindowStore };
  let { store }: Props = $props();

  type Capture =
    | { status: "idle" }
    | { status: "listening"; command: string }
    | { status: "asking"; command: string; combo: string; others: readonly string[] };

  let capture = $state.raw<Capture>({ status: "idle" });
  let filter = $state("");
  let dialog: HTMLElement | undefined = $state();
  let search: HTMLInputElement | undefined = $state();

  const mac = $derived(conditions.current.platform === "mac");
  const entries = $derived(
    listCommands(contributions.items, conditions.current, keys.map, { palette: false, all: true }),
  );
  const shown = $derived(
    filter.trim() === ""
      ? entries
      : entries.filter((e) => fuzzyMatch(filter, t(e.command.label)) !== null),
  );
  const clashes = $derived(new Set(conflicts(keys.map).flatMap((c) => c.commands)));
  const overridden = $derived(Object.keys(store.keymap));

  function name(command: string): string {
    const found = entries.find((e) => e.command.id === command);
    return found ? t(found.command.label) : command;
  }

  function save(command: string, combo: string | null): void {
    void store.changeSetting({ key: "keymap", value: { command, combo } });
    capture = { status: "idle" };
  }

  /** Who else would hold `combo` if `command` took it. */
  function othersFor(command: string, combo: string): string[] {
    const next = buildKeymap(
      contributions.items.flatMap((i) =>
        i.kind === "shortcut" ? [{ command: i.command, keys: i.keys }] : [],
      ),
      { ...store.keymap, [command]: combo },
      entries.map((e) => e.command.id),
    );
    return [...heldBy(next, combo, command)];
  }

  function onCapture(event: KeyboardEvent): void {
    if (capture.status !== "listening") return;
    event.preventDefault();
    event.stopPropagation();
    if (isComposition(event)) return;
    if (event.key === "Escape") {
      capture = { status: "idle" };
      return;
    }
    const command = capture.command;
    if (
      (event.key === "Backspace" || event.key === "Delete") &&
      !event.ctrlKey &&
      !event.metaKey &&
      !event.altKey
    ) {
      save(command, "");
      return;
    }
    const combo = comboFromEvent(event, mac);
    if (combo === null) return;
    const others = othersFor(command, combo);
    if (others.length === 0) save(command, combo);
    else capture = { status: "asking", command, combo, others };
  }

  function onkeydown(event: KeyboardEvent): void {
    if (event.key === "Escape" && capture.status === "idle") {
      event.preventDefault();
      ui.closeKeyboard();
    }
  }

  // while a key is awaited, every key press goes to the dialog (the app's own shortcuts included)
  $effect(() => {
    if (capture.status !== "listening") return;
    window.addEventListener("keydown", onCapture, true);
    return () => window.removeEventListener("keydown", onCapture, true);
  });

  $effect(() => {
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    search?.focus();
    return () => before?.focus();
  });
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div
  class="backdrop"
  onclick={(e) => {
    if (e.target === e.currentTarget) ui.closeKeyboard();
  }}
>
  <div
    bind:this={dialog}
    class="dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="keymap-title"
    tabindex="-1"
    {onkeydown}
  >
    <header>
      <h2 id="keymap-title">{t("keymap.title")}</h2>
      <input
        bind:this={search}
        type="search"
        class="filter"
        placeholder={t("keymap.search")}
        aria-label={t("keymap.search")}
        bind:value={filter}
      />
    </header>

    <ul class="rows">
      {#each shown as entry (entry.command.id)}
        {@const id = entry.command.id}
        <li class="row" class:clash={clashes.has(id)}>
          <span class="label">{t(entry.command.label)}</span>
          <span class="keys">
            {#if capture.status === "listening" && capture.command === id}
              <span class="listening" role="status">{t("keymap.press")}</span>
            {:else if entry.combos.length === 0}
              <span class="none">{t("keymap.unbound")}</span>
            {:else}
              {#each entry.combos as combo (combo)}
                <kbd>{comboLabel(combo, mac)}</kbd>
              {/each}
            {/if}
          </span>
          <span class="actions">
            <button
              type="button"
              class="btn btn--sm"
              onclick={() => (capture = { status: "listening", command: id })}
              >{t("keymap.change")}</button
            >
            {#if overridden.includes(id)}
              <button type="button" class="btn btn--sm btn--ghost" onclick={() => save(id, null)}
                >{t("keymap.reset")}</button
              >
            {/if}
          </span>
          {#if capture.status === "asking" && capture.command === id}
            <p class="ask" role="alert">
              {t("keymap.taken", {
                keys: comboLabel(capture.combo, mac),
                others: capture.others.map(name).join(", "),
              })}
              <button
                type="button"
                class="btn btn--sm btn--primary"
                onclick={() => {
                  if (capture.status === "asking") save(capture.command, capture.combo);
                }}>{t("keymap.keepAnyway")}</button
              >
              <button
                type="button"
                class="btn btn--sm"
                onclick={() => (capture = { status: "idle" })}>{t("common.cancel")}</button
              >
            </p>
          {:else if clashes.has(id) && entry.combos[0]}
            <p class="shared">
              {t("keymap.shared", {
                others: [...heldBy(keys.map, entry.combos[0], id)].map(name).join(", "),
              })}
            </p>
          {/if}
        </li>
      {/each}
      {#if shown.length === 0}
        <li class="empty">{t("keymap.none")}</li>
      {/if}
    </ul>

    <footer>
      <button
        type="button"
        class="btn btn--sm btn--ghost"
        disabled={overridden.length === 0}
        onclick={() => {
          for (const command of overridden) save(command, null);
        }}>{t("keymap.resetAll")}</button
      >
      <span class="spacer"></span>
      <button type="button" class="btn btn--primary" onclick={() => ui.closeKeyboard()}
        >{t("common.close")}</button
      >
    </footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 56;
    display: grid;
    place-items: center;
    padding: var(--sp-5);
    background: var(--backdrop);
    animation: pm-fade var(--dur) var(--ease);
  }
  .dialog {
    display: flex;
    flex-direction: column;
    width: min(620px, 100%);
    max-height: min(640px, 90vh);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--bg);
    box-shadow: var(--shadow-pop);
    outline: none;
  }
  header {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    padding: var(--sp-4) var(--sp-4) var(--sp-2);
  }
  h2 {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
  }
  .filter {
    flex: 1;
    min-width: 0;
    padding: 5px var(--sp-2);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    font: inherit;
  }
  .rows {
    flex: 1;
    min-height: 0;
    margin: 0;
    padding: 0 var(--sp-2);
    overflow-y: auto;
    list-style: none;
  }
  .row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto auto;
    align-items: center;
    gap: var(--sp-2) var(--sp-3);
    padding: 6px var(--sp-2);
    border-radius: var(--radius-control);
  }
  .row:hover {
    background: var(--hover);
  }
  .label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .keys {
    display: flex;
    gap: var(--sp-1);
    justify-content: flex-end;
    min-width: 7em;
  }
  kbd {
    padding: 1px 6px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--surface);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-ui-xs);
  }
  .row.clash kbd {
    border-color: var(--git-modified);
  }
  .none {
    color: var(--faint);
    font-size: var(--fs-ui-xs);
  }
  .listening {
    color: var(--accent-text);
    font-size: var(--fs-ui-sm);
  }
  .actions {
    display: flex;
    gap: var(--sp-1);
  }
  .ask,
  .shared {
    grid-column: 1 / -1;
    margin: 0;
    color: var(--git-modified-text);
    font-size: var(--fs-ui-sm);
  }
  .empty {
    padding: var(--sp-4);
    color: var(--muted);
  }
  footer {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    padding: var(--sp-3) var(--sp-4);
    border-top: 1px solid var(--border);
  }
</style>
