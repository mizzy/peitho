# Load-independent timing tests (Issue #430)

Date: 2026-08-17
Issue: #430 — CDP export and present-server tests fail under parallel load

## Problem

Two families of tests encode wall-clock assumptions that blow under CPU
contention (observed repeatedly on CI and under local parallel load,
2026-08-16..17):

1. `cdp_export_port_timeout_reports_stderr_and_reaps_chrome` expects the
   fake Chrome's stderr to be captured by the time the DevToolsActivePort
   timeout fires — under load the reader loses the race and the error
   carries empty stderr.
2. `cdp_export_exited_before_port_reports_immediately_and_reaps_chrome`
   asserts detection latency ("promptly"), a bound with no correctness
   content.
3. Five `tests/present.rs` server tests give the spawned `peitho present`
   process 5 seconds to print its startup lines; a degraded runner
   (or a stray lingering present process locally) exceeds that.

## Fix

Make the assertions order-based, not latency-based:

1. **Production-side determinism for the timeout error**: when the
   DevToolsActivePort wait times out, kill/reap the child and JOIN the
   stderr reader **before** composing the error, so whatever stderr the
   process wrote is always complete in the message (EOF-bounded, no
   timing window). The test then asserts content deterministically. This
   is a genuine error-quality improvement, not a test relaxation: today a
   loaded machine can produce a stderr-less timeout error for a real
   Chrome failure too.
2. **Ordering instead of latency**: the dead-Chrome test asserts the
   error is the exited-before-port variant (not the timeout variant)
   within the overall deadline — no "promptly" bound. If the production
   path needs a distinct error kind to make that assertable, prefer
   introducing the typed distinction over string matching.
3. **Generous ceilings for process startup**: the present tests' 5s
   stdout deadlines become 30s (startup is normally <1s; the bound only
   guards true hangs, so a generous ceiling loses no signal). One shared
   constant, not five literals.

## Tests

The changed tests themselves are the deliverable; additionally run the
affected suites 5× consecutively under an artificial CPU load (e.g. a
parallel `cargo build` in another worktree) locally to demonstrate the
flake is gone.

## Non-goals

- No changes to production timeout durations (only error composition
  ordering).
- No serialization of the present test suite.
