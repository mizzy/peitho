# Code Line Emphasis — Design

**Issue:** #372
**Date:** 2026-08-01
**Status:** Approved by author (notation, static-vs-stepped split, distribution policy, untagged-block support decided 2026-08-01)

## Goal

Draw the audience's eye to specific lines of a code block: "the line I'm talking about right now". Optionally move that emphasis as the talk progresses, so a long code block can be walked through in place instead of being split across several slides.

This is **not** syntax highlighting. Syntax highlighting is a property of the content (in Rust, `fn` is a keyword); line emphasis is a property of the *talk* (right now I am discussing line 3). The two live on separate layers and must not be conflated — emphasis renders on top of, and independently of, the `hl-*` spans syntect produces.

The hard constraints (from the three pillars):

- **Pillar ③**: out-of-range lines, malformed ranges, and unparseable notation are all line-numbered build errors with help. Silent dropping is forbidden — an emphasis the author believes is active but that silently does nothing would send them on stage misinformed.
- **§16 event contract**: the slide body stays passive; only the shell executes transitions.
- Decks that do not use the notation must build byte-identical to today.

Issue #364 (moving presentation vocabulary out of Markdown into layouts) is explicitly **not** a constraint here (author decision 2026-08-01: "#364 is a best-effort direction, don't factor it in for now"). Note also that line emphasis is structurally unable to move layout-side: a layout is a container shared across slides, so it cannot name "line 3 of this particular code block". If #364 is ever pursued, this notation belongs to the residue it already contemplates ("accepting that some residue stays in Markdown").

## Author decisions (2026-08-01)

| Question | Decision |
| --- | --- |
| Where the notation lives | Code fence info string, after the language token |
| Static vs stepped | The `\|` separator is the sole discriminator: no `\|` → static (0 steps), `\|` present → one step per group |
| Step number space | Shared with incremental reveal (#290) — same counter, same `data-reveal-step` attribute |
| Line numbering basis | Per fragment, always starting at 1 (not per concatenated `<pre>`) |
| Distributed artifacts (PDF / preview / lint / `dist/`) | Stepped emphasis: absent. Static emphasis: present |
| Untagged code blocks | Supported — emphasis works without a language tag |
| Non-contiguous lines | Supported from v1 via `,` |
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
spec      := "{" group ("|" group)* "}"
group     := item ("," item)*
item      := N | N "-" M          (1-based, inclusive, N <= M)
```

- **`|` absent** — static emphasis. The listed lines are emphasized whenever the slide is shown. Consumes **no** reveal steps.
- **`|` present** — stepped emphasis. Group *k* is emphasized at reveal step *k*; each group replaces the previous one. Consumes exactly *n* steps for *n* groups.
- The language token is optional: ` ```{2-4} ` (no language) emphasizes lines of an unhighlighted block. Detection is positional — an info string whose first token starts with `{` has no language.

### Collision with Pandoc attribute syntax

Positional detection means peitho claims a leading `{…}` for itself, which is where Pandoc and some CommonMark extensions put *attributes*: ` ```{.rust} ` and ` ```{=html} ` specify a language or output format, not line numbers. Under this notation those parse as a malformed emphasis spec.

No existing deck regresses — peitho currently treats the whole info string as a language name, so ` ```{.rust} ` is already an `unknown code language` error today. But a block pasted in from Pandoc-flavored Markdown will now fail with a *different* error, and the message must not send the author hunting for a line-number bug. The emphasis parse error therefore names the case explicitly: Pandoc-style attribute blocks are not supported; write the language bare (` ```rust `).

````markdown
```rust {2,5-7|9}
```
````

reads: step 1 emphasizes lines 2 and 5–7; step 2 emphasizes line 9.

### Why `|` is the discriminator

The alternative — treating static emphasis as "stepped emphasis with one group" — was rejected. If `{2-4}` consumed a step, the slide would first appear with no emphasis and require one keypress to reach the state the author wrote, which contradicts "these lines are always the important ones". Making the rule implicit instead ("one group means zero steps") creates a discontinuity: deleting one group from `{2-4|6-8}` would silently change the remaining group from stepped to static.

With `|` as an explicit marker the rule is uniform and local: count the separators.

### Why line numbers restart per code block

Multiple code fragments routed to the same slot are joined into a single `<pre>` at render time (an emitted-HTML detail). Authors see two code blocks and write `{2}` in the second one meaning *its* line 2. Numbering across the concatenation would also make emphasis in a later block shift whenever a line is added to an earlier one — a fragility with no upside. Emphasis spans are stamped inside per-fragment highlighting, before concatenation, so per-fragment numbering is what falls out naturally.

## Architecture

### 1. The spec is parsed at parse time and rides as a fragment annotation

`SourceFragment` gains `emphasis: Option<LineEmphasis>`, handled exactly like the existing `reveal_span`:

