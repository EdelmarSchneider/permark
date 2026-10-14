<script lang="ts">
  import type { Snippet } from "svelte";
  import type { HTMLButtonAttributes } from "svelte/elements";
  import Icon, { type IconName } from "./Icon.svelte";

  type Props = Omit<HTMLButtonAttributes, "class" | "children"> & {
    icon: IconName;
    /** Accessible name; also the tooltip unless `hint` is given. */
    label: string;
    hint?: string;
    pressed?: boolean;
    small?: boolean;
    /** Shows the check mark ("Copied") instead of the icon. */
    done?: boolean;
    class?: string;
    /** Extra content drawn over the icon (a notification dot). */
    children?: Snippet;
  };
  let {
    icon,
    label,
    hint,
    pressed,
    small = false,
    done = false,
    class: extra = "",
    children,
    ...rest
  }: Props = $props();
</script>

<button
  type="button"
  class={["icon-btn", small && "icon-btn--sm", done && "is-done", extra]}
  aria-label={label}
  title={hint ?? label}
  aria-pressed={pressed}
  {...rest}
>
  <Icon name={done ? "check" : icon} size={small ? "sm" : "md"} />
  {@render children?.()}
</button>
