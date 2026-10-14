# Present-only slides (Issue #702)

## Goal

A slide marked `<!-- {"present_only":true} -->` appears in `peitho present` and
`peitho preview` but in no shareable output: `peitho build` (`dist/`, therefore
`peitho publish`), `peitho export pdf`, and `peitho lint` exclude it.

## Author decisions (2026-10-14)

- Whole slides only (no block-level variant).
- build/publish/PDF/lint exclude; present/preview include. No CLI switch.
- `present_only` + `section` is a line-numbered build error, like `draft`.
- Name: `present_only`.

## Design

- `peitho_core::Audience::{Live, Public}` is a required argument of
  `parse_deck_and_transform`, the only parse entry that leads to rendering.
  `Public` drops present-only slides at parse end, right after the draft drop
  and before final index assignment and section resolution, so no downstream
  phase can observe them (same seam as draft, not a filter per output).
  Derived keys are parse-time identity and are not re-derived after the drop.
- `parse_deck` (preview saves) and `referenced_image_paths` (watch inputs)
  parse as `Live`: saves must find every slide and watch inputs are a superset.
- The CLI chooses through the closed `BuildTarget::{Public, Present, Preview}`,
  so a public build can never carry preview edit annotations and every build
  names its audience. `build`, `build --watch`, `export pdf`, and `lint` use
  `Public`; `present` uses `Present`; `preview` uses `Preview`; `layouts`
  explains dispatch for every slide (`Live`).
- Keyed CSS: present-only keys ride `DeckSettings` into `OverrideKeys`. In a
  public build a selector whose keys all name present-only slides is accepted
  without slot validation (the live build validates it); the rule text stays in
  the stylesheet (documented, CSS rewriting needs a CSS parser).
- Errors: `draft` + `present_only`, `present_only` + section marker, and a
  public build with every non-draft slide present-only.

## Tests

- Parser: public drop (indexes, `source_index`, keys, notes), live keep,
  section ranges per audience, all-present-only error, `false` value, the two
  combination errors.
- CLI: public artifacts omit the slide and its text and accept keyed CSS for
  it; present and preview artifacts keep it.
