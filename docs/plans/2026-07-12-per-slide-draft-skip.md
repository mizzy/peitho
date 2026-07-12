# Per-slide `draft` / `skip` flags (Issue #242)

Date: 2026-07-12
Issue: https://github.com/mizzy/peitho/issues/242

## Summary

Add two per-slide page-settings keys to the existing JSON page comment:

- `{"draft": true}` — the slide is excluded from the build entirely: not emitted
  to `dist/`, not counted in totals, not indexed in `manifest.json`, absent from
  `notes.json`.
- `{"skip": true}` — the slide is rendered and counted, but keyboard navigation
  (`next`/`prev`) in both the present shell and the preview shell steps over it.
  Direct navigation (`{index}` / `{key}` targets, Home/End first/last targets,
  overview grid selection) still lands on it. The present and preview shells
  open on the first non-skipped slide, with slide 0 as the all-skipped fallback.
  The presenter "next slide" preview is skip-aware.

## Design decisions (with rationale)

### D1. `draft` drops at **parse end, before index assignment and section resolution**

The issue text says "Mapped-phase drop", but that names the observable outcome
(excluded from generation), not the correct seam. `ParsedSlide.index` is
assigned once during parsing and copied verbatim through every phase; it drives
both the manifest `index` and the `slides/NNN-key.html` filename. Section
ranges (`DeckSection { start, end }`) are derived at parse end from positional
indices over the slide list. Dropping at Mapped would therefore require
renumbering survivors *and* recomputing section ranges stored in
`DeckSettings` — two seams, one of them mutating deck settings mid-pipeline.

Dropping draft slides at parse end, before final indices are assigned and
before `resolve_deck_sections` runs, restores the invariant at the point where
indices are born:

- **Root cause**: "index = position in the deck" holds by construction; no
  renumber pass, no downstream consumer ever sees a draft slide.
- **Type safety**: `Deck<Parsed>` only ever contains real slides. No phase, and
  no future consumer of any phase, can observe a draft slide or a hole in the
  index sequence. `draft` never becomes a field on `ParsedSlide` — it exists
  only inside the parser, so it is unrepresentable downstream.
