import { describe, expect, it } from "vitest";
import en from "../i18n/locales/en";
import fixture from "./fixture.json";
import { available, type Availability, type Context, type Requirement } from "./available";
import { reasonKey, remedyKey } from "./guidance";

type Case = {
  name: string;
  requirement: Requirement;
  context: Partial<Context>;
  expected: Availability;
};

// JSON import is typed structurally; the shapes are the generated ones (the Rust test parses the same file strictly).
const base: Context = fixture.base as Context;
const cases: Case[] = fixture.cases as Case[];

describe("available (shared fixture with Rust)", () => {
  it("has the shared cases", () => {
    expect(cases.length).toBeGreaterThanOrEqual(20);
  });

  for (const c of cases) {
    it(c.name, () => {
      expect(available(c.requirement, { ...base, ...c.context })).toEqual(c.expected);
    });
  }
});

describe("guidance", () => {
  it("has a sentence for every need and a label for every remedy", () => {
    for (const c of cases) {
      if (c.expected.status !== "unavailable") continue;
      expect(Object.keys(en)).toContain(reasonKey(c.expected.reason));
      const remedy = c.expected.reason.remedy;
      if (remedy !== null) expect(remedyKey(remedy)).not.toBeNull();
    }
    expect(remedyKey("plugins.enable:review")).toBe("condition.remedy.plugins.enable");
    expect(remedyKey("rm -rf")).toBeNull();
    expect(remedyKey(null)).toBeNull();
  });
});
