# PDF export: require evidence that flattening ran (2026-08-05, Issue #402)

## Problem

`pdf_flatten.js` finishes at the end of an async chain that is not ordered
against Chrome's `--print-to-pdf` completion. When the print wins, export still
**succeeds** and writes a PDF in which gradients and box-shadows were never
flattened. The user gets a silently degraded file, not an error.

Same root cause as the lint flake fixed in #396, different failure mode — which
is why it never surfaced as a CI failure.

### Why the predicates diverge

| Variant | `is_ready_after_successful_exit` | Losing the race means |
| --- | --- | --- |
| `LintResultLogged` | delegates to `is_ready` (signal required) | hard error |
| `EmbedMeasured` | `embed_dump_has_complete_title(stdout)` | hard error |
| `PngWritten` | delegates to `is_ready` (signal required) | hard error |
| `PdfWritten` | `output_file_is_nonempty(output_path)` | **success** |

`PdfWritten` is the only variant that *relaxes* its check after exit. Every other
variant demands the same evidence it demanded while running. That asymmetry is
the bug: the file existing proves Chrome printed, not that peitho's flattening
ran first.

`pdf_flatten.js` records its outcome in `data-peitho-pdf-flattened` /
`data-peitho-pdf-shadow-flattened`, but nothing on the Rust side reads those
attributes, and the script logs to console only on *failure*. There is no seam
where "flattening actually completed" is observable.

## Why not order it with `beforeprint`

