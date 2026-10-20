<script lang="ts">
  // The front matter of a document in Edit mode, as properties to edit. The syntax plugin (YAML | TOML) reads the
  // text between the fences and replaces one value at a time; the fences and everything else stay as written.
  // Text the plugin cannot show as properties stays as source.
  import { api, type Body, type NewValue } from "../../api";
  import { t } from "../i18n";
  import ConfigSheet from "./ConfigSheet.svelte";
  import { joinFrontMatter, splitFrontMatter } from "./frontMatter";

  type Props = {
    source: string;
    syntax: "yaml" | "toml";
    /** Puts the edited block in the document. */
    replace: (source: string) => void;
  };
  let { source, syntax, replace }: Props = $props();

  const parts = $derived(splitFrontMatter(source));
  /** A name the backend picks the syntax by. */
  const path = $derived(`front-matter.${syntax}`);
  let body = $state.raw<Body | null>(null);
  let failed = $state(false);

  // the sheet of the properties: asked for the text now (a new source makes a new node, a new card)
  $effect(() => {
    const text = parts?.body ?? "";
    let gone = false;
    void api.configSheet(path, text).then((result) => {
      if (gone) return;
      if (result.ok && result.value.kind === "sheet") body = result.value;
      else failed = true;
    });
    return () => {
      gone = true;
    };
  });

  async function commit(property: string, value: NewValue): Promise<string | null> {
    if (parts === null) return "unsupported";
    const result = await api.configEdit(path, parts.body, { op: "set", path: property, value });
    if (!result.ok) {
      return result.error.kind === "backend"
        ? (result.error.error.detail ?? result.error.error.kind)
        : "other";
    }
    replace(joinFrontMatter(parts, result.value.text));
    return null;
  }
</script>

{#if body?.kind === "sheet"}
  <div class="card">
    <ConfigSheet sheet={body.sheet} editable={true} {commit} source={null} />
  </div>
{:else if failed}
  <pre class="raw" aria-label={t("config.noSheet")}>{source}</pre>
{/if}

<style>
  /* the sheet fills the block: its own width rules apply to a page, not to a card inside the editor */
  .card :global(.sheet) {
    width: 100%;
    padding: var(--sp-2) var(--sp-3);
  }
  .raw {
    margin: 0;
    padding: var(--sp-2) var(--sp-3);
    overflow-x: auto;
    font: var(--fs-code) / 1.5 var(--font-code);
  }
</style>
