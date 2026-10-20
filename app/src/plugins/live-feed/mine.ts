// What the panel says about one of the person's own actions (from the project's activity journal). Pure.

import type { ActivityEntry, ActivityFact, FeedItem } from "../../api";
import type { MessageKey, Params } from "../../core/i18n";

/** Which rows the Changes panel lists. */
export type FeedShow = "all" | "mine" | "outside";

export type MineLabel = {
  key: MessageKey;
  params?: Params;
  /** Sections | the section | where a result went. */
  detail?: string;
  /** Done by an undo. */
  undone?: boolean;
};

function name(path: string): string {
  return path.split("/").at(-1) ?? path;
}

/** The words of an action; `null` for what is not an action of the person (changes outside, marks). */
export function mineLabel(fact: ActivityFact): MineLabel | null {
  switch (fact.kind) {
    case "file": {
      const undone = fact.undo === true;
      switch (fact.op) {
        case "new":
          return { key: "feed.mine.new", undone };
        case "delete":
          return { key: "feed.mine.deleted", undone };
        case "move":
          return { key: "feed.mine.moved", params: { name: name(fact.from ?? "") }, undone };
        case "import":
          return { key: "feed.mine.imported", undone };
        case "restore":
          return { key: "feed.mine.restored", undone };
        default:
          return fact satisfies never;
      }
    }
    case "save":
      return { key: "feed.mine.saved", detail: fact.sections.join(" · ").replaceAll(" > ", " › ") };
    case "convert":
      return {
        key: "feed.mine.converted",
        params: { format: fact.to.toUpperCase() },
        ...(fact.saved ? { detail: name(fact.saved) } : {}),
      };
    case "comment": {
      const keys = {
        add: "feed.mine.commentAdd",
        reply: "feed.mine.commentReply",
        done: "feed.mine.commentDone",
        reopen: "feed.mine.commentReopen",
        remove: "feed.mine.commentRemove",
      } as const satisfies Record<typeof fact.op, MessageKey>;
      return {
        key: keys[fact.op],
        ...(fact.heading ? { detail: fact.heading.replaceAll(" > ", " › ") } : {}),
      };
    }
    case "tag":
      return {
        key: fact.added ? "feed.mine.tagAdded" : "feed.mine.tagRemoved",
        params: { tag: fact.tag },
      };
    case "review":
      return {
        key: fact.op === "keep" ? "feed.mine.kept" : "feed.mine.reverted",
        ...(fact.section ? { detail: fact.section.replaceAll(" > ", " › ") } : {}),
      };
    case "outside":
    case "handOff":
    case "closed":
      return null;
    default:
      return fact satisfies never;
  }
}

/** The path an action is about (none for marks). */
export function pathOf(fact: ActivityFact): string | null {
  switch (fact.kind) {
    case "file":
    case "save":
    case "convert":
    case "comment":
    case "tag":
    case "review":
    case "outside":
      return fact.path;
    case "handOff":
    case "closed":
      return null;
    default:
      return fact satisfies never;
  }
}

/** One row of the panel: a change the disk reported, or one of the person's actions | a hand-off mark. */
export type PanelRow =
  | { kind: "feed"; key: string; at: number; item: FeedItem }
  | { kind: "mine"; key: string; at: number; entry: ActivityEntry };

/** A feed item that is the person's own doing (the journal says it better). */
function own(item: FeedItem): boolean {
  switch (item.item) {
    case "entry":
    case "group":
      return item.own;
    case "resync":
      return false;
    default:
      return item satisfies never;
  }
}

/** The rows a filter shows, newest first: `outside` = what the disk reported that the person did not do; `mine` = the
 * person's actions and the hand-offs; `all` = both (the person's own disk changes only once, as actions). */
export function panelRows(
  feed: readonly FeedItem[],
  mine: readonly ActivityEntry[],
  show: FeedShow,
): PanelRow[] {
  const rows: PanelRow[] = [];
  if (show !== "mine") {
    for (const item of feed) {
      if (!own(item)) rows.push({ kind: "feed", key: `f${String(item.id)}`, at: item.atMs, item });
    }
  }
  if (show !== "outside") {
    for (const entry of mine) {
      if (entry.fact.kind === "handOff" || mineLabel(entry.fact) !== null) {
        rows.push({ kind: "mine", key: `m${String(entry.seq)}`, at: entry.at, entry });
      }
    }
  }
  return rows.sort((a, b) => b.at - a.at);
}
