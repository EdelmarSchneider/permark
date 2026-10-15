// The external editor command as a setting (Settings, "External editor"). It is app-wide and kept by the backend, which
// is also what runs it: the UI only edits the text. A command the backend can't read (an unknown placeholder, an open
// quote) is not kept; the last good one stays in force and the field says why.

import { api } from "../api";
import { errorText } from "../errors";
import { t } from "../i18n";
import { commandOf, presetOf, type EditorPresetId } from "../externalEditor";

/** The command is kept this long after the last keystroke. */
const SAVE_DELAY_MS = 400;

/** The diagnostic the backend attaches to an error (what is wrong with the command), else its message. */
function detailOf(error: unknown): string {
  if (error && typeof error === "object") {
    const detail = (error as Record<string, unknown>).detail;
    if (typeof detail === "string" && detail !== "") return detail;
  }
  return errorText(error);
}

class EditorCommandStore {
  /** The command that is in force; blank means the system's default app. */
  command = $state("");
  /** What the field shows: the text being typed, which may not be in force yet. */
  draft = $state("");
  /** "Custom" was picked in the list: the text stays as it is, whatever it matches. */
  customPicked = $state(false);
  /** Why the draft was not kept, when it was not. */
  error = $state<string | null>(null);

  /** The entry of the list: Custom when picked or typed, else the preset the text is. */
  preset = $derived<EditorPresetId>(this.customPicked ? "custom" : presetOf(this.draft));

  private timer: ReturnType<typeof setTimeout> | null = null;
  private pending: string | null = null;
  private chain: Promise<void> = Promise.resolve();

  /** Reads the saved command. The text being typed is left alone. */
  async load(): Promise<void> {
    let command: string;
    try {
      command = await api.editorCommand();
    } catch {
      return; // the last known command stays
    }
    this.command = command;
    if (this.pending === null) {
      this.draft = command;
      this.customPicked = false;
      this.error = null;
    }
  }

  /** The entry picked in the list: a preset fills the command in; Custom keeps the text and lets it be edited. */
  pick(preset: EditorPresetId): void {
    const command = commandOf(preset);
    if (command === null) {
      this.customPicked = true;
      return;
    }
    this.customPicked = false;
    this.type(command);
  }

  /** The text of the field changed. */
  type(text: string): void {
    this.customPicked = false;
    this.draft = text;
    this.pending = text;
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = setTimeout(() => void this.flush(), SAVE_DELAY_MS);
  }

  /** Keeps what was typed now (the field is closing). Resolves once the backend has answered. */
  flush(): Promise<void> {
    if (this.timer !== null) clearTimeout(this.timer);
    this.timer = null;
    const text = this.pending;
    this.pending = null;
    if (text !== null) this.chain = this.chain.then(() => this.save(text));
    return this.chain;
  }

  private async save(text: string): Promise<void> {
    try {
      await api.setEditorCommand(text);
      this.command = text.trim();
      // a newer keystroke owns the message now
      if (this.pending === null) this.error = null;
    } catch (error) {
      if (this.pending === null) {
        this.error = t("settings.editorNotSaved", { detail: detailOf(error) });
      }
    }
  }
}

export const editorCommand = new EditorCommandStore();
