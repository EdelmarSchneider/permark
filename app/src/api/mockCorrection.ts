// What the correction plugin answers in the mock backend: a mock piece run (`mockPieceRun.ts`) on the model of job
// "correction" whose answer is a fixed "correction": common English misspellings fixed, case kept.

import { createMockPieceRun, type PieceRunHandlers, type PieceRunWiring } from "./mockPieceRun";

/** Misspelling -> fix (lowercase); the mock model's whole knowledge. */
const FIXES: ReadonlyMap<string, string> = new Map([
  ["teh", "the"],
  ["recieve", "receive"],
  ["seperate", "separate"],
  ["definately", "definitely"],
  ["occured", "occurred"],
  ["wich", "which"],
]);

/** The mock model's answer for `text`: the known misspellings fixed, first letter's case kept. */
export function mockCorrect(text: string): string {
  return text.replace(/\p{L}+/gu, (word) => {
    const fix = FIXES.get(word.toLowerCase());
    if (fix === undefined) return word;
    const first = word.charAt(0);
    const upper = first !== first.toLowerCase();
    return upper ? fix.charAt(0).toUpperCase() + fix.slice(1) : fix;
  });
}

export function createMockCorrection(wiring: PieceRunWiring) {
  const { status, run, cancel, probe } = createMockPieceRun(
    "correction",
    "correction",
    mockCorrect,
    wiring,
  );
  const handlers: PieceRunHandlers<"correction"> = {
    "correction.status": status,
    "correction.run": run,
    "correction.cancel": cancel,
  };
  return { handlers, probe };
}