- **Long-term**: section validation ("first slide must carry a marker", "totals
  must equal deck time") automatically operates on the surviving slides with no
  special-casing; a future consumer of Parsed/Mapped/Checked needs no filter.

All three lenses select the same option, so this deviation from the issue's
phrasing is noted in the PR body per the pick-issue protocol.

Derived keys are assigned from the original parse position and are **not**
re-derived after draft slides are dropped. The key is a stable slide identity,
not a display ordinal; reassigning `slide-2` to a later survivor would silently
retarget keyed CSS overrides. The built `index` is still reassigned over
survivors, and `source_index` rides `ParsedSlide` → `MappedSlide` →
`CheckedSlide` so post-parse build errors keep reporting the source slide
number. Key uniqueness is validated over all parsed slides before the drop,
because keys are source-document identity.

### D2. Validation rules (all line-numbered build errors, per pillar 3)

1. `{"draft": true, "skip": true}` on one slide → build error (redundant; the
   issue prescribes this).
2. `{"draft": true}` on a slide that also declares `{"section": …}` → build
   error, help text telling the author to move the marker to the next
   non-draft slide (the issue's "simpler rule").
3. If dropping drafts leaves **zero** slides → build error at the line of the
   first draft flag ("every slide in the deck is marked draft"), so an empty
   deck can never silently reach later phases.
4. After the drop, the existing rule "if any section marker exists, the first
   slide must carry one" is evaluated against the **surviving** first slide —
   this falls out of D1 with no extra code, but gets an explicit test.
5. `{"draft": false}` / `{"skip": false}` are accepted no-ops (the value is the
   semantics; erroring on explicit `false` would be surprising). A comment
   containing only `{"draft": false}` still counts as "has settings" for the
   empty-object guard.
6. Unknown keys remain rejected by `#[serde(deny_unknown_fields)]`; the
   allowed-keys help string in `parse_page_comment` gains `draft` and `skip`.

### D3. `skip` rides the pipeline like `notes` and surfaces in the manifest

`skip: bool` is stored on `ParsedSlide`, copied to `MappedSlide` and
`CheckedSlide` (the manifest is built from `Checked`; `RenderedSlide` does not
need it), and surfaces as `ManifestSlide.skip: bool` (serde `rename = "skip"`
not needed — single word — but follows the `hasNotes` pattern for placement).
Missing `skip` in an existing manifest deserializes to `false`
(`#[serde(default)]`), matching the additive-compat pattern already tested for
other fields. `bindings/ManifestSlide.ts` is regenerated and committed.

- Skipped slides keep their notes in `notes.json` (they can be presented via
  direct navigation).
- Skipped slides are included in PDF export and `publish` (they are real
  rendered slides; appendix/backup material belongs in the PDF).
- `skip` does not affect section ranges or section time (the issue prescribes
  this): sections and the agenda treat the slide as present.

### D4. Navigation semantics (TS shell — §16 preserved)

Keyboard components keep emitting only request events; all skip logic lives in
the shells' target resolution:

- **Present shell** (`shell.ts` `resolveTarget`): `next` walks forward from
  `currentIndex+1` to the first slide with `meta.skip !== true`; if none, the
  navigation is a no-op (stay put — mirrors the existing clamp-at-last no-op,
  including not emitting `slidechange`). `prev` walks backward symmetrically.
  Startup also resolves to the first non-skipped slide, falling back to slide 0
  when every slide is skipped. `{index}`, `{key}`, `first`, and `last` targets
  are unchanged (direct navigation lands on skipped slides).
- **Preview shell** (`preview.ts` `resolveTarget`): same walk for `next`/`prev`
  in single-slide mode. With no restored session state, startup also resolves to
  the first non-skipped slide; restored indexes stay exact because the author may
  be editing a skipped slide. Grid-mode selection movement
  (`resolveGridVerticalTarget`, left/right within the grid) is direct navigation
  and does **not** skip — every slide is visible and selectable in the overview,
  per the issue.
- **Presenter next-slide preview** (`presenter.ts` `updateFromSlide`): the
  naive `index + 1` becomes "first non-skipped index after `index`", so the
  preview shows where Space will actually go; if none remains, the existing
  "End" branch renders. The `NN / NN` badge and on-slide counter keep counting
  all slides including skipped ones (the issue: skipped slides contribute to
  the total).
- Edge: if the **current** slide is itself skipped (reached via direct
  navigation), `next`/`prev` walk from it normally — the walk already handles
  this with no special case.

## Implementation steps (TDD — each step is red → green)

### Rust (crates/peitho-core)

1. **Parser wire + validation** (`parser.rs`)
   - Add `draft: Option<bool>`, `skip: Option<bool>` to `PageComment`;
     add both to the validated `PageSettings`; extend the all-`None`
     empty-settings guard and the allowed-keys help string.
   - `draft && skip` → error (D2-1). `draft && section` → error (D2-2).
   - Thread the flags through `process_html_chunk` accumulators (respecting
     the existing one-comment-per-slide guard).
   - Tests beside the existing page-comment cluster (`parser.rs` `#[cfg(test)]`):
     parse both flags, reject `draft+skip`, reject `draft+section`, reject
     unknown keys still, `false` values accepted, empty-object guard still
     fires for `{}` but not `{"draft":false}`.
2. **Draft drop at parse end** (`parser.rs`)
   - Drop draft slides from the draft list before final index assignment and
     before `resolve_deck_sections` / `finalize_section_settings`; indices are
     assigned over survivors only.
   - All-slides-draft → error (D2-3).
   - Tests: draft slide absent from `Deck<Parsed>`; indices contiguous;
     first-surviving-slide section rule (D2-4); section ranges/time computed
     over survivors; draft slide's notes do not surface.
3. **`skip` through phases** (`phase.rs`, `mapping.rs`, `check.rs`)
   - `ParsedSlide.skip` → `MappedSlide.skip` → `CheckedSlide.skip` (+ carried
     through `resolve_image_paths`), accessor on `CheckedSlide`.
4. **Manifest** (`manifest.rs`)
   - `ManifestSlide.skip: bool` with `#[serde(default)]`;
     `ManifestSlide::new` removed in favor of named-field struct construction
     with `pub(crate)` fields; all construction sites updated; `build_manifest`
     passes `slide.skip()`.
   - Tests: schema serialization includes `skip`; additive-deser test
     (missing `skip` → `false`); bindings-export test updated.
5. **Bindings**: regenerate `bindings/ManifestSlide.ts`, commit.

### CLI integration (crates/peitho)

6. **End-to-end build tests** (`crates/peitho/tests/build.rs`): a deck with a
   draft slide → absent from `dist/slides/`, `manifest.json`, `notes.json`,
   `slideCount`; a deck with a skip slide → present, counted,
   `"skip": true` in the manifest.

### TS shell (packages/peitho-present)

7. **Present shell**: skip-aware `next`/`prev` in `resolveTarget`; no-op when
   nothing non-skipped remains in that direction. Tests in the existing nav
   suite: steps over one and multiple consecutive skips, no-op at
   all-skipped-tail (no `slidechange`), direct `{index}`/`{key}` lands on skip.
8. **Preview shell**: same for single-mode `next`/`prev`; grid movement
   unchanged (test asserts grid selection can land on a skipped slide).
9. **Presenter**: next-preview picks first non-skipped index; "End" when only
   skipped slides remain. Test in `presenter.test.ts`.
10. **generated.test.ts**: expect `skip` on the binding shape.
11. Rebuild embedded bundles (`npm run build`) so `dist/shell.js` /
    `dist/preview.js` drift gates pass.

### Docs

12. Update `CLAUDE.md` supported page-settings keys mention if needed and add
    this plan's decisions; user-facing docs (`site/`) only if the guide
    documents page settings (check `site/content/guide/` — update the page
    that lists page-comment keys).

## Gates

```
cargo test --workspace   # ×3
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
git diff --exit-code bindings/
cd packages/peitho-present && npm run build && npm test && npm run typecheck
git diff --exit-code packages/peitho-present/dist/shell.js packages/peitho-present/dist/preview.js
```

Plus an end-to-end check in a real browser: build an example deck with one
skip slide, `peitho preview`, verify Space steps over it and the overview
lands on it.

## Non-goals

- No `freeze` equivalent (per the issue).
- No draft/skip in frontmatter (per-slide concerns stay in page comments).
- Presenter agenda changes: none needed — skipped slides stay in sections.
