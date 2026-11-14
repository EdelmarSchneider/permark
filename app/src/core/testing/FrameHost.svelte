<script lang="ts">
  // A frame with plain content in every region, for the tests of focus mode, of hidden panels and of the bottom
  // region (absent unless asked for).
  import Frame from "../shell/Frame.svelte";

  type Props = {
    focus?: boolean;
    moving?: boolean;
    sidebar?: boolean;
    panel?: boolean;
    bottom?: boolean;
    sidebarHidden?: boolean;
    rightHidden?: boolean;
    bottomHidden?: boolean;
    bottomHeight?: number;
    onBottomHeight?: (px: number) => void;
  };
  let {
    focus = false,
    moving = false,
    sidebar = true,
    panel = true,
    bottom = false,
    sidebarHidden = false,
    rightHidden = false,
    bottomHidden = false,
    bottomHeight,
    onBottomHeight,
  }: Props = $props();
</script>

{#snippet top()}<span id="top-text">top</span>{/snippet}
{#snippet side()}<button id="side-button" type="button">side</button>{/snippet}
{#snippet page()}<p id="page-text">the page</p>{/snippet}
{#snippet under()}<button id="bottom-button" type="button">bottom</button>{/snippet}
{#snippet right()}<span id="right-text">right</span>{/snippet}
{#snippet status()}<span id="status-text">status</span>{/snippet}

<Frame
  {top}
  sidebar={sidebar ? side : undefined}
  main={page}
  bottom={bottom ? under : undefined}
  panel={panel ? right : undefined}
  {status}
  {sidebarHidden}
  {rightHidden}
  {bottomHidden}
  {bottomHeight}
  {onBottomHeight}
  {focus}
  {moving}
/>
