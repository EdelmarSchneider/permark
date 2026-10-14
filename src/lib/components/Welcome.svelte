<script lang="ts">
  import { onMount } from "svelte";
  import { t } from "../i18n";
  import { app } from "../stores/app.svelte";
  import { clock } from "../stores/clock.svelte";
  import { relativeTime } from "../time";
  import Icon from "./Icon.svelte";
  import BrandMark from "./BrandMark.svelte";

  let active = $state(0);
  let list: HTMLElement | undefined = $state();

  const recents = $derived(app.visibleRecents);

  // Up/Down/Enter/Del work as soon as the screen appears
  onMount(() => list?.focus());

  function missing(root: string): boolean {
    return app.missingRecents.has(root);
  }

  function onkeydown(e: KeyboardEvent): void {
    if (recents.length === 0) return;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      active = (active + (e.key === "ArrowDown" ? 1 : -1) + recents.length) % recents.length;
    } else if (e.key === "Enter") {
      e.preventDefault();
      void open(recents[active]?.root);
    } else if (e.key === "Delete") {
      const recent = recents[active];
      if (recent) {
        e.preventDefault();
        app.removeRecent(recent.root);
        active = Math.max(0, Math.min(active, recents.length - 2));
      }
    }
  }

  async function open(root: string | undefined): Promise<void> {
    if (root && !missing(root)) await app.openProject(root);
  }
</script>

<div class="welcome">
  <div class="welcome__logo"><BrandMark size={64} /></div>
  <h1>{t("app.name")}</h1>
  <p class="welcome__tagline">{t("welcome.tagline")}</p>

  <div class="welcome__actions">
    <button
      type="button"
      class="btn btn--primary"
      disabled={app.opening}
      onclick={() => void app.chooseFolder()}
    >
      {#if app.opening}<span class="spinner"></span>{:else}<Icon
          name="folder-open"
          size="sm"
        />{/if}
      {t("welcome.openFolder")} <kbd>Ctrl+O</kbd>
    </button>
    <button type="button" class="btn" disabled={app.opening} onclick={() => void app.chooseFile()}>
      <Icon name="file-text" size="sm" />{t("welcome.openFile")} <kbd>Ctrl+Shift+O</kbd>
    </button>
  </div>
  {#if app.openError}<p class="welcome__error" role="alert">{app.openError}</p>{/if}

  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div
    class="recents"
    tabindex="0"
    role="group"
    aria-label={t("welcome.recent")}
    bind:this={list}
    {onkeydown}
  >
    <div class="menu-label">{t("welcome.recent")}</div>
    {#if recents.length === 0}
      <p class="panel-note" style="padding-left:8px">{t("welcome.noRecents")}</p>
    {:else}
      <ul class="recents__list">
        {#each recents as recent, i (recent.root)}
          <li>
            <div
              class={[
                "recent-row",
                i === active && "is-active",
                missing(recent.root) && "is-missing",
              ]}
              role="button"
              tabindex="-1"
              onclick={() => {
                active = i;
                void open(recent.root);
              }}
              onkeydown={() => {}}
            >
              <span class="recent-row__name">{recent.name}</span>
              <span class="recent-row__path">
                {recent.root}{missing(recent.root) ? ` ${t("welcome.notFound")}` : ""}
              </span>
              {#if missing(recent.root)}
                <button
                  type="button"
                  class="btn btn--ghost btn--sm"
                  onclick={(e) => {
                    e.stopPropagation();
                    app.removeRecent(recent.root);
                  }}
                >
                  {t("welcome.remove")}
                </button>
              {:else}
                <span class="recent-row__time">{relativeTime(recent.lastOpenedMs, clock.now)}</span>
              {/if}
            </div>
          </li>
        {/each}
      </ul>
    {/if}
  </div>
  <p class="welcome__hint">{t("welcome.dropHint")}</p>
</div>
