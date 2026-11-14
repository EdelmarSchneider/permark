<script module lang="ts">
  /** How many times this view was created: a view that moves to another group | region is never created again. */
  export const probe = { mounts: 0 };
</script>

<script lang="ts">
  // A view with what a move must keep: a field the person typed in, a list scrolled down, a header with the options
  // button of the views.
  import type { PanelViewProps } from "../plugins/props";

  let { viewMenu }: PanelViewProps = $props();
  probe.mounts += 1;
</script>

<section data-probe>
  <header data-view-header>
    <h2>Probe</h2>
    {#if viewMenu}{@render viewMenu()}{/if}
  </header>
  <input class="probe-field" aria-label="Probe" />
  <div class="probe-list" style:overflow="auto" style:height="40px">
    <div style:height="400px"></div>
  </div>
</section>
