# Slide page numbers (Issue #321)

## Motivation

Displaying "which slide is this" helps the audience (Q&A: "on slide N,
you said…") and the speaker (position-at-a-glance without leaning on
the presenter view). The feature is opt-in per deck.

## The three pillars — which one fires here

Pillar ① (separation of content and design) fires. Page numbers are a
*design* concern; they must live in layout CSS, not in the presentation
shell. The shell already renders index/timer chrome, but that is
speaker-only — audience-visible number chrome would blur the boundary.
So the mechanism is: the renderer bakes structured hints
(`data-peitho-page-number` / `data-peitho-page-total`) onto the
slide's `<section>` root, and `themes/base.css` renders them via a
pseudo-element the layout author can restyle or hide. The presentation
shell is not touched.

## Configuration surface

### Deck-level (frontmatter)

```yaml
page_numbers: current            # → "3"
page_numbers: current_of_total   # → "3 / 24"
# key omitted → off
```

Naming: `current` says exactly what it prints. `current_of_total` reads
as "current *of* total" — the value itself explains the format, unlike
the earlier `both` (which failed the "what are the two things?" test
from the design review).

Accepted values are exactly `current` and `current_of_total`. Any
other scalar — including `true`, `false`, an integer, an unrelated
string — is a line-numbered build error with the help text
`use "current" or "current_of_total"`. Rejecting `false` is
deliberate: the deck-level default is off, so `false` would be dead
config, and dead config is a well-known drift trap that the "no silent
path" invariant covers.

