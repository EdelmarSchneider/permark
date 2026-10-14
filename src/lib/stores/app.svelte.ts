// Application controller: boot, opening projects, backend events, navigation, copy actions and the
// keyboard map. Stores hold state; this module wires them together.

import { SvelteSet } from "svelte/reactivity";
import { api, type Unlisten } from "../api";
import { bridge } from "../bridge";
import { isTextSelected } from "../dom/domUtil";
import { blankTemplate, newFilePath, targetFolder } from "../editor/newFile";
import { errorKind, errorText } from "../errors";
import { followCandidate } from "../follow";
import { t } from "../i18n";
import { hasOpenOverlay } from "../overlays";
import { formatQuote, formatReference } from "../reference";
import { toggleOutline as outlineToggled, type RightTab } from "../rightPanel";
import type { ShortcutAction } from "../shortcuts";
import { planReveal } from "../reveal";
import { selectOnly } from "../tagModel";
import { copyText, openExternalUrl, pickFile, pickFolder, watchFileDrop } from "../platform";
import { normalizeTreeFilter, treeLoadOf } from "../treeFilter";
import type { FsBatch, LinkTarget, RecentVault, SearchHit, VaultInfo } from "../types";
import { clock } from "./clock.svelte";
import { docs, type OpenOptions } from "./docs.svelte";
import { edit } from "./edit.svelte";
import { feed } from "./feed.svelte";
import { fileops } from "./fileops.svelte";
import { filter } from "./filter.svelte";
import { outline } from "./outline.svelte";
import { search } from "./search.svelte";
import { session } from "./session.svelte";
import { tags } from "./tags.svelte";
import { toast } from "./toast.svelte";
import { ui } from "./ui.svelte";
import { vault } from "./vault.svelte";

const HIDDEN_RECENTS_KEY = "trailmark.recents.hidden";
const FOLLOW_CHIP_MS = 5000;
const TREE_REFRESH_MS = 250;
const SINCE_REFRESH_MS = 1200;

function readHiddenRecents(): string[] {
  try {
    const raw = localStorage.getItem(HIDDEN_RECENTS_KEY);
    const parsed: unknown = raw ? JSON.parse(raw) : [];
    return Array.isArray(parsed) ? parsed.filter((v): v is string => typeof v === "string") : [];
  } catch {
    return [];
  }
}

class AppStore {
  screen = $state<"boot" | "welcome" | "project">("boot");
  opening = $state(false);
  openError = $state<string | null>(null);
  recents = $state.raw<RecentVault[]>([]);
  /** Recent folders that failed to open in this run (dimmed, only "Remove" is offered). */
  missingRecents = new SvelteSet<string>();
  hiddenRecents = new SvelteSet<string>(readHiddenRecents());

  private unlisten: Unlisten[] = [];
  private treeTimer: ReturnType<typeof setTimeout> | null = null;
  private sinceTimer: ReturnType<typeof setTimeout> | null = null;
  private followTimer: ReturnType<typeof setTimeout> | null = null;
  private chipCount = 0;

  // ---- boot and projects ------------------------------------------------------------------------

  async boot(): Promise<void> {
    session.watchSystemTheme();
    clock.start();
    await this.subscribe();
    window.addEventListener("blur", () => void session.flush());
    document.addEventListener("visibilitychange", () => {
      if (document.visibilityState === "hidden") void session.flush();
    });
    this.unlisten.push(
      await watchFileDrop({
        over: (count) => {
          ui.drag = { count };
        },
        leave: () => {
          ui.drag = null;
        },
        drop: (paths) => void this.handleDrop(paths),
      }),
    );

    let initial: string | null = null;
    try {
      initial = await api.initialPath();
    } catch {
      initial = null;
    }
    if (initial) {
      const ok = await this.openProject(initial);
      if (ok) return;
    }
    await this.showWelcome();
  }

  async showWelcome(): Promise<void> {
    try {
      this.recents = await api.recentVaults();
    } catch {
      this.recents = [];
    }
    this.screen = "welcome";
  }

