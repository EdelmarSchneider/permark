// The commands and default keys the core itself contributes (owner `core`). Each acts through a store the window
// already has; the palette, the keys and the keyboard dialog all read them from the contribution registry.

import type { WorkspaceKind } from "../../api/generated/WorkspaceKind";
import { documents } from "../doc/document.svelte";
import { reading, WIDTHS } from "../doc/reading.svelte";
import { chooseMode, editBlockedReason, editingNow } from "../editing/choose";
import { confirmLeave } from "../editing/guard";
import { editing } from "../editing/session.svelte";
import { copyPathOf, newInTree, revealInTree } from "../explorer/current";
import type { MessageKey } from "../i18n";
import {
  CORE_OWNER,
  type ContributionInput,
  type ContributionRegistry,
} from "../plugins/registry.svelte";
import { ui } from "../shell/ui.svelte";
import { windowStore as store } from "../window.svelte";
import { NO_REQUIREMENT, type Requirement } from "../conditions/available";

function workspace(...kinds: WorkspaceKind[]): Requirement {
  return { needs: [{ need: "workspace", kinds }], prefers: [] };
}

const PROJECT = workspace("project");
const DOCUMENTS = workspace("project", "singleFile");
/** What shows a document: a project, a single file, the Notes. */
const READING = workspace("project", "singleFile", "notes");

type Spec = {
  id: string;
  label: MessageKey;
  keys?: readonly string[];
  requires?: Requirement;
  when?: () => boolean;
  inPalette?: false;
  run: () => void | Promise<void>;
};

const hasDocument = (): boolean => documents.path !== null;
const readable = (): boolean => hasDocument() && !editingNow();
const canEdit = (): boolean => (hasDocument() && editBlockedReason() === null) || editingNow();

async function closeWorkspace(): Promise<void> {
  if (await confirmLeave({ kind: "workspace" })) await store.closeWorkspace();
}

function cycleWidth(): void {
  const at = WIDTHS.indexOf(reading.width);
  reading.setWidth(WIDTHS[(at + 1) % WIDTHS.length] ?? "comfortable");
}

