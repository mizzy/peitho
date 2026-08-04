# Tweet embeds in slides via build-time screenshot (Issue #395)

Date: 2026-08-04
Status: approved by author (approach, syntax, and scope decided 2026-08-04)

## Goal

Let a deck quote a tweet (X post) in a slide without hand-screenshotting it.
The author writes a reference in Markdown; `peitho build` resolves it into a
static artifact that looks like the official X embed and renders identically
in present, preview, PDF, and `dist/` — with no external scripts anywhere in
the output.

Raw HTML passthrough stays a build error. It would leak HTML vocabulary into
content (pillar ①), the official `widgets.js` cannot see blockquotes inside
the slide shadow roots, and a runtime script renders as nothing in PDF export
and offline viewing.

## Decisions (author, 2026-08-04)

1. **v1 renders via Chrome screenshot of the official embed.** Fidelity to
   the original site is the requirement, and the official renderer is the
   ceiling of fidelity — a hand-styled card is always an approximation. A
   theme-matched self-rendered card (from oEmbed data) is a follow-up,
   opt-in, separate issue.
2. **Syntax is an `embed` fenced code block** containing exactly one URL:

   ````markdown
   ```embed
   https://x.com/gosukenator/status/2083825695709597710
   ```
   ````

   Bare-URL auto-detection was rejected (indistinguishable from "I want a
   link", and implicit transformation sits badly with the no-silent-drop
   stance). Extending `![](…)` image syntax was rejected (`RawImagePath`
   rejects remote URLs by construction; weakening that type is a bigger
   change than the feature).
3. **v1 accepts only X status URLs** (`https://x.com/<user>/status/<id>`,
   `https://twitter.com/<user>/status/<id>`). Any other URL is a
   line-numbered build error. Generic oEmbed-discovery support is a
   follow-up issue; the internal shape is a URL→provider dispatch so adding
   providers does not reshape the pipeline.

## Approach

The pipeline mirrors `code_images` (reference in a fenced block → build-time
transform on `Deck<Parsed>` → cache under `.peitho/` → the fragment becomes a
`FragmentKind::Image` riding the existing asset pipeline).

### Resolver integration

`embed` becomes a built-in tag in the single typed resolver, exactly like
`mermaid` and `math` (CLAUDE.md invariant: no `tag == "embed"` carve-outs
anywhere else):

- `CodeImagesConfig::renderer_for("embed")` returns a new
  `CodeImageRenderer::BuiltinEmbed` when there is no explicit
  `code_images.embed:` entry.
- An explicit `code_images.embed: <command>` keeps its current meaning (an
  external SVG command receiving the block body on stdin) — the same
  override semantics as `code_images.mermaid:`/`code_images.math:`.
- Because the parser already excludes resolver-backed tags from
  unknown-language validation and rejects line emphasis on them, `embed`
  inherits both behaviors with no new parser branches.

### Parse-time validation (all line-numbered errors with help)

Validated when the transform encounters a `BuiltinEmbed` block:

- empty block, more than one non-blank line, or surrounding junk
- URL that is not an X status URL (error names the supported forms and
  points at the follow-up for other providers)
- the existing rule set already covers: emphasis on the block, unknown tags

### Rendering (CLI-side, core stays pure)

`peitho-core` gets an `EmbedRenderer` seam (trait like `SvgRunner`, one
method: URL in, PNG bytes out) so the core transform stays pure and testable
with a fixture renderer. The CLI implements it with the existing
`locate_chrome` + `run_child_with_timeout` one-shot runner discipline
(never `Command::output()`; throwaway `--user-data-dir`; kill+reap):

1. Write a wrapper HTML page to a temp dir: the official blockquote snippet
   for the URL + `platform.x.com/widgets.js`, constrained to the standard
   550 CSS px embed width.
