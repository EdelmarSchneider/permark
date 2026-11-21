<script lang="ts">
  // Edit mode of a config file: the sheet with values that take typing. Every change goes to the backend, which
  // replaces only that value in the text and answers with the new text and sheet; the editing session compares the
  // text with the version on disk (unsaved), and saving writes only the changed lines through the usual path
  // (hash check, conflicts, line breaks).
  import { tick } from "svelte";
  import { api, type Body, type ConfigEdit, type NewValue } from "../../api";
  import type { Content, EditingSession } from "../editing/session.svelte";
  import { t } from "../i18n";
  import ConfigSheet from "./ConfigSheet.svelte";
  import { Matches } from "./matches.svelte";

  type Props = {
    session: EditingSession;
    content: Content;
  };
  let { session, content }: Props = $props();

  /** The text of the editor now (LF). Not reactive: the sheet is. */
  let text = "";
  let body = $state.raw<Body | null>(null);
  let failed = $state(false);
  const sheet = $derived(body?.kind === "sheet" ? body.sheet : null);
  const matches = new Matches();
  let shown = $state<{
    locate(line: number, asked: boolean): Promise<void>;
    topLine(): number | null;
  }>();
  /** A line asked for before the sheet was drawn: shown once it is. */
  let asked: number | null = null;
  /** The last edit sent: the next one waits for its answer, so it starts from the text that answer made (two edits
   *  sent at once would both start from the same text, and the later answer would drop the other edit). */
  let queue: Promise<unknown> = Promise.resolve();

  /** Runs of lines that differ from `base` (the save message). */
  function changedRuns(base: string, next: string): number {
    const a = base.split("\n");
    const b = next.split("\n");
    let runs = 0;
    let inRun = false;
    for (let i = 0; i < Math.max(a.length, b.length); i++) {
      const differs = a[i] !== b[i];
      if (differs && !inRun) runs += 1;
      inRun = differs;
    }
    return runs;
  }

  // The editor of this content: its text, its first sheet, its place in the session.
  $effect(() => {
    const current = content;
    text = current.text;
    body = null;
    failed = false;
    let gone = false;
    void api.configSheet(current.path, current.text).then((result) => {
      if (gone) return;
      if (result.ok && result.value.kind === "sheet") {
        body = result.value;
        if (result.value.sheet.kind === "ignore") matches.request(current.path, text, true);
        // the editor opens where it was asked to (a mode switch keeps the place; line 1 is where a sheet opens)
        const line = asked ?? current.line;
        const tinted = asked !== null;
        asked = null;
        if (line > 1 || tinted) {
          void tick().then(() => {
            if (!gone) void shown?.locate(line, tinted);
          });
        }
      } else failed = true;
    });
    const detach = session.attach(current.seq, {
      kind: "edit",
      text: () => text,
      changes: (base) => changedRuns(base, text),
      topLine: () => shown?.topLine() ?? current.line,
      showLine: (line) => {
        if (shown) void shown.locate(line, true);
        else asked = line;
      },
      selection: () => "",
      focus: () => {},
    });
    return () => {
      gone = true;
      detach();
      matches.stop();
    };
  });

  /** The word of a refusal (what the backend said, else its error kind). */
  function wordOf(error: Awaited<ReturnType<typeof api.configEdit>> & { ok: false }): string {
    return error.error.kind === "backend"
      ? (error.error.error.detail ?? error.error.error.kind)
      : "other";
  }

  function apply(edit: ConfigEdit): Promise<string | null> {
    const next = queue.then(() => send(edit));
    queue = next.catch(() => undefined);
    return next;
  }

  async function send(edit: ConfigEdit): Promise<string | null> {
    const path = content.path;
    const result = await api.configEdit(path, text, edit);
    if (content.path !== path) return null;
    if (!result.ok) return wordOf(result);
    text = result.value.text;
    body = result.value.body;
    session.changed();
    if (result.value.body.kind === "sheet" && result.value.body.sheet.kind === "ignore") {
      matches.request(path, text);
    }
    return null;
  }

  const commit = (path: string, value: NewValue): Promise<string | null> =>
    apply({ op: "set", path, value });
  const remove = (path: string): Promise<string | null> => apply({ op: "remove", path });
  const add = (after: string | null, line: string): Promise<string | null> =>
    apply({ op: "insert", after, text: line });
  const edit = (change: ConfigEdit): Promise<string | null> => apply(change);
</script>

{#if sheet}
  <ConfigSheet
    bind:this={shown}
    {sheet}
    editable={true}
    {commit}
    {remove}
    {add}
    {edit}
    source={(line) => void session.switchTo("source", line)}
    matches={sheet.kind === "ignore" ? matches.counts : null}
    capped={matches.capped}
    spread
  />
{:else if failed}
  <p class="failed" role="alert">{t("config.noSheet")}</p>
{/if}

<style>
  .failed {
    margin: var(--sp-6) var(--sp-5);
    color: var(--muted);
    text-align: center;
  }
</style>
