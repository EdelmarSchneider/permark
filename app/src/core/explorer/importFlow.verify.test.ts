// Verifier (adversarial): the import question, rapid answers.
import { describe, expect, it } from "vitest";
import { createMockApi } from "../../api/mock";
import { ImportFlow } from "./importFlow.svelte";

describe("verifier: import flow", () => {
  it("Copy here pressed twice fast applies once", async () => {
    const mock = createMockApi();
    let applies = 0;
    const api = {
      ...mock,
      applyImport: (...args: Parameters<typeof mock.applyImport>) => {
        applies += 1;
        return mock.applyImport(...args);
      },
    };
    const flow = new ImportFlow(api, {
      fail: () => undefined,
      note: () => undefined,
      action: () => undefined,
      project: () => "demo",
      view: () => "main",
      shown: () => true,
      focus: () => undefined,
      end: () => undefined,
      placed: () => undefined,
      undo: () => Promise.resolve(),
    });
    await flow.plan("", ["C:/outside/a.md"], { x: 1, y: 1 });
    expect(flow.state.status).toBe("asking");
    const one = flow.confirm("copy");
    const two = flow.confirm("move");
    await Promise.all([one, two]);
    expect(applies).toBe(1);
  });
});
