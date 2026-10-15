// Defaults and sanitizing for `SessionState` (panel widths, zoom, theme, ...).

import { DEFAULT_REF_TEMPLATE } from "./reference";
import { DEFAULT_WIDTH, normalizeWidth } from "./readingWidth";
import { DEFAULT_EDIT_MODE, normalizeEditMode } from "./editMode";
import { DEFAULT_RIGHT_TAB, normalizeRightTab } from "./rightPanel";
import { DEFAULT_LINK_UPDATE, normalizeLinkUpdate } from "./fileOps";
import { normalizeTreeFilter } from "./treeFilter";
import type { SessionState } from "./types";

export const PANEL_MIN = 200;
export const PANEL_MAX = 480;
export const LEFT_DEFAULT = 260;
export const RIGHT_DEFAULT = 280;
export const ZOOM_MIN = 0.6;
export const ZOOM_MAX = 2.4;
export const ZOOM_STEP = 0.1;

export function clampPanelWidth(width: number): number {
  if (!Number.isFinite(width)) return PANEL_MIN;
  return Math.round(Math.min(PANEL_MAX, Math.max(PANEL_MIN, width)));
}

export function clampZoom(zoom: number): number {
  if (!Number.isFinite(zoom)) return 1;
  return Math.round(Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, zoom)) * 100) / 100;
}

/** One zoom step in or out (rounded to avoid 1.0000000002 drift). */
export function stepZoom(zoom: number, direction: 1 | -1): number {
  return clampZoom(Math.round((zoom + direction * ZOOM_STEP) * 100) / 100);
}

export function defaultSession(): SessionState {
  return {
    lastDoc: null,
    scrollTop: 0,
    zoom: 1,
    width: DEFAULT_WIDTH,
    refTemplate: DEFAULT_REF_TEMPLATE,
    leftPanel: { open: true, width: LEFT_DEFAULT },
    rightPanel: { open: true, width: RIGHT_DEFAULT },
    theme: "system",
    follow: false,
    treeFilter: null,
    autoReveal: true,
    rightTab: DEFAULT_RIGHT_TAB,
    linkUpdate: DEFAULT_LINK_UPDATE,
    editMode: DEFAULT_EDIT_MODE,
    initHintNever: false,
    agentConnection: false,
  };
}

/** Fills missing fields and clamps out-of-range values from a stored (possibly older) session. */
export function normalizeSession(stored: Partial<SessionState> | null | undefined): SessionState {
  const base = defaultSession();
  if (!stored) return base;
  const theme =
    stored.theme === "light" || stored.theme === "dark" || stored.theme === "system"
      ? stored.theme
      : base.theme;
  return {
    lastDoc: typeof stored.lastDoc === "string" ? stored.lastDoc : null,
    scrollTop: Number.isFinite(stored.scrollTop) ? Math.max(0, stored.scrollTop as number) : 0,
    zoom: clampZoom(stored.zoom ?? 1),
    width: normalizeWidth(stored.width, stored.wide),
    refTemplate:
      typeof stored.refTemplate === "string" && stored.refTemplate.trim() !== ""
        ? stored.refTemplate
        : base.refTemplate,
    leftPanel: {
      open: stored.leftPanel?.open ?? true,
      width: clampPanelWidth(stored.leftPanel?.width ?? LEFT_DEFAULT),
    },
    rightPanel: {
      open: stored.rightPanel?.open ?? true,
      width: clampPanelWidth(stored.rightPanel?.width ?? RIGHT_DEFAULT),
    },
    theme,
    follow: stored.follow === true,
    treeFilter: normalizeTreeFilter(stored.treeFilter),
    autoReveal: stored.autoReveal !== false,
    rightTab: normalizeRightTab(stored.rightTab),
    linkUpdate: normalizeLinkUpdate(stored.linkUpdate),
    editMode: normalizeEditMode(stored.editMode),
    initHintNever: stored.initHintNever === true,
    agentConnection: stored.agentConnection === true,
  };
}
