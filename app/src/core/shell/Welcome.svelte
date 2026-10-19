<script lang="ts">
  // Start screen: open folder | file, recent folders (missing marked, removable; arrows + Enter + Delete), drop hint.
  import type { RecentFolder } from "../../api";
  import { t } from "../i18n";
  import type { WindowStore } from "../window.svelte";

  type Props = { store: WindowStore };
  let { store }: Props = $props();

  let active = $state(0);
  let list: HTMLElement | undefined = $state();

  const recents = $derived(store.recent);
  const opening = $derived(store.view.state === "opening");

  function open(recent: RecentFolder | undefined): void {
    if (recent && !recent.missing) void store.open(recent.path, "ask");
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.isComposing || recents.length === 0) return;
    switch (e.key) {
      case "ArrowDown":
      case "ArrowUp":
        e.preventDefault();
        active = (active + (e.key === "ArrowDown" ? 1 : -1) + recents.length) % recents.length;
        return;
      case "Enter":
        e.preventDefault();
        open(recents[active]);
        return;
      case "Delete": {
        const recent = recents[active];
        if (!recent) return;
        e.preventDefault();
        void store.removeRecent(recent.path);
        active = Math.max(0, Math.min(active, recents.length - 2));
        return;
      }
      default:
        return;
    }
  }

  // keyboard works as soon as the screen shows (imperative DOM: focus once on mount)
  $effect(() => {
    list?.focus({ preventScroll: true });
  });
</script>

<div class="welcome">
  <h1>{t("app.name")}</h1>
  <p class="tagline">{t("welcome.tagline")}</p>

  <div class="actions">
    <button
      type="button"
      class="btn btn--primary"
      disabled={opening}
      onclick={() => void store.openFolder()}
    >
      {t("welcome.openFolder")}
    </button>
    <button type="button" class="btn" disabled={opening} onclick={() => void store.openFile()}>
      {t("welcome.openFile")}
    </button>
  </div>

  <div
    class="recents"
    tabindex="0"
    role="listbox"
    aria-label={t("welcome.recent")}
    aria-activedescendant={recents[active] ? `recent-${active}` : undefined}
    bind:this={list}
    {onkeydown}
  >
    <div class="menu-label">{t("welcome.recent")}</div>
    {#if recents.length === 0}
      <p class="empty">{t("welcome.noRecents")}</p>
    {:else}
      <ul>
        {#each recents as recent, i (recent.path)}
          <li
            id={`recent-${i}`}
            role="option"
            aria-selected={i === active}
            aria-disabled={recent.missing}
            class={["row", i === active && "is-active", recent.missing && "is-missing"]}
            onclick={() => {
              active = i;
              open(recent);
            }}
            onkeydown={() => {}}
          >
            <span class="name">{recent.name}</span>
            <span class="path"
              >{recent.path}{recent.missing ? ` ${t("welcome.notFound")}` : ""}</span
            >
            {#if recent.missing}
              <button
                type="button"
                class="btn btn--ghost btn--sm"
                onclick={(e) => {
                  e.stopPropagation();
                  void store.removeRecent(recent.path);
                }}
              >
                {t("welcome.remove")}
              </button>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </div>
  <p class="hint">{t("welcome.dropHint")}</p>
</div>

<style>
  .welcome {
    height: 100%;
    overflow: auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 12vh var(--sp-5) var(--sp-6);
  }
  h1 {
    margin: 0;
    font-size: 28px;
    font-weight: 700;
    letter-spacing: -0.02em;
  }
  .tagline {
    margin: 6px 0 var(--sp-5);
    color: var(--muted);
    font-size: 14px;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 10px;
  }
  .actions .btn {
    height: 34px;
    padding: 0 14px;
    font-size: 13px;
  }
  .recents {
    width: min(600px, 100%);
    margin-top: var(--sp-6);
    outline: none;
  }
  .recents:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 4px;
    border-radius: var(--radius-control);
  }
  ul {
    list-style: none;
    margin: 4px 0 0;
    padding: 0;
  }
  .empty {
    margin: 4px 0 0;
    padding-left: 10px;
    color: var(--faint);
    font-size: var(--fs-ui-sm);
  }
  .row {
    display: grid;
    grid-template-columns: minmax(0, auto) minmax(0, 1fr) auto;
    align-items: center;
    gap: 14px;
    height: 36px;
    padding: 0 10px;
    border-radius: var(--radius-control);
    cursor: pointer;
  }
  .row:hover {
    background: var(--hover);
  }
  .row.is-active {
    background: var(--accent-soft);
  }
  .row.is-missing {
    opacity: 0.55;
    cursor: default;
  }
  .name {
    font-weight: 500;
  }
  .path {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--faint);
    font-size: var(--fs-ui-sm);
  }
  .hint {
    margin-top: var(--sp-6);
    color: var(--faint);
    font-size: var(--fs-ui-sm);
  }
</style>
