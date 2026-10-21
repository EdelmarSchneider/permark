// Command line of `ux:probe` | `ux:calibrate` (pure; tested). Options override the scenario's own fields.

import {
  locale,
  oneOf,
  parseInline,
  parseScenario,
  type Engine,
  type Motion,
  type Scenario,
  type Theme,
} from "./scenario.ts";

export class UsageError extends Error {}

export type Options = {
  /** Scenario file path or inline steps. */
  source: string;
  engine?: Engine;
  theme?: Theme;
  motion?: Motion;
  locale?: string;
  width?: number;
  height?: number;
  axe: boolean;
  out?: string;
  app?: string;
  url?: string;
};

function num(v: string | undefined, flag: string): number {
  const n = Number(v);
  if (v === undefined || !Number.isInteger(n) || n < 200 || n > 4000) {
    throw new UsageError(`${flag}: whole number 200..4000 expected`);
  }
  return n;
}

function value(argv: string[], at: number, flag: string): string {
  const v = argv[at + 1];
  if (v === undefined || v.startsWith("--")) throw new UsageError(`${flag}: value expected`);
  return v;
}

export function parseOptions(argv: string[]): Options {
  const out: Partial<Options> & { axe: boolean } = { axe: true };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i] ?? "";
    switch (a) {
      case "--engine":
        out.engine = oneOf(value(argv, i++, a), ["chromium", "msedge", "webkit"], a, "chromium");
        break;
      case "--theme":
        out.theme = oneOf(value(argv, i++, a), ["light", "dark"], a, "light");
        break;
      case "--motion":
        out.motion = oneOf(value(argv, i++, a), ["reduce", "full"], a, "reduce");
        break;
      case "--locale":
        out.locale = locale(value(argv, i++, a), a);
        break;
      case "--width":
        out.width = num(value(argv, i++, a), a);
        break;
      case "--height":
        out.height = num(value(argv, i++, a), a);
        break;
      case "--no-axe":
        out.axe = false;
        break;
      case "--out":
        out.out = value(argv, i++, a);
        break;
      case "--app":
        out.app = value(argv, i++, a);
        break;
      case "--url": {
        const url = value(argv, i++, a);
        if (!/^http:\/\/(127\.0\.0\.1|localhost|\[::1\])(:\d+)?\//.test(url)) {
          throw new UsageError("--url: loopback http URL expected (http://127.0.0.1:<port>/)");
        }
        out.url = url;
        break;
      }
      default:
        if (a.startsWith("--")) throw new UsageError(`unknown option ${a}`);
        if (out.source !== undefined)
          throw new UsageError("one scenario only (quote inline steps)");
        out.source = a;
    }
  }
  if (out.source === undefined) throw new UsageError("scenario file or inline steps expected");
  return out as Options;
}

/** The scenario to run: JSON (file content) or inline steps, with the options applied. */
export function scenarioFrom(raw: unknown, options: Options): Scenario {
  const scenario =
    typeof raw === "string"
      ? { ...parseScenario({ name: "inline", steps: [{ do: "probe" }] }), steps: parseInline(raw) }
      : parseScenario(raw);
  return {
    ...scenario,
    engine: options.engine ?? scenario.engine,
    theme: options.theme ?? scenario.theme,
    motion: options.motion ?? scenario.motion,
    locale: options.locale ?? scenario.locale,
    viewport: {
      width: options.width ?? scenario.viewport.width,
      height: options.height ?? scenario.viewport.height,
    },
  };
}
