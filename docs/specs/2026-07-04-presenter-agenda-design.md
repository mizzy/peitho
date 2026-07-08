# Presenter agenda design (2026-07-04)

## Goal

Implement the Agenda section from the Claude Design mock (`Presenter.dc.html`). Split the deck into named sections and, in the presenter view's timer card (between the tracker and controls), show each section's "name, slide range, actual/planned time, delta". Decks without declared sections show nothing (current display unchanged).

Design origin: the `.agenda` family in `Presenter.dc.html` at https://claude.ai/design/p/73ba6a5b-288b-46f7-a2c3-cb06863e3c5b. After adoption, the CSS in `render_presenter_index()` is authoritative.

## Adopted approach and rationale

**Approach A: manifest-driven with client-side measurement** (approved by author 2026-07-04)

- Sections are declared in a page comment; core (Rust) resolves and validates them at parse time and puts them in `manifest.json`. manifest is the single source of the contract, and sections are part of deck structure, so they belong here
- Actual time is measured client-side by the presenter window. In-memory only (lost on reload) — consistent with the existing timer that is lost on reload

Rejected alternatives:
- sections.json sidecar approach — no reason to separate deck structure from the manifest
- shell/server sync with persisted actuals — over-engineering that would be asymmetric with the timer itself not being persisted

## Notation (author decision 2026-07-04)

Declared in the page comment of the section's first slide. Frontmatter and heading-derivation approaches are not adopted (author decision). The flat-key constraint on frontmatter (2026-07-03 author decision) is preserved.

```markdown
---
time: 15m
---

<!-- {"section": "Setup", "time": "1m"} -->
# Title

---

<!-- {"section": "Why HTML decks", "time": "3m", "layout": "cover"} -->
# Why HTML
```

- `section` = section name (display label). The slide range runs to just before the next `section` marker (or to the end of the deck for the last one). The range is derived automatically at build time — no structural accident where inserting or reordering slides misaligns a range declaration
- `time` = planned time for that section. Reuses the same grammar as the deck frontmatter's `time` (`15m`/`90s`/`1h30m`/bare integer = minutes), the same `PlannedTime` type, and the same validation (non-zero, upper bound)
- May co-live with existing `key`/`layout` in the same comment. `PageComment` remains `deny_unknown_fields`

## Validation rules (all line-numbered build errors with help. No silent path)

1. `time` is required with `section` (allowing omission would break the meaning of derived totals)
2. A page comment with `time` but no `section` is an error (help: put deck-wide `time` in frontmatter)
3. `section` as an empty string is an error
4. If any marker exists, **a marker on the first slide is required**. Otherwise an error (do not synthesize an implicit unnamed section — the prohibition on implicit resolution of ambiguity)
5. **Total match check** (author decision 2026-07-04): if both frontmatter `time` and the sum of section `time`s are present, they must match. Mismatches error with both values in the message. If frontmatter `time` is omitted, the section sum becomes the total. Sums use `checked_add`; overflow and exceeding the `PlannedTime` upper bound are errors
6. Duplicate section names are allowed (they are display labels, not keys. Uniqueness is not required because state is determined positionally)
7. **A second page-settings comment on the same slide is an error** (uniform rule across `key`/`layout`/`section`). The previous field-wise last-wins merge would silently overwrite a section marker with a second comment (= silent drop), so the fix is to forbid duplicate comments at the root rather than guard field-by-field (finalized during implementation review 2026-07-04)

Validation happens once at the end of parsing (inside `parse_markdown`, when all slide markers are known); the resolved `Vec<DeckSection>` is finalized at `Deck<Parsed>` construction. Subsequent phases only carry the validated value (the same "validate once at construction" policy as `PlannedTime`). **After derivation, `DeckSettings`'s `planned_time` always holds the total** (filled in here from the sum when omitted), so downstream (tracker and manifest `plannedDurationMs`) works unchanged.

## Types and data flow

```
PageComment { key, layout, section, time }        parser.rs (deny_unknown_fields preserved)
  ↓ Resolved and validated at parse end
DeckSection { name: String, planned: PlannedTime, start: usize, end: usize }  phase.rs
  ↓ Stored in DeckSettings { planned_time, sections: Vec<DeckSection> }
     (Copy is removed — mechanical change. Rides Parsed→Mapped→Checked→Rendered on the same path as existing planned_time)
  ↓ build_manifest
Manifest { ..., sections: Vec<ManifestSection> }   manifest.rs
ManifestSection { name, startIndex, endIndex, plannedDurationMs }
  ↓ ts-rs → bindings/Manifest.ts, bindings/ManifestSection.ts (committed + CI drift check)
presenter.ts / agenda.ts (new) consume shell.manifest.sections
```

- `start`/`end` are 0-based indices, same as `ManifestSlide.index` (1-based-ified at display time). Milliseconds appear only at the manifest boundary (existing policy)
- `sections` is an empty array for decks without sections (the field itself always exists — no TS-side optional branching)
- manifest `version` stays 1 (additive change. Update the golden test `serializes_manifest_schema_exactly`)
- The dist (publish) manifest also carries `sections`, but the present (audience) shell does not read it. Unrelated to the "no presenter shell / notes in dist" policy (the manifest is part of the dist contract to begin with)

