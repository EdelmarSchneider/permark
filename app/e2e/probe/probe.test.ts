// The probe's pure parts: scenario + command line parsing, text diff, effect, flags, expectations, thumb pixels.
// The browser half is proven by the calibration fixtures (`npm run ux:calibrate`).

import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { describe, expect, it } from "vitest";
import { commitAt, parseFixture } from "../calibration/fixture.ts";
import { effectOf, flagsOf, keyNames, promisedBy, textDiff, unmet, type Change } from "./facts.ts";
import { parseOptions, scenarioFrom } from "./options.ts";
import type { FocusFact, Observed, Snapshot } from "./page.ts";
import { thumbColumns } from "./pixels.ts";
import { parseInline, parseScenario, probed, ScenarioError } from "./scenario.ts";

const observed = (o: Partial<Observed> = {}): Observed => ({
  mutations: 0,
  firstAtMs: null,
  outside: 0,
  lastAtMs: null,
  focusMoved: false,
  previousFocusAlive: true,
  changed: [],
  ...o,
});

const snap = (s: Partial<Snapshot> = {}): Snapshot => ({
  focus: { kind: "body" },
  regions: [],
  paintsOver: [],
  covered: [],
  scrollers: [],
  text: [],
  live: [],
  keyHints: [],
  viewport: { width: 1280, height: 800 },
  theme: { theme: "light", tone: "white" },
  watched: true,
  ...s,
});

const chip = (name: string, current: boolean, groupCurrent: string): FocusFact => ({
  kind: "element",
  selector: `button[aria-label="${name}"]`,
  role: "button",
  name,
  tag: "button",
  box: { x: 0, y: 0, w: 100, h: 30 },
  visible: true,
  inViewport: true,
  clipped: false,
  covered: 0,
  coveredBy: null,
  focusVisible: true,
  indicator: true,
  current,
  group: { selector: "nav > ul.row", current: groupCurrent },
});

const change = (c: Partial<Change> = {}): Change => ({
  mutations: 0,
  changed: [],
  firstAtMs: null,
  outside: 0,
  focusMoved: false,
  effect: "none",
  explained: false,
  textAdded: [],
  textRemoved: [],
  ...c,
});

const base = { thumbs: [], axe: [], expectChange: false } as const;
const ids = (flags: { id: string }[]): string[] => flags.map((f) => f.id);

