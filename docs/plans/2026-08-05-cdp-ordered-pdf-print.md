# PDF export: order the print after flattening via CDP (2026-08-05, Issue #408)

## Problem

`--print-to-pdf` prints when *Chrome* considers the page ready; nothing orders
that against the async flatten chain in `pdf_flatten.js`. Issue #402 (PR #409)
made "flattening never ran" a loud failure via a stderr sentinel, but proved by
measurement that "flattening ran and lost the race" is detectable by neither the
sentinel (bytes already on disk when it arrives) nor artifact-marker inspection
(the flattener deliberately skips shapes that leave markers, and cannot see
inside SVGs). The only shape that closes the gap is one that *orders* rather
than *observes*: do not ask Chrome to print until flattening has completed.

Issue #408 lists two directions. Three-lens evaluation:

| Lens | CDP-gated print | Skip reconciliation |
| --- | --- | --- |
| Root cause | orders the print itself | still observation; marker attribution provably impossible |
| Long-term | race class unrepresentable | carve-out per skip class; SVG blind spot permanent |
| Type safety | neutral | neutral |

All lenses select CDP gating, so per the pick-issue rules this proceeds without
a design question.

## Feasibility — measured (macOS Chrome, spike 2026-08-05)

A hand-rolled CDP session (the exact sequence the Rust code will perform)
against a page embedding the real `pdf_flatten.js`:

