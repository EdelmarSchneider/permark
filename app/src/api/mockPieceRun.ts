// What a plugin that runs pieces answers in the mock backend: its `status` | `run` | `cancel` and its news
// `<plugin>:run`. Timer-driven runs (`stepMs` per phase): mock engine not installed (mock local-ai `engineInstalled`)
// -> `engine` 0 -> 50 -> 100 % of its bytes first (then installed, as `ModelHost::open`), loading 0 -> 50 -> 100 %,
// one `piece` per piece (the plugin's `answer` for its text and what the run was sent beside its id + pieces: a
// target language, …; pieces over `TOO_LONG` chars -> `discarded` tooLong), then `done` | `probe.failRun`. A new run
// stops the old one (cancelled), as the plugins. Status: `probe.available` when set, else whether mock local-ai has a
// model on the plugin's job.

import type { PiecePlugin } from "../plugins/local-ai/pieces/review.svelte";
import type { AiTask } from "./generated/AiTask";
import type { PieceRunPhase } from "./generated/PieceRunPhase";
import type { PieceRunPiece } from "./generated/PieceRunPiece";
import type { IpcError } from "./generated/IpcError";
import { MOCK_ENGINE_BYTES } from "./mockLocalAi";
import type { AppPluginEventName, AppPluginEvents, PluginMethods } from "./types";

/** Pieces longer than this are "too long for the model". */
const TOO_LONG = 2000;

/** A plugin whose run takes its id + pieces alone. */
type NoRunArgs = Record<never, never>;

/** What a run is sent: its id, its pieces and what the plugin's run takes beside them (`A`). */
type Sent<A> = { run: string; pieces: PieceRunPiece[] } & A;

export type PieceRunProbe<A = NoRunArgs> = {
  /** Calls made: `status`, `run <id> <pieces>`, `cancel <id>`. */
  calls: string[];
  /** What each run was sent, in order. */
  runs: Sent<A>[];
  /** Time of one run phase. Read when a run starts. */
  stepMs: number;
  /** The `status` answer (`null`: a model on the plugin's job in mock local-ai). */
  available: boolean | null;
  /** The next run fails with this after loading (`null`: it succeeds). */
  failRun: IpcError | null;
  /** Stops the run timer (tests). */
  stopAll(): void;
};

/** What a mock piece run takes of the mock backend around it (the same for every plugin). */
export type PieceRunWiring = {
  emit: <E extends AppPluginEventName>(event: E, payload: AppPluginEvents[E]) => void;
  /** Mock local-ai has a model on job `task`. */
  assigned: (task: AiTask) => boolean;
  /** Mock local-ai's engine files (a run gets them when missing). */
  engine: { engineInstalled: boolean };
};

/** The methods of plugin `P` a mock piece run serves, under the plugin's names. */
export type PieceRunHandlers<P extends PiecePlugin> = {
  [M in `${P}.status` | `${P}.run` | `${P}.cancel`]: (
    args: PluginMethods[M]["args"],
  ) => PluginMethods[M]["result"];
};

/** The mock backend of `plugin`: runs on the model of job `task`, `answer` = the mock model's output for a piece's
 *  text in a run that was sent `sent` (what the plugin's run takes beside its id + pieces: `A`). */
export function createMockPieceRun<A = NoRunArgs>(
  plugin: PiecePlugin,
  task: AiTask,
  answer: (text: string, sent: A) => string,
  { emit, assigned, engine }: PieceRunWiring,
) {
  let current: { run: string; next: PieceRunPhase[] } | null = null;
  let timer: ReturnType<typeof setInterval> | null = null;
  const stopTimer = (): void => {
    if (timer !== null) clearInterval(timer);
    timer = null;
  };
  const probe: PieceRunProbe<A> = {
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
    emit(`${plugin}:run`, { run, phase });
  }

  function stop(): void {
    if (current === null) return;
    const run = current.run;
    stopTimer();
    current = null;
    emit(`${plugin}:run`, { run, phase: { kind: "cancelled" } });
  }

  function start(sent: Sent<A>): null {
    const { run, pieces } = sent;
    stop();
    const fetch = engine.engineInstalled
      ? []
      : [0, 0.5, 1].map((part): PieceRunPhase => ({
          kind: "engine",
          receivedBytes: Math.round(MOCK_ENGINE_BYTES * part),
          totalBytes: MOCK_ENGINE_BYTES,
        }));
    const loading = [0, 0.5, 1].map((progress): PieceRunPhase => ({ kind: "loading", progress }));
    const failure = probe.failRun;
    probe.failRun = null;
    const answers = pieces.map(({ id, text }): PieceRunPhase => ({
      kind: "piece",
      id,
      result:
        text.length > TOO_LONG
          ? { kind: "discarded", reason: "tooLong" }
          : { kind: "output", text: answer(text, sent) },
    }));
    const end: PieceRunPhase[] =
      failure === null ? [...answers, { kind: "done" }] : [{ kind: "failed", error: failure }];
    current = { run, next: [...fetch, ...loading, ...end] };
    timer = setInterval(step, probe.stepMs);
    return null;
  }

  return {
    status: (): { available: boolean } => {
      probe.calls.push("status");
      return { available: probe.available ?? assigned(task) };
    },
    run: (sent: Sent<A>): null => {
      probe.calls.push(`run ${sent.run} ${sent.pieces.length}`);
      probe.runs.push(structuredClone(sent));
      return start(sent);
    },
    cancel: ({ run }: { run: string }): null => {
      probe.calls.push(`cancel ${run}`);
      if (current?.run === run) stop();
      return null;
    },
    probe,
  };
}
