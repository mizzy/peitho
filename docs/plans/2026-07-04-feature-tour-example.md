# feature-tour example implementation plan (2026-07-04)

## Motivation

A coverage inventory of the existing four examples surfaced features that none of them exercise:

1. Explicit layout selection `{"layout":"name"}` (the top-priority rule of hybrid dispatch, yet no example uses it)
2. `accepts="list"` slots
3. Multiple note comments per slide (blank-line join)
4. Syntax highlighting for non-Rust languages (every code block uses `rust`)
5. Inline decorations (emphasis, links) and nested lists
6. Dispatch across three or more layouts

Add a fifth example, `examples/feature-tour/`, that covers all of these in a single deck. The subject is a self-referential tour of peitho's own features, in English. Confirmed with the author (2026-07-04).

As a side finding from the investigation, `Accepts::Image`/`Accepts::Text` have no path where the parser produces Image/Text fragments (Markdown images fail with `unsupported construct`), so they are currently unreachable vocabulary from Markdown. That is an author-judgment matter (whether to support images) that an example cannot fill, so it is out of scope for this plan.

## Deck design (7 slides, 4 layouts)

frontmatter `time: 8m`. Sections: Basics 2m (S1-2) / Contracts 3m (S3-4) /
Presenting 3m (S5-7). Total 8m = frontmatter value (passes the parse-end validation).

Layouts and contracts:

| layout | slots | Structural match shape |
|---|---|---|
| `cover` | title inline 1 | heading only |
| `topic` | title inline 1, body blocks 1..* | heading + paragraph/list |
| `agenda` | title inline 1, body list 1..* | heading + list only |
| `code-demo` | title inline 1, body blocks 0..*, code code 1..* | includes code |

**Design pivot**: a list-only slide structurally matches both `topic` (blocks accepts List) and `agenda`, hitting the ambiguity error. S3 deliberately steps on this and resolves it with the explicit `{"layout":"agenda"}` selector — showing the "ambiguity is never silently resolved; explicit selection is required" rule in a real deck.

Slides:

1. cover structural match. **Two** note comments (blank-line join demo). key+section
2. topic. Uses **bold**, *italic*, [links], and nested lists in the body
3. agenda explicit selection (as above). List of what check catches
4. code-demo. **Two code blocks** (rust + typescript) — self-referential nod to arity 1..* and the ts-rs contrast
5. code-demo. bash CLI demo (three commands)
6. topic. Explains time management, sections, and notes (quoting this deck's own settings)
7. cover. Closing (no notes = presenter's dimmed placeholder is also visible)

Syntax highlighting languages: rust / typescript / bash (syntect-recognized tokens; unknown tags are build errors, so `peitho build` is the check).

## Theme

A "light product tour" tone that does not overlap with the existing four (ivory default / dark poster / terminal / cream serif): white background + indigo accent + system sans-serif. Follows the `.peitho-slide` 1280x720 convention.

CSS validation demo:
- `base.css`: bare `.slot-*` (validated against the union of slots across all layouts)
- `overrides.css`: two or more keyed selectors (`[data-slide-key="..."]` is validated against the slots of that slide's layout)

## Files changed

- `examples/feature-tour/deck.md`
- `examples/feature-tour/layouts/{cover,topic,agenda,code-demo}.html`
- `examples/feature-tour/css/{base,overrides}.css`
- `crates/peitho/tests/build.rs` — integration test that pins slide count, sections, and notes.json, following the existing lightning-talk pattern
- `Makefile` — `feature-tour`(+`-windowed`) targets, help, DEMO_DECKS, demo-site
- `demo/index.html` — add deck card
- `README.md` — examples table row + one screenshot
- `scripts/take-screenshots.sh` — add to DECKS

## Gates

All CLAUDE.md gates + `peitho build examples/feature-tour/deck.md` succeeds + visual confirmation via screenshot.
