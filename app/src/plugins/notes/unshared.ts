// Notes an earlier version shared with every project's agents: the update turned that sharing off (a note now goes to
// one project at a time and is never shared again without the person). The backend hands the names once; the first
// window that asks says so in one notice, naming the notes by title.

import { t, tn } from "../../core/i18n";
import type { PluginClient } from "../../core/plugins/client";
import type { WindowStore } from "../../core/window.svelte";
import { shownName } from "./model";

/** Notes named in full; the rest are counted. */
const NAMED = 3;

/** Takes the notes whose sharing the update turned off and, when there are some, says so. `openAgent`: what opens this
 *  project's agent panel (`null`: none here, such as outside a project), asked once the answer is in. */
export async function tellUnshared(
  client: PluginClient,
  window: WindowStore,
  openAgent: () => (() => void) | null,
): Promise<void> {
  const taken = await client.call("notes.takeUnshared", {});
  if (!taken.ok || taken.value.length === 0) return;
  const names = await titled(client, taken.value);
  const shown =
    names.length > NAMED
      ? tn("agent.migrated.more", names.length - NAMED, {
          names: names.slice(0, NAMED).join(", "),
        })
      : names.join(", ");
  const open = openAgent();
  window.notifyKeyed(
    "notes.unshared",
    t("agent.migrated", { names: shown }),
    { label: t("agent.migrated.ok"), run: () => {} },
    ...(open === null ? [] : [{ label: t("agent.migrated.open"), run: open }]),
  );
}

/** The notes' titles as the list shows them; a note the list no longer has keeps its file name. */
async function titled(client: PluginClient, files: readonly string[]): Promise<string[]> {
  const list = await client.call("notes.list", {});
  const entries = list.ok ? list.value.entries : [];
  return files.map((name) => {
    const entry = entries.find((e) => e.kind === "note" && e.name === name);
    return shownName(entry ?? { title: "", name });
  });
}
