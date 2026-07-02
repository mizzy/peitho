# Peitho Milestone 1 Vertical Slice Implementation Plan

この計画は `/Users/mizzy/Downloads/PEITHO_KICKOFF.md` 全文を唯一の設計正として書く。

## Purpose

マイルストーン1は、Markdown 1枚から契約検査済み HTML を出す最小の縦切りを通す。実証対象は、Markdown と HTML/CSS テンプレートの分離、テンプレート自身から抽出するスロット契約、安定キーを狙う override CSS、契約違反のビルド時エラーである。

対象は Rust 側のみ。`packages/peitho-present`、`bindings`、`ts-rs`、`schemars`、`present`、`publish`、fenced div 明示スロット記法、型駆動レイアウト探索、watch は作らない。

## File Structure Map

| Path | Responsibility | Depends on |
| --- | --- | --- |
| `.gitignore` | ignore generated `target/` and `dist/` artifacts | none |
| `Cargo.toml` | workspace root; `crates/peitho-core` and `crates/peitho` members | none |
| `crates/peitho-core/Cargo.toml` | core library dependencies | workspace |
| `crates/peitho-core/src/lib.rs` | public module exports | all core modules |
| `crates/peitho-core/src/domain.rs` | `SlideKey`, `SlotName`, `Accepts`, `Arity`, `SlotContract`, content fragment types | none |
| `crates/peitho-core/src/error.rs` | line/help-bearing build errors | `domain.rs` only for display values |
| `crates/peitho-core/src/phase.rs` | real typestate data: `Deck<Parsed>`, `Deck<Mapped>`, `Deck<Checked>`, `Deck<Rendered>` | `domain.rs` |
| `crates/peitho-core/src/parser.rs` | Markdown to `Deck<Parsed>`; explicit key comment and derived key fallback | `domain.rs`, `error.rs`, `phase.rs` |
| `crates/peitho-core/src/template.rs` | HTML template to `Template` containing domain `SlotContract` values | `domain.rs`, `error.rs` |
| `crates/peitho-core/src/mapping.rs` | convention mapping: shallowest heading to `title`, code blocks to `code`, remaining blocks to `body` | `domain.rs`, `error.rs`, `phase.rs`, `template.rs` |
| `crates/peitho-core/src/check.rs` | four checks: assignment, accepts, arity, residual plus required slots | `domain.rs`, `error.rs`, `phase.rs`, `template.rs` |
| `crates/peitho-core/src/render.rs` | `Deck<Checked>` to HTML with `data-slide-key` and `.slot-*` | `domain.rs`, `error.rs`, `phase.rs`, `template.rs` |
| `crates/peitho-core/src/theme.rs` | combine `base.css` and `overrides.css`; validate slide keys and `.slot-*` refs | `domain.rs`, `error.rs`, `phase.rs`, `template.rs` |
| `crates/peitho/Cargo.toml` | CLI binary dependencies and CLI integration-test dev dependencies | `peitho-core` |
| `crates/peitho/src/main.rs` | `peitho build <md>` orchestration | `peitho-core` |
| `crates/peitho/tests/build.rs` | black-box CLI success and failure tests | CLI binary |
| `templates/title-body-code.html` | base template with `title`, `body`, `code` slots | none |
| `themes/base.css` | shared base CSS | generated slot classes |
| `themes/overrides.css` | one-line `arch-1` override | `examples/deck.md`, template slots |
| `examples/deck.md` | one-slide deck with `<!-- {"key":"arch-1"} -->` | template convention |

Dependency direction is one-way: `peitho` CLI calls `peitho-core`; core modules never call the CLI. `parser` and `template` are independent inputs, `mapping` joins them, `check` turns `Mapped` into `Checked`, `render` accepts only `Checked`, and `theme` validates CSS against the same checked keys and template slots.

## Implementation Tasks

### Task 1 - Initialize Cargo Workspace and CLI Shell

Goal: create the Rust workspace shape required by §17 and a CLI binary whose help exposes `build`.

Files:

- `Cargo.toml`
- `.gitignore`
- `crates/peitho-core/Cargo.toml`
- `crates/peitho-core/src/lib.rs`
- `crates/peitho/Cargo.toml`
- `crates/peitho/src/main.rs`
- `crates/peitho/tests/cli_help.rs`

Test:

```rust
// crates/peitho/tests/cli_help.rs
use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn help_mentions_build_subcommand() {
    Command::cargo_bin("peitho")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("build"));
}
```

Implementation:

```toml
# Cargo.toml
[workspace]
members = ["crates/peitho-core", "crates/peitho"]
resolver = "2"

[workspace.package]
edition = "2021"
license = "MIT"
version = "0.1.0"

[workspace.dependencies]
assert_cmd = "2"
clap = { version = "4", features = ["derive"] }
html-escape = "0.2"
lol_html = "2"
miette = { version = "7", features = ["fancy"] }
predicates = "3"
pulldown-cmark = "0.10"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tempfile = "3"
thiserror = "1"
```

```toml
# crates/peitho-core/Cargo.toml
[package]
name = "peitho-core"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
html-escape.workspace = true
lol_html.workspace = true
pulldown-cmark.workspace = true
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
```

```rust
// crates/peitho-core/src/lib.rs
pub fn crate_name() -> &'static str {
    "peitho-core"
}
```

```toml
# crates/peitho/Cargo.toml
[package]
name = "peitho"
version.workspace = true
edition.workspace = true
license.workspace = true

[[bin]]
name = "peitho"
path = "src/main.rs"

[dependencies]
clap.workspace = true
miette.workspace = true
peitho-core = { path = "../peitho-core" }

[dev-dependencies]
assert_cmd.workspace = true
predicates.workspace = true
tempfile.workspace = true
```

```gitignore
# .gitignore
/target
/dist
```

```rust
// crates/peitho/src/main.rs
use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "peitho")]
#[command(about = "Build HTML-native presentations from Markdown")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Build { input: PathBuf },
}

fn main() {
    let _cli = Cli::parse();
}
```

Verification:

```bash
cargo test --workspace
cargo run -p peitho -- --help
test -f .gitignore
```

### Task 2 - Define Contract Domain Types

Goal: encode slot contracts and content kinds once in `peitho-core`.

Files:

- `crates/peitho-core/src/lib.rs`
- `crates/peitho-core/src/domain.rs`

Test:

```rust
// crates/peitho-core/src/domain.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_slot_accepts_values() {
        assert_eq!("inline".parse::<Accepts>().unwrap(), Accepts::Inline);
        assert_eq!("blocks".parse::<Accepts>().unwrap(), Accepts::Blocks);
        assert_eq!("text".parse::<Accepts>().unwrap(), Accepts::Text);
        assert_eq!("code".parse::<Accepts>().unwrap(), Accepts::Code);
        assert_eq!("image".parse::<Accepts>().unwrap(), Accepts::Image);
        assert_eq!("list".parse::<Accepts>().unwrap(), Accepts::List);
    }

    #[test]
    fn arity_bounds_match_spec_values() {
        assert!(Arity::ExactlyOne.allows(1));
        assert!(!Arity::ExactlyOne.allows(0));
        assert!(Arity::ZeroOrOne.allows(0));
        assert!(!Arity::ZeroOrOne.allows(2));
        assert!(Arity::OneOrMore.allows(3));
        assert!(Arity::ZeroOrMore.allows(0));
    }
}
```

Implementation:

