# peitho new: scaffold a starter deck

Issue #246. Adds `peitho new [<dir>] [--layouts default|split|cover] [--theme light|dark] [--force]`,
which generates a deck directory where `peitho preview` Just Works with zero
frontmatter, riding the existing deck-adjacent auto-detect convention.

## Decisions already made in the issue

- Templates are in-tree, embedded via `include_str!`. Nothing is fetched.
- Refuse to write into an existing non-empty target directory unless `--force`.
- Generated directory names (`layouts/`, `css/`) match the auto-detect roots,
  so the generated `deck.md` needs no asset frontmatter.

## Facts from the current codebase that shape the design

- **Deck-adjacent `layouts/` and `css/` fully replace the embedded defaults**
  (`load_layouts` / `load_css` in `crates/peitho/src/main.rs`): when the
  directory exists, the built-in `title-body-code` layout and embedded
  `base.css` are not loaded at all. Therefore the scaffold must emit a
  *complete* theme and a complete layout set, not deltas.
- **Root-class size lint (Issue #260)**: layout root `<section>` classes must
  not declare `width`/`height` at the same specificity as `.peitho-slide`
  (the renderer adds `.peitho-slide` to the root alongside the layout's own
  classes). Scaffold CSS never sizes the layout root classes.
- **Hybrid layout dispatch**: with two layouts present, a heading-only slide
  matches both `cover` (title only) and `title-body-code` (title + optional
  body/code) — ambiguity is a build error, so the cover variant's title slide
  carries an explicit `{"layout":"cover"}`. Slides with body content uniquely
  match `title-body-code`; slides using `::: {slot=left/right}` uniquely match
  `two-column`. A slide accepts at most ONE page settings comment, so the
  cover slide merges `layout` + `section` + `time` into a single JSON comment.
- **Sections**: if any section marker exists the first slide must carry one,
  and section times must sum to the frontmatter `time`. The scaffold uses
  `time: 15m` with `Introduction 5m` + `Details 10m`.
- **CSS validation**: bare `.slot-*` selectors are checked against the union
  of provided layouts. `title-body-code.html` is generated in every variant,
  so the base `.slot-title/.slot-body/.slot-code` rules always validate;
  `.slot-left/.slot-right` rules are only emitted together with
  `two-column.html`.

## CLI shape

```
peitho new [<dir>] [--layouts <default|split|cover>] [--theme <light|dark>] [--force]
```

- `<dir>` defaults to `.`. Created (with parents) if missing.
- If the target exists and is non-empty, error with help suggesting `--force`
  (or a fresh directory). `--force` writes the scaffold files, overwriting
  same-named files and leaving everything else alone.
- `--layouts` and `--theme` are clap `ValueEnum`s; defaults `default`/`light`.
- On success, print the generated file list and a next-steps hint
  (`cd <dir> && peitho preview` — omit the `cd` when dir is `.`).

## Generated tree

Every variant:

- `deck.md` — frontmatter (`time: 15m`, `aspect_ratio: 16:9`), a title slide,
  a section-marker example (two sections summing to 15m), a speaker-note
  comment, and a code-slot slide. Variant `split` appends a two-column slide
  using `::: {slot=left}` / `::: {slot=right}`; variant `cover` renders the
  title slide heading-only with `{"layout":"cover", ...}`.
- `layouts/title-body-code.html` — byte-identical copy of the embedded
  default (`include_str!` of the same repo file `layouts/title-body-code.html`;
  single source, cannot drift).
- `layouts/two-column.html` (split only), `layouts/cover.html` (cover only) —
  new scaffold templates, simpler than the example decks' versions.
- `css/base.css` — assembled by concatenation, in order:
  1. a header comment stating this file replaces peitho's embedded base
     (`themes/base.css`) and is the author's to edit,
  2. the embedded base theme (`include_str!` of `themes/base.css`),
  3. the extra layout's styles (split/cover only; no `width`/`height` on the
     layout root classes per the #260 lint),
  4. the dark override blocks (`--theme dark` only): a layout-independent
     base block flips `.peitho-slide` background/foreground and the `.hl-*`
     palette via the normal cascade, and a per-layout dark block (split/cover
     only) restyles that layout's surfaces — dark CSS never references a
     layout that is not generated. Light emits nothing extra — light *is*
     the embedded base.
- `.gitignore` — `dist/` and `.peitho/`.

## Template layout

New scaffold-only template files live under `crates/peitho/templates/new/`
and are embedded with `include_str!`:

- `deck-default.md`, `deck-split.md`, `deck-cover.md`
- `two-column.html`, `cover.html`
- `two-column.css`, `cover.css`, `dark.css`, `dark-two-column.css`,
  `dark-cover.css` (the appended blocks; the `dark-*` pair are the
  per-layout dark overrides)
- `gitignore` (written as `.gitignore`)

The default layout HTML and light base CSS reuse the already-embedded repo
files — no copies added.

## Implementation seam

A `new` module in the CLI crate (`crates/peitho/src/new_cmd.rs` or similar)
exposing a pure planning function (variant + theme → list of
`(relative path, content)`) and a thin `run` that does the directory checks
and writes. The pure function is the unit-test surface; the emptiness/force
rules are tested against temp dirs.

## Tests (TDD order)

1. Planning function: default variant yields exactly
   `deck.md`, `layouts/title-body-code.html`, `css/base.css`, `.gitignore`;
   split/cover add exactly their layout file; dark appends the override block;
   generated layout HTML for the default equals the embedded built-in.
2. Directory rules: missing dir is created; empty existing dir is fine;
   non-empty dir errors mentioning `--force`; with `--force` files are
   (over)written and unrelated files survive.
3. **End-to-end per combination (the issue's acceptance bar)**: for all six
   `--layouts` × `--theme` combinations, scaffold into a temp dir and run the
   real build pipeline on the generated deck — it must succeed. This pins
   dispatch uniqueness, section-time arithmetic, CSS selector validation, and
   the #260 root-class lint all at once.
4. CLI wiring: `peitho new` appears in `--help`; the subcommand runs the
   scaffold (covered via the existing main.rs test patterns).

## Non-goals

- No template fetching, no interactive prompts, no `--title` flag (not in the
  issue's proposal).
- No new example deck and no docs-site page in this PR (the guide can gain a
  "getting started" mention separately if the author wants one).
