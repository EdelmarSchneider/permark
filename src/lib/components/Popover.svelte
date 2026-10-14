<script lang="ts" module>
  /** Viewport rectangle a popover is placed next to (a DOMRect works). */
  export type Anchor = { left: number; right: number; top: number; bottom: number };
</script>

<script lang="ts">
  import { onMount, type Snippet } from "svelte";
  import { registerOverlay, rememberFocus } from "../overlays";

  type Props = {
    anchor: Anchor;
    side?: "bottom" | "top" | "right";
    align?: "start" | "end";
    variant?: "menu" | "panel" | "bar";
    label: string;
    onclose: () => void;
    children: Snippet;
  };
  let {
    anchor,
    side = "bottom",
    align = "start",
    variant = "menu",
    label,
    onclose,
    children,
  }: Props = $props();

  let el: HTMLElement | undefined = $state();
  let pos = $state<{ left: number; top: number } | null>(null);

  const MARGIN = 8;

  function place(): void {
    if (!el) return;
    const w = el.offsetWidth;
    const h = el.offsetHeight;
    let left = align === "end" ? anchor.right - w : anchor.left;
    let top = anchor.bottom + 4;
    if (side === "top") top = anchor.top - h - 4;
    if (side === "right") {
      left = anchor.right + 4;
      top = anchor.top;
    }
    left = Math.max(MARGIN, Math.min(window.innerWidth - w - MARGIN, left));
    top = Math.max(MARGIN, Math.min(window.innerHeight - h - MARGIN, top));
    pos = { left, top };
  }

  $effect(() => {
    // measure once the content is in the DOM, and again when the anchor moves
    void anchor;
    place();
  });

  onMount(() => {
    const restore = rememberFocus();
    const unregister = registerOverlay(onclose);
    const close = (e: PointerEvent): void => {
      const target = e.target as Element | null;
      if (el?.contains(target) || target?.closest("[data-popover-trigger]")) return;
      onclose();
    };
    // attach after the click that opened the popover has finished propagating
    const timer = setTimeout(() => window.addEventListener("pointerdown", close, true), 0);
    window.addEventListener("blur", onclose);
    window.addEventListener("resize", onclose);
    return () => {
      clearTimeout(timer);
      unregister();
      restore();
      window.removeEventListener("pointerdown", close, true);
      window.removeEventListener("blur", onclose);
      window.removeEventListener("resize", onclose);
    };
  });
</script>

<div
  bind:this={el}
  class={[
    "popover",
    variant === "menu" && "popover--menu",
    variant === "panel" && "popover--panel",
  ]}
  style:left="{pos?.left ?? 0}px"
  style:top="{pos?.top ?? 0}px"
  style:visibility={pos ? "visible" : "hidden"}
  role={variant === "bar" ? "toolbar" : variant === "menu" ? "menu" : "dialog"}
  aria-label={label}
>
  {@render children()}
</div>
