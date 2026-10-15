import { describe, expect, it } from "vitest";
import {
  buildContext,
  headingAtLine,
  headingPath,
  sameContext,
  sectionAt,
  type Heading,
  type View,
} from "./agentContext";

// # Plan (1) / ## Intro (3) / ## Rollout (10) / ### Week 1 (14) / ### Week 2 (20) / # Appendix (30)
const headings: Heading[] = [
  { level: 1, text: "Plan", line: 1 },
  { level: 2, text: "Intro", line: 3 },
  { level: 2, text: "Rollout", line: 10 },
  { level: 3, text: "Week 1", line: 14 },
  { level: 3, text: "Week 2", line: 20 },
  { level: 1, text: "Appendix", line: 30 },
];

function view(over: Partial<View> = {}): View {
  return {
    document: "docs/plan.md",
    mode: "reading",
    headings,
    lastLine: 40,
    current: 3,
    selection: null,
    unsaved: false,
    ...over,
  };
}

describe("headingPath", () => {
  it("lists the headings a section sits under, then its own", () => {
    expect(headingPath(headings, 3)).toEqual(["Plan", "Rollout", "Week 1"]);
    expect(headingPath(headings, 2)).toEqual(["Plan", "Rollout"]);
    expect(headingPath(headings, 0)).toEqual(["Plan"]);
  });

  it("does not take a sibling for a parent", () => {
    expect(headingPath(headings, 4)).toEqual(["Plan", "Rollout", "Week 2"]);
    expect(headingPath(headings, 5)).toEqual(["Appendix"]);
  });

  it("copes with a document that starts below the top level and skips levels", () => {
    const odd: Heading[] = [
      { level: 3, text: "Deep", line: 1 },
      { level: 1, text: "Top", line: 5 },
      { level: 3, text: "Skipped", line: 8 },
    ];
    expect(headingPath(odd, 0)).toEqual(["Deep"]);
    expect(headingPath(odd, 2)).toEqual(["Top", "Skipped"]);
  });

  it("is empty for a heading that does not exist", () => {
    expect(headingPath(headings, 99)).toEqual([]);
    expect(headingPath([], 0)).toEqual([]);
  });
});

describe("headingAtLine", () => {
  it("finds the last heading at or above the line", () => {
    expect(headingAtLine(headings, 1)).toBe(0);
    expect(headingAtLine(headings, 9)).toBe(1);
    expect(headingAtLine(headings, 10)).toBe(2);
    expect(headingAtLine(headings, 25)).toBe(4);
    expect(headingAtLine(headings, 400)).toBe(5);
  });

  it("is null above the first heading", () => {
    expect(headingAtLine([{ level: 1, text: "A", line: 5 }], 3)).toBeNull();
    expect(headingAtLine([], 3)).toBeNull();
  });
});

describe("sectionAt", () => {
  it("runs from the heading to the line before the next one, of any level", () => {
    expect(sectionAt(headings, 2, 40)).toEqual({
      headingPath: ["Plan", "Rollout"],
      startLine: 10,
      endLine: 13,
    });
  });

  it("runs the last section to the end of the document", () => {
    expect(sectionAt(headings, 5, 40)).toEqual({
      headingPath: ["Appendix"],
      startLine: 30,
      endLine: 40,
    });
  });

  it("is at least the heading's own line", () => {
    expect(sectionAt(headings, 5, 10)?.endLine).toBe(30);
    expect(sectionAt(headings, 99, 40)).toBeNull();
  });
});

describe("buildContext", () => {
  it("shares nothing without a document", () => {
    expect(buildContext(view({ document: null, selection: { text: "x" }, unsaved: true }))).toEqual(
      { document: null, mode: null, section: null, selection: null, unsaved: false },
    );
  });

  it("names the document, the mode and the section being read", () => {
    expect(buildContext(view())).toEqual({
      document: "docs/plan.md",
      mode: "reading",
      section: { headingPath: ["Plan", "Rollout", "Week 1"], startLine: 14, endLine: 19 },
      selection: null,
      unsaved: false,
    });
  });

  it("has no section above the first heading", () => {
    expect(buildContext(view({ current: null })).section).toBeNull();
    expect(buildContext(view({ headings: [], current: null })).section).toBeNull();
  });

  it("reports editing and unsaved changes", () => {
    const context = buildContext(view({ mode: "source", unsaved: true }));
    expect(context.mode).toBe("source");
    expect(context.unsaved).toBe(true);
  });

  it("places a selection in the section its first line is in", () => {
    const context = buildContext(
      view({ selection: { text: "ship it", startLine: 22, endLine: 24 } }),
    );
    expect(context.selection).toEqual({
      text: "ship it",
      headingPath: ["Plan", "Rollout", "Week 2"],
      startLine: 22,
      endLine: 24,
    });
    // while the section on screen is another one
    expect(context.section?.headingPath).toEqual(["Plan", "Rollout", "Week 1"]);
  });

  it("falls back to the section on screen when the view cannot tell the lines", () => {
    const context = buildContext(view({ selection: { text: "ship it" } }));
    expect(context.selection).toEqual({
      text: "ship it",
      headingPath: ["Plan", "Rollout", "Week 1"],
    });
    expect(context.selection).not.toHaveProperty("startLine");
  });

  it("does not make up an end line", () => {
    const context = buildContext(view({ selection: { text: "x", startLine: 12 } }));
    expect(context.selection).toMatchObject({ startLine: 12 });
    expect(context.selection).not.toHaveProperty("endLine");
  });

  it("shares no selection that is only blanks", () => {
    expect(buildContext(view({ selection: { text: " \n " } })).selection).toBeNull();
  });
});

describe("sameContext", () => {
  it("tells a repeat from a change", () => {
    const a = buildContext(view());
    expect(sameContext(a, buildContext(view()))).toBe(true);
    expect(sameContext(a, buildContext(view({ unsaved: true })))).toBe(false);
    expect(sameContext(a, buildContext(view({ current: 4 })))).toBe(false);
    expect(sameContext(null, a)).toBe(false);
    expect(sameContext(null, null)).toBe(false);
  });
});
