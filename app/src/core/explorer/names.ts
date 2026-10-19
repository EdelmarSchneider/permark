// Words for what can be wrong with a name typed in the Explorer.

import { t, type MessageKey } from "../i18n";
import type { EditProblem } from "./store.svelte";

const KEYS: Record<EditProblem, MessageKey> = {
  empty: "fileOps.nameEmpty",
  chars: "fileOps.nameInvalidChars",
  dot: "fileOps.nameInvalidChars",
  edge: "fileOps.nameInvalidEnd",
  reserved: "fileOps.nameReserved",
  tooLong: "fileOps.nameTooLong",
  notMarkdown: "newFile.notMarkdown",
  exists: "fileOps.nameExists",
};

export function problemText(problem: EditProblem): string {
  return t(KEYS[problem]);
}
