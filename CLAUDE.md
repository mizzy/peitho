# peitho

An HTML-native presentation tool with Markdown as the source of truth. The authoritative design reference is `docs/PEITHO_KICKOFF.md` (the kickoff spec). When in doubt about a design decision, check §18 "Undecided Items" in the spec; for new decisions not covered there, check with the author.

## The three pillars (invariants that must not be broken)

1. **Separation of content and design**: Content is Markdown, design is layout HTML+CSS. Do not mix them
2. **Git-manageable HTML/CSS layouts**: The layout itself is the schema (`<slot name accepts arity>`). Do not split the contract into a separate file
3. **Type-checked slot contracts and keyed overrides**: Slot excess/deficiency, type mismatches, broken references, and unassigned content are all build errors with line numbers + help. **Silent dropping is absolutely forbidden** (never let the parser swallow unknown structures with `_ => {}`)

Other invariants:
- typestate `Parsed→Mapped→Checked→Rendered`. Phase constructors are private within the crate. An unchecked deck cannot be passed to the renderer (pinned down by a compile_fail doctest)
- Multiple layouts use hybrid dispatch (type-driven approach from §18 adopted at the author's discretion, 2026-07-03): explicit `{"layout":"name"}` > unconditional if there's only one > unique structural match. Ambiguous or zero matches are never silently resolved — they are build errors. Slides carry their own Layout from Mapped onward, so no lookup-failure path is created downstream
- Syntax highlighting is done at build time with syntect (adopted 2026-07-03). Output is spans with `hl-*` classes, colors come from the theme CSS. Unknown language tags are a parse-time error with a line number (no tag means plain text)
- Single source for the contract: domain types are authoritative in peitho-core (Rust). TS types are generated into `bindings/*.ts` via ts-rs and committed. CI checks for drift
- §16 event contract: only the shell executes transitions. UI components only emit request events like `peitho:navigate`/`peitho:timercontrol`. The slide body itself has no knowledge of the shell's existence
- Do not mix the presentation shell or notes into distributed artifacts (dist/) (publish gatekeeps this with a contamination check)

## Structure

```
crates/peitho-core/   Contract & pipeline (parser/layout/mapping/check/render/theme/manifest/notes)
crates/peitho/        CLI (build/present/publish), server.rs (serving + /sync long polling), browser.rs, displays.rs
packages/peitho-present/  TS presentation shell (canvas/shell/controls/keyboard/sync/presenter)
bindings/             ts-rs generated TS types (committed)
layouts/ themes/ examples/  Shared layouts, base theme, samples (the default layouts/base.css/presentation shell dist/shell.js are embedded in the binary via include_str!. Used when no CLI flag is given. shell.js is a build artifact but, like bindings/, is committed + checked for CI drift). `--layouts`/`--css` can be a file or a directory (reads `*.html`/`*.css` in filename order). When not specified, auto-detects `layouts/`/`css/` **next to the deck** (zero-config convention, adopted per Issue #17; falls back to the built-in default if absent). CSS validation is uniform across all files: keyed selectors are checked against the slots of that slide's layout, and bare `.slot-*` are checked against the union of provided layouts
docs/plans/           Implementation plans for each milestone (history)
```

## Gates (all must pass before committing)

```
cargo test --workspace          # run 3 times in a row (past test-race incidents)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
git diff --exit-code bindings/  # contract drift
cd packages/peitho-present && npm run build && npm test && npm run typecheck
git diff --exit-code packages/peitho-present/dist/shell.js  # embedded shell drift (after npm run build)
```

Always verify UX changes end-to-end in a real browser/real display (jsdom cannot detect layout, flashing, or window behavior. Past incidents — a fully black screen, undelivered SSE, infinite rebuild loops — were only caught via E2E). For checking present, a fixed `--port` + `curl POST /sync` + `screencapture -x -D <n>` is handy.

## Pitfalls (facts confirmed by measurement — no need to re-investigate)

- **SSE does not work with tiny_http**: when data_length is None, bodies below a threshold are buffered until EOF, and the chunk encoder doesn't flush small chunks either. That's why /sync uses long polling (`GET /sync?seq=N`; a GET with no query returns the current seq immediately = join handshake; `POST /sync` sends `{index}|{close:true}`)
- **Flag handoff to an already-running Chrome instance only works via `--app`**: `--start-fullscreen`/`--window-position`/`--window-size` are ignored. To reliably apply them, launch a new process with a separate `--user-data-dir` (which is why slides/presenter use two instances, `~/.peitho/chrome-profile-{slides,presenter}`)
- **On macOS, Chrome's process lingers even after all windows are closed**: if the previous present instance is still holding the peitho profile, the next launch becomes a handoff and all placement flags are lost. That's why present terminates any lingering process before opening
- **Lingering Chrome processes must be terminated normally, not killed with SIGTERM**: SIGTERM registers as a crash to Chrome (`exit_type: Crashed`), triggering crash recovery on the next launch which restores the old session's windows/bounds. Using `NSRunningApplication.terminate` (in JXA this fires via **parenthesis-less** access, due to the bridge — confirmed by testing) terminates it as Normal. Note that `exit_type` always shows Crashed while running (it's written up front at launch, then flipped back to Normal on clean exit), so reading it while the process is active is meaningless. Restrict target pids to the Chrome main process (no `--type=`) from `ps` (`pgrep -f` also catches the shell itself if it contains the pattern)
- **Chrome's `--app` window position restoration breaks if the app name contains a dot**: the position is saved under `browser.app_window_placement` keyed by a URL-derived app name (host+path), but a dot in the name (as in `127.0.0.1` or `.html`) gets expanded as a pref path when written, causing a mismatch on read and preventing restoration (confirmed Chromium behavior, measured). That's why presenter opens at `http://localhost:<port>/presenter` (an extensionless route). Since the app name doesn't include the port, restoration still works even though the port changes each time
- **On the first `--app` launch with no placement flags, which display the window appears on is undetermined**: if it appears on the slides-side display, it can end up completely hidden behind the fullscreen Space (it won't even show up in CGWindowList's OnScreenOnly). That's why windowed mode only seeds `--window-position`+`--window-size` to center on the primary display when there's no saved placement or the saved position is off-screen; when there is a visible saved placement, it's left flag-free and Chrome's own restoration is trusted (the check is peitho reading the profile's Preferences). Chrome clamps partially off-screen saved bounds at launch
- **BroadcastChannel does not reach across different profiles**: that's why sync goes through the server (a deliberate extension from §15; the layering — DOM events bridged to a transport — remains unchanged)
- **A CLI-launched app window is closed via `window.close()`** (because it has only 1 history entry). Esc triggers `peitho:closerequest` → `{close:true}` broadcast to all windows → each closes itself → the server also unblocks and exits after a grace period
- **requestFullscreen/window.open require transient user activation**: awaiting a permission prompt in between invalidates it. In-browser window placement failed twice this way before the switch to CLI-driven placement (M8/M9/M10)
- NSScreen has a bottom-left origin. Chrome's `--window-position` uses top-left. The conversion lives in `displays.rs` (pure functions + tests against measured values)
- In vitest tests, always destroy/clean up the shell/listeners (listener contamination on the shared window causes multiple firings)
- `.peitho/present-cache/` is recreated every time (the adopted value from the §18 cache policy). `dist/slides/` is also cleared on every build (to prevent stale fragments from leaking into publish)

## Undecided — awaiting author's judgment (do not decide unilaterally)

- Markdown notation for speaker notes (the notes.json schema, TS types, and presenter display wiring are implemented, but always empty)
- Explicit fenced div slot notation `::: {slot=...}` (§18)
- peitho.toml (once the need for customization arises; currently operating on the zero-config convention) and peitho.gosu.ke deployment

Remaining tasks are registered in GitHub Issues. Before starting work, write a plan in `docs/plans/` and then implement.
