---
time: 3m
---

<!-- {"layout":"counter"} -->

# A slide that counts

The button is wired by the layout's own script.

---

<!-- {"layout":"counter"} -->

# Two slides, one layout

Each slide keeps its own count; the script mounts every slide as it is announced, once.

---

<!-- {"layout":"plain"} -->

# How it works

- The layout's `<script>` runs on every surface.
- It finds its slide through `peitho:shadow-mounted` and `window.__peithoShadowRoots`.
- Install once, mount each root once.
