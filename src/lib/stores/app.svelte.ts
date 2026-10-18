// Application controller: boot, opening projects, backend events, navigation, copy actions and the
// keyboard map. Stores hold state; this module wires them together.

import { SvelteSet } from "svelte/reactivity";
import { api, type Unlisten } from "../api";
import { mcpJson } from "../agentSetup";
import { bridge } from "../bridge";
import { refLabel } from "../codeRefs";
import { isTextSelected } from "../dom/domUtil";
import { blankTemplate, newFilePath, targetFolder } from "../editor/newFile";
import { errorKind, errorMessage } from "../errors";
import { usesSystemApp } from "../externalEditor";
import { followCandidate } from "../follow";
import { t, tn } from "../i18n";
import { hasOpenOverlay } from "../overlays";
import { basename, isMarkdown, isWebPage } from "../paths";
import { formatQuote, formatReference } from "../reference";
import { toggleOutline as outlineToggled, type RightTab } from "../rightPanel";
import type { CommandAction } from "../commands";
import type { Anchor } from "../components/Popover.svelte";
import { sameFolder } from "../folderSwitcher";
import { planReveal } from "../reveal";
import { selectOnly } from "../tagModel";
import { batchTouches, nextUnread } from "../unread";
import {
  closeThisWindow,
  copyText,
  openExternalUrl,
  pickFile,
  pickFolder,
  watchFileDrop,
} from "../platform";
import { normalizeTreeFilter, treeLoadOf } from "../treeFilter";
import type {
  AgentOpen,
  FsBatch,
  LinkTarget,
  Comment,
  NotesOverview,
  NotesRequest,
  RecentVault,
  SearchHit,
  VaultInfo,
} from "../types";
import { agent } from "./agent.svelte";
import { clock } from "./clock.svelte";
import { distribution } from "./distribution.svelte";
import { docs, type OpenOptions } from "./docs.svelte";
import { edit } from "./edit.svelte";
import { editorCommand } from "./editorCommand.svelte";
import { feed } from "./feed.svelte";
import { fileops } from "./fileops.svelte";
import { filter } from "./filter.svelte";
import { brokenLinks } from "./brokenLinks.svelte";
import { comments } from "./comments.svelte";
import { review } from "./review.svelte";
import { notes } from "./notes.svelte";
import { media } from "./media.svelte";
import { outline } from "./outline.svelte";
import { search } from "./search.svelte";
import { session } from "./session.svelte";
import { tags } from "./tags.svelte";
import { toast } from "./toast.svelte";
import { ui } from "./ui.svelte";
import { unread } from "./unread.svelte";
import { updates } from "./updates.svelte";
import { vault } from "./vault.svelte";

const HIDDEN_RECENTS_KEY = "permark.recents.hidden";
const FOLLOW_CHIP_MS = 5000;
const TREE_REFRESH_MS = 250;
const SINCE_REFRESH_MS = 1200;

/**
 * Where the folder switcher opens when nothing was clicked (the command in Search everywhere): under the project's name in the
 * Explorer when that is on screen, else near the top left of the window.
 */
