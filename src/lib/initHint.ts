// When to offer "Keep tags and PerMark settings with this project?": a quiet banner for a folder that is a
// repository and has no `.permark/` yet (docs/design/v0.2.md section 13). Pure logic; the banner is Banners.svelte.

export type InitHintInput = {
  /** Simple mode (one file's folder) is not a project to keep settings with. */
  simple: boolean;
  /** The folder is a git repository. */
  isRepo: boolean;
  /** Where the project's state lives: "project" once `.permark/` exists. */
  stateLocation: "appData" | "project";
  /** "Never for this folder" was chosen (kept per project, in the app's own storage). */
  never: boolean;
  /** "Not now" was chosen for this folder since the app started. */
  snoozed: boolean;
  /** The Initialize dialog is open: it already is the question. */
  dialogOpen: boolean;
};

export function showInitHint(input: InitHintInput): boolean {
  return (
    !input.simple &&
    input.isRepo &&
    input.stateLocation === "appData" &&
    !input.never &&
    !input.snoozed &&
    !input.dialogOpen
  );
}
