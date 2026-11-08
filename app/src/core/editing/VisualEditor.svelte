<script lang="ts">
  // Edit mode: Milkdown over the blocks of the text (loaded on first use). Untouched blocks keep their bytes
  // (compose); locked blocks look as View draws their lines (View's render of the text, asked for when the editor is
  // made), their source only where View's blocks cannot be matched. Toolbar, slash menu, link and picture popovers.
  import { mount, unmount, untrack, type Component } from "svelte";
  import type { Node as PMNode } from "@milkdown/kit/prose/model";
  import { api, type Body } from "../../api";
  import type { FrontMatter } from "../../api/generated/FrontMatter";
  import { NearScreen, type BlockHooks } from "../doc/blocks";
  import type { CodeColorer, FrontMatterProps } from "../plugins/props";
  import { apiErrorText } from "../errors";
  import { conditions } from "../conditions/context.svelte";
  import { t, type MessageKey } from "../i18n";
  import { contributions } from "../plugins/instance.svelte";
  import type { ContributionOf } from "../plugins/registry.svelte";
  import "../doc/markdown.css";
  import { codeActionsFor } from "../doc/CodeActions.svelte";
  import { codeEmbed, codeRefTitle, type CodeRefTarget } from "../doc/decorate";
  import { inApp } from "../doc/codeRefs";
  import ContextMenu from "../explorer/ContextMenu.svelte";
  import type { MenuEntry } from "../explorer/menu";
  import { ui } from "../shell/ui.svelte";
  import EditorToolbar from "./EditorToolbar.svelte";
  import CharacterPicker from "./visual/CharacterPicker.svelte";
  import { frontMatterSyntax } from "../config/frontMatter";
  import { documents } from "../doc/document.svelte";
  import { reading } from "../doc/reading.svelte";
  import { pictureDisplayUrl, pictureFiles, pictureLink } from "./pictures";
  import { baseName, pictureType } from "../explorer/importModel";
  import { relativeLink } from "./source/complete";
  import type { Content, EditingSession, TextRange } from "./session.svelte";
  import type {
    BlockName,
    Built,
    CodeLabel,
    Command,
    Composed,
    Core,
    Format,
    LockReason,
    LookBook,
    LooksHost,
    SlashState,
  } from "./visual";
  import type { PickerKind } from "./visual/characters";

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
  let frame: HTMLElement | undefined = $state();
  let ready = $state(false);
  let format = $state.raw<Format | null>(null);
  let slash = $state.raw<SlashState | null>(null);
  let slashIndex = $state(0);
  let slashAt = $state<{ left: number; top: number }>({ left: 0, top: 0 });
  type Popover =
    | { kind: "none" }
    | { kind: "link"; href: string; missing: boolean }
    | { kind: "image"; src: string; alt: string }
    | { kind: "picker"; picker: PickerKind };
  let popover = $state<Popover>({ kind: "none" });
  let targets = $state.raw<string[]>([]);
  /** The files a Ctrl | Cmd + clicked code reference can mean, to choose from. */
  let refMenu = $state.raw<{ x: number; y: number; entries: readonly MenuEntry[] } | null>(null);

  let mod: Module | null = null;
  let core: Core | null = null;
  let built: Built | null = null;
  const composedOf = new WeakMap<PMNode, Composed>();

  /** How long Edit waits for View's render before it shows the document (blocks kept as written: their source). */
  const FIRST_LOOKS_MS = 1500;

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

  const NAMES: Record<BlockName, MessageKey> = {
    properties: "doc.frontmatter",
    html: "edit.block.html",
    formula: "edit.block.formula",
    footnote: "edit.block.footnote",
    link: "link.label",
    diagram: "edit.block.diagram",
    alert: "edit.block.alert",
    other: "edit.block.other",
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
    const found: { element: HTMLElement | null; index: number } = { element: null, index: -1 };
    view.state.doc.forEach((node, offset, index) => {
      const at = lines.get(node);
      const dom = view.nodeDOM(offset);
      if (at !== undefined && at <= line && dom instanceof HTMLElement) {
        found.element = dom;
        found.index = index;
      }
    });
    const element = found.element;
    // the first block is the top of the document: the page opens there, header included, as the reading view does
    if (found.index === 0) scroller.scrollTop = 0;
    else if (element) {
      scroller.scrollTop +=
        element.getBoundingClientRect().top - scroller.getBoundingClientRect().top - 48;
    }
  }

  /** Selects `range` and scrolls it into view; not found in its block: the block scrolled into view. */
  function reveal(range: TextRange): void {
    const now = composed();
    if (!core || !mod || !now || mod.selectRange(core.view, now, range)) return;
    const index = mod.blockOf(now, range.from, range.from);
    const node = index === null ? null : core.view.state.doc.maybeChild(index);
    const line = node ? now.lines.get(node) : undefined;
    if (line !== undefined) scrollToLine(line);
  }

  /** The top edge of the text on screen: below the toolbar that stays at the top of the pane while it scrolls. */
  function textEdge(): number {
    const top = scroller.getBoundingClientRect().top;
    const bar = frame?.querySelector(":scope > [role=toolbar]");
    return bar ? Math.max(top, bar.getBoundingClientRect().bottom) : top;
  }

  /** The headings of the text in document order, inside quotes and lists too (the outline counts those). */
  function headings(): HTMLElement[] {
    const view = core?.view;
    if (!view) return [];
    const found: HTMLElement[] = [];
    view.state.doc.descendants((node, pos) => {
      if (node.type.name !== "heading") return !node.isTextblock;
      const dom = view.nodeDOM(pos);
      if (dom instanceof HTMLElement) found.push(dom);
      return false;
    });
    return found;
  }

  /** Heading `index` a little below the top edge of the text, as View's jump to a heading leaves it. */
  function showHeading(index: number): void {
    const element = headings()[index];
    if (element) scroller.scrollTop += element.getBoundingClientRect().top - textEdge() - 12;
  }

  /** View's render of `text` as the file at `path` (null: none to be had, locked blocks show their source). */
  function viewOf(path: string, text: string): Promise<Body | null> {
    return api.configSheet(path, text).then(
      (result) => (result.ok ? result.value : null),
      () => null,
    );
  }

  /** View's look again for every locked block, from a render of the text as it is now. */
  async function refreshLooks(path: string, book: LookBook): Promise<void> {
    const now = composed();
    const doc = core?.view.state.doc;
    if (!now || !doc) return;
    const ticket = book.ask();
    const body = await viewOf(path, now.text);
    if (body !== null) book.take(ticket, body, doc, now);
  }

  /**
   * View's hooks for the backend blocks locked blocks draw: structure at once; embeds and links once a block comes
   * within a screen of the pane, as in View. `stop` undoes what is left.
   */
  function viewHooks(): { hooks: BlockHooks; stop: () => void } {
    const nearby = new NearScreen(scroller, (element) => documents.decorator.near(element));
    return {
      hooks: {
        enhance: documents.enhance,
        near: (element) => {
          nearby.watch(element);
          return () => nearby.forget(element);
        },
      },
      stop: () => nearby.stop(),
    };
  }

  /** View's front matter card, loaded before the document is drawn (nothing moves when it arrives). */
  async function loadCard(): Promise<Component<FrontMatterProps> | null> {
    const card = contributions.visible("frontMatterCard", conditions.current)[0];
    return card ? (await card.view()).default : null;
  }

  function mountCard(
    card: Component<FrontMatterProps> | null,
    target: HTMLElement,
    front: FrontMatter,
  ): (() => void) | null {
    if (card === null) return null;
    const mounted = mount(card, {
      target,
      props: { syntax: front.syntax, raw: front.raw, fields: front.fields },
    });
    return () => void unmount(mounted);
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

  /** The embed that draws a code block of this language in the reading view (its language's, else any code's). */
  function codeEmbedOf(language: string): ContributionOf<"embed"> | undefined {
    const embeds = contributions.visible("embed", conditions.current);
    return codeEmbed(language, (word) => embeds.find((e) => e.language === word));
  }

  /**
   * A code block's label as the reading view gives it: named by the embed that draws the block there; right of it,
   * the actions that take the language and Copy.
   */
  function codeLabel(language: string): CodeLabel | null {
    const text = codeEmbedOf(language)?.label?.(language) ?? null;
    if (text === null) return null;
    return { text, buttons: codeActionsFor(language).length + 1 };
  }

  /** A code block's token colors from the same embed (none when no plugin colors code). */
  function codeColors(language: string): Promise<CodeColorer> | null {
    return codeEmbedOf(language)?.colors?.() ?? null;
  }

  /**
   * View's lookups for the inline looks of `path`: its link cache, its code reference resolution, its tooltips; a
   * Ctrl | Cmd + click opens a reference as View's click does. Files that come or go make links be looked up again.
   */
  function looksHost(path: string): LooksHost {
    return {
      links: (hrefs) => documents.decorator.links.resolve(hrefs),
      codeRefs: async (written) => {
        const result = await api.resolveCodeRefs(path, [...written]);
        return result.ok ? result.value : null;
      },
      promptText: () => promptText,
      refTitle: (paths, line) => {
        const { editorSet, mod } = documents.decorator.refSetup();
        const [first] = paths;
        if (paths.length !== 1 || first === undefined) {
          return t("edit.codeRefTitleMany", { count: paths.length, mod });
        }
        // Ctrl | Cmd + click here does what a click does in View: View's Ctrl | Cmd + click to the editor has no
        // gesture left in Edit, so the tooltip does not offer it
        const title = codeRefTitle(paths, line, { editorSet: editorSet && !inApp(first), mod });
        return t("edit.codeRefHint", { title, mod });
      },
      missingTitle: (missing) => t("link.missing", { path: missing }),
      openRef: (target: CodeRefTarget, at) => {
        const entries = documents.codeRefChoices(target, false);
        if (entries !== null) refMenu = { ...at, entries };
      },
      forget: () => documents.decorator.links.forget(),
      onFiles: (changed) => {
        let gone = false;
        let stop: (() => void) | null = null;
        // the Explorer's tree events: files coming or going in folders it hides (hidden, ignored) refresh no link
        // until the document opens again
        void api
          .on("tree-changed", ({ delta }) => {
            if (delta.added.length + delta.removed.length + delta.renamed.length === 0) return;
            changed();
          })
          .then((unlisten) => {
            if (gone) unlisten();
            else stop = unlisten;
          });
        return () => {
          gone = true;
          stop?.();
        };
      },
    };
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
    let nearby: ReturnType<typeof viewHooks> | null = null;
    let card: Component<FrontMatterProps> | null = null;
    void (async () => {
      // asked for at once when the backend keeps blocks as written already (the editor may keep more)
      const early = (current.segments ?? []).some((s) => s.lock !== null)
        ? viewOf(current.path, current.text)
        : null;
      const loaded = await import("./visual");
      if (gone) return;
      mod = loaded;
      const hooks = viewHooks();
      nearby = hooks;
      // View's look of each locked block (attrs feed the clipboard and node equality: the look stays out of them)
      const book = new loaded.LookBook();
      made = await loaded.createCore({
        host: target,
        label: t("edit.documentText"),
        locked: {
          render: (node) => book.get(node),
          inline: (source) => book.inline(source),
          refresh: () => refreshLooks(current.path, book),
          blocks: hooks.hooks,
          card: (body, front) => mountCard(card, body, front),
          properties: propertiesCard,
          reasonText: (reason) => t(REASONS[reason]),
          editLabel: t("edit.editInSource"),
          edit: (node) => void session.switchTo("source", lineOf(node)),
          sourceLabel: t("edit.blockSource"),
          sourceHint: t("edit.blockSourceHint"),
          blockLabel: (name) => t(NAMES[name]),
          openHint: t("edit.block.openHint"),
        },
        code: { label: codeLabel, colors: codeColors },
        looks: looksHost(current.path),
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
          displaySrc: (src) =>
            pictureDisplayUrl(content.path, src, (path) =>
              api.assetUrl(documents.workspaceId ?? 0, path, null),
            ),
        }),
      });
      if (gone) {
        await made.destroy();
        return;
      }
      const editor = made;
      const doc = await loaded.buildDoc(editor, current.text, current.segments ?? []);
      // locked blocks and HTML written in a line draw View's look from their first paint when the render and the card
      // come in time; a render that comes later (a large document, a busy backend) brings it then, the blocks show
      // their source until it does
      const asked =
        doc.sources.some((s) => s.locked !== null) ||
        loaded.pieceSources(doc.doc, "html").length > 0
          ? (early ?? viewOf(current.path, current.text))
          : null;
      const ticket = book.ask();
      let deadline: ReturnType<typeof setTimeout> | undefined;
      const first =
        asked === null
          ? null
          : await Promise.race([
              asked,
              new Promise<"late">((done) => {
                deadline = setTimeout(() => done("late"), FIRST_LOOKS_MS);
              }),
            ]);
      clearTimeout(deadline);
      const body = first === "late" ? null : first;
      const withCard = async (render: Body | null): Promise<void> => {
        if (render?.kind === "markdown" && render.frontMatter !== null && card === null) {
          card = await loadCard().catch(() => null);
        }
      };
      await withCard(body);
      if (gone) {
        await editor.destroy();
        return;
      }
      const now = loaded.compose(editor, doc, doc.doc);
      composedOf.set(doc.doc, now);
      if (body !== null) book.take(ticket, body, doc.doc, now);
      built = doc;
      core = editor;
      editor.setDoc(doc.doc);
      if (first === "late" && asked !== null) {
        void asked.then(async (late) => {
          await withCard(late);
          // a block changed meanwhile asked for a newer render: that one brings the looks
          if (gone || late === null || !book.take(ticket, late, doc.doc, now)) return;
          loaded.relook(editor.view);
        });
      }
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
        scope: (kind) => {
          const now = composed();
          return core && mod && now ? mod.scopeIn(core.view.state, now, kind) : null;
        },
        apply: (edits) => {
          const now = composed();
          if (!core || !mod || !now || core.view.composing) return false;
          return mod.applyEdits(core, now, edits);
        },
        reveal: (range) => reveal(range),
        marks: (ranges, current, pick) => {
          const now = composed();
          if (core && mod && now) mod.setMarks(core.view, now, ranges, current, pick);
        },
        markAt: () => (core && mod ? mod.markAt(core.view.state) : null),
        composing: () => core?.view.composing ?? false,
        lost: () => (composed()?.lost.length ?? 0) > 0,
        find: loaded.editFinder(() => core?.view ?? null, scroller),
        headingTops: () => {
          const edge = textEdge();
          return headings().map((element) => element.getBoundingClientRect().top - edge);
        },
        showHeading,
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
      nearby?.stop();
    };
  });

  /** View marks `{{variables}}` and `@file` references while the prompt-blocks plugin is on. */
  const promptText = $derived(conditions.current.plugins.includes("prompt-blocks"));
  /** What a code reference's tooltip offers depends on whether an editor is chosen. */
  const editorSet = $derived(documents.decorator.refSetup().editorSet);

  // prompt text switched or the editor setting changed while editing: the inline looks are drawn again
  $effect(() => {
    void promptText;
    void editorSet;
    untrack(() => {
      if (core && mod) mod.repaintLooks(core.view);
    });
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

  /** The emoji | symbols picker: the popup the toolbar buttons open, riding the same popover state. */
  function openPicker(picker: PickerKind): void {
    popover = { kind: "picker", picker };
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

  /** A character picked: insert at the caret and hand the keyboard back to the editor. */
  function applyCharacter(character: string): void {
    popover = { kind: "none" };
    runCommand({ kind: "text", text: character });
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
    if (saved.length > 0) view.dispatch(tr.scrollIntoView());
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

<div class="visual" data-drop-editor bind:this={frame}>
  {#if format && ready && ui.focus === "off"}
    <EditorToolbar
      {format}
      run={runCommand}
      link={openLink}
      image={openImage}
      emoji={() => openPicker("emoji")}
      symbols={() => openPicker("symbols")}
    />
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
  {:else if popover.kind === "picker"}
    {@const current = popover}
    <div class="popover popover--picker">
      <CharacterPicker
        kind={current.picker}
        apply={applyCharacter}
        close={() => (popover = { kind: "none" })}
        popoverKey={onPopoverKey}
      />
    </div>
  {/if}
  {#if refMenu}
    <ContextMenu
      x={refMenu.x}
      y={refMenu.y}
      entries={refMenu.entries}
      close={() => (refMenu = null)}
    />
  {/if}
  <datalist id="pm-link-targets">
    {#each targets as target (target)}<option value={target}></option>{/each}
  </datalist>

  <div
    class="page md"
    class:is-wide={reading.width === "wide"}
    class:is-full={reading.width === "full"}
    style:--zoom={reading.zoom / 100}
  >
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
  /* View's column (`.md`: the reading width x zoom) with the side padding View's `.doc-column` puts around it */
  .page {
    position: relative;
    width: 100%;
    max-width: calc(var(--column) * var(--zoom) + 2 * var(--sp-5));
    margin: 0 auto;
    padding: var(--sp-5) var(--sp-5) var(--sp-8);
  }
  /* white space as typed (ProseMirror's own stylesheet sets it): with the browser's default a contenteditable puts a
     no-break space in for a second space, which the file would get as `&nbsp;` */
  .host :global(.ProseMirror) {
    outline: none;
    min-height: 40vh;
    white-space: pre-wrap;
    word-wrap: break-word;
  }
  /* View's look drawn in a kept block | piece: its HTML has line breaks between tags, laid out as View lays them out
     (the source, its box and code set their own) */
  .host :global(:is(.pm-locked, .pm-inline)) {
    white-space: normal;
  }
  .host :global(.pm-editor img) {
    max-width: 100%;
    height: auto;
    border-radius: var(--radius-card);
  }
  .host :global(.pm-editor img.ProseMirror-selectednode) {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  /* a locked block View's blocks cannot be matched with: its source, the reason and the way to Source (space above
     and below only: inside a prompt box it keeps the box's inset) */
  .host :global(.pm-locked[data-look="source"]) {
    margin-block: var(--sp-3);
    border: 1px dashed var(--border-strong);
    border-radius: var(--radius-card);
    background: var(--surface);
  }
  .host :global(.pm-locked.ProseMirror-selectednode) {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  /* a selected block at rest is one thing, shown by the outline: the browser's selection does not tint what View drew
     (the editor marks its root so while a block is selected); the front matter card is never text to select */
  .host
    :global(
      .ProseMirror-hideselection
        :is(
          .pm-locked:not([data-look="open"], [data-look="properties"]),
          .pm-inline:not([data-look="open"])
        )
        *::selection
    ) {
    background: transparent;
  }
  /* a piece of a line kept as written (a formula): one thing to select; opened, its field floats under it */
  .host :global(.pm-inline) {
    position: relative;
  }
  .host :global(.pm-inline.ProseMirror-selectednode) {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
    border-radius: 2px;
  }
  .host :global(.pm-inline__box) {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    z-index: 7;
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    min-width: 16em;
    padding: var(--sp-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--surface);
    box-shadow: var(--shadow-pop);
  }
  .host :global(.pm-inline__field) {
    height: 26px;
    padding: 0 8px;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    font: var(--fs-code) / 1.4 var(--font-code);
  }
  .host :global(.pm-inline__field:focus) {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  .host :global(.pm-inline__field[aria-invalid="true"]) {
    border-color: var(--git-deleted-text);
  }
  .host :global(.pm-inline__hint) {
    color: var(--muted);
    font: var(--fs-ui-sm) / 1.4 var(--font-ui);
    white-space: nowrap;
  }
  .host :global(.pm-locked[data-look="card"]) {
    -webkit-user-select: none;
    user-select: none;
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
  /* a locked block opened in place: its source in a box shaped as View's code block, View's look of it below */
  .host :global(.pm-locked[data-look="open"]) {
    margin-block: 0 1em;
  }
  /* opened, the box | the properties' own frame shows where the keys go: no selection outline around it too */
  .host
    :global(.pm-locked:is([data-look="open"], [data-look="properties"]).ProseMirror-selectednode) {
    outline: none;
  }
  .host :global(.pm-locked__editor) {
    display: block;
    box-sizing: border-box;
    width: 100%;
    margin: 0;
    padding: 14px 16px;
    overflow: hidden;
    resize: none;
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--surface);
    color: var(--text);
    font: calc(var(--fs-code) * var(--zoom)) / 1.6 var(--font-code);
    white-space: pre-wrap;
  }
  .host :global(.pm-locked__editor:focus) {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
  .host :global(.pm-locked__hint) {
    margin-top: var(--sp-1);
    color: var(--muted);
    font: var(--fs-ui-sm) / 1.4 var(--font-ui);
  }
  .host :global(.pm-locked__preview) {
    margin-top: var(--sp-3);
  }
  .host :global(.pm-locked__preview > *) {
    margin: 0 0 1em;
  }
  .host :global(.pm-locked__preview > :last-child) {
    margin-bottom: 0;
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
  .popover--picker {
    display: block;
    padding: 0;
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
