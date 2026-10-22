// The document this window shows: one state machine (empty | loading | ready | failed, plus "deleted" on top of
// ready), the blocks in the DOM, the trail back and forward, links, and the reader's place.
//
// The backend owns the truth of what the file says; this holds what is on screen. A newer open supersedes a slower
// one (sequence number); an update of the same document asks only for the blocks it does not hold.

import {
  api as defaultApi,
  type Api,
  type ApiError,
  type Block,
  type DocumentChanged,
  type DocumentView,
  type LinkTarget,
  type WorkspaceView,
} from "../../api";
import { BlockHost } from "./blocks";
import { decideRef, inApp } from "./codeRefs";
import { Decorator, codeRefOf, type CodeRefTarget, type EmbedLookup } from "./decorate";
import { enhanceBlock, type EnhanceEnv } from "./enhance";
import { currentIndex } from "./outline";
import { changedLines, type LineRange } from "./textDiff";
import { reading } from "./reading.svelte";
import { webPictures } from "./webPictures.svelte";
import { layout } from "../shell/layout.svelte";
import { recentFiles } from "../search/recent.svelte";
import { formatQuote, formatReference, type Reference } from "./reference";
import { Trail, type Place } from "./trail";
import type { SavePolicy } from "../editing/docMachine";
import { confirmLeave } from "../editing/guard";
import { t } from "../i18n";

export type Deleted = { movedTo: string | null; at: number };

export type DocState =
  | { status: "empty" }
  /** `view`: what stays on screen meanwhile (another document, or the same one being reloaded). */
  | { status: "loading"; path: string; view: DocumentView | null }
  | { status: "ready"; view: DocumentView; deleted: Deleted | null; fresh: boolean }
  | { status: "failed"; path: string; error: ApiError };

/** What an update of the page that was not a fresh document changed, in block ids. */
export type BlockUpdate = {
  /** Blocks that are new on the page (not there before the update). */
  added: readonly string[];
  /** Blocks that went, each with the block before it that stayed (`null`: the top). */
  removed: readonly { id: string; after: string | null }[];
};

/** What a reload of a text document changed: lines of the new text that are different, places where lines went. */
export type TextUpdate = {
  path: string;
  added: readonly LineRange[];
  /** After this line of the new text (0: before the first). */
  removedAfter: readonly number[];
};

/** Lines of a text document that glow, and the places that mark where lines went (set by the live feed). */
export type LineGlow = {
  path: string;
  added: readonly LineRange[];
  removedAfter: readonly number[];
};

/** A view of the blocks to show instead of the document's own (a plugin's review). */
export type BlockTransform = (blocks: readonly Block[], view: DocumentView) => readonly Block[];

/** A button of a quiet note. */
export type HookAction = { label: string; run: () => void };

/** A line of a text document (or of a sheet) to bring into view, told to the viewer that shows it. */
export type LineRequest = { path: string; line: number; seq: number };

export type Hooks = {
  /** A quiet note for the person. */
  note(text: string): void;
  /** A quiet note with buttons. */
  action(text: string, ...actions: HookAction[]): void;
  /** A failed request, worded by the shell. */
  fail(error: ApiError): void;
  /** The folder of the project's tree, shown. */
  reveal(path: string): void;
  /** The person chose an editor (setting `editor` not empty). */
  editorSet(): boolean;
  /** How the platform names the key held for "in your editor" (`Ctrl` | `Cmd`). */
  modKey(): string;
  /** Settings at the editor choice. */
  chooseEditor(): void;
  /** The folder of a single file, opened as a project. */
  openFolder(): void;
};

const NO_HOOKS: Hooks = {
  note: () => {},
  action: () => {},
  fail: () => {},
  reveal: () => {},
  editorSet: () => false,
  modKey: () => "Ctrl",
  chooseEditor: () => {},
  openFolder: () => {},
};

export class DocumentController {
  state = $state.raw<DocState>({ status: "empty" });
  /** The heading the reader is at (index in the outline), -1 before the first. */
  currentHeading = $state(-1);
  /** A picture shown large. */
  zoomed = $state.raw<{ src: string; alt: string } | null>(null);
  readonly host = new BlockHost();
  readonly trail = new Trail();
  readonly decorator: Decorator;

