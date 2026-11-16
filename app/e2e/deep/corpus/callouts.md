# Callouts and containers

> [!NOTE]
> A note with the default title.

> [!TIP] A tip with its own title
> Text of the tip, with `code` and a [link](gfm-blocks.md).

> [!IMPORTANT]
> Important text.

> [!WARNING]
> A warning with a list:
>
> - first
> - second

> [!CAUTION]
> Caution text.

> [!info]- Folded by default
> Hidden until opened.

> [!success]+ Open and foldable
> Shown, can be folded.

> [!bug] A bug callout
> > [!question] Nested question
> > A callout inside a callout.

> [!unknownkind] An unknown kind
> Falls back to a note.

:::note A container with a title
A container holds a longer aside, with **strong** text.

- and a list
:::

:::tip
A tip container without a title.
:::

:::warning Nested content
```js
const inside = "a code block in a container";
```
:::

<details>
<summary>A plain details block</summary>

Text that most readers can skip.

</details>
