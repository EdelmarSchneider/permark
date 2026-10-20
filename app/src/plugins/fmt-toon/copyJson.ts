// "Copy as JSON": the data of a TOON text, decoded by the backend (the pinned spec), on the clipboard.

import { pluginClient, type PluginClient } from "../../core/plugins/client";

/** Copies the JSON of `text`; resolves to false when it could not be decoded or copied. */
export async function copyAsJson(
  text: string,
  client: PluginClient = pluginClient,
): Promise<boolean> {
  const result = await client.call("fmt-toon.toJson", { text });
  if (!result.ok) return false;
  try {
    await navigator.clipboard.writeText(result.value);
    return true;
  } catch {
    // the system refused the clipboard
    return false;
  }
}
