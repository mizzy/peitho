# Incremental Reveal

The published deck shows the final state; presenter mode steps through the same
content.

::: {reveal}

- Start from shared context
- Add one claim at a time
  - Nested evidence travels with its parent
- Close on the decision

:::

---

# Mixed Steps, Same Slide

This baseline stays visible while both reveal groups advance.

::: {reveal}

First, introduce the code path.

```rust
fn reveal_step(label: &str) -> String {
    format!("show {label}")
}
```

:::

The bridge text is also always visible between groups.

::: {reveal}

- Then reveal the checklist
- Keep numbering continuous across groups
- End with the next action

:::

---

# Static Slide for Contrast

Everything on this slide is visible from the start.

- No reveal fences
- No presenter steps
- Useful for summaries, reference material, or handoff content
