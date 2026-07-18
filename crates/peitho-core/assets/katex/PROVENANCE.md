# KaTeX asset provenance

- **KaTeX version**: 0.16.25
- **Source**: https://cdn.jsdelivr.net/npm/katex@0.16.25/dist/
  (`katex.min.css`, `fonts/*.woff2` — woff2 only; woff/ttf legacy
  fallbacks are intentionally not vendored)
- **License**: MIT (see `LICENSE`, fetched from
  https://github.com/KaTeX/KaTeX/blob/v0.16.25/LICENSE)
- **Retrieved**: 2026-07-18
- **Why 0.16.25**: the `katex-rs` crate (0.2.4) tracks upstream KaTeX
  commit `785315c0f630f65347cac14b3ec72629cfe7631e`, whose
  `package.json` declares version 0.16.25. CSS class names and font
  metrics are coupled to the emitting renderer, so the vendored assets
  must match that version, not the latest release.
- **Update procedure**: when bumping the `katex-rs` dependency, read
  the tracked KaTeX commit from its README, resolve the version via
  that commit's `package.json`, and re-vendor all files here from the
  matching npm dist. Update this file and the design record
  `docs/plans/2026-07-18-builtin-math.md`.
