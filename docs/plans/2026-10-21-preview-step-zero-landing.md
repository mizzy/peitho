# Preview step-zero direct landings

Date: 2026-10-21  
Issue: #729

This record supersedes only the preview direct-landing and missing/invalid
saved-step rules in `2026-10-20-preview-reveal-steps.md`.

## Goal

Make every direct landing on a slide in preview single mode start at reveal
step 0, so an author sees the same unrevealed starting state before navigating
the slide's sequence. Preserve the existing horizontal step-walking behavior,
fully revealed grid surfaces, and restored valid steps.

## Rule

- `resolveDirectIndex(index, destinationMode)` is the single authority for a
  direct landing: a `single` destination resolves to step 0, while a `grid`
  destination resolves to the slide's reveal-step count.
- ArrowUp/ArrowDown, filmstrip clicks, grid opens, Home/End, direct index/key
  navigation, and leaving grid are direct single-mode landings and therefore
  start at step 0. Clicking the current slide's filmstrip thumbnail is also a
  direct landing and resets it to step 0.
- Grid-mode navigation and entering grid stay fully revealed. Entering grid
  from a partial step continues to publish the full step through
  `peitho:stepchange`; grid tiles and filmstrip thumbnails remain final-state
  renderings, and the grid attribute reset is unchanged.
- ArrowRight/PageDown and ArrowLeft/PageUp continue through shared
  `resolveStepTarget`; in particular, moving backward into the previous slide
  still lands fully revealed.
- A valid saved single-mode step is clamped and restored. A missing or invalid
  saved single-mode step restores at step 0. Grid restoration remains fully
  revealed.
- Because hidden later-step content is not an interactive target, text revealed
  by a later step cannot be clicked for inline editing until the author steps
  to it.

## Tasks

1. Update the existing preview navigation and restoration Vitest cases to
   assert step-zero single-mode direct landings, and run them before the code
   change to record the expected failures. Keep the grid-entry full-step event
   assertion.
2. Pass the destination mode explicitly through every `resolveDirectIndex`
   caller in `preview.ts`, return step 0 for `single` and the reveal-step count
   for `grid`, and make missing or invalid restored single-mode steps start at
   step 0.
3. Update the preview behavior documentation in `CLAUDE.md`, `README.md`, the
   guide, and relevant examples, while leaving distribution-viewer direct-jump
   semantics unchanged.
4. Rebuild `dist/preview.js`, run the complete `peitho-present` Vitest suite and
   TypeScript typecheck, and inspect the final diff. Then run the real-browser
   E2E with `peitho preview examples/incremental-reveal/deck.md`: verify that a
   grid open, ArrowUp/ArrowDown, Home/End, and a filmstrip click land at step 0,
   while ArrowLeft into the previous slide lands fully revealed.
