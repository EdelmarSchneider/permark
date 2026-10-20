<script lang="ts">
  // The buttons plugins put next to Copy on a fenced code block of a language they take (convert the block).
  import { conditions } from "../conditions/context.svelte";
  import { t } from "../i18n";
  import { contributions } from "../plugins/instance.svelte";

  type Props = { source: string; language: string };
  let { source, language }: Props = $props();

  const actions = $derived(
    contributions
      .visible("codeAction", conditions.current)
      .filter((action) => action.applies(language.toLowerCase())),
  );
</script>

{#each actions as action (action.id)}
  {@const Icon = action.icon}
  <button
    type="button"
    class="icon-btn icon-btn--sm code-action"
    title={t(action.label)}
    aria-label={t(action.label)}
    onclick={() => action.run(source, language.toLowerCase())}
  >
    <Icon size={14} />
  </button>
{/each}
