// Feed items for tests.

import type { FeedItem } from "../../api";

type Extra = { own?: boolean; isDir?: boolean; saves?: number; atMs?: number; from?: string };

/** One feed entry. */
export function entry(
  id: number,
  path: string,
  kind: "created" | "modified" | "removed" | "renamed" = "modified",
  extra: Extra = {},
): FeedItem {
  const base = {
    item: "entry" as const,
    id,
    path,
    atMs: extra.atMs ?? 1000 + id,
    saves: extra.saves ?? 1,
    own: extra.own ?? false,
    isDir: extra.isDir ?? false,
  };
  switch (kind) {
    case "created":
      return { ...base, kind: "created" };
    case "modified":
      return { ...base, kind: "modified" };
    case "removed":
      return { ...base, kind: "removed" };
    case "renamed":
      return { ...base, kind: "renamed", from: extra.from ?? "old.md" };
    default:
      return kind satisfies never;
  }
}