```rust
/// Line emphasis groups for one code fragment.
///
/// `groups` is never empty. `stepped` records whether the author wrote `|`:
/// stepped emphasis consumes one reveal step per group, static emphasis
/// consumes none and is always visible.
pub struct LineEmphasis {
    groups: Vec<LineGroup>,   // each a set of 1-based inclusive ranges
    stepped: bool,
}
```

No new `FragmentKind` variant, and **`FragmentKind::Code` stays a unit variant**. Making it `Code { emphasis }` would break exhaustive matches in 15+ sites across `check.rs`, `render.rs`, `code_images.rs`, `mapping.rs`, `plain.rs`, `domain.rs`, and `parser.rs` for no benefit — the language and source text already live on `SourceFragment` rather than in the variant, and emphasis belongs with them.

Consequences:

- Mapping, slot-contract checking, and `Accepts` validation are unchanged.
- `code_images.rs` needs **no** emphasis handling: emphasis on a `code_images` block is rejected at parse time (see "Rejected combinations"), so no annotated fragment ever reaches `transform_fragment`. This must be enforced by ordering, not assumed — the validation has to run at the same point the parser already resolves `renderer_for(language)` to decide whether a block is a diagram, so a rejected combination cannot slip past into the transform. An emphasis annotation surviving into `transform_fragment` would be silently dropped when that function rebuilds the fragment, which is exactly the failure mode pillar ③ forbids; a debug assertion there is cheap insurance against the ordering being broken later.
- `try_map_image_src_inner` destructures and rebuilds `SourceFragment`; it gains one field.

### 2. Step counting keeps its single source of truth

Step counting already has exactly one parse-time authority (`reveal_span_len`), with the renderer consuming spans and never recounting — an invariant the reveal implementation guards with an `unreachable!` when stamped elements and span length disagree. Line emphasis extends that authority rather than adding a second one:

- `reveal_span_len` gains a `FragmentKind::Code` arm: `groups.len()` for stepped emphasis, `1` for a code fragment inside `::: {reveal}` with no stepped emphasis, and — for stepped emphasis — the fragment is *not* additionally counted as one appearing block.
- The renderer stamps exactly `groups.len()` distinct step values and asserts agreement in the same shape as `render_revealed_list_fragment` does today.

### 3. Rendering: line-wrapping spans

Every line of every emphasis-carrying code block is wrapped:

```html
<pre class="slot-code"><code
><span class="code-line">fn main() {</span>
<span class="code-line" data-emphasis-step="1">    let config = load();</span>
```

- **Static emphasis** → `class="code-line code-line-emphasis"`, no step attribute. Present in all outputs including `dist/`.
- **Stepped emphasis** → `data-emphasis-step="N"`, no emphasis class. The shell adds the emphasis styling for the matching step.
- Blocks with no emphasis spec are **not** line-wrapped at all, so existing decks render byte-identical.

The implementation seam is `Highlighter::highlight_html`, which already iterates line by line but appends into one `ClassedHTMLGenerator` buffer, losing line boundaries; syntect's outer scope span (`<span class="hl-source hl-yaml">`) also wraps the whole block. Emitting per-line spans requires driving `ParseState`/`ScopeStack` directly and closing/reopening the open scope stack at each line boundary (`line_tokens_to_classed_spans`). This is the only substantial new code in the feature. The untagged path (plain escaped text) needs the same wrapping and is trivial by comparison — worth implementing first as a test scaffold.

### 4. Emphasis is a distinct attribute from reveal, deliberately

Reveal hides with `visibility: hidden`; emphasis must not hide anything — the surrounding code stays readable, it is merely de-emphasized. Reusing `data-reveal-step`/`data-reveal-hidden` would make un-emphasized lines *disappear*.

So the shell gains a parallel, equally small mechanism alongside `applyRevealState`: toggle `data-emphasis-active` on `[data-emphasis-step]` elements whose step equals the current step (note: **equals**, not "less than or equal" — emphasis moves, it does not accumulate), with `[data-emphasis-active]` styling injected next to the existing `REVEAL_HIDDEN_CSS`.

`stepnav.ts`, `sync.ts`, the `{index, step}` wire format, `ManifestSlide.revealSteps`, presenter, and remote are all **unchanged**: emphasis steps are reveal steps, counted by the same parse-time authority and surfaced through the same manifest field.

### Styling

Theme CSS (`themes/base.css`, next to the existing `hl-*` rules) defines the default:

```css
.slot-code .code-line-emphasis,
.slot-code [data-emphasis-active] { background: …; }
.slot-code:has([data-emphasis-active]) .code-line:not([data-emphasis-active]) { opacity: …; }
```

Colors stay in theme CSS, consistent with syntax highlighting: the build emits classes, the theme decides appearance.

## Errors (all line-numbered, with help)

