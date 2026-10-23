# Partial code-line emphasis

Issue: #740
Date: 2026-10-23
Branch: `issue-740-partial-line-emphasis`

## Goal and invariants

- Extend code emphasis entries with `N:"text"`, preserving the existing static, implicit-step, and explicit-`@N` behavior for whole-line entries.
- Resolve every partial entry at the parser seam against `emphasis_code(text).lines()`. The text must occur exactly once, counting overlapping starts, and the resolved pipeline value carries only a line number and byte range.
- Keep one membership accessor. `LineEmphasis::mark_for_line` reports whole-line membership and normalized, non-overlapping partial segments: it removes tokens already supplied by the whole line and coalesces adjacent segments with equal marks.
- Insert partial-range boundaries while source byte offsets and syntect scope operations are available. Never inspect or rewrite generated HTML, and preserve the existing output path byte-for-byte for blocks without partial entries.
- Render partial emphasis as a tight inline box. An active partial range keeps its containing line undimmed; whole-line emphasis retains its marker bar. Static partial emphasis still triggers the embedded emphasis stylesheet.

## Implementation

1. Add failing grammar tests for quote-aware `|`, `,`, and `@` splitting; preserved spaces and punctuation inside text; mixed whole/partial entries; explicit steps; exact-match validation including overlapping matches; and focused malformed-quote, brace-truncation, empty-text, and range-plus-text errors.
2. Replace a group's range-only representation with a typed whole-line-or-partial entry. Resolve partial text to byte ranges during `parse_emphasis_spec`, then extend `LineMark` to expose whole-line state plus normalized partial segments from the same accessor.
3. Teach `highlight_lines` to close and reopen its live `hl-*` scope stack at partial boundaries, keeping one balanced `code-range` span around each segment. Add the equivalent offset-driven escaped-text path for untagged code, while leaving non-partial highlighting unchanged.
4. Add static and stepped range styles, scope the whole-line marker bar to line elements, and exclude lines containing an active range from dimming. Update step-state tests and rebuild the committed presentation bundles.
5. Document the syntax and constraints in the guide, design record, and `CLAUDE.md`; add one partial-emphasis example slide and update only its new or renumbered slide-hash snapshot entries.

## Verification

- Run focused Rust and TypeScript tests throughout the change.
- Run `cargo test --workspace` three consecutive times, Clippy with warnings denied, rustfmt check, bindings drift check, and the presentation build/tests/typecheck plus all committed bundle drift checks.
