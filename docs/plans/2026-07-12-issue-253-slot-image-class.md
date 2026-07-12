# Issue #253: slot-image wrapper for image slot fragments

## Problem

`render_slot` emits `<span class="slot-<name>">`, `<pre class="slot-<name>"><code>`, or `<div class="slot-<name>">` wrappers for inline / code / block slot kinds, but the `Accepts::Image` branch (crates/peitho-core/src/render.rs:166) joins bare `<img>` tags without a slot-carrying container. Layout / theme authors reaching for `:has(.slot-image)` — the pattern every other slot supports — get a silent no-op. This bit the `examples/code-images/` deck's rendered pane (PR #251, commit `9a7e6b2`) and had to be worked around with `:has(.rendered-pane img)`.

## Decision — Option 1: `<div class="slot-image">` wrapper

Author decision (2026-07-12): wrap all image fragments of a single image slot in one `<div class="slot-image">…</div>`.

Considered:
- **Option 2** (document the bareness) — leaves the trap in place. Rejected by issue framing.
- **Option 3** (`class="slot-image"` on each `<img>`) — one class per image, breaks the "1 slot = 1 class instance in the DOM" shape that block slots have. If a future slot rendering needs a caption or a grid of images, the markup shape has to change; Option 1 leaves room in the wrapper.

Option 1 aligns with `Accepts::Blocks | Text | List` (single `<div class="slot-<name>">` wrapping all fragments of that slot). Inline (`<span>`) and code (`<pre>`) are also single-container per slot — Option 1 keeps image consistent with that invariant. The class rides the outermost rendered element for every slot kind.

## Change

`crates/peitho-core/src/render.rs`:

```rust
Accepts::Image => {
    let body = fragments
        .iter()
        .map(render_image_fragment)
        .collect::<Result<Vec<_>>>()?
        .join("\n");
    format!(r#"<div class="{class_name}">{body}</div>"#)
}
```

Behavior: exactly one `<div class="slot-image">` per image slot, containing 1..N `<img>` children (arity is enforced upstream by check.rs, so `fragments.is_empty()` early-returns as before at line 144). Empty slots continue to emit nothing (no empty wrapper).

## Tests

- Update `renders_image_with_resolved_src_and_escaped_alt` (crates/peitho-core/src/render.rs:1142) to assert both the `<div class="slot-image">` wrapper and the wrapped `<img>` (escaping unchanged).
- Add a multi-image image-slot test: two `SourceFragment::image` fragments produce one wrapper with two `<img>` children.
- Add an assertion that a slot with zero image fragments emits no `slot-image` wrapper (the `if fragments.is_empty()` guard).

## Non-changes

- No changes to `Accepts::Image` in check.rs or mapping.rs. The slot contract is unchanged.
- No changes to `examples/code-images/`. The existing `:not(:has(img))` selector keeps working; a future PR can migrate it to `:not(:has(.slot-image))` if desired, but that is not part of this issue's scope (this issue is about consistency at the renderer level).
- No bindings changes — this is a render-time HTML shape change only. `bindings/*.ts` are Rust-domain types; the emitted HTML is not part of that contract.
- No shell/preview changes — the class lives in slide HTML, not the shell.

## Verification

Standard gates:
- `cargo test --workspace` (3 runs)
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo fmt --all --check`
- `git diff --exit-code bindings/` (should be clean)
- `cd packages/peitho-present && npm run build && npm test && npm run typecheck`
- `git diff --exit-code packages/peitho-present/dist/{shell,preview}.js`

E2E: rebuild `examples/code-images/` and grep the emitted HTML for `class="slot-image"`. Confirm the deck still renders (workaround selector `:not(:has(img))` continues to match).
