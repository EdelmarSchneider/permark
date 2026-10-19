// The permission groups as the window names them (ids are the backend's).

import type { MessageKey } from "../../core/i18n";

export type GroupRow = { id: string; label: MessageKey };

export const GROUPS: readonly GroupRow[] = [
  { id: "context", label: "agent.group.context" },
  { id: "changes", label: "agent.group.changes" },
  { id: "find", label: "agent.group.find" },
  { id: "comments.read", label: "agent.group.commentsRead" },
  { id: "comments.resolve", label: "agent.group.commentsResolve" },
  { id: "notes", label: "agent.group.notes" },
  { id: "show", label: "agent.group.show" },
];
