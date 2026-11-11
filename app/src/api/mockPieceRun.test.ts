// The mock piece run as the plugin makes it: its job's model, its news, its answer per piece. A whole run through
// the mock backend (engine files, loading, cancel, failure) is driven where a plugin uses it: `mockCorrection.test.ts`.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AiTask } from "./generated/AiTask";
import { createMockPieceRun } from "./mockPieceRun";

let emitted: { event: string; payload: unknown }[] = [];
let asked: AiTask[] = [];
let mock: ReturnType<typeof createMockPieceRun>;

beforeEach(() => {
  vi.useFakeTimers();
  emitted = [];
  asked = [];
  mock = createMockPieceRun("correction", "translation", (text) => text.toUpperCase(), {
    emit: (event, payload) => void emitted.push({ event, payload }),
    assigned: (task) => {
      asked.push(task);
      return task === "translation";
    },
    engine: { engineInstalled: true },
  });
  mock.probe.stepMs = 10;
});
afterEach(() => {
  mock.probe.stopAll();
  vi.useRealTimers();
});

describe("mock piece run", () => {
  it("status: a model on the plugin's job unless the probe says", () => {
    expect(mock.status()).toEqual({ available: true });
    expect(asked).toEqual(["translation"]);
    mock.probe.available = false;
    expect(mock.status()).toEqual({ available: false });
  });

  it("a run: the plugin's news, its answer for each piece, too long ones left out", async () => {
    mock.run({
      run: "r1",
      pieces: [
        { id: 0, text: "one" },
        { id: 1, text: "x".repeat(2001) },
      ],
    });
    await vi.advanceTimersByTimeAsync(100);
    expect(new Set(emitted.map((e) => e.event))).toEqual(new Set(["correction:run"]));
    expect(emitted.slice(-3).map((e) => e.payload)).toEqual([
      { run: "r1", phase: { kind: "piece", id: 0, result: { kind: "output", text: "ONE" } } },
      {
        run: "r1",
        phase: { kind: "piece", id: 1, result: { kind: "discarded", reason: "tooLong" } },
      },
      { run: "r1", phase: { kind: "done" } },
    ]);
    expect(mock.probe.calls).toEqual(["run r1 2"]);
    expect(mock.probe.runs).toEqual([
      {
        run: "r1",
        pieces: [
          { id: 0, text: "one" },
          { id: 1, text: "x".repeat(2001) },
        ],
      },
    ]);
  });

  it("what a run is sent beside its id and pieces: kept for the probe, given to the plugin's answer", async () => {
    const worded = createMockPieceRun(
      "correction",
      "translation",
      (text, sent: { to: { tag: string }; from: string | null }) => `${text} in ${sent.to.tag}`,
      {
        emit: (event, payload) => void emitted.push({ event, payload }),
        assigned: () => true,
        engine: { engineInstalled: true },
      },
    );
    worded.probe.stepMs = 10;
    const pieces = [{ id: 0, text: "one" }];
    worded.run({ run: "r1", pieces, to: { tag: "de" }, from: null });
    await vi.advanceTimersByTimeAsync(100);
    worded.run({ run: "r2", pieces, to: { tag: "pt-BR" }, from: "en" });
    await vi.advanceTimersByTimeAsync(100);
    worded.probe.stopAll();
    expect(worded.probe.runs).toEqual([
      { run: "r1", pieces, to: { tag: "de" }, from: null },
      { run: "r2", pieces, to: { tag: "pt-BR" }, from: "en" },
    ]);
    const outputs = emitted.flatMap(
      ({ payload }) => JSON.stringify(payload).match(/one in [^"]+/g) ?? [],
    );
    expect(outputs).toEqual(["one in de", "one in pt-BR"]);
  });
});
