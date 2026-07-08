# Explicit slot assignment syntax design (2026-07-05)

## Goal

Introduce a syntax for authors to explicitly designate slots in the slide body for layouts where convention mapping (the four kinds: title/code/image/body) is ambiguous — two-column, left/right, multiple code slots, etc. Do not create silent drop / silent fallback, and do not break pillar ① "Separation of content (Markdown) and design (HTML+CSS)".

Target: Issue [#21](https://github.com/mizzy/peitho/issues/21). One of the §18 "Undecided Items". The prerequisite multi-layout support (hybrid dispatch) is already implemented.

## Adopted approach and rationale

**Approach A: Pandoc fenced div `::: {slot=name}`** (approved by author 2026-07-05)

- Syntax designed as an extension of Markdown notation (Pandoc/MyST/Quarto family). It can be handled as a structured node in an extended Markdown parser, so information rides types all the way from parser to mapping
- Does not drag HTML fragments (like `<template>`) into Markdown — does not break pillar ① "Separation of content and design"
- `:::` is a line-head marker so open/close/attribute line numbers can be pinned. Matches the existing error policy (line number + help)
- Attribute parsing has the same 1-line structure as frontmatter's `key: value` parsing. Aligns with existing parser conventions

Rejected alternatives:

- **`<template #name>` (Slidev-style)**: relies on HTML passthrough and is an opaque block from the Markdown parser's viewpoint. Attribute extraction has to be written by hand, and the inner Markdown gets passed through untouched. Above all, exposing the `<template>` element itself in content clashes with pillar ①
- **Attribute-less shorthand `::: name`**: in Pandoc, `.name` is syntactic sugar for a class attribute, and could collide with a slot name. Only the explicit form `{slot=name}` is allowed

## Notation (author decision 2026-07-05)

````markdown
# Title

::: {slot=left}
Left column body.
- Lists are also OK
:::

::: {slot=right}

```rust
fn main() {}
```

:::
````

- The opening marker is `:::` at line head (exactly three colons) + a required `{slot=name}` attribute. The closing marker is a bare `:::` line with no attribute
- Four or more colons (Pandoc's long fence for nesting) is a v1 error. Reserved for future nesting; never silently treated as three-colon
- The inner Markdown is parsed as-is by pulldown-cmark (fenced code, lists, paragraphs, images all allowed)
- Only `slot=name` is allowed as an attribute. Any other key, multiple keys, or missing value is a build error
- The `name` in the attribute value reuses the existing `SlotName` validation (identifier rules)
- HTML comments inside a div follow the same rule as outside (JSON → page settings comment, plaintext → speaker note). Wrapping in a div does not change per-slide comment semantics

## Parse strategy

### Processing order (consistency with existing implementation)

Slide splitting happens first; fenced div scanning second. Splitting is done on the whole source via pulldown-cmark's `Event::Rule` scan (`split_slide_ranges`), and pulldown-cmark does not know `:::`, so **if `---` appears inside an open div, that is where the slide splits**. That div ends up "reaching slide end without hitting the closing marker" and errors under validation rule #5 (never silently span slides). A div is a slide-internal structure; div spanning across slides is not expressible in v1 — this is spec, not a limitation.

### Two-pass parse inside a slide

Adopt the frontmatter/slide-split "two-pass" pattern.

1. **Pre-tokenize**: line-scan each slide for `:::` lines and extract open/close pairs with line numbers. **The scanner tracks fenced code block (` ``` ` / `~~~`) interiors and does not treat `:::` inside a code fence as a marker** (essential for slides explaining Pandoc syntax, etc.)
2. **Inner parse**: pass the extracted inner Markdown to pulldown-cmark to get a regular `Fragment` sequence
3. **Outer parse**: pass the rest with div blocks removed to pulldown-cmark to get a regular `Fragment` sequence
4. **Node composition**: insert `Fragment::SlotGroup { name, children, line }` into the outer fragment sequence in line-number order to form a single fragment sequence

There is no matching `ENABLE_*` flag in pulldown-cmark, so hand-tokenizing is the only choice. Same policy as frontmatter extraction — not a stretch.

## Domain type changes

```rust
pub enum FragmentKind {
    Heading { .. },
    Paragraph,
    List,
    Text,
    Code,
    Image { .. },
    SlotGroup { name: ExplicitSlot, children: Vec<SourceFragment> },
}
```

`ExplicitSlot` is a newtype wrapping `SlotName` (no public constructor, produced only inside the parser). It stays type-distinguishable from convention mapping — per the CLAUDE.md "long-term view + type safety" rule, explicit and convention-based designations must not share the same type or a silent override could sneak back in.

No impact on `bindings/` expected (only Manifest/Notes/PresentConfig-family types are exposed on the TS side; `Fragment` is a Rust-internal type). Confirm no change with the drift check (`git diff --exit-code bindings/`).

## Interaction with layout dispatch

Fold `SlotGroup` into the "structural match" of hybrid dispatch (explicit `{"layout":…}` > sole layout unconditionally > unique structural match):

- If a slide contains `slot=left`, a layout without a slot named `left` **drops out of the structural match candidates** (an explicit slot name is a strong structural signal)
- SlotGroup contents are **not counted** in convention mapping's structural checks (title/code/image/body satisfaction) — the contents go directly to the specified slot, so mixing them into convention-slot satisfaction double-counts them
- This yields the natural behavior "in a deck where a two-column layout coexists with a one-column layout, a slide that writes `slot=left`/`slot=right` uniquely matches the two-column layout automatically". Ambiguous and zero-match remain errors as before

## Interaction with title inference

Convention title inference (`shallowest_heading_line`) targets **only fragments outside SlotGroups**. Headings inside a SlotGroup go directly to the specified slot and do not become title candidates. Writing `::: {slot=title}` explicitly also allows explicit title injection (accepts/arity follow existing checks).

## Mapping changes

Add a branch to `mapping.rs::map_slide`:

- `FragmentKind::SlotGroup { name, children }` → inject directly into the slot with the given `name`. `children` are pushed into `MappedSlot` as-is **without going through convention mapping**
- If the layout has no such slot → build error (line number at the opening marker line, help lists the layout's slots). Unknown slots in convention mapping error at check time via `unassigned`, but explicit designation makes the author's intent (e.g. slot-name typo) concrete at mapping time, so we surface a more specific error sooner
- A `SlotGroup` nested inside a `SlotGroup` → v1 build error (with help saying "nesting is not supported")
- Everything else applies the existing convention mapping unchanged

**Collisions between the slot picked up by convention and the slot indicated explicitly** are allowed — if both convention and explicit populate the same slot, arity checking naturally catches it (no silent drop).

## Validation rules (all line-numbered build errors with help. No silent path)

1. `:::` opening marker without `{slot=…}` attribute → error
2. Attribute key other than `slot` → error ("only `slot=name` allowed")
3. Multiple attributes (e.g. `{slot=a slot=b}`) → error
4. Slot name violates `SlotName` identifier rules → reuse the existing `SlotName` error
5. No closing `:::` before slide end for an opening marker → error (a `---`-based slide split inside a div falls into this path too. help includes "close divs within a slide")
6. Closing `:::` has an attribute → error
7. Empty `SlotGroup` (zero fragments between open and close) → error (explicitly writing empty is an author mistake; even a slot with arity `0..*` can express "empty" by simply not writing it)
8. `SlotGroup` nested inside `SlotGroup` → error (v1 unsupported)
9. Four-colon or longer fence → error (reserved for future nesting)
10. Specified slot name does not exist on the layout → mapping-time error (help: list of the layout's slot names)
11. accepts violations like injecting an inline paragraph explicitly into an `accepts=code` slot → error through the existing check.rs path (do not duplicate check implementation. Pin with a test that the existing path catches it)

## Sample (examples/ addition)

Plan to add `examples/two-column/`:

- `layouts/two-column.html`: 3 slots — `title` + `left` (accepts=blocks) + `right` (accepts=blocks)
- `deck.md`: uses `::: {slot=left}` / `::: {slot=right}` to show that convention cannot tell left/right apart
- `css/`: left/right two-column CSS

That way "what becomes possible with the new syntax" also serves as documentation.

## Implementation phases

1. Parser: `:::` pre-tokenize (with code-fence tracking) + generate `FragmentKind::SlotGroup` + syntax error family (#1–#9)
2. Mapping/Dispatch: `SlotGroup` branch + integration into structural match + unknown-slot-name error (#10)
3. Check: existing path unchanged (pin with a test that accepts violations still error on the existing route)
4. Examples: add `examples/two-column/`
5. Docs: keep this spec; move relevant items from CLAUDE.md's "Undecided" section into decided

## Intentionally deferred in v1

- **Nesting**: there is groundwork for expressing grid / tabs / steps by nesting fenced divs in the future, but v1 is flat, single-level only. Errors on nesting (#8) and long fences (#9) seal seams so a silent path cannot be created when the feature is eventually unlocked
- **`::: name` shorthand**: no attribute-less form. Pandoc's `.name` is treated as a class and collides, so only the explicit form `{slot=name}`
- **Divs spanning slides**: not expressible (see processing order). Spec explicitly says so

## Author decisions (2026-07-05)

1. **Syntax is finalized as approach A (fenced div `::: {slot=name}`)**. Slidev-style `<template #name>` is not adopted
2. **Add `examples/two-column/`** as a new example (rather than adding to an existing example)
3. **Explicit designation of names that convention could also pick up (title/body/code) is allowed**. Making intent explicit is harmless; arity overflow is caught by existing checks. Enables the "hedge: this paragraph goes to body for sure" use case
4. Treatment of attribute separators (commas etc.) is **not applicable in v1**, so it is deferred — v1 attributes are a single `slot=name`; multiple keys are an error (validation rule #3). Decide when a future extension introduces multiple attributes

## Related

- pillar ①: Content/Design separation — main rationale for adopting approach A (no HTML fragment in content)
- pillar ③: typed slot contract — `ExplicitSlot` newtype separates convention and explicit at the type level
- CLAUDE.md "long-term view + type safety": enforcing at the type level prevents a future new caller (parser extension) from creating a silent path
- Prerequisite: multi-layout support (hybrid dispatch, adopted 2026-07-03) is done