describe("scenario", () => {
  it("parses a JSON scenario with defaults", () => {
    const s = parseScenario({
      steps: [
        { do: "seed", use: "project" },
        { do: "press", key: "Escape" },
      ],
    });
    expect(s.viewport).toEqual({ width: 1280, height: 800 });
    expect(s.theme).toBe("light");
    expect(s.engine).toBe("chromium");
    expect(s.motion).toBe("reduce");
    // the app's "system" language follows the browser: pinned, not the machine's
    expect(s.locale).toBe("en-US");
    expect(parseScenario({ locale: "pt-BR", steps: [{ do: "probe" }] }).locale).toBe("pt-BR");
    expect(() => parseScenario({ locale: "english!", steps: [{ do: "probe" }] })).toThrow(
      /language tag/,
    );
    expect(s.steps[0]).toEqual({ do: "seed", use: "project", args: {} });
    expect(probed(s.steps[0]!)).toBe(false);
    expect(probed(s.steps[1]!)).toBe(true);
  });

  it("keeps seed arguments apart from step fields", () => {
    const [step] = parseScenario({
      steps: [
        {
          do: "seed",
          use: "importFiles",
          target: "",
          sources: ["a.md"],
          probe: true,
          watch: "[role=tree]",
        },
      ],
    }).steps;
    expect(step).toEqual({
      do: "seed",
      use: "importFiles",
      args: { target: "", sources: ["a.md"] },
      probe: true,
      watch: "[role=tree]",
    });
  });

  it("refuses malformed scenarios with the step named", () => {
    expect(() => parseScenario({ steps: [] })).toThrow(ScenarioError);
    expect(() => parseScenario({ steps: [{ do: "fly" }] })).toThrow(/step 1/);
    expect(() => parseScenario({ steps: [{ do: "click" }] })).toThrow(/step 1 target/);
    expect(() => parseScenario({ steps: [{ do: "seed", use: "nope" }] })).toThrow(/seed "use"/);
    expect(() => parseScenario({ steps: [{ do: "wait" }] })).toThrow(/target or ms/);
    expect(() => parseScenario({ steps: [{ do: "wait", ms: 60_000 }] })).toThrow(/0\.\.5000/);
    expect(() =>
      parseScenario({ viewport: { width: 10, height: 800 }, steps: [{ do: "probe" }] }),
    ).toThrow();
    expect(() => parseScenario({ theme: "blue", steps: [{ do: "probe" }] })).toThrow(/theme/);
    expect(() => parseScenario({ steps: [{ do: "probe", within: -1 }] })).toThrow(/within/);
    expect(() => parseScenario([1, 2])).toThrow(ScenarioError);
  });

  it("parses inline steps, selectors with spaces and seed key=value arguments", () => {
    const steps = parseInline(
      " seed project ;seed doc docs/guide.md; click role=button[name='Keep all']; press Control+Alt+ArrowRight; " +
        "seed setting key=language value=de; seed foreignTreeNews count=3; viewport 900x700; wait 200; probe end;",
    );
    expect(steps.map((s) => s.do)).toEqual([
      "seed",
      "seed",
      "click",
      "press",
      "seed",
      "seed",
      "viewport",
      "wait",
      "probe",
    ]);
    expect(steps[1]).toMatchObject({ use: "doc", args: { path: "docs/guide.md" } });
    expect(steps[2]).toMatchObject({ target: "role=button[name='Keep all']" });
    expect(steps[4]).toMatchObject({ args: { key: "language", value: "de" } });
    expect(steps[5]).toMatchObject({ args: { count: 3 } });
    expect(steps[6]).toMatchObject({ width: 900, height: 700 });
    expect(steps[8]).toMatchObject({ name: "end" });
  });

  it("drags and drops files from outside: default one file, or the paths given; probed", () => {
    const steps = parseScenario({
      steps: [
        { do: "drag", target: "[role=treeitem][data-path='docs']" },
        { do: "drop", target: "[data-path='docs']", paths: ["C:/a b/x.md", "C:/y.png"] },
      ],
    }).steps;
    expect(steps[0]).toMatchObject({
      do: "drag",
      paths: ["C:/Users/demo/Downloads/dropped-note.md"],
    });
    expect(steps[1]).toMatchObject({ do: "drop", paths: ["C:/a b/x.md", "C:/y.png"] });
    expect(steps.every(probed)).toBe(true);
    const inline = parseInline(
      `drag [data-path='docs']; drop [data-path='notes'] paths=["C:/Users/demo/z.md"]`,
    );
    expect(inline[0]).toMatchObject({ do: "drag", target: "[data-path='docs']" });
    expect(inline[1]).toMatchObject({
      do: "drop",
      target: "[data-path='notes']",
      paths: ["C:/Users/demo/z.md"],
    });
    expect(() => parseScenario({ steps: [{ do: "drop" }] })).toThrow(/step 1 target/);
    for (const paths of [[], [""], "C:/x.md", [1], Array.from({ length: 51 }, (_, i) => `C:/${i}`)])
      expect(() => parseScenario({ steps: [{ do: "drop", target: "body", paths }] })).toThrow(
        /paths/,
      );
  });

  it("refuses bad inline steps", () => {
    expect(() => parseInline("  ;  ")).toThrow(/no steps/);
    expect(() => parseInline("jump high")).toThrow(/unknown verb/);
    expect(() => parseInline("viewport big")).toThrow(/WIDTHxHEIGHT/);
    expect(() => parseInline("seed doc a.md b.md")).toThrow(/one positional/);
    expect(() => parseInline("click")).toThrow(/target/);
    expect(() => parseInline("seed failPlugin method=x match={bad")).toThrow(/bad JSON/);
  });
});