```rust
// crates/peitho-core/src/domain.rs
use std::{fmt, str::FromStr};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SlideKey(String);

impl SlideKey {
    pub fn new(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if value.is_empty() || value.contains('"') || value.chars().any(char::is_control) {
            return Err("slide key must be nonempty and HTML-attribute safe".to_owned());
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SlotName(String);

impl SlotName {
    pub fn new(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        let valid = value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if value.is_empty() || !valid {
            return Err("slot name must use lowercase ascii, digits, or '-'".to_owned());
        }
        Ok(Self(value))
    }

    pub fn class_name(&self) -> String {
        format!("slot-{}", self.0)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accepts {
    Inline,
    Blocks,
    Text,
    Code,
    Image,
    List,
}

impl FromStr for Accepts {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "inline" => Ok(Self::Inline),
            "blocks" => Ok(Self::Blocks),
            "text" => Ok(Self::Text),
            "code" => Ok(Self::Code),
            "image" => Ok(Self::Image),
            "list" => Ok(Self::List),
            other => Err(format!("unknown accepts value '{other}'")),
        }
    }
}

impl fmt::Display for Accepts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Inline => "inline",
            Self::Blocks => "blocks",
            Self::Text => "text",
            Self::Code => "code",
            Self::Image => "image",
            Self::List => "list",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arity {
    ExactlyOne,
    ZeroOrOne,
    OneOrMore,
    ZeroOrMore,
}

impl Arity {
    pub fn allows(self, count: usize) -> bool {
        match self {
            Self::ExactlyOne => count == 1,
            Self::ZeroOrOne => count <= 1,
            Self::OneOrMore => count >= 1,
            Self::ZeroOrMore => true,
        }
    }
}

impl fmt::Display for Arity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ExactlyOne => "1",
            Self::ZeroOrOne => "0..1",
            Self::OneOrMore => "1..*",
            Self::ZeroOrMore => "0..*",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotContract {
    pub name: SlotName,
    pub accepts: Accepts,
    pub arity: Arity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FragmentKind {
    Heading { level: u8 },
    Paragraph,
    Text,
    Code,
    Image,
    List,
}

impl FragmentKind {
    pub fn default_accepts(self) -> Accepts {
        match self {
            Self::Heading { .. } => Accepts::Inline,
            Self::Paragraph => Accepts::Blocks,
            Self::Text => Accepts::Text,
            Self::Code => Accepts::Code,
            Self::Image => Accepts::Image,
            Self::List => Accepts::List,
        }
    }

    pub fn removal_noun(self) -> &'static str {
        match self {
            Self::Heading { .. } => "heading",
            Self::Paragraph => "paragraph",
            Self::Text => "text block",
            Self::Code => "code block",
            Self::Image => "image",
            Self::List => "list",
        }
    }
}

impl fmt::Display for FragmentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Heading { .. } => "heading",
            Self::Paragraph => "paragraph",
            Self::Text => "text",
            Self::Code => "code",
            Self::Image => "image",
            Self::List => "list",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFragment {
    line: usize,
    kind: FragmentKind,
    markdown: String,
    text: String,
    code: String,
    language: Option<String>,
}

impl SourceFragment {
    // Milestone 1 uses plain text only for title headings; body and list slots render from markdown().
    pub fn heading(
        line: usize,
        level: u8,
        markdown: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        let markdown = markdown.into();
        let text = text.into();
        Self {
            line,
            kind: FragmentKind::Heading { level },
            markdown,
            text,
            code: String::new(),
            language: None,
        }
    }

    pub fn paragraph(line: usize, markdown: impl Into<String>) -> Self {
        let markdown = markdown.into();
        Self {
            line,
            kind: FragmentKind::Paragraph,
            text: String::new(),
            markdown,
            code: String::new(),
            language: None,
        }
    }

    pub fn list(line: usize, markdown: impl Into<String>) -> Self {
        let markdown = markdown.into();
        Self {
            line,
            kind: FragmentKind::List,
            text: String::new(),
            markdown,
            code: String::new(),
            language: None,
        }
    }

    pub fn code(line: usize, language: Option<String>, code: impl Into<String>) -> Self {
        let code = code.into();
        Self {
            line,
            kind: FragmentKind::Code,
            markdown: code.clone(),
            text: String::new(),
            code,
            language,
        }
    }

    pub fn line(&self) -> usize { self.line }
    pub fn kind(&self) -> FragmentKind { self.kind }
    pub fn markdown(&self) -> &str { &self.markdown }
    pub fn plain_text(&self) -> &str { &self.text }
    pub fn code_text(&self) -> &str { &self.code }
    pub fn heading_text(&self) -> Option<String> {
        matches!(self.kind, FragmentKind::Heading { .. }).then(|| self.text.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedSlide {
    index: usize,
    key: SlideKey,
    html: String,
}

impl RenderedSlide {
    pub(crate) fn new(index: usize, key: SlideKey, html: String) -> Self {
        Self { index, key, html }
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn key(&self) -> &SlideKey {
        &self.key
    }

    pub fn html(&self) -> &str {
        &self.html
    }
}
```

Verification:

```bash
cargo test -p peitho-core domain
```

### Task 3 - Add Line and Help Bearing Errors

Goal: every contract failure can carry a source line and concrete help text.

Files:

- `crates/peitho-core/src/lib.rs`
- `crates/peitho-core/src/error.rs`

Test:

```rust
// crates/peitho-core/src/error.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_includes_line_and_help() {
        let err = BuildError::new(
            ErrorKind::Arity,
            Some(12),
            "slot 'code' got 2 item(s), but layout 'title-body-code' allows 0..1",
            "use a layout with two code slots or remove one code block",
        );

        assert_eq!(err.line, Some(12));
        assert_eq!(
            err.help,
            "use a layout with two code slots or remove one code block"
        );
        assert!(err.to_string().contains("line 12"));
        assert!(err.to_string().contains("slot 'code' got 2 item(s)"));
    }
}
```

Implementation:

```rust
// crates/peitho-core/src/error.rs
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    Parse,
    Template,
    Assignment,
    Accepts,
    Arity,
    ResidualContent,
    Theme,
    Io,
}

#[derive(Debug, Clone, Error)]
#[error("{rendered}")]
pub struct BuildError {
    pub kind: ErrorKind,
    pub line: Option<usize>,
    pub message: String,
    pub help: String,
    rendered: String,
}

impl BuildError {
    pub fn new(
        kind: ErrorKind,
        line: Option<usize>,
        message: impl Into<String>,
        help: impl Into<String>,
    ) -> Self {
        let message = message.into();
        let help = help.into();
        let rendered = match line {
            Some(line) => format!("line {line}: {message}\n  = help: {help}"),
            None => format!("{message}\n  = help: {help}"),
        };
        Self {
            kind,
            line,
            message,
            help,
            rendered,
        }
    }
}

pub type Result<T> = std::result::Result<T, BuildError>;
```

Verification:

```bash
cargo test -p peitho-core error
```

### Task 4 - Encode Real Typestate Phases

Goal: represent `Parsed -> Mapped -> Checked -> Rendered` as distinct data shapes, not marker-only states.

Files:

- `crates/peitho-core/src/lib.rs`
- `crates/peitho-core/src/phase.rs`

Test:

```rust
// crates/peitho-core/src/phase.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{SlideKey, SourceFragment};

    #[test]
    fn parsed_deck_owns_source_fragments() {
        let deck = Deck::parsed(vec![ParsedSlide {
            key: SlideKey::new("arch-1").unwrap(),
            index: 0,
            fragments: vec![SourceFragment::paragraph(3, "body")],
        }]);

        assert_eq!(deck.parsed_slides()[0].fragments[0].line(), 3);
    }
}
```

Compile guard:

```rust
// crates/peitho-core/src/phase.rs
/// ```compile_fail
/// use peitho_core::{require_checked_for_render, Deck, Mapped};
///
/// fn cannot_render_mapped(deck: &Deck<Mapped>) {
///     require_checked_for_render(deck);
/// }
/// ```
pub fn require_checked_for_render(_: &Deck<Checked>) {}
```

Implementation:

```rust
// crates/peitho-core/src/lib.rs
pub mod domain;
pub mod phase;

pub use phase::{require_checked_for_render, Deck, Mapped};
```

```rust
// crates/peitho-core/src/phase.rs
use std::collections::BTreeMap;

use crate::domain::{RenderedSlide, SlideKey, SlotContract, SlotName, SourceFragment};

#[derive(Debug, Clone)]
pub struct Deck<P> {
    phase: P,
}

#[derive(Debug, Clone)]
pub struct Parsed {
    slides: Vec<ParsedSlide>,
}

#[derive(Debug, Clone)]
pub struct ParsedSlide {
    pub index: usize,
    pub key: SlideKey,
    pub fragments: Vec<SourceFragment>,
}

#[derive(Debug, Clone)]
pub struct Mapped {
    slides: Vec<MappedSlide>,
}

#[derive(Debug, Clone)]
pub struct MappedSlide {
    pub(crate) index: usize,
    pub(crate) key: SlideKey,
    pub(crate) slots: BTreeMap<SlotName, MappedSlot>,
    pub(crate) unassigned: Vec<UnassignedFragment>,
}

#[derive(Debug, Clone)]
pub struct MappedSlot {
    contract: SlotContract,
    fragments: Vec<SourceFragment>,
}

impl MappedSlot {
    pub(crate) fn new(contract: SlotContract) -> Self {
        Self {
            contract,
            fragments: Vec::new(),
        }
    }

    pub(crate) fn push(&mut self, fragment: SourceFragment) {
        self.fragments.push(fragment);
    }

    pub fn contract(&self) -> &SlotContract {
        &self.contract
    }

    pub fn fragments(&self) -> &[SourceFragment] {
        &self.fragments
    }
}

#[derive(Debug, Clone)]
pub struct UnassignedFragment {
    expected_slot: SlotName,
    fragment: SourceFragment,
}

impl UnassignedFragment {
    pub(crate) fn new(expected_slot: SlotName, fragment: SourceFragment) -> Self {
        Self {
            expected_slot,
            fragment,
        }
    }

    pub fn expected_slot(&self) -> &SlotName {
        &self.expected_slot
    }

    pub fn fragment(&self) -> &SourceFragment {
        &self.fragment
    }
}

#[derive(Debug, Clone)]
pub struct Checked {
    slides: Vec<CheckedSlide>,
}

#[derive(Debug, Clone)]
pub struct CheckedSlide {
    index: usize,
    key: SlideKey,
    slots: BTreeMap<SlotName, Vec<SourceFragment>>,
}

#[derive(Debug, Clone)]
pub struct Rendered {
    slides: Vec<RenderedSlide>,
    css: String,
}

impl Deck<Parsed> {
    pub(crate) fn parsed(slides: Vec<ParsedSlide>) -> Self {
        Self {
            phase: Parsed { slides },
        }
    }

    pub fn parsed_slides(&self) -> &[ParsedSlide] {
        &self.phase.slides
    }

