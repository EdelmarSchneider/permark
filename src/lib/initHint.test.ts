import { describe, expect, it } from "vitest";
import { showInitHint, type InitHintInput } from "./initHint";
import { defaultSession, normalizeSession } from "./session";

// A repository opened in the IDE, with no `.trailmark/` yet, where nothing was answered: the case the hint is for.
const ASKING: InitHintInput = {
  simple: false,
  isRepo: true,
  stateLocation: "appData",
  never: false,
  snoozed: false,
  dialogOpen: false,
};

describe("showInitHint", () => {
  it("is shown for a repository without .trailmark/", () => {
    expect(showInitHint(ASKING)).toBe(true);
  });

  it("is not shown for a folder that is not a repository", () => {
    expect(showInitHint({ ...ASKING, isRepo: false })).toBe(false);
  });

  it("is not shown once the project has .trailmark/", () => {
    expect(showInitHint({ ...ASKING, stateLocation: "project" })).toBe(false);
  });

  it("is not shown in Simple mode, which has no project to keep settings with", () => {
    expect(showInitHint({ ...ASKING, simple: true })).toBe(false);
  });

  it("stays away for this session after Not now", () => {
    expect(showInitHint({ ...ASKING, snoozed: true })).toBe(false);
  });

  it("stays away for good after Never for this folder", () => {
    expect(showInitHint({ ...ASKING, never: true })).toBe(false);
  });

  it("steps aside while the Initialize dialog is open", () => {
    expect(showInitHint({ ...ASKING, dialogOpen: true })).toBe(false);
  });
});

describe("Never for this folder", () => {
  it("is remembered with the project's session, and starts off", () => {
    expect(defaultSession().initHintNever).toBe(false);
    expect(normalizeSession({}).initHintNever).toBe(false);
    expect(normalizeSession({ initHintNever: true }).initHintNever).toBe(true);
  });

  it("reads only a real true, so a damaged save asks again instead of staying silent", () => {
    for (const value of ["true", 1, null, {}]) {
      expect(normalizeSession({ initHintNever: value as never }).initHintNever).toBe(false);
    }
  });
});
