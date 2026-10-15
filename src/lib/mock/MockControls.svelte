<script lang="ts">
  // Developer panel shown only with VITE_MOCK=1: triggers the backend activity the real watcher would produce.
  // It is deliberately not part of the app and uses plain English strings.
  import { bridge } from "../bridge";
  import { docs } from "../stores/docs.svelte";
  import { edit } from "../stores/edit.svelte";
  import { vault } from "../stores/vault.svelte";
  import {
    emitBulk,
    emitBurst,
    emitDelete,
    emitRestore,
    emitSave,
    setGitError,
    setRepoState,
  } from "./driver";
  import { mockFiles } from "./fixtures";
  import { agentAsks, agentConnects, agentOpens } from "./mockAgent";
  import { setMetaProblem } from "./mockMeta";

  let open = $state(false);
  let agentHere = $state(false);
  let rebasing = $state(false);
  let gitError = $state(false);
  let metaConflict = $state(false);
  let failSave = $state(false);

  const target = $derived(docs.path ?? "docs/specs/live-feed.md");

  // handy in the browser console while developing against the mock
  (window as unknown as Record<string, unknown>).__tm = { docs, vault, edit, bridge };
</script>

{#if vault.info && !vault.isSimple}
  <div class="demo" aria-label="Mock controls">
    <b>Mock backend</b>
    <button class="btn" onclick={() => (open = !open)}>{open ? "Hide" : "Controls"}</button>
    {#if open}
      <button class="btn" onclick={() => emitSave(target)}>Save open doc</button>
      <button class="btn" onclick={emitBurst}>Burst (4 docs)</button>
      <button class="btn" onclick={emitBulk}>Bulk checkout</button>
      <button class="btn" onclick={() => emitDelete(target)}>Delete open doc</button>
      <button class="btn" onclick={() => emitRestore(target)}>Restore</button>
      <button
        class="btn"
        onclick={() => {
          rebasing = !rebasing;
          setRepoState(rebasing ? "rebase" : "clean");
        }}
      >
        Rebase {rebasing ? "off" : "on"}
      </button>
      <button
        class="btn"
        onclick={() => {
          gitError = !gitError;
          setGitError(gitError);
        }}
      >
        Git error {gitError ? "off" : "on"}
      </button>
      <button
        class="btn"
        onclick={() => {
          metaConflict = !metaConflict;
          setMetaProblem(metaConflict);
        }}
      >
        meta.toml conflict {metaConflict ? "off" : "on"}
      </button>
      <button
        class="btn"
        onclick={() => {
          failSave = !failSave;
          mockFiles.failNextSave = failSave;
        }}
      >
        Next save fails {failSave ? "(armed)" : "off"}
      </button>
      <button
        class="btn"
        onclick={() => {
          agentHere = !agentHere;
          agentConnects(agentHere);
        }}
      >
        Agent {agentHere ? "leaves" : "connects"}
      </button>
      <button class="btn" onclick={() => agentAsks("get_context")}>Agent asks context</button>
      <button class="btn" onclick={() => agentAsks("search", "rollout plan")}>
        Agent searches
      </button>
      <button class="btn" onclick={() => agentOpens("docs/specs/watcher.md", 12)}>
        Agent opens a place
      </button>
    {/if}
  </div>
{/if}