The lint fix (#396) published from a `beforeprint` listener, which is ordered
before the PDF bytes are written. That does not transfer here. Measured against
real Chrome with the export flags:

| Work inside `beforeprint` | Runs before the print? |
| --- | --- |
| synchronous statements | yes |
| microtask (`Promise.resolve().then`) | yes |
| `setTimeout(…, 0)` | **no** |
| `image.onload` after `toDataURL` | **no** |

`beforeprint` does not await anything. Flattening rasterizes every gradient and
shadow target in sequence and waits on image loads, so it cannot be hoisted into
that hook without being made fully synchronous — a large rewrite of working,
measured-correct code, which would still leave the outcome unverified.

Impact is not cosmetic (both already recorded in CLAUDE.md, both measured):
Chrome emits blurred box-shadows as `/S /Luminosity` soft masks that Quartz
renders as **hard black rectangles**, and Type 4 shadings render pathologically
slowly and incorrectly in Preview.app.

## Approach

Make the outcome observable and require it, bringing `PdfWritten` in line with
every other variant.

1. `pdf_flatten.js` logs a completion signal (`PEITHO_PDF_FLATTEN_DONE`) once the
   chain settles — on both the success path and the top-level `catch`, since a
   flatten failure already degrades gracefully per-target by design and must stay
   a completed export, not a hang.
2. `ChromeCompletion::PdfWritten` scans stderr for that signal, exactly as
   `LintResultLogged` does, and `is_ready_after_successful_exit` stops relaxing to
   a bare file check.

This restores the invariant at the one upstream seam rather than guarding at
consumer sites, and it removes the special case rather than adding another.

**Behaviour change, accepted by the author (2026-08-05):** exports that
previously produced a silently unflattened PDF now fail with a diagnostic. A
silently wrong file is worse than a loud failure.

### Why the signal alone is not sufficient (found in review, 2026-08-05)

The completion signal proves flattening *happened*, not that it happened
*before the print*. `is_ready` ANDs two independently-latched conditions:

```rust
output_file_is_nonempty(output_path) && state.signal_seen
```

Neither records *when* it became true, so both orderings satisfy it. Measured
with a fake Chrome that writes the PDF and only then signals a second later —
the exact #402 failure ordering — `run_one_shot_chrome` returns `Ok`. The
export succeeds and the file on disk is the unflattened one.

This is not fixable by reordering the predicate: by the time the sentinel
reaches peitho's stderr the PDF bytes are already written and Chrome is exiting.
There is no point at which peitho can veto the print. The signal therefore
catches only "flattening never ran at all" (missing script, JS parse error,
never-settling chain) — a real class, but not the one the issue targets.

### Artifact verification: implemented, measured, and rejected (2026-08-05)

Scanning the produced PDF for the structures flattening removes was tried,
because unlike the sentinel it inspects the actual bytes rather than a promise
about them. On identical content printed with and without `pdf_flatten.js` the
signal looked clean:

| Marker | Flattened | Unflattened |
| --- | --- | --- |
| `/Shading` | 0 | 2 |
| `/PatternType` | 0 | 1 |
| `/Luminosity` | 0 | 1 |

**It does not work, and the approach is abandoned.** "Marker present" does not
mean "flattening lost the race" — it means "an unflattened structure exists",
which is a different proposition. `pdf_flatten.js` *deliberately declines* to
flatten several shapes, and each one still emits a marker. The skip conditions
live in `collectGradientTargets` and `collectShadowTargets`: `background-clip:
text`, `background-attachment: fixed`, non-normal `background-blend-mode`,
`url()` layers, `getClientRects().length !== 1`, an ancestor `transform`,
non-zero box insets, and oversize canvases.

Measured against real decks with the check in place:

- `examples/peitho-tour` — the project's flagship demo — was rejected on
  `/PatternType`. That marker was `/PatternType 1` (a *tiling* pattern), not
  `/PatternType 2` (a shading pattern). The pre-change `main` build produces the
  same two occurrences, so it was never related to flattening at all.
- A deck whose only unusual CSS is `background-clip: text` is rejected on
  `/Shading`, with flattening working exactly as designed.
- A `box-shadow` under a `transform` is rejected on `/Luminosity`, and the
  rendered shadow is correctly soft — the Quartz black-rectangle pathology the
  flattener exists to prevent did not occur.
- Worst: a gradient inside an SVG emitted by `code_images` can **never** pass.
  `pdf_flatten.js` only walks `getComputedStyle` over DOM elements and cannot
  see inside an `<img>`-referenced SVG. No timing resolves this.

Dropping `/PatternType` from the marker set does not rescue it; every case above
still fails. The failure also fires *after* `--print-to-pdf` has written the
file, so a rejected export leaves a complete, correct PDF at the user's `-o`
path next to an error claiming it is broken.

Rejecting valid decks is a worse regression than the silent degradation this
issue set out to fix: a user loses the ability to export at all, and the trigger
is ordinary CSS. So this PR ships the sentinel only.

### What this PR does and does not close

**Closes**: flattening that never ran at all — a missing or unparseable script,
or a chain that never settles. Previously silent; now a loud, named failure.

**Does not close**: flattening that ran but lost the race to the print. The
sentinel cannot detect it (proven above), and artifact inspection cannot
distinguish it from legitimate non-flattening (proven above). Tracked as
Issue #408 rather than shipped broken.

### Three lenses

- **Long-term**: `PdfWritten` stops being the one variant with a weaker
  post-exit check, so a future variant copying the pattern copies the strict one.
- **Type safety**: the evidence requirement moves into the predicate the runner
  already consults, so no caller has to remember to verify separately.
- **Root cause**: partial. The seam "completion is unverified" is closed for the
  never-ran class. The ordering gap is genuinely unreachable from either
  mechanism available here, and is documented rather than papered over.

## Tasks

1. `crates/peitho-core/src/pdf_flatten.js` — emit the completion signal from a
   single seam covering both the success and top-level-catch paths.
2. `crates/peitho-core/src/render.rs` — assert the script emits the signal, and
   keeps not containing the raw sentinel (mirrors the lint script's tests).
3. `crates/peitho/src/main.rs` — `PdfWritten` requires the signal in `is_ready`
   and no longer relaxes in `is_ready_after_successful_exit`; update the existing
   `PdfWritten` unit tests, which currently assert the relaxed behaviour.
4. Export paths that print without `pdf_flatten.js` (if any) must be checked — a
   page that never loads the script would now never signal and would hang until
   timeout. This is the main regression risk; enumerate every `PdfWritten` call
   site and confirm each renders `pdf.html` with the script embedded.
   *Done: `run_chrome_print` is the only production site and always renders
   `render_pdf_document`, which embeds `PDF_FLATTEN_JS` unconditionally. Lint
   uses `LintResultLogged`, so routing it through the shared `chrome_print_args`
   does not subject it to this predicate.*
5. `crates/peitho-core/src/pdf_flatten.js` — bound `waitForWindowLoad` the way
   `lint_measure.js` already does (commit `ca87934`). An unbounded wait meant no
   signal, which under the new predicate is a hard export failure rather than the
   old silent degradation. *Done.*
6. `crates/peitho/src/main.rs` — `PdfWritten`'s `description`/`retry_help`/
   `timeout_help` must name flattening, not "PDF output"; the old strings
   described the removed predicate and contradicted the stderr shown alongside
   them. *Done.*
7. ~~Verify the produced PDF carries no unflattened markers before reporting
   success.~~ *Implemented, measured against every deck in `examples/`, and
   reverted — it rejects valid decks. See "Artifact verification" above.*

## Verification

- Full gate list from CLAUDE.md.
- `peitho export` end-to-end on a deck with gradients and shadows: confirm it
  still succeeds and the PDF is actually flattened.
- Negative check: stub the signal out of the script and confirm export now fails
  loudly instead of producing a degraded PDF.
- macOS cannot reproduce the original race (virtual time resolves the chain
  early); the Linux `e2e` job is the real check for the timing behaviour.
