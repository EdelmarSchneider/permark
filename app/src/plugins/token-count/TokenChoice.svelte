<script lang="ts">
  // The choice of tokenizers with where it is kept: for every project, or for this project only (shared with the team in
  // `.permark/config.toml`, only in a project that has it). `mode`: where the choice can be made from.
  import { t } from "../../core/i18n";
  import TokenChooser from "./TokenChooser.svelte";
  import { tokens } from "./store.svelte";
  import { infoOf } from "./tokenizers";

  type Props = {
    /** `app`: the choice for every project. `project`: this project's own. `both`: this project's when it has one. */
    mode: "app" | "project" | "both";
    removable?: boolean;
  };
  let { mode, removable = false }: Props = $props();

  const canOwn = $derived(mode !== "app" && tokens.projectCanChoose);
  const own = $derived(tokens.projectList.length > 0);
  /** The list the chooser edits. */
  const scope = $derived(mode === "project" || (canOwn && own) ? "project" : "app");
  const ids = $derived(
    (scope === "project" ? tokens.projectList : tokens.appList).filter((id) => infoOf(id) !== null),
  );

  function ownChanged(on: boolean): void {
    void tokens.choose("project", on ? tokens.chosen.map((info) => info.id) : []);
  }
</script>

{#if canOwn}
  <label class="own">
    <input type="checkbox" checked={own} onchange={(e) => ownChanged(e.currentTarget.checked)} />
    <span class="text">
      <span>{t("tokens.own")}</span>
      <span class="hint">{t("tokens.ownHint")}</span>
    </span>
  </label>
{/if}
{#if mode === "project" && !canOwn}
  <p class="note">{t("tokens.projectNone")}</p>
{:else if mode !== "project" || own}
  <TokenChooser {ids} {removable} onchange={(next) => void tokens.choose(scope, next)} />
{:else}
  <p class="note">{t("tokens.projectUsesApp")}</p>
{/if}

<style>
  .own {
    display: flex;
    align-items: flex-start;
    gap: var(--sp-2);
    padding-bottom: var(--sp-2);
    cursor: pointer;
  }
  .own input {
    margin-top: 3px;
  }
  .text {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .hint,
  .note {
    color: var(--muted);
    font-size: var(--fs-ui-xs);
  }
  .note {
    margin: 0;
  }
</style>
