# URL deep-link to a specific slide (Issue #115)

## Motivation

The published deck viewer always starts at slide 1. There is no way to
link directly to slide N (SpeakerDeck-style `?slide=10`), which hurts
both authoring (sharing a specific point during review) and reading
(someone linking to "the slide where you said X").

The runtime already jumps to any index via `showSlide(n)` — it just is
not wired to the URL.

## Scope

**Published-deck runtime only.** The change lives entirely inside the
inline `<script>` block of `render_distribution_index()` in
`crates/peitho-core/src/render.rs`, which is what ships to `dist/`.

**Explicitly out of scope:**

- `render_present_index()` / `render_presenter_index()`. Those are used
  only by `peitho present`. Navigation there is owned by
  `packages/peitho-present` and the sync bridge; the server holds the
  authoritative `index` and every `/sync` response carries it. Writing
  `?slide=N` there would race the server, and it is not what the Issue
  asks for.
- Slide-key deep links (`?slide=arch-1`). The Issue calls this
  nice-to-have, not required for v1.
- Any change to `manifest.json` or the build pipeline. Runtime-only.

## Design

URL contract (matches SpeakerDeck):

- Primary: `?slide=N` (1-based; internal `currentIndex` stays 0-based).
- Fallbacks accepted on load: `#slide=N`, `#N`.
- On invalid / OOB / non-integer: clamp to `[1, slides.length]`; if
  totally unparseable, start at 1.

Behavior:

1. **On load**, before calling `showSlide`, read the desired index
   from the URL (primary → hash `slide=` → bare `#N`) and pass
   `desiredIndex - 1` to `showSlide`. `showSlide` already clamps, so
   we only need to parse.
2. **On every `showSlide(index)`**, immediately after `currentIndex`
   is set, call the URL writer. It reads
   `URLSearchParams(location.search)`, sets `slide`, and rebuilds the
   URL as `location.pathname + '?' + params.toString() + location.hash`
   so non-slide query params and the fragment are preserved.
   Doing it inside `showSlide` (rather than at each `navigate`
   caller) is the *single point* of URL sync: keyboard, click, and
   any future caller get it for free — no "remember to also update
   the URL" convention leaks to callers. This is the root-cause /
   long-term-view choice.
3. **On `popstate`**, re-read the URL and call `showSlide(index - 1)`.
   Because we only ever `replaceState`, popstate rarely fires inside
   the deck (there is no per-slide history entry to pop to); it is
   installed as a belt-and-suspenders for browser session restore,
   manual URL bar edits, or scripts that mutate history externally.

Compatibility with existing behavior:

- `exitToIndex()` (Escape) still uses `history.back()`. Because we
  only ever `replaceState` (never `pushState`), history depth is
  unchanged and Escape still leaves the deck. No change needed.
- Referrer-scoped back / `location.assign('/')` fallback is unchanged.

## Non-behavior

- No `pushState`. Ever. `pushState` per slide would trap the browser
  Back button inside the deck — a known anti-pattern the Issue calls
  out.
- The runtime does not emit `#slide=N`, but any user-authored fragment
  (e.g. `#note`) is preserved on subsequent `replaceState` writes.
- Do not react to `hashchange`. The hash form (`#slide=N`, `#N`) is a
  load-time fallback only; the query is canonical, and
  `readSlideIndexFromUrl` prioritizes query over hash on any re-read.
  Reacting to `hashchange` would be a no-op (or worse, misleading)
  because the query would still win.
- Do not touch `render_present_index()` or `render_presenter_index()`.
- No new manifest fields, no new module files. The runtime is small
  and self-contained; a helper function inside the inline script is
  fine.

## Test plan

Rust side (same shape as the existing `distribution_index_*` tests in
`render.rs`):

- `distribution_index_reads_slide_query_on_load` — asserts the
  runtime contains a call to parse `?slide=` (e.g. via
  `URLSearchParams` on `location.search`) and passes the parsed index
  to `showSlide`.
- `distribution_index_supports_hash_fallback` — asserts hash parsing
  (`#slide=N` and bare `#N`) is present.
- `distribution_index_updates_url_on_slide_change` — asserts
  `history.replaceState` is called with a query starting with
  `?slide=` from inside `showSlide`.
- `distribution_index_never_uses_pushstate` — asserts
  `history.pushState` does **not** appear anywhere in the runtime
  (guard against regressions).
- `distribution_index_handles_popstate` — asserts a `popstate`
  listener is installed.
- `distribution_index_preserves_query_and_hash_when_writing_slide` —
  asserts the writer preserves the path, non-slide query params, and
  fragment while setting `slide`.
- `distribution_index_url_deep_link_leaves_present_and_presenter_alone`
  — asserts `render_present_index()` and `render_presenter_index()`
  contain no `?slide=` / `URLSearchParams` markers (invariant guard).

TS side (`packages/peitho-present`): no code change, no new tests.

E2E (real browser, per the CLAUDE.md rule that jsdom cannot detect
layout / navigation behavior):

1. `peitho build` a small deck; serve `dist/` with any static server.
2. Open `?slide=3` — asserts the 3rd slide is displayed on load.
3. Arrow-right — asserts the URL becomes `?slide=4`.
4. Browser Back — asserts navigation leaves the deck (not stepping
   back to `?slide=3`).
5. Open `#slide=2` — asserts fallback parsing.
6. Open `?slide=999` — asserts clamp to last slide.
7. Open `?slide=abc` — asserts clamp to slide 1.

## Downstream

Once merged and tagged, `mizzy/decks` bumps `PEITHO_VERSION` in
`.github/workflows/deploy.yml` to pick this up (per Issue #115).
