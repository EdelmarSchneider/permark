<script lang="ts">
  // "Copy as JSON": the data of a TOON text on the clipboard, through the shared copy button (its result shown and
  // said).
  import Braces from "@lucide/svelte/icons/braces";
  import CopyButton from "../../core/doc/CopyButton.svelte";
  import { t } from "../../core/i18n";
  import { copyAsJson } from "./copyJson";

  type Props = {
    /** The TOON text (asked when pressed: a file is read then). */
    text: () => Promise<string | null>;
    /** Icon only (a code block's corner). */
    compact?: boolean;
  };
  let { text, compact = false }: Props = $props();

  async function copy(): Promise<boolean> {
    const source = await text();
    return source !== null && (await copyAsJson(source));
  }
</script>

<CopyButton
  {copy}
  label={t("toon.copyJson")}
  failedLabel={t("toon.copyFailed")}
  icon={Braces}
  {compact}
  class="copy-json"
/>
