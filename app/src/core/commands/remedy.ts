// What a remedy button of the palette does. Only the remedies the app can carry out today have a handler; for the
// others the reason is the whole help (no button).

import { ui } from "../shell/ui.svelte";
import { windowStore } from "../window.svelte";

/** The action for a remedy command (`plugins.enable:<id>`, `settings.git`), or `null` when there is none. */
export function remedyAction(command: string | null): (() => void) | null {
  if (command === null) return null;
  const [name, argument] = command.split(":", 2);
  switch (name) {
    case "plugins.enable":
      return argument
        ? () =>
            void windowStore.changeSetting({
              key: "pluginEnabled",
              value: { plugin: argument, enabled: true },
            })
        : null;
    case "settings.git":
      return () => ui.openSettings();
    default:
      return null;
  }
}