describe("command line", () => {
  it("reads options; they override the scenario", () => {
    const o = parseOptions([
      "s.json",
      "--engine",
      "webkit",
      "--theme",
      "dark",
      "--width",
      "900",
      "--locale",
      "de",
      "--no-axe",
    ]);
    expect(o).toMatchObject({
      source: "s.json",
      engine: "webkit",
      theme: "dark",
      width: 900,
      axe: false,
    });
    const s = scenarioFrom({ engine: "msedge", steps: [{ do: "probe" }] }, o);
    expect(s).toMatchObject({
      engine: "webkit",
      theme: "dark",
      locale: "de",
      viewport: { width: 900, height: 800 },
    });
    expect(scenarioFrom("seed project; probe", parseOptions(["x"])).steps).toHaveLength(2);
  });

  it("refuses bad options, and URLs off this machine", () => {
    expect(() => parseOptions([])).toThrow(/scenario/);
    expect(() => parseOptions(["a", "b"])).toThrow(/one scenario/);
    expect(() => parseOptions(["a", "--width"])).toThrow(/value expected/);
    expect(() => parseOptions(["a", "--width", "wide"])).toThrow(/whole number/);
    expect(() => parseOptions(["a", "--engine", "firefox"])).toThrow(/--engine/);
    expect(() => parseOptions(["a", "--headed"])).toThrow(/unknown option/);
    expect(() => parseOptions(["a", "--url", "https://example.com/"])).toThrow(/loopback/);
    expect(parseOptions(["a", "--url", "http://127.0.0.1:5173/"]).url).toBe(
      "http://127.0.0.1:5173/",
    );
  });
});

