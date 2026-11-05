import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { CorrectionRun } from "./generated/CorrectionRun";
import { createMockApi } from "./mock";
import { mockCorrect } from "./mockCorrection";

let mock: ReturnType<typeof createMockApi>;
let news: CorrectionRun[] = [];

beforeEach(async () => {
  vi.useFakeTimers();
  mock = createMockApi();
  mock.probe.correction.stepMs = 10;
  news = [];
  await mock.onAppPlugin("correction:run", (n) => void news.push(n));
});
afterEach(() => {
  mock.probe.correction.stopAll();
  vi.useRealTimers();
});

const phases = (run: string): string[] =>
  news.filter((n) => n.run === run).map((n) => n.phase.kind);

async function status(): Promise<boolean | null> {
  const answer = await mock.pluginCall("correction.status", {});
  return answer.ok ? answer.value.available : null;
}

describe("mock correction", () => {
  it("fixes the known misspellings, case kept, markers untouched", () => {
    expect(mockCorrect("Teh cat will recieve ⟦1⟧ teh mail.")).toBe(
      "The cat will receive ⟦1⟧ the mail.",
    );
  });

  it("status follows the Local AI assignment unless the probe says", async () => {
    expect(await status()).toBe(true);
    await mock.pluginCall("local-ai.assign", { task: "correction", model: null });
    expect(await status()).toBe(false);
    mock.probe.correction.available = true;
    expect(await status()).toBe(true);
  });

  it("a run: loading, one answer per piece, done", async () => {
    await mock.pluginCall("correction.run", {
      run: "r1",
      pieces: [
        { id: 0, text: "teh end" },
        { id: 1, text: "x".repeat(2001) },
      ],
    });
    await vi.advanceTimersByTimeAsync(100);
    expect(phases("r1")).toEqual(["loading", "loading", "loading", "piece", "piece", "done"]);
    expect(news[3]?.phase).toEqual({
      kind: "piece",
      id: 0,
      result: { kind: "output", text: "the end" },
    });
    expect(news[4]?.phase).toEqual({
      kind: "piece",
      id: 1,
      result: { kind: "discarded", reason: "tooLong" },
    });
  });

  it("cancel | a new run stops the old one; a failure ends it", async () => {
    const run = (id: string) =>
      mock.pluginCall("correction.run", { run: id, pieces: [{ id: 0, text: "teh" }] });
    await run("r1");
    await run("r2");
    expect(phases("r1")).toEqual(["cancelled"]);
    await mock.pluginCall("correction.cancel", { run: "r2" });
    expect(phases("r2")).toEqual(["cancelled"]);
    mock.probe.correction.failRun = { kind: "unavailable", subject: "model", detail: "noModel" };
    await run("r3");
    await vi.advanceTimersByTimeAsync(100);
    expect(phases("r3")).toEqual(["loading", "loading", "loading", "failed"]);
    expect(mock.probe.correction.calls).toEqual(["run r1 1", "run r2 1", "cancel r2", "run r3 1"]);
  });
});
