// User settings that live in the per-project session (theme, zoom, width, panels, Follow), plus the
// last document and scroll position. Saved through `saveSession` shortly after every change.

import { api } from "../api";
import { DEFAULT_REF_TEMPLATE } from "../reference";
import { DEFAULT_WIDTH, nextWidth, type ReadingWidth } from "../readingWidth";
import { DEFAULT_LINK_UPDATE, mapPath } from "../fileOps";
import { DEFAULT_EDIT_MODE, type EditMode } from "../editMode";
import { DEFAULT_RIGHT_TAB, type RightTab } from "../rightPanel";
import {
  LEFT_DEFAULT,
  RIGHT_DEFAULT,
  clampPanelWidth,
  clampZoom,
  normalizeSession,
  stepZoom,
} from "../session";
import type { LinkUpdate, Move, SessionState, TreeFilter } from "../types";

const SAVE_DELAY_MS = 600;

class SessionStore {
  theme = $state<SessionState["theme"]>("system");
  zoom = $state(1);
  width = $state<ReadingWidth>(DEFAULT_WIDTH);
  refTemplate = $state(DEFAULT_REF_TEMPLATE);
  left = $state({ open: true, width: LEFT_DEFAULT });
  right = $state({ open: true, width: RIGHT_DEFAULT });
  follow = $state(false);
  /** The Explorer filter this person chose for the project; null until they do. */
  treeFilter = $state.raw<TreeFilter | null>(null);
  /** The Explorer selects and scrolls to the document that was just opened. */
  autoReveal = $state(true);
  /** The view the right panel shows. */
  rightTab = $state<RightTab>(DEFAULT_RIGHT_TAB);
  /** Whether moving files in the Explorer rewrites the links to them. */
  linkUpdate = $state<LinkUpdate>(DEFAULT_LINK_UPDATE);
  /** How a document opens for editing (the switch in the header changes the mode of the open document only). */
  editMode = $state<EditMode>(DEFAULT_EDIT_MODE);
  systemDark = $state(false);

  lastDoc: string | null = null;
  scrollTop = 0;

  resolvedTheme = $derived<"light" | "dark">(
    this.theme === "system" ? (this.systemDark ? "dark" : "light") : this.theme,
  );

  private enabled = false;
  private timer: ReturnType<typeof setTimeout> | null = null;

  /** Tracks the OS color scheme for the "system" theme. */
  watchSystemTheme(): void {
    const query = window.matchMedia("(prefers-color-scheme: dark)");
    this.systemDark = query.matches;
    query.addEventListener("change", (e) => {
      this.systemDark = e.matches;
    });
  }

  /** Applies a stored session; saving starts only after this ran for the opened project. */
  load(stored: SessionState | null): SessionState {
    const s = normalizeSession(stored);
    this.theme = s.theme;
    this.zoom = s.zoom;
    this.width = s.width;
    this.refTemplate = s.refTemplate;
    this.left = { ...s.leftPanel };
    this.right = { ...s.rightPanel };
    this.follow = s.follow;
    this.treeFilter = s.treeFilter;
    this.autoReveal = s.autoReveal;
    this.rightTab = s.rightTab;
    this.linkUpdate = s.linkUpdate;
    this.editMode = s.editMode;
    this.lastDoc = s.lastDoc;
    this.scrollTop = s.scrollTop;
    this.enabled = true;
    return s;
  }

  snapshot(): SessionState {
    return {
      lastDoc: this.lastDoc,
      scrollTop: Math.round(this.scrollTop),
      zoom: this.zoom,
      width: this.width,
      refTemplate: this.refTemplate,
      leftPanel: { open: this.left.open, width: this.left.width },
      rightPanel: { open: this.right.open, width: this.right.width },
      theme: this.theme,
      follow: this.follow,
      treeFilter: this.treeFilter,
      autoReveal: this.autoReveal,
      rightTab: this.rightTab,
      linkUpdate: this.linkUpdate,
      editMode: this.editMode,
    };
  }

  persist(delay = SAVE_DELAY_MS): void {
    if (!this.enabled) return;
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = setTimeout(() => void this.flush(), delay);
  }

  async flush(): Promise<void> {
    if (!this.enabled) return;
    if (this.timer !== null) {
      clearTimeout(this.timer);
      this.timer = null;
    }
    try {
      await api.saveSession(this.snapshot());
    } catch {
      // the session is a convenience; failing to save must never interrupt reading
    }
  }

  /** Stops saving (the project is being closed or replaced). */
  suspend(): void {
    this.enabled = false;
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
  }

  noteDoc(path: string | null): void {
    this.lastDoc = path;
    this.scrollTop = 0;
    this.persist();
  }

  /** Files or folders were moved or renamed: the document to reopen next time is where it is now. */
  followMoves(moves: readonly Move[]): void {
    if (this.lastDoc === null) return;
    this.lastDoc = mapPath(this.lastDoc, moves);
    this.persist();
  }

  noteScroll(top: number): void {
    this.scrollTop = top;
    this.persist(1500);
  }

  setEditMode(mode: EditMode): void {
    this.editMode = mode;
    this.persist();
  }

  setTheme(theme: SessionState["theme"]): void {
    this.theme = theme;
    this.persist();
  }

  cycleTheme(): void {
    const order: SessionState["theme"][] = ["system", "light", "dark"];
    this.setTheme(order[(order.indexOf(this.theme) + 1) % order.length]);
  }

  setZoom(zoom: number): void {
    this.zoom = clampZoom(zoom);
    this.persist();
  }

  zoomBy(direction: 1 | -1): void {
    this.setZoom(stepZoom(this.zoom, direction));
  }

  setWidth(width: ReadingWidth): void {
    this.width = width;
    this.persist();
  }

  cycleWidth(): void {
    this.setWidth(nextWidth(this.width));
  }

  setRefTemplate(template: string): void {
    this.refTemplate = template;
    this.persist();
  }

  toggleFollow(): void {
    this.follow = !this.follow;
    this.persist();
  }

  setTreeFilter(filter: TreeFilter): void {
    this.treeFilter = filter;
    this.persist();
  }

  toggleAutoReveal(): void {
    this.autoReveal = !this.autoReveal;
    this.persist();
  }

  setLinkUpdate(value: LinkUpdate): void {
    this.linkUpdate = value;
    this.persist();
  }

  setRightTab(tab: RightTab): void {
    this.rightTab = tab;
    this.persist();
  }

  setPanel(side: "left" | "right", patch: { open?: boolean; width?: number }): void {
    const panel = side === "left" ? this.left : this.right;
    if (patch.open !== undefined) panel.open = patch.open;
    if (patch.width !== undefined) panel.width = clampPanelWidth(patch.width);
    this.persist();
  }
}

export const session = new SessionStore();
