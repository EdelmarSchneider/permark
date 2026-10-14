import { describe, expect, it } from "vitest";
import {
  PANEL_MAX,
  PANEL_MIN,
  ZOOM_MAX,
  ZOOM_MIN,
  clampPanelWidth,
  clampZoom,
  defaultSession,
  normalizeSession,
  stepZoom,
} from "./session";

describe("clampPanelWidth", () => {
  it("keeps panel widths between 200 and 480 px", () => {
    expect(clampPanelWidth(100)).toBe(PANEL_MIN);
    expect(clampPanelWidth(900)).toBe(PANEL_MAX);
    expect(clampPanelWidth(301.6)).toBe(302);
    expect(clampPanelWidth(Number.NaN)).toBe(PANEL_MIN);
  });
});

describe("zoom", () => {
  it("steps by 10% without float drift", () => {
    let z = 1;
    for (let i = 0; i < 3; i++) z = stepZoom(z, 1);
    expect(z).toBe(1.3);
    expect(stepZoom(1.3, -1)).toBe(1.2);
  });
  it("clamps to the supported range", () => {
    expect(stepZoom(ZOOM_MAX, 1)).toBe(ZOOM_MAX);
    expect(stepZoom(ZOOM_MIN, -1)).toBe(ZOOM_MIN);
    expect(clampZoom(Number.NaN)).toBe(1);
  });
});

describe("normalizeSession", () => {
  it("returns the defaults when nothing was stored", () => {
    expect(normalizeSession(null)).toEqual(defaultSession());
  });

  it("clamps and fills a partial or corrupted session", () => {
    const s = normalizeSession({
      lastDoc: "docs/a.md",
      scrollTop: -20,
      zoom: 9,
      leftPanel: { open: false, width: 40 },
      theme: "purple" as never,
      refTemplate: "  ",
    });
    expect(s.lastDoc).toBe("docs/a.md");
    expect(s.scrollTop).toBe(0);
    expect(s.zoom).toBe(ZOOM_MAX);
    expect(s.leftPanel).toEqual({ open: false, width: PANEL_MIN });
    expect(s.rightPanel.open).toBe(true);
    expect(s.theme).toBe("system");
    expect(s.refTemplate).toBe("@{path}");
  });

  it("keeps the Explorer filter only when it names a known preset", () => {
    expect(normalizeSession({}).treeFilter).toBeNull();
    expect(normalizeSession({ treeFilter: null }).treeFilter).toBeNull();
    expect(normalizeSession({ treeFilter: { show: "nope" } as never }).treeFilter).toBeNull();
    const filter = {
      show: "custom" as const,
      categories: ["data" as const],
      patterns: "+*.sql",
      hideEmptyFolders: false,
    };
    expect(normalizeSession({ treeFilter: filter }).treeFilter).toEqual(filter);
  });

  it("keeps valid values", () => {
    const stored = {
      ...defaultSession(),
      lastDoc: "x.md",
      scrollTop: 320,
      zoom: 1.2,
      wide: true,
      follow: true,
      theme: "dark" as const,
      refTemplate: "see {path}",
    };
    expect(normalizeSession(stored)).toEqual(stored);
  });
});
