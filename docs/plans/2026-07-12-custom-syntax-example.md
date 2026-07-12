# Custom syntax example (Issue #267)

Date: 2026-07-12

## Goal

Give custom syntax definitions (`syntaxes:` frontmatter / deck-adjacent `syntaxes/`
auto-detect, `.sublime-syntax` files augmenting the built-in set) a working example
deck. The pair of behaviors worth showing is: an unknown language tag is a
line-numbered parse error by design, and a `.sublime-syntax` file next to the deck is
the fix.

## Shape

`examples/custom-syntax/` — the deck's only asset is `syntaxes/`; layout and theme
stay built-in, which keeps the message "the one thing this deck adds is a grammar".

```
examples/custom-syntax/
  deck.md                    # no frontmatter; built-in layout and theme
  syntaxes/
    toml.sublime-syntax      # compact self-authored TOML grammar (~40 lines)
```

TOML is the demo language because it is real, common, and genuinely absent from
syntect's built-in set (measured: a bare ```` ```toml ```` fence fails the build with
`unknown code language 'toml'`). The grammar is written for this example, so there is
no license to carry; scopes are chosen to hit the `hl-*` classes the built-in theme
colors (`comment`, `string`, `constant`, `keyword`, `function`, `type`).

## Content

Four slides on the built-in layout: a cover that leads with an actually-highlighted
TOML block (the deck building at all proves the grammar loads — and it doubles as the
gallery screenshot), the exact build error an unknown tag produces, the `syntaxes/`
auto-detect convention, and a look inside the grammar itself (a YAML code slide).

## Wiring

- `Makefile`: add `custom-syntax` to `DEMO_DECKS`
- `site/content/examples/custom-syntax.md`: weight 66 (right after Code Images in the
  code-focused group)

## Non-goals

- No explicit `syntaxes:` frontmatter variant (the guide documents it)
- No custom theme CSS — built-in `hl-*` colors are part of the point
