<script lang="ts">
  import { onMount } from "svelte";
  import { keys, t } from "../i18n";
  import { isComposingKey } from "../ime";
  import { app } from "../stores/app.svelte";
  import { clock } from "../stores/clock.svelte";
  import { relativeTime } from "../time";
  import Icon from "./Icon.svelte";
  import BrandMark from "./BrandMark.svelte";
  import IconButton from "./IconButton.svelte";
  import { ui } from "../stores/ui.svelte";

  let active = $state(0);
  let list: HTMLElement | undefined = $state();

  const recents = $derived(app.visibleRecents);

  // Up/Down/Enter/Del work as soon as the screen appears (without scrolling the logo away when the list is long)
  onMount(() => list?.focus({ preventScroll: true }));

  // The Notes column: the pinned notes, then the three latest, then the button that starts a new one.
  const overview = $derived(app.notesOverview);
  const shownNotes = $derived(
    overview ? [...overview.summary.pinned, ...overview.summary.latest] : [],
  );

  function missing(root: string): boolean {
    return app.missingRecents.has(root);
  }

  function onkeydown(e: KeyboardEvent): void {
    if (isComposingKey(e) || recents.length === 0) return;
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
    if (root && !missing(root)) await app.placeProject(root);
  }
</script>

<div class="welcome-screen">
  <nav class="activitybar" aria-label={t("activity.label")}>
    <IconButton
      class="ab-btn"
      icon="folder"
      label={t("activity.projects")}
      hint={t("switcher.hint")}
      pressed={ui.folderSwitcher !== null}
      data-projects
      data-popover-trigger
      onclick={(e) =>
        void app.openFolderSwitcher((e.currentTarget as HTMLElement).getBoundingClientRect())}
    />
  </nav>
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
      {t("welcome.openFolder")} <kbd>{keys("Ctrl+O")}</kbd>
    </button>
    <button type="button" class="btn" disabled={app.opening} onclick={() => void app.chooseFile()}>
      <Icon name="file-text" size="sm" />{t("welcome.openFile")} <kbd>{keys("Ctrl+Shift+O")}</kbd>
    </button>
  </div>
  {#if app.openError}<p class="welcome__error" role="alert">{app.openError}</p>{/if}

  <div class="welcome__columns">
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
                  <span class="recent-row__time"
                    >{relativeTime(recent.lastOpenedMs, clock.now)}</span
                  >
                {/if}
              </div>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
    <section class="welcome-notes" aria-label={t("welcome.notes")}>
      <div class="menu-label">{t("welcome.notes")}</div>
      {#if overview?.folder.status === "missing"}
        <p class="panel-note" style="padding-left:8px">
          {t("welcome.notesMissing", { path: overview.folder.path })}
        </p>
      {:else if shownNotes.length === 0}
        <p class="panel-note" style="padding-left:8px">{t("welcome.noNotes")}</p>
      {:else}
        <ul class="recents__list">
          {#each shownNotes as note (note.path)}
            <li>
              <button
                type="button"
                class="recent-row recent-row--note"
                title={note.title === "" ? t("notes.untitled") : note.title}
                onclick={() => void app.showNotes(false, note.path)}
              >
                <span class="recent-row__pin">
                  {#if note.pinned}<Icon name="pin" size="xs" />{/if}
                </span>
                <span class={["recent-row__name", note.title === "" && "is-untitled"]}>
                  {note.title === "" ? t("notes.untitled") : note.title}
                </span>
                <span class="recent-row__time">{relativeTime(note.modifiedMs, clock.now)}</span>
              </button>
            </li>
          {/each}
        </ul>
      {/if}
      <button type="button" class="btn welcome-notes__new" onclick={() => void app.newNote()}>
        <Icon name="sticky-note" size="sm" />{t("welcome.newNote")}
        <kbd>{keys("Ctrl+Shift+N")}</kbd>
      </button>
    </section>
  </div>
  <p class="welcome__hint">{t("welcome.dropHint")}</p>
  </div>
</div>
