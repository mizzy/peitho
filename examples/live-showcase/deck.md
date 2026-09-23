---
time: 5m
---

<!-- {"layout":"cover"} -->

# Slides that move

Markdown content, layout scripts, no framework.

---

<!-- {"layout":"chart"} -->

# Stars over the year

Drawn by the layout from JSON in the Markdown, replayed on every visit.

```json
{
  "Jan": 120, "Feb": 180, "Mar": 260, "Apr": 310,
  "May": 420, "Jun": 510, "Jul": 640, "Aug": 780
}
```

---

<!-- {"layout":"playground"} -->

# Run it on stage

```js
function fib(n) {
  return n < 2 ? n : fib(n-1) + fib(n-2);
}
for (let i = 0; i < 8; i++) {
  console.log(`fib(${i}) =`, fib(i));
}
```

---

<!-- {"layout":"plain"} -->

# How it works

- The Markdown holds only content: a title, a sentence, a JSON block, a code block.
- Each layout's `<script>` finds its slide through `peitho:shadow-mounted`.
- The chart reads its data from the rendered code slot, so the numbers stay in Markdown.
- PDF export prints each script's final frame.