/** In registration order: the palette lists them so. */
const SPECS: readonly Spec[] = [
  { id: "app.openFolder", label: "cmd.openFolder", keys: ["Mod+o"], run: () => store.openFolder() },
  { id: "app.openFile", label: "cmd.openFile", keys: ["Mod+Shift+o"], run: () => store.openFile() },
  { id: "app.settings", label: "cmd.settings", keys: ["Mod+,"], run: () => ui.toggleSettings() },
  { id: "app.keyboard", label: "cmd.keyboard", run: () => ui.openKeyboard() },
  {
    id: "app.closeWorkspace",
    label: "cmd.closeFolder",
    requires: READING,
    run: () => closeWorkspace(),
  },
  {
    id: "search.quickOpen",
    label: "cmd.quickOpen",
    keys: ["Mod+p"],
    requires: PROJECT,
    inPalette: false,
    run: () => ui.openFinder("quick"),
  },
  {
    id: "search.everywhere",
    label: "cmd.searchEverywhere",
    keys: ["Mod+t"],
    requires: DOCUMENTS,
    inPalette: false,
    run: () => ui.openFinder("everywhere"),
  },
  {
    id: "search.all",
    label: "cmd.searchAll",
    keys: ["Mod+Shift+f"],
    requires: PROJECT,
    run: () => ui.showSidebar("search"),
  },
  {
    id: "palette.open",
    label: "cmd.palette",
    keys: ["Mod+Shift+p"],
    inPalette: false,
    run: () => ui.openFinder("commands"),
  },
  {
    id: "sidebar.files",
    label: "cmd.explorer",
    keys: ["Mod+Shift+x"],
    requires: PROJECT,
    run: () => ui.showSidebar("files"),
  },
  {
    id: "file.new",
    label: "cmd.newFile",
    keys: ["Mod+n"],
    requires: PROJECT,
    run: () => {
      ui.showSidebar("files");
      newInTree("file");
    },
  },
  {
    id: "doc.find",
    label: "cmd.find",
    keys: ["Mod+f"],
    requires: READING,
    when: readable,
    run: () => ui.openFind(),
  },
  {
    id: "doc.back",
    label: "cmd.back",
    keys: ["Alt+ArrowLeft"],
    requires: READING,
    when: readable,
    run: () => documents.back(),
  },
  {
    id: "doc.forward",
    label: "cmd.forward",
    keys: ["Alt+ArrowRight"],
    requires: READING,
    when: readable,
    run: () => documents.forward(),
  },
  {
    id: "doc.zoomIn",
    label: "cmd.zoomIn",
    keys: ["Mod+="],
    requires: READING,
    when: () => !editingNow(),
    run: () => reading.zoomBy(1),
  },
  {
    id: "doc.zoomOut",
    label: "cmd.zoomOut",
    keys: ["Mod+-"],
    requires: READING,
    when: () => !editingNow(),
    run: () => reading.zoomBy(-1),
  },
  {
    id: "doc.zoomReset",
    label: "cmd.zoomReset",
    keys: ["Mod+0"],
    requires: READING,
    when: () => !editingNow(),
    run: () => reading.setZoom(100),
  },
  {
    id: "doc.cycleWidth",
    label: "cmd.cycleWidth",
    keys: ["Mod+Alt+w"],
    requires: READING,
    run: cycleWidth,
  },
  {
    id: "doc.outline",
    label: "cmd.toggleOutline",
    keys: ["Mod+Alt+o"],
    requires: READING,
    when: () => documents.view?.body.kind === "markdown",
    run: () => reading.toggleOutline(),
  },
  {
    id: "doc.copyReference",
    label: "cmd.copyReference",
    keys: ["Mod+Alt+c"],
    requires: READING,
    when: hasDocument,
    run: () => documents.copyReference(store.referenceTemplate),
  },
  {
    id: "doc.copyPath",
    label: "cmd.copyPath",
    keys: ["Mod+Alt+Shift+c"],
    requires: PROJECT,
    when: hasDocument,
    run: () => {
      if (documents.path !== null) copyPathOf(documents.path, false);
    },
  },
  {
    id: "doc.copyDocument",
    label: "cmd.copyDocument",
    keys: ["Mod+Shift+c"],
    requires: READING,
    when: hasDocument,
    run: () => documents.copyDocument(),
  },
  {
    id: "doc.reveal",
    label: "cmd.revealInTree",
    requires: PROJECT,
    when: hasDocument,
    run: () => {
      ui.showSidebar("files");
      revealInTree(null);
    },
  },
  {
    id: "doc.openEditor",
    label: "cmd.openExternal",
    keys: ["Mod+Shift+e"],
    requires: READING,
    when: hasDocument,
    run: () => documents.openInEditor(),
  },
  {
    id: "edit.toggle",
    label: "cmd.toggleEdit",
    keys: ["Mod+e"],
    requires: READING,
    when: canEdit,
    run: () => chooseMode(editing.mode === "view" ? editing.lastEditOr(store.editMode) : "view"),
  },
  {
    id: "edit.source",
    label: "cmd.toggleEditMode",
    keys: ["Mod+Alt+e"],
    requires: READING,
    when: canEdit,
    run: () => chooseMode("source"),
  },
  {
    id: "edit.save",
    label: "cmd.save",
    keys: ["Mod+s"],
    requires: READING,
    when: editingNow,
    run: () => editing.save(),
  },
];

/** Registers the core's commands and default keys; returns the removal of all of them. */
export function registerCoreCommands(registry: ContributionRegistry): () => void {
  const offs: (() => void)[] = [];
  const add = (item: ContributionInput): void => {
    offs.push(registry.register({ ...item, owner: CORE_OWNER }));
  };
  for (const spec of SPECS) {
    add({
      kind: "command",
      id: spec.id,
      label: spec.label,
      run: spec.run,
      requires: spec.requires ?? NO_REQUIREMENT,
      ...(spec.when ? { when: spec.when } : {}),
      ...(spec.inPalette === false ? { inPalette: false } : {}),
    });
    for (const keys of spec.keys ?? []) {
      add({ kind: "shortcut", keys, command: spec.id, requires: NO_REQUIREMENT });
    }
  }
  return () => offs.forEach((off) => off());
}
