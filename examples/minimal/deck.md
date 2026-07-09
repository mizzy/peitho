<!-- {"key":"arch-1"} -->
# Peitho Architecture

Markdown is the source of truth, while HTML and CSS own layout.

```rust
enum Phase { Parsed, Mapped, Checked, Rendered }
```

<!--
Open with the two-line pitch: separation of content and design,
enforced by the typestate pipeline.
-->

---

# Convention Mapping

- Shallowest heading maps to title
- Code blocks map to code
- Remaining blocks map to body

<!-- Remind the audience: no config here — it's convention over configuration. -->

---
<!-- {"key":"dist-1"} -->
# Distribution

The build output writes slide fragments plus a manifest.