  #api: Api;
  #hooks: Hooks = NO_HOOKS;
  #workspace: WorkspaceView | null = null;
  #seq = 0;
  #shown: string | null = null;
  #painted: DocumentView | null = null;
  /** The open of the shown document that has not finished. */
  #pending: Promise<void> | null = null;
  #intent: (Place & { line: number | null }) | null = null;
  #viaTrail: string | null = null;
  #anchor: string | null = null;
  /** A line to scroll to when the document in the making is shown (a search hit without a heading). */
  #line: number | null = null;
  #headings: (HTMLElement | null)[] = [];
  #painters = new Set<() => void>();
  #updaters = new Set<(update: BlockUpdate) => void>();
  #textUpdaters = new Set<(update: TextUpdate) => void>();
  #repaint = false;
  /** Goes up when the blocks shown have to be made again (a plugin changed what it adds). */
  paintTick = $state(0);
  /** Lines of the text shown that glow for a moment (the text viewer draws them). */
  lineGlow = $state.raw<LineGlow | null>(null);
  /** A line the viewer of a text | sheet document brings into view (a code reference, a search hit). */
  lineRequest = $state.raw<LineRequest | null>(null);
  #lineSeq = 0;
  /**
   * The file a single-file window shows when a link led away from its own (`null`: its own). Single-file windows have
   * no project registry document: the window holds it here.
   */
  singleDoc = $state<string | null>(null);
  /** The hint "choose an editor for code" was given in this window. */
  #hintedEditor = false;

  constructor(api: Api = defaultApi) {
    this.#api = api;
    this.decorator = new Decorator(api);
    this.decorator.refSetup = () => ({
      editorSet: this.#hooks.editorSet(),
      mod: this.#hooks.modKey(),
    });
  }

  bind(hooks: Hooks): void {
    this.#hooks = hooks;
  }

  /** A quiet note from a viewer of the page. */
  notify(text: string): void {
    this.#hooks.note(text);
  }

  // ---- what is shown ---------------------------------------------------------------------------------------

  get view(): DocumentView | null {
    const s = this.state;
    return s.status === "ready" ? s.view : s.status === "loading" ? s.view : null;
  }

  get path(): string | null {
    return this.#shown;
  }

  /** The path of what the page shows now. Reactive (it follows the view), unlike `path`, which only the controller moves. */
  get shown(): string | null {
    return this.view?.path ?? null;
  }

  /** Notes save by themselves; project documents and single files on request. */
  get savePolicy(): SavePolicy {
    return this.#workspace?.kind === "notes"
      ? { kind: "auto", delayMs: 800 }
      : { kind: "explicit" };
  }

  get workspaceId(): number | null {
    return this.#workspace?.id ?? null;
  }

