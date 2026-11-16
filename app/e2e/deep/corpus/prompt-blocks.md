---
title: A prompt file
tags: [prompt]
---

# Prompt blocks

<system>

You are a careful assistant writing for {{audience}} about week {{week}}.

</system>

<context>

- The data sheet: @station/readings.toon
- The style guide: @style-guide.md

</context>

<instructions>

1. Read the context.
2. Write no more than {{max_words}} words.

</instructions>

<example>
A short example answer.
</example>

Text after the blocks, with a `{{variable}}` in code that stays as it is.
