# Incremental Reveal (Fragments) — Design

**Issue:** #290
**Date:** 2026-07-31
**Status:** Approved by author (notation scope, step semantics, PDF policy, preview policy, and overall architecture decided 2026-07-31)

## Goal

Progressive disclosure of slide content: advancing "next" reveals the slide's content one step at a time before moving to the next slide. Opt-in per slide, with zero impact on decks that don't use it.

The hard constraints (from the issue and the three pillars):

- **Pillar ①**: no presentation vocabulary embedded per-item in content. The notation must declare *structure* (what forms a step group), not *staging* (when/how things appear).
- **§16 event contract**: the slide body stays passive; only the shell executes transitions.
- **No silent behavior**: invalid notation is a line-numbered build error with help.
- **PDF/preview output must be decided, not defaulted** (author decisions below).

## Author decisions (2026-07-31)

| Question | Decision |
| --- | --- |
| Notation scope for v1 | `::: {reveal}` fenced-div block grouping (not slide-level page setting, not per-item inline markers) |
| Group semantics | **Per-child stepping**: each direct child block of a group is one step; a list child contributes one step per top-level list item |
| PDF export | Final state only, one page per slide (consistent with skip slides appearing in PDF: distributed artifacts are the complete version) |
| Preview (`peitho preview`) | Always final state, both grid and single mode; step walkthrough is `peitho present`'s job |

## Notation

```markdown
# Agenda

::: {reveal}
- Background          ← step 1
- Current problems    ← step 2
- Proposal            ← step 3
:::

::: {reveal}
First, the premise.   ← step 4 (paragraph)

​```rust
fn main() {}          ← step 5 (code block)
​```
:::
```

- `::: {reveal}` reuses the explicit-slot fenced-div tokenizer (line-first, fenced-code interiors excluded).
- `reveal` is a bare attribute. `{reveal=<anything>}` is a line-numbered error — attribute values are reserved for future extensions (e.g. group-as-one-step).
- Each **direct child block** of a group is one step. A `List` child is the exception: each **top-level list item** is one step (nested sub-items appear with their parent item).
- Multiple groups per slide are allowed; steps number sequentially across groups in source order.
- Content outside any group is the always-visible baseline (shown at step 0).
- Why `:::` and not a page setting or inline markers: `:::` is already the established seam for author-declared structure (explicit slots). A group declares "these blocks form a sequence" — structure, not staging. How and when steps appear remains entirely the shell's business, so pillar ① and §16 both hold. Per-item inline markers (Marp-style) were rejected because they embed staging timing in content; a slide-level page setting alone was rejected because it cannot express which blocks participate.

## Architecture

Two load-bearing decisions:

### 1. Groups dissolve at parse time into per-fragment annotations

The parser does **not** introduce a new `FragmentKind` that rides the pipeline (unlike `SlotGroup`, which exists only until Mapped). Instead, at parse time each fragment that was inside a `::: {reveal}` group gets a **reveal span annotation** — `Option<RevealSpan { start, len }>` on `SourceFragment` — and the group itself vanishes.

Consequences:

- Convention mapping, explicit-slot mapping, slot-contract checking, and `Accepts` validation are **unchanged**: fragments route individually exactly as if the group were not there.
- A group whose children map to different slots (e.g. a paragraph → body, a code block → code) is legal: steps are stamped per element and the shell hides by step number regardless of slot. Cross-slot stepping is a feature (reveal an image after text in a two-column layout), not an error.
- Step counting has a **single source of truth**: one parse-time helper computes each fragment's span (`len = 1` for non-list blocks; for a `List`, `len` = the number of top-level items, counted by running pulldown-cmark over the fragment's markdown with the same options the renderer uses). The renderer only consumes spans; it never recounts.
- `ParsedSlide` gains `step_count: usize` (the sum of spans), riding Parsed → Mapped → Checked exactly like `skip`, surfacing in `manifest.json` as `ManifestSlide.revealSteps` (serde default `0`). ts-rs bindings regenerate.

### 2. Default is fully visible; only the present shell hides

The renderer stamps deterministic attributes into the emitted HTML (visible in diffs, part of the rendered contract):

- `data-reveal-step="N"` on each step's top-level element(s) — for a list, on each top-level `<li>`; for other blocks, on the block's root element. Stamping happens where the fragment's markdown is rendered (event-stream interception around pulldown-cmark's output for lists; the existing lol_html rewriter idiom is the fallback seam).
- `data-reveal-steps="{total}"` on the slide `<section>`, via the same `HtmlRewriter` pass that already stamps `data-slide-key` — only when the slide has steps, so decks without reveal groups build byte-identical to today.

No CSS in the rendered output hides anything. The present shell owns a small style (`[data-reveal-hidden] { visibility: hidden }` — `visibility`, not `display`, so layout stays stable) and toggles `data-reveal-hidden` on `[data-reveal-step]` elements whose step exceeds the current shown count.

