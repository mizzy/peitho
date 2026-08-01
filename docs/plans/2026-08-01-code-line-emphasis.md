# Code line emphasis

<!-- constrained-by ../specs/2026-08-01-code-line-emphasis-design.md -->

Issue: #372
Date: 2026-08-01
Branch: `feat/code-line-emphasis`

Invariants from the approved design:

- The `|` separator alone discriminates static emphasis (0 steps, present in distributed artifacts) from stepped emphasis (one step per group, present-shell only).
- Emphasis steps share the reveal step space and the single parse-time counting authority (`reveal_span_len`); `stepnav.ts`, `sync.ts`, the `{index, step}` wire format, and `ManifestSlide.revealSteps` are unchanged.
- `FragmentKind::Code` stays a unit variant; emphasis rides `SourceFragment` like `reveal_span`.
- Line numbers are per code fragment, 1-based, and validated against the block's line count — out of range is a build error.
- Emphasis on a `code_images` block is rejected at parse time, enforced by ordering at the `renderer_for` seam.
- Blocks without an emphasis spec are not line-wrapped, so existing decks build byte-identical.
- Emphasis is a distinct attribute from reveal: `data-emphasis-step` / `data-emphasis-active` (equality, not accumulation), never `data-reveal-hidden`.

Task order is deliberate: the untagged rendering path (Task 4) lands before the syntect path (Task 5) so line-wrapping semantics are pinned by tests before the hard scope-stack work begins.

## Task 1: emphasis spec grammar

**Goal**: Parse `{2-4}`, `{2,5-7|9}` and reject every malformed shape with a line-numbered `BuildError`.

**Files**: `crates/peitho-core/src/emphasis.rs` (new), `crates/peitho-core/src/lib.rs`

A standalone module with no pipeline dependencies, so the grammar is testable in isolation.

```rust
/// Line emphasis groups for one code fragment.
///
/// `groups` is never empty. `stepped` records whether the author wrote `|`:
/// stepped emphasis consumes one reveal step per group, static emphasis
/// consumes none and is always visible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LineEmphasis {
    groups: Vec<LineGroup>,
    stepped: bool,
}

/// One emphasis group: the set of lines emphasized together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LineGroup {
    ranges: Vec<RangeInclusive<usize>>,
}
```

`parse_emphasis_spec(spec: &str, line: usize) -> Result<LineEmphasis>` takes the text between the braces.

**Test**:

```rust
#[test]
fn parses_static_and_stepped_specs() {
    let e = parse_emphasis_spec("2-4", 3).unwrap();
    assert!(!e.stepped());
    assert_eq!(e.groups().len(), 1);

    let e = parse_emphasis_spec("2,5-7|9", 3).unwrap();
    assert!(e.stepped());
    assert_eq!(e.groups().len(), 2);
    assert_eq!(e.groups()[0].lines().collect::<Vec<_>>(), vec![2, 5, 6, 7]);
    assert_eq!(e.groups()[1].lines().collect::<Vec<_>>(), vec![9]);

    // A single line is a valid range.
    assert!(parse_emphasis_spec("3", 3).is_ok());
}

#[test]
fn emphasis_spec_errors_are_line_numbered() {
    for (spec, message) in [
        ("0", "code line numbers start at 1"),
        ("4-2", "emphasis range end is before its start"),
        ("2-", "incomplete emphasis range"),
        ("a", "emphasis spec expects line numbers"),
        ("", "empty emphasis spec"),
        ("2||4", "empty emphasis group"),
        ("2,,4", "empty emphasis group"),
    ] {
        let err = parse_emphasis_spec(spec, 7).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Parse, "spec {spec:?}");
        assert_eq!(err.line, Some(7), "spec {spec:?}");
        assert_eq!(err.message, message, "spec {spec:?}");
        assert!(!err.help.is_empty(), "spec {spec:?}");
    }
}
```

**Implementation**: split on `|` (stepped = separator present), split each group on `,`, parse each item as `N` or `N-M`. Reject empty groups and empty items. Overflow-safe parse (`usize::from_str`) — an absurd line number becomes an out-of-range error in Task 3, not a panic.

## Task 2: info string splitting and Pandoc-aware errors

**Goal**: Split ` ```rust {2-4} ` into language and emphasis spec; keep the unknown-language error intact; give Pandoc attribute blocks a self-explaining error.

**Files**: `crates/peitho-core/src/emphasis.rs`, `crates/peitho-core/src/parser.rs` (around the `Tag::CodeBlock` arm)

**Test**:

```rust
#[test]
fn splits_info_string_into_language_and_spec() {
    assert_eq!(split_info("rust"), (Some("rust"), None));
    assert_eq!(split_info("rust {2-4}"), (Some("rust"), Some("2-4")));
    assert_eq!(split_info("{2-4}"), (None, Some("2-4")));
    assert_eq!(split_info(""), (None, None));
    // Extra whitespace is tolerated; the language is the first token.
    assert_eq!(split_info("rust  {2-4}"), (Some("rust"), Some("2-4")));
}

