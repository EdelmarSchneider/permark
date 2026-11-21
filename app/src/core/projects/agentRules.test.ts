// The line about a rule file PerMark could not read: the file, why in words for every reason, and what stands in.
import { describe, expect, it } from "vitest";
import type { RulesWhy } from "../../api";
import { rulesProblemText } from "./agentRules";

const WHY: readonly (readonly [RulesWhy, string])[] = [
  ["tooLarge", "it's too large"],
  ["notText", "it isn't plain text"],
  ["notToml", "it isn't valid TOML"],
  ["badValue", "one of its settings has the wrong kind of value"],
  ["badPattern", "one of its patterns isn't valid"],
  ["notAFile", "it isn't a regular file"],
  ["locked", "another program is using it"],
  ["denied", "PerMark isn't allowed to read it"],
  ["failed", "it couldn't be read"],
];

describe("rulesProblemText", () => {
  it("says the file, why, and that its last rules that worked stand in", () => {
    for (const [why, words] of WHY) {
      expect(rulesProblemText({ file: ".aiignore", why, fallback: "lastGood" })).toBe(
        `Couldn't read .aiignore: ${words}. PerMark uses the last rules from it that worked until you fix it.`,
      );
    }
  });

  it("says agents get nothing the file covers while no earlier rules of it stand in", () => {
    expect(rulesProblemText({ file: "../.aiignore", why: "denied", fallback: "closed" })).toBe(
      "Couldn't read ../.aiignore: PerMark isn't allowed to read it. Agents get nothing it covers until you fix it.",
    );
  });
});
