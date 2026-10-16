// How a toast lays itself out. Pure, so the rule can be tested; `components/Toast.svelte` draws it.

import type { ToastModel } from "./stores/toast.svelte";

/**
 * Whether the buttons of a toast go on a row of their own under the message. A message with any button is stacked: a
 * button takes the width of its own label ("Show in file manager" is 235 px in Spanish), and next to it the message would
 * be left a few characters wide in a toast that is 348 px. A message with no button, a change and an update keep their row.
 */
export function isStacked(model: ToastModel): boolean {
  return model.type === "message" && model.action !== undefined;
}