Consequences: PDF export, `peitho preview`, lint, and published `dist/` all show the final state **by construction** — the author decisions for PDF and preview require zero code in those paths, and no shell knowledge leaks into distributed artifacts.

## Runtime

### Step state and navigation semantics

The shell tracks `currentStep` (`0..=stepCount`, the number of steps currently shown) alongside `currentIndex`. Step counts come from the manifest (`revealSteps`), the same route the shell already reads `skip`.

- **next**: `currentStep < stepCount` → show step + 1. Otherwise → next non-skipped slide at step 0.
- **prev**: `currentStep > 0` → show step − 1. Otherwise → previous non-skipped slide **fully revealed** (Marp/reveal.js convention: backing up shows where you left off).
- **Direct jumps** (`{index}`, `{key}`, `first`, `last`, preview-grid click): land fully revealed — orientation beats re-staging when jumping around.
- Initial slide on present open: step 0 (the presentation starts un-revealed).
- `show()`'s identity guard becomes the `(index, step)` pair, so step transitions aren't swallowed by the existing same-index early return.

### Events (§16)

- `peitho:navigate` targets gain an optional step: `{index, step?}` (used by sync replay). `"next"`/`"prev"` resolution becomes step-aware inside the shell; emitters (keyboard, controls, swipe, remote, presenter buttons) are unchanged — they keep emitting request events only.
- `peitho:slidechange` keeps its semantics (fires only when the slide index changes) and its detail gains `step`/`stepCount` for the entry state. Existing consumers (section actuals, rehearsal reporter, presenter notes) are keyed on index and see no behavioral change.
- New `peitho:stepchange` event with `{index, step, stepCount}` fires on step-only transitions. Only the sync bridge and step-display UI listen to it.
- No new keyboard shortcuts in v1: Space/ArrowRight/PageDown are "next" and become step-aware for free.

### Sync

Following the absolute-state-not-toggle rule (the channel coalesces):

- `POST /sync` index message extends to `{"index": N, "step": M}` — one message carries the full navigation state atomically (a paired-but-separate step message could coalesce apart from its index). The shell posts both on every slide/step change.
- Server folds `step` into `SyncState` next to `index`; **every** GET response (handshake and poll) carries `step`, and `deliverReplayState` replays `{index, step}` as one navigate after each poll — same per-poll replay that makes swap converge.
- Swap navigation reloads the window; the handshake replay restores `(index, step)`, so step state survives a display swap.
- The remote computes button enablement and targets locally from the manifest (`revealSteps` + `skip`) and posts absolute `{index, step}`.
- The presenter's current-slide stage mirrors the live step via the sync-replayed state; the next-slide preview pane stays at default (= final state), which is correct for "what's coming".

## Validation (all line-numbered build errors with help)

| Case | Handling |
| --- | --- |
| Unclosed / nested `:::` groups | Existing explicit-slot rules apply unchanged (nesting `reveal` inside `slot=` divs is therefore an error — v1 limitation, revisit via attribute combination if needed) |
| `{reveal=value}` | Error: attribute values reserved for future syntax |
| `{slot=x reveal}` (multiple attributes) | Existing multi-attribute error applies |
| Empty `::: {reveal}` group | Error: reveal group has no content |
| `reveal` + `{"draft":true}` slide | Allowed — the slide (and its groups) drop at parse end; unlike `section`, reveal is slide-local so no global structure is affected |
| `reveal` + `{"skip":true}` slide | Allowed — a skipped slide is never entered sequentially; direct navigation lands fully revealed like any direct jump |
| Sections / planned time | No interaction: steps don't change slide count, indices, or section ranges |

## Non-goals (v1)

- Group-as-one-step (`{reveal=group}`-style) and reveal inside explicit slot divs — reserved, both currently hard errors.
- Step transition animations/styling hooks for themes.
- Step-aware preview or PDF step expansion (explicitly decided against, see author decisions).
- New keyboard shortcuts (e.g. "skip all steps of this slide").

## Touched surfaces (map for the implementation plan)

- `crates/peitho-core/src/parser.rs` — `:::` attribute grammar, group dissolution, span helper, `step_count`
- `crates/peitho-core/src/phase.rs`, `mapping.rs`, `check.rs` — thread `reveal` span / `step_count` (the `skip` precedent)
- `crates/peitho-core/src/render.rs` — `data-reveal-step` stamping, `data-reveal-steps` on `<section>`
- `crates/peitho-core/src/manifest.rs` + `bindings/ManifestSlide.ts` — `revealSteps`
- `crates/peitho/src/server.rs` — `SyncIndexMessage.step`, state fold, GET response field
- `packages/peitho-present/src/shell.ts` — step state, navigation semantics, hide/show toggling, `stepchange`
- `packages/peitho-present/src/sync.ts` — POST `{index, step}`, replay
- `packages/peitho-present/src/remote.ts`, `presenter.ts` — manifest-driven step awareness
- No changes: `preview.ts` behavior, `render_pdf_document`, lint, publish contamination check
