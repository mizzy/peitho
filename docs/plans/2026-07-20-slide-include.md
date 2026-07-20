# Slide-level Markdown include

Issue: #330. Author decisions (2026-07-20):

- **Syntax**: page-settings comment `<!-- {"include":"foo.md"} -->`. Reuses
  the existing `parse_page_comment` seam, its JSON validation, and its
  line-numbered errors. No new lexical surface. The one wrinkle — the
  same syntactic shape now describes both "attributes of this slide" and
  "expand into zero-or-more slides at this location" — is resolved by
  making `include` mutually exclusive with every other page-settings key
  and by requiring the include comment to sit on a slide with no body
  content (see "Include slide shape" below). The include slide is a
  container, not a real slide.
- **Frontmatter in included files**: forbidden. If the leading `---` of
  an included file parses as YAML frontmatter, the include is a
  line-numbered build error attributed to the included file. Only the
  top-level deck carries frontmatter, so `time`, `layouts`, `css`,
  `syntaxes`, `fonts`, `code_images`, `breaks`, `aspect_ratio`, and
  `resolution` all live in one place and section-total-vs-`time`
  validation is trivially preserved. This is the most easily-relaxed
  choice; ban first, revisit if a concrete need appears.
- **Section markers in included files**: allowed. An included file may
  contain `{"section":"Name","time":"1m"}` markers on its slides. After
  the include expansion (which produces a flat ordered slide list),
  `resolve_deck_sections` runs against the expanded list and the
  frontmatter-`time`-vs-section-total check applies unchanged. This lets
  a shared intro/outro carry its own section timing without special
  cases downstream.

## Non-goals (v1)

- URL / remote includes.
- Partial-slide fragment includes (splicing headings / lists / code
  blocks into a slide body).
- Templating / parameter substitution.
- Code-file includes into fenced blocks (`{{ include "foo.rs" }}`).

Each of these becomes its own follow-up issue if desired.

## Expansion seam

Include expansion happens **before** `parse_markdown`, **after** the
top-level file has been read from disk and its frontmatter has been
consumed by `parse_frontmatter`. The included files are not fed through
`parse_frontmatter`; instead the include expander splits each included
file into slide ranges and splices those slides into the top-level
deck's slide sequence as raw markdown, then hands the resulting single
combined source string to `parse_markdown` alongside the top-level
`ParsedFrontmatter`.

Concretely: a new `crates/peitho-core/src/include.rs` module exposes

```rust
pub fn expand_includes(
    top_source: &str,
    top_body_start: usize,
    top_path: &Path,
) -> Result<ExpandedSource>;
```

where

```rust
pub struct ExpandedSource {
    pub source: String,               // top frontmatter + spliced body
    pub body_start: usize,            // unchanged from top_body_start
    pub line_map: LineMap,            // per-line origin (file path + original line)
}
```

`expand_includes` performs one lightweight scan over the top file's
body to find include comments (see "Detection" below), reads each
included file (line-numbered `Missing`/`IoError` on failure), recursively
expands its includes (tracking a visited-paths stack for cycle
detection), and produces a combined source. The combined source is
what `parse_markdown` sees.

The CLI (main.rs and asset_resolution.rs) already reads the file
itself and calls `parse_frontmatter(&markdown)` then
`parse_deck_and_transform(&markdown, frontmatter, ...)`. The seam
change: after `parse_frontmatter` returns, call `expand_includes` with
the top file's body range and canonical path, then pass
`expanded.source` (not the original `markdown`) into
`parse_deck_and_transform`. The `ParsedFrontmatter.body_start` is
unchanged — expansion preserves the byte offset where the body begins
because the top frontmatter block sits at the very start of the
combined string, byte-identical to the original.

Callers touched: `crates/peitho/src/main.rs` (cmd_build, cmd_layouts,
cmd_preview, cmd_present, cmd_export, cmd_publish, cmd_lint's paths
that read the deck source) and `crates/peitho/src/doctor.rs` and the
in-crate `code_images::parse_deck_and_transform` call sites in tests.
The seam is added as a single helper in `main.rs`
(`load_and_expand_deck_source(path)`) that all commands funnel through
so the expansion pass is not sprinkled site by site.

## Detection: how includes are recognized before markdown parsing

The include expander does not run pulldown-cmark. It performs a
line-oriented scan that recognizes an include comment only in
positions where a page-settings comment is legal today: a
whole-line HTML comment `<!-- {...} -->` that sits inside a slide
region (between two `---` slide separators or between the last
frontmatter `---` and the first following `---`), skipping content
inside fenced code blocks. This mirrors the way peitho already
distinguishes legitimate slide `---` splits from fenced-code `---`
lines.

