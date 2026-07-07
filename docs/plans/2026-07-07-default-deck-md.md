# 入力ファイル省略時に deck.md をデフォルトにする (Issue #171)

## ゴール

`peitho build` / `peitho present` / `peitho export pdf` の入力引数を省略可能にし、省略時はカレントディレクトリの `deck.md` を使う。ファイル名が慣例と違う時だけ明示指定する。

## 変更点

1. **clap デフォルト値**: `Command::Build.input` / `Command::Present.input` / `ExportCommand::Pdf.input` に `#[arg(default_value = "deck.md")]` を付ける。`Option<PathBuf>` + 手動 unwrap にはしない — clap レベルのデフォルトなら `--help` にも `[default: deck.md]` と出て自己文書化され、下流の型は `PathBuf` のまま変わらない (呼び出し側に解決の記憶を要求しない)
2. **読み取りエラーの自己説明化**: `build_artifacts` 冒頭の `fs::read_to_string(input).into_diagnostic()?` を、パスと help 付きのエラーに map する。デフォルト化で「`deck.md` の無いディレクトリで裸の `peitho build`」が増えるが、現状のエラーは `No such file or directory (os error 2)` とパスすら出ない。デフォルト由来か明示指定かで分岐はしない — どちらでもパスと復旧手段を示す 1 つのメッセージで足りる (症状別分岐を作らない)
3. **README**: Usage の各例に省略形を反映

## テスト (TDD)

- `build_command_defaults_input_to_deck_md`: `Cli::parse_from(["peitho","build"])` → `input == "deck.md"`
- `present_command_defaults_input_to_deck_md`: 同上
- `export_pdf_command_defaults_input_to_deck_md`: 同上
- `build_artifacts_missing_input_error_names_path_and_default`: 存在しないパスで `build_artifacts` → エラー文字列にパスと help が含まれる

## スコープ外

- `publish` は入力がデッキではなく dist なので対象外
- deck.md 以外の慣例名 (index.md 等) の探索はしない — 単一のデフォルト + 明示指定のみ
