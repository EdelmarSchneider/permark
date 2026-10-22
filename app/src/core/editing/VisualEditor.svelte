<script lang="ts">
  // Edit mode: Milkdown over the blocks of the text (loaded on first use). Untouched blocks keep their bytes
  // (compose); locked blocks show their source with the reason and a way to Source. Toolbar, slash menu, link and
  // picture popovers.
  import { mount, unmount } from "svelte";
  import type { Node as PMNode } from "@milkdown/kit/prose/model";
  import { api } from "../../api";
  import { apiErrorText } from "../errors";
  import { conditions } from "../conditions/context.svelte";
  import { t, type MessageKey } from "../i18n";
  import { contributions } from "../plugins/instance.svelte";
  import "../doc/markdown.css";
  import { ui } from "../shell/ui.svelte";
  import EditorToolbar from "./EditorToolbar.svelte";
  import { frontMatterSyntax } from "../config/frontMatter";
  import { documents } from "../doc/document.svelte";
  import { pictureFiles, pictureLink, pictureSource } from "./pictures";
  import { baseName, pictureType } from "../explorer/importModel";
  import { relativeLink } from "./source/complete";
  import type { Content, EditingSession } from "./session.svelte";
  import type {
    Built,
    Command,
    Composed,
    Core,
    Format,
    LockReason,
    PictureView,
    SlashState,
  } from "./visual";

  type Props = {
    session: EditingSession;
    content: Content;
    scroller: HTMLElement;
    project: boolean;
    pictures: (files: File[]) => Promise<string[]>;
    /** A short message in the status area (a picture that could not be read). */
    note: (text: string) => void;
    /** The visual editor could not be made. */
    failed: (error: unknown) => void;
  };
  let { session, content, scroller, project, pictures, note, failed }: Props = $props();

  type Module = typeof import("./visual");

  let host: HTMLElement | undefined = $state();
  let ready = $state(false);
  let format = $state.raw<Format | null>(null);
  let slash = $state.raw<SlashState | null>(null);
  let slashIndex = $state(0);
  let slashAt = $state<{ left: number; top: number }>({ left: 0, top: 0 });
  type Popover =
    | { kind: "none" }
    | { kind: "link"; href: string; missing: boolean }
    | { kind: "image"; src: string; alt: string };
  let popover = $state<Popover>({ kind: "none" });
  let targets = $state.raw<string[]>([]);

  let mod: Module | null = null;
  let core: Core | null = null;
  let built: Built | null = null;
  const composedOf = new WeakMap<PMNode, Composed>();

  const REASONS: Record<LockReason, MessageKey> = {
    frontMatter: "edit.locked.frontMatter",
    html: "edit.locked.html",
    math: "edit.locked.math",
    footnote: "edit.locked.footnote",
    diagram: "edit.locked.diagram",
    data: "edit.locked.data",
    definition: "edit.locked.definition",
    kind: "edit.locked.kind",
    parse: "edit.locked.parse",
    reference: "edit.locked.reference",
    differs: "edit.locked.differs",
  };

  type SlashItem = { id: string; label: MessageKey; command: Command | "image" };
  const SLASH: SlashItem[] = [
    { id: "text", label: "toolbar.paragraph", command: { kind: "block", to: "paragraph" } },
    { id: "h1", label: "toolbar.h1", command: { kind: "block", to: "h1" } },
    { id: "h2", label: "toolbar.h2", command: { kind: "block", to: "h2" } },
    { id: "h3", label: "toolbar.h3", command: { kind: "block", to: "h3" } },
    { id: "bullet", label: "toolbar.bulletList", command: { kind: "bulletList" } },
    { id: "ordered", label: "toolbar.orderedList", command: { kind: "orderedList" } },
    { id: "task", label: "toolbar.taskList", command: { kind: "taskList" } },
    { id: "quote", label: "toolbar.quote", command: { kind: "quote" } },
    { id: "code", label: "toolbar.codeBlock", command: { kind: "codeBlock" } },
    { id: "table", label: "toolbar.table", command: { kind: "table" } },
    { id: "rule", label: "toolbar.rule", command: { kind: "rule" } },
    { id: "image", label: "toolbar.image", command: "image" },
  ];

  const slashItems = $derived.by(() => {
    if (slash === null) return [];
    const query = slash.query.toLowerCase();
    return SLASH.filter(
      (item) => item.id.startsWith(query) || t(item.label).toLowerCase().includes(query),
    );
  });

  function composed(): Composed | null {
    if (!core || !built || !mod) return null;
    const doc = core.view.state.doc;
    let found = composedOf.get(doc);
    if (!found) {
      found = mod.compose(core, built, doc);
      composedOf.set(doc, found);
    }
    return found;
  }

  function lineOf(node: PMNode): number {
    return composed()?.lines.get(node) ?? 1;
  }

  /** The first top-level node at the top of the pane, as a line. */
  function topLine(): number {
    if (!core) return 1;
    const top = scroller.getBoundingClientRect().top;
    const view = core.view;
    let line = 1;
    let found = false;
    view.state.doc.forEach((node, offset) => {
      if (found) return;
      const dom = view.nodeDOM(offset);
      if (dom instanceof HTMLElement && dom.getBoundingClientRect().bottom > top + 1) {
        line = lineOf(node);
        found = true;
      }
    });
    return line;
  }

  function scrollToLine(line: number): void {
    const lines = composed()?.lines;
    if (!core || !lines) return;
    const view = core.view;
    const found: { element: HTMLElement | null } = { element: null };
    view.state.doc.forEach((node, offset) => {
      const at = lines.get(node);
      const dom = view.nodeDOM(offset);
      if (at !== undefined && at <= line && dom instanceof HTMLElement) found.element = dom;
    });
    const element = found.element;
    if (element) {
      scroller.scrollTop +=
        element.getBoundingClientRect().top - scroller.getBoundingClientRect().top - 48;
    }
  }

  /** A diagram drawn in a locked block; the front matter as properties that can be edited. */
  function draw(
    body: HTMLElement,
    kind: string,
    source: string,
    replace: (source: string) => void,
  ): (() => void) | null {
    if (kind === "frontMatter") return propertiesCard(body, source, replace);
    if (kind !== "code") return null;
    const match = /^ {0,3}(?:`{3,}|~{3,})[ \t]*(\S+)/.exec(source);
    const language = (match?.[1] ?? "").toLowerCase();
    const wanted = language === "puml" ? "plantuml" : language;
    const embed = contributions
      .visible("embed", conditions.current)
      .find((e) => e.language === wanted);
    if (!embed) return null;
    const code = source.split("\n").slice(1, -1).join("\n");
    let gone = false;
    let mounted: ReturnType<typeof mount> | null = null;
    void embed.view().then((loaded) => {
      if (gone) return;
      mounted = mount(loaded.default, {
        target: body,
        props: { source: code, info: language, display: false },
      });
    });
    return () => {
      gone = true;
      if (mounted) void unmount(mounted);
    };
  }

  /** The front matter as editable properties, when a plugin reads its syntax (YAML | TOML). */
  function propertiesCard(
    body: HTMLElement,
    source: string,
    replace: (source: string) => void,
  ): (() => void) | null {
    const syntax = frontMatterSyntax(source);
    const readable = contributions
      .visible("viewer", conditions.current)
      .some((viewer) => viewer.id === syntax);
    if (syntax === null || !readable) return null;
    let gone = false;
    let mounted: ReturnType<typeof mount> | null = null;
    void import("../config/FrontMatterEditor.svelte").then((loaded) => {
      if (gone) return;
      mounted = mount(loaded.default, { target: body, props: { source, syntax, replace } });
    });
    return () => {
      gone = true;
      if (mounted) void unmount(mounted);
    };
  }

  /** A picture of the document as View draws it: a file next to it loads, a web picture is a chip. */
  function pictureView(src: string, alt: string): PictureView {
    const source = pictureSource(content.path, src);
    switch (source.kind) {
      case "file":
        return { kind: "load", url: documents.enhanceEnv.assetUrl(source.path) };
      case "inline":
        return { kind: "load", url: source.src };
      case "web":
        return {
          kind: "chip",
          text: alt === "" ? t("doc.remoteImageNoAlt") : alt,
          title: t("doc.remoteImage", { url: source.url }),
        };
      case "none":
        return { kind: "none" };
      default:
        return source satisfies never;
    }
  }

  function refresh(): void {
    if (core && mod) format = mod.formatOf(core.view.state);
  }

  function runCommand(command: Command): void {
    if (!core || !mod) return;
    mod.run(core, command);
    core.view.focus();
    refresh();
  }

  // the editor: made when the content is new, torn down with it
  $effect(() => {
    const target = host;
    const current = content;
    if (!target) return;
    let gone = false;
    let detach = (): void => {};
    let made: Core | null = null;
    void (async () => {
      const loaded = await import("./visual");
      if (gone) return;
      mod = loaded;
      made = await loaded.createCore({
        host: target,
        locked: {
          reasonText: (reason) => t(REASONS[reason]),
          editLabel: t("edit.editInSource"),
          cardText: t("config.fm.bar"),
          edit: (node) => void session.switchTo("source", lineOf(node)),
          draw,
        },
        pictures: { view: pictureView },
        first: loaded.appPlugins({
          core: () => core,
          changed: () => {
            session.changed();
            refresh();
          },
          selected: () => {
            session.selected();
            refresh();
          },
          save: () => session.save(),
          modeKey: (toSource) => void session.switchTo(toSource ? "source" : "view"),
          linkKey: () => openLink(),
          slash: (state) => {
            slash = state;
            slashIndex = 0;
            if (state && core) {
              const coords = core.view.coordsAtPos(state.to);
              const box = (target.parentElement ?? target).getBoundingClientRect();
              slashAt = { left: coords.left - box.left, top: coords.bottom - box.top + 4 };
            }
          },
          slashKey: (key) => slashKeyed(key),
          pictures: (files, at) => void insertPictures(files, at),
          followLink: (href) => void followLink(href),
        }),
      });
      if (gone) {
        await made.destroy();
        return;
      }
      const doc = await loaded.buildDoc(made, current.text, current.segments ?? []);
      if (gone) {
        await made.destroy();
        return;
      }
      built = doc;
      core = made;
      made.setDoc(doc.doc);
      ready = true;
      refresh();
      scrollToLine(current.line);
      made.view.focus();
      detach = session.attach(current.seq, {
        kind: "edit",
        text: () => composed()?.text ?? current.text,
        changes: () => composed()?.changed ?? 0,
        topLine,
        selection: () => {
          if (!core) return "";
          const { from, to } = core.view.state.selection;
          return core.view.state.doc.textBetween(from, to, "\n", " ");
        },
        focus: () => core?.view.focus(),
        pictures: (files, point) => {
          const view = core?.view;
          if (!view) return;
          const at =
            view.posAtCoords({ left: point.x, top: point.y })?.pos ?? view.state.selection.from;
          void insertPictures(files, at);
        },
      });
    })().catch((error: unknown) => {
      // the visual editor could not be made: the document opens in Source instead
      if (gone) return;
      failed(error);
    });
    return () => {
      gone = true;
      detach();
      ready = false;
      core = null;
      built = null;
      if (made) void made.destroy();
    };
  });

  // ---- slash menu -------------------------------------------------------------------------------------------

  function slashKeyed(key: "up" | "down" | "enter" | "escape"): boolean {
    if (slash === null || slashItems.length === 0) return key === "escape" && slash !== null;
    switch (key) {
      case "up":
        slashIndex = (slashIndex - 1 + slashItems.length) % slashItems.length;
        return true;
      case "down":
        slashIndex = (slashIndex + 1) % slashItems.length;
        return true;
      case "enter": {
        const item = slashItems[slashIndex];
        if (item) choose(item);
        return true;
      }
      case "escape":
        slash = null;
        return true;
      default:
        return key satisfies never;
    }
  }

  function choose(item: SlashItem): void {
    const state = slash;
    if (!core || !state) return;
    const view = core.view;
    view.dispatch(view.state.tr.delete(state.from, state.to));
    slash = null;
    if (item.command === "image") openImage();
    else runCommand(item.command);
  }

  // ---- links and pictures -----------------------------------------------------------------------------------

  async function loadTargets(): Promise<void> {
    if (!project || targets.length > 0) return;
    const result = await api.editLinkTargets();
    if (result.ok) targets = result.value.map((p) => relativeLink(content.path, p));
  }

  function openLink(): void {
    popover = { kind: "link", href: format?.link ?? "", missing: false };
    void loadTargets();
  }

  function openImage(): void {
    popover = { kind: "image", src: "", alt: "" };
    void loadTargets();
  }

  async function checkLink(href: string): Promise<void> {
    if (href === "" || /^[a-z]+:/i.test(href) || href.startsWith("#")) return;
    const result = await api.resolveLinks(content.path, [href]);
    if (result.ok && popover.kind === "link" && popover.href === href) {
      popover = { ...popover, missing: result.value[0]?.type === "missing" };
    }
  }

  function applyLink(href: string | null): void {
    popover = { kind: "none" };
    runCommand({ kind: "link", href: href === null || href.trim() === "" ? null : href.trim() });
  }

  function applyImage(src: string, alt: string): void {
    popover = { kind: "none" };
    if (src.trim() !== "") runCommand({ kind: "image", src: src.trim(), alt });
  }

  /** The picture button's file dialog: each chosen picture is copied next to the document, like a paste. */
  async function choosePictures(): Promise<void> {
    const paths = await api.pickPictures(t("image.chooseTitle"), t("dialog.filter.pictures"));
    if (paths.length === 0 || !core) return;
    const at = core.view.state.selection.from;
    const files: File[] = [];
    for (const path of paths) {
      const read = await api.readPicture(path);
      if (read.ok) files.push(new File([read.value], baseName(path), { type: pictureType(path) }));
      else note(apiErrorText(read.error));
    }
    popover = { kind: "none" };
    if (files.length > 0) await insertPictures(files, at);
    core.view.focus();
  }

  async function insertPictures(files: File[], at: number): Promise<void> {
    const saved = await pictures(pictureFiles(files));
    if (!core || !mod) return;
    const view = core.view;
    const image = view.state.schema.nodes.image;
    if (!image) return;
    let tr = view.state.tr;
    let pos = Math.min(at, tr.doc.content.size);
    for (const path of saved) {
      const node = image.create({ src: pictureLink(content.path, path), alt: "", title: "" });
      tr = tr.insert(pos, node);
      pos += node.nodeSize;
    }
    if (saved.length > 0) view.dispatch(tr);
  }

  async function followLink(href: string): Promise<void> {
    if (/^(https?|mailto):/i.test(href)) await api.openWeb(href);
  }

  function onPopoverKey(event: KeyboardEvent): void {
    if (event.isComposing) return;
    if (event.key === "Escape") {
      event.preventDefault();
      popover = { kind: "none" };
      core?.view.focus();
    }
  }
</script>

<div class="visual" data-drop-editor>
  {#if format && ready && ui.focus === "off"}
    <EditorToolbar {format} run={runCommand} link={openLink} image={openImage} />
  {/if}
  {#if popover.kind === "link"}
    {@const current = popover}
    <form
      class="popover"
      aria-label={t("link.label")}
      onsubmit={(e) => {
        e.preventDefault();
        applyLink(current.href);
      }}
    >
      <!-- svelte-ignore a11y_autofocus -->
      <input
        class="field"
        list="pm-link-targets"
        placeholder={t("link.target")}
        aria-label={t("link.target")}
        autofocus
        onkeydown={onPopoverKey}
        value={current.href}
        oninput={(e) => {
          popover = { kind: "link", href: e.currentTarget.value, missing: false };
          void checkLink(e.currentTarget.value);
        }}
      />
      {#if current.missing}<span class="missing" role="status">{t("link.broken")}</span>{/if}
      <button type="submit" class="btn btn--sm btn--primary">{t("link.apply")}</button>
      {#if format?.link}
        <button type="button" class="btn btn--sm" onclick={() => applyLink(null)}
          >{t("link.remove")}</button
        >
      {/if}
      <button
        type="button"
        class="btn btn--sm btn--ghost"
        onclick={() => (popover = { kind: "none" })}
      >
        {t("common.cancel")}
      </button>
    </form>
  {:else if popover.kind === "image"}
    {@const current = popover}
    <form
      class="popover"
      aria-label={t("toolbar.image")}
      onsubmit={(e) => {
        e.preventDefault();
        applyImage(current.src, current.alt);
      }}
    >
      <!-- svelte-ignore a11y_autofocus -->
      <input
        class="field"
        list="pm-link-targets"
        placeholder={t("image.path")}
        aria-label={t("image.path")}
        autofocus
        onkeydown={onPopoverKey}
        value={current.src}
        oninput={(e) => (popover = { ...current, src: e.currentTarget.value })}
      />
      <input
        class="field"
        placeholder={t("image.alt")}
        aria-label={t("image.alt")}
        onkeydown={onPopoverKey}
        value={current.alt}
        oninput={(e) => (popover = { ...current, alt: e.currentTarget.value })}
      />
      <button type="submit" class="btn btn--sm btn--primary">{t("link.apply")}</button>
      <button type="button" class="btn btn--sm" onclick={() => void choosePictures()}>
        {t("image.choose")}
      </button>
      <button
        type="button"
        class="btn btn--sm btn--ghost"
        onclick={() => (popover = { kind: "none" })}
      >
        {t("common.cancel")}
      </button>
    </form>
  {/if}
  <datalist id="pm-link-targets">
    {#each targets as target (target)}<option value={target}></option>{/each}
  </datalist>

  <div class="page md">
    <div class="host" bind:this={host}></div>
    {#if slash !== null && slashItems.length > 0}
      <div
        class="slash"
        role="listbox"
        aria-label={t("slash.label")}
        style:left="{slashAt.left}px"
        style:top="{slashAt.top}px"
      >
        {#each slashItems as item, i (item.id)}
          <button
            type="button"
            role="option"
            aria-selected={i === slashIndex}
            class:active={i === slashIndex}
            onmousedown={(e) => {
              e.preventDefault();
              choose(item);
            }}
          >
            {t(item.label)}
          </button>
        {/each}
      </div>
    {/if}
  </div>
</div>

<style>
  .visual {
    display: flex;
    flex-direction: column;
    min-height: 100%;
  }
  .page {
    position: relative;
    width: 100%;
    max-width: var(--measure);
    margin: 0 auto;
    padding: var(--sp-5) var(--sp-5) var(--sp-8);
  }
  .host :global(.ProseMirror) {
    outline: none;
    min-height: 40vh;
  }
  .host :global(.pm-locked) {
    margin: var(--sp-3) 0;
    border: 1px dashed var(--border-strong);
    border-radius: var(--radius-card);
    background: var(--surface);
  }
  .host :global(.pm-locked.ProseMirror-selectednode) {
    outline: 2px solid var(--accent);
  }
  .host :global(.pm-locked__bar) {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    padding: 4px var(--sp-3);
    border-bottom: 1px solid var(--border);
    color: var(--muted);
    font: 500 var(--fs-ui-sm) / 1.4 var(--font-ui);
  }
  .host :global(.pm-locked__why) {
    flex: 1;
  }
  .host :global(.pm-locked__why::before) {
    content: "🔒 ";
  }
  .host :global(.pm-locked__source) {
    margin: 0;
    padding: var(--sp-2) var(--sp-3);
    overflow-x: auto;
    background: transparent;
    font: var(--fs-code) / 1.5 var(--font-code);
    white-space: pre;
  }
  .host :global(.pm-locked__drawing) {
    padding: var(--sp-2) var(--sp-3);
    border-top: 1px solid var(--border);
  }
  /* the front matter as properties: the card replaces the source */
  .host :global(.pm-locked--card .pm-locked__source) {
    display: none;
  }
  .host :global(.pm-locked--card .pm-locked__drawing) {
    padding: 0;
    border-top: 0;
  }
  .popover {
    position: sticky;
    top: 36px;
    z-index: 6;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-2);
    margin: var(--sp-2) auto 0;
    padding: var(--sp-2) var(--sp-3);
    max-width: var(--measure);
    width: calc(100% - 2 * var(--sp-4));
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--surface);
    box-shadow: var(--shadow-pop);
  }
  .field {
    flex: 1;
    min-width: 140px;
    height: 26px;
    padding: 0 8px;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    font-size: var(--fs-ui);
  }
  .missing {
    color: var(--git-deleted-text);
    font-size: var(--fs-ui-sm);
  }
  .slash {
    position: absolute;
    z-index: 8;
    display: flex;
    flex-direction: column;
    min-width: 200px;
    max-height: 280px;
    overflow-y: auto;
    padding: 4px;
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--bg);
    box-shadow: var(--shadow-pop);
  }
  .slash button {
    padding: 6px 10px;
    border: none;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--text);
    text-align: left;
    font-size: var(--fs-ui);
    cursor: pointer;
  }
  .slash button.active,
  .slash button:hover {
    background: var(--accent-soft);
  }
</style>