Frontmatter grammar constraint is unchanged: the key sits at the flat
top level, per the existing CLAUDE.md rule ("The frontmatter body is
restricted to flat `key:` lines, plus one nested `code_images:`
mapping level"). No new nested key.

### Per-slide (page settings JSON comment)

Extends the existing single-comment-per-slide JSON:

```html
<!-- {"page_number": false} -->
```

`false` hides the number on that slide (title cards, section
dividers). `true` is **rejected** as a line-numbered build error:
turning numbers on is a deck-level decision (`page_numbers` in
frontmatter), and per-slide `true` would mean "turn it on for this
one slide even though the deck is off" — a shape the design
deliberately does not offer, because it would let a deck without deck-
level opt-in still stamp numbers on one slide. The one-way opt-out
shape mirrors the existing `draft` / `skip` flags (both are
one-directional booleans).

The comment continues to obey the single-comment-per-slide rule and
`serde(deny_unknown_fields)`, so typos are already caught. The
allowed-keys help string in `parse_page_comment` gains
`"page_number":false`.

### Rejection matrix (all line-numbered build errors)

| Condition                                                                | Reason                                                                                                          |
| ------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------- |
| `page_numbers` is any value other than `current` / `current_of_total`    | Rejected values (incl. `false` / `true`) would silently do nothing or be ambiguous — force the author to choose |
| `{"page_number": true}` on any slide                                     | Enabling is deck-level; per-slide `true` would let a deck without opt-in still print a number                   |
| `{"page_number": false}` when the deck has no `page_numbers` frontmatter | Silent no-op — the opt-out cannot hide something that is already off                                            |
| `{"page_number": false}` combined with `{"draft": true}` on same slide   | Draft slides never render — the flag is noise (same shape as the existing `draft`+`skip` error)                 |

`{"page_number": false}` combined with `{"skip": true}` is **accepted**
— skip slides render, so hiding the number on them is meaningful (e.g.
appendix slides you might jump to during Q&A but don't want to spoil
with a running count).

## Numbering semantics

- **Draft slides** are already dropped at parse end *before* the final
  `ParsedSlide.index` assignment (per the `draft`/`skip` design
  record). They never reach the numbering seam. Total naturally
  excludes them.
- **Skip slides** stay in the deck, in output, in PDF, in publish
  (per the same design record). They **receive a number** and count
  toward the `current_of_total` denominator. Excluding them from the
  denominator would allow `24 / 22`-style displays where the printed
  current exceeds the printed total — the audience would read that as
  a bug.
- **Consequence**: page number = final `index + 1`, total = slide
  count. No new numbering pipeline exists — both values are already
  present when the renderer runs.

## Contract changes

Rust:

```rust
// crates/peitho-core/src/phase.rs (DeckSettings)
enum PageNumberFormat { Current, CurrentOfTotal }

pub struct DeckSettings {
    // …existing fields…
    page_numbers: Option<PageNumberFormat>,
}
```

```rust
// crates/peitho-core/src/phase.rs (ParsedSlide, MappedSlide, CheckedSlide)
pub struct ParsedSlide {
    // …existing fields…
    pub page_number_hidden: bool,   // rides Parsed→Mapped→Checked like `skip`
}
```

Manifest / bindings:

- `ManifestSlide` gets **no** new field. The value is derivable from
  `index + 1`, and adding it to the manifest would invite a
  shell-side rendering path — the very approach rejected up front.
  The exception would be if some legitimate manifest consumer needed
  the number without knowing the deck size; none exists today.
- ts-rs regenerates `bindings/` because `DeckSettings` changed.
  `page_numbers` is `Option<PageNumberFormat>` → the TS type must
  reflect it (the presentation shell doesn't read it, but the
  bindings check gates drift).

## Rendering

`crates/peitho-core/src/render.rs` already runs an `element!("section", …)`
handler that stamps `data-slide-key`. Extend the same handler:

- When the deck's `page_numbers` is `Some(_)` **and** the slide's
  `page_number_hidden` is `false`, set `data-peitho-page-number` to
  `index + 1`.
- When the deck's `page_numbers` is `Some(CurrentOfTotal)` and the
  slide is not hidden, additionally set `data-peitho-page-total` to
  the total slide count (`checked.slides().len()` passed through).
- Hidden slides get no attribute (CSS selector `[data-peitho-page-
  number]` naturally skips them; no separate hide rule needed).

Baking at this seam covers every downstream artifact — build, preview
cache, present cache, PDF export, publish — automatically.

`themes/base.css` gains:

```css
.peitho-slide[data-peitho-page-number]::after {
  content: attr(data-peitho-page-number);
  position: absolute;
  right: 24px;
  bottom: 20px;
  font-size: 18px;
  color: #8a8578;
  font-variant-numeric: tabular-nums;
}

.peitho-slide[data-peitho-page-total]::after {
  content: attr(data-peitho-page-number) " / " attr(data-peitho-page-total);
}

.peitho-slide { position: relative; }  /* so ::after can absolute-position */
```

Why `attr()` and not a CSS custom property: `content:` cannot render a
numeric custom property as text. `content: var(--n)` requires the
custom property to already be a string; `attr()` on a data attribute
is the mechanism CSS actually provides for text-from-DOM.

Layout authors override with any standard CSS mechanism:

```css
/* Hide numbers in a specific layout */
.peitho-slide[data-peitho-page-number]::after { display: none; }

/* Restyle */
.peitho-slide[data-peitho-page-number]::after { color: #fff; left: 24px; right: auto; }
```

## Phase-by-phase changes

1. **parser** (`crates/peitho-core/src/parser.rs`)
   - Add `page_numbers` to `DeckFrontmatter` with a custom
     deserializer that accepts only `"current"` / `"current_of_total"`
     (bare `visit_str` returning a typed enum, mirroring
     `deserialize_optional_planned_time`).
   - Add `page_number` to `PageComment`; validate at
     `parse_page_comment`: reject `true`, and thread the parsed value
     onto `PendingSlide` so combination checks (deck-level required,
     draft-mutex) can run at the same point as `draft`+`skip`.
   - Extend `parse_page_comment`'s allowed-keys help string to
     include `"page_number":false`.
2. **flag through phases** (`phase.rs`, `mapping.rs`, `check.rs`)
   - `ParsedSlide.page_number_hidden: bool` → `MappedSlide` →
     `CheckedSlide` (public getter, matches `skip`).
   - No behavior in mapping/check — the flag is pass-through.
3. **renderer** (`render.rs`)
   - Take the total from the caller (already computed) and stamp both
     attributes on the `<section>` root inside the existing
     `element!("section", …)` handler, gated by
     `settings.page_numbers()` and per-slide `page_number_hidden`.
4. **theme** (`themes/base.css`)
   - Add the pseudo-element rule described above.
   - Regenerate the embedded copy check: `BUILTIN_BASE_CSS` uses
     `include_str!("../../../themes/base.css")` so no rebuild step is
     needed beyond editing the file.
5. **bindings** — ts-rs generates the TS enum + optional field.

## Tests (TDD order)

Author each test first, watch it fail, then make it pass.

### Parser

1. `page_numbers: current` in frontmatter parses to
   `Some(PageNumberFormat::Current)`.
2. `page_numbers: current_of_total` parses to
   `Some(PageNumberFormat::CurrentOfTotal)`.
3. `page_numbers: both` → line-numbered error naming allowed values.
4. `page_numbers: false` → same error (dead config).
5. `page_numbers: true` → same error.
6. Key omitted → `None`.
7. `{"page_number": false}` on a slide → `page_number_hidden = true`.
8. `{"page_number": true}` on a slide → line-numbered error.
9. `{"page_number": false}` on a `{"draft": true}` slide → line-numbered
   error (draft slides never render).
10. `{"page_number": false}` on a `{"skip": true}` slide → accepted;
    both flags set on the resulting slide.
11. `{"page_number": false}` in a deck without deck-level
    `page_numbers` → line-numbered error naming the absence.

### Rendering

12. Deck with `page_numbers: current`, three slides → each rendered
    `<section>` has `data-peitho-page-number="1"`, `"2"`, `"3"`; none
    has `data-peitho-page-total`.
13. Deck with `page_numbers: current_of_total`, three slides → each
    has both `data-peitho-page-number` and
    `data-peitho-page-total="3"`.
14. Slide with `{"page_number": false}` → the `<section>` has neither
    attribute.
15. Deck with skip slide → skip slide has `data-peitho-page-number`
    and counts in the denominator.
16. Deck with draft slide → total excludes the draft; remaining
    slides number 1..N-1 (the existing draft-drop test proves the
    index reassignment; this test asserts the total).

### Manifest / bindings

17. `ManifestSlide` schema test unchanged — assert no `page_number`
    field appears.
18. `bindings/` diff test still passes (regenerated for the
    `page_numbers` frontmatter enum only).

### CSS integration

19. Snapshot test on the emitted `<section>` shape for both
    `current` and `current_of_total`. No test of pixel positioning
    (that's what E2E is for).

## Verification (gates from CLAUDE.md)

- `cargo test --workspace` × 3 (race sensitivity).
- `cargo clippy --workspace --all-targets -- -D warnings`.
- `cargo fmt --all --check`.
- `git diff --exit-code bindings/` after ts-rs run.
- `cd packages/peitho-present && npm run build && npm test && npm run typecheck`.
- Present / preview / remote shell drift diffs (expected: no
  changes — the shell is not touched).
- **E2E**: real browser check on a deck that exercises each mode.
  - `page_numbers: current` shows just a number.
  - `page_numbers: current_of_total` shows `N / M`.
  - `{"page_number": false}` slide shows nothing.
  - Skip slide shows a number and counts in the denominator.
  - PDF export carries the numbers on every page.
  - Layout override (`display: none` in a custom `base.css`)
    successfully hides the numbers.

## Explicitly out of scope

- Shell-side overlay rendering (crosses the content/design boundary).
- Roman numerals or other custom formats (revisit only if requested).
- Presenter-view page indicator changes (already shows index).
- Slidev-style `{N}` mustache in Markdown body (would leak numbering
  into content — pillar ① violation).

## Long-term / type-safety self-check

- **Root-cause seam**: renderer stamps once at `<section>` creation.
  No consumer downstream needs a filter/guard.
- **Broken state unrepresentable**: `page_numbers` is a typed enum
  (`Option<PageNumberFormat>`), not a raw string. Adding a new format
  requires editing the enum, which forces every consumer to be
  updated.
- **New caller tomorrow**: any future artifact (PDF, publish, present)
  that goes through the renderer inherits the attributes
  automatically. There is no per-consumer "remember to render page
  numbers" step.
- **Manifest not extended**: derivable state stays out of the wire
  contract, blocking the shell-overlay drift path.
