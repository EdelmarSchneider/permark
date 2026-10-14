// Application controller: boot, opening projects, backend events, navigation, copy actions and the
// keyboard map. Stores hold state; this module wires them together.

import { SvelteSet } from "svelte/reactivity";
import { api, type Unlisten } from "../api";
import { bridge } from "../bridge";
import { isTextSelected } from "../dom/domUtil";
import { errorText } from "../errors";
import { followCandidate } from "../follow";
import { t } from "../i18n";
import { hasOpenOverlay } from "../overlays";
import { formatQuote, formatReference } from "../reference";
import type { ShortcutAction } from "../shortcuts";
import { selectOnly } from "../tagModel";
import { copyText, openExternalUrl, pickFile, pickFolder, watchFileDrop } from "../platform";
import type { FsBatch, LinkTarget, RecentVault, SearchHit, VaultInfo } from "../types";
import { clock } from "./clock.svelte";
import { docs, type OpenOptions } from "./docs.svelte";
import { feed } from "./feed.svelte";
import { filter } from "./filter.svelte";
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
      docs.reset();
      feed.clear();
      filter.reset();
      search.reset();
      tags.reset();
      toast.clear();
      vault.reset();
      ui.followChip = null;
      ui.find.open = false;
      ui.quickOpen = false;
      ui.tagEditor = null;
      ui.sidebarView = "explorer";
      ui.dismissedRepoState = null;
      vault.info = info;

      const [stored, tree, git, location] = await Promise.all([
        api.loadSession().catch(() => null),
        api.listTree(),
        api.gitSnapshot(),
        api.stateLocation().catch(() => "appData" as const),
      ]);
      vault.tree = tree;
      vault.git = git;
      vault.stateLocation = location;
      void vault.refreshSince();
      if (info.mode === "ide") void tags.refreshIndex();
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

  private followBlocked(): boolean {
    return isTextSelected() || hasOpenOverlay();
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
    if (session.right.open) ui.rightNotify = false;
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
        if (ide && docs.current) ui.openTagEditor();
        return;
      case "find":
        return this.openFind();
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
      case "settings":
        ui.settingsOpen = true;
        return;
    }
  }
}

export const app = new AppStore();
