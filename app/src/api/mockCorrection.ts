// What the correction plugin answers in the mock backend. Timer-driven runs (`stepMs` per phase): mock engine not
// installed (mock local-ai `engineInstalled`) -> `engine` 0 -> 50 -> 100 % of its bytes first (then installed, as
// `ModelHost::open`), loading 0 -> 50 -> 100 %, one `piece` per piece (a fixed "correction": common English misspellings fixed, case kept; pieces over
// `TOO_LONG` chars -> `discarded` tooLong), then `done` | `probe.failRun`. A new run stops the old one (cancelled), as
// the plugin. Status: `probe.available` when set, else whether mock local-ai has a model on job "correction".

import type { CorrectionPhase } from "./generated/CorrectionPhase";
import type { CorrectionPiece } from "./generated/CorrectionPiece";
import type { IpcError } from "./generated/IpcError";
import { MOCK_ENGINE_BYTES } from "./mockLocalAi";
import type { AppPluginEventName, AppPluginEvents, PluginMethods } from "./types";

type CorrectionMethods = Pick<
  PluginMethods,
  "correction.status" | "correction.run" | "correction.cancel"
>;

/** Misspelling -> fix (lowercase); the mock model's whole knowledge. */
const FIXES: ReadonlyMap<string, string> = new Map([
  ["teh", "the"],
  ["recieve", "receive"],
  ["seperate", "separate"],
  ["definately", "definitely"],
  ["occured", "occurred"],
  ["wich", "which"],
]);
/** Pieces longer than this are "too long for the model". */
const TOO_LONG = 2000;

export type CorrectionProbe = {
  /** Calls made: `status`, `run <id> <pieces>`, `cancel <id>`. */
  calls: string[];
  /** What each run was sent, in order. */
  runs: { run: string; pieces: CorrectionPiece[] }[];
  /** Time of one run phase. Read when a run starts. */
  stepMs: number;
  /** `correction.status` answer (`null`: a model on job "correction" in mock local-ai). */
  available: boolean | null;
  /** The next run fails with this after loading (`null`: it succeeds). */
  failRun: IpcError | null;
  /** Stops the run timer (tests). */
  stopAll(): void;
};

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

export function createMockCorrection(
  emit: <E extends AppPluginEventName>(event: E, payload: AppPluginEvents[E]) => void = () => {},
  /** Mock local-ai has a model on job "correction". */
  assigned: () => boolean = () => true,
  /** Mock local-ai's engine files (a run gets them when missing). */
  engine: { engineInstalled: boolean } = { engineInstalled: true },
) {
  let current: { run: string; next: CorrectionPhase[] } | null = null;
  let timer: ReturnType<typeof setInterval> | null = null;
  const stopTimer = (): void => {
    if (timer !== null) clearInterval(timer);
    timer = null;
  };
  const probe: CorrectionProbe = {
    calls: [],
    runs: [],
    stepMs: 300,
    available: null,
    failRun: null,
    stopAll: stopTimer,
  };

  function step(): void {
    const phase = current?.next.shift();
    if (current === null || phase === undefined) {
      stopTimer();
      return;
    }
    const run = current.run;
    if (current.next.length === 0) {
      stopTimer();
      current = null;
    }
    if (phase.kind === "engine" && phase.receivedBytes === phase.totalBytes)
      engine.engineInstalled = true;
    emit("correction:run", { run, phase });
  }

  function stop(): void {
    if (current === null) return;
    const run = current.run;
    stopTimer();
    current = null;
    emit("correction:run", { run, phase: { kind: "cancelled" } });
  }

  function start(run: string, pieces: readonly CorrectionPiece[]): null {
    stop();
    const fetch = engine.engineInstalled
      ? []
      : [0, 0.5, 1].map((part): CorrectionPhase => ({
          kind: "engine",
          receivedBytes: Math.round(MOCK_ENGINE_BYTES * part),
          totalBytes: MOCK_ENGINE_BYTES,
        }));
    const loading = [0, 0.5, 1].map((progress): CorrectionPhase => ({ kind: "loading", progress }));
    const failure = probe.failRun;
    probe.failRun = null;
    const answers = pieces.map(({ id, text }): CorrectionPhase => ({
      kind: "piece",
      id,
      result:
        text.length > TOO_LONG
          ? { kind: "discarded", reason: "tooLong" }
          : { kind: "output", text: mockCorrect(text) },
    }));
    const end: CorrectionPhase[] =
      failure === null ? [...answers, { kind: "done" }] : [{ kind: "failed", error: failure }];
    current = { run, next: [...fetch, ...loading, ...end] };
    timer = setInterval(step, probe.stepMs);
    return null;
  }

  const handlers: {
    [M in keyof CorrectionMethods]: (
      args: CorrectionMethods[M]["args"],
    ) => CorrectionMethods[M]["result"];
  } = {
    "correction.status": () => {
      probe.calls.push("status");
      return { available: probe.available ?? assigned() };
    },
    "correction.run": ({ run, pieces }) => {
      probe.calls.push(`run ${run} ${pieces.length}`);
      probe.runs.push({ run, pieces: structuredClone(pieces) });
      return start(run, pieces);
    },
    "correction.cancel": ({ run }) => {
      probe.calls.push(`cancel ${run}`);
      if (current?.run === run) stop();
      return null;
    },
  };
  return { handlers, probe };
}
