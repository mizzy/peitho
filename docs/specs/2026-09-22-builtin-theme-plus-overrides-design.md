# Layering CSS on top of a theme

Design record for Issue #620. Author decision 2026-09-22.

## The problem

A deck that wants the built-in theme *plus* a few tweaks has no way to say so.
A deck-adjacent `css/` directory replaces `themes/base.css` wholesale, so the
only way to keep the theme is to vendor a byte-identical copy of it and append
to that copy.

`peitho new` institutionalises this: it copies the entire built-in theme into
every scaffolded deck under a header reading "This file replaces peitho's
embedded themes/base.css for this deck."

Measured consequences in this repository:

- `examples/footnotes/css/base.css` is a verbatim copy of the theme that was
  never customised. Its own contribution is the separate 8-line
  `css/overrides.css`. The copy is identical to `themes/base.css` through line
  212; only the trailing emphasis block has fallen behind.
- 36 commits touched `themes/base.css`; 14 of them did not touch that copy.
  The copy was hand-synced three times.
- Nothing in CI notices. The SHA-256 example snapshot hashes slide HTML only,
  so CSS-only divergence is invisible.

The copy is the symptom. The root cause is that "theme + overrides" is not
expressible, which forces the copy in the first place.

Be precise about what this fixes, though: it is an **expressiveness** fix, not
a typed guarantee. After this change a deck *can* keep the theme without
copying it, and `examples/footnotes` does. Nothing makes the broken state
unrepresentable — a deck may still vendor a copy into `css/` tomorrow and let
it rot. That is accepted rather than solved, because the obvious enforcement
(a CI check that no example CSS matches the theme byte for byte) has no honest
answer here: the other 15 example stylesheets legitimately differ from the
theme by 203–330 lines each, so "is this fork intentional?" is not a question
byte comparison can answer. CSS layering is a policy, not an invariant.

This also contradicts the kickoff spec. §8 describes CSS as two layers — a
shared **base theme** and a deck-specific **per-slide override** — and
`themes/` ships `base.css` and `overrides.css` side by side. The two-layer
model exists in the vocabulary; it is only the asset-resolution implementation
that collapses it.

## The rule

A new asset key, `overrides`, resolving exactly like the four that already
exist:

1. explicit `overrides:` frontmatter path
2. deck-adjacent `overrides/` directory
3. nothing

Whatever it resolves to is appended **after** the resolved `css` target,
whether that target is the built-in theme or the deck's own stylesheets. The
existing `css` key is untouched: a `css/` directory still replaces the built-in
theme, exactly as it does today.

```
css/        absent  + overrides/ present  ->  built-in theme, then overrides
css/        present + overrides/ present  ->  deck's css, then overrides
css/        present + overrides/ absent   ->  deck's css                (unchanged)
css/        absent  + overrides/ absent   ->  built-in theme            (unchanged)
```

`overrides` reads `*.css` in filename order and accepts a file or a directory,
identical to `css`. An explicit path that does not exist is a line-numbered
build error, identical to the other four keys.

### Why a separate key and not a rule about `css/`'s contents

The first draft of this design made `css/` inherit the theme when its only
stylesheet happened to be named `overrides.css`. That was rejected on
consistency grounds before implementation.

Every existing asset key decides one thing from one signal: **is the directory
there?** `layouts/` present means layouts, absent means built-in. Deciding
instead from *the set of file names inside* a directory would introduce a
judgement rule that no other asset key has, and it breaks in ways the
directory-presence rule cannot:

- Adding a second file to such a directory would silently switch it back to
  replacement and the theme would vanish — a file addition changing the
  meaning of every other file in the directory.
- It could not explain why `css: ./css/overrides.css` naming the same file
  directly should behave differently from the directory containing it.
- "`base.css` next to `overrides.css` keeps replacing" had no principled
  justification; it was only a description of what four existing example decks
  happen to contain.

A second key keeps the single signal — directory presence — and adds no new
kind of decision anywhere. Composition becomes visible in the deck's own
directory listing rather than inferred from file names.

### Why not a frontmatter-only flag

A `css_extends: builtin` style key would put the decision in two places: the
directory layout already expresses intent for every other asset, and zero-config
deck-adjacent detection is the established convention (Issue #17). `overrides:`
exists as a frontmatter key too, but only as the explicit form of the same
resolution every other asset key offers.

A CSS-side `@import "peitho:base";` was rejected for inventing non-standard CSS
syntax inside author stylesheets.

## Validation

`build_theme_css` is unchanged. It receives the combined list — theme files
first, override files after — and runs selector-contract validation and
root-class size checks across all of them exactly as it does for a vendored
`base.css` plus `overrides.css` today. A selector in an override file that
targets a slot the layout does not define stays a line-numbered build error.

Built-in theme files keep the diagnostic name `base.css (built-in)`.

`theme-fonts/` is already written unconditionally on every build, so the
`@font-face` rules in the built-in theme resolve for layering decks with no
change to asset emission.

## Consequences elsewhere

- `AssetKey::ALL` gains a fifth member, so `peitho doctor` reports the new key
  automatically through the existing loop in `doctor.rs`.
- The preview watcher tracks asset files through `collect_asset_files` per
  resolved target, so the new target is watched by the same mechanism once it
  is part of resolution; the deck-adjacent `overrides/` candidate needs the
  same explicit `Missing` state the other four have, so that *creating* the
  directory triggers a rebuild.
- `ResolvedAssets` gains a field. It is a plain struct with four `Provenance`
  values today; the fifth is the same shape.

## Scope

In scope:

- The `overrides` asset key: resolution, loading, appending after `css`.
- `peitho doctor` coverage and preview-watch coverage falling out of the
  above.
- Migrating `examples/footnotes`: delete the vendored `css/base.css` and move
  `css/overrides.css` to `overrides/overrides.css`. Measured on the parent
  commit: slide HTML is byte-identical and the deck then renders on the
  built-in theme plus its own 8 lines, with the stale `.code-line` rules gone.

Out of scope, deliberately:

- **`peitho new`** (author decision 2026-09-22, after adversarial review pushed
  back on the deferral). `new_cmd.rs`'s `base_css` copies the whole built-in
  theme into every scaffolded deck and then appends its variant blocks — it is
  literally theme-plus-overrides done by string concatenation, and its own test
  is named `dark_theme_appends_overrides_after_the_embedded_base_css`. The
  objection is therefore fair: this is the *upstream* site, not a sibling site,
  and leaving it means the binary carries two implementations of the same idea,
  one typed and one concatenated.

  It stays out anyway, deliberately. What a scaffolded deck should contain is a
  question about the default authoring experience — handing the author the whole
  theme to edit is a defensible default, and the answer does not follow from
  this feature being available. It is better decided after the key has been used
  in anger than bundled with the change that introduces it. Filed as a
  follow-up issue referencing this PR so the remaining hazard is tracked rather
  than assumed away.
- Any change to `css` / `layouts` / `syntaxes` / `fonts` resolution.
- A CI check comparing example CSS to the theme. With this feature the
  vendored copy is gone, and "is this fork intentional?" has no
  byte-comparison answer: the other 15 example stylesheets legitimately differ
  from the theme by 203–330 lines each.

## Compatibility

No existing deck changes behaviour. `overrides/` does not exist anywhere in
the repository, and no deck declares an `overrides:` frontmatter key, so every
current deck resolves exactly as it does today. The four example decks that
ship a full theme alongside a file named `css/overrides.css` (`code-images`,
`code-walkthrough`, `custom-fonts`, `peitho-tour`) keep reading both files
from `css/` as one replacement set — the new key is not involved.
