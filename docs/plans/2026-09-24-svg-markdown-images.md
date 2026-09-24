# SVG as Markdown images (Issue #655)

## Goal

`![](img/diagram.svg)` builds instead of failing with "unsupported image
extension 'svg'". An author keeps one SVG source instead of an SVG plus an
exported PNG that drifts from it.

## Design

1. `svg` joins `SUPPORTED_IMAGE_EXTENSIONS` (`domain.rs`). Nothing else about
   path validation changes.
2. **One seam checks every SVG image**: `resolve_image_paths` (`phase.rs`) is
   the only transition from `RawImagePath` to `ResolvedImagePath`, and every
   image — Markdown `![]()` and `code_images` output alike — passes through it.
   After the resolver returns a `ResolvedImageAsset` whose raw path has an
   `svg` extension (ASCII case-insensitive), it reads `source_abs` and runs
   `check_image_svg` (`code_images.rs`, reusing its root-tag parser):
   - the root `<svg>` is located by `find_root_svg_tag` (no scan limit, so a
     long prolog or an Illustrator DOCTYPE is fine);
   - the root declares `xmlns="http://www.w3.org/2000/svg"` — without it
     Chrome shows a broken image in `<img>`;
   - the root carries usable absolute `width` and `height`
     (`svg_root_has_usable_dimensions`, the same predicate the code-images
     cache uses since #261).
   Failures are line-numbered build errors attached through
   `attach_image_resolve_context`, with distinct messages for "not an SVG
   document", "root `<svg>` not found", "missing SVG namespace", and "no
   usable intrinsic size"; the last one's help names the viewBox width/height
   as the values to write. `code_images` output passes the same check: its
   output seam rejects a namespace-less SVG before the cache write with a
   code-image error naming the tag (the resolve-time message would name a
   cache file the author cannot fix), sizes are normalized there, and
   `valid_cached_svg` uses `check_image_svg`, so an old namespace-less cache
   entry is a miss.
3. **User SVGs stay verbatim.** The #261 design record already decided that
   peitho does not mutate user-owned files; the copy in `dist/` is
   byte-identical to the author's file. A missing size is an error the author
   fixes once in the source, not a silent rewrite.
4. SVGs referenced directly by layout HTML (`<img src>`, `<object data>`, …)
   are not checked: the layout author writes the element and its CSS, and
   those references never become `RawImagePath`.
5. Display stays `<img>`: scripts inside the SVG never run. Fonts: an SVG in
   `<img>` cannot load the deck's web fonts and draws with system fonts; the
   guide says so (convert text to paths or embed the font in the SVG).

## Known limits

The check is a root-tag inspection, not an XML parser. Malformed XML (an HTML
entity such as `&nbsp;`, an unclosed tag), a prefixed root (`<svg:svg>`),
uppercase units (`400PX`), and UTF-16 files are not diagnosed precisely:
malformed XML builds and shows a broken image, the rest are rejected even
though a browser would render them. Revisit with a
real XML parser if authors hit them.

## Tasks

1. Tests first (core): a Markdown `svg` path parses; `resolve_image_paths`
   accepts an SVG with absolute width/height, and rejects with line numbers
   an SVG with only `viewBox` / `width="100%"`, a non-SVG file named `.svg`,
   and an uppercase `.SVG` without size. Update the existing parser test that
   asserts `svg` is unsupported.
2. Implementation in `domain.rs`, `phase.rs`, and `code_images.rs` (expose the
   two predicates to `phase.rs` as `pub(crate)`).
3. CLI build test: a deck with `![](img/d.svg)` builds and `dist/assets/`
   holds a byte-identical copy.
4. Docs: guide `writing-decks.md`, README, CLAUDE.md invariant bullet.
