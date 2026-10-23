# Code Line Emphasis — Design

**Issues:** #372, #737, #740
**Date:** 2026-08-01
**Status:** Approved by author (original notation decided 2026-08-01; explicit absolute steps and partial-line matching extended 2026-10-23)

## Goal

Draw the audience's eye to specific lines of a code block: "the line I'm talking about right now". Optionally move that emphasis as the talk progresses, so a long code block can be walked through in place instead of being split across several slides.

This is **not** syntax highlighting. Syntax highlighting is a property of the content (in Rust, `fn` is a keyword); line emphasis is a property of the *talk* (right now I am discussing line 3). The two live on separate layers and must not be conflated — emphasis renders on top of, and independently of, the `hl-*` spans syntect produces.

The hard constraints (from the three pillars):

- **Pillar ③**: out-of-range lines, malformed ranges, and unparseable notation are all line-numbered build errors with help. Silent dropping is forbidden — an emphasis the author believes is active but that silently does nothing would send them on stage misinformed.
- **§16 event contract**: the slide body stays passive; only the shell executes transitions.
- Decks that do not use the notation must build byte-identical to today.

Issue #364 (moving presentation vocabulary out of Markdown into layouts) is explicitly **not** a constraint here (author decision 2026-08-01: "#364 is a best-effort direction, don't factor it in for now"). Note also that line emphasis is structurally unable to move layout-side: a layout is a container shared across slides, so it cannot name "line 3 of this particular code block". If #364 is ever pursued, this notation belongs to the residue it already contemplates ("accepting that some residue stays in Markdown").

## Author decisions (2026-08-01; extended 2026-10-23 by Issues #737 and #740)

