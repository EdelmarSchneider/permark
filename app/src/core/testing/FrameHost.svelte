<script lang="ts">
  // A frame with plain content in every region, for the tests of focus mode, of hidden panels and of the bottom
  // region (absent unless asked for). As the window does, it makes the bottom region's height fit the center column
  // the frame reports (too short for it: off).
  import Frame from "../shell/Frame.svelte";
  import { BOTTOM, fitBottom } from "../shell/layout";

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
    onCenterHeight?: (px: number) => void;
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
    bottomHeight = BOTTOM.def,
    onBottomHeight,
    onCenterHeight,
  }: Props = $props();

  let center = $state(0);
  const fitted = $derived(fitBottom(center, bottomHeight));
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
  bottomHidden={bottomHidden || fitted === null}
  bottomHeight={fitted ?? bottomHeight}
  {onBottomHeight}
  onCenterHeight={(px) => {
    center = px;
    onCenterHeight?.(px);
  }}
  {focus}
  {moving}
/>
