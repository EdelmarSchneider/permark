// The description being typed in the tag editor. It belongs to the document the editor was opened on:
// whatever the reader does meanwhile (back, forward, a link), a save goes to that document.

export class DescriptionDraft {
  private saved: string;

  /** `saved` is the description the document has now; `write` stores one for a document. */
  constructor(
    readonly path: string,
    saved: string,
    private readonly write: (path: string, text: string) => void,
  ) {
    this.saved = saved;
  }

  /** Saves `text` to this draft's document when it differs from what is saved. Returns whether it did. */
  flush(text: string, canWrite: boolean): boolean {
    if (!canWrite || text === this.saved) return false;
    this.saved = text;
    this.write(this.path, text);
    return true;
  }

  /** The stored description changed elsewhere (an agent edited meta.toml): what is saved is that now. */
  adopt(text: string): void {
    this.saved = text;
  }
}