  get visibleRecents(): RecentVault[] {
    return this.recents.filter((r) => !this.hiddenRecents.has(r.root));
  }

  removeRecent(root: string): void {
    this.hiddenRecents.add(root);
    try {
      localStorage.setItem(HIDDEN_RECENTS_KEY, JSON.stringify([...this.hiddenRecents]));
    } catch {
      // the entry reappears next launch; nothing else depends on it
    }
  }

  async chooseFolder(): Promise<void> {
    try {
      const path = await pickFolder();
      if (path) await this.openProject(path);
    } catch (error) {
      this.openError = errorText(error);
    }
  }

  async chooseFile(): Promise<void> {
    try {
      const path = await pickFile();
      if (path) await this.openProject(path);
    } catch (error) {
      this.openError = errorText(error);
    }
  }

  private async handleDrop(paths: string[]): Promise<void> {
    ui.drag = null;
    // a file dropped on the window while editing must not replace the project; images are pasted
    if (edit.active) {
      toast.message(t("images.dropWhileEditing"), "info");
      return;
    }
    if (paths.length !== 1) {
      this.openError = t("welcome.dropOne");
      toast.message(t("welcome.dropOne"), "warn");
      return;
    }
    await this.openProject(paths[0]);
  }

  /** Opens a folder (IDE mode) or a file (Simple mode), replacing the current project. */
  async openProject(path: string): Promise<boolean> {
    if (this.opening) return false;
    // unsaved edits are asked about first; the answer carries on with this
    if (this.screen === "project" && edit.holdNavigation(() => void this.openProject(path))) {
      return false;
    }
    this.opening = true;
    this.openError = null;
    try {
      await session.flush();
      let info: VaultInfo;
      try {
        info = await api.openVault(path);
      } catch (error) {
        const message = t("welcome.openFailed", { path, message: errorText(error) });
        this.openError = message;
        if (this.screen !== "project") this.missingRecents.add(path);
        else toast.message(message, "warn");
        return false;
      }

      session.suspend();
      edit.reset();
      docs.reset();
      feed.clear();
      fileops.reset();
      filter.reset();
      outline.reset();
      search.reset();
      tags.reset();
      toast.clear();
      vault.reset();
      ui.followChip = null;
      ui.find.open = false;
      ui.quickOpen = false;
      ui.tagEditor = null;
      ui.outlinePopover = false;
      ui.changesNotify = false;
      ui.rightTabBack = "changes";
      ui.sidebarView = "explorer";
      ui.dismissedRepoState = null;
      ui.newFile = null;
      ui.treeCursor = null;
      vault.info = info;

      // the tree is loaded the way the saved Explorer filter asks for it (ignored and hidden files)
      const storedSession = api.loadSession().catch(() => null);
      const [stored, tree, git, location] = await Promise.all([
        storedSession,
        storedSession.then((s) => {
          const load = treeLoadOf(normalizeTreeFilter(s?.treeFilter));
          return api.listTree(load.showIgnored, load.showHidden);
        }),
        api.gitSnapshot(),
        api.stateLocation().catch(() => "appData" as const),
      ]);
      vault.tree = tree;
      vault.git = git;
      vault.stateLocation = location;
      void vault.refreshSince();
      if (info.mode === "ide") {
        void tags.refreshIndex();
        void vault.refreshImageRefs();
      }
      const restored = session.load(stored);
      this.screen = "project";

      const first = this.initialDocument(info, restored.lastDoc);
      if (first) {
        await this.openDoc(first.path, {
          scrollTop: first.restoreScroll ? restored.scrollTop : null,
          silent: true,
        });
      }
      return true;
    } catch (error) {
      this.openError = t("welcome.openFailed", { path, message: errorText(error) });
      return false;
    } finally {
      this.opening = false;
    }
  }

