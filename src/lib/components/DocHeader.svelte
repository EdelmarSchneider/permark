<script lang="ts">
  import { gitStateLabel } from "../gitLabels";
  import { noteText } from "../errors";
  import { t } from "../i18n";
  import { basename, breadcrumbs } from "../paths";
  import { formatReference } from "../reference";
  import { app } from "../stores/app.svelte";
  import { clock } from "../stores/clock.svelte";
  import { docs } from "../stores/docs.svelte";
  import { edit } from "../stores/edit.svelte";
  import { session } from "../stores/session.svelte";
  import { ui } from "../stores/ui.svelte";
  import { unread } from "../stores/unread.svelte";
  import { vault } from "../stores/vault.svelte";
  import { savedLabel } from "../time";
  import { badgeFor } from "../tree";
  import Icon from "./Icon.svelte";
  import IconButton from "./IconButton.svelte";
  import Menu, { type MenuEntry } from "./Menu.svelte";
  import ModeSwitch from "./ModeSwitch.svelte";

  let menuAnchor = $state<DOMRect | null>(null);

  const view = $derived(docs.current);
  const path = $derived(docs.path ?? "");
  const crumbs = $derived(path ? breadcrumbs(path) : []);
  const folders = $derived(crumbs.slice(0, -1));
  const file = $derived(path ? basename(path) : "");
  const badge = $derived(
    docs.deleted
      ? badgeFor("deleted")
      : vault.isSimple
        ? null
        : badgeFor(vault.git?.files[path]?.state),
  );
  const referenceHint = $derived(
    path ? formatReference(session.refTemplate, path) : t("header.copyReference"),
  );

  const entries = $derived<MenuEntry[]>([
    ...(vault.isSimple
      ? ([
          {
            id: "outline",
            label: t("menu.outline"),
            icon: "outline",
            hint: "Ctrl+Alt+O",
            disabled: !view,
            onselect: () => (ui.outlinePopover = true),
          },
          { type: "separator" },
        ] satisfies MenuEntry[])
      : ([
          {
            id: "tags",
            label: t("menu.editTags"),
            icon: "hash",
            hint: "T",
            disabled: !view,
            onselect: () => ui.openTagEditor(),
          },
          {
            id: "markUnread",
            label: t("unread.markUnread"),
            disabled: !view || unread.isUnread(path),
            onselect: () => void unread.markUnread(path, true),
          },
          { type: "separator" },
        ] satisfies MenuEntry[])),
    {
      id: "path",
      label: t("menu.copyPath"),
      icon: "copy",
      hint: "Shift+Alt+C",
      disabled: !view,
      onselect: () => void app.copyPath(),
    },
    ...(vault.isSimple
      ? ([
          {
            id: "folder",
            label: t("menu.openFolder"),
            icon: "folder-open",
            onselect: () => void app.openContainingFolder(),
          },
        ] satisfies MenuEntry[])
      : ([
          {
            id: "reveal",
            label: t("menu.reveal"),
            icon: "folder",
            disabled: !view,
            onselect: () => app.revealInTree(path),
          },
        ] satisfies MenuEntry[])),
    {
      id: "external",
      label: t("header.openExternal"),
      icon: "external",
      hint: "Ctrl+Shift+E",
      disabled: !view,
      onselect: () => void app.openExternally(),
    },
    { type: "separator" },
    {
      id: "width",
      label: t("menu.width", { name: t(`width.${session.width}`) }),
      hint: "Ctrl+Alt+W",
      onselect: () => session.cycleWidth(),
    },
    { id: "zoomIn", label: t("menu.zoomIn"), hint: "Ctrl+=", onselect: () => session.zoomBy(1) },
    { id: "zoomOut", label: t("menu.zoomOut"), hint: "Ctrl+-", onselect: () => session.zoomBy(-1) },
    {
      id: "zoomReset",
      label: t("menu.zoomReset"),
      hint: "Ctrl+0",
      onselect: () => session.setZoom(1),
    },
  ]);

  function toggleMenu(e: MouseEvent): void {
    menuAnchor = menuAnchor ? null : (e.currentTarget as HTMLElement).getBoundingClientRect();
  }
</script>