| Question | Decision |
| --- | --- |
| Where the notation lives | Code fence info string, after the language token |
| Static vs stepped | The `\|` separator is the sole discriminator: no `\|` → static (0 steps), `\|` present → one step per group |
| Step number space | Shared with incremental reveal (#290); implicit groups take the next source-order step, while `@N` names an absolute slide step |
| Explicit-step consistency | All stepped groups on a slide use `@N`, or none do; explicit steps cover `1..max` without gaps and cannot share a slide with `::: {reveal}` |
| Line numbering basis | Per fenced block, always starting at 1; each block renders as its own `<pre>` |
| Output behavior | Static emphasis is always present; stepped emphasis is active in present and preview, and in distributions/PDF only with `public_reveal: steps` |
| Untagged code blocks | Supported — emphasis works without a language tag |
| Non-contiguous lines | Supported from v1 via `,` |
| Partial lines | `N:"text"` emphasizes only the unique matching text on line `N`; whole and partial entries may share a group |
| Partial match cardinality | Exactly one occurrence, counting overlapping starts; zero and multiple occurrences are distinct errors |
| Quoted text | No escapes; the next `"` ends the text, the first `}` still ends brace extraction, and neither character can be matched |
| Partial styling | Tight rounded box using `--peitho-emphasis-background`; no inset marker bar; an active partial keeps its containing line undimmed |
| Out-of-range / malformed | Line-numbered build error |

## Notation

The emphasis spec is a brace group in the fence info string, after the language token:

````markdown
```rust {2-4}
fn main() {
    let config = load();      ← emphasized, always
    let server = Server::new(config);
    server.run();
}
```
````

Grammar:

```
spec           := "{" (static-group | stepped-groups) "}"
static-group   := items
stepped-groups := "|" stepped-group ("|" stepped-group)*
                | stepped-group "|" stepped-group ("|" stepped-group)*
stepped-group  := items ("@" step)?
items          := item ("," item)*
item           := N | N "-" M | N ":" '"' text '"'
step           := N                      (1-based)
```

`N` and `step` contain ASCII decimal digits only, with no sign. Both are
positive: `0`, `+2`, and `@+1` are errors.

- **`|` absent** — static emphasis. The listed lines are emphasized whenever the slide is shown. Consumes **no** reveal steps.
- **`|` present, no `@N`** — implicit stepped emphasis. Groups take consecutive steps from the slide's source-order counter, preserving the original behavior.
- **`|` present, every group has `@N`** — explicit stepped emphasis. Each group uses the named absolute slide step, so groups in different blocks can interleave.
- A leading `|` makes a single group stepped: `{|3}` (Issue #693).
- A single explicit group therefore uses the same marker: `{|3@2}`. `@N` on a static spec is an error.
- Groups may overlap: a line listed in several groups is emphasized at each of their steps, so `{1-5|3}` emphasizes lines 1–5, then only line 3 (Issue #696).
- Whole-line and partial entries are independent within a group, so `{1,2:"string"}` is valid. A substring cannot attach to a range: `2-4:"string"` is an error.
- Within one block, groups may be authored in any step order, but an explicit step may occur only once. Across blocks, the same step is allowed and activates all matching lines together.
- If any stepped group on a slide uses `@N`, every stepped group on that slide must use it. The union of explicit steps must cover `1..max` without a gap, and that slide cannot contain a `::: {reveal}` group.
- The language token is optional: ` ```{2-4} ` (no language) emphasizes lines of an unhighlighted block. Detection is positional — an info string whose first token starts with `{` has no language.

### Partial-line entries

`N:"text"` resolves `text` against line `N` of the same
`emphasis_code(text).lines()` view used for whole-line bounds checks. The
match must occur exactly once. Occurrences are counted at every character
start, including overlaps, so `"aa"` has three occurrences in `aaaa`. A missing
match and a non-unique match are separate line-numbered errors; the latter
tells the author to lengthen the string until it is unique. No occurrence
index exists.

The parser preserves every byte inside the quotes, including leading and
trailing spaces. Its `|`, `,`, and `@` splitting is quote-aware; those
characters, plus `-`, `:`, and spaces, are literal inside the text. There are
no escapes. Text ends at the next `"`, so a quote cannot be matched. Fence-info
brace extraction deliberately still stops at the first `}`; if that brace
lands inside an open partial string, the extractor reports that the text was
truncated instead of allowing a later, misleading fence-option error. Empty
text and an unclosed quote have their own focused errors.

After validation, the authored text is discarded. The typed group entry keeps
only the 1-based line and resolved byte range, making a missing or ambiguous
match unrepresentable downstream.

### Collision with Pandoc attribute syntax

Positional detection means peitho claims a leading `{…}` for itself, which is where Pandoc and some CommonMark extensions put *attributes*: ` ```{.rust} ` and ` ```{=html} ` specify a language or output format, not line numbers. Under this notation those parse as a malformed emphasis spec.

No existing deck regresses — peitho currently treats the whole info string as a language name, so ` ```{.rust} ` is already an `unknown code language` error today. But a block pasted in from Pandoc-flavored Markdown will now fail with a *different* error, and the message must not send the author hunting for a line-number bug. The emphasis parse error therefore names the case explicitly: Pandoc-style attribute blocks are not supported; write the language bare (` ```rust `).

````markdown
```rust {2,5-7|9}
```
````

reads: step 1 emphasizes lines 2 and 5–7; step 2 emphasizes line 9.

Explicit steps allow interleaving across blocks. `{1-3@1|8@3}` on block A
and `{|1-8@2}` on block B produces A lines 1–3, then B lines 1–8, then A
line 8.

### Why `|` is the discriminator

The alternative — treating static emphasis as "stepped emphasis with one group" — was rejected. If `{2-4}` consumed a step, the slide would first appear with no emphasis and require one keypress to reach the state the author wrote, which contradicts "these lines are always the important ones". Making the rule implicit instead ("one group means zero steps") creates a discontinuity: deleting one group from `{2-4|6-8}` would silently change the remaining group from stepped to static.

With `|` as an explicit marker the rule is uniform and local: a spec is stepped iff it contains a `|`. An author who removes a group and wants the rest to stay stepped writes `{|2-4}` (Issue #693).

### Why line numbers restart per code block

Each fenced code block remains a distinct fragment and renders as its own `<pre>`, even when multiple fragments are routed to the same slot. Authors write `{2}` in the second block meaning *its* line 2. This keeps emphasis in a later block from shifting whenever a line is added to an earlier one, and per-fragment numbering matches the rendered block boundary.

## Architecture

### 1. The spec is parsed at parse time and rides as a fragment annotation

Issue #696 gives parse validation and rendering one shared definition of an
emphasizable code block: trim every trailing `\n`, then take the lines of the
remaining text. `LineEmphasis` construction validates its highest referenced
line against that count, and the renderer splits the same trimmed slice. This
keeps trailing blank lines from passing validation only to disappear during
rendering.

`SourceFragment` carries `emphasis: Option<LineEmphasis>` independently of
`reveal_span`. Parsing first produces an unresolved spec; the parser then
assigns implicit groups consecutive steps or accepts each explicit `@N` and
constructs the resolved annotation through an emphasis-module-private
constructor:

```rust
pub struct LineEmphasis {
    kind: LineEmphasisKind,
}

enum LineEmphasisKind {
    Static(Vec<LineGroup>),
    Stepped(Vec<SteppedGroup>),
}

struct SteppedGroup {
    group: LineGroup,
    step: NonZeroUsize, // absolute slide step
}

enum GroupEntry {
    WholeLines(RangeInclusive<usize>),
    Partial { line: usize, range: Range<usize> },
}

enum PartialEmphasis {
    Static,
    Stepped(Vec<NonZeroUsize>),
}
```

Thus, “stepped without a step” is unrepresentable after parsing. The single
line-membership accessor returns a `LineMark` containing the whole-line
`EmphasisMark` plus non-overlapping `PartialMark` segments whose mark is a
`PartialEmphasis`, so an unmarked partial segment is unrepresentable. Each
partial segment carries `Static` or every covering group's `NonZeroUsize`
absolute step, less any membership already supplied by the whole line. Range
boundaries from overlapping groups split the line only where the effective
mark changes; adjacent segments with equal marks coalesce. For example, ranges
`0..4` at step 1 and `2..6` at step 2 produce `0..2` with `1`, `2..4` with
`1 2`, and `4..6` with `2`. This preserves overlapping groups without duplicate
emphasis, a first-match path, or a second membership accessor. The unresolved
form exposes one matchable static/implicit/explicit view; its lazy resolver
reserves a counter range only for implicit groups.

No new `FragmentKind` variant, and **`FragmentKind::Code` stays a unit variant**. Making it `Code { emphasis }` would break exhaustive matches in 15+ sites across `check.rs`, `render.rs`, `code_images.rs`, `mapping.rs`, `plain.rs`, `domain.rs`, and `parser.rs` for no benefit — the language and source text already live on `SourceFragment` rather than in the variant, and emphasis belongs with them.

Consequences:

- Mapping, slot-contract checking, and `Accepts` validation are unchanged.
- `code_images.rs` passes ordinary code fragments through with their emphasis intact, but never rebuilds an emphasized fragment: emphasis on a block with a resolved `code_images` renderer is rejected at parse time (see "Rejected combinations"). This must be enforced by ordering, not assumed — validation runs at the same point the parser resolves `renderer_for(language)`, and a debug assertion protects the transform seam from silently dropping the annotation.
- `try_map_image_src_inner` destructures and rebuilds `SourceFragment`; it gains one field.

### 2. Step counting remains a parse-time decision

`reveal_span_len` continues to count only content that actually appears from a
`::: {reveal}` group. Stepped emphasis never receives a `RevealSpan`.

For implicit emphasis, the parser resolves each group from
`next_reveal_step` and advances that counter exactly as before. At slide end,
explicit-mode validation rejects mixing, reveal coexistence, and gaps; a valid
explicit slide's step count is its maximum `@N`. The renderer consumes the
absolute steps stored on `LineEmphasis` and never derives them from fragment
order or a span base. `ManifestSlide.revealSteps` still comes from this one
parse-time `step_count`.

### 3. Rendering: line-wrapping spans

Every line of every emphasis-carrying code block is wrapped:

```html
<pre class="slot-code"><code
><span class="code-line">fn main() {</span>
<span class="code-line" data-emphasis-step="1">    let config = load();</span>
```

- **Static emphasis** → `class="code-line code-line-emphasis"`, no step attribute. Present in all outputs including `dist/`.
- **Stepped emphasis** → `data-emphasis-step="N"`, no emphasis class. A line
  in overlapping groups carries every step as a space-separated token list,
  for example `data-emphasis-step="1 2"`; a line in one group keeps the
  single-token form byte-for-byte. This is the overlap behavior adopted in
  [Issue #696](https://github.com/mizzy/peitho/issues/696).
- **Static partial emphasis** → one outer
  `<span class="code-range code-range-emphasis">` around the matched bytes.
- **Stepped partial emphasis** → one outer `.code-range` with the same
  space-separated `data-emphasis-step` token contract. Overlapping ranges are
  split where their effective step sets differ, and adjacent segments with the
  same set coalesce into one box.
- Blocks with no emphasis spec are **not** line-wrapped at all, so existing decks render byte-identical.

`Highlighter::highlight_lines` drives `ParseState`/`ScopeStack` directly. It
already closes and reopens scopes at line boundaries; partial emphasis adds
the same operation at each resolved byte boundary. At a range start it closes
the currently rendered `hl-*` stack, opens the `.code-range`, and reopens the
stack. At the range end it closes the stack, closes the range, and reopens the
stack for following text. The syntax state itself remains live throughout, so
one balanced range box can contain several syntax spans without inspecting or
reparsing generated HTML. The untagged path splits and escapes the original
line at the same byte ranges; with no partial ranges, it emits the same bytes
as the previous plain path.

### 4. Emphasis is a distinct attribute from reveal, deliberately

Reveal hides with `visibility: hidden`; emphasis must not hide anything — the surrounding code stays readable, it is merely de-emphasized. Reusing `data-reveal-step`/`data-reveal-hidden` would make un-emphasized lines *disappear*.

So the shell gains a parallel, equally small mechanism alongside `applyRevealState`: split each `[data-emphasis-step]` value on whitespace and toggle `data-emphasis-active` when any whitespace-separated step token equals the current step (note: **equals per token**, not "less than or equal" — emphasis moves, it does not accumulate), with `[data-emphasis-active]` styling injected next to the existing `REVEAL_HIDDEN_CSS`.

`stepnav.ts`, `sync.ts`, the `{index, step}` wire format, `ManifestSlide.revealSteps`, presenter, and remote are all **unchanged**: emphasis steps share the slide step space, are resolved by the parser, and surface through the same manifest field.

### Styling

Static-emphasis defaults live in the embedded
`crates/peitho-core/assets/code-emphasis.css`. When a checked deck contains at
least one non-stepped emphasis annotation, the renderer prepends that asset
once to `RenderedDeck::css()`, before author/theme CSS:

```css
.code-line-emphasis,
.code-range-emphasis { background: …; }
.code-line-emphasis { box-shadow: inset …; }
.code-range-emphasis { border-radius: 0.15em; }
pre:has(.code-line-emphasis, .code-range-emphasis)
  .code-line:not(.code-line-emphasis):not(:has(.code-range-emphasis)) {
  opacity: …;
}
```

The selectors deliberately do not use a `.slot-code` prefix: explicit
`::: {slot=name}` routing can place code in any code-accepting slot. Because
all build, preview, present, PDF, and lint workspaces write the same rendered
deck CSS, static markup and its defaults cannot diverge when a deck replaces
the built-in theme with its own `css/`.

Shared `stepState.ts` injects only the stepped `[data-emphasis-active]`
appearance and dimming rules on step-aware surfaces. It also retains the
shared `.code-line` layout rule because a stepped-only deck has no
static-emphasis block to prepend. Both embedded blocks supply only
`--peitho-emphasis-*` fallbacks; author/theme CSS remains last and can override
the defaults. The inset marker selector is scoped to active `.code-line`
elements; an active `.code-range` gets the tight rounded background only. Both
static and stepped dimming selectors exclude a line that contains an active
range, while lines with no active emphasis still dim whenever the block has an
active line or range.

## Errors (all line-numbered, with help)

| Condition | Rationale |
| --- | --- |
| Line number `0`, or `N-M` with `N > M` | No valid interpretation |
| Any line beyond the block's line count | Pillar ③ — an emphasis pointing at nothing must not be silently ignored |
| Partial text has zero matches | The authored pointer would silently disappear |
| Partial text has multiple matches, including overlaps | There is no occurrence index; lengthen the string until it is unique |
| Empty or unclosed partial text | There is no unambiguous substring to resolve |
| `}` ends the brace group while partial text is open | Brace extraction still ends at the first `}`; report the truncation at that seam |
| A range carries partial text (`2-4:"x"`) | Partial text names exactly one source line; list separate entries instead |
| Malformed spec (`{2-}`, `{a}`, unclosed `{`) | Parse failure must be loud |
| Pandoc-style attribute block (`{.rust}`, `{=html}`) | Help names the case and points at the bare-language form, so the author is not sent hunting for a line-number bug |
| Empty spec `{}` or empty group (`{2||4}`, `{2,,4}`) | Ambiguous intent |
| Missing, non-numeric, signed, oversized, or zero `@N`, or more than one `@` in a group | An explicit group must name exactly one unsigned positive ASCII-decimal step |
| `@N` on a static spec | Explicit steps are meaningful only for stepped emphasis; write `{|2-4@2}` |
| Implicit and explicit stepped groups mixed within a block or across a slide | A slide must use one numbering mode |
| The same explicit step repeated within one block | Merge the line entries into one group, e.g. `{|2,5@1}`; the same step across different blocks remains valid |
| A gap in the slide-wide explicit step set | Navigation must cover every position from 1 through the maximum |
| Explicit stepped emphasis and `::: {reveal}` anywhere on one slide | Explicit numbering owns the complete slide step sequence |
| Stepped emphasis on a code block inside `::: {reveal}` | See below |
| Emphasis on a `code_images` block (`mermaid`, `math`, declared external renderers) | The block becomes an image/math node; line emphasis has no meaning |

Existing behavior that must not regress: an unknown language tag is already a build error (`unknown code language 'notalang'`). Splitting the info string must keep that intact for the language token — only the trailing `{…}` is newly recognized.

### Rejected combinations

**Stepped emphasis inside `::: {reveal}`** is an error. “The code block
appears at step 5, then emphasis moves within it at steps 6–8” would create a
nested step space, while peitho has one flat slide sequence. Rather than invent
nesting, this is rejected with a line-numbered error suggesting the two
supported alternatives: keep the block in the reveal group with static
emphasis, or move it out of the group and use stepped emphasis. Explicit
`@N` emphasis is stricter: its slide cannot contain a reveal group anywhere.

**Static emphasis inside `::: {reveal}`** is fine and needs no special handling: the block appears at its reveal step with its emphasis already applied.

## Known tradeoff: line numbers go stale silently

Line numbers address lines **positionally**, so editing the code shifts what gets emphasized. Pillar ③ catches only the out-of-range case: insert a line above the emphasized region and `{2-4}` keeps pointing at lines 2–4, now the wrong ones, with no error and nothing visually wrong at build time. The realistic failure is editing a code block shortly before a talk and having the wrong line light up on stage.

This is accepted, not overlooked. The alternative — marker comments in the code (`// [!hl]`) — makes emphasis robust against edits but contaminates the code itself with presentation vocabulary, which is a worse trade under pillar ① and directly against the direction recorded in #364. Between "the pointer can go stale" and "the content is no longer clean code", peitho takes the first.

No mitigation is planned. A lint heuristic ("this block's line count changed and the emphasis is near the end") would be guessing at intent and would cry wolf on ordinary edits; a warning that fires on correct decks is worse than none. Authors reviewing a deck before presenting will see the emphasis in `peitho preview` (static) or by stepping through in `peitho present` (stepped), which is the real check.

Recorded here so that a future "emphasis pointed at the wrong line" report is recognized as a known consequence of the notation rather than a bug in it. Revisiting means revisiting the notation.

Partial entries reduce this risk for the emphasized bytes: edits that remove or
duplicate the text fail the next build. Their line number remains positional,
however, so moving an otherwise identical unique string to the addressed line
can still change intent without an error. Whole-line entries retain the
original tradeoff unchanged.

## Distribution policy

| Output | Static emphasis | Stepped emphasis |
| --- | --- | --- |
| `peitho present` | shown | steps through |
| `peitho preview` | shown | steps through on the stage; inactive in thumbnails/grid |
| PDF export | shown | inactive by default; exact step pages with `public_reveal: steps` |
| `dist/` (publish) | shown | inactive by default; steps through with `public_reveal: steps` |
| lint | shown | no emphasis |

This split follows from what the two notations mean. Static emphasis says "these lines are the important ones" — a property of the content, which belongs in distributed artifacts. Stepped emphasis is a pointer that tracks the speaker's narration; default/final output does not freeze an arbitrary moment, while `public_reveal: steps` deliberately emits or walks every exact state.

Static-emphasis markup and its conditional embedded CSS travel together in
`RenderedDeck::css()` through every output writer. Stepped emphasis is stamped
as `data-emphasis-step`; shared `stepState.ts` applies equality matching in
present, preview, stepped distribution, and steps-mode PDF expansion. Other
surfaces leave it inactive.

## Test plan

- **Parser**: valid specs (static / stepped / multi-item / untagged / with-language / mixed whole and partial); quote-aware `|`, `,`, and `@` with preserved spaces and punctuation; exact unique matches; overlapping duplicate matches; every error row above, asserting line number and help text; unknown-language regression; `{.rust}` produces the Pandoc-specific help rather than a generic parse failure; an emphasis spec on a `mermaid`/`math`/external-renderer block is rejected at parse time (asserted before any transform runs, pinning the ordering the architecture depends on).
- **Step counting**: implicit stepped emphasis contributes one consecutive step per group; explicit A→B→A blocks use their authored absolute steps and set `revealSteps` to the maximum; static contributes `0`; a code block inside `::: {reveal}` with static emphasis still contributes `1`.
- **Slide validation**: cover mixed numbering modes, reveal coexistence, gaps, cross-block shared steps, in-block duplicates, and groups authored out of step order, always asserting line number and help.
- **Render**: line-wrapping preserves syntect's `hl-*` classes and produces well-formed nesting across line boundaries (multi-line string literals and block comments are the adversarial cases, since they leave scopes open at end of line); one partial match spanning several `hl-*` spans remains one balanced box; plain code splits before escaping; overlapping ranges split only where the effective mark changes; whole-line membership is removed from ranges; adjacent equal marks coalesce; static and stepped markup remain distinct; no partial entry → byte-identical output to today; stepped-emphasis `<pre>` elements have no `data-reveal-step`.
- **Shell (vitest)**: `data-emphasis-active` follows the current step exactly (not cumulatively); emphasis clears when stepping past the last group; a multi-token marker is active at each listed step only; line and range selectors receive different appearances, and the dimming selector exempts a line containing an active range; listeners torn down per test.
- **E2E (browser, required)**: explicit A→B→A emphasis moves in authored step order; default/final output leaves the pointer inactive, while `public_reveal: steps` walks or emits each exact state. Per CLAUDE.md, jsdom cannot confirm the visual result.
- **Example deck**: extend `examples/` with a walked-through code block so the feature is covered by the demo site build.

## Resolved styling decision (2026-08-01; partial ranges extended 2026-10-23)

Emphasis uses **both** a background tint on emphasized lines and dimming of the rest. Dimming is scoped by `:has()` so it applies only while some line in that block is emphasized — a block whose stepped emphasis has not yet started, or has been stepped past, renders as ordinary undimmed code. Untagged blocks get identical styling; the only difference is the absence of `hl-*` spans inside the line.

Whole lines retain the inset marker bar. Partial ranges use the same background
as a tight box with a slight radius and no marker bar. A line containing an
active partial range is part of the current focus and therefore is not dimmed.

Default colors and opacity live in the renderer's embedded static CSS and
shared `stepState.ts` stepped CSS. Both use `--peitho-emphasis-*` fallbacks, so
author/theme CSS can change the appearance without touching the build.
