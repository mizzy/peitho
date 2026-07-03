# Zero-config convention (auto-detect deck-adjacent layouts/, css/)

## Author decision (2026-07-03, Issue #17 policy)

For per-deck repo operation, we'll go first with **convention-over-configuration zero-config**. peitho.toml will be considered once a need for customization arises.

## Convention

- When `--layouts` is not specified, if `layouts/` exists in **the same directory as the deck file**, use it. Otherwise, use the built-in layouts
- When `--css` is not specified, likewise, if `css/` exists, use it. Otherwise, use the built-in base theme
- Explicit flags always take precedence over the convention
- If the convention directory exists but the target files are empty, this is a **build error** (if you created it, there must be intent — no silent fallback to the built-in)
- The reference point is the deck's location, not the current directory (so the result is the same no matter where you invoke the deck repo from)

With this, a typical deck repo just needs to place `deck.md` + `layouts/` + `css/`, and `peitho present deck.md` works.

## Verification

- Unit: resolution order of flag priority / convention detection / built-in if neither exists
- Integration: build a deck with adjacent layouts/+css/ without flags → the convention side is used. A deck without them → built-in
- E2E: build/present examples/keynote without flags (examples already has this structure from the previous PR)
- --watch: include the directory resolved via convention in the watch targets (resolved at startup; if a directory is newly created after startup, a watch restart is required — this is acceptable)
