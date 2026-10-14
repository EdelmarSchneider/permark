<script lang="ts">
  // The Visual editor in the document pane (docs/design/v0.2.md section 3): the toolbar, the Milkdown document drawn with the
  // reading styles, the slash menu and the link popover. Milkdown is loaded on first use, so reading never pays for it.
  import { onMount, untrack } from "svelte";
  import { bridge } from "../bridge";
  import { enhanceContextFor } from "../dom/enhanceContext";
  import { createLockedHost, type LockedHostHandle } from "../editor/lockedHost";
  import { EditorOutline } from "../editor/outlineSync";
  import type { SlashState, VisualEditor } from "../editor/visual/editor";
  import { NO_FORMAT, type CommandId, type FormatState } from "../editor/visual/format";
  import type { LinkTarget } from "../editor/visual/link";
  import type { SlashItem } from "../editor/visual/toolbarModel";
  import { t } from "../i18n";
  import { basename, resolveRelative } from "../paths";
  import { assetUrl } from "../platform";
  import { app } from "../stores/app.svelte";
  import { edit } from "../stores/edit.svelte";
  import { outline } from "../stores/outline.svelte";
  import { session } from "../stores/session.svelte";
  import EditorToolbar from "./EditorToolbar.svelte";
  import LinkPopover from "./LinkPopover.svelte";
  import Notice from "./Notice.svelte";
  import type { Anchor } from "./Popover.svelte";
  import SlashMenu, { type SlashApi } from "./SlashMenu.svelte";

  let host: HTMLElement | undefined = $state();
  let ready = $state(false);
  let failed = $state(false);
  let format = $state.raw<FormatState>(NO_FORMAT);
  let slash = $state.raw<SlashState | null>(null);
  let slashApi = $state<SlashApi | undefined>();
  let link = $state.raw<{ anchor: Anchor; target: LinkTarget } | null>(null);
  let fileInput: HTMLInputElement | undefined = $state();
  let scroller: HTMLElement | undefined = $state();
  let editor: VisualEditor | null = null;
  let sync: EditorOutline | null = null;
  let locked: LockedHostHandle | null = null;

  /** The address an `<img>` loads for the `src` written in the Markdown: the app's image scheme for files of the folder. */
  function imageUrl(src: string): string {
    const path = resolveRelative(edit.path ?? "", src);
    return path === null ? src : assetUrl(path);
  }

  function chooseImage(): void {
    if (!fileInput) return;
    fileInput.value = "";
    fileInput.click();
  }

  function openLink(): void {
    if (!editor || format.atom) return;
    const rect = editor.caretRect() ?? host?.getBoundingClientRect() ?? null;
    if (!rect) return;
    link = { anchor: rect, target: editor.linkTarget() };
  }

  function closeLink(): void {
    link = null;
    editor?.focus();
  }

  function pickSlash(item: SlashItem): void {
    slash = null;
    editor?.runSlash(item.id);
    if (item.id === "image") chooseImage();
  }

  function closeSlash(): void {
    editor?.dismissSlash();
    slash = null;
  }

  function onFiles(e: Event): void {
    const input = e.currentTarget as HTMLInputElement;
    const files = Array.from(input.files ?? []);
    if (files.length > 0) void edit.addFiles(files, "drop");
  }

  // a click on the empty space around the text puts the caret at the end, as in any editor
  function onScrollerClick(e: MouseEvent): void {
    if (
      e.target === e.currentTarget ||
      (e.target as HTMLElement).classList.contains("visual__column")
    ) {
      editor?.focusEnd();
    }
  }

  onMount(() => {
    const start = edit.visualStart;
    const mount = host;
    if (!mount || !start) return;
    const path = edit.path ?? "";
    let gone = false;
    const hostHandle = createLockedHost({
      context: () => enhanceContextFor(path, session.resolvedTheme),
      edit: (block) => void edit.switchTo("source", { block }),
      // a link in a locked block opens as it does when reading (another document asks about unsaved work first); a link to
      // a place in this document has nowhere to go while editing
      link: (href) => {
        if (href !== "" && !href.startsWith("#")) void app.followLink(path, href);
      },
      labels: {
        edit: t("edit.editInSource"),
        editHint: t("edit.editInSourceHint"),
        region: (kind) =>
          kind === "frontmatter" || kind === "html" || kind === "math" || kind === "footnoteDef"
            ? t(`edit.locked.${kind}`)
            : kind === "code"
              ? t("edit.locked.code")
              : t("edit.locked.other"),
        reason: (reason) => t(`edit.lockedWhy.${reason as "kind"}`),
      },
    });
    locked = hostHandle;
    import("../editor/visual/editor")
      .then(async ({ VisualEditor: Editor }) => {
        const created = await Editor.create({
          host: mount,
          label: t("edit.visualLabel", { name: basename(path) }),
          locked: hostHandle.host,
          scroller,
          imageUrl,
          handlers: {
            changed: () => {
              if (!editor) return;
              format = editor.format();
              edit.noteVisual();
              sync?.changed();
            },
            files: (files, source) => void edit.addFiles(files, source),
            editInSource: (block) => void edit.switchTo("source", { block }),
            link: openLink,
            slash: (state) => (slash = state),
            slashKey: (key) => slashApi?.key(key) ?? false,
          },
        });
        if (gone) {
          await created.destroy();
          return;
        }
        editor = created;
        created.load(start);
        bridge.editor = created;
        // the reading view is hidden while editing, so the Outline is built from, and scrolls, this editor
        if (edit.path) {
          sync = new EditorOutline(edit.path, created, outline);
          bridge.editorOutline = sync;
          sync.start();
        }
        ready = true;
        created.focus();
        edit.noteVisual();
      })
      .catch(() => {
        if (gone) return;
        failed = true;
        edit.visualUnavailable();
      });
    return () => {
      gone = true;
      if (editor && bridge.editor === editor) bridge.editor = undefined;
      if (sync && bridge.editorOutline === sync) bridge.editorOutline = undefined;
      sync?.dispose();
      sync = null;
      hostHandle.dispose();
      void editor?.destroy();
      editor = null;
    };
  });

  // the editor is drawn at the reading zoom, and its diagrams in the reading theme
  $effect(() => {
    const zoom = session.zoom;
    if (ready) untrack(() => editor?.core.view.dom.style.setProperty("--zoom", String(zoom)));
  });
  $effect(() => {
    void session.resolvedTheme;
    untrack(() => locked?.retheme());
  });

  const run = (id: CommandId): void => void editor?.run(id);
