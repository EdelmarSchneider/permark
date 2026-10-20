// The words CodeMirror's search panel, go-to-line dialog and special-character marks show, mapped to the app's
// translation keys (`EditorState.phrases`). `$` is CodeMirror's placeholder for a number.

import type { MessageKey } from "../../i18n";

export const SEARCH_PHRASES: readonly (readonly [string, MessageKey])[] = [
  ["Find", "source.find"],
  ["Replace", "source.replace"],
  ["next", "source.next"],
  ["previous", "source.previous"],
  ["all", "source.all"],
  ["match case", "source.matchCase"],
  ["regexp", "source.regexp"],
  ["by word", "source.byWord"],
  ["replace", "source.replaceOne"],
  ["replace all", "source.replaceAll"],
  ["close", "source.close"],
  ["current match", "source.currentMatch"],
  ["on line", "source.onLine"],
  ["replaced $ matches", "source.replacedMatches"],
  ["replaced match on line $", "source.replacedOnLine"],
  ["Go to line", "source.goToLine"],
  ["go", "source.go"],
  ["Control character", "source.controlCharacter"],
];
