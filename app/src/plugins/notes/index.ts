// Plugin `notes`: the Notes window. Plain Markdown notes that save themselves, grouped by time, with pins, earlier
// versions, crash recovery, loose files, sharing with agents and moving into a project. Turned off, the welcome screen
// shows no notes and the Notes cannot be opened.

import { NO_REQUIREMENT, type Requirement } from "../../core/conditions/available";
import { definePlugin } from "../../core/plugins/registry.svelte";
import { windowStore } from "../../core/window.svelte";
import { notes } from "./store.svelte";

const IN_NOTES: Requirement = { needs: [{ need: "workspace", kinds: ["notes"] }], prefers: [] };

export default definePlugin({
  manifest: { id: "notes", kind: "feature", dependsOn: [] },
  activate(cx) {
    const removals = [
      cx.contribute({
        kind: "workspaceView",
        workspace: "notes",
        requires: NO_REQUIREMENT,
        sidebar: () => import("./NotesSidebar.svelte"),
        main: () => import("./NotesMain.svelte"),
      }),
      cx.contribute({
        kind: "welcomeSection",
        id: "notes.welcome",
        order: 10,
        requires: NO_REQUIREMENT,
        view: () => import("./WelcomeNotes.svelte"),
      }),
      cx.contribute({
        kind: "settingsSection",
        id: "notes.settings",
        label: "notes.title",
        requires: NO_REQUIREMENT,
        view: () => import("./NotesSettings.svelte"),
      }),
      cx.contribute({
        kind: "statusItem",
        id: "notes.saved",
        slot: "right",
        order: 9,
        requires: IN_NOTES,
        view: () => import("./SaveStatus.svelte"),
      }),
      cx.contribute({
        kind: "command",
        id: "notes.open",
        label: "cmd.notesOpen",
        requires: NO_REQUIREMENT,
        run: () => windowStore.openNotes("ask"),
      }),
      cx.contribute({
        kind: "command",
        id: "notes.new",
        label: "cmd.notesNew",
        requires: IN_NOTES,
        run: () => notes.newNote(),
      }),
      cx.contribute({
        kind: "shortcut",
        keys: "Mod+n",
        command: "notes.new",
        requires: IN_NOTES,
      }),
      cx.contribute({
        kind: "command",
        id: "notes.next",
        label: "cmd.notesNext",
        requires: IN_NOTES,
        run: () => notes.step(1),
      }),
      cx.contribute({
        kind: "shortcut",
        keys: "Mod+Alt+ArrowDown",
        command: "notes.next",
        requires: IN_NOTES,
      }),
      cx.contribute({
        kind: "command",
        id: "notes.previous",
        label: "cmd.notesPrevious",
        requires: IN_NOTES,
        run: () => notes.step(-1),
      }),
      cx.contribute({
        kind: "shortcut",
        keys: "Mod+Alt+ArrowUp",
        command: "notes.previous",
        requires: IN_NOTES,
      }),
      cx.contribute({
        kind: "command",
        id: "notes.copy",
        label: "cmd.notesCopy",
        requires: IN_NOTES,
        when: () => notes.current !== null,
        run: () => (notes.current === null ? undefined : notes.copyNote(notes.current)),
      }),
      cx.contribute({
        kind: "shortcut",
        keys: "Mod+Shift+c",
        command: "notes.copy",
        requires: IN_NOTES,
      }),
      cx.contribute({
        kind: "command",
        id: "notes.versions",
        label: "cmd.notesVersions",
        requires: IN_NOTES,
        when: () => notes.entry?.kind === "note",
        run: () => (notes.current === null ? undefined : notes.openVersions(notes.current)),
      }),
      cx.contribute({
        kind: "command",
        id: "notes.openFile",
        label: "cmd.notesOpenFile",
        requires: IN_NOTES,
        run: () => notes.chooseFile(),
      }),
      ...[1, 2, 3, 4, 5, 6, 7, 8, 9].flatMap((n) => [
        cx.contribute({
          kind: "command",
          id: `notes.pinned${n}`,
          label: "cmd.notesPinned",
          requires: IN_NOTES,
          inPalette: false,
          run: () => notes.openPinned(n),
        }),
        cx.contribute({
          kind: "shortcut",
          keys: `Mod+${n}`,
          command: `notes.pinned${n}`,
          requires: IN_NOTES,
        }),
      ]),
    ];
    return () => {
      for (const remove of removals) remove();
    };
  },
});