function switcherAnchor(): Anchor {
  const name =
    session.left.open && ui.sidebarView === "explorer"
      ? document.querySelector<HTMLElement>("[data-folder-switcher]")
      : null;
  const rect = name?.getBoundingClientRect();
  return rect && rect.width > 0 ? rect : { left: 56, right: 56, top: 40, bottom: 40 };
}

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
  /** "notes" is the Notes window: a program of its own (see `notes.svelte.ts`). */
  screen = $state<"boot" | "welcome" | "project" | "notes">("boot");
  opening = $state(false);
  openError = $state<string | null>(null);
  recents = $state.raw<RecentVault[]>([]);
  /** The notes folder and the pinned and latest notes, for the column of the welcome screen. */
  notesOverview = $state.raw<NotesOverview | null>(null);
  /** Recent folders that failed to open in this run (dimmed, only "Remove" is offered). */
  missingRecents = new SvelteSet<string>();
  hiddenRecents = new SvelteSet<string>(readHiddenRecents());

  private unlisten: Unlisten[] = [];
  /** The welcome screen and the project window take files dropped on the window; the Notes window listens on its own. */
  private dropListener: Unlisten | null = null;
  /** The welcome screen is turning into the Notes window (asked once; the answer is on its way). */
  private becoming = false;
  private treeTimer: ReturnType<typeof setTimeout> | null = null;
  private sinceTimer: ReturnType<typeof setTimeout> | null = null;
  private followTimer: ReturnType<typeof setTimeout> | null = null;
  private chipCount = 0;

  // ---- boot and projects ------------------------------------------------------------------------

  async boot(): Promise<void> {
    void distribution.load();
    session.watchSystemTheme();
    clock.start();
    await this.subscribe();
    window.addEventListener("blur", () => void session.flush());
    document.addEventListener("visibilitychange", () => {
      if (document.visibilityState === "hidden") void session.flush();
    });
    // The Notes window is a program of its own: it has no project to open, and nothing to check for updates (a window of a
    // project does that)
    const mode = await api.launchMode().catch(() => ({ notes: false }));
    if (mode.notes) {
      this.screen = "notes";
      await notes.start();
      return;
    }
    void updates.scheduleStartupCheck();
    // a document counts as read only in a window the person is looking at
    const noteFocus = (): void =>
      unread.setFocused(document.hasFocus() && document.visibilityState === "visible");
    window.addEventListener("focus", noteFocus);
    window.addEventListener("blur", noteFocus);
    document.addEventListener("visibilitychange", noteFocus);
    noteFocus();
    this.dropListener = await watchFileDrop({
      over: (count) => {
        ui.drag = { count };
      },
      leave: () => {
        ui.drag = null;
      },
      drop: (paths) => void this.handleDrop(paths),
    });

    let initial: string | null = null;
    try {
      initial = await api.initialPath();
    } catch {
      initial = null;
    }
    if (initial) {
      const where = await api.classifyPath(initial).catch(() => null);
      if (where?.kind === "file") {
        // a file the system opened with this window (Finder does it after the program started) is the Notes window's: this
        // window has nothing to show
        if (await this.openFile(initial, null)) await closeThisWindow();
      } else if (await this.openProject(initial)) {
        await this.showLaunched();
        return;
      }
    }
    await this.showWelcome();
  }

  /**
   * A window started by "Open in project" (a note was moved into the project) is asked to show that file: it is shown once the
   * project is open, when the project holds it.
   */
  private async showLaunched(): Promise<void> {
    const show = await api.launchShow().catch(() => null);
    if (!show) return;
    const where = await api.classifyPath(show).catch(() => null);
    if (where?.inProject) await this.openDoc(where.inProject);
  }

  async showWelcome(): Promise<void> {
    await this.loadWelcome();
    this.screen = "welcome";
  }

  /** What the welcome screen lists: the recent folders, and the Notes column. */
  private async loadWelcome(): Promise<void> {
    try {
      this.recents = await api.recentVaults();
    } catch {
      this.recents = [];
    }
    // the Notes column is a convenience: the screen is there without it
    this.notesOverview = await api.notesOverview().catch(() => null);
  }

  /** `Ctrl+Shift+N` from any window: the Notes window comes forward with a new note ready (and starts when it is not running). */
  async newNote(): Promise<void> {
    if (this.screen === "notes") return notes.newNote();
    await this.showNotes(true, null);
  }

  /** `Ctrl+Shift+D`, the Notes icon: the Notes window comes forward as it was, or shows a given note. */
  async showNotes(newNote = false, open: string | null = null): Promise<void> {
    if (this.screen === "notes") {
      notes.focusList();
      return;
    }
    // the welcome screen turns into the Notes window; a project window stays as it is and the Notes window comes forward
    if (this.canBecomeNotes()) {
      await this.becomeNotes({ newNote, open, openFile: null }, errorMessage);
      return;
    }
    try {
      await api.showNotes(newNote, open);
    } catch (error) {
      toast.message(errorMessage(error), "warn");
    }
  }

  /**
   * A window that shows only the welcome screen (no project open, none being opened) can turn into the Notes window. There is
   * only ever one Notes window, so asking for it from here must not leave this window behind.
   */
  private canBecomeNotes(): boolean {
    return this.screen === "welcome" && !this.opening;
  }

  /**
   * The welcome screen asks for the Notes window with `request`. When none is running, this very window becomes it (the same
   * native window, so its place and size stay; the screen changes from the welcome screen to the notes). When one runs in another
   * program, that one is asked to come forward and this window closes. Resolves to whether the request was taken; when it was not,
   * `describe` says why on the welcome screen, which stays.
   */
  private async becomeNotes(
    request: NotesRequest,
    describe: (error: unknown) => string,
  ): Promise<boolean> {
    if (this.becoming) return false;
    this.becoming = true;
    try {
      const handoff = await api.becomeNotesWindow(request.newNote, request.open, request.openFile);
      if (handoff === "forwarded") {
        await closeThisWindow();
        return true;
      }
      // Files dropped on the window are the Notes window's from here on (it listens by itself), and a window that was dragged over
      // is no longer being dragged over.
      this.dropListener?.();
      this.dropListener = null;
      ui.drag = null;
      this.openError = null;
      this.screen = "notes";
      await notes.start();
      return true;
    } catch (error) {
      this.openError = describe(error);
      return false;
    } finally {
      this.becoming = false;
    }
  }

  /**
   * `Ctrl+Shift+C`: copies the whole open document as plain text, exactly as the file holds it (what the editor has, while
   * editing). In the Notes window it copies the note.
   */
  async copyDocument(): Promise<void> {
    if (this.screen === "notes") return notes.copyNote();
    const path = docs.path;
    if (!path) return;
    let text: string;
    try {
      text = edit.active ? edit.exactText() : (await api.documentSource(path)).text;
    } catch (error) {
      toast.message(errorMessage(error), "warn");
      return;
    }
    if (!(await copyText(text))) {
      toast.message(t("copy.failed"), "warn");
      return;
    }
    toast.message(tn("notes.copied", Array.from(text).length), "info");
  }

  get visibleRecents(): RecentVault[] {
    return this.recents.filter((r) => !this.hiddenRecents.has(r.root));
  }

  /** Forgets a folder in the list of recent folders. Only the entry goes: the folder and its files are not touched. */
  removeRecent(root: string): void {
    this.hiddenRecents.add(root);
    this.saveHiddenRecents();
  }

  /** A folder opened again after it was removed from the list is back in it. */
  private unhideRecent(root: string): void {
    let changed = false;
    for (const hidden of [...this.hiddenRecents]) {
      if (sameFolder(hidden, root)) {
        this.hiddenRecents.delete(hidden);
        changed = true;
      }
    }
    if (changed) this.saveHiddenRecents();
  }

  private saveHiddenRecents(): void {
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
      this.openError = errorMessage(error);
    }
  }

  async chooseFile(): Promise<void> {
    // in the Notes window a file is opened there: under Files, or as the note it is
    if (this.screen === "notes") return notes.chooseFile();
    try {
      const path = await pickFile();
      if (path) await this.openProject(path);
    } catch (error) {
      this.openError = errorMessage(error);
    }
  }

  /**
   * A file is not a project. Inside the folder this window shows it opens here; anywhere else it is a loose file of the Notes
   * window, which comes forward with it (and starts when it is not running). This window stays as it is.
   */
  private async openFile(path: string, inProject: string | null): Promise<boolean> {
    if (inProject !== null && this.screen === "project") return this.openDoc(inProject);
    // from the welcome screen the window turns into the Notes window, with the file open in it
    if (this.canBecomeNotes()) {
      return this.becomeNotes({ newNote: false, open: null, openFile: path }, (error) =>
        t("welcome.openFailed", { path, message: errorMessage(error) }),
      );
    }
    try {
      await api.showNotes(false, null, path);
      return true;
    } catch (error) {
      const message = t("welcome.openFailed", { path, message: errorMessage(error) });
      if (this.screen === "project") toast.message(message, "warn");
      else this.openError = message;
      return false;
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

  /** Opens a folder as the project of this window (replacing the current one); a file is shown where files are (see `openFile`). */
  async openProject(path: string): Promise<boolean> {
    if (this.opening) return false;
    const where = await api.classifyPath(path).catch(() => null);
    if (where?.kind === "file") return this.openFile(path, where.inProject);
    // unsaved edits are asked about first; the answer carries on with this
    if (this.screen === "project" && edit.holdNavigation(() => void this.openProject(path))) {
      return false;
    }
    this.opening = true;
    this.openError = null;
    try {
      await session.flush();
      await review.releaseAll();
      let info: VaultInfo;
      try {
        info = await api.openVault(path);
      } catch (error) {
        const message = t("welcome.openFailed", { path, message: errorMessage(error) });
        this.openError = message;
        // a recent folder that does not open is dimmed in the lists (the welcome screen, the folder switcher)
        this.missingRecents.add(path);
        if (this.screen === "project") toast.message(message, "warn");
        return false;
      }

      this.forgetProject();
      this.unhideRecent(info.root);
      this.missingRecents.delete(path);
      this.missingRecents.delete(info.root);
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
        void tags.refreshStyles();
        void vault.refreshImageRefs();
        void unread.refresh();
        void comments.refresh();
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
      void agent.onProjectOpened();
      // a place an agent asked for while PerMark was starting
      void this.openFromAgent();
      return true;
    } catch (error) {
      this.openError = t("welcome.openFailed", { path, message: errorMessage(error) });
      return false;
    } finally {
      this.opening = false;
    }
  }

  /** Lets go of everything the window knew about the project it showed (a new project is opened, or none is shown any more). */
  private forgetProject(): void {
    session.suspend();
    agent.reset();
    edit.reset();
    docs.reset();
    feed.clear();
    fileops.reset();
    filter.reset();
    outline.reset();
    search.reset();
    tags.reset();
    toast.clear();
    comments.reset();
    review.clear();
    unread.reset();
    vault.reset();
    ui.followChip = null;
    ui.find.open = false;
    ui.closeSearch();
    ui.folderSwitcher = null;
    ui.tagEditor = null;
    ui.outlinePopover = false;
    ui.changesNotify = false;
    ui.rightTabBack = "changes";
    ui.sidebarView = "explorer";
    ui.dismissedRepoState = null;
    ui.newFile = null;
    ui.treeCursor = null;
  }

  // ---- the folder switcher -----------------------------------------------------------------------

  /** Reads the list of recent folders again: the folder this window shows was added to it when it opened. */
  async refreshRecents(): Promise<void> {
    try {
      this.recents = await api.recentVaults();
    } catch {
      // the list that is known stays
    }
  }

  /**
   * Opens the folder switcher (the menu on the project's name), or closes it when it is open. It is placed under `anchor`, else
   * under the project's name in the Explorer, else near the top left of the window when the Explorer is not on screen.
   */
  async openFolderSwitcher(anchor?: Anchor): Promise<void> {
    if (this.screen !== "project" || vault.isNotes || vault.isSimple) return;
    if (ui.folderSwitcher) {
      ui.folderSwitcher = null;
      return;
    }
    await this.refreshRecents();
    // the window may have changed while the list was read
    if (this.screen === "project") ui.folderSwitcher = { anchor: anchor ?? switcherAnchor() };
  }

  /**
   * Shows another folder in this window. The project that is open is replaced the way it is by "Open folder": unsaved edits are
   * asked about first (and nothing is lost), and a folder that cannot be opened leaves the open one as it is.
   */
  async switchFolder(root: string): Promise<boolean> {
    if (this.screen !== "project" || sameFolder(root, vault.info?.root)) return false;
    return this.openProject(root);
  }

  /**
   * Closes the folder this window shows and goes back to the welcome screen. Unsaved edits are asked about first, as when another
   * folder is opened. Nothing on disk is touched: the folder stays in the list of recent folders.
   */
  async closeFolder(): Promise<void> {
    if (this.screen !== "project" || this.opening) return;
    if (edit.holdNavigation(() => void this.closeFolder())) return;
    this.opening = true;
    try {
      await session.flush();
      await review.releaseAll();
      try {
        await api.closeVault();
      } catch (error) {
        toast.message(errorMessage(error), "warn");
        return;
      }
      // the welcome screen is ready before it shows, and the project's views are gone the moment it does
      await this.loadWelcome();
      this.forgetProject();
      this.openError = null;
      this.screen = "welcome";
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
      await api.on("unread-changed", (paths) => {
        if (this.screen === "project" && !vault.isSimple) unread.onChanged(paths);
      }),
      await api.on("review-changed", (entries) => review.setEntries(entries)),
      await api.on("comments-changed", () => {
        if (this.screen === "project" && !vault.isSimple) comments.onChanged();
      }),
      await api.on("open-path", (path) => void this.openProject(path)),
      await api.on("show-in-project", (path) => {
        if (this.screen === "project" && !vault.isSimple) void this.openDoc(path);
        else if (this.screen === "notes") void notes.openNote(path);
      }),
      await api.on("agent-status", (status) => {
        if (this.screen === "project") agent.onStatus(status);
      }),
      await api.on("agent-open", () => void this.openFromAgent()),
      await api.on("notes-request", () => {
        if (this.screen === "notes")
          void api.takeNotesRequest().then((r) => notes.handleRequest(r));
      }),
    );
  }

  // ---- agent connection -----------------------------------------------------------------------------

  /**
   * An agent asked to show a place. It is shown without taking focus from the agent's terminal: the document opens,
   * the section is tinted, and a notice offers the way back. A person who is editing is not taken away from their
   * work: the notice offers the place instead.
   */
  async openFromAgent(): Promise<void> {
    if (this.screen !== "project" || vault.isSimple) return;
    let place: AgentOpen | null;
    try {
      place = await api.takeAgentOpen();
    } catch {
      return;
    }
    if (place) await this.showAgentPlace(place);
  }

  private async showAgentPlace(place: AgentOpen): Promise<void> {
    if (edit.active) {
      toast.message(t("agent.openWhileEditing", { path: place.path }), "info", {
        label: t("agent.show"),
        run: () => void this.leaveEditingForAgent(place),
      });
      return;
    }
    const from = docs.path;
    const locate = place.line === undefined ? null : { line: place.line, find: null };
    const opened = await this.openDoc(place.path, { locate });
    if (!opened) return;
    // "Back" goes to the document the person was reading; moving within one has nowhere to go back to
    const back =
      from !== null && from !== place.path
        ? { label: t("agent.back"), run: () => void docs.back() }
        : undefined;
    toast.message(
      place.headingNotFound ? t("agent.openedNoHeading") : t("agent.opened"),
      "info",
      back,
    );
  }

  /** The person chose to see what an agent wanted to show them while they were editing. */
  private async leaveEditingForAgent(place: AgentOpen): Promise<void> {
    await this.openDoc(place.path, {
      locate: place.line === undefined ? null : { line: place.line, find: null },
    });
  }

  /** "Copy agent setup": the line every MCP client takes, with the program's own path. */
  async copyAgentSetup(): Promise<void> {
    const copied = await copyText(mcpJson(agent.status.program));
    toast.message(copied ? t("agent.setupCopied") : t("copy.failed"), copied ? "info" : "warn");
  }

  private onTreeChanged(): void {
    if (this.screen === "notes") {
      notes.folderChanged();
      return;
    }
    if (this.treeTimer !== null) clearTimeout(this.treeTimer);
    this.treeTimer = setTimeout(() => void vault.refreshTree(), TREE_REFRESH_MS);
  }

  private scheduleSinceRefresh(): void {
    if (this.sinceTimer !== null) clearTimeout(this.sinceTimer);
    this.sinceTimer = setTimeout(() => void vault.refreshSince(), SINCE_REFRESH_MS);
  }

  private onFsChange(batch: FsBatch): void {
    if (this.screen === "notes") {
      // another program wrote in the notes folder: the note on screen is looked at, and the list is asked for again
      edit.onFsChange(batch);
      docs.handleBatch(batch, false);
      notes.folderChanged();
      return;
    }
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
    brokenLinks.filesChanged();
    comments.filesChanged(batch);

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
    if (docs.path !== null && batchTouches(batch, docs.path)) unread.contentChanged(docs.path);

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
    // in the Notes window a document is a note: leaving the one that is open and entering the editor is the notes' to do
    if (this.screen === "notes") return notes.openNote(path);
    return docs.open(path, opts);
  }

  /** Opens a document because it changed (feed row, toast, Follow): marks what is new in it. */
  openChanged(path: string): Promise<boolean> {
    return this.openDoc(path, { fromChange: true });
  }

  // ---- comments -----------------------------------------------------------------------------------

  /** Shows the Comments tab of the right panel, opening the panel when it is collapsed. */
  showComments(): void {
    if (vault.isSimple) return;
    if (!session.right.open) session.setPanel("right", { open: true });
    ui.rightNotify = false;
    this.showRightTab("comments");
  }

  /** A bar in the margin was clicked: the Comments tab shows the comment. */
  showComment(id: string): void {
    this.showComments();
    comments.show(id);
  }

  /** Opens the document a comment is on at its passage (the section it points to when the passage changed). */
  async openComment(comment: Comment): Promise<void> {
    if (comment.state === "documentGone") {
      toast.message(t("comments.jump.gone", { path: comment.path }), "info");
      return;
    }
    await this.openDoc(comment.path, { locate: { line: comment.line, find: null } });
  }

  /** Opens the file at the section a search hit is in, marking the words that were searched for. */
  async openHit(hit: SearchHit, query = search.answered || search.query.trim()): Promise<void> {
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
      toast.message(errorMessage(error), "warn");
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
          toast.message(t("link.openFailed", { message: errorMessage(error) }), "warn");
        }
        return null;
      case "anchor":
        docs.jumpTo(target.anchor);
        return null;
      case "missing":
        return { missing: target.path };
    }
  }

  // ---- unread ----------------------------------------------------------------------------------------

  /** Alt+U: opens the next unread document (the live feed first, newest first, then the tree), or says there is none. */
  async openNextUnread(): Promise<void> {
    const next = nextUnread({
      unread: unread.set,
      feed: feed.entries.flatMap((entry) => (entry.type === "file" ? [entry.path] : [])),
      tree: vault.files,
      current: docs.path,
    });
    if (next === null) {
      toast.message(t("unread.allCaughtUp"), "info");
      return;
    }
    await this.openDoc(next);
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
      return errorKind(error) === "exists" ? t("newFile.exists") : errorMessage(error);
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
      toast.message(errorMessage(error), "warn");
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

  showLinks(): void {
    if (vault.isSimple) return;
    ui.sidebarView = "links";
    if (!session.left.open) session.setPanel("left", { open: true });
    ui.linksFocusTick++;
    void brokenLinks.refresh();
  }

  /** A tag chip was clicked: the Tags view, showing the files with that tag. */
  showTagsFor(tag: string): void {
    if (vault.isSimple) return;
    tags.filter = "";
    tags.select(selectOnly(tag));
    this.showTags();
  }

  /** Activity bar: switch the sidebar view, or collapse the sidebar when its view is already showing. */
  clickView(view: "explorer" | "search" | "tags" | "links"): void {
    if (ui.sidebarView === view && session.left.open) {
      session.setPanel("left", { open: false });
      return;
    }
    if (view === "search") this.showSearch();
    else if (view === "tags") this.showTags();
    else if (view === "links") this.showLinks();
    else this.showExplorer();
  }

  revealInTree(path: string): void {
    if (vault.isSimple) return;
    ui.sidebarView = "explorer";
    if (!session.left.open) session.setPanel("left", { open: true });
    vault.revealPath(path);
  }

  /**
   * A folder found in Quick open or Search everywhere: the Explorer opens the folders above it, scrolls to it, opens it and
   * takes the keyboard focus, so the arrow keys go on from there.
   */
  revealFolder(path: string): void {
    if (vault.isSimple) return;
    this.showExplorer();
    vault.revealPath(path);
    vault.expanded.add(path);
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

  /** Copies the reference to a file, with a line (`@src/main.rs:42`) when the reference has one. */
  async copyReference(
    path = docs.path,
    flashId = "reference",
    line: number | null = null,
  ): Promise<void> {
    if (!path) return;
    const text = formatReference(
      session.refTemplate,
      refLabel(await this.referencePath(path), line),
    );
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
      toast.message(errorMessage(error), "warn");
    }
  }

  async copy(text: string, flashId: string): Promise<void> {
    const ok = await copyText(text);
    if (ok) ui.flash(flashId);
    else toast.message(t("copy.failed"), "warn");
  }

  /**
   * "Open in external editor" (Ctrl+Shift+E, the menus): the file opens in the editor of the settings. For the open document
   * the line is the block at the top of the view, so the editor lands where the reader is.
   */
  async openExternally(path = docs.path): Promise<void> {
    if (!path) return;
    await this.openInEditor(path, path === docs.path ? this.topLine() : undefined);
  }

  /** The 1-based source line of the block at the top of the open document's view (the editor's, while editing). */
  private topLine(): number | undefined {
    const line = bridge.editor ? bridge.editor.topLine() + 1 : bridge.topLine?.();
    return line ?? undefined;
  }

  /**
   * Runs the editor command of the settings for a file, at a line and column (from 1). Images and PDF files go to the
   * system's application, which shows them better than a code editor does. A failure is a toast, with a way to Settings.
   */
  async openInEditor(path: string, line?: number, col?: number): Promise<void> {
    try {
      if (usesSystemApp(path)) await api.openExternal(path);
      else await api.openInEditor(path, line, col);
    } catch (error) {
      await this.editorFailed(path, error);
    }
  }

  private async editorFailed(path: string, error: unknown): Promise<void> {
    const settings = {
      label: t("settings.title"),
      run: () => {
        ui.settingsOpen = true;
      },
    };
    if (errorKind(error) === "editor") {
      toast.message(errorMessage(error), "warn", settings);
      return;
    }
    // The system's default app only opens documents and images: for a code file the way out is an editor command.
    if (errorKind(error) === "invalidPath") {
      await editorCommand.load();
      if (editorCommand.command.trim() === "") {
        toast.message(t("codeRef.systemCantOpen", { name: basename(path) }), "warn", settings);
        return;
      }
    }
    toast.message(t("doc.externalFailed", { message: errorMessage(error) }), "warn");
  }

  /**
   * Follows a code reference. A Markdown file opens here, at the line; any other file opens in the editor, and so does a
   * Markdown file when `external` is set (Ctrl+click).
   */
  async openCodeRef(
    path: string,
    line: number | null,
    col: number | null,
    external = false,
  ): Promise<void> {
    if (!external && isMarkdown(path)) {
      await this.openDoc(path, line === null ? {} : { locate: { line, find: null } });
      return;
    }
    await this.openInEditor(path, line ?? undefined, col ?? undefined);
  }

  /**
   * "Open with default app": the file goes to the program the system has for its type, whatever the file is. A program,
   * script, installer or shortcut is refused by the backend; the message names its kind and offers to show the file in
   * the file manager instead.
   */
  async openWithDefaultApp(path = docs.path): Promise<void> {
    if (!path) return;
    try {
      await api.openWithDefaultApp(path);
    } catch (error) {
      if (errorKind(error) === "blockedType") {
        toast.message(errorMessage(error), "warn", {
          label: t("menu.showInFileManager"),
          run: () => void this.revealInFolder(path),
        });
        return;
      }
      toast.message(t("doc.externalFailed", { message: errorMessage(error) }), "warn");
    }
  }

  /** The web page that "Open in browser" acts on from the command palette: the open document, else the file the Explorer is on. */
  get browserPage(): string | null {
    if (docs.path && isWebPage(docs.path)) return docs.path;
    const cursor = ui.treeCursor;
    return cursor && !cursor.isDir && isWebPage(cursor.path) ? cursor.path : null;
  }

  /** "Open in browser": a web page of the folder goes to the system's browser. A failure is a toast. */
  async openInBrowser(path: string | null = this.browserPage): Promise<void> {
    if (!path) return;
    try {
      await api.openInBrowser(path);
    } catch (error) {
      toast.message(t("doc.externalFailed", { message: errorMessage(error) }), "warn");
    }
  }

  /** Ctrl+= and Ctrl+-: the zoom of the picture on show, else of the text. */
  zoomBy(direction: 1 | -1): void {
    if (docs.current?.kind === "image") media.zoomBy(direction);
    else session.zoomBy(direction);
  }

  /** Ctrl+0: a picture goes back to fitting the pane, the text to its usual size. */
  resetZoom(): void {
    if (docs.current?.kind === "image") media.reset();
    else session.setZoom(1);
  }

  async revealInFolder(path: string): Promise<void> {
    try {
      await api.revealInFolder(path);
    } catch (error) {
      toast.message(t("doc.externalFailed", { message: errorMessage(error) }), "warn");
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

  /** Runs an action of the keyboard map, or a command of Search everywhere that has no key (`commands.ts`). */
  run(action: CommandAction): void {
    if (this.screen === "welcome") {
      if (action === "openFolder") void this.chooseFolder();
      else if (action === "openFile") void this.chooseFile();
      else if (action === "newNote") void this.newNote();
      else if (action === "showNotes") void this.showNotes();
      return;
    }
    if (this.screen === "notes") return this.runInNotes(action);
    if (this.screen !== "project") return;
    const ide = !vault.isSimple;
    switch (action) {
      case "openFolder":
        return void this.chooseFolder();
      case "openFile":
        return void this.chooseFile();
      case "switchFolder":
        if (ide) void this.openFolderSwitcher();
        return;
      case "closeFolder":
        if (ide) void this.closeFolder();
        return;
      case "searchEverywhere":
        if (ide) this.openSearchBox();
        return;
      case "quickOpen":
        if (ide) this.openSearchBox("files");
        return;
      case "commands":
        // the box takes the selection away: a comment asked for from it is on what was selected a moment ago
        comments.remember();
        ui.openSearch("commands");
        return;
      case "searchAll":
        return this.showSearch();
      case "explorer":
        return this.showExplorer();
      case "tagsView":
        return this.showTags();
      case "brokenLinks":
        return this.showLinks();
      case "editTags":
        if (ide && docs.current && !edit.active) ui.openTagEditor();
        return;
      case "addComment":
        if (ide && !edit.active) comments.begin();
        return;
      case "showComments":
        if (ide) this.showComments();
        return;
      case "reviewChanges":
        if (ide && !edit.active) review.start();
        return;
      case "reviewKeepAll":
        if (ide && !edit.active) void review.keepAll();
        return;
      case "reviewRevertAll":
        if (ide && !edit.active) void review.revertAll();
        return;
      case "edit":
        if (docs.current && !edit.active) void edit.enter();
        return;
      case "toggleEdit":
        if (docs.current || edit.active) edit.toggle();
        return;
      case "toggleEditMode":
        if (docs.current || edit.active) void edit.toggleMode();
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
      case "nextUnread":
        if (ide) void this.openNextUnread();
        return;
      case "markAllRead":
        if (ide) void unread.markAllRead();
        return;
      case "showOnlyUnread":
        if (!ide) return;
        this.openExplorerPanel();
        return unread.toggleOnlyUnread();
      case "markFolderRead": {
        if (!ide) return;
        // the folder the tree cursor is in, else the open document's
        const folder = targetFolder(ui.treeCursor, docs.path);
        return void (folder === "" ? unread.markAllRead() : unread.markFolderRead(folder));
      }
      case "markUnread":
        if (ide && docs.path) void unread.markUnread(docs.path, true);
        return;
      case "toggleFollow":
        if (ide) this.toggleFollow();
        return;
      case "newNote":
        return void this.newNote();
      case "showNotes":
        return void this.showNotes();
      case "copyDocument":
        return void this.copyDocument();
      case "copyReference":
        return void this.copyReferenceContextAware();
      case "copyPath":
        return void this.copyPath();
      case "openExternal":
        return void this.openExternally();
      case "openDefaultApp":
        if (ide) void this.openWithDefaultApp();
        return;
      case "openInBrowser":
        return void this.openInBrowser();
      case "trail":
        // the header's buttons are where the menu opens, and the notes have none
        if (this.screen === "project" && (docs.canBack || docs.canForward)) {
          ui.trail = { view: "all", anchor: null };
        }
        return;
      case "toggleLeft":
        if (ide) this.toggleLeft();
        return;
      case "toggleRight":
        if (ide) this.toggleRight();
        return;
      case "zoomIn":
        return this.zoomBy(1);
      case "zoomOut":
        return this.zoomBy(-1);
      case "zoomReset":
        return this.resetZoom();
      case "cycleWidth":
        return session.cycleWidth();
      case "toggleOutline":
        return this.toggleOutline();
      case "settings":
        ui.settingsOpen = true;
        return;
      case "initProject":
        if (ide && vault.stateLocation !== "project") ui.initDialog = true;
        return;
      case "checkUpdates":
        // the Microsoft Store updates the app: there is nothing to check (the command is not offered either)
        if (distribution.isStore) return;
        // the answer ("You're up to date", or the offer) shows in the settings
        ui.settingsOpen = true;
        return void updates.check(true);
      case "newFolder":
        if (!ide) return;
        this.openExplorerPanel();
        return fileops.startNewFolder(targetFolder(ui.treeCursor, docs.path));
      case "collapseAll":
        if (!ide) return;
        this.showExplorer();
        return vault.collapseAll();
      case "revealInTree":
        if (docs.path) this.revealInTree(docs.path);
        return;
      case "baselineLastVisit":
        if (ide) void this.setBaseline("lastVisit");
        return;
      case "baselineCommit":
        if (ide) void this.setBaseline("head");
        return;
      case "openContainingFolder":
        if (!ide) void this.openContainingFolder();
        return;
      case "agentOn":
        if (ide) void agent.setEnabled(true);
        return;
      case "agentOff":
        if (ide) void agent.setEnabled(false);
        return;
      case "agentCopySetup":
        if (ide) void this.copyAgentSetup();
        return;
    }
  }

  /** The keyboard map of the Notes window: the project actions that make no sense there do nothing. */
  private runInNotes(action: CommandAction): void {
    switch (action) {
      case "newNote":
      case "newFile":
        return void notes.newNote();
      case "showNotes":
        return notes.focusList();
      case "copyDocument":
        return void notes.copyNote();
      case "moveToProject":
        if (notes.movable) void notes.moveToProject();
        return;
      case "searchEverywhere":
        return ui.openSearch();
      case "quickOpen":
        return ui.openSearch("files");
      case "searchAll":
        return ui.openSearch("sections");
      case "commands":
        return ui.openSearch("commands");
      case "edit":
        if (docs.current && !edit.active) void edit.enter();
        return;
      case "toggleEdit":
        if (docs.current || edit.active) edit.toggle();
        return;
      case "toggleEditMode":
        if (docs.current || edit.active) void edit.toggleMode();
        return;
      case "save":
        // a note is saved as it is typed: Ctrl+S only makes sure it is written now. A loose file is written only here.
        if (edit.active) void (edit.loose ? edit.save() : edit.flush());
        return;
      case "openFile":
        return void notes.chooseFile();
      case "find":
        if (!edit.active) this.openFind();
        return;
      case "toggleLeft":
        return this.toggleLeft();
      case "zoomIn":
        return this.zoomBy(1);
      case "zoomOut":
        return this.zoomBy(-1);
      case "zoomReset":
        return this.resetZoom();
      case "cycleWidth":
        return session.cycleWidth();
      case "toggleOutline":
        ui.outlinePopover = !ui.outlinePopover;
        return;
      case "settings":
        ui.settingsOpen = true;
        return;
      case "checkUpdates":
        if (distribution.isStore) return;
        ui.settingsOpen = true;
        return void updates.check(true);
      default:
        // git, unread markers, comments, tags, the Explorer and the agent connection are not in this window
        return;
    }
  }

  /** Opens Search everywhere. A comment asked for from the box is on what was selected before it opened. */
  private openSearchBox(scope?: "files"): void {
    comments.remember();
    ui.openSearch(scope);
  }

  /** The Explorer is the sidebar's view and the sidebar is open; the focus stays where it is. */
  private openExplorerPanel(): void {
    ui.sidebarView = "explorer";
    if (!session.left.open) session.setPanel("left", { open: true });
  }
}

export const app = new AppStore();
