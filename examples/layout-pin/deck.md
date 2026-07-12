<!-- {"key":"cover","layout":"spotlight"} -->
# The author picks the layout

This deck's two layouts accept exactly the same shape — so every slide here carries an explicit `{"layout":"…"}` pin.

<!-- Both layouts declare title + body + optional code. Structural dispatch has nothing to distinguish them, which is deliberate. -->

---
<!-- {"key":"dispatch","layout":"statement"} -->
# Three ways a slide finds its layout

- An explicit `{"layout":"name"}` pin in the slide's page settings wins
- With a single layout, dispatch is unconditional
- Otherwise the slide's structure must match exactly one layout

Ambiguous or zero matches are never silently resolved.

---
<!-- {"key":"ambiguous","layout":"statement"} -->
# Same shape, no pin? The build stops

Remove this slide's pin and the build fails with a line number:

```
× slide 3 ('ambiguous'), line 20: slide matches multiple layouts:
  spotlight, statement
  = help: pick one explicitly with <!-- {"layout":"…"} -->
```

---
<!-- {"key":"typo","layout":"spotlight"} -->
# Unknown names list the candidates

Misspell a pin and the error hands you the layouts that exist.

```
× slide 4 ('typo'), line 31: unknown layout 'sptolight'
  = help: use one of: spotlight, statement
```