  /** Which document opens first: the CLI file, the session's last document, or the README. */
  private initialDocument(
    info: VaultInfo,
    lastDoc: string | null,
  ): { path: string; restoreScroll: boolean } | null {
    if (info.initialFile) return { path: info.initialFile, restoreScroll: false };
    if (info.mode === "simple") return null;
    if (lastDoc && vault.files.includes(lastDoc)) return { path: lastDoc, restoreScroll: true };
    const readme = vault.files.find((f) => /^(readme|index)\.md$/i.test(f));
    return readme ? { path: readme, restoreScroll: false } : null;
  }

  // ---- backend events ---------------------------------------------------------------------------

  private async subscribe(): Promise<void> {
    this.unlisten.push(
      await api.on("fs-change", (batch) => this.onFsChange(batch)),
      await api.on("tree-changed", () => this.onTreeChanged()),
      await api.on("git-status-changed", (snapshot) => {
        vault.git = snapshot;
        this.scheduleSinceRefresh();
      }),
      await api.on("index-progress", (progress) => {
        vault.indexProgress = progress.indexed >= progress.total ? null : progress;
      }),
      await api.on("meta-changed", () => {
        if (this.screen === "project" && !vault.isSimple) tags.onMetaChanged();
      }),
      await api.on("image-refs-changed", () => {
        if (this.screen === "project" && !vault.isSimple) void vault.refreshImageRefs();
      }),
    );
  }

  private onTreeChanged(): void {
    if (this.treeTimer !== null) clearTimeout(this.treeTimer);
    this.treeTimer = setTimeout(() => void vault.refreshTree(), TREE_REFRESH_MS);
  }

  private scheduleSinceRefresh(): void {
    if (this.sinceTimer !== null) clearTimeout(this.sinceTimer);
    this.sinceTimer = setTimeout(() => void vault.refreshSince(), SINCE_REFRESH_MS);
  }

  private onFsChange(batch: FsBatch): void {
    if (this.screen !== "project") return;
    edit.onFsChange(batch);
    const now = Date.now();
    if (vault.isSimple) {
      docs.handleBatch(batch, false);
      return;
    }
    const update = feed.apply(batch, now);
    if (batch.rescan) void vault.refreshTree();
    tags.onFilesChanged();

    const docChanged = update.bulk !== null || update.files.some((f) => f.isDoc);
    if (docChanged && !session.right.open) ui.rightNotify = true;
    // the same rule for the Changes tab while the Outline is the view showing
    if (docChanged && session.rightTab !== "changes") ui.changesNotify = true;

    let jumped: string | null = null;
    if (session.follow && update.bulk === null) {
      const candidate = followCandidate(batch);
      if (candidate !== null && candidate !== docs.path) {
        jumped = feed.gate.offer(candidate, now, this.followBlocked());
        if (jumped !== null) void this.followTo(jumped);
        else this.scheduleFollowRetry();
      }
    }
    docs.handleBatch(batch, session.follow);

    if (update.bulk !== null) return;
    for (const file of update.files) {
      if (!file.isDoc && feed.docsOnly) continue;
      if (file.path === docs.path || file.path === jumped) continue;
      toast.noteChange(file.path, file.kind, now);
    }
    if (docChanged) this.scheduleSinceRefresh();
  }

  // ---- Follow -------------------------------------------------------------------------------------

  /** Follow never jumps away from a document being edited (clean or not). */
  private followBlocked(): boolean {
    return edit.active || isTextSelected() || hasOpenOverlay();
  }

  private async followTo(path: string): Promise<void> {
    const from = docs.path;
    const ok = await this.openDoc(path, { fromChange: true });
    if (!ok || !from) return;
    const n = ++this.chipCount;
    ui.followChip = { from, to: path, n };
    setTimeout(() => {
      if (ui.followChip?.n === n) ui.followChip = null;
    }, FOLLOW_CHIP_MS);
  }

  private scheduleFollowRetry(): void {
    const wait = feed.gate.msUntilDue(Date.now());
    if (wait === null) return;
    if (this.followTimer !== null) clearTimeout(this.followTimer);
    this.followTimer = setTimeout(() => {
      this.followTimer = null;
      const path = feed.gate.due(Date.now(), this.followBlocked());
      if (path !== null && path !== docs.path && session.follow) void this.followTo(path);
    }, wait + 20);
  }

