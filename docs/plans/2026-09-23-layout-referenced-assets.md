# Layout-referenced assets and Range requests (Issue #529)

Reported by @kfly8 with a working fix in his fork
([kfly8/peitho#1](https://github.com/kfly8/peitho/pull/1)), offered as
"pick it up or reimplement however fits best". This plan reimplements the
first half at a different seam and takes the second half close to as-written.

## Author decisions (2026-09-23)

1. **CSS `url()` is out of scope** — filed as #625. Same bug class, but it
   needs a CSS parser rather than one more lol_html handler, i.e. a different
   mechanism, not a different call site.
2. **`srcset` is refused in v1** with a line-numbered error naming the
   layout and attribute. Slides render at a fixed resolution decided by
   peitho, not by the browser, so a responsive candidate list has no clear
   use here; one high-resolution image is the simpler answer. Refusing keeps
   pillar ③ (no silent drop) and leaves the parse for measured demand.
3. **Reimplement with credit** to @kfly8 in the PR body and `Co-Authored-By`.
4. **Add a video-background example deck** to the demo site
   (`$(DEMO_DECKS)` + `site/content/examples/`), with a small committed video.

## Problem

Two independent bugs, confirmed by reading the code on `main`:

### 1. A layout's own asset references are invisible to the pipeline

`ImageResolver` (`crates/peitho/src/main.rs:5356`) is fed exclusively by
`resolve_image_paths` (`crates/peitho-core/src/phase.rs:828`), which walks
`FragmentKind::Image` fragments — i.e. Markdown `![](...)` only. A layout that
writes `<video src="assets/hero.mp4">`, `<img src="bg.png">`, or
`<script src="mount.js">` directly gets nothing: no resolution, no copy, no
manifest entry.

The consequence is not one 404 but **three** symptoms from one root cause:

| stage | Markdown `![](img/a.png)` | layout `<video src="hero.mp4">` |
|---|---|---|
| discovery | `RawImagePath::new`, line-numbered | **none** — layout parsing reads only `class` / `name` / `accepts` / `arity` (`layout.rs:171-194`) |
| resolution | `resolve_image_paths` → `ImageResolver::resolve` | **none** |
| emit (all 5 roots) | `write_image_assets` via `write_shared_assets` | **none**, and `assets/` is `remove_dir_all`'d each emit (`main.rs:5347`) |
| publish validation | manifest `images()` existence-checked (`main.rs:4362`) | **not in the manifest → unchecked** |
| watch rebuild on edit | tracked as `(mtime, len)` (`main.rs:494`) | **not an input → stale** |
| nonexistent path | line-numbered build error (`main.rs:5390`) | **silent 404 at runtime** |
| serving | `png`/`jpg`/`gif`/`webp`/`svg` mapped | `mp4` → `application/octet-stream`, no `Range` |

The root broken invariant: **a layout is a first-class, version-controlled
part of the deck (pillar ②), but only Markdown is treated as a source of asset
references.** Every row above is that one gap seen from a different consumer.

### 2. The static server cannot serve video at all

`crates/peitho/src/server.rs:350` has no `mp4`/`webm`/`ogv` mapping, so video
falls through to `application/octet-stream`, and `respond_static`
(`server.rs:1253`) ignores `Range:` entirely — always a whole-body `200`,
never `Accept-Ranges`. @kfly8 measured that WebKit's `<video>` never leaves
`networkState: NETWORK_NO_SOURCE` without Range support, so even a correctly
copied video would not play. This half is a plain missing feature in one
function; no design tension.

## Review of the fork's approach to bug 1

His `copy_deck_assets_not_already_resolved` copies every regular file in the
deck's `assets/` directory into the output `assets/` directory, skipping names
a hashed asset already claims. It works for his deck, and the name-collision
and no-directory cases are tested. Three reasons not to take it as-is:

1. **`assets/` is peitho's *output* namespace, not an input one.** Every
   existing deck keeps images in `img/` (`examples/*/deck.md`), and
   `write_image_assets` writes content-hashed names into `dist/assets/`.
   Making the deck's `assets/` a magic input directory mixed into the same
   output directory means author-named and hash-named files share one
   namespace — and a deck whose `assets/` happens to contain a stale
   `<hash>-photo.png` silently shadows nothing today but is one rename away
   from doing so.
2. **It drops the three guarantees Markdown assets get.** No existence check
   (a typo'd `src` stays a runtime 404 instead of a build error), no
   deck-directory escape check (`main.rs:5380`), and no manifest entry.
   Pillar ③ says silent dropping is forbidden; a directory sweep cannot know
   a reference was mistyped because it never reads the references.
3. **It copies what is not referenced and misses what is.** Files in
   `assets/` that no layout mentions are shipped into `dist/` (publish
   contamination surface), while `<video src="media/hero.mp4">` outside
   `assets/` is still missing. Discovery by directory cannot match discovery
   by reference.

The watch-input symptom is untouched by his patch, which follows from the same
cause — resolution is where watch targets come from (`main.rs:1631`).

## Design — discover references in the layout, reuse the existing resolver

Layout HTML is **already** parsed with lol_html twice: once in
`parse_layout` (`crates/peitho-core/src/layout.rs:161`, collecting `<section>`
and `<slot>`) and once in the render pass (`render.rs:255-300`, replacing
`<slot>`). Asset discovery belongs in the first and path rewriting in the
second. No new parser, no new file format, no directory convention.

### Where each piece goes

**a. Discovery — `layout.rs`**

Add one `element!` handler to `parse_layout` collecting asset references from
the attributes that load a subresource:

```
img[src], img[srcset], video[src], video[poster], audio[src],
source[src], source[srcset], track[src], script[src], link[href],
object[data], embed[src], iframe[src]
```

Store them on `Layout` as an ordered, deduplicated
`Vec<LayoutAssetRef>` behind an accessor, in the same shape as `slots()`.
`LayoutAssetRef` keeps the raw attribute value plus which element/attribute it
came from, so an error message can say *where* in the layout.

A reference is only a **candidate**; classification is:

- absolute `http:`/`https:`/`data:`/`//` → ignored, left verbatim (matches
  `open_external_links_in_new_tab`'s treatment of external hrefs)
- `#fragment`-only → ignored
- anything else → a deck-relative asset reference to resolve

`srcset` is **refused** in v1 (author decision 1 above) with a line-numbered
error naming the layout and attribute — not ignored, which would 404 silently.
So the collected attribute set is `src`, `poster`, `href`, `data`, and
`srcset`-as-error.

**b. Resolution — reuse `ImageResolver` verbatim**

`ImageResolver::resolve` (`main.rs:5370`) already does exactly what these
references need: canonicalize, reject deck-directory escape, error with
"file not found" help when missing, content-hash, dedupe by hash, and produce
`ResolvedImageAsset { source_abs, dist_rel }`. It is not image-specific — the
only image-flavored things in it are three error message strings.

Critically, **`ResolvedImagePath` is already extension-agnostic**:
`from_hashed_asset` (`domain.rs:410`) validates only the 16-hex hash and that
the basename has no separators or URL delimiters, so `assets/<hash>-hero.mp4`
is representable today with no change to the type. The closed
`SUPPORTED_IMAGE_EXTENSIONS` 5-element whitelist (`domain.rs:451`) lives on
`RawImagePath`, which is the *Markdown* parse-time type — layout references
never become one, so video needs no whitelist edit. That is the difference
between this seam and a new one: the output type already fits.

So: rename the message helpers to speak of "asset" rather than "image", and
feed the layout's references through the same resolver. Two callers, one
resolver, one hashed-asset namespace, automatic dedupe when a layout and a
Markdown fragment reference the same file (same hash → same `dist_rel`).

Resolution happens where the resolver already runs, at `main.rs:1895` — its own
phase transition between check and render (`phase.rs:828`, "the only transition
from `Deck<Checked<RawImagePath>>` to `Deck<Checked<ResolvedImagePath>>`"), so
the layout's assets join `image_assets` and reach **every** existing consumer
with no new emit code:

- `write_shared_assets` → `write_image_assets` (`main.rs:2414`) — the one
  funnel all five output roots already share: `dist/`, preview cache
  generation dir, present cache, PDF workspace, and the lint workspace
  (`lint.rs:239`, which calls it as `crate::write_shared_assets`)
- `build_manifest` (`manifest.rs:403`) — `images` gains the layout's assets,
  which also means **publish validation covers them for free**:
  `validate_manifest_refs` (`main.rs:4362`) existence-checks every
  `manifest.images()` entry against `dist/`. A directory-sweep approach gets
  none of that, because swept files are in no manifest.
- `resolve_watch_targets` (`main.rs:1631`) — see (d)

`write_image_assets` needs **no change at all**. That is the test that this is
the right seam. (Note it does `remove_dir_all` on `assets/` every emit, so
anything not in `image_assets` cannot survive there anyway — another reason
discovery has to feed the list rather than the directory.)

**c. Path rewriting — `render.rs`, and it revises a documented invariant**

The emitted file is `dist/assets/<hash>-hero.mp4`, but the layout says
`src="hero.mp4"`, so the rendered HTML must be rewritten to the resolved path
— exactly as Markdown images are. The render pass already rewrites layout
HTML in two targeted ways (`render.rs:229-294`: `<section>` gets
`data-slide-key` etc. and the `peitho-slide` class; `<slot>` is replaced
wholesale), so this is a third handler in an existing rewriter, not a new pass.

**But CLAUDE.md:42 currently says, as part of the external-links bullet,
"layout HTML is never rewritten".** In context that sentence scopes the *link*
pass — `open_external_links_in_new_tab` runs at the `render_slot` seam
(`render.rs:387`), on slot content only, and deliberately does not touch
layout anchors. Since `<section>` attributes are already rewritten, the
sentence is about that pass rather than a global invariant. Still, adding an
attribute rewrite to layout HTML makes the wording actively misleading, so
**this plan must update CLAUDE.md:42 in the same PR** to say what is
rewritten and what is not. Flagging rather than deciding: if you read that
line as a hard global invariant you want kept, the alternative is to resolve
the path without rewriting — which means the emitted `src` must already equal
the dist path, i.e. the author writes `assets/<hash>-...` by hand. That is
unusable, so I recommend the rewrite plus the doc fix.

Slide HTML lives at `dist/slides/*.html` while assets live at `dist/assets/`,
so the written value must use the same relative form the Markdown image path
already resolves to — read what `ResolvedImagePath::as_str` yields at the
existing `<img>` site and match it rather than hand-rolling a prefix.

This makes `render_deck` need the resolved layout references. Deck-carried
types already ride the phases (`Deck<Checked<ResolvedImagePath>>`), so the
resolved mapping should ride the same way rather than being passed as a loose
side-channel argument — the typed reshape, not a parameter every future
caller must remember to populate.

**d. Watch inputs — `parser.rs` / `main.rs:1631`**

`referenced_image_paths` (`parser.rs:574`) answers "what files does this deck
reference" for the watcher, walking Markdown fragments only. Layout references
must join its result (or a sibling function that unions both) and be added as
`WatchRoot::input(path, None)` entries in `WatchTargets::new` (`main.rs:478`),
the same `(mtime, len)` treatment images get.

Note this is **not** already covered by the existing `layouts` watch root: that
root is registered with `Some("html")` (`main.rs:496`), so
`collect_asset_files` globs `*.html` only and a `.mp4` sitting inside
`layouts/` is invisible to it. Without this task, editing the layout rebuilds
but replacing the video does not, and bug 1 is only two-thirds fixed —
invisible until an author swaps a video and sees nothing change.

**e. Publish contamination**

Layout assets are legitimate `dist/` content, so
`PRESENTATION_ONLY_DIST_FILES` (`main.rs:909`) is unchanged — it is a
seven-name denylist, not a whitelist, and there is no stray-file sweep over
`dist/` at all (the only recursive walk is
`reject_preview_edit_annotations`, which is indifferent to file names). That
cuts both ways: it means nothing rejects a layout asset, and it also means the
fork's sweep-everything copy would ship unreferenced files into `dist/`
unremarked. Reference-driven discovery never copies what no layout mentions,
so the risk does not arise here.

### Deliberately out of scope

- **`url()` in deck CSS.** `background: url("hero.png")` in a deck's own
  `css/` file has the identical gap (nothing scans CSS for `url()`;
  `themes/base.css` works only because `write_theme_fonts_assets` copies
  `theme-fonts/` unconditionally). It is the same class of bug at a sibling
  site, which normally means fixing both here — but it needs a CSS parser
  rather than one more lol_html handler, i.e. a different mechanism, not a
  different call site. **Flagging for your call: fold CSS `url()` into this
  issue, or file it separately?** If it stays separate it should be filed
  before this merges, not left implicit.
- Markdown `<video>` written as raw inline HTML in slide body text. Inline
  HTML in a block already suppresses edit spans and is not a layout concern.

## Design — bug 2, Range requests

Take the fork's shape; it is the right one. `parse_range_header` is a pure
function with the full RFC 7233 single-range grammar (bounded, open-ended,
suffix) and returns `None` for multi-range, inverted, out-of-bounds, non-bytes
units, and empty resources — each already covered by a named test. Serve
`206` + `Content-Range` on a hit, `200` on a miss, and `Accept-Ranges: bytes`
either way.

Changes from his version:

1. **Return a typed range, not `(usize, usize)`.** A bare pair of `usize`
   invites a start/end swap at the one call site that slices with it. A
   two-field struct built only by the parser makes the inclusive-end
   convention part of the type rather than a comment.
2. **`fs::read` before slicing reads the whole file into memory**
   (`server.rs:1265`) — a 200 MB background video is fully read to answer a
   1 KB range probe, and WebKit issues several. Open the file, `seek`, and
   read only the requested span; tiny_http takes a reader, and this is the
   only place that matters since it is the only route serving large media.
   Keeping `fs::read` would make Range support technically correct and
   practically sluggish on the exact file type that motivated it.
3. Out-of-bounds falling back to `200` is what his test asserts. RFC 7233 says
   416 for an unsatisfiable range; `200` is friendlier and no client depends on
   416 here. Keep `200`, with a comment saying it is deliberate.

Content types: add `mp4`/`webm`/`ogv` as he has them. Also add `m4v`, `mov`,
`mp3`, `wav`, `ogg`, and `avif` in the same match — the same one-line class of
omission, cheaper to add now than to rediscover per format.

`respond_static` is shared by present and preview, so both get this at once.

## Tasks

1. `LayoutAssetRef` + discovery handler in `parse_layout`; unit tests per
   attribute, plus external/data/fragment URLs ignored, and `srcset` parsed
   (or refused — pending your call).
2. Generalize `ImageResolver`'s error strings to "asset"; no behavior change
   (existing tests must pass untouched).
3. Resolve layout references through `ImageResolver` at `main.rs:1895`; assert
   a layout asset lands in `image_assets`, and that a layout and Markdown
   reference to the same bytes dedupe to one copy.
4. Nonexistent layout `src` → build error naming the layout and attribute;
   deck-directory escape → error. Adversarial test for each.
5. Rewrite resolved attributes in the render pass; assert the emitted slide
   HTML points at the hashed path and that a deck with no layout assets is
   **byte-identical** to before (the snapshot discipline used for
   `EditAnnotations::Off`).
6. Union layout references into watch inputs; test that touching a
   layout-referenced file marks the snapshot changed.
7. E2E: a deck whose layout has `<video src>` — `build`, `preview`, and
   `present` all serve it, and it actually plays. Per CLAUDE.md this must be a
   real browser, and per the pitfalls list a real device/WebKit check is what
   found the Range bug in the first place; I can drive Chrome, but the WebKit
   half needs you or @kfly8.
8. `parse_range_header` + typed range + seek-based slicing + content types,
   with the fork's test set adapted.
9. Update CLAUDE.md:42's "layout HTML is never rewritten" to scope it to the
   link pass and name the new attribute rewrite (see (c)), plus a bullet for
   layout-referenced assets in the invariants list.
10. A new `examples/` deck with a video background, if you want this covered by
    the demo site — that means `$(DEMO_DECKS)` + `site/content/examples/`, and
    a committed video in the repo. **Your call; I would not add it unasked.**

## The rejected alternative, for the record

The nearest working precedent in the codebase is the `fonts:` pipeline: a
deck-adjacent directory copied verbatim and recursively into all five output
roots, no extension filter, no hashing, no manifest entry
(`write_fonts_assets`, `main.rs:2420`). One could add a fifth `AssetKey` —
`media:` or `assets:` — and get layout video working by copying a directory.

It is rejected for the same reason as the fork's sweep, plus one more: it
would make the *author* declare where assets live in frontmatter, when the
layout already says so in its own `src` attributes. Pillar ② says the layout
is the schema; asking for the same information twice, in two files that can
disagree, is the thing that invariant exists to prevent. Fonts are different
because `@font-face` names live in CSS text that nothing parses, so there is
no reference to discover — here there is.

## Attribution

The diagnosis, the Range measurement, and a working fix are @kfly8's. If this
lands as a reimplementation, the PR should say so and credit him; if you would
rather merge his commits and refactor on top, the discovery seam above applies
equally as a follow-up.

## Open questions for you

1. **CSS `url()`** — same bug class, different mechanism. Fold in, or separate
   issue filed now?
2. **`srcset`** — parse the descriptor list, or refuse it in v1?
3. **Attribution shape** — reimplement with credit, or merge his PR and
   refactor on top?
4. **Demo deck** — add a video example to the docs site, or keep the repo free
   of committed media?
