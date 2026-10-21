// Verifier (adversarial): two project tabs = two views; every view hears every `import-progress` (emit_to does not
// narrow `listen` with the default Any target). Plan ids restart per project, so both views can run plan 1: the view
// label in the payload tells them apart.
import { describe, expect, it } from "vitest";
import type { Api, ImportProgressEvent } from "../../api";
import { createMockApi } from "../../api/mock";
import { ImportFlow, type ImportState } from "./importFlow.svelte";

/** The state now (a read after an await: no narrowing carried over). */
const now = (flow: ImportFlow): ImportState => flow.state;

function bus(): { on: Api["on"]; send: (p: ImportProgressEvent) => void } {
  const handlers = new Set<(p: ImportProgressEvent) => void>();
  return {
    on: ((event: string, handler: (p: ImportProgressEvent) => void) => {
      if (event === "import-progress") handlers.add(handler);
      return Promise.resolve(() => handlers.delete(handler));
    }) as Api["on"],
    send: (p) => handlers.forEach((h) => h(p)),
  };
}

function view(on: Api["on"], label: string): ImportFlow {
  const mock = createMockApi(); // own project: own plan numbering, starts at 1
  const api: Api = { ...mock, on, applyImport: () => new Promise(() => undefined) };
  return new ImportFlow(api, {
    fail: () => undefined,
    note: () => undefined,
    action: () => undefined,
    project: () => "p",
    view: () => label,
    shown: () => true,
    focus: () => undefined,
    end: () => undefined,
    placed: () => undefined,
    undo: () => Promise.resolve(),
  });
}

describe("verifier: import progress scope", () => {
  it("a view ignores another project's import progress with the same plan id", async () => {
    const shared = bus();
    const a = view(shared.on, "view-a");
    const b = view(shared.on, "view-b");
    await a.start();
    await b.start();
    await a.plan("", ["C:/outside/a.md"], { x: 1, y: 1 });
    await b.plan("", ["C:/outside/b.md"], { x: 1, y: 1 });
    if (a.state.status !== "asking" || b.state.status !== "asking") throw new Error("not asking");
    expect(a.state.plan.planId).toBe(b.state.plan.planId); // the collision
    void a.confirm("copy");
    void b.confirm("copy");
    // project A's backend reports its import (emit_to A, heard by both views)
    shared.send({
      window: "view-a",
      progress: {
        planId: ((s) => (s.status === "running" ? s.plan.planId : -1))(now(a)),
        phase: "copying",
        itemsDone: 7,
        itemsTotal: 9,
        filesDone: 7,
        filesTotal: 9,
        bytesDone: 700,
        bytesTotal: 900,
        item: "A-only.md",
      },
    });
    const [sa, sb] = [now(a), now(b)];
    expect(sa.status === "running" && sa.progress?.item).toBe("A-only.md");
    expect(sb.status === "running" ? sb.progress : "not running").toBeNull();
  });
});
