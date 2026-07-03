# Example gallery (add 3 patterns with different content/design)

## Purpose

`examples/` has only one deck, which doesn't make a convincing case for "separation of content and design." Add 3 self-contained samples that all differ in content, template structure, and theme, to demonstrate that templates are schemas (variations on slot contracts) and to show practical examples of keyed overrides.

## Structure

The existing `examples/deck.md` remains as the minimal sample that works with the default flags (`templates/`, `themes/`) (it's also a test fixture). The new samples are self-contained in their own directories:

```
examples/
  deck.md              # minimal (works with default flags)
  lightning-talk/      # Japanese lightning talk. Text only, template with no code slot
  code-walkthrough/    # English code walkthrough. code arity=1 two-column, practical use of keyed overrides
  keynote/             # Japanese keynote. Centered editorial style
    deck.md
    template.html
    base.css
    overrides.css
```

What each sample demonstrates:

| Sample | Contract highlight | Design |
|---|---|---|
| lightning-talk | No code slot = writing code causes a build error | Dark, poster-style with large typography |
| code-walkthrough | `code accepts="code" arity="1"` = code required on every slide | Terminal-style. overrides.css emphasizes the code on the payoff slide |
| keynote | Minimal contract with only title+body | Cream background + serif + centered |

## Constraints (follow the already-implemented spec)

- The template has exactly one `<section>`. The renderer injects the `peitho-slide` class and data-slide-key
- The only classes overrides.css selectors can use are slot classes (`.slot-*`). Keys must be ones that actually exist in the deck
- All themes use a fixed 1280x720 canvas, overflow hidden, system fonts only (works offline)

## Verification

Run `peitho build` on each sample and visually check all slides in an actual browser (look at screenshots for overflow/collapse issues). Add the sample list and build commands to the README.
