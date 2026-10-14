<script lang="ts">
  import { t } from "../i18n";
  import { LEFT_DEFAULT, RIGHT_DEFAULT } from "../session";
  import { session } from "../stores/session.svelte";
  import { ui } from "../stores/ui.svelte";

  type Props = { side: "left" | "right" };
  let { side }: Props = $props();

  let startX = 0;
  let startWidth = 0;
  let dragging = false;

  const panel = $derived(side === "left" ? session.left : session.right);

  function onpointerdown(e: PointerEvent): void {
    if (e.button !== 0) return;
    dragging = true;
    startX = e.clientX;
    startWidth = panel.width;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    ui.resizing = true;
  }

  function onpointermove(e: PointerEvent): void {
    if (!dragging) return;
    const dx = e.clientX - startX;
    session.setPanel(side, { width: side === "left" ? startWidth + dx : startWidth - dx });
  }

  function end(): void {
    dragging = false;
    ui.resizing = false;
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    e.preventDefault();
    const grow = (e.key === "ArrowRight") === (side === "left");
    session.setPanel(side, { width: panel.width + (grow ? 16 : -16) });
  }
</script>

<!-- a focusable separator is the WAI-ARIA "window splitter" pattern -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div
  class={["resizer", side === "left" ? "resizer--right" : "resizer--left"]}
  role="separator"
  aria-orientation="vertical"
  aria-label={side === "left" ? t("panel.resizeLeft") : t("panel.resizeRight")}
  aria-valuenow={panel.width}
  aria-valuemin={200}
  aria-valuemax={480}
  title={t("panel.resizeHint")}
  tabindex="0"
  {onpointerdown}
  {onpointermove}
  onpointerup={end}
  onpointercancel={end}
  ondblclick={() =>
    session.setPanel(side, { width: side === "left" ? LEFT_DEFAULT : RIGHT_DEFAULT })}
  {onkeydown}
></div>
