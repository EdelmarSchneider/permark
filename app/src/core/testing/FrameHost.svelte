<script lang="ts">
  // A frame with plain content in every region, for the tests of focus mode and of hidden side panels.
  import Frame from "../shell/Frame.svelte";

  type Props = {
    focus?: boolean;
    moving?: boolean;
    sidebar?: boolean;
    panel?: boolean;
    sidebarHidden?: boolean;
    rightHidden?: boolean;
  };
  let {
    focus = false,
    moving = false,
    sidebar = true,
    panel = true,
    sidebarHidden = false,
    rightHidden = false,
  }: Props = $props();
</script>

{#snippet top()}<span id="top-text">top</span>{/snippet}
{#snippet side()}<button id="side-button" type="button">side</button>{/snippet}
{#snippet page()}<p id="page-text">the page</p>{/snippet}
{#snippet right()}<span id="right-text">right</span>{/snippet}
{#snippet status()}<span id="status-text">status</span>{/snippet}

<Frame
  {top}
  sidebar={sidebar ? side : undefined}
  main={page}
  panel={panel ? right : undefined}
  {status}
  {sidebarHidden}
  {rightHidden}
  {focus}
  {moving}
/>
