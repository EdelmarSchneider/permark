// The commands and default keys the core itself contributes (owner `core`). Each acts through a store the window
// already has; the palette, the keys and the keyboard dialog all read them from the contribution registry.

import type { WorkspaceKind } from "../../api/generated/WorkspaceKind";
import { documents } from "../doc/document.svelte";
import { reading, WIDTHS } from "../doc/reading.svelte";
import { webPictures } from "../doc/webPictures.svelte";
import { chooseMode, editBlockedReason, editingNow } from "../editing/choose";
import { editing } from "../editing/session.svelte";
import { diagramPanel } from "../editing/diagramPanel.svelte";
import { copyPathOf, newInTree, revealInTree } from "../explorer/current";
import { t, type MessageKey } from "../i18n";
import {
  CORE_OWNER,
  type ContributionInput,
  type ContributionRegistry,
} from "../plugins/registry.svelte";
import { closeOwnTab } from "../shell/closeWorkspace";
import { askInitHere } from "../shell/initProject";
import { rightAvailable } from "../shell/layout";
import { rightSide, toggleOutlinePanel, toggleRightPanel, toggleSidebar } from "../shell/sides";
import { tabs } from "../shell/tabs.svelte";
import { ui } from "../shell/ui.svelte";
import { conditions } from "../conditions/context.svelte";
import { stepChange } from "./changes";
import { windowStore as store } from "../window.svelte";
import { NO_REQUIREMENT, type Requirement } from "../conditions/available";

function workspace(...kinds: WorkspaceKind[]): Requirement {
  return { needs: [{ need: "workspace", kinds }], prefers: [] };
}

const PROJECT = workspace("project");
const DOCUMENTS = workspace("project", "singleFile");
/** What shows a document: a project, a single file, the Notes. */
const READING = workspace("project", "singleFile", "notes");
/** What has a sidebar: a project, the Notes. */
const SIDEBAR = workspace("project", "notes");

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

function cycleWidth(): void {
  const at = WIDTHS.indexOf(reading.width);
  reading.setWidth(WIDTHS[(at + 1) % WIDTHS.length] ?? "comfortable");
}

/** `tab.select1..8` (Mod+1..8): tab n of this window; none there -> nothing. */
const TAB_KEYS: readonly Spec[] = (
  [
    "cmd.tab1",
    "cmd.tab2",
    "cmd.tab3",
    "cmd.tab4",
    "cmd.tab5",
    "cmd.tab6",
    "cmd.tab7",
    "cmd.tab8",
  ] as const
).map((label, at) => ({
  id: `tab.select${at + 1}`,
  label,
  keys: [`Mod+${at + 1}`],
  inPalette: false,
  when: () => tabs.list.length > at,
  run: () => tabs.select(at + 1),
}));

/** In registration order: the palette lists them so. */
const SPECS: readonly Spec[] = [
  { id: "app.openFolder", label: "cmd.openFolder", keys: ["Mod+o"], run: () => store.openFolder() },
  { id: "app.openFile", label: "cmd.openFile", keys: ["Mod+Shift+o"], run: () => store.openFile() },
  { id: "app.settings", label: "cmd.settings", keys: ["Mod+,"], run: () => ui.toggleSettings() },
  { id: "app.settingsPage", label: "cmd.settingsPage", run: () => ui.openSettingsPage() },
  { id: "app.keyboard", label: "cmd.keyboard", run: () => ui.openKeyboard() },
  {
    id: "tab.next",
    label: "cmd.tabNext",
    keys: ["Ctrl+Tab", "Mod+Alt+ArrowRight"],
    when: () => tabs.canSwitch,
    run: () => tabs.step(1),
  },
  {
    id: "tab.previous",
    label: "cmd.tabPrevious",
    keys: ["Ctrl+Shift+Tab", "Mod+Alt+ArrowLeft"],
    when: () => tabs.canSwitch,
    run: () => tabs.step(-1),
  },
  ...TAB_KEYS,
  {
    id: "tab.selectLast",
    label: "cmd.tabLast",
    keys: ["Mod+9"],
    inPalette: false,
    when: () => tabs.list.length > 0,
    run: () => tabs.selectLast(),
  },
  {
    id: "project.init",
    label: "cmd.initProject",
    requires: PROJECT,
    when: () => conditions.current.permark === "notInitialized",
    run: () => askInitHere(),
  },
  {
    id: "tab.close",
    label: "cmd.tabClose",
    keys: ["Mod+w"],
    when: () => tabs.active !== null,
    run: () => closeOwnTab(),
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
    id: "sidebar.toggle",
    label: "cmd.toggleSidebar",
    keys: ["Mod+b"],
    requires: SIDEBAR,
    run: () => toggleSidebar(),
  },
  {
    id: "panel.toggle",
    label: "cmd.toggleRight",
    keys: ["Mod+Alt+b"],
    requires: READING,
    when: () => rightAvailable(rightSide()),
    run: () => toggleRightPanel(),
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
    id: "edit.diagramPanel",
    label: "cmd.diagramPanel",
    requires: READING,
    when: () => editing.mode === "source" && editing.kind === "markdown",
    run: () => diagramPanel.toggle(),
  },
  {
    id: "reading.webPictures",
    label: "cmd.webPictures",
    requires: PROJECT,
    run: () => documents.setWebPictures(!webPictures.always, document.querySelector(".md-body")),
  },
  {
    id: "doc.focus",
    label: "cmd.focus",
    keys: ["Mod+Alt+f"],
    requires: READING,
    // the way out never disappears: while focus is on the command runs even with no document
    when: () => ui.focus === "on" || hasDocument(),
    run: () => ui.toggleFocus(),
  },
  {
    id: "doc.outline",
    label: "cmd.toggleOutline",
    keys: ["Mod+Alt+o"],
    requires: READING,
    when: () => documents.view?.body.kind === "markdown",
    run: () => toggleOutlinePanel(),
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
    label: "cmd.openSource",
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
  // next | previous change: offered while a plugin knows how to find one (none in a window without them)
  const changes: readonly { id: string; label: MessageKey; keys: string; direction: 1 | -1 }[] = [
    { id: "doc.nextChange", label: "cmd.nextChange", keys: "F7", direction: 1 },
    { id: "doc.prevChange", label: "cmd.prevChange", keys: "Shift+F7", direction: -1 },
  ];
  for (const { id, label, keys, direction } of changes) {
    add({
      kind: "command",
      id,
      label,
      requires: READING,
      when: () => readable() && registry.of("changeStep").length > 0,
      run: async () => {
        if (!(await stepChange(registry, direction))) store.notifyNote(t("doc.noChange"));
      },
    });
    add({ kind: "shortcut", keys, command: id, requires: NO_REQUIREMENT });
  }
  return () => offs.forEach((off) => off());
}