describe("facts", () => {
  it("diffs text as multisets, capped", () => {
    expect(textDiff(["a", "b", "b"], ["b", "c", "a"])).toEqual({ added: ["c"], removed: ["b"] });
    expect(textDiff([], [])).toEqual({ added: [], removed: [] });
    const many = Array.from({ length: 100 }, (_, i) => `l${i}`);
    expect(textDiff([], many).added).toHaveLength(40);
  });

  it("classes the effect of an action", () => {
    expect(effectOf(observed())).toBe("none");
    expect(effectOf(observed({ mutations: 3, outside: 0 }))).toBe("selfOnly");
    expect(effectOf(observed({ mutations: 3, outside: 1 }))).toBe("elsewhere");
    expect(effectOf(observed({ focusMoved: true }))).toBe("elsewhere");
  });

  it("names keys the way hints write them", () => {
    expect(keyNames("Escape")).toEqual(["escape", "esc"]);
    expect(keyNames("Control+Shift+k")).toEqual(["ctrl+shift+k"]);
    const hints = [
      { element: "button.done", hint: "Esc" },
      { element: "button.keep", hint: "Shift+K" },
      { element: "a", hint: "see notes" },
    ];
    expect(promisedBy("Escape", hints)).toEqual([hints[0]]);
    expect(promisedBy("Shift+k", hints)).toEqual([hints[1]]);
    expect(promisedBy("Enter", hints)).toEqual([]);
  });

  it("flags focus left on a chip that is no longer current", () => {
    const before = snap({ focus: chip("one", true, "one") });
    const after = snap({ focus: chip("one", false, "two") });
    const flags = flagsOf({
      ...base,
      action: "press",
      key: "Control+Tab",
      before,
      after,
      change: change({ effect: "elsewhere" }),
      observed: observed({ mutations: 4, outside: 4 }),
    });
    expect(ids(flags)).toEqual(["focus-stale-in-group"]);
    // arrow keys inside the group move focus without changing the current one: no flag
    const roving = snap({ focus: chip("two", false, "one") });
    expect(
      ids(
        flagsOf({
          ...base,
          action: "press",
          key: "ArrowRight",
          before,
          after: roving,
          change: change({ effect: "elsewhere" }),
          observed: observed({ focusMoved: true }),
        }),
      ),
    ).toEqual([]);
  });

  it("flags hidden, clipped and ring-less focus, and focus lost to the page", () => {
    const hidden = { ...chip("x", true, "x"), covered: 1, coveredBy: "header" } as FocusFact;
    const part = { ...chip("x", true, "x"), covered: 0.4, coveredBy: "header" } as FocusFact;
    const cut = { ...chip("x", true, "x"), clipped: true } as FocusFact;
    const bare = { ...chip("x", true, "x"), indicator: false } as FocusFact;
    const run = (focus: FocusFact) =>
      ids(
        flagsOf({
          ...base,
          action: "none",
          before: null,
          after: snap({ focus }),
          change: null,
          observed: null,
        }),
      );
    expect(run(hidden)).toEqual(["focus-obscured"]);
    expect(run(part)).toEqual(["focus-partly-obscured"]);
    expect(run(cut)).toEqual(["focus-clipped"]);
    expect(run(bare)).toEqual(["focus-no-indicator"]);
    const lost = flagsOf({
      ...base,
      action: "press",
      key: "Enter",
      before: snap({ focus: chip("x", true, "x") }),
      after: snap(),
      change: change({ effect: "elsewhere" }),
      observed: observed({ focusMoved: true, mutations: 1, outside: 1 }),
    });
    expect(ids(lost)).toEqual(["focus-lost"]);
  });

  it("flags dead clicks and keys unless the page said something", () => {
    const run = (
      action: "click" | "press",
      c: Change,
      key?: string,
      hints: Snapshot["keyHints"] = [],
    ) =>
      ids(
        flagsOf({
          ...base,
          action,
          ...(key ? { key } : {}),
          before: snap(),
          after: snap({ keyHints: hints }),
          change: c,
          observed: observed(),
        }),
      );
    expect(run("click", change())).toEqual(["dead-control"]);
    expect(run("click", change({ effect: "selfOnly" }))).toEqual(["no-effect-beyond-control"]);
    expect(run("click", change({ effect: "selfOnly", explained: true }))).toEqual([]);
    // clicking the option that is already chosen is expected to change nothing
    const chosen = flagsOf({
      ...base,
      action: "click",
      before: snap(),
      after: snap(),
      change: change(),
      observed: observed(),
      selected: true,
    });
    expect(ids(chosen)).toEqual([]);
    expect(run("press", change(), "Escape")).toEqual(["dead-key"]);
    expect(run("press", change(), "Escape", [{ element: "button", hint: "Esc" }])).toEqual([
      "dead-key",
      "dead-key-promised",
    ]);
    expect(
      run("press", change({ effect: "elsewhere" }), "Escape", [{ element: "button", hint: "Esc" }]),
    ).toEqual([]);
  });

  it("flags drawing over another area, overlapping areas, covered controls, missing change, scroll bars, axe", () => {
    const after = snap({
      paintsOver: [
        {
          element: "div.doc-header",
          from: "main",
          over: "aside.panel",
          box: { x: 1, y: 1, w: 9, h: 9 },
        },
      ],
      regions: [
        { selector: "main", label: "", box: { x: 0, y: 0, w: 500, h: 500 }, overlay: false },
        { selector: "aside", label: "", box: { x: 490, y: 0, w: 300, h: 500 }, overlay: false },
        { selector: "dialog", label: "", box: { x: 0, y: 0, w: 900, h: 900 }, overlay: true },
        { selector: "footer", label: "", box: { x: 0, y: 500, w: 900, h: 20 }, overlay: false },
      ],
      covered: [{ element: "button", name: "Save", by: "div.toast" }],
    });
    const flags = flagsOf({
      action: "other",
      before: snap(),
      after,
      change: change({ effect: "elsewhere" }),
      observed: observed({ mutations: 1, outside: 1 }),
      thumbs: [
        { selector: "div.scroller", visible: false, width: 0 },
        { selector: "div.tree", visible: true, width: 3 },
        { selector: "div.ok", visible: true, width: 8 },
      ],
      axe: [{ id: "color-contrast" }],
      expectChange: true,
    });
    expect(ids(flags)).toEqual([
      "paints-over-region",
      "region-overlap",
      "control-covered",
      "no-change",
      "scrollbar-invisible",
      "scrollbar-thin",
      "axe:color-contrast",
    ]);
  });

  it("checks expectations: present, absent, prefix, missing step", () => {
    const steps = [
      {
        name: "esc",
        flags: [
          { id: "dead-key", detail: "" },
          { id: "axe:label", detail: "" },
        ],
      },
    ];
    expect(unmet([{ step: "esc", flag: "dead-key" }], steps)).toEqual([]);
    expect(unmet([{ step: "esc", flag: "axe:*" }], steps)).toEqual([]);
    expect(unmet([{ step: "esc", flag: "dead-key", absent: true }], steps)).toEqual([
      "esc: unexpected dead-key",
    ]);
    expect(unmet([{ step: "esc", flag: "focus-lost" }], steps)).toEqual([
      "esc: missing focus-lost",
    ]);
    expect(unmet([{ step: "go", flag: "x" }], steps)).toEqual(['step "go" not run']);
  });
});

