<script lang="ts">
  // A project path: the file name first and strong, its folder after it and dim; the characters the query matched
  // are marked.
  import { markRuns } from "./fuzzy";

  type Props = { path: string; positions?: readonly number[]; folderOnly?: boolean };
  let { path, positions = [], folderOnly = false }: Props = $props();

  const chars = $derived(Array.from(path));
  const cut = $derived(folderOnly ? 0 : chars.lastIndexOf("/") + 1);
  const name = $derived(
    markRuns(
      chars.slice(cut).join(""),
      positions.map((p) => p - cut),
    ),
  );
  const folder = $derived(cut === 0 ? [] : markRuns(chars.slice(0, cut - 1).join(""), positions));
</script>

<span class="name" class:folder-only={folderOnly}
  >{#each name as run, i (i)}{#if run.mark}<mark>{run.text}</mark
      >{:else}{run.text}{/if}{/each}</span
>{#if folder.length > 0}<span class="folder"
    >{#each folder as run, i (i)}{#if run.mark}<mark>{run.text}</mark
        >{:else}{run.text}{/if}{/each}</span
  >{/if}

<style>
  .name {
    font-weight: 500;
  }
  .name.folder-only {
    font-weight: 400;
  }
  .folder {
    margin-left: var(--sp-2);
    color: var(--faint);
    font-size: var(--fs-ui-sm);
  }
  mark {
    background: var(--mark);
    color: inherit;
    border-radius: 2px;
  }
</style>