    pub(crate) fn into_parsed_slides(self) -> Vec<ParsedSlide> {
        self.phase.slides
    }
}

impl Deck<Mapped> {
    pub(crate) fn mapped(slides: Vec<MappedSlide>) -> Self {
        Self {
            phase: Mapped { slides },
        }
    }

    pub fn mapped_slides(&self) -> &[MappedSlide] {
        &self.phase.slides
    }

    pub(crate) fn into_mapped_slides(self) -> Vec<MappedSlide> {
        self.phase.slides
    }
}

impl CheckedSlide {
    pub(crate) fn new(
        index: usize,
        key: SlideKey,
        slots: BTreeMap<SlotName, Vec<SourceFragment>>,
    ) -> Self {
        Self { index, key, slots }
    }

    pub(crate) fn index(&self) -> usize {
        self.index
    }

    pub(crate) fn key(&self) -> &SlideKey {
        &self.key
    }

    pub(crate) fn slots(&self) -> &BTreeMap<SlotName, Vec<SourceFragment>> {
        &self.slots
    }
}

impl Deck<Checked> {
    pub(crate) fn checked(slides: Vec<CheckedSlide>) -> Self {
        Self {
            phase: Checked { slides },
        }
    }

    pub fn slide_count(&self) -> usize {
        self.phase.slides.len()
    }

    pub fn slide_keys(&self) -> impl Iterator<Item = &SlideKey> {
        self.phase.slides.iter().map(|slide| &slide.key)
    }

    pub(crate) fn into_checked_slides(self) -> Vec<CheckedSlide> {
        self.phase.slides
    }
}

impl Deck<Rendered> {
    pub(crate) fn rendered(slides: Vec<RenderedSlide>, css: String) -> Self {
        Self {
            phase: Rendered { slides, css },
        }
    }

    pub fn slide_count(&self) -> usize {
        self.phase.slides.len()
    }

    pub fn slides(&self) -> &[RenderedSlide] {
        &self.phase.slides
    }

    pub fn css(&self) -> &str {
        &self.phase.css
    }
}
```

Verification:

```bash
cargo test -p peitho-core phase
cargo test -p peitho-core --doc
```

### Task 5 - Parse Markdown Key, Heading, Body, and Code

Goal: parse the one-slide Markdown shape and preserve source lines for later diagnostics.

Files:

- `crates/peitho-core/src/domain.rs`
- `crates/peitho-core/src/lib.rs`
- `crates/peitho-core/src/parser.rs`

Test:

```rust
// crates/peitho-core/src/parser.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{domain::FragmentKind, error::ErrorKind};

    #[test]
    fn preserves_inline_markdown_and_generates_list_fragments() {
        let markdown = r#"<!-- {"key":"arch-1"} -->
# **Architecture** `Phase`

Markdown is the **source** of [truth](https://example.com).

- Markdown = source of truth
- Deterministic rendering

```rust
enum Phase { Parsed, Mapped, Checked }
```
"#;

        let deck = parse_markdown(markdown).unwrap();
        let slide = &deck.parsed_slides()[0];

        assert_eq!(slide.key.as_str(), "arch-1");
        assert_eq!(slide.fragments[0].kind(), FragmentKind::Heading { level: 1 });
        assert_eq!(slide.fragments[0].line(), 2);
        assert_eq!(slide.fragments[0].markdown(), "# **Architecture** `Phase`");
        assert_eq!(slide.fragments[1].kind(), FragmentKind::Paragraph);
        assert_eq!(slide.fragments[1].markdown(), "Markdown is the **source** of [truth](https://example.com).");
        assert_eq!(slide.fragments[2].kind(), FragmentKind::List);
        assert_eq!(slide.fragments[2].line(), 6);
        assert!(slide.fragments[2].markdown().contains("- Markdown = source of truth"));
        assert_eq!(slide.fragments[3].kind(), FragmentKind::Code);
        assert_eq!(slide.fragments[3].line(), 9);
    }

    #[test]
    fn keeps_loose_nested_list_and_item_code_as_one_list_fragment() {
        let markdown = r#"# Title

- loose a

- loose b
  - nested

  ```rust
  fn inside_item() {}
  ```

After list
"#;

        let deck = parse_markdown(markdown).unwrap();
        let slide = &deck.parsed_slides()[0];

        assert_eq!(slide.fragments[1].kind(), FragmentKind::List);
        assert!(slide.fragments[1].markdown().contains("- loose a"));
        assert!(slide.fragments[1].markdown().contains("  - nested"));
        assert!(slide.fragments[1].markdown().contains("fn inside_item()"));
        assert_eq!(slide.fragments[2].kind(), FragmentKind::Paragraph);
        assert_eq!(slide.fragments[2].markdown(), "After list");
    }

    #[test]
    fn rejects_unsupported_construct_with_line_and_help() {
        let err = parse_markdown("# Title\n\n> quoted").unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(3));
        assert!(err.to_string().contains("unsupported construct 'blockquote'"));
        assert_eq!(err.help, "rewrite this slide using headings, paragraphs, lists, or fenced code blocks for milestone 1");
    }
}
```

Implementation:

```rust
// crates/peitho-core/src/parser.rs
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use serde::Deserialize;

use crate::{
    domain::{SlideKey, SourceFragment},
    error::{BuildError, ErrorKind, Result},
    phase::{Deck, Parsed, ParsedSlide},
};

#[derive(Debug, Deserialize)]
struct KeyComment {
    key: String,
}

enum OpenBlock {
    Heading { level: u8, start: usize, text: String },
    Paragraph { start: usize },
    Code { start: usize, language: Option<String>, text: String },
}

