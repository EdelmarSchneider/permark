// The one line about a rule file on what agents may see that PerMark could not read (`.aiignore`, the folder's
// `.permark/config.toml`): which file, why, and what stands in for it: its last rules that worked, or nothing (agents
// get nothing it covers until it is fixed). The agent connection's settings and the Assistant panel say it alike.

import type { RulesProblem, RulesWhy } from "../../api";
import { t, type MessageKey } from "../i18n";

function whyKey(why: RulesWhy): MessageKey {
  switch (why) {
    case "tooLarge":
      return "agentRules.why.tooLarge";
    case "notText":
      return "agentRules.why.notText";
    case "notToml":
      return "agentRules.why.notToml";
    case "badValue":
      return "agentRules.why.badValue";
    case "badPattern":
      return "agentRules.why.badPattern";
    case "notAFile":
      return "agentRules.why.notAFile";
    case "locked":
      return "agentRules.why.locked";
    case "denied":
      return "agentRules.why.denied";
    case "failed":
      return "agentRules.why.failed";
    default:
      return why satisfies never;
  }
}

/** The line telling the person about `problem`. */
export function rulesProblemText(problem: RulesProblem): string {
  const params = { file: problem.file, why: t(whyKey(problem.why)) };
  switch (problem.fallback) {
    case "lastGood":
      return t("agentRules.lastGood", params);
    case "closed":
      return t("agentRules.closed", params);
    default:
      return problem.fallback satisfies never;
  }
}
