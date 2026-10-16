<script lang="ts">
  // A long explanation kept behind a small (i) button: a click (or Enter, Space) shows it right under the setting
  // and a second one hides it; the text is also the button's tooltip. The button and the text are two siblings, so
  // the parent decides where they sit (see `.field__row--info` and `.check-row`).
  import { t } from "../i18n";
  import IconButton from "./IconButton.svelte";

  type Props = {
    /** The id of the text: what other controls point at with `aria-describedby`. */
    id: string;
    text: string;
    /** What the hint is about (the setting's name), so that each button has its own name. */
    subject: string;
  };
  let { id, text, subject }: Props = $props();

  let open = $state(false);
</script>

<IconButton
  small
  class="info-btn"
  icon="info"
  label={t("common.moreAbout", { name: subject })}
  hint={text}
  aria-expanded={open}
  aria-controls={id}
  onclick={() => (open = !open)}
/>
<span {id} class="field__hint info-hint" hidden={!open}>{text}</span>