An include comment is a whole-line `<!-- {"include":"path"} -->`. The
JSON is parsed with `serde_json`. If the object has any key besides
`include`, or if `include` is not a non-empty string, or if the
containing slide region has any non-whitespace / non-comment content,
the expander emits a line-numbered `BuildError` and never falls back
to treating the include comment as an ordinary page-settings comment
(the parser must never see it — see "Include slide shape").

Only a comment discovered by this pre-scan is treated as an include.
`parse_page_comment` gains a symmetric guard: if it ever sees
`"include"` in a page-settings JSON, that is a hard error
("include comment must be the only content of its slide" — the
expander would already have removed it, so seeing it here means the
comment was in a position the expander does not recognize, which we
want to catch loudly rather than silently split-brain).

## Include slide shape

An include comment must be the entire content of its slide region:

```
---
<!-- {"include":"shared/intro.md"} -->
---
```

Anything else in the slide (headings, body text, other page-settings
comments, speaker notes) is a line-numbered build error with help
"place the include comment in its own slide bounded by `---`". This
keeps the expansion mechanical (delete the include slide, splice the
included slides in its place) and prevents confusing constructs like a
title above an include that expands to a title-bearing slide of its
own.

The rationale for reusing the page-comment shape while forbidding
sibling content: the alternative (a shape that allows sibling content)
raises unanswerable design questions — does the sibling content
survive the expansion? attach to the first spliced slide? the last?
None of those compose with `draft`/`skip`/`section` on the outer slide.
The container-only rule sidesteps all of it.

## Nested includes and cycles

Nested includes are allowed. The expander walks includes recursively
with a visited-paths stack keyed on canonical (`std::fs::canonicalize`)
paths. If a canonical path is already on the stack, the expander emits
a line-numbered `Cycle` error naming the include chain
(`A.md → B.md → A.md`). If canonicalization fails (broken symlink,
etc.), the expander falls back to the lexically-normalized path for
the cycle key so we still detect cycles rather than infinite-loop.

The expander enforces `MAX_INCLUDE_DEPTH = 64` as a stack-safety cap
alongside cycle detection: a legitimate non-cyclic chain deeper than
64 produces a line-numbered "include chain exceeds max depth of 64"
error rather than blowing the thread stack.

## Frontmatter in included files: forbidden

After reading an included file, the expander runs the leading-`---`
detector already implemented in `parse_frontmatter` (via a
pub(crate) helper it exposes for this purpose) purely to detect the
presence of frontmatter. If a frontmatter block is present, the
expander emits:

```
error: included file has frontmatter, which is not supported
  --> shared/intro.md:1
  help: move frontmatter to the top-level deck; included files may only contain slides
```

Detection is stricter than "the file starts with `---`" — it must
match the frontmatter grammar so a legitimate first slide starting
with `---` on its own is not mistaken for frontmatter. In practice
this means: if the leading `---` opens a YAML block that closes at a
matching `---` before any real slide content, it is frontmatter.

## Section markers in included files: allowed

Section markers are just page-settings comments. Included slides go
into the flat slide list, `resolve_deck_sections` runs over the
expanded list, and the existing
"if frontmatter `time` is set, section total must equal it"
invariant applies unchanged. If the top-level deck sets `time: 30m`
and an included intro contributes a 3m section and the outro
contributes a 2m section, the deck body slides must account for 25m
of sections; a mismatch is a line-numbered error attributed to the
first mismatching marker (existing behavior).

Two markers on the same slide (e.g. an included slide already has a
marker and someone tries to add another) is already a line-numbered
error via the "at most one page settings comment per slide" rule.
Nothing new.

## Interaction with draft / skip / speaker notes

- `draft` on an included slide works exactly as it does on a top-level
  slide: it is dropped at parse end before `ParsedSlide.index`
  assignment. Draft dropping happens after include expansion, so it
  sees the expanded list uniformly.
- `skip` on an included slide surfaces as `ManifestSlide.skip`
  unchanged.
- Speaker notes (non-JSON HTML comments) on included slides ride
  through to `notes.json` unchanged.

The include slide itself is a container and cannot carry `draft`,
`skip`, `section`, `layout`, `key`, `page_number`, or notes — all of
those on the same slide as an `include` are line-numbered errors.
To skip an entire included file, mark the include slide with
`draft` at… no: that is disallowed. To conditionally omit an included
file, the author edits the top file. This is a deliberate v1
simplification.

