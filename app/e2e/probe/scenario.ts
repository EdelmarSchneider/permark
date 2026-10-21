// Scenario = what the probe does in the mock app, step by step (spec: ux-capability-specs/probe.md §5). Pure: parsed and
// checked here, run by run.ts.

export type Engine = "chromium" | "msedge" | "webkit";
export type Theme = "light" | "dark";
export type Motion = "reduce" | "full";

export const SEEDS = [
  "project",
  "tab",
  "notes",
  "doc",
  "file",
  "setting",
  "rightPanel",
  "foreignTreeNews",
  "failPlugin",
  "importFiles",
] as const;
export type SeedUse = (typeof SEEDS)[number];

export type Common = {
  name?: string;
  /** Region whose text is compared before | after (selector; default `body`). */
  watch?: string;
  /** Milliseconds to wait for the effects of the action. */
  within?: number;
  probe?: boolean;
  axe?: boolean;
  /** The person expects the watched region to change (a missing change is flagged). */
  expectChange?: boolean;
};

export type Step = Common &
  (
    | { do: "seed"; use: SeedUse; args: Record<string, unknown> }
    | { do: "click" | "dblclick" | "hover" | "focus"; target: string }
    | { do: "press"; key: string; target?: string }
    | { do: "type"; text: string }
    | { do: "wait"; target?: string; ms?: number }
    | { do: "probe" }
    | { do: "sweep"; target: string; max?: number }
    | { do: "viewport"; width: number; height: number }
  );

export type Scenario = {
  name: string;
  viewport: { width: number; height: number };
  theme: Theme;
  engine: Engine;
  motion: Motion;
  /** Browser language (BCP 47): the app's "system" language follows it. */
  locale: string;
  steps: Step[];
};

export const DEFAULT_VIEWPORT = { width: 1280, height: 800 } as const;
const MAX_WITHIN = 10_000;
const MAX_WAIT = 5_000;

export class ScenarioError extends Error {}

const isObject = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v);

function text(v: unknown, what: string): string {
  if (typeof v !== "string" || v.trim() === "") throw new ScenarioError(`${what}: text expected`);
  return v;
}

function size(v: unknown, what: string, min: number, max: number): number {
  if (typeof v !== "number" || !Number.isInteger(v) || v < min || v > max) {
    throw new ScenarioError(`${what}: whole number ${min}..${max} expected`);
  }
  return v;
}

function common(raw: Record<string, unknown>, at: number): Common {
  const out: Common = {};
  if (raw["name"] !== undefined) out.name = text(raw["name"], `step ${at} name`);
  if (raw["watch"] !== undefined) out.watch = text(raw["watch"], `step ${at} watch`);
  if (raw["within"] !== undefined)
    out.within = size(raw["within"], `step ${at} within`, 0, MAX_WITHIN);
  for (const key of ["probe", "axe", "expectChange"] as const) {
    const v = raw[key];
    if (v === undefined) continue;
    if (typeof v !== "boolean") throw new ScenarioError(`step ${at} ${key}: true | false expected`);
    out[key] = v;
  }
  return out;
}

/** One step from JSON. */
export function parseStep(raw: unknown, at: number): Step {
  if (!isObject(raw)) throw new ScenarioError(`step ${at}: object expected`);
  const base = common(raw, at);
  const kind = raw["do"];
  switch (kind) {
    case "seed": {
      const use = raw["use"];
      if (typeof use !== "string" || !(SEEDS as readonly string[]).includes(use)) {
        throw new ScenarioError(`step ${at}: seed "use" one of ${SEEDS.join(", ")}`);
      }
      const args: Record<string, unknown> = {};
      for (const [k, v] of Object.entries(raw)) {
        if (!["do", "use", "name", "watch", "within", "probe", "axe", "expectChange"].includes(k)) {
          args[k] = v;
        }
      }
      return { ...base, do: "seed", use: use as SeedUse, args };
    }
    case "click":
    case "dblclick":
    case "hover":
    case "focus":
      return { ...base, do: kind, target: text(raw["target"], `step ${at} target`) };
    case "press": {
      const step: Step = { ...base, do: "press", key: text(raw["key"], `step ${at} key`) };
      if (raw["target"] !== undefined) step.target = text(raw["target"], `step ${at} target`);
      return step;
    }
    case "type":
      if (typeof raw["text"] !== "string")
        throw new ScenarioError(`step ${at} text: text expected`);
      return { ...base, do: "type", text: raw["text"] };
    case "wait": {
      const step: Step = { ...base, do: "wait" };
      if (raw["target"] !== undefined) step.target = text(raw["target"], `step ${at} target`);
      if (raw["ms"] !== undefined) step.ms = size(raw["ms"], `step ${at} ms`, 0, MAX_WAIT);
      if (step.target === undefined && step.ms === undefined) {
        throw new ScenarioError(`step ${at}: wait needs target or ms`);
      }
      return step;
    }
    case "probe":
      return { ...base, do: "probe" };
    case "sweep": {
      const step: Step = { ...base, do: "sweep", target: text(raw["target"], `step ${at} target`) };
      if (raw["max"] !== undefined) step.max = size(raw["max"], `step ${at} max`, 1, 50);
      return step;
    }
    case "viewport":
      return {
        ...base,
        do: "viewport",
        width: size(raw["width"], `step ${at} width`, 200, 4000),
        height: size(raw["height"], `step ${at} height`, 200, 4000),
      };
    default:
      throw new ScenarioError(`step ${at}: unknown "do" ${JSON.stringify(kind)}`);
  }
}

