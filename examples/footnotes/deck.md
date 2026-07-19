<!-- {"key":"footnotes-title"} -->
# Footnotes for Dense Slides

Keep the slide readable while preserving the source behind a claim.

<!--
Frame footnotes as a presentation affordance: they carry supporting detail
without forcing every source into the main visual hierarchy.
-->

---

# Claims stay short

- A claim can stay crisp while the evidence stays nearby[^study]
- The same source can support a second point without a new number[^study]
- A separate source receives the next number by first reference[^survey]

[^study]: The same label keeps the same number every time it appears on this slide.
[^survey]: Numbering follows first-reference order, not definition order.

<!-- Point out that repeated references are useful when one source supports several bullets. -->

---

# Notes work inside lists

- Define a term where it appears[^term]
- Keep URLs out of the main line[^source]
- Reuse labels freely on another slide; scope is per slide[^term]

[^term]: Footnote bodies support inline Markdown: *emphasis*, `code`, and [links](https://example.com).
[^source]: Undefined references are build errors, so a missing citation never ships silently.

---

<!-- {"layout":"literal-flow"} -->
# Literal markers stay literal

Use code when the marker is part of the subject.

`[^x]` is a code span, not a citation.

Even a regex character class such as `/^[^a-z]+$/` stays literal inline.

```
/^[^a-z]+$/
```

::: {slot=outro}

The actual citation still resolves[^regex].

:::

[^regex]: Escaped markers and code-delimited markers stay literal; real references remain checked.
