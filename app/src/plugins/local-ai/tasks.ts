// TS mirror of `permark-plugin-local-ai::tasks` (same table; shared fixture `tasks.json` keeps them equal).

import type { AiTask } from "../../api/generated/AiTask";
import type { ModelCapability } from "../../api/generated/ModelCapability";

export type { AiTask, ModelCapability };

/** Every task, in the order the settings page lists them. */
export const TASKS: readonly AiTask[] = [
  "correction",
  "translation",
  "semanticIndex",
  "readAloud",
  "dictation",
  "assistant",
];

/** What a model needs per task: alternatives (any one), each a set of capabilities (all of them). */
export const TASK_NEEDS: Readonly<Record<AiTask, readonly (readonly ModelCapability[])[]>> = {
  correction: [["chat"]],
  translation: [["translate"], ["chat"]],
  semanticIndex: [["embed"]],
  readAloud: [["speak"]],
  dictation: [["transcribe"]],
  assistant: [["chat", "tools"]],
};

/** Whether a model with `capabilities` can take `task`. */
export function accepts(task: AiTask, capabilities: readonly ModelCapability[]): boolean {
  return TASK_NEEDS[task].some((set) => set.every((need) => capabilities.includes(need)));
}

/** The tasks a model with `capabilities` can take, in list order. */
export function tasksFor(capabilities: readonly ModelCapability[]): AiTask[] {
  return TASKS.filter((task) => accepts(task, capabilities));
}
