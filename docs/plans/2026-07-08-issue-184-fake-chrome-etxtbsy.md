# Issue #184: fake Chrome ETXTBSY race

## Problem

The fake Chrome tests write a shell script and then execute that same path
directly. Under parallel `cargo test`, a sibling fork can briefly inherit the
writer file descriptor, leaving the script text busy while another test tries
to `exec` it. That makes `spawn` fail with ETXTBSY before the test reaches the
intended one-shot Chrome behavior.

## Plan

- Run fake Chrome scripts through `/bin/sh`, passing the script path as the
  shell script argument instead of executing the freshly-written file.
- Rename the helper from executable-script writing to plain script writing and
  remove permission changes that are no longer needed.
- Add actual error text to the relevant `contains` assertions so a future CI
  failure is diagnosable from logs.
- Keep the existing one-shot timeout behavior unchanged.
- Verify with repeated `cargo test -p peitho --bin peitho`, workspace tests,
  clippy, and rustfmt.
