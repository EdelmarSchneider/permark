// The words the backend gives for "this part is not edited here" and "this edit was declined", as UI strings.

import type { MessageKey } from "../i18n";

const REASONS: Readonly<Record<string, MessageKey>> = {
  multiline: "config.reason.multiline",
  alias: "config.reason.alias",
  anchor: "config.reason.anchor",
  tag: "config.reason.tag",
  flow: "config.reason.flow",
  inlineTable: "config.reason.inlineTable",
  nested: "config.reason.nested",
  mixed: "config.reason.mixed",
  blockScalar: "config.reason.blockScalar",
  continuation: "config.reason.continuation",
  duplicate: "config.reason.duplicate",
  multiDoc: "config.reason.multiDoc",
  noValue: "config.reason.noValue",
  empty: "config.reason.empty",
  unterminated: "config.reason.unterminated",
  firstField: "config.reason.firstField",
};

const REFUSALS: Readonly<Record<string, MessageKey>> = {
  newline: "config.refused.newline",
  empty: "config.refused.empty",
  kind: "config.refused.kind",
  literal: "config.refused.literal",
  notRepresentable: "config.refused.notRepresentable",
  syntax: "config.refused.syntax",
  roundTrip: "config.refused.roundTrip",
  expectedText: "config.refused.kind",
  expectedLiteral: "config.refused.kind",
  continuation: "config.refused.continuation",
  notFound: "config.refused.notFound",
  unsupported: "config.refused.unsupported",
  duplicate: "config.refused.duplicate",
};

/** Why a part is read only. */
export function reasonKey(reason: string): MessageKey {
  return REASONS[reason] ?? "config.reason.other";
}

/** Why an edit was declined (a read-only reason or a refusal word). */
export function refusalKey(word: string): MessageKey {
  return REFUSALS[word] ?? REASONS[word] ?? "config.refused.other";
}