</script>

<div class="visual" aria-busy={!ready && !failed}>
  <EditorToolbar
    {format}
    {ready}
    {run}
    link={openLink}
    image={chooseImage}
    focus={() => editor?.focus()}
    setLanguage={(language) => editor?.setLanguage(language)}
  />
  <!-- the empty space around the text is a target of its own: a click there puts the caret at the end -->
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div bind:this={scroller} class="visual__scroll" onclick={onScrollerClick}>
    <div class="visual__column">
      <div bind:this={host} class="visual__host"></div>
    </div>
  </div>
  {#if failed}
    <Notice icon="circle-alert" title={t("edit.loadFailed")} />
  {:else if !ready}
    <div class="skeleton visual__skeleton" aria-hidden="true">
      <span></span><span></span><span></span><span></span>
    </div>
  {/if}
</div>

{#if slash}
  <SlashMenu {slash} bind:api={slashApi} onpick={pickSlash} onclose={closeSlash} />
{/if}
{#if link}
  <LinkPopover
    anchor={link.anchor}
    target={link.target}
    docPath={edit.path ?? ""}
    onapply={(next) => {
      if (link) editor?.applyLink(link.target, next);
      closeLink();
    }}
    onremove={() => {
      if (link) editor?.applyLink(link.target, { text: link.target.text, href: "" });
      closeLink();
    }}
    onclose={closeLink}
  />
{/if}
<input
  bind:this={fileInput}
  type="file"
  accept="image/png,image/jpeg,image/gif,image/webp,image/svg+xml"
  multiple
  hidden
  tabindex="-1"
  aria-hidden="true"
  onchange={onFiles}
/>
