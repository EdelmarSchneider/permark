<script lang="ts">
  // Many paths dropped to open: "Open 12 items as tabs?" at the drop point, before any tab opens.
  import { t } from "../i18n";
  import DropPopover from "./DropPopover.svelte";

  type Props = {
    at: { x: number; y: number };
    count: number;
    confirm: () => void;
    cancel: () => void;
  };
  let { at, count, confirm, cancel }: Props = $props();
</script>

<DropPopover {at} labelledby="open-many-title" close={cancel}>
  <h2 id="open-many-title">{t("drop.many.title", { n: count })}</h2>
  <footer>
    <button type="button" class="btn btn--primary" data-autofocus onclick={confirm}>
      {t("drop.many.open")}
    </button>
    <button type="button" class="btn" onclick={cancel}>{t("common.cancel")}</button>
  </footer>
</DropPopover>

<style>
  h2 {
    margin: 0;
    font-size: var(--fs-ui);
    font-weight: 600;
  }
  footer {
    display: flex;
    justify-content: flex-end;
    gap: var(--sp-2);
    margin-top: var(--sp-3);
  }
</style>
