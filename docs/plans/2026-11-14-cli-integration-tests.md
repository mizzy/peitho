# Binary-spawning CLI tests live in integration tests (Issue #747)

## Problem

Rust 1.100 (CI's `stable` since 2026-11-09) broke 16 unit tests in
`crates/peitho/src/docs.rs` and `crates/peitho/src/main.rs` with
`CARGO_BIN_EXE_peitho is unset`. They spawned the CLI through
`assert_cmd::Command::cargo_bin("peitho")`. Cargo sets `CARGO_BIN_EXE_<name>`
only for integration tests, and assert_cmd 2.2.2 (the latest release) no longer
finds the binary from a unit test with the newer cargo. The local 1.95 toolchain
still passed, so local gates did not catch it.

## Rule

A test that spawns the `peitho` binary lives under `crates/peitho/tests/`.
Nothing under `crates/*/src` calls `cargo_bin(`. A unit test that only needs the
command's output calls the function that produces it instead of the binary.

## Changes

- `tests/layouts.rs`: the ten `layouts` / `layouts --explain` tests, unchanged
  in what they assert.
- `tests/new.rs`: `new_command_scaffolds_from_cli`.
- `tests/cli_help.rs`: `new_command_is_listed_in_help`.
- `docs.rs`: `docs_list`, `docs_topic`, and `docs_all` stay unit tests and call
  `docs::render`, which returns exactly what the command prints, so they keep
  their comparisons against `TOPICS` and `render_topic` without a snapshot or a
  second frontmatter parser.
- `docs_unknown_topic_names_valid_topics` is removed: the existing
  `docs_topic_lists_every_guide_slug_as_a_possible_value` parser test already
  pins the slug list clap reports.
- `tests/docs.rs`: one smoke test for the command wiring through main's
  dispatch: the topic list, `docs getting-started`, `docs --all` containing that
  topic, and the unknown-topic error.

## Verification

`cargo +1.100.0 test --workspace` and `cargo test --workspace` (three runs),
clippy with `-D warnings`, and `cargo fmt --all --check`.