  toggleFollow(): void {
    session.toggleFollow();
    if (!session.follow) {
      feed.gate.clear();
      ui.followChip = null;
    }
  }

  // ---- navigation ---------------------------------------------------------------------------------

  /** Opens a document (the Explorer shows it too, unless "Always select the opened file" is off). */
  openDoc(path: string, opts: OpenOptions = {}): Promise<boolean> {
    return docs.open(path, opts);
  }

  /** Opens a document because it changed (feed row, toast, Follow): marks what is new in it. */
  openChanged(path: string): Promise<boolean> {
    return this.openDoc(path, { fromChange: true });
  }

  async openHit(hit: SearchHit): Promise<void> {
    const query = search.answered || search.query.trim();
    await this.openDoc(hit.path, {
      locate: { line: hit.startLine, find: query ? { query, mode: "terms" } : null },
    });
  }

  /** Follows a link clicked in the document. Resolves to the missing path when it does not exist. */
  async followLink(from: string, href: string): Promise<{ missing: string } | null> {
    let target: LinkTarget;
    try {
      target = await api.resolveLink(from, href);
    } catch (error) {
      toast.message(errorText(error), "warn");
      return null;
    }
    switch (target.kind) {
      case "internal":
        await this.openDoc(target.path, { anchor: target.anchor });
        return null;
      case "external":
        try {
          await openExternalUrl(target.url);
        } catch (error) {
          toast.message(t("link.openFailed", { message: errorText(error) }), "warn");
        }
        return null;
      case "anchor":
        docs.jumpTo(target.anchor);
        return null;
      case "missing":
        return { missing: target.path };
    }
  }

  // ---- new file -------------------------------------------------------------------------------------

  /** Ctrl+N and "New file...": the name is typed in the tree, under the selected folder or the open document's. */
  startNewFile(folder?: string): void {
    if (vault.isSimple) {
      toast.message(t("newFile.simpleMode"), "info");
      return;
    }
    // The new file replaces the open document, so unsaved edits are asked about before anything is made: Save or Discard goes
    // on to the name, Keep editing creates nothing.
    if (edit.active && edit.dirty && edit.holdNavigation(() => this.startNewFile(folder))) return;
    const target = folder ?? targetFolder(ui.treeCursor, docs.path);
    ui.sidebarView = "explorer";
    if (!session.left.open) session.setPanel("left", { open: true });
    // the folders above the new file's folder open, and so does the folder itself
    const plan = planReveal(target === "" ? "x" : `${target}/x`, vault.expanded);
    ui.collapsedSections.delete(plan.section);
    for (const dir of plan.expand) vault.expanded.add(dir);
    if (target !== "") vault.expanded.add(target);
    ui.newFile = { folder: target, n: (ui.newFile?.n ?? 0) + 1 };
  }

  /**
   * Makes the file the tree input asked for and opens it for editing, the caret after its title. Resolves to the
   * message to show under the input when it cannot be made, else null.
   */
  async createNewFile(folder: string, name: string): Promise<string | null> {
    const path = newFilePath(folder, name);
    try {
      await api.createDocument(path, blankTemplate(name));
    } catch (error) {
      return errorKind(error) === "exists" ? t("newFile.exists") : errorText(error);
    }
    ui.newFile = null;
    await vault.refreshTree();
    // Editing starts once the file is open. Unsaved work in the open document was asked about before the name (see
    // `startNewFile`); if there is some again by now (typed while the name was open) the open waits for the answer as
    // any other move does, and the start of editing travels with it.
    await this.openDoc(path, { afterOpen: () => void edit.enter(true) });
    return null;
  }

  // ---- baseline, panels, views ---------------------------------------------------------------------

  async setBaseline(kind: "lastVisit" | "head"): Promise<void> {
    try {
      vault.git = await api.setGitBaseline(kind);
      await vault.refreshSince();
    } catch (error) {
      toast.message(errorText(error), "warn");
    }
  }

  toggleLeft(): void {
    session.setPanel("left", { open: !session.left.open });
  }

