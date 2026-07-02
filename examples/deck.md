<!-- {"key":"arch-1"} -->
# Peitho Architecture

Markdown is the source of truth, while HTML and CSS own layout.

- Markdown content stays separate from design
- Template slots are checked before render

```rust
enum Phase {
    Parsed,
    Mapped,
    Checked,
    Rendered,
}
```