/** A whole scenario from JSON (file or fixture). */
export function parseScenario(raw: unknown): Scenario {
  if (!isObject(raw)) throw new ScenarioError("scenario: object expected");
  const steps = raw["steps"];
  if (!Array.isArray(steps) || steps.length === 0) {
    throw new ScenarioError("scenario: non-empty steps[] expected");
  }
  const vp = raw["viewport"];
  let viewport: Scenario["viewport"] = { ...DEFAULT_VIEWPORT };
  if (vp !== undefined) {
    if (!isObject(vp)) throw new ScenarioError("viewport: {width, height} expected");
    viewport = {
      width: size(vp["width"], "viewport width", 200, 4000),
      height: size(vp["height"], "viewport height", 200, 4000),
    };
  }
  return {
    name: raw["name"] === undefined ? "probe" : text(raw["name"], "name"),
    viewport,
    theme: oneOf(raw["theme"], ["light", "dark"], "theme", "light"),
    engine: oneOf(raw["engine"], ["chromium", "msedge", "webkit"], "engine", "chromium"),
    motion: oneOf(raw["motion"], ["reduce", "full"], "motion", "reduce"),
    locale: raw["locale"] === undefined ? "en-US" : locale(raw["locale"], "locale"),
    steps: steps.map((s, i) => parseStep(s, i + 1)),
  };
}

export function oneOf<const T extends string>(
  v: unknown,
  all: readonly T[],
  what: string,
  def: T,
): T {
  if (v === undefined) return def;
  if (typeof v !== "string" || !(all as readonly string[]).includes(v)) {
    throw new ScenarioError(`${what}: one of ${all.join(", ")}`);
  }
  return v as T;
}

export function locale(v: unknown, what: string): string {
  if (typeof v !== "string" || !/^[a-z]{2,3}(-[A-Za-z0-9]{2,8})*$/.test(v)) {
    throw new ScenarioError(`${what}: language tag expected (en-US, de, pt-BR)`);
  }
  return v;
}

/** `key=value` -> value typed: numbers, true | false, JSON objects | arrays; else text. */
function argValue(v: string): unknown {
  if (/^-?\d+$/.test(v)) return Number(v);
  if (v === "true" || v === "false") return v === "true";
  if (v.startsWith("{") || v.startsWith("[")) {
    try {
      return JSON.parse(v) as unknown;
    } catch {
      throw new ScenarioError(`seed arg: bad JSON ${v}`);
    }
  }
  return v;
}

/** Inline steps (CLI): `seed project; seed doc README.md; click role=button[name='Review']; press Escape; probe`. */
export function parseInline(line: string): Step[] {
  const parts = line
    .split(";")
    .map((p) => p.trim())
    .filter((p) => p !== "");
  if (parts.length === 0) throw new ScenarioError("inline: no steps");
  return parts.map((part, i) => {
    const at = i + 1;
    const space = part.indexOf(" ");
    const verb = space < 0 ? part : part.slice(0, space);
    const rest = space < 0 ? "" : part.slice(space + 1).trim();
    switch (verb) {
      case "seed": {
        const [use, ...args] = rest.split(/\s+/).filter((a) => a !== "");
        const raw: Record<string, unknown> = { do: "seed", use };
        for (const arg of args) {
          const eq = arg.indexOf("=");
          if (eq > 0) raw[arg.slice(0, eq)] = argValue(arg.slice(eq + 1));
          else if (raw["path"] === undefined) raw["path"] = arg;
          else throw new ScenarioError(`step ${at}: one positional seed arg (path) only`);
        }
        return parseStep(raw, at);
      }
      case "click":
      case "dblclick":
      case "hover":
      case "focus":
      case "sweep":
        return parseStep({ do: verb, target: rest }, at);
      case "press":
        return parseStep({ do: "press", key: rest }, at);
      case "type":
        return parseStep({ do: "type", text: rest }, at);
      case "wait":
        return parseStep(
          /^\d+$/.test(rest) ? { do: "wait", ms: Number(rest) } : { do: "wait", target: rest },
          at,
        );
      case "probe":
        return parseStep(rest === "" ? { do: "probe" } : { do: "probe", name: rest }, at);
      case "viewport": {
        const m = /^(\d+)x(\d+)$/.exec(rest);
        if (!m) throw new ScenarioError(`step ${at}: viewport WIDTHxHEIGHT`);
        return parseStep({ do: "viewport", width: Number(m[1]), height: Number(m[2]) }, at);
      }
      default:
        throw new ScenarioError(`step ${at}: unknown verb "${verb}"`);
    }
  });
}

/** Whether a step is probed (facts + screenshot) when it does not say. */
export function probed(step: Step): boolean {
  if (step.probe !== undefined) return step.probe;
  return step.do !== "seed" && step.do !== "wait" && step.do !== "viewport";
}

/** Label of a step: its name or `step-<n>`. */
export function stepName(step: Step, at: number): string {
  return step.name ?? `step-${at}`;
}
