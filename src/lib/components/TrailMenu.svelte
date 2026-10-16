<script lang="ts">
  // The places the reader came from or can go to again, as a menu: under the Back or Forward button (right-click or long
  // press), and for the whole trail from the command palette ("Go to a recent place..."). Choosing one jumps there.
  import { untrack } from "svelte";
  import type { TrailStop } from "../history";
  import { t } from "../i18n";
  import { basename, folderLabel } from "../paths";
  import { docs } from "../stores/docs.svelte";
  import { ui } from "../stores/ui.svelte";
  import Menu, { type MenuEntry } from "./Menu.svelte";
  import type { Anchor } from "./Popover.svelte";

  // The menu is a picture of the trail as it was when it opened: the first click on anything else closes it.
  const request = untrack(() => ui.trail);

  /** Under the header's buttons when it was opened from the keyboard or the palette. */
  function findAnchor(): Anchor {
    const group = document.querySelector<HTMLElement>(".doc-header .nav-group");
    const r = group?.getBoundingClientRect();
    return {
      left: r?.left ?? 24,
      right: r?.right ?? 24,
      top: r?.top ?? 44,
      bottom: r?.bottom ?? 44,
    };
  }
  const anchor = request?.anchor ?? findAnchor();

  const view = request?.view ?? "all";
  const stops: TrailStop[] = (() => {
    if (view === "all") return docs.trail();
    // one side of the trail, under the open document so the reader sees where the list starts
    const here = docs.trail().find((stop) => stop.current);
    return [...(here ? [here] : []), ...docs.trailStops(view)];
  })();

  /** The keyboard starts on the nearest place the reader came from, else on the nearest one ahead. */
  const first = stops.find((stop) => stop.offset === -1) ?? stops.find((stop) => stop.offset === 1);

  const entries: MenuEntry[] = stops.map((stop) => ({
    id: String(stop.offset),
    label: basename(stop.path),
    detail: folderLabel(stop.path),
    // the check column marks the open document and keeps the names in line
    checked: stop.current,
    title: stop.current ? t("trail.here") : stop.path,
    initial: stop === first,
    onselect: () => void docs.go(stop.offset),
  }));

  const label = $derived(
    t(view === "back" ? "trail.back" : view === "forward" ? "trail.forward" : "trail.all"),
  );
</script>

<Menu {label} {anchor} {entries} scroll onclose={() => (ui.trail = null)} />
