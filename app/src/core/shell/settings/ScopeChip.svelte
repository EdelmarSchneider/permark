<script lang="ts">
  // Who a folder setting is for: everyone sharing the folder (the value is kept in its `.permark/config.toml`) or only
  // this person on this computer. A short word on screen; the whole sentence for screen readers, inside the row's label
  // so it is part of the control's name. App settings wear none.
  import { t } from "../../i18n";

  type Props = { scope: "everyone" | "you" };
  let { scope }: Props = $props();
</script>

<span class="scope-chip" data-scope={scope}>
  <span aria-hidden="true"
    >{scope === "everyone" ? t("settings.scope.everyone") : t("settings.scope.you")}</span
  >
  <span class="sr-only"
    >{scope === "everyone" ? t("settings.scope.everyoneFull") : t("settings.scope.youFull")}</span
  >
</span>

<style>
  .scope-chip {
    display: inline-flex;
    align-items: center;
    margin-left: var(--sp-1);
    padding: 0 6px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    color: var(--muted);
    font-size: var(--fs-ui-xs);
    font-weight: 500;
    line-height: 18px;
    vertical-align: 1px;
    white-space: nowrap;
  }
</style>
