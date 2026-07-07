# Issue 175: render slots by checked contract

## Problem

`render_slot` dispatches from the first fragment kind. A `blocks` slot that starts
with a heading is rendered through the inline-heading path, so later paragraph or
list fragments can become empty output. This violates the no-silent-drop
invariant because the checked slot contract is ignored after checking.

## Plan

1. Add red tests for a `blocks` slot containing heading, paragraph, and list
   fragments; inline title rendering; code/image rendering; and the two-column
   example slide that currently loses paragraph text.
2. Preserve slot contracts in the checked phase by carrying a checked slot value
   instead of only `Vec<SourceFragment>`.
3. Pass the checked `Accepts` contract into `render_slot`.
4. Replace first-fragment dispatch with an exhaustive `Accepts` match. Keep
   inline/code/image behavior unchanged; render `blocks`, `list`, and existing
   `text` slots through markdown block rendering.
5. Keep mismatch handling explicit in specialized branches so accidental
   contract drift returns a build error instead of producing silent output loss.

## Gates

- `cargo fmt --all --check`
- `cargo test --workspace` three consecutive times
- `cargo clippy --workspace --all-targets -- -D warnings`
- `git diff --exit-code bindings/`
