// Calibration fixture (spec: probe.md §9): one director-found UX bug, the scenario that shows it, the flags expected at
// the commit before its fix, at the fix, and now. Pure; tested.

import type { Expectation } from "../probe/facts.ts";
import { parseScenario, type Scenario } from "../probe/scenario.ts";

export type At = "now" | "before" | "after";

export type Fixture = {
  id: string;
  bug: string;
  found: string;
  persona: string;
  severityMin: "blocker" | "major" | "minor" | "polish";
  /** Commit that fixed it; null = open bug. */
  fix: string | null;
  scenario: Scenario;
  expect: Partial<Record<At, Expectation[]>>;
};

const SEVERITIES = ["blocker", "major", "minor", "polish"] as const;

function expectations(raw: unknown, what: string): Expectation[] {
  if (!Array.isArray(raw)) throw new Error(`${what}: list expected`);
  return raw.map((e, i) => {
    if (typeof e !== "object" || e === null) throw new Error(`${what}[${i}]: object expected`);
    const { step, flag, absent } = e as Record<string, unknown>;
    if (typeof step !== "string" || typeof flag !== "string") {
      throw new Error(`${what}[${i}]: step + flag text expected`);
    }
    if (absent !== undefined && typeof absent !== "boolean")
      throw new Error(`${what}[${i}]: absent true | false`);
    return absent === undefined ? { step, flag } : { step, flag, absent };
  });
}

export function parseFixture(raw: unknown): Fixture {
  if (typeof raw !== "object" || raw === null) throw new Error("fixture: object expected");
  const r = raw as Record<string, unknown>;
  for (const key of ["id", "bug", "found", "persona"] as const) {
    if (typeof r[key] !== "string" || r[key] === "")
      throw new Error(`fixture ${key}: text expected`);
  }
  const severity = r["severityMin"];
  if (typeof severity !== "string" || !(SEVERITIES as readonly string[]).includes(severity)) {
    throw new Error(`fixture severityMin: one of ${SEVERITIES.join(", ")}`);
  }
  const fix = r["fix"];
  if (fix !== null && (typeof fix !== "string" || !/^[0-9a-f]{7,40}$/.test(fix))) {
    throw new Error("fixture fix: commit id | null");
  }
  const rawExpect = r["expect"];
  if (typeof rawExpect !== "object" || rawExpect === null)
    throw new Error("fixture expect: object expected");
  const expect: Fixture["expect"] = {};
  for (const at of ["now", "before", "after"] as const) {
    const list = (rawExpect as Record<string, unknown>)[at];
    if (list !== undefined) expect[at] = expectations(list, `expect.${at}`);
  }
  if (expect.now === undefined) throw new Error("fixture expect.now: required");
  if (fix !== null && (expect.before === undefined || expect.after === undefined)) {
    throw new Error("fixture with a fix: expect.before + expect.after required");
  }
  return {
    id: r["id"] as string,
    bug: r["bug"] as string,
    found: r["found"] as string,
    persona: r["persona"] as string,
    severityMin: severity as Fixture["severityMin"],
    fix,
    scenario: parseScenario(r["scenario"]),
    expect,
  };
}

/** The commit a fixture runs at (`null` = this working tree). */
export function commitAt(fixture: Fixture, at: At): string | null {
  if (at === "now") return null;
  if (fixture.fix === null) throw new Error(`${fixture.id}: open bug, no "${at}" commit`);
  return at === "before" ? `${fixture.fix}^` : fixture.fix;
}