| Step | Result |
| --- | --- |
| `--remote-debugging-port=0` + `DevToolsActivePort` discovery | port readable in 378ms |
| `GET /json/list` over plain HTTP | page target + `webSocketDebuggerUrl` (frame by Content-Length, never by EOF — the spike's connection stayed open and an EOF read hung; the shipped code also sends `Connection: close` but still treats Content-Length as the only terminator) |
| Poll `Runtime.evaluate` for `data-peitho-pdf-flattened` | non-null after 132ms (value `2` = both targets flattened) |
| `Page.printToPDF` after readiness | valid `%PDF-`, 497KB, `/Shading` 0, `/Luminosity` 0, 3 rasterized images |
| `Browser.close` | Chrome exits 0, no lingering process |

`preferCSSPageSize: true` honors the existing `@page { size: WxH px; margin: 0 }`
in `pdf.html`; `displayHeaderFooter: false` replaces `--no-pdf-header-footer`;
`printBackground: true` is required for slide backgrounds.

## Design

### New export flow (`run_chrome_print`)

1. Spawn Chrome: `--headless=new --disable-gpu --no-sandbox
   --remote-debugging-port=0 --enable-logging=stderr --user-data-dir=<tmp>
   about:blank`. No `--print-to-pdf`, no `--virtual-time-budget`. Stderr stays
   piped for diagnostics on failure.
2. Wait for `<profile>/DevToolsActivePort` (first line = port), bounded.
3. `GET /json/list` on `127.0.0.1:<port>` (hand-rolled HTTP/1.1 over
   `TcpStream`; parse `Content-Length`, do not wait for close), take the
   `type == "page"` target's `webSocketDebuggerUrl`.
4. WebSocket via `tungstenite` (ws:// on loopback only — no TLS features).
   `Page.enable`, `Page.navigate` to the `pdf.html` file URL.
5. Poll `Runtime.evaluate` on
   `document.documentElement.getAttribute('data-peitho-pdf-flattened')` until
   non-null. The attribute is set on both the success path and the top-level
   catch of `flattenPdfArtifacts`, so it always appears when the script parses;
   a parse failure or never-settling chain hits the deadline → loud error
   naming flattening readiness. **The attribute written since the flatten
   feature landed finally has its reader.**
6. Only then `Page.printToPDF { printBackground, preferCSSPageSize,
   displayHeaderFooter: false }`; decode base64, write the output file.
7. `Browser.close`, wait bounded, kill+reap if it lingers (throwaway profile —
   the SIGTERM crash-recovery pitfall applies only to persistent profiles).

Every phase up to and including the PDF write shares one overall deadline
(`CHROME_ONE_SHOT_TIMEOUT`); any failure on that stretch kills and reaps the
child and surfaces captured stderr. Once the PDF is written the deliverable is
complete, so shutdown runs on its **own short grace window**
(`POST_PRINT_SHUTDOWN_GRACE`) as best-effort cleanup — `Browser.close`, then
kill+reap — and can never fail the export (the Chrome-149 GoogleUpdater
lingering pitfall must not turn a finished export into an error). There is
**no fallback** to the racy `--print-to-pdf` path — a silent fallback would be
the #402 bug reintroduced with extra steps.

### What this removes

- `ChromeCompletion::PdfWritten` and its tests: export no longer flows through
  the one-shot completion-predicate runner, and export was that variant's only
  production caller. Lint (`LintResultLogged`) and embeds (`EmbedMeasured`/
  `PngWritten`) keep the runner unchanged.
- Export's dependence on `--virtual-time-budget`, and with it the whole
  virtual-time hazard class (`image.decode()` hang, virtual time outrunning
  real resource work) *for export*. Lint keeps virtual time and its bounded
  waits; `pdf_flatten.js` keeps its own bounds (they become real-time bounds
  that only bite when fonts/load genuinely stall) but its virtual-time comments
  must be updated.
- The `PEITHO_PDF_FLATTEN_DONE` stderr sentinel stays in the script as a debug
  aid for kept workspaces (and its no-raw-sentinel tests stay), but the
  readiness contract is the attribute.

### Accepted behaviour changes

- CSS animations: the old budget advanced virtual time 10s deterministically
  before printing; now the print happens at flatten completion (sub-second,
  real time), so an animating deck prints an earlier frame. A PDF is a single
  frame either way.
- `chrome_print_args` becomes lint-only; export gets its own argv builder.

### Dependency

`tungstenite` (sync, ws:// only, no TLS feature) — hand-rolling RFC 6455
client framing is protocol code peitho should not own. The `/json/list` HTTP
GET is trivial enough to hand-roll over `TcpStream`.

Found in review: tungstenite's default caps (16 MiB/frame, 64 MiB/message)
must be lifted — `Page.printToPDF` returns the whole PDF as base64 in one
message, and Chromium's DevTools server sends one message as one frame, so an
image-heavy deck fails at the transport where `--print-to-pdf` had no limit.
`cdp_websocket_config` is the single config source `connect` must route
through, pinned by test; measured end-to-end with a 68.7 MiB PDF (91.6 MiB
base64 — over both default caps).

Also found in review, both landed with tests: the `/json/list` fetch retries
within the deadline while the page target is not yet registered (port-file
appearance is not ordered against target registration), and the port-file wait
consults child liveness so a Chrome that dies at startup fails in well under a
second instead of sleeping out the full deadline. Transient navigate-commit
CDP errors (`Execution context was destroyed` / `Cannot find default execution
context`, code -32000) are retried by the readiness poll only; every other
error stays fatal.

## Tasks (TDD)

1. `crates/peitho/src/cdp.rs` — minimal sync CDP client: DevToolsActivePort
   wait/parse, `/json/list` fetch/parse, ws message send/recv with id
   correlation, typed wrappers for the five calls used. Pure parsing
   (port file, target list, response framing) unit-tested without Chrome.
2. `run_chrome_print` rewired to the flow above; timeout/kill/reap behaviour
   unit-tested with a fake chrome script where feasible (port-file never
   appears → deadline error; process reaped).
3. Remove `ChromeCompletion::PdfWritten` + its tests; keep the runner intact
   for lint/embed. Export error messages name flattening readiness and point
   at `pdf.html` in the kept workspace.
4. Update stale virtual-time comments in `pdf_flatten.js` and the
   export-related CLAUDE.md pitfalls (done by Opus, in-PR). Found in review:
   the in-page readiness bounds also had to *change*, not just be re-commented
   — `WINDOW_LOAD_TIMEOUT_MS`/`FONT_READY_TIMEOUT_MS` rose 2000→20000. The 2s
   values were sized to lose gracefully under virtual time; with the print now
   waiting on the page, short bounds became a new hazard (printing before
   slow-loading fonts/images settle) while long ones are safe — the Rust
   deadline is the true cap, and the bounds only guarantee eventual settling.

## Verification

- Full gate list from CLAUDE.md.
- Real-Chrome e2e: all existing `export_pdf` `--ignored` tests, plus the full
  `examples/` sweep (19 decks) — every deck must export, including
  `background-clip: text` content and `code_images` SVGs.
- Negative checks with real Chrome: stub the flatten script so the attribute
  never appears → export fails loudly naming flattening readiness, no PDF
  claimed as success; kill-path leaves no lingering Chrome.
- Multi-slide pagination via `peitho-tour` (page-break handling under
  `Page.printToPDF`).
- macOS validates the mechanics; the Linux `e2e` job validates the environment
  where the race actually fired.