  toggleRight(): void {
    session.setPanel("right", { open: !session.right.open });
    if (session.right.open) {
      ui.rightNotify = false;
      if (session.rightTab === "changes") ui.changesNotify = false;
    }
  }

  /** Shows one view of the right panel (a click on its tab). */
  showRightTab(tab: RightTab): void {
    if (session.rightTab === tab) return;
    ui.rightTabBack = session.rightTab;
    session.setRightTab(tab);
    if (tab === "changes") ui.changesNotify = false;
  }

  /**
   * Ctrl+Alt+O: the Outline tab (opening the right panel if it is collapsed), and back to the previous tab when
   * pressed again. Simple mode has no panel: the outline opens as a popover under the header.
   */
  toggleOutline(): void {
    if (vault.isSimple) {
      ui.outlinePopover = !ui.outlinePopover;
      return;
    }
    const next = outlineToggled({
      open: session.right.open,
      tab: session.rightTab,
      back: ui.rightTabBack,
    });
    ui.rightTabBack = next.back;
    if (!session.right.open) session.setPanel("right", { open: true });
    ui.rightNotify = false;
    if (next.tab !== session.rightTab) session.setRightTab(next.tab);
    if (next.tab === "outline") {
      ui.outlineFocusTick++;
    } else {
      // the list that had the focus is gone: the reader goes back to the document
      ui.changesNotify = false;
      bridge.outline?.focusDoc();
    }
  }

  showSearch(): void {
    if (vault.isSimple) return;
    ui.sidebarView = "search";
    if (!session.left.open) session.setPanel("left", { open: true });
    ui.searchFocusTick++;
  }

  showExplorer(): void {
    if (vault.isSimple) return;
    ui.sidebarView = "explorer";
    if (!session.left.open) session.setPanel("left", { open: true });
    ui.explorerFocusTick++;
  }

  showTags(): void {
    if (vault.isSimple) return;
    ui.sidebarView = "tags";
    if (!session.left.open) session.setPanel("left", { open: true });
    ui.tagsFocusTick++;
    void tags.refreshOverview();
    void tags.refreshFiles();
  }

  /** A tag chip was clicked: the Tags view, showing the files with that tag. */
  showTagsFor(tag: string): void {
    if (vault.isSimple) return;
    tags.filter = "";
    tags.select(selectOnly(tag));
    this.showTags();
  }

  /** Activity bar: switch the sidebar view, or collapse the sidebar when its view is already showing. */
  clickView(view: "explorer" | "search" | "tags"): void {
    if (ui.sidebarView === view && session.left.open) {
      session.setPanel("left", { open: false });
      return;
    }
    if (view === "search") this.showSearch();
    else if (view === "tags") this.showTags();
    else this.showExplorer();
  }

  revealInTree(path: string): void {
    if (vault.isSimple) return;
    ui.sidebarView = "explorer";
    if (!session.left.open) session.setPanel("left", { open: true });
    vault.revealPath(path);
  }

  openFind(prefill?: string): void {
    if (!docs.current) return;
    const selected = prefill ?? bridge.getSelection?.()?.text;
    if (selected && !selected.includes("\n")) {
      ui.find.query = selected;
      ui.find.mode = "phrase";
    }
    ui.find.open = true;
    ui.find.takeFocus = true;
    ui.find.focusTick++;
  }

  // ---- copy and external actions ---------------------------------------------------------------------

  private async referencePath(path: string): Promise<string> {
    // worktree paths (@wt/...) mean nothing to a terminal: use the real location
    if (!path.startsWith("@wt/")) return path;
    try {
      return (await api.absolutePath(path)).replace(/\\/g, "/");
    } catch {
      return path;
    }
  }

  async copyReference(path = docs.path, flashId = "reference"): Promise<void> {
    if (!path) return;
    const text = formatReference(session.refTemplate, await this.referencePath(path));
    await this.copy(text, flashId);
  }

