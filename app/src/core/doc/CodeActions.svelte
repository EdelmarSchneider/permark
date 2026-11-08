<script module lang="ts">
  import { conditions } from "../conditions/context.svelte";
  import { contributions } from "../plugins/instance.svelte";
  import type { ContributionOf } from "../plugins/registry.svelte";

  /** The actions on offer for a fenced code block of this language (Edit counts them to size its label). */
  export function codeActionsFor(language: string): ContributionOf<"codeAction">[] {
    const word = language.toLowerCase();
    return contributions
      .visible("codeAction", conditions.current)
      .filter((action) => action.applies(word));
  }
</script>

<script lang="ts">
  // The buttons plugins put next to Copy on a fenced code block of a language they take (convert the block).
  import { t } from "../i18n";

  type Props = { source: string; language: string };
  let { source, language }: Props = $props();

  const actions = $derived(codeActionsFor(language));
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
