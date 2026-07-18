# Explicit slot assignment syntax — implementation plan (2026-07-05)

Spec: `docs/specs/2026-07-05-explicit-slot-syntax.md`
Issue: [#21](https://github.com/mizzy/peitho/issues/21)

Break the spec-final design down to implementable steps. Everything is done in TDD (write the test first, then the implementation).

## Alignment with the three pillars / invariants

- **Pillar ① (separation of content and design)**: the new notation is a Markdown-side extension (`:::` + attribute). No HTML fragment is imposed on the author. ✅
- **Pillar ② (Version-controllable HTML/CSS layouts)**: no layout-side changes. The slot contract stays single-sourced from the layout HTML. ✅
- **Pillar ③ (type-checked slot contract, no silent drop)**:
  - `ExplicitSlot(SlotName)` newtype separates convention-driven and explicit routing at the type level
  - All 11 validation rules (see spec) become line-numbered build errors with help
  - Unclosed inner region, unknown slot name, nesting, long fence — no silent path
- **typestate**: unchanged. Only a new variant on `FragmentKind`; phase transitions ride the existing path

## Implementation decisions (details not in the spec)

### Decision 1: SlotGroup is "expanded at mapping time"

For the spec's "push `SlotGroup { name, children }` into MappedSlot", we adopt **expand SlotGroup into child fragments at the mapping stage, and push them as individual fragments into the target slot**.

Reasons:
- `check.rs` already runs `check_accepts` / `check_arity` per individual fragment. Passing SlotGroup to check as a wrapper would require rewriting `check_accepts` to descend the fragment tree — the diff would spread
- Expanding SlotGroup leaves check.rs entirely untouched. Accepts violations (validation rule #11) become errors automatically along the existing route
- SlotGroup no longer has to traverse phases; from Mapped onward only the flat "slot → sequence of individual fragments" structure flows, as before

Trade-off: `MappedSlot` fragments no longer carry a record of "entered via explicit assignment". Arity/accepts errors, however, still identify the offending source with a line number (of the fragment inside the SlotGroup), which is sufficient in practice.

### Decision 2: ExplicitSlot is an intermediate type that only appears inside `FragmentKind`

`ExplicitSlot` is an intermediate type stored on the SlotGroup fragment at the Parsed stage and does not appear after Mapped. It is not exposed through lib.rs and stays `pub(crate)`.

This satisfies CLAUDE.md's "long-term view + type safety":
- Only the parser can construct an `ExplicitSlot::new(slot_name)`
- Mapping consumes the `ExplicitSlot` and reduces it to a `SlotName` while expanding SlotGroup
- Even if a future new caller of SlotGroup appears (e.g. a renderer extension), it cannot fabricate an `ExplicitSlot`, so no silent path opens up

### Decision 3: Inside SlotGroup, run pulldown-cmark once and decide over the event stream rather than re-parsing raw Markdown

The spec described "split inner/outer raw Markdown and run pulldown-cmark twice", but implementation-wise it is more straightforward to **run pulldown-cmark in a single pass while detecting `:::` lines against the events and switching the SlotGroup context**.

Reasons:
- pulldown-cmark picks up `:::` as an HTML block (or paragraph), but with the offset-iter we can track line positions, so events on lines containing `:::` are per-line identifiable
- Re-parsing raw Markdown is prone to off-by-one errors when carving out a range that crosses ` ``` ` code fences
- Existing `parse_slide` is an event loop, so it's enough to add a single stack for the SlotGroup context

Concretely, at the top of `parse_slide` scan all source lines to build a map of `:::` lines (`line -> SlotDivMarker`); during the event loop, push onto the stack when the current event is on a SlotGroup opening line, pop on a closing line. When pushing a fragment, look at the stack top and tie the fragment's "belongs-to" to the SlotGroup context.

Exclusion of `:::` inside code fences: while doing the line scan, track ` ``` ` / `~~~` fences and ignore lines inside them.

### Decision 4: Representation of "belongs-to"

Concrete fragment structure:

```rust
// domain.rs
pub(crate) struct ExplicitSlot(SlotName);

impl ExplicitSlot {
    pub(crate) fn into_slot_name(self) -> SlotName { self.0 }
    pub(crate) fn as_slot_name(&self) -> &SlotName { &self.0 }
}

pub enum FragmentKind<S = RawImagePath> {
    Heading { level: u8 },
    Paragraph,
    Text,
    Code,
    Image { alt: String, src: S },
    List,
    // New: contains one or more child fragments. Nesting is forbidden by validation rule #8
    SlotGroup { name: ExplicitSlot, children: Vec<SourceFragment<S>> },
}
```

The SlotGroup fragment itself carries line (`:::` opening line), and children is the sequence of normal fragments that appeared inside the SlotGroup.

## Parser-side changes (`crates/peitho-core/src/parser.rs`)

### Step 1: `:::` line scanner

At the top of `parse_slide`, scan the slide slice (= `source[range.start..range.end]`) line by line to build:

```rust
struct SlotDivMarker {
    line: usize,           // line number in the whole source
    kind: SlotDivKind,
}
enum SlotDivKind {
    Open(ExplicitSlot),
    Close,
}
```

Scan rules:
- Detect lines starting with `:::` from the head (no leading whitespace allowed)
- Ignore lines inside a code fence pair (` ``` ` / `~~~`)
- ` :::` (with leading whitespace), `::::` (4+ colons), `::` (only 2 colons) are not SlotDivMarkers
  - 4+ colons is validation rule #9 (explicit error), so also detect "4+ colon lines" separately and error immediately
- Opening line: `:::` followed by whitespace, `{slot=name}`, allowed whitespace, then end of line
  - No attribute → validation rule #1 error
  - Attribute key is not `slot` → validation rule #2 error
  - Malformed attribute (multiple keys, missing `=`) → validation rule #3 error
  - Invalid slot name → reuse existing `SlotName::new` error (validation rule #4)
- Closing line: `:::` alone (trailing whitespace allowed)
  - `:::` with attribute → validation rule #6 error

### Step 2: Event loop changes

In `parse_slide`'s event loop:
- Current event's line number = look up `line_for_offset(source, global_start)` against the SlotDivMarker map
- On hitting an Open marker line, verify **stack depth is 0** (if not, validation rule #8: nesting forbidden error). Create a fresh `Vec<SourceFragment>` to accumulate SlotGroup contents and push it onto the stack
- On hitting a Close marker line:
  - Stack depth 0 → "`:::` without matching opener" = essentially the inverse of validation rule #5 (or a pure syntax error)
  - Depth 1 → pop, construct a SlotGroup fragment, append to the outer fragment sequence
  - Depth 2 is impossible by ordering (the check happened at Open time)
- On SlotDivMarker lines, absorb pulldown-cmark's events (HTML-block push etc.)

`Event::Rule` keeps the existing "thematic break inside a slide is an error" policy. If `---` appears inside a div, splitting kicks in, which surfaces the "unclosed div" (validation rule #5) at slide end. Actually this happens after `split_slide_ranges` has already run, but `split_slide_ranges` does not know about `:::`, so it lands on the same conclusion (unclosed → error).

### Step 3: SlotGroup fragmentization

At Close detection:
- Empty children → validation rule #7 error (empty SlotGroup forbidden)
- Non-empty → construct `SourceFragment::slot_group(open_line, ExplicitSlot, children)` and append to the outer sequence

### Step 4: Attribute parser

Implement a single-line parser for `{slot=name}` with tests. Protocol:
- Leading `{`, trailing `}` required
- Content is `slot=name`. Surrounding whitespace allowed
- `name` is passed to `SlotName::new` (existing identifier rule)
- Any other form (missing `=`, multiple keys, missing value) is a build error with a specific message + help

### Tests (parser.rs `#[cfg(test)]`)

- `slot_group_open_close_produces_fragment`: a single simple div → 1 SlotGroup fragment
- `slot_group_children_are_parsed`: headings, paragraphs, lists, code inside a div land in children
- `nested_slot_group_is_error`: `::: {slot=b}` inside `::: {slot=a}` → validation rule #8
- `long_fence_four_colons_is_error`: `::::` → validation rule #9
- `unclosed_slot_group_is_error`: only opening marker until slide end → validation rule #5
- `slot_group_missing_attr_is_error`: bare `:::` open (other than a close) → validation rule #1
- `slot_group_unknown_attr_is_error`: `::: {layout=x}` → validation rule #2
- `slot_group_multi_attr_is_error`: `::: {slot=a slot=b}` → validation rule #3
- `slot_group_invalid_slot_name_is_error`: `::: {slot=Foo}` → error via SlotName (validation rule #4)
- `close_marker_with_attr_is_error`: `::: {slot=x}` used as a close → validation rule #6
- `empty_slot_group_is_error`: 0 fragments between open and close → validation rule #7
- `slot_group_in_code_fence_is_ignored`: `::: {slot=x}` inside ` ``` ` is not treated as a marker
- `slot_group_across_thematic_break_is_error`: `---` inside a div → unclosed div error

## Domain-side changes (`crates/peitho-core/src/domain.rs`)

### Add `ExplicitSlot` newtype

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExplicitSlot(SlotName);

impl ExplicitSlot {
    pub(crate) fn new(name: SlotName) -> Self { Self(name) }
    pub(crate) fn into_slot_name(self) -> SlotName { self.0 }
    pub(crate) fn as_slot_name(&self) -> &SlotName { &self.0 }
}
```

Kept `pub(crate)` so external crates cannot construct one.

### Add `SlotGroup` variant to `FragmentKind`

```rust
pub enum FragmentKind<S = RawImagePath> {
    ...,
    SlotGroup { name: ExplicitSlot, children: Vec<SourceFragment<S>> },
}
```

Implement `default_accepts` / `removal_noun` / `Display` (SlotGroup is expanded before Mapped, so these are never used on the phased path; set conservative values like `Accepts::Blocks` / `"slot group"`. Since expansion runs first, none of this is reachable in practice, but avoid `unreachable!`).

`try_map_image_src` recursively maps children for SlotGroup.

### Add `SourceFragment::slot_group` constructor

```rust
impl SourceFragment<RawImagePath> {
    pub(crate) fn slot_group(
        line: usize,
        name: ExplicitSlot,
        children: Vec<SourceFragment<RawImagePath>>,
    ) -> Self { ... }
}
```

### bindings impact

The `Fragment` family is not exposed through TS bindings (bindings/ only holds Manifest / Notes / PresentConfig etc.). `git diff --exit-code bindings/` should show zero.

## Mapping-side changes (`crates/peitho-core/src/mapping.rs`)

### SlotGroup expansion in `map_slide`

Current loop:
```rust
for fragment in slide.fragments.iter().cloned() {
    let target = match fragment.kind() { ... };
    // push fragment onto target slot
}
```

After the change:
```rust
for fragment in slide.fragments.iter().cloned() {
    if let FragmentKind::SlotGroup { name, children } = fragment.kind() {
        let target = name.as_slot_name().clone();
        // If the layout does not have this slot, error early (validation rule #10)
        let Some(contract) = layout.slot_by_name(&target).cloned() else {
            return Err(unknown_explicit_slot_error(target, fragment.line(), layout));
        };
        for child in children.iter().cloned() {
            slots.entry(target.clone())
                .or_insert_with(|| MappedSlot::new(contract.clone()))
                .push(child);
        }
        continue;
    }
    // Existing convention mapping
    let target = match fragment.kind() { ... };
    ...
}
```

`unknown_explicit_slot_error` includes "layout's slot list" in the help (same style as the existing unknown layout error).

### Exclude SlotGroup from `shallowest_heading_line`

Currently only the top level of the fragment tree is walked, so headings inside SlotGroup are automatically excluded (by Decision 1's expansion, the SlotGroup fragment itself is not a Heading, so the existing filter naturally rejects it).

Pin with an explicit test: `title_inferred_from_outside_slot_group` (headings inside SlotGroup are not used for title inference).

### Tests (mapping.rs `#[cfg(test)]`)

- `explicit_slot_routes_fragment_to_named_slot`: contents of `::: {slot=left}` in a two-column layout land in the left slot
- `unknown_explicit_slot_is_error`: slot name not in the layout → validation rule #10
- `explicit_slot_body_is_allowed`: explicit assignment to a name (body) that would also be caught by convention is OK (author decision #3)
- `explicit_and_conventional_share_slot_check_arity`: mixing convention and explicit into the same slot with arity overflow → existing check error
- `title_inferred_from_outside_slot_group`: heading inside SlotGroup is not promoted to title
- `accepts_violation_via_explicit_slot`: explicitly assigning a paragraph to a slot with `accepts=code` → falls through to existing check_accepts error (validation rule #11)

## Dispatch-side changes (`crates/peitho-core/src/mapping.rs::dispatch_slide`)

Implementation of the spec's "interaction with layout dispatch":

- The current structural match is "for each layout, try `map_slide` + `check_slide`, collect matches"
- Because of Decision 1, SlotGroup expansion inside `map_slide` immediately fails with "slot name not in layout", so a layout without the explicit slot name is naturally dropped during probing ✅
- SlotGroup contents are expanded into the assigned slot, so there is no room for them to contaminate satisfaction counts for convention slots (title/body/code) — SlotGroup itself never reaches the existing convention branches ✅

Therefore dispatch_slide logic is unchanged and the spec's interaction holds. Pin with tests:

- `dispatch_prefers_layout_with_explicit_slot_name`: a deck with two-column + title-only layouts; a slide containing `::: {slot=left}` uniquely matches two-column
- `dispatch_rejects_when_no_layout_has_explicit_slot`: zero layouts have the explicit slot name → existing "no layout matches" error (with "unknown explicit slot" in the rejections)

## Example (`examples/two-column/`)

New directory:

```
examples/two-column/
├── deck.md
├── layouts/
│   └── two-column.html
└── css/
    └── base.css
```

- `layouts/two-column.html`: single layout file. `title` + `left` (accepts=blocks) + `right` (accepts=blocks)
- `deck.md`: frontmatter with e.g. `time: 5m`; multiple slides using `::: {slot=left}` / `::: {slot=right}`
- `css/base.css`: minimum style putting `.slot-left` / `.slot-right` side-by-side with display:grid 1fr 1fr

## Gates

- `cargo test --workspace` passing **three times in a row** (past flaky incidents, mandated by CLAUDE.md)
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all --check`
- `git diff --exit-code bindings/` (expected to be zero this time)
- `cd packages/peitho-present && npm run build && npm test && npm run typecheck`
- `git diff --exit-code packages/peitho-present/dist/shell.js` (TS side should have no changes)
- Run `cargo run --bin peitho -- build examples/two-column` on `examples/two-column/` and visually verify the generated `dist/index.html` (two columns side-by-side via CSS)

## CLAUDE.md update

Remove the `Explicit fenced div slot notation ::: {slot=...} (§18)` line from the "Undecided — awaiting author's judgment" section and add an invariant to the confirmed items, roughly: "Explicit slot assignment uses fenced div `::: {slot=name}` notation. The parser produces an `ExplicitSlot` newtype; mapping expands it and routes directly to the named slot (adopted 2026-07-05, spec: `docs/specs/2026-07-05-explicit-slot-syntax.md`)".

## Order of operations (TDD)

1. domain.rs: `ExplicitSlot` newtype + `FragmentKind::SlotGroup` + tests (types only)
2. parser.rs: `:::` scanner + attribute parser + SlotGroup fragmentization + tests (validation rules #1–#9)
3. mapping.rs: SlotGroup expansion + unknown slot name error (#10) + tests
4. check.rs: pin test that accepts violation (#11) is captured on the existing route (no implementation change)
5. dispatch: pin test for the interaction (no implementation change)
6. Add `examples/two-column/`
7. Update CLAUDE.md
8. Run all gates