<header class={["doc-header", docs.scrolled && "is-scrolled", edit.active && "is-editing"]}>
  <div class="nav-group">
    <IconButton
      icon="arrow-left"
      label={t("header.back")}
      hint={t("header.backHint")}
      disabled={!docs.canBack}
      onclick={() => void docs.back()}
    />
    <IconButton
      icon="arrow-right"
      label={t("header.forward")}
      hint={t("header.forwardHint")}
      disabled={!docs.canForward}
      onclick={() => void docs.forward()}
    />
  </div>

  {#if path}
    <nav class="breadcrumb" aria-label={t("header.path")}>
      {#if vault.isSimple}
        <span class="current" title={file}>{file}</span>
        <span class="parent">{vault.info?.name}</span>
      {:else}
        {#each folders as crumb (crumb.path)}
          <button
            type="button"
            title={t("header.reveal")}
            onclick={() => app.revealInTree(crumb.path)}
          >
            {crumb.name}
          </button>
          <span class="sep">/</span>
        {/each}
        <span class="current">{file}</span>
      {/if}
    </nav>
    {#if badge}
      <span class={["badge", badge.className]} title={gitStateLabel(badge.state)}
        >{badge.letter}</span
      >
    {/if}
    {#if edit.active}
      {#if edit.dirty}
        <span class="edit-state" role="status" aria-label={t("edit.unsaved")}>
          <span class="dirty-dot" aria-hidden="true"></span><span class="edit-state__text"
            >{t("edit.unsaved")}</span
          >
        </span>
      {/if}
      {@const meta = edit.saving
        ? t("edit.saving")
        : (edit.note ?? savedLabel(edit.modifiedMs, clock.now))}
      <span class="doc-meta" role="status" title={meta}>{meta}</span>
    {:else if view}
      {@const saved = savedLabel(view.modifiedMs, clock.now)}
      <span class="doc-meta" title={saved}>{saved}</span>
      {#if view.encodingNote}
        {@const note = noteText(view.encodingNote)}
        <span class="doc-chip" title={note}>{note}</span>
      {/if}
    {/if}
  {/if}

  <span class="spacer"></span>

  <div class="actions">
    {#if view}
      <button
        type="button"
        class="btn btn--ghost btn--sm"
        title={t("header.copyReferenceHint", { reference: referenceHint })}
        onclick={() => void app.copyReference()}
      >
        <Icon name={ui.flashed === "reference" ? "check" : "at"} size="sm" />
        <span>{ui.flashed === "reference" ? t("common.copied") : t("header.copyReference")}</span>
      </button>
    {/if}
    {#if view || edit.active}<ModeSwitch />{/if}
    {#if edit.active}
      <button
        type="button"
        class="btn btn--sm"
        title={t("edit.saveHint")}
        disabled={edit.saving}
        onclick={() => void edit.save()}
      >
        <Icon name="save" size="sm" /><span>{t("edit.save")}</span>
      </button>
      <button
        type="button"
        class="btn btn--sm btn--primary btn-done"
        title={t("edit.doneHint")}
        onclick={() => edit.requestExit()}
      >
        <Icon name="check" size="sm" /><span>{t("edit.done")}</span>
      </button>
    {/if}
    {#if vault.isSimple}
      <button
        type="button"
        class="btn btn--ghost btn--sm"
        title={t("status.zoomHint")}
        onclick={() => session.setZoom(1)}
      >
        {Math.round(session.zoom * 100)}%
      </button>
    {/if}
    <IconButton
      icon="more"
      label={t("header.more")}
      hint={t("header.moreHint")}
      data-popover-trigger
      onclick={toggleMenu}
    />
    {#if vault.isSimple}
      <IconButton
        icon="settings"
        label={t("activity.settings")}
        data-settings-anchor
        data-popover-trigger
        onclick={() => (ui.settingsOpen = !ui.settingsOpen)}
      />
    {/if}
  </div>
  {#if docs.loadingVisible}<div
      class="doc-header__loading"
      role="progressbar"
      aria-label={t("doc.loading")}
    ></div>{/if}
</header>

{#if menuAnchor}
  <Menu
    label={t("header.more")}
    anchor={menuAnchor}
    align="end"
    {entries}
    onclose={() => (menuAnchor = null)}
  />
{/if}
