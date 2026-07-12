# code_images: normalize SVG intrinsic size at the cache-write seam (Issue #261)

Date: 2026-07-12
Issue: #261

## Problem

mermaid-cli emits SVGs whose root tag is `<svg width="100%" style="max-width: …px" viewBox="0 0 W H">` — no `height` attribute, no absolute `width`, only an aspect ratio. Consumed via `<img>`, such an SVG has no usable intrinsic dimensions; in any CSS context that does not hand the image a definite size it collapses to 0×0. The slot-image wrapper introduced by #257 removed the accidental definite-size context the demo layouts had relied on (the `<img>` used to be a direct grid child), which made every mermaid diagram on the deployed demo invisible. graphviz output carries `width="181pt" height="293pt"` and kept rendering — the "sometimes visible" symptom was per-generator, not flaky.

## Decision

Fix at the single seam where peitho produces these assets: the code-images cache write in `crates/peitho-core/src/code_images.rs`. The invariant is **every SVG written to `.peitho/code-images-cache/` carries usable absolute `width`/`height` attributes on its root tag**. Consumers (build, preview, present, PDF export, the docs site, any layout CSS) need no knowledge of this.

Rejected alternatives, evaluated against long-term view / type-structure / root-cause:

1. Sizing the wrapper in each deck's CSS — a per-consumer carve-out every future layout must remember.
2. Making the renderer's `slot-<name>` wrapper layout-transparent (`display: contents`) — conflicts with the point of #253 (keyed overrides target the wrapper as a box).
3. (Adopted) Build-time normalization — restores the invariant upstream, once.

## Behavior

After `validate_svg_output` and before the atomic cache write, `normalize_svg_intrinsic_size`:

- locates the root `<svg …>` tag, skipping a leading UTF-8 BOM, XML declaration, comments (which may contain the text `<svg`), and DOCTYPE including a bracketed internal subset;
- parses the root tag's `width`/`height`/`viewBox` byte spans (single-, double-, and unquoted values). Attribute names match **exact case** — these files are consumed via `<img>`, i.e. XML parsing, where browsers honor only exact `width`/`height`/`viewBox`; matching what the browser ignores would either ship the bug (`WIDTH="100"`) or derive dimensions from an attribute the browser does not read (`viewbox`);
- a length is *usable* iff it is a positive finite number with a non-percentage unit;
- both usable → bytes pass through byte-identical (graphviz path);
- otherwise a valid positive viewBox supplies **both** `width` and `height` (raw viewBox byte tokens, so output is deterministic and idempotent): present attributes are replaced in place — including a usable one sitting next to an unusable one, because keeping it would pair it with a viewBox-derived counterpart and distort the intrinsic aspect ratio — and missing attributes are inserted before the tag close;
- no usable dims and no viewBox → line-numbered build error with help; root tag not locatable at all → a distinct root-not-found error. No silent path.

Cache self-heal: `valid_cached_svg` additionally requires the cached bytes to satisfy the same predicate (`svg_root_has_usable_dimensions`, shared with the normalizer so the two cannot drift). A cache file written by an older peitho is a miss; the command re-runs once and the normalized output overwrites it via the existing atomic temp-file rename.

## Non-goals

User-referenced `![](foo.svg)` assets are copied verbatim by design (same as fonts); peitho does not mutate user-owned files, so a hand-authored SVG without intrinsic dimensions still behaves as it would anywhere on the web. If that ever warrants a diagnostic, it is a separate decision for the author.
