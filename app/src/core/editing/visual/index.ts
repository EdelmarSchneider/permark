// Edit mode, loaded on first use (Milkdown is a chunk of its own).

export { buildDoc, compose, type Built, type Composed } from "./compose";
export {
  createCore,
  LOCKED,
  type Core,
  type LockReason,
  type LockedHost,
  type PictureHost,
  type PictureView,
} from "./core";
export { formatOf, run, type BlockType, type Command, type Format } from "./commands";
export { appPlugins, slashAt, type PluginHost, type SlashState } from "./plugins";