pub fn parse_markdown(source: &str) -> Result<Deck<Parsed>> {
    let mut explicit_key = None;
    let mut fragments = Vec::new();
    let mut block: Option<OpenBlock> = None;
    let mut list_depth = 0usize;
    let mut list_start = None;

    for (event, range) in Parser::new_ext(source, Options::empty()).into_offset_iter() {
        let line = line_for_offset(source, range.start);
        match event {
            Event::Start(Tag::List(_)) => {
                if list_depth == 0 {
                    list_start = Some(range.start);
                }
                list_depth += 1;
            }
            Event::End(TagEnd::List(_)) => {
                if list_depth == 0 {
                    return Err(unsupported_construct(line, "list end without list start"));
                }
                list_depth -= 1;
                if list_depth == 0 {
                    if let Some(start) = list_start.take() {
                        fragments.push(SourceFragment::list(
                            line_for_offset(source, start),
                            source_slice(source, start, range.end),
                        ));
                    }
                }
            }
            _ if list_depth > 0 => {}
            Event::Html(html) => {
                if let Some(key) = parse_key_comment(html.as_ref())? {
                    explicit_key = Some(key);
                } else if !html.trim().is_empty() {
                    return Err(unsupported_construct(line, "html"));
                }
            }
            Event::Start(Tag::Heading { level, .. }) => {
                block = Some(OpenBlock::Heading {
                    level: heading_level_to_u8(level),
                    start: range.start,
                    text: String::new(),
                });
            }
            Event::End(TagEnd::Heading(_)) => {
                if matches!(block, Some(OpenBlock::Heading { .. })) {
                    let Some(OpenBlock::Heading { level, start, text }) = block.take() else {
                        unreachable!();
                    };
                    fragments.push(SourceFragment::heading(
                        line_for_offset(source, start),
                        level,
                        source_slice(source, start, range.end),
                        text.trim(),
                    ));
                }
            }
            Event::Start(Tag::Paragraph) => {
                block = Some(OpenBlock::Paragraph { start: range.start });
            }
            Event::End(TagEnd::Paragraph) => {
                if matches!(block, Some(OpenBlock::Paragraph { .. })) {
                    let Some(OpenBlock::Paragraph { start }) = block.take() else {
                        unreachable!();
                    };
                    fragments.push(SourceFragment::paragraph(
                        line_for_offset(source, start),
                        source_slice(source, start, range.end),
                    ));
                }
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let language = match kind {
                    CodeBlockKind::Fenced(lang) if !lang.is_empty() => Some(lang.to_string()),
                    _ => None,
                };
                block = Some(OpenBlock::Code {
                    start: range.start,
                    language,
                    text: String::new(),
                });
            }
            Event::End(TagEnd::CodeBlock) => {
                if matches!(block, Some(OpenBlock::Code { .. })) {
                    let Some(OpenBlock::Code { start, language, text }) = block.take() else {
                        unreachable!();
                    };
                    fragments.push(SourceFragment::code(line_for_offset(source, start), language, text));
                }
            }
            Event::Text(text) | Event::Code(text) => {
                match block.as_mut() {
                    Some(OpenBlock::Heading { text: heading_text, .. }) => heading_text.push_str(&text),
                    Some(OpenBlock::Code { text: code_text, .. }) => code_text.push_str(&text),
                    Some(OpenBlock::Paragraph { .. }) => {}
                    None if text.trim().is_empty() => {}
                    None => return Err(unsupported_construct(line, "text outside block")),
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(OpenBlock::Code { text, .. }) = block.as_mut() {
                    text.push('\n');
                }
            }
            Event::Start(tag) if unsupported_tag(&tag) => {
                return Err(unsupported_construct(line, unsupported_tag_name(&tag)));
            }
            Event::Start(Tag::Item)
            | Event::End(TagEnd::Item)
            | Event::Start(Tag::Emphasis)
            | Event::End(TagEnd::Emphasis)
            | Event::Start(Tag::Strong)
            | Event::End(TagEnd::Strong)
            | Event::Start(Tag::Link { .. })
            | Event::End(TagEnd::Link) => {}
            other => return Err(unsupported_construct(line, event_name(&other))),
        }
    }

    let key = explicit_key.unwrap_or_else(|| derive_key_from_fragments(&fragments, 0));
    Ok(Deck::parsed(vec![ParsedSlide {
        index: 0,
        key,
        fragments,
    }]))
}

fn parse_key_comment(raw: &str) -> Result<Option<SlideKey>> {
    let trimmed = raw.trim();
    if !trimmed.starts_with("<!--") || !trimmed.ends_with("-->") {
        return Ok(None);
    }
    let json = trimmed.trim_start_matches("<!--").trim_end_matches("-->").trim();
    let parsed: KeyComment = serde_json::from_str(json).map_err(|err| {
        BuildError::new(
            ErrorKind::Parse,
            None,
            format!("invalid slide key comment: {err}"),
            r#"use <!-- {"key":"arch-1"} --> before the slide heading"#,
        )
    })?;
    SlideKey::new(parsed.key)
        .map(Some)
        .map_err(|message| BuildError::new(ErrorKind::Parse, None, message, "change the key string"))
}

fn line_for_offset(source: &str, offset: usize) -> usize {
    source[..offset].bytes().filter(|byte| *byte == b'\n').count() + 1
}

fn source_slice(source: &str, start: usize, end: usize) -> String {
    source[start..end].trim().to_owned()
}

fn unsupported_construct(line: usize, name: &str) -> BuildError {
    BuildError::new(
        ErrorKind::Parse,
        Some(line),
        format!("unsupported construct '{name}'"),
        "rewrite this slide using headings, paragraphs, lists, or fenced code blocks for milestone 1",
    )
}

fn unsupported_tag(tag: &Tag<'_>) -> bool {
    matches!(
        tag,
        Tag::BlockQuote
            | Tag::Table(_)
            | Tag::TableHead
            | Tag::TableRow
            | Tag::TableCell
            | Tag::Image { .. }
            | Tag::FootnoteDefinition(_)
    )
}

fn unsupported_tag_name(tag: &Tag<'_>) -> &'static str {
    match tag {
        Tag::BlockQuote => "blockquote",
        Tag::Table(_) | Tag::TableHead | Tag::TableRow | Tag::TableCell => "table",
        Tag::Image { .. } => "image",
        Tag::FootnoteDefinition(_) => "footnote",
        _ => "markdown",
    }
}

fn event_name(event: &Event<'_>) -> &'static str {
    match event {
        Event::Rule => "thematic break",
        Event::TaskListMarker(_) => "task list marker",
        _ => "markdown",
    }
}

fn heading_level_to_u8(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}
```

Verification:

```bash
cargo test -p peitho-core parser::tests::preserves_inline_markdown_and_generates_list_fragments
cargo test -p peitho-core parser::tests::keeps_loose_nested_list_and_item_code_as_one_list_fragment
cargo test -p peitho-core parser::tests::rejects_unsupported_construct_with_line_and_help
```

### Task 6 - Derive a Fallback Slide Key from Title

Goal: keep explicit keys primary and provide a deterministic fallback when no key comment exists.

Files:

- `crates/peitho-core/src/parser.rs`

Test:

```rust
// crates/peitho-core/src/parser.rs
#[test]
fn derives_key_from_first_heading_when_comment_is_absent() {
    let deck = parse_markdown("# Architecture Overview\n\nBody").unwrap();
    assert_eq!(deck.parsed_slides()[0].key.as_str(), "architecture-overview");
}

#[test]
fn explicit_key_wins_over_derived_key() {
    let deck = parse_markdown("<!-- {\"key\":\"arch-1\"} -->\n# Renamed Title").unwrap();
    assert_eq!(deck.parsed_slides()[0].key.as_str(), "arch-1");
}
```

Implementation:

```rust
// crates/peitho-core/src/parser.rs
fn derive_key_from_fragments(fragments: &[SourceFragment], index: usize) -> SlideKey {
    let title = fragments.iter().find_map(SourceFragment::heading_text);
    let raw = title.unwrap_or_else(|| format!("slide-{}", index + 1));
    let slug = raw
        .chars()
        .flat_map(char::to_lowercase)
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    SlideKey::new(if slug.is_empty() { format!("slide-{}", index + 1) } else { slug }).unwrap()
}
```

Verification:

```bash
cargo test -p peitho-core parser::tests::derives_key_from_first_heading_when_comment_is_absent
cargo test -p peitho-core parser::tests::explicit_key_wins_over_derived_key
```

### Task 7 - Extract Slot Contracts from HTML Template

Goal: treat the template itself as the schema by parsing `<slot name accepts arity>`.

Files:

- `crates/peitho-core/src/domain.rs`
- `crates/peitho-core/src/lib.rs`
- `crates/peitho-core/src/template.rs`

Test:

```rust
// crates/peitho-core/src/template.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Accepts, Arity};

    #[test]
    fn extracts_title_body_code_slot_contracts() {
        let html = r#"<section class="slide">
  <h1><slot name="title" accepts="inline" arity="1"></slot></h1>
  <div><slot name="body" accepts="blocks" arity="0..*"></slot></div>
  <figure><slot name="code" accepts="code" arity="0..1"></slot></figure>
</section>"#;

        let template = parse_template("title-body-code", html).unwrap();

        assert_eq!(template.slot("title").unwrap().accepts, Accepts::Inline);
        assert_eq!(template.slot("body").unwrap().arity, Arity::ZeroOrMore);
        assert_eq!(template.slot("code").unwrap().accepts, Accepts::Code);
    }
}
```

Implementation:

```rust
// crates/peitho-core/src/template.rs
use std::{cell::RefCell, collections::BTreeMap, error::Error, rc::Rc};

use lol_html::{element, errors::RewritingError, HtmlRewriter, Settings};

use crate::{
    domain::{Accepts, Arity, SlotContract, SlotName},
    error::{BuildError, ErrorKind, Result},
};

#[derive(Debug, Clone)]
pub struct Template {
    pub name: String,
    pub html: String,
    pub slots: BTreeMap<SlotName, SlotContract>,
}

impl Template {
    pub fn slot(&self, name: &str) -> Option<&SlotContract> {
        SlotName::new(name)
            .ok()
            .and_then(|slot| self.slots.get(&slot))
    }

    pub fn slot_by_name(&self, name: &SlotName) -> Option<&SlotContract> {
        self.slots.get(name)
    }
}

pub fn parse_template(name: impl Into<String>, html: &str) -> Result<Template> {
    let slots = Rc::new(RefCell::new(BTreeMap::new()));
    let sink = slots.clone();
    let mut rewriter = HtmlRewriter::new(
        Settings {
            element_content_handlers: vec![element!("slot", move |el| {
                let contract = SlotContract::from_element(el).map_err(box_build_error)?;
                let key = contract.name.clone();
                let mut slots = sink.borrow_mut();
                if slots.contains_key(&key) {
                    return Err(box_build_error(BuildError::new(
                        ErrorKind::Template,
                        None,
                        format!("duplicate slot '{}'", key.as_str()),
                        "rename one slot so every slot contract has a unique name",
                    )));
                }
                slots.insert(key, contract);
                Ok(())
            })],
            ..Settings::default()
        },
        |_chunk: &[u8]| {},
    );
    rewriter.write(html.as_bytes()).map_err(template_parse_error)?;
    rewriter.end().map_err(template_parse_error)?;

    Ok(Template {
        name: name.into(),
        html: html.to_owned(),
        slots: Rc::try_unwrap(slots).unwrap().into_inner(),
    })
}

fn box_build_error(err: BuildError) -> Box<dyn Error + Send + Sync> {
    Box::new(err)
}

fn template_parse_error(err: RewritingError) -> BuildError {
    match err {
        RewritingError::ContentHandlerError(inner) => match inner.downcast::<BuildError>() {
            Ok(build_error) => *build_error,
            Err(inner) => BuildError::new(
                ErrorKind::Template,
                None,
                format!("template content handler failed: {inner}"),
                "keep the template HTML well-formed and slot attributes complete",
            ),
        },
        other => BuildError::new(
            ErrorKind::Template,
            None,
            format!("failed to parse template: {other}"),
            "keep the template HTML well-formed and slot attributes complete",
        ),
    }
}
```

Verification:

```bash
cargo test -p peitho-core template::tests::extracts_title_body_code_slot_contracts
```

### Task 8 - Reject Invalid Template Slot Contracts

Goal: fail early on duplicate slot names and malformed `name`, `accepts`, or `arity`.

Files:

- `crates/peitho-core/src/domain.rs`
- `crates/peitho-core/src/template.rs`

Test:

```rust
// crates/peitho-core/src/template.rs
#[test]
fn rejects_unknown_accepts_value_with_help() {
    let err = parse_template(
        "bad",
        r#"<slot name="title" accepts="heading" arity="1"></slot>"#,
    )
    .unwrap_err();

    assert_eq!(err.kind, ErrorKind::Template);
    assert!(err.to_string().contains("unknown accepts value 'heading'"));
    assert_eq!(err.help, "use one of inline, blocks, text, code, image, list");
}

#[test]
fn rejects_duplicate_slot_name() {
    let err = parse_template(
        "bad",
        r#"<slot name="title" accepts="inline" arity="1"></slot>
           <slot name="title" accepts="blocks" arity="0..*"></slot>"#,
    )
    .unwrap_err();

    assert!(err.to_string().contains("duplicate slot 'title'"));
}
```

Implementation:

```rust
// crates/peitho-core/src/domain.rs
impl FromStr for Arity {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "1" => Ok(Self::ExactlyOne),
            "0..1" => Ok(Self::ZeroOrOne),
            "1..*" => Ok(Self::OneOrMore),
            "0..*" => Ok(Self::ZeroOrMore),
            other => Err(format!("unknown arity value '{other}'")),
        }
    }
}
```

```rust
// crates/peitho-core/src/template.rs
impl SlotContract {
    fn from_element(el: &mut lol_html::html_content::Element) -> Result<Self> {
        let raw_name = required_attr(el, "name")?;
        let raw_accepts = required_attr(el, "accepts")?;
        let raw_arity = required_attr(el, "arity")?;
        Ok(Self {
            name: SlotName::new(raw_name).map_err(|message| {
                BuildError::new(ErrorKind::Template, None, message, "rename the slot")
            })?,
            accepts: raw_accepts.parse().map_err(|message| {
                BuildError::new(
                    ErrorKind::Template,
                    None,
                    message,
                    "use one of inline, blocks, text, code, image, list",
                )
            })?,
            arity: raw_arity.parse().map_err(|message| {
                BuildError::new(
                    ErrorKind::Template,
                    None,
                    message,
                    "use one of 1, 0..1, 1..*, 0..*",
                )
            })?,
        })
    }
}

