// The numbers of the text on screen, for each tokenizer in use: the document, the document without its front matter, the
// selection. An estimator's number comes at once (counted here, marked approximate). An exact tokenizer shows the same
// kind of stand-in until the backend answers (<= 300 ms after typing stops), then the exact count replaces it.
// Call `createFigures()` while a component starts: its effects follow that component.

import { api } from "../../api";
import type { AnalysisText } from "../../core/doc/analysisText.svelte";
import { estimate } from "./estimate";
import { frontMatterEnd, lineCount, lineStart, type Figure } from "./model";
import { tokens } from "./store.svelte";
import type { TokenizerId, TokenizerInfo } from "./tokenizers";

/** Longer texts are counted by the backend only (counting here would hold the window). */
export const LOCAL_LIMIT = 200_000;
/** Typing: the exact count is asked for this long after the last key. */
const TYPING_PAUSE = 300;

/** What the backend counted for one tokenizer, valid for the text it was asked about. */
type Counted = {
  tokenizer: TokenizerId;
  text: string;
  selection: string;
  whole: number;
  body: number | null;
  part: number | null;
};

export type TokenFigures = {
  /** The whole text, front matter included. `null` = nothing to show yet (a long text still being counted). */
  whole(info: TokenizerInfo): Figure | null;
  /** The text without its front matter; `null` = the text has none (or no number yet). */
  body(info: TokenizerInfo): Figure | null;
  /** The selection; `null` = nothing selected (or no number yet). */
  selection(info: TokenizerInfo): Figure | null;
};

export function createFigures(source: AnalysisText): TokenFigures {
  let counted = $state.raw<readonly Counted[]>([]);

  const text = $derived(source.text);
  const selection = $derived(source.selection);
  const markdown = $derived(source.markdown);
  const frontEnd = $derived(markdown ? frontMatterEnd(text) : 0);
  const bodyText = $derived(frontEnd > 0 ? text.slice(lineStart(text, frontEnd + 1)) : "");
  const small = $derived(text.length <= LOCAL_LIMIT);
  /** Only the primary tokenizer shows in the bar; the others when the popover is open. */
  const wanted = $derived(tokens.popover ? tokens.chosen : tokens.chosen.slice(0, 1));

  // what the backend must count: exact tokenizers with their vocabulary, and every tokenizer for a long text
  const asked = $derived(
    wanted.filter((info) => (info.exact || !small) && tokens.ready(info)).map((info) => info.id),
  );

  $effect(() => {
    const ids = asked;
    const whole = text;
    const part = selection;
    const end = frontEnd;
    if (ids.length === 0 || (whole === "" && part === "")) return;
    let stale = false;
    const run = (): void => {
      const parts: [number, number][] | undefined =
        end > 0 ? [[end + 1, lineCount(whole)]] : undefined;
      const document =
        whole === ""
          ? null
          : api.pluginCall("token-count.count", { tokenizers: ids, text: whole, parts });
      const selected =
        part === "" ? null : api.pluginCall("token-count.count", { tokenizers: ids, text: part });
      void Promise.all([document, selected]).then(([doc, sel]) => {
        if (stale) return;
        const next: Counted[] = [];
        for (const id of ids) {
          const own = doc?.ok ? doc.value.find((r) => r.tokenizer === id) : undefined;
          const chosen = sel?.ok ? sel.value.find((r) => r.tokenizer === id) : undefined;
          if (own?.status === "ok" || chosen?.status === "ok") {
            next.push({
              tokenizer: id,
              text: whole,
              selection: part,
              whole: own?.status === "ok" ? own.whole : 0,
              body: own?.status === "ok" ? (own.parts[0] ?? null) : null,
              part: chosen?.status === "ok" ? chosen.whole : null,
            });
          }
        }
        // keep the answers for other tokenizers; the ones just made replace theirs
        counted = [...counted.filter((c) => !ids.includes(c.tokenizer)), ...next];
      });
    };
    const timer = setTimeout(run, source.editing ? TYPING_PAUSE : 0);
    return () => {
      stale = true;
      clearTimeout(timer);
    };
  });

  /** The backend's answer for this tokenizer, when it was made for the text and selection on screen. */
  function exactFor(id: TokenizerId): Counted | null {
    return (
      counted.find((c) => c.tokenizer === id && c.text === text && c.selection === selection) ??
      null
    );
  }

  return {
    whole(info) {
      const exact = exactFor(info.id);
      if (exact !== null && (info.exact || !small))
        return { value: exact.whole, approx: !info.exact };
      if (!small) return null;
      return { value: estimate(info.id, text), approx: true };
    },
    body(info) {
      if (frontEnd === 0) return null;
      const exact = exactFor(info.id);
      if (exact?.body != null && (info.exact || !small))
        return { value: exact.body, approx: !info.exact };
      if (!small) return null;
      return { value: estimate(info.id, bodyText), approx: true };
    },
    selection(info) {
      if (selection === "") return null;
      const exact = exactFor(info.id);
      if (exact?.part != null) return { value: exact.part, approx: !info.exact };
      return { value: estimate(info.id, selection), approx: true };
    },
  };
}
