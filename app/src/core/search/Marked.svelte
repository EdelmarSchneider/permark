<script lang="ts">
  // Text in runs, the matched ones marked. One span per run, the same shape on every update (no branches that swap
  // text nodes: the list of runs changes length while the person types).
  type Run = { text: string; mark: boolean };
  type Props = { runs: readonly Run[] };
  let { runs }: Props = $props();
</script>

{#each runs as run, i (`${i}:${run.mark ? 1 : 0}:${run.text}`)}<span class:hit={run.mark}
    >{run.text}</span
  >{/each}

<style>
  .hit {
    background: var(--mark);
    border-radius: 2px;
  }
</style>