fn required_attr(el: &lol_html::html_content::Element, name: &str) -> Result<String> {
    el.get_attribute(name).ok_or_else(|| {
        BuildError::new(
            ErrorKind::Template,
            None,
            format!("slot is missing '{name}'"),
            r#"write <slot name="title" accepts="inline" arity="1"></slot>"#,
        )
    })
}
```

Verification:

```bash
cargo test -p peitho-core template::tests::rejects_unknown_accepts_value_with_help
cargo test -p peitho-core template::tests::rejects_duplicate_slot_name
```

### Task 9 - Map Parsed Content by Convention

Goal: implement the MVP mapping rule: shallowest heading to `title`, code blocks to `code`, remaining blocks to `body`.

Files:

- `crates/peitho-core/src/lib.rs`
- `crates/peitho-core/src/mapping.rs`

Test:

```rust
// crates/peitho-core/src/mapping.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{parser::parse_markdown, template::parse_template};

    #[test]
    fn maps_title_body_and_code_slots_by_convention() {
        let markdown = "<!-- {\"key\":\"arch-1\"} -->\n# Architecture\n\nBody\n\n```rust\nfn main() {}\n```";
        let template = parse_template(
            "title-body-code",
            r#"<slot name="title" accepts="inline" arity="1"></slot>
               <slot name="body" accepts="blocks" arity="0..*"></slot>
               <slot name="code" accepts="code" arity="0..1"></slot>"#,
        )
        .unwrap();

        let mapped = map_by_convention(parse_markdown(markdown).unwrap(), &template).unwrap();
        let slide = &mapped.mapped_slides()[0];

        assert_eq!(slide.slots[&SlotName::new("title").unwrap()].fragments().len(), 1);
        assert_eq!(slide.slots[&SlotName::new("body").unwrap()].fragments().len(), 1);
        assert_eq!(slide.slots[&SlotName::new("code").unwrap()].fragments().len(), 1);
        assert!(slide.unassigned.is_empty());
    }
}
```

Implementation:

```rust
// crates/peitho-core/src/mapping.rs
use std::collections::BTreeMap;

use crate::{
    domain::{FragmentKind, SlotName, SourceFragment},
    error::Result,
    phase::{Deck, Mapped, MappedSlide, MappedSlot, Parsed, UnassignedFragment},
    template::Template,
};

pub fn map_by_convention(deck: Deck<Parsed>, template: &Template) -> Result<Deck<Mapped>> {
    let mut slides = Vec::new();
    for slide in deck.into_parsed_slides() {
        let title_line = shallowest_heading_line(&slide.fragments);
        let mut slots: BTreeMap<SlotName, MappedSlot> = BTreeMap::new();
        let mut unassigned = Vec::new();

        for fragment in slide.fragments {
            let target = match fragment.kind() {
                FragmentKind::Heading { .. } if Some(fragment.line()) == title_line => "title",
                FragmentKind::Code => "code",
                FragmentKind::Heading { .. } | FragmentKind::Paragraph | FragmentKind::List => "body",
                FragmentKind::Image => "body",
                FragmentKind::Text => "body",
            };
            let slot = SlotName::new(target).unwrap();
            if let Some(contract) = template.slot_by_name(&slot).cloned() {
                slots
                    .entry(slot.clone())
                    .or_insert_with(|| MappedSlot::new(contract))
                    .push(fragment);
            } else {
                unassigned.push(UnassignedFragment::new(slot, fragment));
            }
        }

        slides.push(MappedSlide {
            index: slide.index,
            key: slide.key,
            slots,
            unassigned,
        });
    }
    Ok(Deck::mapped(slides))
}

fn shallowest_heading_line(fragments: &[SourceFragment]) -> Option<usize> {
    fragments
        .iter()
        .filter_map(|fragment| match fragment.kind() {
            FragmentKind::Heading { level } => Some((level, fragment.line())),
            _ => None,
        })
        .min_by_key(|(level, line)| (*level, *line))
        .map(|(_level, line)| line)
}
```

Verification:

```bash
cargo test -p peitho-core mapping::tests::maps_title_body_and_code_slots_by_convention
```

### Task 10 - Check `accepts` Type Compatibility

Goal: reject fragments whose content kind does not match the target slot's `accepts`.

Files:

- `crates/peitho-core/src/check.rs`
- `crates/peitho-core/src/lib.rs`

Test:

```rust
// crates/peitho-core/src/check.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{mapping::map_by_convention, parser::parse_markdown, template::parse_template};

    #[test]
    fn rejects_paragraph_in_inline_slot_with_line_and_help() {
        let template = parse_template(
            "bad-body",
            r#"<slot name="title" accepts="inline" arity="1"></slot>
               <slot name="body" accepts="inline" arity="0..*"></slot>"#,
        )
        .unwrap();
        let mapped = map_by_convention(parse_markdown("# Title\n\nBody paragraph").unwrap(), &template).unwrap();

        let err = check_deck(mapped, &template).unwrap_err();

        assert_eq!(err.kind, ErrorKind::Accepts);
        assert_eq!(err.line, Some(3));
        assert!(err.to_string().contains("slot 'body' accepts inline"));
        assert_eq!(err.help, "change the template accepts to 'blocks' or move this content to a blocks slot");
    }
}
```

Implementation:

```rust
// crates/peitho-core/src/check.rs
use std::collections::BTreeMap;

use crate::{
    domain::{Accepts, FragmentKind, SlotName, SourceFragment},
    error::{BuildError, ErrorKind, Result},
    phase::{Checked, CheckedSlide, Deck, Mapped, MappedSlot, UnassignedFragment},
    template::Template,
};

pub fn check_deck(deck: Deck<Mapped>, template: &Template) -> Result<Deck<Checked>> {
    let mut slides = Vec::new();
    for slide in deck.into_mapped_slides() {
        for (slot, mapped_slot) in &slide.slots {
            let contract = mapped_slot.contract();
            for fragment in mapped_slot.fragments() {
                if !accepts_fragment(contract.accepts, fragment) {
                    return Err(BuildError::new(
                        ErrorKind::Accepts,
                        Some(fragment.line()),
                        format!(
                            "slot '{}' accepts {}, but got {}",
                            slot.as_str(),
                            contract.accepts,
                            fragment.kind()
                        ),
                        format!(
                            "change the template accepts to '{}' or move this content to a {} slot",
                            fragment.kind().default_accepts(),
                            fragment.kind().default_accepts()
                        ),
                    ));
                }
            }
        }
        check_arity(&slide.slots, template)?;
        check_no_unassigned(&slide.unassigned)?;
        let checked_slots = slide
            .slots
            .into_iter()
            .map(|(slot, mapped_slot)| (slot, mapped_slot.fragments().to_vec()))
            .collect();
        slides.push(CheckedSlide::new(slide.index, slide.key, checked_slots));
    }
    Ok(Deck::checked(slides))
}

