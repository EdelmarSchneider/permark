// The tone of the light theme as a setting: read once at boot (before the first paint, so there is no flash of white),
// changed from Settings, and kept app-wide by the backend. Like the language, it belongs to the person and not to a
// project, so the welcome screen and the Notes window wear it too.

import { api } from "../api";
import { errorMessage } from "../errors";
import { t } from "../i18n";
import {
  applyTone,
  cacheTone,
  cachedTone,
  DEFAULT_TONE,
  normalizeTone,
  pageStorage,
  type Tone,
} from "../tone";
import { toast } from "./toast.svelte";

class ToneChoice {
  current = $state<Tone>(DEFAULT_TONE);
}

export const tone = new ToneChoice();

function show(choice: Tone): void {
  tone.current = choice;
  applyTone(document.documentElement, choice);
}

/** Paints the tone this page showed last time, at once: the backend's answer comes a moment later. */
export function showCachedTone(): void {
  show(cachedTone(pageStorage()));
}

/** Applies the stored choice. Never rejects: a tone that cannot be read is the default. */
export async function loadTone(): Promise<void> {
  let stored: Tone;
  try {
    stored = normalizeTone(await api.appTone());
  } catch {
    stored = DEFAULT_TONE;
  }
  show(stored);
  cacheTone(pageStorage(), stored);
}

/** Switches the light theme to `choice` now and keeps it for the next start. */
export async function chooseTone(choice: Tone): Promise<void> {
  show(choice);
  cacheTone(pageStorage(), choice);
  try {
    await api.setAppTone(choice);
  } catch (error) {
    toast.message(t("settings.toneNotSaved", { message: errorMessage(error) }), "warn");
  }
}
