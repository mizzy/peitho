# Default input to deck.md when omitted (Issue #171)

## Goal

Make the input argument to `peitho build` / `peitho present` / `peitho export pdf` optional; when omitted, use `deck.md` in the current directory. Explicit specification is only needed when the filename differs from the convention.

## Changes

1. **clap default value**: attach `#[arg(default_value = "deck.md")]` to `Command::Build.input` / `Command::Present.input` / `ExportCommand::Pdf.input`. Do not use `Option<PathBuf>` + manual unwrap — a clap-level default self-documents by showing `[default: deck.md]` in `--help`, and the downstream type stays `PathBuf` (no resolution logic required at call sites)
2. **Self-explanatory read errors**: map the `fs::read_to_string(input).into_diagnostic()?` at the top of `build_artifacts` into an error carrying the path and a help. Defaulting will increase "bare `peitho build` in a directory with no `deck.md`", but the current error is `No such file or directory (os error 2)` and does not even show the path. Do not branch on whether the value came from a default or an explicit argument — a single message that shows the path and recovery works for both (do not create per-symptom branches)
3. **README**: reflect the shorthand form in each Usage example

## Tests (TDD)

- `build_command_defaults_input_to_deck_md`: `Cli::parse_from(["peitho","build"])` → `input == "deck.md"`
- `present_command_defaults_input_to_deck_md`: same
- `export_pdf_command_defaults_input_to_deck_md`: same
- `build_artifacts_missing_input_error_names_path_and_default`: `build_artifacts` on a non-existent path → error string contains the path and a help

## Out of scope

- `publish` takes dist, not a deck, so it is out of scope
- No lookup for other convention names besides deck.md (e.g. index.md) — single default + explicit specification only
