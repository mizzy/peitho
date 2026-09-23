---
time: 2m
---

<!-- {"layout":"video-cover"} -->

# Video backgrounds

A layout can reference a file directly. peitho resolves it like any other asset.

---

<!-- {"layout":"plain"} -->

# How it works

The layout writes the reference itself. This deck's Markdown never names it.

- peitho resolves it, copies it under a content-hashed name, and rewrites the tag
- Works for `<img>`, `<script>`, `<link>`, and the rest — not just `<video>`
- A path that does not exist is a **build error naming the attribute**
