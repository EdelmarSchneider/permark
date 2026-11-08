// Edit mode, loaded on first use (Milkdown is a chunk of its own).

export { buildDoc, compose, type Built, type Composed } from "./compose";
export {
  createCore,
  LOCKED,
  type CodeLabel,
  type Core,
  type LockReason,
  type LockedHost,
} from "./core";
export { applyEdits, blockOf, docRange, scopeIn, selectRange } from "./edits";
export { markAt, marksPlugin, setMarks } from "./marks";
export { formatOf, run, type BlockType, type Command, type Format } from "./commands";
export { appPlugins, slashAt, type PluginHost, type SlashState } from "./plugins";