## Actual-time measurement (author decision 2026-07-04: cumulative)

- On each 250ms tick, add the delta from the previous `elapsedMs()` to the section that **the currently displayed slide belongs to**. Time spent going back to re-explain also lands in that section's actuals. Pausing halts `elapsedMs` so is naturally excluded
- `done`/`current`/`upcoming` are decided by the current slide's position: current = the section the current slide belongs to, done = anything before it, upcoming = anything after. Going back to a previous section makes it current again
- `under`/`over` on done is decided live. The sole judgment source is the **second-rounded difference** `Math.round((actual − planned) / 1000)` — so the sign and color of the delta display are structurally consistent (using raw milliseconds would produce contradictions like "+0:00 shown as over" and "just-on-target shown as −0:00 under"; finalized during implementation review 2026-07-04). Rounded delta > 0 → `over`, otherwise `under`
- When the timer is not started (stopped), all section actuals are 0. Following the mock, done/current show `actual / planned`, upcoming shows `— / planned`, and delta is shown only for done as `±M:SS` (current/upcoming show `·`)
- Time display uses the same `m:ss` format as the tracker ticks (shared with the existing formatter in `timeTracker.ts`)

## Implementation placement

**Rust (crates/peitho-core)**
- `parser.rs`: extend `PageComment`, add `section`/`time` validation, resolve and validate sections at parse end (errors via existing `ErrorKind::Parse` + line number + help)
- `phase.rs`: introduce `DeckSection`; add `sections` to `DeckSettings` (remove `Copy`); accessors
- `manifest.rs`: introduce `ManifestSection`; add `Manifest.sections`; update golden tests; add ts-rs tests
- `render.rs` `render_presenter_index()`: port the `.agenda` family CSS from the mock (rewrite selectors on a `data-peitho-*` basis). Update render tests

**TS (packages/peitho-present)**
- New `agenda.ts` (same shape as `timeTracker.ts`): `installAgenda({root, shell, sections})`. Subscribes to `peitho:slidechange` + 250ms interval. Teardown required (vitest listener contamination mitigation). **The empty check lives in one place inside `installAgenda`**: if `sections` is empty, return a no-op that mounts nothing; `presenter.ts` calls it unconditionally (do not duplicate the guard)
- On slide transitions, use `slidechange`'s `previousIndex` to flush the delta since the previous tick to the **section of the pre-transition slide** before updating the display (prevents misattribution at 250ms polling granularity). Also subscribe to `peitho:timercontrol` `reset` requests to zero actuals immediately (if reset → immediate start slips inside the 250ms polling gap, old actuals would linger. This is a conscious tradeoff against §16 "only the shell executes transitions" — we substitute observation of the request event; a dedicated event to observe the shell's reset execution was judged over-engineering for now. Finalized during implementation review 2026-07-04)
- `presenter.ts`: inside the `.clock` card, add an agenda-slot between tracker-slot and controls (the slot itself always exists). **Keep `.clock` as flex column + `.controls { margin-top: auto }`** (switching to grid regresses buttons growing vertically — measured)
- Recommit `bindings/`, rebuild and commit `dist/shell.js`

**examples**
- Add section markers to `examples/lightning-talk/deck.md` (serves as both E2E confirmation and documentation)

## DOM/CSS constraints

- DOM hooks are `data-peitho-*` attributes only (no class-dependent selectors — established design decision). State is expressed via `data-peitho-agenda-state="done|current|upcoming"` + `data-peitho-agenda-outcome="under|over"` (attached only to done rows), and CSS applies via compound selectors on state + outcome. The initial plan's `data-peitho-agenda-delta` collided with the hook name on the delta cell span — a bare `[data-peitho-agenda-delta]` selector would match the entire done row — so it was renamed to `outcome` (finalized during implementation review 2026-07-04)
- The mock's `.agenda` is `overflow: hidden` (overflow is clipped). Scroll handling for many sections is out of scope this time (separate issue when needed)
- The present (audience) `timeTracker` present-variant DOM is **byte-invariant** (fixed by snapshot test). Do not touch

## Edge cases

| Case | Behavior |
|---|---|
| Deck without section markers | Agenda hidden. Existing behavior fully unchanged |
| `section` without `time` | Build error (help: write time) |
| `time` without `section` | Build error (help: deck-wide `time` goes in frontmatter) |
| Empty-string `section` | Build error |
| No marker on first slide (with other markers) | Build error (points at the first marker line) |
| Frontmatter `time` vs section sum mismatch | Build error (both values in message) |
| No frontmatter `time` + sections present | Total = sum. Tracker still shows |
| Section sum overflow / exceeds upper bound | Build error |
| Duplicate section name | Allowed (display label) |
| Section of one slide / single section | Allowed |
| Go back to a prior section | It becomes current again; actuals resume accumulating |
| Presenter reload | Actuals lost (same as timer) |
| Timer pause | Actual accumulation stops (per `elapsedMs`) |

## Out of scope

- Clicking an agenda row to jump to that section (separate issue if requested)
- Persisting actuals / restoring on reload
- Scrollable UI for many sections
- Section display on the present (audience) side
