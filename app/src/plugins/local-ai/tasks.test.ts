import { describe, expect, it } from "vitest";
import type { AiTaskInfo } from "../../api/generated/AiTaskInfo";
import { createMockLocalAi } from "../../api/mockLocalAi";
import fixture from "./tasks.json";
import { TASK_NEEDS, TASKS, tasksFor, type AiTask, type ModelCapability } from "./tasks";

type Case = { name: string; capabilities: ModelCapability[]; fits: AiTask[] };

// JSON import is typed structurally; the shapes are the generated ones (the Rust test parses the same file strictly).
const tasks = fixture.tasks as AiTaskInfo[];
const cases = fixture.cases as Case[];

describe("local AI tasks (shared fixture with Rust)", () => {
  it("lists the same tasks with the same needs, in order", () => {
    expect(tasks.map((t) => t.task)).toEqual(TASKS);
    for (const { task, needs } of tasks) expect(TASK_NEEDS[task], task).toEqual(needs);
  });

  it("offers a model only the tasks its capabilities cover", () => {
    expect(cases.length).toBeGreaterThanOrEqual(8);
    for (const c of cases) expect(tasksFor(c.capabilities), c.name).toEqual(c.fits);
  });
});

describe("local AI mock backend", () => {
  it("answers the task list and a catalog whose models fit some task", () => {
    const { handlers } = createMockLocalAi();
    expect(handlers["local-ai.tasks"]({})).toEqual(tasks);
    const models = handlers["local-ai.catalog"]({}).models;
    expect(models.length).toBeGreaterThan(0);
    for (const m of models) {
      expect(m.repo.startsWith(`${m.publisher}/`), m.id).toBe(true);
      expect(tasksFor(m.capabilities).length, m.id).toBeGreaterThan(0);
    }
  });
});
