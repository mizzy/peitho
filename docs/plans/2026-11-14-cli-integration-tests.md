# Move binary-spawning tests into integration tests

Issue: #747

## Goal

Rust 1.100 exposes the Cargo-built binary path to integration tests through
CARGO_BIN_EXE_peitho. Remove binary spawning from all 16 affected unit tests:
move 12 CLI tests into integration targets and exercise the four docs behaviors
directly through the renderer or CLI parser. Keep every original behavior
assertion and keep implementation details private.

## Changes

Following the [repository instructions](../../CLAUDE.md), organize tests from
[docs.rs](../../crates/peitho/src/docs.rs) and
[main.rs](../../crates/peitho/src/main.rs) as follows:

- docs unit tests: retain docs_list, docs_topic, and docs_all in the docs module.
  Call render directly and retain the assertions against TOPICS, render_topic,
  weight order, and removed frontmatter. Use the implementation's authoritative
  topic metadata and renderer; do not duplicate guide parsing or rendered output.
- CLI parser unit test: move docs_unknown_topic_names_valid_topics into main.rs's
  test module. Use Cli::try_parse_from to assert the clap error text and every
  slug returned by docs::topic_slugs().
- docs integration target: keep one smoke test that checks both the successful
  docs listing and rejection of a missing topic. Delete the rendered guide
  fixture and the integration test's guide parser.
- cli_help integration target: keep the moved new-command help test.
- new integration target: keep the moved scaffold CLI test.
- layouts integration target: keep the ten moved summary, JSON, and dispatch
  explanation tests, including the Unix code-image transformation fixture.

Follow [cli_help.rs](../../crates/peitho/tests/cli_help.rs) for binary invocation.
Remove unused source imports and helpers. Reuse shared test utilities only where
they fit without adding dead-code allowances. Preserve the original test function
names and record their old and final locations below.
Graph tooling is unavailable in this environment; verify references directly.

## Validation

1. Confirm there are no cargo_bin calls under crates/*/src.
2. Run cargo +1.100.0 test -p peitho docs.
3. Run cargo test -p peitho docs.
4. Run cargo clippy --workspace --all-targets -- -D warnings.
5. Run cargo fmt --all --check and inspect the diff for unintended changes.

Leave all changes uncommitted.

## Test-name mapping

The original functions keep their names. The final location identifies either
the unit-test module or the integration target.

| Old unit-test name | Final test location |
| --- | --- |
| `docs::tests::docs_list` | unit `docs::tests::docs_list` |
| `docs::tests::docs_topic` | unit `docs::tests::docs_topic` |
| `docs::tests::docs_all` | unit `docs::tests::docs_all` |
| `docs::tests::docs_unknown_topic_names_valid_topics` | unit `tests::docs_unknown_topic_names_valid_topics` |
| `tests::new_command_is_listed_in_help` | `cli_help::new_command_is_listed_in_help` |
| `tests::new_command_scaffolds_from_cli` | `new::new_command_scaffolds_from_cli` |
| `tests::layouts_command_prints_builtin_layout_summary` | `layouts::layouts_command_prints_builtin_layout_summary` |
| `tests::layouts_command_json_prints_expected_shape` | `layouts::layouts_command_json_prints_expected_shape` |
| `tests::layouts_explain_matching_slide_exits_zero` | `layouts::layouts_explain_matching_slide_exits_zero` |
| `tests::layouts_explain_applies_code_images_transform` | `layouts::layouts_explain_applies_code_images_transform` |
| `tests::layouts_explain_unknown_slide_key_exits_two_with_help` | `layouts::layouts_explain_unknown_slide_key_exits_two_with_help` |
| `tests::layouts_explain_unknown_slide_key_with_json_emits_structured_error` | `layouts::layouts_explain_unknown_slide_key_with_json_emits_structured_error` |
| `tests::layouts_explain_unknown_explicit_layout_prints_trace_and_exits_one` | `layouts::layouts_explain_unknown_explicit_layout_prints_trace_and_exits_one` |
| `tests::layouts_explain_dispatch_failure_exits_one_and_prints_trace` | `layouts::layouts_explain_dispatch_failure_exits_one_and_prints_trace` |
| `tests::layouts_explain_json_includes_reason_for_sole_layout_no_match` | `layouts::layouts_explain_json_includes_reason_for_sole_layout_no_match` |
| `tests::layouts_explain_sole_layout_failure_prints_reason` | `layouts::layouts_explain_sole_layout_failure_prints_reason` |
| New smoke test | integration `docs::docs_command_smoke` |

## Validation results

- The original Rust 1.100 panic in docs::tests::docs_topic was reproduced before
  removing subprocess use: CARGO_BIN_EXE_peitho was unset.
- Both docs-focused commands pass on Rust 1.100 and the default Rust 1.95
  toolchain: 15 unit tests and one integration smoke test per run.
- All original assertions in docs_list, docs_topic, and docs_all are preserved
  exactly. The unknown-topic parser test retains the error-text and slug checks.
- Clippy with all workspace targets and warnings denied, formatting, and
  whitespace checks pass.
- No cargo_bin calls or AssertCommand imports remain under crates/*/src. The
  rendered guide fixture and duplicate guide parser are removed.
- The layouts, new, and cli_help integration files are byte-for-byte unchanged
  by the docs rework. No visibility was widened or dead-code allowances added.

Validation used /Users/mizzy/.cargo/bin/cargo directly with separate target
directories under /private/tmp. The managed Cargo wrapper cannot lock its shared
target directory inside this sandbox. The docs-focused runs exclude the known
sandbox socket-binding failures.
