// Lifecycle of a document being edited: one pure transition function `(state, event) -> { state, effects }`.
// Effects are run by the editing session (`session.svelte.ts`); the text itself lives in the editor, the machine
// holds only the version the edits are based on. A pair not listed keeps the state and does nothing.

/** The disk version the edits are based on: its hash and its edit text. */
export type Base = { hash: string; text: string };

export type SavePolicy = { kind: "explicit" } | { kind: "auto"; delayMs: number };

/** Where the person goes once the document is let go. */
export type Leave =
  | { kind: "view" }
  | { kind: "document"; path: string }
  | { kind: "workspace" }
  | { kind: "window" }
  | { kind: "open"; path: string };

/** A state the unsaved prompt can go back to. */
export type Unsaved =
  | { s: "dirty"; base: Base }
  | { s: "conflict"; base: Base; disk: string | null }
  | { s: "saveError"; base: Base; error: string };

export type DocState =
  | { s: "loading" }
  | { s: "clean"; base: Base }
  | Unsaved
  | {
      s: "saving";
      base: Base;
      /** Where to go once saved (the prompt's "Save"). */
      then: Leave | null;
      /** Edits made while the save runs: still dirty afterwards. */
      editedSince: boolean;
    }
  | { s: "unsavedPrompt"; back: Unsaved; leave: Leave }
  | { s: "closed" };

export type DocEvent =
  | { e: "loaded"; base: Base }
  | { e: "edited" }
  /** The text is the base text again (undo back, typed and deleted). */
  | { e: "reverted" }
  | { e: "save" }
  | { e: "autosaveDue" }
  | { e: "saved"; base: Base }
  | { e: "savedAs"; path: string; sameFile: boolean }
  | { e: "conflict"; disk: string | null }
  | { e: "saveFailed"; error: string }
  | { e: "retry" }
  | { e: "diskChanged"; hash: string }
  | { e: "diskDeleted" }
  | { e: "reloadTheirs" }
  | { e: "keepMine" }
  | { e: "saveAsNew" }
  | { e: "leave"; leave: Leave }
  | { e: "discard" }
  | { e: "keepEditing" };

export type Effect =
  /** Save the editor's text: line edits against the base (`against: "base"`) or against the disk's version. */
  | { type: "save"; against: "base" | "disk" }
  /** Write the editor's text as a new file (`exact`: at the document's own path, which is free). */
  | { type: "saveAs"; exact: boolean }
  /** Read the document again (place kept). */
  | { type: "load" }
  /** Read what the disk holds (conflict view). */
  | { type: "fetchDisk" }
  | { type: "leave"; leave: Leave }
  | { type: "setUnsaved"; unsaved: boolean }
  | { type: "scheduleAutosave"; delayMs: number }
  | { type: "cancelAutosave" };

export type Step = { state: DocState; effects: Effect[] };

const stay = (state: DocState): Step => ({ state, effects: [] });

function schedule(policy: SavePolicy): Effect[] {
  return policy.kind === "auto" ? [{ type: "scheduleAutosave", delayMs: policy.delayMs }] : [];
}

/** Whether the state holds edits not on disk. */
export function isUnsaved(state: DocState): boolean {
  switch (state.s) {
    case "dirty":
    case "conflict":
    case "saveError":
    case "saving":
    case "unsavedPrompt":
      return true;
    case "loading":
    case "clean":
    case "closed":
      return false;
    default:
      return state satisfies never;
  }
}

function startSave(base: Base, then: Leave | null): Step {
  return {
    state: { s: "saving", base, then, editedSince: false },
    effects: [{ type: "cancelAutosave" }, { type: "save", against: "base" }],
  };
}

function toConflict(base: Base, disk: string | null): Step {
  return {
    state: { s: "conflict", base, disk },
    effects: [{ type: "cancelAutosave" }, { type: "fetchDisk" }],
  };
}

