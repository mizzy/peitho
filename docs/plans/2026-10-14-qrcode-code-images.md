# Built-in `qrcode` code images (Issue #705)

## Goal

A fenced `qrcode` block renders to a QR code SVG at build time and routes as
an ordinary image, exactly like built-in `mermaid`:

````md
```qrcode
https://decks.gosu.ke/sapporo-sre-1/
```
````

Today authors generate the SVG outside peitho (`qrencode`) and commit it; the
URL and the generating command drift apart.

## Author decisions (2026-10-14)

- **Image path, fixed colors.** The output is an SVG file referenced through
  `<img>` (the mermaid path), so it needs an `accepts="image"` slot. Page CSS
  cannot reach inside an `<img>` SVG, so the issue's `currentColor` idea does
  not apply; colors are fixed black modules on a white background with the
  standard quiet zone, which keeps the code scannable on dark slides. CSS
  `filter` remains possible for authors who want something else.
- **Body rules.** The encoded text is the block body with surrounding
  whitespace (including blank lines around the single line) trimmed. An empty
  (whitespace-only) body is a line-numbered build error. A trimmed body that
  still contains a line break (more than one line) is a line-numbered build
  error. Nothing is guessed or silently joined. An external
  `code_images.qrcode` override receives the body verbatim, like every other
  external renderer.
- **No fence options in v1.** Error correction is at least Medium (qrcodegen
  boosts it when a higher level fits the same symbol size); any fence info
  tail (`qrcode foo=bar`) is the existing unexpected-tail error, and a
  line-emphasis spec (`qrcode {1}`) is the existing "line emphasis is not
  supported on rendered code images" error, the same as `mermaid`. Options
  wait for measured demand.

## Design

- `CodeImagesConfig::renderer_for` stays the single resolver: bare `qrcode`
  resolves to a new `CodeImageRenderer::BuiltinQrCode`; an explicit
  `code_images.qrcode: <command>` frontmatter entry still wins and becomes
  `External`, like every other built-in. The parser's unknown-language check
  and the transform both go through it, so no tag-string carve-out exists.
- The transform narrows it into the existing `SvgCodeImageRenderer` (new
  variant `BuiltinQrCode`) so the output goes through the same seam as
  mermaid: validate → normalize → cache → `RawImagePath::from_code_images_cache`.
  `CodeImageOutputContext` becomes `{ExternalCommand, Builtin(BuiltinSvgRenderer)}`
  with `BuiltinSvgRenderer::{Mermaid, QrCode}` carrying each built-in's
  override help, so output errors name the built-in with no panic arm; the
  SVG renderer variant carries the validated text (`BuiltinQrCode(&str)`).
- Body validation happens before the cache lookup, so an invalid body is an
  error even if some cache file happens to exist.
- The cache key is domain-separated (`\0peitho-builtin-qrcode\0` + crate
  version + body), mirroring `builtin_mermaid_cache_key`.
- Encoding uses `qrcodegen` (already a workspace dependency, used by the
  remote QR in the CLI) with `QrCodeEcc::Medium`; data too long is a
  line-numbered error. The SVG declares the SVG namespace, carries absolute
  `width`/`height` and a matching `viewBox` (4-module quiet zone), a white
  background `<rect>`, one black `<path>` of the dark modules, and
  `shape-rendering="crispEdges"`.
- Alt text is `QR code: <body>` instead of the generic `diagram (<tag>)`, so a
  screen reader announces the encoded URL.
- Line emphasis on a `qrcode` block is already rejected by the
  `renderer_for` check in the parser.

## Tests (TDD)

- `renderer_for("qrcode")` → `BuiltinQrCode`; an explicit entry → `External`.
- A deck with a `qrcode` block builds into an image fragment whose cached SVG
  passes `check_image_svg`, decodes to the expected module grid size, and is
  deterministic.
- Empty body, whitespace-only body, and two-line body are line-numbered
  errors naming `qrcode`.
- A fence tail on `qrcode` is the unexpected-tail error.
- Over-capacity text is a line-numbered error.
- Alt text is `QR code: <body>`.

## Docs

- `site/content/guide/writing-decks.md` and `frontmatter.md` beside mermaid.
- `examples/code-images/deck.md` gains a `qrcode` slide.
- `CLAUDE.md` code_images bullet lists `BuiltinQrCode`.