#[test]
fn pandoc_attribute_block_names_itself_in_the_error() {
    let err = parse_markdown(
        "# T\n\n```{.rust}\nx\n```\n",
        &crate::highlight::Highlighter::defaults(),
    )
    .unwrap_err();
    assert_eq!(err.line, Some(3));
    assert!(err.message.contains("Pandoc"), "message: {}", err.message);
    assert!(err.help.contains("```rust"), "help: {}", err.help);
}

#[test]
fn unknown_language_error_survives_info_splitting() {
    // Regression: the existing error must still fire on the language token.
    let err = parse_markdown(
        "# T\n\n```notalang\nx\n```\n",
        &crate::highlight::Highlighter::defaults(),
    )
    .unwrap_err();
    assert_eq!(err.message, "unknown code language 'notalang'");

    // And still fire when an emphasis spec follows a bad language.
    let err = parse_markdown(
        "# T\n\n```notalang {1}\nx\n```\n",
        &crate::highlight::Highlighter::defaults(),
    )
    .unwrap_err();
    assert_eq!(err.message, "unknown code language 'notalang'");
}
```

**Implementation**: the first whitespace-delimited token is the language unless it starts with `{`. The remainder, if any, must be a single `{…}` group — anything else (trailing junk, unclosed brace) is an error. A spec whose first character is `.` or `=` after `{` produces the Pandoc-specific message instead of the generic parse failure. Language validation stays exactly where it is (before emphasis validation) so its error keeps priority.

## Task 3: fragment annotation and line-count validation

**Goal**: `SourceFragment` carries emphasis; out-of-range lines are a build error; `code_images` blocks reject emphasis at the `renderer_for` seam.

**Files**: `crates/peitho-core/src/domain.rs`, `crates/peitho-core/src/parser.rs`

**Test**:

```rust
#[test]
fn emphasis_beyond_the_block_is_an_error() {
    let err = parse_markdown(
        "# T\n\n```rust {2-4}\nfn main() {}\n```\n",
        &crate::highlight::Highlighter::defaults(),
    )
    .unwrap_err();
    assert_eq!(err.line, Some(3));
    assert_eq!(err.message, "emphasis line 2 is past the end of a 1-line code block");
    assert!(err.help.contains("1"));
}

#[test]
fn emphasis_on_a_code_images_block_is_rejected_before_transform() {
    // mermaid resolves to a builtin renderer; emphasis has no meaning there.
    let err = parse_markdown_with_code_images(
        "# T\n\n```mermaid {1}\ngraph TD;\n```\n",
        &CodeImagesConfig::builtin_only(),
    )
    .unwrap_err();
    assert_eq!(err.line, Some(3));
    assert!(err.message.contains("emphasis"));
    assert!(err.help.contains("mermaid"));
}

#[test]
fn emphasis_rides_the_fragment_through_parse() {
    let deck = parse_markdown(
        "# T\n\n```rust {1}\nfn main() {}\n```\n",
        &crate::highlight::Highlighter::defaults(),
    )
    .unwrap();
    let fragment = /* the Code fragment */;
    assert!(fragment.emphasis().is_some());
}
```

**Implementation**: add `emphasis: Option<LineEmphasis>` to `SourceFragment` plus a getter and a `pub(crate)` `with_emphasis`; extend `try_map_image_src_inner`'s destructure/rebuild by one field. In the `TagEnd::CodeBlock` arm, order is: language validation → `renderer_for` check (reject emphasis on diagram blocks here, so an annotated fragment can never reach `transform_fragment`) → emphasis parse → line-count validation against `text.lines().count()`.

Add a `debug_assert!(fragment.emphasis().is_none())` at the top of `code_images::transform_fragment` as insurance against the ordering being broken later — the design calls this out explicitly because that function rebuilds the fragment and would silently drop the annotation.

## Task 4: rendering — untagged blocks

**Goal**: Line-wrap untagged code blocks that carry emphasis; static emphasis emits classes, stepped emphasis emits step attributes; no spec means byte-identical output.

**Files**: `crates/peitho-core/src/render.rs`

Deliberately first: this path does not touch syntect, so it pins the wrapping contract before Task 5.

**Test**:

```rust
#[test]
fn untagged_block_without_emphasis_is_unchanged() {
    // Byte-identical to today: no line spans at all.
    let html = render_one("# T\n\n```\nplain\n```\n");
    assert!(html.contains("<pre class=\"slot-code\"><code>plain</code></pre>"));
    assert!(!html.contains("code-line"));
}

#[test]
fn static_emphasis_emits_classes_and_no_steps() {
    let html = render_one("# T\n\n```\na\nb\nc\n```\n".replace("```\n", "```{2}\n"));
    assert!(html.contains(r#"<span class="code-line">a</span>"#));
    assert!(html.contains(r#"<span class="code-line code-line-emphasis">b</span>"#));
    assert!(!html.contains("data-emphasis-step"));
}

#[test]
fn stepped_emphasis_emits_step_attributes_and_no_classes() {
    let html = render_one_stepped(); // ```{1|3}
    assert!(html.contains(r#"<span class="code-line" data-emphasis-step="1">"#));
    assert!(html.contains(r#"<span class="code-line" data-emphasis-step="2">"#));
    assert!(!html.contains("code-line-emphasis"));
}

#[test]
fn emphasis_escapes_html_in_the_line_text() {
    let html = render_one_with("<script>", "{1}");
    assert!(html.contains("&lt;script&gt;"));
}
```

**Implementation**: in `render_code_fragment`'s `None` branch, when emphasis is present, escape and wrap each line. A line in group *k*: stepped → `data-emphasis-step="{span.start + k}"`, static → the `code-line-emphasis` class. Lines in no group get the bare `code-line` class. Absent emphasis keeps today's single escaped string.

## Task 5: rendering — syntect line splitting

**Goal**: The same wrapping for highlighted blocks, with `hl-*` spans correctly closed and reopened at line boundaries.

**Files**: `crates/peitho-core/src/highlight.rs`, `crates/peitho-core/src/render.rs`

The one substantial piece of new code. `ClassedHTMLGenerator` appends into a single buffer and never closes the open scope stack at end of line, so per-line spans require driving `ParseState`/`ScopeStack` directly via `line_tokens_to_classed_spans`, closing the stack at each line end and reopening it at the next line start.

**Test**:

```rust
#[test]
fn highlighted_lines_are_individually_wrapped() {
    let html = highlight_with_emphasis("fn main() {}\nlet x = 1;\n", "rust", "{2}");
    assert!(html.contains(r#"<span class="code-line">"#));
    assert!(html.contains(r#"<span class="code-line code-line-emphasis">"#));
    assert!(html.contains("hl-keyword"), "syntax classes survive wrapping");
}

#[test]
fn scopes_spanning_lines_stay_balanced() {
    // Adversarial: a multi-line string and a block comment both leave scopes
    // open at end of line. Every line span must be independently balanced.
    let html = highlight_with_emphasis(
        "let s = \"aaa\nbbb\";\n/* c1\n   c2 */\nlet y = 2;\n",
        "rust",
        "{1}",
    );
    for line_html in extract_line_spans(&html) {
        assert_eq!(
            count_open_spans(&line_html),
            count_close_spans(&line_html),
            "unbalanced line: {line_html}"
        );
    }
}

#[test]
fn highlighting_without_emphasis_is_byte_identical() {
    // The whole no-emphasis path must not change.
    let before = highlight_plain("fn main() {}\n", "rust");
    assert!(!before.contains("code-line"));
}
```

**Implementation**: keep `highlight_html` as-is for the no-emphasis path (guaranteeing byte-identical output), and add a sibling that produces `Vec<String>` of per-line inner HTML. The caller wraps each line. Balancing is the crux: close the open scope stack in reverse at end of line, reopen it at the start of the next.

## Task 6: step counting

**Goal**: Stepped emphasis consumes `groups.len()` steps from the shared counter; static consumes none; the invariant between counting and stamping is asserted.

**Files**: `crates/peitho-core/src/parser.rs`, `crates/peitho-core/src/render.rs`

**Test**:

```rust
#[test]
fn stepped_emphasis_contributes_one_step_per_group() {
    let deck = parse_markdown("# T\n\n```rust {1|2|3}\na\nb\nc\n```\n", &hl()).unwrap();
    assert_eq!(deck.slides()[0].step_count(), 3);
}

#[test]
fn static_emphasis_contributes_no_steps() {
    let deck = parse_markdown("# T\n\n```rust {1-2}\na\nb\n```\n", &hl()).unwrap();
    assert_eq!(deck.slides()[0].step_count(), 0);
}

#[test]
fn static_emphasis_inside_reveal_still_counts_as_one_appearing_block() {
    let deck = parse_markdown(
        "# T\n\n::: {reveal}\n\n```rust {1}\na\n```\n\n:::\n",
        &hl(),
    ).unwrap();
    assert_eq!(deck.slides()[0].step_count(), 1);
}

#[test]
fn stepped_emphasis_inside_reveal_is_rejected() {
    let err = parse_markdown(
        "# T\n\n::: {reveal}\n\n```rust {1|2}\na\nb\n```\n\n:::\n",
        &hl(),
    ).unwrap_err();
    assert!(err.message.contains("stepped"));
    // Help must name both supported alternatives.
    assert!(err.help.contains("static") || err.help.contains("outside"));
}

#[test]
fn emphasis_steps_reach_the_manifest() {
    // revealSteps is the shared field; emphasis steps are reveal steps.
    let manifest = build_manifest("# T\n\n```rust {1|2}\na\nb\n```\n");
    assert_eq!(manifest.slides()[0].reveal_steps(), 2);
}
```

**Implementation**: extend `reveal_span_len`'s `FragmentKind::Code` handling — stepped emphasis returns `groups.len()`, and a code fragment with stepped emphasis is not additionally counted as one appearing block. Stepped emphasis inside a `::: {reveal}` group is rejected at the point the group dissolves (where both the span and the emphasis are visible). The renderer stamps exactly `groups.len()` distinct step values and asserts agreement in the same shape as `render_revealed_list_fragment`'s existing `unreachable!`.

Note that stepped emphasis outside any reveal group still needs step numbers: the fragment gets a span assigned from the same slide counter even though it was never in a group.

## Task 7: present shell

**Goal**: `data-emphasis-active` tracks the current step exactly; nothing else in the shell changes.

**Files**: `packages/peitho-present/src/shell.ts`, `packages/peitho-present/test/shell.test.ts`

**Test** (vitest):

```ts
it("activates only the group matching the current step", () => {
  // <span data-emphasis-step="1">, <span data-emphasis-step="2">
  shell.show(0, 1);
  expect(active(host)).toEqual(["1"]);
  shell.show(0, 2);
  expect(active(host)).toEqual(["2"]); // moves, does not accumulate
  shell.show(0, 0);
  expect(active(host)).toEqual([]);
});

it("leaves reveal behavior untouched", () => {
  // A slide with both reveal and emphasis: hidden and active are independent.
  shell.show(0, 1);
  expect(hidden(host)).toEqual([/* reveal steps > 1 */]);
});
```

Per CLAUDE.md, destroy the shell and its listeners in `afterEach`.

**Implementation**: a sibling of `applyRevealState`, toggling `data-emphasis-active` where `Number(dataset.emphasisStep) === step`. Add the emphasis CSS next to `REVEAL_HIDDEN_CSS`. `stepnav.ts` and `sync.ts` stay untouched.

## Task 8: theme CSS

**Goal**: Emphasized lines get a background tint; the rest dim only while some line is active.

**Files**: `themes/base.css`

```css
.slot-code .code-line-emphasis,
.slot-code [data-emphasis-active] { background: …; }

.slot-code:has(.code-line-emphasis) .code-line:not(.code-line-emphasis),
.slot-code:has([data-emphasis-active]) .code-line:not([data-emphasis-active]) { opacity: …; }
```

The `:has()` scoping is what keeps a stepped block undimmed before its first step and after its last.

## Task 9: example deck and E2E

**Goal**: The feature is exercised by a real deck and confirmed in a real browser.

**Files**: `examples/…`, `Makefile` (`DEMO_DECKS`), `site/content/examples/<name>.md`

Per CLAUDE.md, adding an example requires exactly two wiring sites (`DEMO_DECKS` and the examples page); the landing gallery derives automatically.

**E2E checklist** (jsdom cannot confirm any of these):

1. `peitho present` on a stepped deck: emphasis is visible and moves with arrow keys; the un-emphasized lines dim without disappearing.
2. A block with both reveal and static emphasis: the block appears at its reveal step with emphasis already applied.
3. `peitho preview`: static emphasis shown, stepped emphasis absent.
4. PDF export: a static deck shows emphasis, a stepped deck shows none.
5. Untagged block emphasis renders identically to a tagged one.

## Task 10: docs

**Files**: `site/content/guide/…`, `CLAUDE.md`

Guide section covering the grammar, the static/stepped split, and the distribution policy. Add a `CLAUDE.md` invariant bullet in the established form, pointing at both the spec and this plan.

## Gates

Per CLAUDE.md, before committing:

```
cargo test --workspace          # 3 times in a row
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
git diff --exit-code bindings/
cd packages/peitho-present && npm run build && npm test && npm run typecheck
git diff --exit-code packages/peitho-present/dist/shell.js
git diff --exit-code packages/peitho-present/dist/preview.js
git diff --exit-code packages/peitho-present/dist/remote.js
```

`bindings/` should show **no** drift: emphasis introduces no new contract type, and `ManifestSlide.revealSteps` already exists. Drift there means emphasis leaked into the manifest contract, which the design says it must not.