fn accepts_fragment(accepts: Accepts, fragment: &SourceFragment) -> bool {
    matches!(
        (accepts, fragment.kind()),
        (Accepts::Inline, FragmentKind::Heading { .. })
            | (Accepts::Blocks, FragmentKind::Paragraph)
            | (Accepts::Blocks, FragmentKind::List)
            | (Accepts::Text, FragmentKind::Text)
            | (Accepts::Code, FragmentKind::Code)
            | (Accepts::Image, FragmentKind::Image)
            | (Accepts::List, FragmentKind::List)
    )
}
```

Verification:

```bash
cargo test -p peitho-core check::tests::rejects_paragraph_in_inline_slot_with_line_and_help
```

### Task 11 - Check Arity and Required Slot Fill

Goal: reject too many items, too few items, and missing required slots with line/help output.

Files:

- `crates/peitho-core/src/check.rs`

Test:

```rust
// crates/peitho-core/src/check.rs
#[test]
fn rejects_two_code_blocks_for_zero_or_one_code_slot() {
    let markdown = "# Title\n\n```rust\nfn a() {}\n```\n\n```rust\nfn b() {}\n```";
    let template = parse_template(
        "title-body-code",
        r#"<slot name="title" accepts="inline" arity="1"></slot>
           <slot name="body" accepts="blocks" arity="0..*"></slot>
           <slot name="code" accepts="code" arity="0..1"></slot>"#,
    )
    .unwrap();
    let mapped = map_by_convention(parse_markdown(markdown).unwrap(), &template).unwrap();

    let err = check_deck(mapped, &template).unwrap_err();

    assert_eq!(err.kind, ErrorKind::Arity);
    assert_eq!(err.line, Some(3));
    assert!(err.to_string().contains("slot 'code' got 2 item(s)"));
    assert_eq!(err.help, "use a layout with more code capacity or remove one code block");
}

