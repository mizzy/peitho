# Explicit code-emphasis steps

Issue: #737
Date: 2026-10-23
Branch: `issue-737-explicit-emphasis-steps`

## Goal and invariants

- Support absolute stepped-emphasis notation such as `{1-3@1|8@3}` so code blocks can interleave their emphasis steps across a slide.
- Make resolved stepped emphasis carry an absolute step on every group. A code fragment must never borrow `RevealSpan` for emphasis, and unresolved stepped emphasis must never reach mapping or rendering.
- Preserve implicit notation and numbering byte-for-byte. The `|` remains the static/stepped discriminator; implicit groups continue to consume consecutive source-order steps.
- Explicit notation is all-or-nothing across a slide, cannot coexist with `::: {reveal}`, must cover every step from 1 through its maximum, may share a step across blocks, and may not repeat a step within one block.
- Keep `ManifestSlide.revealSteps`, the TypeScript step-state contract, and generated bindings unchanged. Continue rejecting stepped emphasis inside reveal groups and all emphasis on rendered code images.

## Implementation

1. Add failing grammar tests for explicit groups and every malformed `@` shape, plus parser tests for interleaving, slide-wide mixing, reveal conflicts, gaps, and duplicate rules. Add a render regression proving absolute `data-emphasis-step` values and no `data-reveal-step` on stepped-emphasis `<pre>` elements.
2. Split emphasis into an unresolved parse result and a resolved `LineEmphasis` representation: static line groups or stepped groups pairing a line group with its absolute `NonZeroUsize` step. Keep one line-membership accessor returning `LineMark::{None, Static, Stepped}` with every matching absolute step.
3. Resolve implicit groups from `next_reveal_step` and explicit groups from authored `@N` values in the parser. Reject repeated explicit steps while parsing each block; track the slide's emphasis mode and relevant source lines, then validate all-or-nothing usage, reveal incompatibility, and contiguous explicit coverage at slide end before computing `step_count`.
4. Remove the stepped-emphasis `RevealSpan` assignment, renderer visibility carve-out, and `base_step` arithmetic. Audit every remaining `reveal_span()` consumer so reveal checks, footnote ranges, mapping, and code-image transforms retain their existing meaning.
5. Update the guide, design record, and `CLAUDE.md` to describe explicit absolute steps and the revised ownership model. Do not change the presentation package unless verification exposes a real contract mismatch.

## Verification

- Run focused emphasis/parser/render tests during TDD.
- Run `cargo test --workspace` three consecutive times, Clippy with warnings denied, rustfmt check, bindings drift check, the presentation build/tests/typecheck, and all four committed bundle drift checks.