## Source-file attribution in errors

`ParsedSlide.source_index` today is a `usize` (the parse-time slide
number) that error messages surface as "slide N". After include
expansion, "slide N" alone is ambiguous — is slide 7 in the deck or in
`shared/intro.md`? The fix is to carry a per-line origin table
alongside the combined source and to translate `BuildError.line` back
through it at error-emission time so messages say `foo.md:12` instead
of a raw combined-source line number.

The origin table (`LineMap`) is a `Vec<LineOrigin>` where index N
holds `{file: PathBuf, original_line: usize}` for combined-source line
N+1. Building it is trivial during splicing (each appended chunk
records its source file and the mapping from its combined-source lines
to original file lines). Translation is a single method
`LineMap::translate(line) -> (PathBuf, usize)`.

The seam for translation: `BuildError` gains an
optional `origin_file: Option<PathBuf>` field (defaulting to the
top-level deck path). The CLI's error printer (`core()` and the
miette adapters in `main.rs` / `doctor.rs`) is the one place that
consults the `LineMap` — parser/mapping/check keep operating on
combined-source line numbers internally. When an error bubbles out to
the CLI, `translate` runs once and the message is formatted with the
per-file line number and the file's display path (relative to the
top-level deck's directory when possible).

Errors raised inside `expand_includes` itself (missing include target,
frontmatter in included file, cycle, malformed include comment) are
attributed directly to the file being expanded and carry
`origin_file` set at construction time.

## Test surface

Unit tests (`crates/peitho-core/src/include.rs` tests):

1. Missing include target → line-numbered error naming the missing
   path.
2. Frontmatter in included file → line-numbered error naming the
   included file.
3. Cycle (self-include) → line-numbered error naming the chain.
4. Cycle (A→B→A) → line-numbered error naming the chain.
5. Nested include (A→B→C, no cycle) → expanded body concatenates all
   three files' slides in order.
6. Include comment with a sibling heading in the same slide → error.
7. Include comment with a sibling page-settings comment in the same
   slide → error.
8. Include JSON with an extra key (`{"include":"x.md","layout":"y"}`)
   → error.
9. Include JSON with a non-string include value → error.
10. Include inside a fenced code block → **not** an include (verbatim
    code content preserved).
11. Whole-source integration test: a top file with two includes
    produces the expected combined slide sequence, and building the
    combined source with `parse_markdown` yields the expected
    `Deck<Parsed>`.
12. `LineMap` translation: an error raised on line X of an included
    file surfaces as `included/file.md:Y` in the CLI error.

CLI integration tests: build a two-file deck, verify manifest and
`notes.json` contents.

Snapshot / doctest updates: a new `docs/plans/…` code sample and a
short `examples/include-decks/` (top deck + shared intro) example
that the demo build can pick up if desired (out of scope for v1;
opening an issue for it if the shape lands).

## Rollout order (TDD-friendly, small green steps)

1. Add `include.rs` module with `LineMap`, `LineOrigin`, and a
   trivial `expand_includes` that returns the input unchanged when no
   include comment is present. Wire it into the CLI at
   `load_and_expand_deck_source`. Verify nothing regresses.
2. Add pre-scan detection of a whole-line include comment inside a
   slide region (no expansion yet — just detect and error on
   malformed shape).
3. Implement single-file (non-nested) expansion: splice one included
   file's raw slides into the combined source, extend the `LineMap`,
   splice out the container include slide.
4. Add included-file frontmatter detection (forbidden).
5. Add nested include recursion + visited-path cycle detection.
6. Add `BuildError.origin_file` and the CLI-side `LineMap`
   translation so error line numbers surface per file.
7. Extend `parse_page_comment` with the belt-and-braces "include key
   not allowed here" guard.

Each step is a Red/Green/Refactor cycle with tests added first.

## Alternatives considered and why they lost

- `!include foo.md` line directive: adds a new lexical surface with no
  reuse of existing infrastructure. Requires a fresh
  fenced-code-aware scanner. Rejected.
- `::: {include="foo.md"}` fence: reuses explicit-slot tokens but the
  semantics are unrelated (slot routing vs deck expansion). Would
  fork the meaning of `:::`. Rejected.
- Allowing sibling content on an include slide: raises unanswerable
  attach-semantics questions with no clean answer. Rejected — the
  container-only rule is a hard constraint from day one.
- Allowing frontmatter in included files with a merge policy: adds a
  merge rule that has to live in author's head forever. Rejected;
  easier to relax later than to remove.
