// The interface language as a setting: read once at boot (before the first paint, so there is no flash of the wrong
// language), changed from Settings, and kept app-wide by the backend.

import { api } from "../api";
import { errorMessage } from "../errors";
import { language, parsePreference, t, type LanguagePreference } from "../i18n";
import { rewordShellEntries } from "./shell.svelte";
import { toast } from "./toast.svelte";

/** Applies the stored choice. Never rejects: a language that cannot be read is "system". */
export async function loadLanguage(): Promise<void> {
  try {
    language.preference = parsePreference(await api.appLanguage());
  } catch {
    language.preference = "system";
  }
  language.watchSystem();
}

/** Switches the interface to `choice` now and keeps it for the next start. */
export async function chooseLanguage(choice: LanguagePreference): Promise<void> {
  language.preference = choice;
  try {
    await api.setAppLanguage(choice);
  } catch (error) {
    toast.message(t("settings.languageNotSaved", { message: errorMessage(error) }), "warn");
  }
  // the right-click entries, when they exist, are worded in the language now on screen
  await rewordShellEntries();
}
