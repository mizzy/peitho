---
time: 3m
---

# Two columns via explicit slots

::: {slot=left}

Convention mapping alone cannot pick between `left` and `right`.

When two `accepts=blocks` slots exist, there is no way to decide where content belongs.

:::

::: {slot=right}

`::: {slot=name}` lets the author declare it.

- A Markdown-native extension (no HTML tags leaked in)
- Carried by types out of the parser
- No silent drops

:::

---

# Compare: convention vs explicit

::: {slot=left}

## Convention mapping

Headings go to `title`, code goes to `code`, everything else goes to `body`. A layout with a single slot needs no extra notation at all.

:::

::: {slot=right}

## Explicit selection

The author writes `::: {slot=name}` to encode intent. Three colons open and close, and the only attribute is `{slot=name}`.

Use it when a layout has multiple `blocks` slots and convention cannot resolve the ambiguity.

:::

---

# Errors come back with a line number and help

::: {slot=left}

## Parse stage

- Unclosed fence
- Malformed attribute
- Nested divs (not supported in v1)
- Empty `:::` block
- Four-or-more-colon fence (reserved)

:::

::: {slot=right}

## Mapping stage

Naming a slot that the layout does not declare is an immediate error.

- "unknown slot 'middle' in explicit `::: {slot=...}` for layout 'two-column'"
- help: "use one of: left, right, title"

:::