  /** Ctrl+Alt+C: the selection as a quote with its reference, or just the reference. */
  async copyReferenceContextAware(): Promise<void> {
    const selection = bridge.getSelection?.();
    if (selection) await this.copyQuote(selection.text, selection.section);
    else await this.copyReference();
  }

  async copyQuote(text: string, section: string | null, flashId = "selection"): Promise<void> {
    const path = docs.path;
    if (!path) return;
    const quote = formatQuote({
      template: session.refTemplate,
      path: await this.referencePath(path),
      section,
      text,
      sectionLabel: (name) => t("reference.section", { name }),
    });
    await this.copy(quote, flashId);
  }

  async copyPath(path = docs.path, flashId = "path"): Promise<void> {
    if (!path) return;
    try {
      await this.copy((await api.absolutePath(path)).replace(/\\/g, "/"), flashId);
    } catch (error) {
      toast.message(errorText(error), "warn");
    }
  }

  async copy(text: string, flashId: string): Promise<void> {
    const ok = await copyText(text);
    if (ok) ui.flash(flashId);
    else toast.message(t("copy.failed"), "warn");
  }

  async openExternally(path = docs.path): Promise<void> {
    if (!path) return;
    try {
      await api.openExternal(path);
    } catch (error) {
      toast.message(t("doc.externalFailed", { message: errorText(error) }), "warn");
    }
  }

  async revealInFolder(path: string): Promise<void> {
    try {
      await api.revealInFolder(path);
    } catch (error) {
      toast.message(t("doc.externalFailed", { message: errorText(error) }), "warn");
    }
  }

  async openContainingFolder(): Promise<void> {
    const root = vault.info?.root;
    if (root) await this.openProject(root);
  }

  async initProject(): Promise<void> {
    await api.initProjectFolder();
    vault.stateLocation = await api.stateLocation();
    ui.dismissHint();
  }

  // ---- keyboard ------------------------------------------------------------------------------------------

  run(action: ShortcutAction): void {
    if (this.screen === "welcome") {
      if (action === "openFolder") void this.chooseFolder();
      else if (action === "openFile") void this.chooseFile();
      return;
    }
    if (this.screen !== "project") return;
    const ide = !vault.isSimple;
    switch (action) {
      case "openFolder":
        return void this.chooseFolder();
      case "openFile":
        return void this.chooseFile();
      case "quickOpen":
        if (ide) ui.quickOpen = true;
        return;
      case "searchAll":
        return this.showSearch();
      case "explorer":
        return this.showExplorer();
      case "tagsView":
        return this.showTags();
      case "editTags":
        if (ide && docs.current && !edit.active) ui.openTagEditor();
        return;
      case "edit":
        if (docs.current && !edit.active) void edit.enter();
        return;
      case "toggleEdit":
        if (docs.current || edit.active) edit.toggle();
        return;
      case "toggleEditMode":
        if (edit.active) void edit.toggleMode();
        return;
      case "save":
        if (edit.active) void edit.save();
        return;
      case "newFile":
        return this.startNewFile();
      case "find":
        // the editor has no find yet, and the reading view it would search is hidden
        if (!edit.active) this.openFind();
        return;
      case "back":
        return void docs.back();
      case "forward":
        return void docs.forward();
      case "nextChange":
        return bridge.jumpChange?.(1);
      case "prevChange":
        return bridge.jumpChange?.(-1);
      case "toggleFollow":
        if (ide) this.toggleFollow();
        return;
      case "copyReference":
        return void this.copyReferenceContextAware();
      case "copyPath":
        return void this.copyPath();
      case "openExternal":
        return void this.openExternally();
      case "toggleLeft":
        if (ide) this.toggleLeft();
        return;
      case "toggleRight":
        if (ide) this.toggleRight();
        return;
      case "zoomIn":
        return session.zoomBy(1);
      case "zoomOut":
        return session.zoomBy(-1);
      case "zoomReset":
        return session.setZoom(1);
      case "cycleWidth":
        return session.cycleWidth();
      case "toggleOutline":
        return this.toggleOutline();
      case "settings":
        ui.settingsOpen = true;
        return;
    }
  }
}

export const app = new AppStore();
