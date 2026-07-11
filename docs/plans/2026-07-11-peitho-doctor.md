# `peitho doctor` — diagnose Chrome, displays, embedded shells, and assets

Issue: #244. Adds a CLI subcommand that runs peitho's runtime dependencies
through a battery of checks and reports pass/warn/fail with remediation
hints for each, so a broken `present` / `preview` / `export pdf` session
can be diagnosed without correlating error messages against the § Pitfalls
lore in CLAUDE.md.

## Design decisions

- **Positional arg**: `peitho doctor [<deck.md>]`, defaulting to
  `deck.md` when omitted. If the deck file does not exist, doctor runs
  environment-only checks (Chrome, displays, embedded shells present).
  When the deck file exists, it adds deck-specific asset resolution checks.
  Consistent with the shape of `peitho layouts` (Issue #243 / PR #247).
- **Exit code**: non-zero (2) on any `fail`. `warn` does not fail the
  exit code (so single-display laptops still exit 0). This matches the
  precedent of `peitho layouts --explain` returning 2 for hard errors.
- **Output**: one line per check, human-first by default with ANSI color
  when `stdout` is a TTY (auto-detected via `std::io::IsTerminal`),
  `--json` for programmatic use. Categories act as section headers in
  human mode.
- **Categories** (fixed order for deterministic output):
  1. `chrome` — binary discovery + persistent profiles
  2. `displays` — enumeration + multi-display availability (macOS only)
  3. `embedded shells` — bundled `shell.js` / `preview.js` present at
     compile time (satisfied by construction of the binary itself, but
     surfaced so the user sees they exist and how big they are)
  4. `assets` — deck-specific; only present when the deck file exists
- **Statuses**: `pass` / `warn` / `fail`. Rules:
  - `warn` = things that reduce peitho functionality but do not break
    the golden path (e.g., only one display, no Chrome on Linux path
    means present-mode will not work but build/preview will).
  - `fail` = things that break the golden path (Chrome cannot be
    located when required for `export pdf` / `present`; explicit
    frontmatter asset path missing).
- **Deck-mode does not require Chrome**: a deck with all-explicit
  assets should be fully checkable even on a headless CI Linux box
  without Chrome. Chrome discovery is still surfaced but stays a
  warn/fail per the category rules, not a hard abort.

## Non-goals

- **No `bindings/` drift check** and **no committed-shell drift check**.
  Both require the peitho source tree; the shipped binary sits outside
  it. These are CI concerns (already covered by `git diff --exit-code`
  in the § Gates section) rather than a runtime user surface.
- **No "Chrome version known-good" check**. The known-good version
  window drifts continually and encoding it in the binary would rot
  faster than the pitfall it warns about. If a specific version becomes
  actively broken, a follow-up can add a fail on that exact version;
  today, reporting the discovered version and letting the user
  cross-reference is enough.
- **No "lingering Chrome process" check**. This is inherently a
  `present`-time concern (peitho already terminates lingering processes
  before opening); running it from `doctor` would be a snapshot that
  goes stale the moment the user launches Chrome. Left to `present`.
- **No `--json` schema stability guarantees** in this PR. The shape is
  documented in the plan and mirrored in a snapshot test; a future
  contract commitment can lock it down.
- **No Linux display enumeration**. Issue #22 is deferred by author
  decision (memory: `issue-22-linux-deferred`); doctor reports
  "not implemented on non-macOS" as an informational skip, not a fail.

## User surface

### `peitho doctor [<deck.md>] [--json]`

Human-readable output:

```
chrome
  ✓ binary-discovered: /Applications/Google Chrome.app/Contents/MacOS/Google Chrome
  ✓ profiles-writable: /Users/alice/.peitho
displays
  ✓ enumeration: 2 display(s) found
embedded shells
  ✓ present-shell: 57769 bytes (sha256: b059b6eee546)
  ✓ preview-shell: 27787 bytes (sha256: c7d6c317f284)

5 passed, 0 warned, 0 failed
```

With `<deck.md>`, an `assets` section is appended:

```
assets
  ✓ layouts: deck-adjacent ./layouts
  ✓ css: built-in
  ✗ syntaxes: syntaxes path does not exist: /Users/alice/deck/syntaxes/missing
      help: fix the syntaxes: frontmatter value, or remove the key
  ✓ fonts: built-in
```

`--json` shape:

```json
{
  "checks": [
    { "category": "chrome", "name": "binary-discovered", "status": "pass",
      "message": "/Applications/Google Chrome.app/...", "help": null },
    { "category": "chrome", "name": "profiles-writable", "status": "pass",
      "message": "/Users/alice/.peitho", "help": null },
    { "category": "displays", "name": "enumeration", "status": "pass",
      "message": "2 display(s) found",
      "details": [
        { "primary": true, "x": 0, "y": 0, "width": 1920, "height": 1080 },
        { "primary": false, "x": 1920, "y": 0, "width": 2560, "height": 1440 }
      ]
    },
    { "category": "embedded-shells", "name": "present-shell", "status": "pass",
      "message": "356532 bytes", "help": null },
    { "category": "embedded-shells", "name": "preview-shell", "status": "pass",
      "message": "176723 bytes", "help": null }
  ],
  "summary": { "pass": 5, "warn": 0, "fail": 0 }
}
```

- One JSON object per check, `null` for absent fields (no
  `serde(skip_serializing_if)`), so schema shape is stable.
- Exit codes:
  - `0` — everything passed or warned
  - `2` — any check failed (mirrors `layouts --explain`)

## Check catalog (implementation-level)

### chrome
- `binary-discovered`: reuse `locate_chrome_with_env` in `main.rs`. If
  it succeeds, `pass` with the discovered path. If it fails, `fail`
  with the existing help string (`install Google Chrome or ...`). The
  `PEITHO_CHROME_PATH` env var is surfaced in the message when set.
- `profiles-writable`: reuse `chrome_profiles_from_home` in
  `browser.rs`. If `$HOME` is unset, `warn` (peitho would still work
  for `build` / `preview` but `present` needs profiles). If the
  `~/.peitho` root cannot be created (permission), `fail`. Actually
  creating the profile subdirectories is deferred to `present`; doctor
  only checks that the root is writable.

### displays
- `enumeration` (macOS only): call `detect_presentation_layout` via the
  same osascript path, parse the display list, report count and
  bounds. If enumeration fails, `warn` with help (`osascript may not be
  installed, or NSScreen access may be denied`).
- On non-macOS: emit one `skip` status (a new fourth status? or
  degrade to `warn`?). To keep the model simple, use `warn` with a
  message "not implemented on this platform (see Issue #22)".

### embedded shells
- Both `BUILTIN_SHELL_JS` and `BUILTIN_PREVIEW_JS` are `include_str!`d
  at compile time, so their presence is compile-guaranteed. Report
  their byte length and a short SHA-256 prefix (first 12 hex chars)
  so a user comparing installs can see they match. This is the check
  that is closest to a status-of-the-installed-binary readout.

### assets (deck mode only)
- For each of `layouts` / `css` / `syntaxes` / `fonts`: call
  `resolve_assets` from `asset_resolution.rs`. Report the resolved
  `Provenance` and path.
- Explicit paths that fail existence are surfaced by `resolve_assets`
  itself as a build error today; doctor should not abort on the first
  failure but continue to report the remaining assets. Split the
  resolution into per-asset calls in doctor (calling
  `resolve_asset(...)` directly — currently `pub(crate)`, needs to be
  exposed) so that one failing asset only fails its own check.

**Implementation note on scope**: exposing `resolve_asset` per-key is a
minor change and matches the doctor-mode requirement (independent
per-asset results). Alternative — running `resolve_assets` and
catching its first-error return — would only surface the first
failure and hide the rest. The per-key exposure is the root-cause
factoring.

## Code structure

New file: `crates/peitho/src/doctor.rs` (or a submodule of `main.rs` if
small — start as a separate module to keep `main.rs` from growing
past its already-large 5,500 LOC).

Shape:

```rust
pub struct DoctorReport {
    pub checks: Vec<DoctorCheck>,
}

pub struct DoctorCheck {
    pub category: DoctorCategory,
    pub name: &'static str,
    pub status: DoctorStatus,
    pub message: String,
    pub help: Option<String>,
    pub details: Option<serde_json::Value>,
}

pub enum DoctorCategory { Chrome, Displays, EmbeddedShells, Assets }
pub enum DoctorStatus { Pass, Warn, Fail }

pub fn run_doctor(deck: &Path, env: &DoctorEnv) -> DoctorReport;
```

Wire-up in `main.rs`:

- Add `Command::Doctor { input: PathBuf, json: bool }` to the
  `Command` enum, with `input` defaulting to `deck.md`.
- Add `mod doctor;` and dispatch from `main`.
- Print the report via `doctor::print_human` or `doctor::print_json`
  and return the correct exit code.

## Testing plan (TDD)

Unit tests live alongside `doctor.rs`. All use dependency injection
(`DoctorEnv`) so no real Chrome / display / filesystem access happens
in unit tests. The pattern is already established by
`locate_chrome_with_env` in `main.rs`.

Red-Green-Refactor cycles:

1. `chrome_binary_discovered_reports_pass` — when `locate_chrome_with_env`
   would return `Ok`, doctor reports `pass` with the path.
2. `chrome_binary_missing_reports_fail_with_help` — when
   `locate_chrome_with_env` errors, doctor reports `fail` and the help
   string is included.
3. `chrome_profiles_writable_reports_pass_for_existing_home` — with a
   writable `HOME` tempdir, doctor reports `pass`.
4. `chrome_profiles_reports_warn_when_home_unset` — no `HOME`, `warn`.
5. `chrome_profiles_reports_fail_when_root_readonly` — mkdir denies,
   `fail`. (`#[cfg(unix)]` gated because chmod semantics.)
6. `displays_enumeration_reports_pass_with_counts` — feed a fixture
   JSON via a test-only `DoctorEnv::displays_provider`; assert
   `pass`, count, and bounds details.
7. `displays_enumeration_reports_warn_when_provider_returns_none` —
   `warn` with help.
8. `displays_reports_warn_on_non_macos` — when
   `DoctorEnv::platform == BrowserPlatform::Linux`, `warn` with a
   pointer to Issue #22.
9. `embedded_shells_report_present_shell_with_byte_count` — asserts the
   `pass` and the exact byte length matches
   `BUILTIN_SHELL_JS.len()`.
10. `embedded_shells_report_preview_shell_with_byte_count` — same
    for preview.
11. `assets_report_skipped_when_deck_does_not_exist` — no `Assets`
    category lines.
12. `assets_report_provenance_for_each_asset` — synthesize a tempdir
    deck with a `layouts/` sibling and no others; assert deck-adjacent
    / built-in provenance is reported per asset.
13. `assets_report_fail_for_missing_explicit_path` — frontmatter
    points at nonexistent path; that asset alone gets `fail`, other
    assets still get their own status.
14. `run_doctor_exit_code_is_pass_when_all_pass` — helper
    `exit_code_for(&report)` returns 0.
15. `run_doctor_exit_code_is_fail_when_any_fail` — returns 2.
16. `run_doctor_warn_does_not_fail_exit_code` — returns 0.
17. `print_human_writes_category_headers_and_status_glyphs` — string
    match on stdout.
18. `print_human_uses_no_color_when_not_tty` — inject
    `is_terminal=false`, assert no ANSI escapes.
19. `print_json_shape_is_stable_categories_names_and_details` —
    snapshot the JSON keys, not the values.
20. `cli_dispatch_calls_run_doctor_and_exits_with_code` — top-level
    integration test through `Cli::parse_from`.

Cover the invariants that matter:
- Fixed check-order (deterministic output).
- Warn vs fail exit-code split.
- All-explicit-assets deck runs clean without Chrome.
- Non-macOS still passes doctor with a warn for displays.

## Rollout

- New subcommand, no changes to existing subcommands or their exit
  codes. Adding a `Doctor` variant to the `Command` enum is
  backward-compatible (clap dispatches by explicit variant name).
- Bindings: no changes to core / no new TS types (doctor is
  CLI-side only, does not touch the manifest/notes contract). Skip
  `git diff --exit-code bindings/`.
- Shell / preview drift: no changes. Skip the npm-side rebuild step.
- Update the guide (`site/content/guide/*`) with a doctor entry after
  the plan lands.

## Records this plan supersedes / references

- Issue #244 (proposal).
- Issue #22 (Linux display enumeration; deferred, referenced in the
  displays check).
- `docs/plans/2026-07-10-peitho-layouts.md` (structural precedent for
  a read-only introspection subcommand).
- `crates/peitho/src/asset_resolution.rs` (`resolve_assets` /
  `Provenance` — the shape the assets check reports).
- CLAUDE.md § Pitfalls (the pain points doctor is designed to
  short-cut).
