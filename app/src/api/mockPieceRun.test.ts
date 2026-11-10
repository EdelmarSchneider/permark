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
  });
});
