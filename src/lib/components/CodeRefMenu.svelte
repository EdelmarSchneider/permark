<script lang="ts" module>
  import type { RefTarget } from "../dom/codeRefsDom";

  /** An open menu of a code reference: the files it could mean (a click on an ambiguous one), or what can be done with the one it means. */
  export type RefMenu = {
    mode: "choose" | "actions";
    target: RefTarget;
    anchor: { left: number; right: number; top: number; bottom: number };
  };
</script>

<script lang="ts">
  import { t } from "../i18n";
  import { isMarkdown } from "../paths";
  import { app } from "../stores/app.svelte";
  import Menu, { type MenuEntry } from "./Menu.svelte";

  type Props = { menu: RefMenu; onclose: () => void };
  let { menu, onclose }: Props = $props();

  const entries = $derived.by<MenuEntry[]>(() => {
    const { target } = menu;
    const { line, col } = target;
    if (menu.mode === "choose" || target.path === null) {
      return [
        { type: "label", label: t("codeRef.chooseLabel") },
        ...target.candidates.map((path): MenuEntry => ({
          id: path,
          label: path,
          icon: "file",
          onselect: () => void app.openCodeRef(path, line, col),
        })),
      ];
    }
    const path = target.path;
    return [
      {
        id: "open",
        label: t("common.open"),
        icon: "file-text",
        onselect: () => void app.openCodeRef(path, line, col),
      },
      // a Markdown file opens here first; any other file only has the editor
      ...(isMarkdown(path)
        ? ([
            {
              id: "external",
              label: t("menu.openExternal"),
              icon: "external",
              onselect: () => void app.openCodeRef(path, line, col, true),
            },
          ] satisfies MenuEntry[])
        : []),
      { type: "separator" },
      {
        id: "path",
        label: t("menu.copyPath"),
        icon: "copy",
        onselect: () => void app.copyPath(path, "code-ref"),
      },
      {
        id: "reference",
        label: t("menu.copyReference"),
        icon: "at",
        onselect: () => void app.copyReference(path, "code-ref", line),
      },
    ];
  });
</script>

<Menu
  label={menu.mode === "choose" ? t("codeRef.chooseLabel") : t("codeRef.menuLabel")}
  anchor={menu.anchor}
  {entries}
  {onclose}
/>
