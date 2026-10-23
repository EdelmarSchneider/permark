// Long lines wrap | stop wrapping everywhere: the setting `wrapLines`, changed from the key, the palette, the reading
// menu or the settings page. The views follow at once (`look.wrap`); `settings-changed` confirms; a refusal puts them
// back and says why. Said by a quiet note (read out) that replaces the one before and closes by itself.

import { apiErrorText } from "../errors";
import { t } from "../i18n";
import { look } from "../look.svelte";
import { windowStore as store } from "../window.svelte";

/** How long the note stays (it never sits over the text for long). */
export const NOTE_MS = 4000;

/** The note of the last switch, still shown (the next one replaces it). */
let note: number | null = null;

/** Sets wrapping; `say`: a note tells what changed (the key and the palette; a switch shows it itself). */
export async function setWrap(on: boolean, say: boolean): Promise<void> {
  if (look.wrap === on && !say) return;
  look.wrap = on;
  const refused = await store.trySetting({ key: "wrapLines", value: on });
  if (refused !== null) {
    look.wrap = !on;
    tell(t("settings.notSaved", { message: apiErrorText(refused) }));
    return;
  }
  if (say) tell(t(on ? "wrap.on" : "wrap.off"));
}

/** Flips wrapping and says so. */
export function toggleWrap(): Promise<void> {
  return setWrap(!look.wrap, true);
}

function tell(text: string): void {
  if (note !== null) store.dismiss(note, "pushedOut");
  store.notifyTimed({ text, actions: [], closeAfterMs: NOTE_MS });
  note = store.notices.at(-1)?.id ?? null;
}
