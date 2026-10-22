// The once-only offer to share a project that holds agent files (`AGENTS.md`, `CLAUDE.md`, ...). Pure: the store
// keeps the answer. The offer never turns anything on: "Set up…" only opens the settings section.

import type { WorkspaceKind } from "../../api/generated/WorkspaceKind";

/** The project session key that remembers the answer (profile only, NEVER `.permark/`; either answer is for good). */
export const OFFER_KEY = "agent.offer";

/** `setup`: the person went to the settings (from the offer, or found the switch alone); `declined`: "No thanks". */
export type OfferAnswer = "setup" | "declined";

/**
 * What the window knows of the answer for its project: still reading it, never asked, answered, or the key could not
 * be read (no offer: nothing for the person to fix).
 */
export type OfferState =
  | { status: "loading" }
  | { status: "unasked" }
  | { status: "answered"; answer: OfferAnswer }
  | { status: "unreadable" };

export function isOfferAnswer(value: unknown): value is OfferAnswer {
  return value === "setup" || value === "declined";
}

/**
 * The stored value as a state: absent = never asked; a value this version does not know (a later version wrote it)
 * counts as answered, so the person is never asked twice.
 */
export function offerStateOf(value: unknown): OfferState {
  if (value === null || value === undefined) return { status: "unasked" };
  return { status: "answered", answer: isOfferAnswer(value) ? value : "declined" };
}

export type OfferInput = {
  workspace: WorkspaceKind;
  /** The agent files at the project root (`Context.agentFiles`). */
  files: readonly string[];
  state: OfferState;
  /** The switch of this project (`null`: not known yet). */
  enabled: boolean | null;
};

/** Whether the strip shows now: a project with agent files, the switch known to be off, never answered. */
export function shouldOfferAgent(input: OfferInput): boolean {
  return (
    input.workspace === "project" &&
    input.files.length > 0 &&
    input.enabled === false &&
    input.state.status === "unasked"
  );
}
