<script lang="ts">
  // A picture shown large over the page. A click or Escape closes it; the wheel zooms; a drag moves it.
  import { t } from "../i18n";

  type Props = { src: string; alt: string; close: () => void };
  let { src, alt, close }: Props = $props();

  let scale = $state(1);
  let x = $state(0);
  let y = $state(0);
  let drag: { px: number; py: number; x: number; y: number; moved: boolean } | null = null;
  let layer: HTMLElement | undefined = $state();

  $effect(() => {
    layer?.focus();
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    return () => before?.focus();
  });

  function onkeydown(event: KeyboardEvent): void {
    if (event.key === "Escape") {
      event.preventDefault();
      close();
    }
  }

  function onwheel(event: WheelEvent): void {
    event.preventDefault();
    scale = Math.min(8, Math.max(0.2, scale * (event.deltaY < 0 ? 1.12 : 1 / 1.12)));
  }

  function onpointerdown(event: PointerEvent): void {
    drag = { px: event.clientX, py: event.clientY, x, y, moved: false };
    if (event.currentTarget instanceof Element)
      event.currentTarget.setPointerCapture(event.pointerId);
  }

  function onpointermove(event: PointerEvent): void {
    if (!drag) return;
    const dx = event.clientX - drag.px;
    const dy = event.clientY - drag.py;
    if (Math.abs(dx) + Math.abs(dy) > 3) drag.moved = true;
    x = drag.x + dx;
    y = drag.y + dy;
  }

  function onpointerup(): void {
    const moved = drag?.moved ?? false;
    drag = null;
    // a click that did not drag closes
    if (!moved) close();
  }
</script>

<div
  class="layer"
  bind:this={layer}
  role="dialog"
  aria-modal="true"
  aria-label={t("doc.imageZoomed")}
  tabindex="-1"
  {onkeydown}
  {onwheel}
  {onpointerdown}
  {onpointermove}
  {onpointerup}
>
  <img
    {src}
    {alt}
    style:transform={`translate(${x}px, ${y}px) scale(${scale})`}
    draggable="false"
  />
</div>

<style>
  .layer {
    position: fixed;
    inset: 0;
    z-index: 80;
    display: grid;
    place-items: center;
    overflow: hidden;
    background: var(--backdrop);
    cursor: zoom-out;
    animation: pm-fade var(--dur-fast) var(--ease);
  }
  .layer:focus-visible {
    outline: none;
  }
  img {
    max-width: 92vw;
    max-height: 92vh;
    border-radius: var(--radius-card);
    box-shadow: var(--shadow-pop);
    background: var(--checker-a);
    transition: transform 60ms linear;
    user-select: none;
  }
</style>
