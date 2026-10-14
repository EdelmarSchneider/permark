// User settings that live in the per-project session (theme, zoom, width, panels, Follow), plus the
// last document and scroll position. Saved through `saveSession` shortly after every change.

import { api } from "../api";
import { DEFAULT_REF_TEMPLATE } from "../reference";
import {
  LEFT_DEFAULT,
  RIGHT_DEFAULT,
  clampPanelWidth,
  clampZoom,
  normalizeSession,
  stepZoom,
} from "../session";
import type { SessionState, TreeFilter } from "../types";

const SAVE_DELAY_MS = 600;

class SessionStore {
  theme = $state<SessionState["theme"]>("system");
  zoom = $state(1);
  wide = $state(false);
  refTemplate = $state(DEFAULT_REF_TEMPLATE);
  left = $state({ open: true, width: LEFT_DEFAULT });
  right = $state({ open: true, width: RIGHT_DEFAULT });
  follow = $state(false);
  /** The Explorer filter this person chose for the project; null until they do. */
  treeFilter = $state.raw<TreeFilter | null>(null);
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
    this.wide = s.wide;
    this.refTemplate = s.refTemplate;
    this.left = { ...s.leftPanel };
    this.right = { ...s.rightPanel };
    this.follow = s.follow;
    this.treeFilter = s.treeFilter;
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
      wide: this.wide,
      refTemplate: this.refTemplate,
      leftPanel: { open: this.left.open, width: this.left.width },
      rightPanel: { open: this.right.open, width: this.right.width },
      theme: this.theme,
      follow: this.follow,
      treeFilter: this.treeFilter,
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

  noteScroll(top: number): void {
    this.scrollTop = top;
    this.persist(1500);
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

  toggleWide(): void {
    this.wide = !this.wide;
    this.persist();
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

  setPanel(side: "left" | "right", patch: { open?: boolean; width?: number }): void {
    const panel = side === "left" ? this.left : this.right;
    if (patch.open !== undefined) panel.open = patch.open;
    if (patch.width !== undefined) panel.width = clampPanelWidth(patch.width);
    this.persist();
  }
}

export const session = new SessionStore();
