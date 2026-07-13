---
time: 15m
aspect_ratio: 16:9
---

<!-- {"section":"Introduction","time":"5m"} -->
# Starter Deck

A Peitho deck starts as Markdown. Edit `deck.md`, keep layouts in `layouts/`, and style the deck in `css/base.css`.

---

<!-- {"section":"Details","time":"10m"} -->
# Shape the Story

- Put one idea on each slide.
- Use speaker notes for presenter-only prompts.
- Let layout HTML define the content slots.

<!-- Speaker note: This note appears in presenter view and stays out of published slides. -->

---

# Compare Options

::: {slot=left}

## Content

- Markdown owns the message.
- Slide breaks stay readable in Git.
- Presenter notes live beside the slide.

:::

::: {slot=right}

## Design

- Layout HTML declares slots.
- CSS owns the visual system.
- The build checks both before rendering.

:::

---

# Code Slide

The default layout includes one code slot.

```rust
fn main() {
    println!("Hello from Peitho");
}
```
