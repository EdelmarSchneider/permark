// Edit mode, loaded on first use (Milkdown is a chunk of its own).

export { buildDoc, compose, type Built, type Composed } from "./compose";
export {
  createCore,
  LOCKED,
  relook,
  type BlockName,
  type CodeLabel,
  type Core,
  type LockReason,
  type LockedHost,
  type LooksHost,
} from "./core";
export { LookBook, lockedLooks, type LockedRender } from "./lockedRender";
export { applyEdits, blockOf, docRange, scopeIn, selectRange } from "./edits";
export { markAt, marksPlugin, setMarks } from "./marks";
export { editFinder } from "./find";
export { repaintLooks } from "./inlineLooks";
export { pieceSources } from "./inlineSource";
export { formatOf, run, type BlockType, type Command, type Format } from "./commands";
export { appPlugins, slashAt, type PluginHost, type SlashState } from "./plugins";