2. Run headless Chrome against it. Completion is signaled by the page
   itself: the wrapper binds `twttr.events.bind('rendered', …)` and
   publishes the rendered widget height. Capture uses
   `--force-device-scale-factor=2` for Retina-quality output. If a
   measure-then-capture two-pass is needed to size the window to the widget
   height, both passes go through the one-shot runner; no new dependencies
   (no CDP client, no image-processing crate) are introduced.
3. A widget that never reaches `rendered` (deleted/private tweet, X blocking
   headless, no network) is a **hard line-numbered build error** naming the
   URL — never a silent blank image. The timeout follows the existing
   Chrome one-shot budget.

The `--virtual-time-budget` + `image.decode()` Linux hang pitfall applies:
the wrapper page must wait on load/error events only.

### Cache

- Location: `.peitho/embeds-cache/<key>.png` (sibling of
  `code-images-cache`, same per-deck resolution, same atomic
  temp+rename write).
- Key: SHA-256 with a domain-separation prefix
  (`\0peitho-builtin-embed\0`) over the normalized URL plus every rendering
  parameter that affects output (embed width, scale factor, theme). The
  crate version is deliberately **not** in the key — busting the cache on
  every peitho upgrade would force a network refetch, the opposite of the
  code_images precedent where the renderer itself lives in the binary.
  Bumping a rendering parameter is the cache-busting mechanism.
- Self-heal: a cached file is a hit only if it is a nonempty PNG (magic
  check). Anything else is a miss, mirroring `valid_cached_svg`.
- No TTL, no auto-refresh (matches code_images). A tweet is snapshotted as
  of the first build — for a presentation this is a feature (the quoted
  content stays as presented even if later deleted). Refresh by deleting
  the cache file; `.peitho/embeds-cache/` never enters `dist/`.
- Offline or Chrome-less builds succeed on cache hits and fail with a
  line-numbered error (naming the cache path and the refresh story) on
  misses.

### Downstream phases: zero new surface

The transform emits `SourceFragment::image` with a `RawImagePath` built by a
new `pub(crate)` constructor (`from_embeds_cache`, mirroring
`from_code_images_cache`). From there everything is the existing image
path: mapping routes it to the image slot, `ImageResolver` hashes and copies
it into `assets/`, render emits a plain `<img>`, PDF flatten and the publish
contamination check gain no new cases. No new `FragmentKind`, no
`bindings/` drift, no shell changes. Reveal spans survive the rebuild the
same way code_images fragments do.

Display sizing is governed by layout CSS like any other image-slot content;
the 2x PNG carries its intrinsic pixel size and the slot's existing
constraints scale it.

## Edge cases and constraints

- Multiple embeds in a deck fetch serially (the transform is a synchronous
  loop, like code_images); each has its own timeout, cache hits skip Chrome
  entirely.
- Widget appearance is the X default (light). A theme option
  (`data-theme=dark`) is future option-line syntax, out of scope for v1.
- Screenshot output is not pixel-deterministic across platforms (font
  rasterization) — same accepted property as external code_images
  commands; the cache makes each machine self-consistent.
- **Accepted residual risk**: the capture pass reloads the wrapper in a
  fresh profile, and Chrome's `--screenshot` fires at virtual-time-budget
  expiry whether or not the widget re-rendered — a transient X failure
  between the measure and capture passes can cache a valid-but-blank PNG.
  Accepted because virtual time waits on pending network (in practice the
  widget settles before expiry), and closing it would require a
  pixel-inspection dependency. Symptom: a blank/partial embed on the
  slide; remedy: delete the named cache file to refresh.
- Text in the slide is not selectable (it is an image). Accepted: fidelity
  was chosen over selectability; the v2 card mode is the selectable option.
- `code_images.embed:` frontmatter continues to mean "external SVG command
  for ```embed blocks" (backward compatible override, same as mermaid/math).

## Follow-ups (separate issues, not in v1)

1. Theme-matched self-rendered card from oEmbed data, opt-in per block —
   selectable text, no Chrome dependency.
2. Generic oEmbed-discovery providers (YouTube, Mastodon, …) behind the
   same URL dispatch.
3. Dark-theme / width option lines on the `embed` block.