  /** The person clicked the chip of a web picture: that picture is fetched and shown. */
  showWebPicture(chip: Element): void {
    void webPictures.show(this.#api, chip, (text) => this.#hooks.note(text));
  }

  /** Kept web pictures take their chips' places; with the project's choice on, the others load. */
  sweepWebPictures(root: Element): void {
    webPictures.sweep(this.#api, root, (text) => this.#hooks.note(text));
  }

  /** The project's choice: web pictures load by themselves (profile only). */
  setWebPictures(on: boolean, root: Element | null): void {
    void webPictures.setAlways(this.#api, on, root, (text) => this.#hooks.note(text));
  }

  get singleFile(): boolean {
    return this.#workspace?.kind === "singleFile";
  }

  /** The window shows this workspace (or none): a different one starts a new reading session. */
  setWorkspace(workspace: WorkspaceView | null): void {
    if (workspace?.id === this.#workspace?.id) return;
    this.#leave();
    this.#reset();
    this.#workspace = workspace;
    this.singleDoc = null;
    this.trail.clear();
    if (workspace !== null) {
      void reading.load(this.#api);
      void webPictures.load(this.#api, workspace.kind === "project");
      void layout.load(this.#api, workspace.kind === "project");
      if (workspace.kind === "project") void recentFiles.load(this.#api);
    }
  }

  /** The page went away (the window left its workspace): nothing is shown any more, no outline, no trail. */
  release(): void {
    if (this.#shown !== null) void this.#api.closeDocument();
    this.setWorkspace(null);
  }

  #reset(): void {
    this.#seq += 1;
    this.#shown = null;
    this.#painted = null;
    this.#intent = null;
    this.#viaTrail = null;
    this.#anchor = null;
    this.#line = null;
    this.lineRequest = null;
    this.#headings = [];
    this.currentHeading = -1;
    this.zoomed = null;
    this.state = { status: "empty" };
    this.decorator.dispose();
    this.decorator.setDocument(null);
  }

  /** The window shows this document (`null`: none). */
  async show(path: string | null): Promise<void> {
    if (path === null) {
      if (this.#shown === null) return;
      this.#leave();
      this.#reset();
      void this.#api.closeDocument();
      return;
    }
    // the same document: whoever asked second waits for the open that is on its way
    if (path === this.#shown && this.state.status !== "failed") return this.#pending ?? undefined;
    this.#leave();
    const intent = this.#intent?.path === path ? this.#intent : null;
    this.#intent = null;
    const viaTrail = this.#viaTrail === path;
    this.#viaTrail = null;
    const anchor = intent?.anchor ?? (viaTrail ? (this.trail.here?.anchor ?? null) : null);
    if (!viaTrail) this.trail.visit({ path, anchor });
    this.#anchor = anchor;
    this.#line = intent?.line ?? null;
    // a line asked for an earlier document is not asked again when a viewer shows up later
    this.lineRequest = null;
    this.#shown = path;
    if (this.#workspace?.kind === "project") recentFiles.visit(path);
    this.#painted = null;
    const opening = this.#open(path, true);
    this.#pending = opening;
    await opening;
    if (this.#pending === opening) this.#pending = null;
  }

  /** `own`: the person's own save is what is read again (their lines do not glow). */
  async #open(path: string, fresh: boolean, own = false): Promise<void> {
    const seq = ++this.#seq;
    if (fresh) this.state = { status: "loading", path, view: this.view };
    const known = fresh ? [] : this.host.ids();
    const result = await this.#api.openDocument(path, known);
    if (seq !== this.#seq) return;
    if (!result.ok) {
      this.state = { status: "failed", path, error: result.error };
      return;
    }
    this.decorator.setDocument(result.value.path);
    const before = this.view;
    this.state = { status: "ready", view: result.value, deleted: null, fresh };
    if (!fresh && !own) this.#toldText(before, result.value);
    // a page of blocks scrolls itself when it is built; another viewer is told the line
    if (fresh && result.value.body.kind !== "markdown" && this.#line !== null) {
      this.#askLine(result.value.path, this.#line);
      this.#line = null;
    }
  }

  #askLine(path: string, line: number): void {
    this.lineRequest = { path, line, seq: ++this.#lineSeq };
  }

  /** A text document was read again: tells who listens which lines are different now. */
  #toldText(before: DocumentView | null, now: DocumentView): void {
    if (this.#textUpdaters.size === 0 || before === null || before.path !== now.path) return;
    const [was, is] = [before.body, now.body];
    if (was.kind !== "text" || is.kind !== "text" || was.partial || is.partial) return;
    const change = changedLines(was.text, is.text);
    if (change.added.length === 0 && change.removedAfter.length === 0) return;
    for (const listener of this.#textUpdaters) listener({ path: now.path, ...change });
  }

  /** Called after a text document was read again with other lines in it. Returns what stops it. */
  onTextUpdated(listener: (update: TextUpdate) => void): () => void {
    this.#textUpdaters.add(listener);
    return () => this.#textUpdaters.delete(listener);
  }

  /** The file changed on disk (or went away). */
  onChanged(event: DocumentChanged): void {
    if (event.path !== this.#shown) return;
    switch (event.change) {
      case "changed":
        void this.#open(event.path, false);
        break;
      case "deleted": {
        const s = this.state;
        if (s.status === "ready") {
          this.state = { ...s, deleted: { movedTo: event.movedTo, at: Date.now() } };
        }
        break;
      }
      default:
        event satisfies never;
    }
  }

  // ---- the page --------------------------------------------------------------------------------------------

  /** The page exists: blocks go into `container`, inside the scrolling pane. */
  attachBody(container: HTMLElement, scroller: HTMLElement): void {
    this.host.attach(container, scroller, {
      enhance: this.enhance,
      near: (element) => this.decorator.near(element),
    });
    // a new page is empty: whatever was painted before has to be painted again
    this.#painted = null;
  }

  detachBody(): void {
    this.#leave();
    this.host.detach();
    this.decorator.dispose();
    this.#painted = null;
    this.#headings = [];
  }

  /** Puts the document in the DOM when its view is new. Called by the body when the state changes. */
  paint(embeds: EmbedLookup, transform: BlockTransform = (blocks) => blocks): void {
    this.decorator.setEmbeds(embeds);
    const s = this.state;
    if (s.status !== "ready") return;
    const view = s.view;
    const body = view.body;
    if (body.kind !== "markdown") return;
    const again = this.#repaint && this.#painted === view;
    this.#repaint = false;
    if (this.#painted === view && !again) return;
    this.#painted = view;
    this.decorator.setDocument(view.path);
    const blocks = transform(body.blocks, view);
    // the blocks of a page that is there are updated, not built again (the place stays)
    const fresh = s.fresh && !again;
    const before = fresh ? null : this.host.ids();
    const applied = this.host.apply(blocks, fresh);
    if (applied.missing.length > 0) {
      // the window was told it holds blocks it does not: ask again for everything
      void this.#open(view.path, true);
      return;
    }
    this.#headings = [];
    if (before !== null) this.#told(before, blocks);
    const seq = this.#seq;
    void applied.settled.then(() => {
      if (seq !== this.#seq) return;
      this.#settled(view, s.fresh && !again);
      for (const painted of this.#painters) painted();
    });
  }

  /** The page has to be made again from the same document (a plugin changed the blocks it adds). */
  refreshBlocks(): void {
    this.#repaint = true;
    this.paintTick += 1;
  }

  /** Called after each update of the blocks of a document that was already on the page. Returns what stops it. */
  onUpdated(listener: (update: BlockUpdate) => void): () => void {
    this.#updaters.add(listener);
    return () => this.#updaters.delete(listener);
  }

  #told(before: readonly string[], blocks: readonly Block[]): void {
    if (this.#updaters.size === 0) return;
    const now = new Set(blocks.map((b) => b.id));
    const had = new Set(before);
    const added = blocks.filter((b) => !had.has(b.id)).map((b) => b.id);
    const removed: { id: string; after: string | null }[] = [];
    let after: string | null = null;
    for (const id of before) {
      if (now.has(id)) after = id;
      else removed.push({ id, after });
    }
    if (added.length === 0 && removed.length === 0) return;
    for (const listener of this.#updaters) listener({ added, removed });
  }

  /** Called after each paint (find re-runs). Returns what stops it. */
  onPainted(listener: () => void): () => void {
    this.#painters.add(listener);
    return () => this.#painters.delete(listener);
  }

  #settled(view: DocumentView, fresh: boolean): void {
    if (!fresh) return;
    const anchor = this.#anchor;
    const line = this.#line;
    this.#anchor = null;
    this.#line = null;
    if (anchor !== null && this.scrollToAnchor(anchor)) return;
    if (line !== null) {
      this.scrollToLine(line, true);
      return;
    }
    const scroller = this.host.scroller;
    if (scroller) scroller.scrollTop = reading.scrollOf(view.path) ?? 0;
  }

  /** Remembers where the document is left. */
  #leave(): void {
    const scroller = this.host.scroller;
    if (this.#shown !== null && scroller) reading.remember(this.#shown, scroller.scrollTop);
  }

  /** The reader scrolled: remember the place, find the heading. */
  noteScroll(): void {
    const scroller = this.host.scroller;
    if (this.#shown !== null && scroller) reading.remember(this.#shown, scroller.scrollTop);
    this.currentHeading = this.#headingAtTop();
  }

  #headingAtTop(): number {
    const scroller = this.host.scroller;
    const view = this.view;
    if (!scroller || view?.body.kind !== "markdown") return -1;
    const outline = view.body.outline;
    if (this.#headings.length !== outline.length) {
      this.#headings = outline.map((item) => this.host.byAnchor(item.id));
    }
    const top = scroller.getBoundingClientRect().top;
    const tops = this.#headings.map((el) =>
      el?.isConnected ? el.getBoundingClientRect().top - top : Infinity,
    );
    // not yet built (a document still arriving in slices) counts as far below
    // a heading within the first bit of the pane counts as reached (a jump leaves it a little below the edge)
    return currentIndex(tops, 40);
  }

  /** Reads the shown document again (after a save of this window: no event comes back for it). */
  reload(): void {
    if (this.#shown !== null) void this.#open(this.#shown, false, true);
  }

  /** Line (1-based) of the first block at the top of the pane (1 when unknown). */
  topLine(): number {
    const view = this.view;
    const scroller = this.host.scroller;
    if (view?.body.kind !== "markdown" || !scroller) return 1;
    const top = scroller.getBoundingClientRect().top;
    for (const block of view.body.blocks) {
      const element = this.host.element(block.id);
      if (element && element.getBoundingClientRect().bottom > top + 1) return block.lineStart;
    }
    return 1;
  }

  /** Scrolls the block holding `line` to the top of the pane; `locate`: it is tinted a moment (a place was asked). */
  scrollToLine(line: number, locate = false): void {
    const view = this.view;
    const scroller = this.host.scroller;
    if (view?.body.kind !== "markdown" || !scroller) return;
    let target: HTMLElement | undefined;
    for (const block of view.body.blocks) {
      if (block.lineStart > line) break;
      target = this.host.element(block.id) ?? target;
    }
    if (!target) return;
    scroller.scrollTop += target.getBoundingClientRect().top - scroller.getBoundingClientRect().top;
    if (locate) tint(target);
  }

  /** Scrolls so that the element with this id is at the top of the pane; false when there is none. */
  scrollToAnchor(id: string): boolean {
    const scroller = this.host.scroller;
    const el = this.host.byAnchor(id);
    if (!scroller || !el) {
      this.#hooks.note(t("doc.sectionMissing", { id }));
      return false;
    }
    const delta = el.getBoundingClientRect().top - scroller.getBoundingClientRect().top - 12;
    scroller.scrollTop += delta;
    tint(el);
    return true;
  }

  // ---- links and navigation --------------------------------------------------------------------------------

  get enhanceEnv(): EnhanceEnv {
    const workspace = this.workspaceId;
    return { assetUrl: (path) => this.#api.assetUrl(workspace ?? 0, path, null) };
  }

  enhance = (element: HTMLElement): HTMLElement => enhanceBlock(element, this.enhanceEnv);

  /** A click on a link of the document. */
  async activateLink(href: string, newTab = false): Promise<void> {
    void newTab;
    const value = href.trim();
    if (value.startsWith("#")) {
      this.#sameDocument(safeDecode(value.slice(1)));
      return;
    }
    if (/^(https?:|mailto:)/i.test(value)) {
      await this.openWeb(value);
      return;
    }
    if (/^[a-z][a-z0-9+.-]*:/i.test(value)) return;
    const path = this.#shown;
    if (path === null) return;
    const [target] = await this.decorator.links.resolve([value]);
    if (target) this.#follow(target);
  }

  #follow(target: LinkTarget): void {
    switch (target.type) {
      case "anchor":
        this.#sameDocument(target.id);
        break;
      case "doc":
        // a single file follows links inside its folder (the backend resolves them there, never above it)
        this.navigate(target.path, target.anchor);
        break;
      case "folder":
        if (!this.singleFile) this.#hooks.reveal(target.path);
        break;
      case "asset":
        if (inApp(target.path)) this.navigate(target.path, null);
        // a single file has no system program to hand a file to
        else if (!this.singleFile) void this.openExternal(target.path, "defaultApp");
        break;
      case "web":
        void this.openWeb(target.url);
        break;
      case "missing":
        this.#hooks.note(t("link.missing", { path: target.path }));
        break;
      case "outside":
        if (this.singleFile) {
          this.#hooks.action(t("link.outsideFile"), {
            label: t("link.openFolder"),
            run: () => this.#hooks.openFolder(),
          });
        } else this.#hooks.note(t("link.outside"));
        break;
      default:
        target satisfies never;
    }
  }

  #sameDocument(id: string): void {
    const path = this.#shown;
    if (path === null) return;
    if (id === "") {
      const scroller = this.host.scroller;
      if (scroller) scroller.scrollTop = 0;
    } else if (!this.scrollToAnchor(id)) {
      return;
    }
    this.trail.visit({ path, anchor: id === "" ? null : id });
  }

  /**
   * Opens another document of the project (a link, the trail, the tree, a search hit). `anchor`: a heading id;
   * `line`: where to scroll when there is no heading to name (the start of a document).
   */
  navigate(path: string, anchor: string | null, line: number | null = null): void {
    if (path === this.#shown) {
      if (anchor === null && line !== null) {
        if (this.view?.body.kind === "markdown") this.scrollToLine(line, true);
        else this.#askLine(path, line);
        this.trail.visit({ path, anchor: null });
      } else this.#sameDocument(anchor ?? "");
      return;
    }
    this.#intent = { path, anchor, line };
    void this.#show(path);
  }

  async #show(path: string): Promise<boolean> {
    if (!(await confirmLeave({ kind: "document", path }))) return false;
    if (this.singleFile) {
      // the window shows it (`DocumentArea` follows `singleDoc`)
      this.singleDoc = path;
      return true;
    }
    const result = await this.#api.showDocument(path);
    if (!result.ok) this.#hooks.fail(result.error);
    return true;
  }

  async back(): Promise<void> {
    const from = this.#trailIndex();
    await this.#go(this.trail.back(), from);
  }

  async forward(): Promise<void> {
    const from = this.#trailIndex();
    await this.#go(this.trail.forward(), from);
  }

  async jump(index: number): Promise<void> {
    const from = this.#trailIndex();
    await this.#go(this.trail.jump(index), from);
  }

  #trailIndex(): number {
    return this.trail.recent(Number.MAX_SAFE_INTEGER).find((r) => r.current)?.index ?? -1;
  }

  async #go(place: Place | null, from: number): Promise<void> {
    if (place === null) return;
    if (place.path === this.#shown) {
      if (place.anchor === null) {
        const scroller = this.host.scroller;
        if (scroller) scroller.scrollTop = 0;
      } else this.scrollToAnchor(place.anchor);
      return;
    }
    this.#viaTrail = place.path;
    if (!(await this.#show(place.path))) {
      // the person stayed (unsaved edits): the trail stays where it was
      this.#viaTrail = null;
      if (from >= 0) this.trail.jump(from);
    }
  }

  /** Follows the document that took this one's place (renamed). */
  followMoved(path: string): void {
    this.navigate(path, null);
  }

  // ---- code references, web, the system --------------------------------------------------------------------

  /**
   * A click | Enter on a code reference to `path` (`modifier`: Ctrl | Cmd held: the editor, whatever the file). Files
   * PerMark shows open here at the line; code opens in the editor, or here when none is chosen. Never an error for
   * want of an editor.
   */
  async openCodeRef(target: CodeRefTarget, path: string, modifier = false): Promise<void> {
    const decision = decideRef(path, this.#hooks.editorSet(), modifier);
    switch (decision.kind) {
      case "inApp":
        this.#refHere(path, target.line, decision.hint);
        return;
      case "chooseEditor":
        this.#noEditor();
        return;
      case "editor": {
        const result = await this.#api.openInEditor(path, target.line, target.col);
        if (result.ok) return;
        const notSet =
          result.error.kind === "backend" && result.error.error.kind === "editorNotSet";
        // the setting went meanwhile: as if none had been chosen
        if (!notSet) this.#hooks.fail(result.error);
        else if (modifier) this.#noEditor();
        else this.#refHere(path, target.line, true);
        return;
      }
      default:
        decision satisfies never;
    }
  }

  #refHere(path: string, line: number | null, hint: boolean): void {
    this.navigate(path, null, line);
    if (!hint || this.#hintedEditor) return;
    this.#hintedEditor = true;
    this.#hooks.action(t("codeRef.hintEditor"), {
      label: t("codeRef.chooseEditor"),
      run: () => this.#hooks.chooseEditor(),
    });
  }

  #noEditor(): void {
    this.#hooks.action(t("codeRef.noEditor"), {
      label: t("codeRef.chooseEditor"),
      run: () => this.#hooks.chooseEditor(),
    });
  }

  codeRefAt(element: Element): CodeRefTarget | null {
    return codeRefOf(element);
  }

  async openInEditor(): Promise<void> {
    const path = this.#shown;
    if (path === null) return;
    const result = await this.#api.openInEditor(path, null, null);
    if (!result.ok) this.#hooks.fail(result.error);
  }

  async openWeb(url: string): Promise<void> {
    const result = await this.#api.openWeb(url);
    if (!result.ok) this.#hooks.fail(result.error);
  }

  async openExternal(path: string, mode: "defaultApp" | "reveal"): Promise<void> {
    const result = await this.#api.openExternal(path, mode);
    if (!result.ok) this.#hooks.fail(result.error);
  }

  // ---- copy ------------------------------------------------------------------------------------------------

  async #copy(text: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      this.#hooks.note(t("doc.copyFailed"));
    }
  }

  /** "Copy reference": where the document lives. */
  async copyReference(template: string): Promise<void> {
    const path = this.#shown;
    if (path === null) return;
    await this.#copy(formatReference(template, { path, section: null, line: null }));
  }

  /** "Copy": the whole text of the file as read. */
  async copyDocument(): Promise<void> {
    const path = this.#shown;
    if (path === null) return;
    const result = await this.#api.documentText(path);
    if (!result.ok) {
      this.#hooks.fail(result.error);
      return;
    }
    await this.#copy(result.value);
  }

  /** The reference of a passage: the document, the heading above it, its first line. */
  referenceAt(node: Node | null): Reference | null {
    const path = this.#shown;
    const view = this.view;
    if (path === null || view?.body.kind !== "markdown")
      return path === null ? null : { path, section: null, line: null };
    const element = node instanceof Element ? node : (node?.parentElement ?? null);
    const block = element?.closest<HTMLElement>("[data-b]") ?? null;
    const id = block?.getAttribute("data-b") ?? null;
    const line = view.body.blocks.find((b) => b.id === id)?.lineStart ?? null;
    let section: string | null = null;
    if (block) {
      // the last heading in or before this block
      const outline = view.body.outline;
      const blocks = view.body.blocks;
      const at = blocks.findIndex((b) => b.id === id);
      for (let i = outline.length - 1; i >= 0; i--) {
        const item = outline[i];
        const itemAt = item ? blocks.findIndex((b) => b.id === item.block) : -1;
        if (item && itemAt !== -1 && itemAt <= at) {
          section = item.id;
          break;
        }
      }
    }
    return { path, section, line };
  }

  /** "Copy with reference": the reference line, then the selection quoted. */
  async copySelection(text: string, node: Node | null, template: string): Promise<void> {
    const ref = this.referenceAt(node);
    if (ref === null) return;
    await this.#copy(formatQuote(template, ref, text));
  }

  async copyPlain(text: string): Promise<void> {
    await this.#copy(text);
  }
}

/** Tints an element a moment (the place that was asked for); the animation starts again when it is asked again. */
function tint(el: HTMLElement): void {
  el.classList.remove("pm-located");
  void el.offsetWidth;
  el.classList.add("pm-located");
  setTimeout(() => el.classList.remove("pm-located"), 1600);
}

function safeDecode(text: string): string {
  try {
    return decodeURIComponent(text);
  } catch {
    return text;
  }
}

export const documents = new DocumentController();