| Condition | Rationale |
| --- | --- |
| Line number `0`, or `N-M` with `N > M` | No valid interpretation |
| Any line beyond the block's line count | Pillar ③ — an emphasis pointing at nothing must not be silently ignored |
| Malformed spec (`{2-}`, `{a}`, unclosed `{`) | Parse failure must be loud |
| Pandoc-style attribute block (`{.rust}`, `{=html}`) | Help names the case and points at the bare-language form, so the author is not sent hunting for a line-number bug |
| Empty spec `{}` or empty group (`{2||4}`, `{2,,4}`) | Ambiguous intent |
| Stepped emphasis on a code block inside `::: {reveal}` | See below |
| Emphasis on a `code_images` block (`mermaid`, `math`, declared external renderers) | The block becomes an image/math node; line emphasis has no meaning |

Existing behavior that must not regress: an unknown language tag is already a build error (`unknown code language 'notalang'`). Splitting the info string must keep that intact for the language token — only the trailing `{…}` is newly recognized.

### Rejected combinations

**Stepped emphasis inside `::: {reveal}`** is an error. The reveal span model is flat (`RevealSpan { start, len }`), and "the code block appears at step 5, then emphasis moves within it at steps 6–8" is a nested step space that representation cannot express. Rather than invent nesting, this is rejected with a line-numbered error suggesting the two supported alternatives: keep the block in the reveal group with static emphasis, or move it out of the group and use stepped emphasis.

**Static emphasis inside `::: {reveal}`** is fine and needs no special handling: the block appears at its reveal step with its emphasis already applied.

## Known tradeoff: line numbers go stale silently

Line numbers address lines **positionally**, so editing the code shifts what gets emphasized. Pillar ③ catches only the out-of-range case: insert a line above the emphasized region and `{2-4}` keeps pointing at lines 2–4, now the wrong ones, with no error and nothing visually wrong at build time. The realistic failure is editing a code block shortly before a talk and having the wrong line light up on stage.

This is accepted, not overlooked. The alternative — marker comments in the code (`// [!hl]`) — makes emphasis robust against edits but contaminates the code itself with presentation vocabulary, which is a worse trade under pillar ① and directly against the direction recorded in #364. Between "the pointer can go stale" and "the content is no longer clean code", peitho takes the first.

No mitigation is planned. A lint heuristic ("this block's line count changed and the emphasis is near the end") would be guessing at intent and would cry wolf on ordinary edits; a warning that fires on correct decks is worse than none. Authors reviewing a deck before presenting will see the emphasis in `peitho preview` (static) or by stepping through in `peitho present` (stepped), which is the real check.

Recorded here so that a future "emphasis pointed at the wrong line" report is recognized as a known consequence of the notation rather than a bug in it. Revisiting means revisiting the notation.

## Distribution policy

| Output | Static emphasis | Stepped emphasis |
| --- | --- | --- |
| `peitho present` | shown | steps through |
| `peitho preview` | shown | final state = no emphasis |
| PDF export | shown | no emphasis |
| `dist/` (publish) | shown | no emphasis |
| lint | shown | no emphasis |

This split follows from what the two notations mean. Static emphasis says "these lines are the important ones" — a property of the content, which belongs in distributed artifacts. Stepped emphasis is a pointer that tracks the speaker's narration; freezing an arbitrary moment of it into a PDF would communicate something the author never asserted.

It also requires no code in those paths: stepped emphasis is stamped as `data-emphasis-step` and only the present shell acts on it, so every other output shows no emphasis **by construction** — the same structure that already gives reveal its PDF/preview behavior.

## Test plan

- **Parser**: valid specs (static / stepped / multi-item / untagged / with-language); every error row above, asserting line number and help text; unknown-language regression; `{.rust}` produces the Pandoc-specific help rather than a generic parse failure; an emphasis spec on a `mermaid`/`math`/external-renderer block is rejected at parse time (asserted before any transform runs, pinning the ordering the architecture depends on).
- **Step counting**: stepped emphasis contributes `groups.len()`; static contributes `0`; a code block inside `::: {reveal}` with static emphasis still contributes `1`; `manifest.json` `revealSteps` reflects the total.
- **Render**: line-wrapping preserves syntect's `hl-*` classes and produces well-formed nesting across line boundaries (multi-line string literals and block comments are the adversarial cases, since they leave scopes open at end of line); no emphasis spec → byte-identical output to today; stamped step values match the counted span.
- **Shell (vitest)**: `data-emphasis-active` follows the current step exactly (not cumulatively); emphasis clears when stepping past the last group; listeners torn down per test.
- **E2E (browser, required)**: emphasis is visible and moves with arrow keys in `peitho present`; PDF export of a stepped deck contains no emphasis while a static deck does. Per CLAUDE.md, jsdom cannot confirm the visual result.
- **Example deck**: extend `examples/` with a walked-through code block so the feature is covered by the demo site build.

## Resolved styling decision (2026-08-01)

Emphasis uses **both** a background tint on emphasized lines and dimming of the rest. Dimming is scoped by `:has()` so it applies only while some line in that block is emphasized — a block whose stepped emphasis has not yet started, or has been stepped past, renders as ordinary undimmed code. Untagged blocks get identical styling; the only difference is the absence of `hl-*` spans inside the line.

Colors and opacity values live in theme CSS, changeable without touching the build.
