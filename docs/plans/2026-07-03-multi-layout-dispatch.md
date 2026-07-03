# Multiple Layouts and Hybrid Dispatch

## Purpose

Currently one build = one layout, and layouts can't be switched within a deck (this was an open item in §18). The author's decision has been made:

- The approach is **hybrid**: by default, auto-select via type-driven matching (structural matching between the shape of the slide's content and the slot contract), and when ambiguous, raise a build error and resolve it via explicit specification
- Take reference from [k1LoW/deck](https://github.com/k1LoW/deck) (page settings are JSON in an HTML comment, explicit specification via the `layout` key. The part corresponding to deck's CEL expression defaults is replaced in peitho by structural matching against the slot contract)
- Change the terminology from `--template` to `--layout`

## Phase A: Rename (preceding PR)

Align with deck and unify the user-facing terminology to "layout".

- CLI: `--template` → `--layout`
- Directories: `templates/` → `layouts/`, sample's `template.html` → `layout.html`
- Core: `Template` type → `Layout`, `parse_template` → `parse_layout`, `template.rs` → `layout.rs` (error messages already say "layout", so this brings them into alignment)
- Follow through in README / CLAUDE.md / Makefile. Past docs/plans/ are history, so they are not rewritten.

## Phase B: Multiple layouts + dispatch

### Syntax (deck-compatible page settings comment)

```markdown
<!-- {"key":"cover","layout":"cover"} -->
```

`layout` is optional. If specified, that layout is used (an unknown name is a build error listing the known layouts).

### CLI

- `--layout <path>` can be specified multiple times (`Vec<PathBuf>`). The layout name is the file stem. Duplicate names are an error
- If unspecified, the built-in `title-body-code` single layout is used as before

### Dispatch rule (deterministic)

1. If a slide has an explicit `layout`, use it. Contract violations produce the same line-numbered error as before
2. If there is only one layout, use it unconditionally (full preservation of current behavior. Error messages also stay as they are)
3. With multiple layouts and no explicit specification, attempt conventional mapping + contract checking (accepts/arity/unassigned) against each layout:
   - Exactly one passes → adopt it
   - Multiple pass → ambiguity error (list of candidates + help saying "specify explicitly with `{"layout":"…"}`")
   - Zero pass → total-failure error (enumerate the mismatch reason for each layout)
4. Order is stable, following CLI specification order

### How the type flows through

`MappedSlide` holds its own `Layout` (given a resolved clone at dispatch time). check/render do not re-reference the registry — this avoids creating a lookup-failure path in later stages. The typestate `Parsed→Mapped→Checked→Rendered` remains unchanged.

### Theme validation

`overrides.css`'s `[data-slide-key="k"] .slot-x` is validated against "the slots owned by slide k's layout". Keyless selector slot classes are validated against the union of all layouts.

### Sample

Turn keynote into a 2-layout composition: `cover.html` (title only) and `statement.html` (title+body required). The cover slide has only a title → falls to cover via type-driven matching; body slides fall uniquely to statement (a demonstration of structural matching). The explicit-specification syntax is documented in the README.

## Verification

- Unit: explicit/unique/ambiguous/total-failure/full compatibility with a single layout, the parser's layout field, per-layout theme validation
- E2E: build the keynote 2-layout composition and confirm in a real browser. Confirm existing samples and existing tests pass unchanged (guarantee of compatibility with a single layout)