#[test]
fn rejects_missing_required_title_slot() {
    let template = parse_template("title-only", r#"<slot name="title" accepts="inline" arity="1"></slot>"#).unwrap();
    let mapped = map_by_convention(parse_markdown("Body only").unwrap(), &template).unwrap();

    let err = check_deck(mapped, &template).unwrap_err();

    assert_eq!(err.kind, ErrorKind::Arity);
    assert!(err.to_string().contains("slot 'title' got 0 item(s)"));
    assert_eq!(err.help, "add a heading for the title slot");
}
```

Implementation:

```rust
// crates/peitho-core/src/check.rs
fn check_arity(slots: &BTreeMap<SlotName, MappedSlot>, template: &Template) -> Result<()> {
    for (slot, contract) in &template.slots {
        let count = slots.get(slot).map(|mapped| mapped.fragments().len()).unwrap_or(0);
        if !contract.arity.allows(count) {
            let line = slots
                .get(slot)
                .and_then(|mapped| mapped.fragments().first())
                .map(SourceFragment::line);
            let help = if count == 0 && slot.as_str() == "title" {
                "add a heading for the title slot".to_owned()
            } else if count == 0 {
                format!("add content for the {} slot", slot.as_str())
            } else {
                format!(
                    "use a layout with more {} capacity or remove one {} block",
                    slot.as_str(),
                    slot.as_str()
                )
            };
            return Err(BuildError::new(
                ErrorKind::Arity,
                line,
                format!(
                    "slot '{}' got {} item(s), but layout '{}' allows {}",
                    slot.as_str(),
                    count,
                    template.name,
                    contract.arity
                ),
                help,
            ));
        }
    }
    Ok(())
}
```

Verification:

```bash
cargo test -p peitho-core check::tests::rejects_two_code_blocks_for_zero_or_one_code_slot
cargo test -p peitho-core check::tests::rejects_missing_required_title_slot
```

### Task 12 - Check Residual Unassigned Content

Goal: enforce the anti-silent-drop rule by failing when mapping leaves any source content unassigned.

Files:

- `crates/peitho-core/src/check.rs`
- `crates/peitho-core/src/mapping.rs`

Test:

```rust
// crates/peitho-core/src/check.rs
#[test]
fn rejects_unassigned_code_when_template_has_no_code_slot() {
    let template = parse_template(
        "title-body",
        r#"<slot name="title" accepts="inline" arity="1"></slot>
           <slot name="body" accepts="blocks" arity="0..*"></slot>"#,
    )
    .unwrap();
    let markdown = "# Title\n\n```rust\nfn lost() {}\n```";
    let mapped = map_by_convention(parse_markdown(markdown).unwrap(), &template).unwrap();

    let err = check_deck(mapped, &template).unwrap_err();

    assert_eq!(err.kind, ErrorKind::ResidualContent);
    assert_eq!(err.line, Some(3));
    assert!(err.to_string().contains("unassigned content remains"));
    assert_eq!(err.help, "add a 'code' slot to the template or remove the code block");
}

#[test]
fn rejects_unassigned_secondary_heading_as_body_content() {
    let template = parse_template(
        "title-only",
        r#"<slot name="title" accepts="inline" arity="1"></slot>"#,
    )
    .unwrap();
    let markdown = "# Title\n\n## Detail";
    let mapped = map_by_convention(parse_markdown(markdown).unwrap(), &template).unwrap();

    let err = check_deck(mapped, &template).unwrap_err();

    assert_eq!(err.kind, ErrorKind::ResidualContent);
    assert_eq!(err.line, Some(3));
    assert_eq!(err.help, "add a 'body' slot to the template or remove the heading");
}
```

Implementation:

```rust
// crates/peitho-core/src/check.rs
fn check_no_unassigned(unassigned: &[UnassignedFragment]) -> Result<()> {
    if let Some(unassigned) = unassigned.first() {
        let fragment = unassigned.fragment();
        let target = unassigned.expected_slot().as_str();
        return Err(BuildError::new(
            ErrorKind::ResidualContent,
            Some(fragment.line()),
            format!("unassigned content remains for missing '{target}' slot"),
            format!("add a '{target}' slot to the template or remove the {}", fragment.kind().removal_noun()),
        ));
    }
    Ok(())
}
```

Verification:

```bash
cargo test -p peitho-core check::tests::rejects_unassigned_code_when_template_has_no_code_slot
cargo test -p peitho-core check::tests::rejects_unassigned_secondary_heading_as_body_content
```

### Task 13 - Render Checked Slides to HTML

Goal: render only `Deck<Checked>` into HTML containing `data-slide-key` and `.slot-*`.

Files:

- `crates/peitho-core/src/lib.rs`
- `crates/peitho-core/src/render.rs`

Test:

```rust
// crates/peitho-core/src/render.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        check::check_deck,
        mapping::map_by_convention,
        parser::parse_markdown,
        template::parse_template,
    };

    #[test]
    fn renders_checked_slide_with_key_and_slot_classes() {
        let markdown = "<!-- {\"key\":\"arch-1\"} -->\n# Architecture\n\nBody\n\n```rust\nfn main() {}\n```";
        let template = parse_template(
            "title-body-code",
            r#"<section class="slide">
  <h1><slot name="title" accepts="inline" arity="1"></slot></h1>
  <div class="body"><slot name="body" accepts="blocks" arity="0..*"></slot></div>
  <figure class="code"><slot name="code" accepts="code" arity="0..1"></slot></figure>
</section>"#,
        )
        .unwrap();
        let checked = check_deck(map_by_convention(parse_markdown(markdown).unwrap(), &template).unwrap(), &template).unwrap();

        let rendered = render_deck(checked, &template).unwrap();
        let html = rendered.slides()[0].html();

        assert!(html.contains(r#"data-slide-key="arch-1""#));
        assert!(html.contains(r#"class="slot-title""#));
        assert!(html.contains(r#"class="slot-body""#));
        assert!(html.contains(r#"class="slot-code""#));
        assert!(html.contains("fn main() {}"));
    }
}
```

Implementation:

```rust
// crates/peitho-core/src/render.rs
use std::error::Error;

use html_escape::encode_text;
use lol_html::{element, errors::RewritingError, html_content::ContentType, HtmlRewriter, Settings};
use pulldown_cmark::{html, Options, Parser};

use crate::{
    domain::{FragmentKind, RenderedSlide, SlotName, SourceFragment},
    error::{BuildError, ErrorKind, Result},
    phase::{Deck, Checked, Rendered},
    template::Template,
};

pub fn render_deck(deck: Deck<Checked>, template: &Template) -> Result<Deck<Rendered>> {
    let mut slides = Vec::new();
    for slide in deck.into_checked_slides() {
        let html = render_slide(slide.key(), slide.slots(), template)?;
        slides.push(RenderedSlide::new(slide.index(), slide.key().clone(), html));
    }
    Ok(Deck::rendered(slides, String::new()))
}

fn render_slide(
    key: &crate::domain::SlideKey,
    slots: &std::collections::BTreeMap<SlotName, Vec<SourceFragment>>,
    template: &Template,
) -> Result<String> {
    let mut output = Vec::new();
    let key_value = key.as_str().to_owned();
    let slot_values = slots.clone();
    let mut rewriter = HtmlRewriter::new(
        Settings {
            element_content_handlers: vec![
                element!("section", move |el| {
                    el.set_attribute("data-slide-key", &key_value)?;
                    let existing = el.get_attribute("class").unwrap_or_default();
                    let class = if existing.split_whitespace().any(|part| part == "peitho-slide") {
                        existing
                    } else if existing.is_empty() {
                        "peitho-slide".to_owned()
                    } else {
                        format!("{existing} peitho-slide")
                    };
                    el.set_attribute("class", &class)?;
                    Ok(())
                }),
                element!("slot", move |el| {
                    let raw_name = el.get_attribute("name").ok_or_else(|| {
                        box_build_error(BuildError::new(
                            ErrorKind::Template,
                            None,
                            "slot is missing 'name'",
                            "add a name attribute to the slot",
                        ))
                    })?;
                    let slot = SlotName::new(raw_name).map_err(|message| {
                        box_build_error(BuildError::new(ErrorKind::Template, None, message, "rename the slot"))
                    })?;
                    let fragments = slot_values.get(&slot).cloned().unwrap_or_default();
                    el.replace(&render_slot(&slot, &fragments), ContentType::Html);
                    Ok(())
                }),
            ],
            ..Settings::default()
        },
        |chunk: &[u8]| output.extend_from_slice(chunk),
    );
    rewriter.write(template.html.as_bytes()).map_err(render_error)?;
    rewriter.end().map_err(render_error)?;
    String::from_utf8(output).map_err(|err| {
        BuildError::new(
            ErrorKind::Template,
            None,
            format!("rendered HTML is not UTF-8: {err}"),
            "keep templates and generated fragments as UTF-8",
        )
    })
}

fn render_slot(slot: &SlotName, fragments: &[SourceFragment]) -> String {
    let class_name = slot.class_name();
    match fragments.first().map(SourceFragment::kind) {
        Some(FragmentKind::Heading { .. }) => {
            let text = fragments.iter().map(SourceFragment::plain_text).collect::<Vec<_>>().join(" ");
            format!(r#"<span class="{class_name}">{}</span>"#, encode_text(&text))
        }
        Some(FragmentKind::Code) => {
            let code = fragments.iter().map(SourceFragment::code_text).collect::<Vec<_>>().join("\n");
            format!(r#"<pre class="{class_name}"><code>{}</code></pre>"#, encode_text(&code))
        }
        _ => {
            let markdown = fragments.iter().map(SourceFragment::markdown).collect::<Vec<_>>().join("\n\n");
            let mut body = String::new();
            html::push_html(&mut body, Parser::new_ext(&markdown, Options::empty()));
            format!(r#"<div class="{class_name}">{body}</div>"#)
        }
    }
}

fn box_build_error(err: BuildError) -> Box<dyn Error + Send + Sync> {
    Box::new(err)
}

fn render_error(err: RewritingError) -> BuildError {
    match err {
        RewritingError::ContentHandlerError(inner) => match inner.downcast::<BuildError>() {
            Ok(build_error) => *build_error,
            Err(inner) => BuildError::new(
                ErrorKind::Template,
                None,
                format!("render content handler failed: {inner}"),
                "keep slot elements well-formed and avoid malformed HTML in the template",
            ),
        },
        other => BuildError::new(
            ErrorKind::Template,
            None,
            format!("failed to render template: {other}"),
            "keep slot elements well-formed and avoid malformed HTML in the template",
        ),
    }
}

pub fn render_index(slides: &[RenderedSlide]) -> String {
    let body = slides
        .iter()
        .map(RenderedSlide::html)
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <link rel="stylesheet" href="peitho.css">
  <title>Peitho Deck</title>
</head>
<body>
{body}
</body>
</html>"#
    )
}
```

Verification:

```bash
cargo test -p peitho-core render::tests::renders_checked_slide_with_key_and_slot_classes
cargo test -p peitho-core --doc
```

### Task 14 - Combine Base CSS and Valid Overrides

Goal: append `overrides.css` after `base.css` and accept selectors targeting known slide keys and slots.

Files:

- `crates/peitho-core/src/lib.rs`
- `crates/peitho-core/src/theme.rs`

Test:

```rust
// crates/peitho-core/src/theme.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{domain::{SlideKey, SlotName}, template::parse_template};

    #[test]
    fn appends_valid_override_after_base_css() {
        let template = parse_template(
            "title-body-code",
            r#"<slot name="title" accepts="inline" arity="1"></slot>
               <slot name="code" accepts="code" arity="0..1"></slot>"#,
        )
        .unwrap();
        let keys = [SlideKey::new("arch-1").unwrap()];
        let css = build_theme_css(
            ".slot-code { color: black; }",
            r#"[data-slide-key="arch-1"] .slot-code { outline: 3px solid #f40; }"#,
            keys.iter(),
            &template,
        )
        .unwrap();

        assert!(css.contains(".slot-code { color: black; }"));
        assert!(css.ends_with(r#"[data-slide-key="arch-1"] .slot-code { outline: 3px solid #f40; }"#));
        assert!(template.slots.contains_key(&SlotName::new("code").unwrap()));
    }
}
```

Implementation:

```rust
// crates/peitho-core/src/theme.rs
use std::collections::BTreeSet;

use crate::{
    domain::{SlideKey, SlotName},
    error::{BuildError, ErrorKind, Result},
    template::Template,
};

pub fn build_theme_css<'a>(
    base_css: &str,
    overrides_css: &str,
    slide_keys: impl Iterator<Item = &'a SlideKey>,
    template: &Template,
) -> Result<String> {
    let known_keys = slide_keys.map(|key| key.as_str().to_owned()).collect::<BTreeSet<_>>();
    validate_override_selectors(overrides_css, &known_keys, template)?;
    Ok(format!("{}\n\n{}", base_css.trim_end(), overrides_css.trim()))
}
```

Verification:

```bash
cargo test -p peitho-core theme::tests::appends_valid_override_after_base_css
```

### Task 15 - Reject Override CSS with Unknown Keys or Slots

Goal: make stale per-slide CSS a build error when a selector references a missing slide key or `.slot-*`.

Files:

- `crates/peitho-core/src/theme.rs`

Test:

```rust
// crates/peitho-core/src/theme.rs
#[test]
fn rejects_unknown_slide_key_in_override_selector() {
    let template = parse_template("title", r#"<slot name="title" accepts="inline" arity="1"></slot>"#).unwrap();
    let keys = [SlideKey::new("arch-1").unwrap()];

    let err = build_theme_css(
        "",
        r#"[data-slide-key="missing"] .slot-title { color: red; }"#,
        keys.iter(),
        &template,
    )
    .unwrap_err();

    assert_eq!(err.kind, ErrorKind::Theme);
    assert_eq!(err.line, Some(1));
    assert!(err.to_string().contains("unknown slide key 'missing'"));
    assert_eq!(err.help, "use one of: arch-1");
}

#[test]
fn rejects_unknown_slot_class_in_override_selector() {
    let template = parse_template("title", r#"<slot name="title" accepts="inline" arity="1"></slot>"#).unwrap();
    let keys = [SlideKey::new("arch-1").unwrap()];

    let err = build_theme_css(
        "",
        r#"[data-slide-key="arch-1"] .slot-code { color: red; }"#,
        keys.iter(),
        &template,
    )
    .unwrap_err();

    assert_eq!(err.kind, ErrorKind::Theme);
    assert_eq!(err.line, Some(1));
    assert!(err.to_string().contains("unknown slot class '.slot-code'"));
    assert_eq!(err.help, "use one of: .slot-title");
}
```

Implementation:

```rust
// crates/peitho-core/src/theme.rs
fn validate_override_selectors(
    css: &str,
    known_keys: &BTreeSet<String>,
    template: &Template,
) -> Result<()> {
    let known_slots = template
        .slots
        .keys()
        .map(SlotName::class_name)
        .collect::<BTreeSet<_>>();

    for (line_index, line) in css.lines().enumerate() {
        let line_no = line_index + 1;
        let selector = line.split('{').next().unwrap_or(line);
        for key in extract_attr_values(selector, r#"[data-slide-key=""#, r#""]"#) {
            if !known_keys.contains(&key) {
                return Err(BuildError::new(
                    ErrorKind::Theme,
                    Some(line_no),
                    format!("unknown slide key '{key}' in override selector"),
                    format!("use one of: {}", known_keys.iter().cloned().collect::<Vec<_>>().join(", ")),
                ));
            }
        }
        for class in extract_slot_classes(selector) {
            if !known_slots.contains(&class) {
                return Err(BuildError::new(
                    ErrorKind::Theme,
                    Some(line_no),
                    format!("unknown slot class '.{class}' in override selector"),
                    format!("use one of: {}", known_slots.iter().map(|s| format!(".{s}")).collect::<Vec<_>>().join(", ")),
                ));
            }
        }
    }
    Ok(())
}

fn extract_slot_classes(selector: &str) -> Vec<String> {
    selector
        .split('.')
        .skip(1)
        .filter_map(|tail| tail.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-')).next())
        .filter(|class| class.starts_with("slot-"))
        .map(str::to_owned)
        .collect()
}

fn extract_attr_values(selector: &str, prefix: &str, suffix: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut rest = selector;
    while let Some(start) = rest.find(prefix) {
        let after_prefix = &rest[start + prefix.len()..];
        if let Some(end) = after_prefix.find(suffix) {
            values.push(after_prefix[..end].to_owned());
            rest = &after_prefix[end + suffix.len()..];
        } else {
            break;
        }
    }
    values
}
```

Verification:

```bash
cargo test -p peitho-core theme::tests::rejects_unknown_slide_key_in_override_selector
cargo test -p peitho-core theme::tests::rejects_unknown_slot_class_in_override_selector
```

### Task 16 - Wire `peitho build` End to End

Goal: make `peitho build <md>` read Markdown, template, base CSS, overrides CSS, then write deterministic HTML and CSS.

Files:

- `crates/peitho-core/src/lib.rs`
- `crates/peitho/src/main.rs`
- `crates/peitho/tests/build.rs`

Test:

```rust
// crates/peitho/tests/build.rs
use std::fs;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

#[test]
fn build_writes_index_html_and_css() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let template = dir.path().join("title-body-code.html");
    let base = dir.path().join("base.css");
    let overrides = dir.path().join("overrides.css");
    let out = dir.path().join("dist");

    fs::write(&deck, "<!-- {\"key\":\"arch-1\"} -->\n# Architecture\n\nBody\n\n```rust\nfn main() {}\n```").unwrap();
    fs::write(&template, r#"<section class="slide"><h1><slot name="title" accepts="inline" arity="1"></slot></h1><slot name="body" accepts="blocks" arity="0..*"></slot><slot name="code" accepts="code" arity="0..1"></slot></section>"#).unwrap();
    fs::write(&base, ".slot-title { font-weight: 700; }").unwrap();
    fs::write(&overrides, r#"[data-slide-key="arch-1"] .slot-code { outline: 3px solid #f40; }"#).unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
            "--template",
            template.to_str().unwrap(),
            "--base-css",
            base.to_str().unwrap(),
            "--overrides-css",
            overrides.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("built 1 slide"));

    assert!(fs::read_to_string(out.join("index.html")).unwrap().contains(r#"data-slide-key="arch-1""#));
    assert!(fs::read_to_string(out.join("peitho.css")).unwrap().contains(r#"[data-slide-key="arch-1"] .slot-code"#));
}
```

Implementation:

```rust
// crates/peitho-core/src/lib.rs
pub mod check;
pub mod domain;
pub mod error;
pub mod mapping;
pub mod parser;
pub mod phase;
pub mod render;
pub mod template;
pub mod theme;

pub use check::check_deck;
pub use error::{BuildError, Result};
pub use mapping::map_by_convention;
pub use parser::parse_markdown;
pub use phase::{require_checked_for_render, Deck, Mapped};
pub use render::{render_deck, render_index};
pub use template::parse_template;
pub use theme::build_theme_css;
```

```rust
// crates/peitho/src/main.rs
#[derive(Debug, Subcommand)]
enum Command {
    Build {
        input: PathBuf,
        #[arg(long, default_value = "templates/title-body-code.html")]
        template: PathBuf,
        #[arg(long, default_value = "themes/base.css")]
        base_css: PathBuf,
        #[arg(long, default_value = "themes/overrides.css")]
        overrides_css: PathBuf,
        #[arg(long, default_value = "dist")]
        out: PathBuf,
    },
}

fn build(input: &Path, template_path: &Path, base_path: &Path, overrides_path: &Path, out: &Path) -> miette::Result<()> {
    let markdown = fs::read_to_string(input).into_diagnostic()?;
    let template_html = fs::read_to_string(template_path).into_diagnostic()?;
    let base_css = fs::read_to_string(base_path).into_diagnostic()?;
    let overrides_css = fs::read_to_string(overrides_path).into_diagnostic()?;

    let template = core(peitho_core::parse_template("title-body-code", &template_html))?;
    let parsed = core(peitho_core::parse_markdown(&markdown))?;
    let mapped = core(peitho_core::map_by_convention(parsed, &template))?;
    let checked = core(peitho_core::check_deck(mapped, &template))?;
    let keys = checked.slide_keys().collect::<Vec<_>>();
    let slide_count = checked.slide_count();
    let css = core(peitho_core::build_theme_css(&base_css, &overrides_css, keys.into_iter(), &template))?;
    let rendered = core(peitho_core::render_deck(checked, &template))?;

    fs::create_dir_all(out).into_diagnostic()?;
    fs::write(out.join("peitho.css"), css).into_diagnostic()?;
    fs::write(out.join("index.html"), peitho_core::render_index(rendered.slides())).into_diagnostic()?;
    println!("built {} slide(s) into {}", slide_count, out.display());
    Ok(())
}

fn core<T>(result: peitho_core::Result<T>) -> miette::Result<T> {
    result.map_err(|err| miette::miette!("{err}"))
}
```

Verification:

```bash
cargo test -p peitho --test build build_writes_index_html_and_css
cargo run -p peitho -- build --help
```

### Task 17 - Surface Contract Failures Through the CLI

Goal: CLI failures print the same line/help information as core errors.

Files:

- `crates/peitho/src/main.rs`
- `crates/peitho/tests/build.rs`

Test:

```rust
// crates/peitho/tests/build.rs
#[test]
fn build_fails_with_line_and_help_for_contract_violation() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let template = dir.path().join("title-body-code.html");
    let base = dir.path().join("base.css");
    let overrides = dir.path().join("overrides.css");
    let out = dir.path().join("dist");

    fs::write(&deck, "# Architecture\n\n```rust\nfn a() {}\n```\n\n```rust\nfn b() {}\n```").unwrap();
    fs::write(&template, r#"<slot name="title" accepts="inline" arity="1"></slot><slot name="code" accepts="code" arity="0..1"></slot>"#).unwrap();
    fs::write(&base, "").unwrap();
    fs::write(&overrides, "").unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            deck.to_str().unwrap(),
            "--template",
            template.to_str().unwrap(),
            "--base-css",
            base.to_str().unwrap(),
            "--overrides-css",
            overrides.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("line 3"))
        .stderr(predicate::str::contains("slot 'code' got 2 item(s)"))
        .stderr(predicate::str::contains("help: use a layout with more code capacity or remove one code block"));
}
```

Implementation:

```rust
// crates/peitho/src/main.rs
fn main() -> miette::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Build {
            input,
            template,
            base_css,
            overrides_css,
            out,
        } => build(&input, &template, &base_css, &overrides_css, &out),
    }
}
```

Verification:

```bash
cargo test -p peitho --test build build_fails_with_line_and_help_for_contract_violation
```

### Task 18 - Add the Minimal Example and Manual Vertical Slice Check

Goal: commit the example deck, base template, base CSS, and one override line that targets `arch-1`.

Files:

- `examples/deck.md`
- `templates/title-body-code.html`
- `themes/base.css`
- `themes/overrides.css`

Test:

```rust
// crates/peitho/tests/build.rs
#[test]
fn repository_example_builds() {
    let out = tempfile::tempdir().unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .args([
            "build",
            "examples/deck.md",
            "--template",
            "templates/title-body-code.html",
            "--base-css",
            "themes/base.css",
            "--overrides-css",
            "themes/overrides.css",
            "--out",
            out.path().to_str().unwrap(),
        ])
        .assert()
        .success();

    let html = fs::read_to_string(out.path().join("index.html")).unwrap();
    let css = fs::read_to_string(out.path().join("peitho.css")).unwrap();
    assert!(html.contains(r#"data-slide-key="arch-1""#));
    assert!(html.contains(r#"class="slot-code""#));
    assert!(css.contains(r#"[data-slide-key="arch-1"] .slot-code"#));
}
```

Implementation:

```markdown
<!-- examples/deck.md -->
<!-- {"key":"arch-1"} -->
# Peitho Architecture

Markdown is the source of truth, while HTML and CSS own layout.

```rust
type Phase = Parsed | Mapped | Checked | Rendered;
```
```

```html
<!-- templates/title-body-code.html -->
<section class="peitho-slide">
  <h1><slot name="title" accepts="inline" arity="1"></slot></h1>
  <div class="body">
    <slot name="body" accepts="blocks" arity="0..*"></slot>
  </div>
  <figure class="code">
    <slot name="code" accepts="code" arity="0..1"></slot>
  </figure>
</section>
```

```css
/* themes/base.css */
.peitho-slide {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(18rem, 0.8fr);
  gap: 2rem;
  min-height: 100vh;
  padding: 4rem;
  box-sizing: border-box;
}

.slot-title {
  display: inline-block;
}

.slot-body {
  font-size: 1.4rem;
  line-height: 1.5;
}

.slot-code {
  display: block;
  padding: 1rem;
  background: #111;
  color: #f7f7f7;
}
```

```css
/* themes/overrides.css */
[data-slide-key="arch-1"] .slot-code { outline: 3px solid #f40; }
```

Verification:

```bash
cargo test --workspace
cargo run -p peitho -- build examples/deck.md --template templates/title-body-code.html --base-css themes/base.css --overrides-css themes/overrides.css --out dist
rg -n 'data-slide-key="arch-1"|class="slot-code"' dist/index.html
rg -n '\[data-slide-key="arch-1"\] \.slot-code' dist/peitho.css
```

## Summary

全18タスクで、まず `.gitignore` と完全な Cargo workspace ファイルを作り、`peitho-core` に契約型、line/help エラー、外部から `Checked` を構築できない typestate を置く。その後、raw Markdown slice と list depth を保持する parser、`RewritingError::ContentHandlerError` から `BuildError` を直接復元する template/render、契約を運ぶ mapping、4段 check、checked-only render、theme override 検査を順に足し、最後に `peitho build` と `examples/deck.md` で `arch-1` の override が HTML/CSS に到達することを確認する。