describe("thumb pixels", () => {
  /** A `w` x `h` RGBA lane of one color with columns `from..to` drawn darker. */
  function lane(w: number, h: number, from: number, to: number): number[] {
    const data: number[] = [];
    for (let y = 0; y < h; y++) {
      for (let x = 0; x < w; x++) {
        const thumb = x >= from && x <= to && y > 5 && y < 30;
        data.push(thumb ? 150 : 250, thumb ? 150 : 250, thumb ? 150 : 250, 255);
      }
    }
    return data;
  }

  it("counts the columns that differ from the lane's own color", () => {
    expect(thumbColumns(lane(14, 100, 3, 10), 14, 100)).toBe(8);
    expect(thumbColumns(lane(14, 100, 99, 99), 14, 100)).toBe(0);
    // a faint shade within tolerance is not a thumb
    const faint = lane(14, 100, 99, 99).map((v, i) => (i % 4 === 3 ? v : i < 4 * 14 ? v - 5 : v));
    expect(thumbColumns(faint, 14, 100)).toBe(0);
  });
});

describe("calibration fixtures", () => {
  const dir = path.join(import.meta.dirname, "..", "calibration");
  const files = readdirSync(dir).filter(
    (f) => f.endsWith(".json") && f !== "not-reproducible.json",
  );

  it("every fixture parses, names its commits and expects flags at each commit it runs at", () => {
    expect(files.length).toBeGreaterThanOrEqual(6);
    for (const f of files) {
      const fx = parseFixture(JSON.parse(readFileSync(path.join(dir, f), "utf8")));
      expect(`${fx.id}.json`).toBe(f);
      expect(fx.expect.now?.length).toBeGreaterThan(0);
      if (fx.fix !== null) {
        expect(commitAt(fx, "before")).toBe(`${fx.fix}^`);
        expect(commitAt(fx, "after")).toBe(fx.fix);
      } else {
        expect(() => commitAt(fx, "before")).toThrow(/open bug/);
      }
      const names = fx.scenario.steps.map((s, i) => s.name ?? `step-${i + 1}`);
      for (const list of Object.values(fx.expect))
        for (const e of list ?? []) expect(names).toContain(e.step);
    }
  });

  it("refuses a fixture without its expectations", () => {
    const raw = JSON.parse(readFileSync(path.join(dir, files[0] ?? ""), "utf8")) as Record<
      string,
      unknown
    >;
    expect(() => parseFixture({ ...raw, expect: {} })).toThrow(/expect.now/);
    expect(() => parseFixture({ ...raw, fix: "not a sha" })).toThrow(/commit id/);
    expect(() => parseFixture({ ...raw, severityMin: "huge" })).toThrow(/severityMin/);
  });
});