export function transition(state: DocState, event: DocEvent, policy: SavePolicy): Step {
  switch (state.s) {
    case "loading":
      switch (event.e) {
        case "loaded":
          return {
            state: { s: "clean", base: event.base },
            effects: [{ type: "setUnsaved", unsaved: false }],
          };
        case "leave":
          return { state: { s: "closed" }, effects: [{ type: "leave", leave: event.leave }] };
        default:
          return stay(state);
      }

    case "clean":
      switch (event.e) {
        case "edited":
          return {
            state: { s: "dirty", base: state.base },
            effects: [{ type: "setUnsaved", unsaved: true }, ...schedule(policy)],
          };
        case "diskChanged":
          return event.hash === state.base.hash
            ? stay(state)
            : { state: { s: "loading" }, effects: [{ type: "load" }] };
        case "diskDeleted":
          return { state: { s: "closed" }, effects: [{ type: "leave", leave: { kind: "view" } }] };
        case "leave":
          return { state: { s: "closed" }, effects: [{ type: "leave", leave: event.leave }] };
        case "loaded":
          return stay({ s: "clean", base: event.base });
        default:
          return stay(state);
      }

    case "dirty":
      switch (event.e) {
        case "edited":
          return { state, effects: schedule(policy) };
        case "reverted":
          return {
            state: { s: "clean", base: state.base },
            effects: [{ type: "cancelAutosave" }, { type: "setUnsaved", unsaved: false }],
          };
        case "save":
        case "autosaveDue":
          return startSave(state.base, null);
        case "diskChanged":
          return event.hash === state.base.hash ? stay(state) : toConflict(state.base, event.hash);
        case "diskDeleted":
          return toConflict(state.base, null);
        case "leave":
          return policy.kind === "auto"
            ? startSave(state.base, event.leave)
            : {
                state: { s: "unsavedPrompt", back: state, leave: event.leave },
                effects: [{ type: "cancelAutosave" }],
              };
        default:
          return stay(state);
      }

    case "saving":
      switch (event.e) {
        case "edited":
        case "reverted":
          return stay({ ...state, editedSince: true });
        case "saved": {
          if (state.then !== null) {
            if (state.editedSince) {
              return {
                state: { s: "saving", base: event.base, then: state.then, editedSince: false },
                effects: [{ type: "save", against: "base" }],
              };
            }
            return {
              state: { s: "closed" },
              effects: [
                { type: "setUnsaved", unsaved: false },
                { type: "leave", leave: state.then },
              ],
            };
          }
          return state.editedSince
            ? { state: { s: "dirty", base: event.base }, effects: schedule(policy) }
            : {
                state: { s: "clean", base: event.base },
                effects: [{ type: "setUnsaved", unsaved: false }],
              };
        }
        case "savedAs":
          return event.sameFile
            ? { state: { s: "loading" }, effects: [{ type: "load" }] }
            : {
                state: { s: "closed" },
                effects: [
                  { type: "setUnsaved", unsaved: false },
                  { type: "leave", leave: { kind: "open", path: event.path } },
                ],
              };
        case "conflict":
          return toConflict(state.base, event.disk);
        case "saveFailed":
          return stay({ s: "saveError", base: state.base, error: event.error });
        case "leave":
          return stay({ ...state, then: event.leave });
        default:
          return stay(state);
      }

    case "conflict":
      switch (event.e) {
        case "reloadTheirs":
          return {
            state: { s: "loading" },
            effects: [{ type: "setUnsaved", unsaved: false }, { type: "load" }],
          };
        case "keepMine":
          return state.disk === null
            ? {
                state: { s: "saving", base: state.base, then: null, editedSince: false },
                effects: [{ type: "saveAs", exact: true }],
              }
            : {
                state: { s: "saving", base: state.base, then: null, editedSince: false },
                effects: [{ type: "save", against: "disk" }],
              };
        case "saveAsNew":
          return {
            state: { s: "saving", base: state.base, then: null, editedSince: false },
            effects: [{ type: "saveAs", exact: false }],
          };
        case "diskChanged":
          return event.hash === state.disk
            ? stay(state)
            : { state: { ...state, disk: event.hash }, effects: [{ type: "fetchDisk" }] };
        case "diskDeleted":
          return state.disk === null ? stay(state) : stay({ ...state, disk: null });
        case "leave":
          return stay({ s: "unsavedPrompt", back: state, leave: event.leave });
        default:
          return stay(state);
      }

    case "saveError":
      switch (event.e) {
        case "retry":
        case "save":
          return startSave(state.base, null);
        case "diskChanged":
          return event.hash === state.base.hash ? stay(state) : toConflict(state.base, event.hash);
        case "diskDeleted":
          return toConflict(state.base, null);
        case "leave":
          return stay({ s: "unsavedPrompt", back: state, leave: event.leave });
        default:
          return stay(state);
      }

    case "unsavedPrompt":
      switch (event.e) {
        case "keepEditing":
          return { state: state.back, effects: schedule(policy) };
        case "discard":
          return {
            state: { s: "closed" },
            effects: [
              { type: "cancelAutosave" },
              { type: "setUnsaved", unsaved: false },
              { type: "leave", leave: state.leave },
            ],
          };
        case "save":
          return state.back.s === "conflict"
            ? stay(state)
            : startSave(state.back.base, state.leave);
        default:
          return stay(state);
      }

    case "closed":
      return stay(state);

    default:
      return state satisfies never;
  }
}
