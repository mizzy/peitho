# Peitho Milestone 2 Multi-Slide Distribution Plan

<!-- constrained-by ../PEITHO_KICKOFF.md#13-projecting-via-emit-from-a-shared-intermediate-representation -->
<!-- constrained-by ../PEITHO_KICKOFF.md#14-two-entry-point-structure-single-underlying-substance -->
<!-- constrained-by ../PEITHO_KICKOFF.md#17-the-manifest--notes-schema-and-the-single-source-of-truth-for-the-contract -->

## Purpose

Milestone 2 parses and checks multiple slides from a single Markdown deck, and generates the §14 distribution `dist/` layout. Parsing, mapping, and checking happen only once, and `slides/` fragments, `manifest.json`, the distribution `index.html`, and `peitho.css` are emitted from that checked model.

Scope is limited to the Rust-side `build`. `present.html`, `notes.json`, the `present` / `publish` commands, `--watch`, explicit fenced div slots, multiple template selection, and `ts-rs` / `schemars` generation are not built here.

## File Structure Map

| Path | Responsibility | Depends on |
| --- | --- | --- |
| `crates/peitho-core/src/domain.rs` | `SlideKey` slug constraints, accessors for fragment and `RenderedSlide` | none |
| `crates/peitho-core/src/error.rs` | `BuildError` display with slide context | `domain.rs` |
| `crates/peitho-core/src/phase.rs` | Multi-slide accessors for `Deck<Parsed/Mapped/Checked/Rendered>`, checked title extraction | `domain.rs` |
| `crates/peitho-core/src/parser.rs` | Slide splitting on `---` top-level thematic breaks, key ownership, duplicate key checking | `domain.rs`, `error.rs`, `phase.rs` |
| `crates/peitho-core/src/mapping.rs` | Convention mapping per slide | `phase.rs`, `template.rs` |
| `crates/peitho-core/src/check.rs` | Four-stage check per slide with slide context attached | `error.rs`, `phase.rs`, `template.rs` |
| `crates/peitho-core/src/render.rs` | Render checked slides into HTML fragments, generate distribution `index.html` | `domain.rs`, `phase.rs`, `template.rs` |
| `crates/peitho-core/src/manifest.rs` | §17 `manifest.json` Rust source of truth and JSON generation | `phase.rs`, `domain.rs`, `serde` |
| `crates/peitho-core/src/theme.rs` | Override checking against every slide key and template slot | `phase.rs`, `template.rs` |
| `crates/peitho-core/src/lib.rs` | `manifest` exports, distribution index export | all core modules |
| `crates/peitho/src/main.rs` | `peitho build` writes `dist/slides/*.html`, `manifest.json`, `index.html`, `peitho.css` | `peitho-core` |
| `crates/peitho/tests/build.rs` | CLI black-box tests: multi-slide dist, manifest markers, error context | CLI binary |
| `examples/deck.md` | 3-slide deck fixture: explicit key, derived key, key comment immediately after a delimiter | parser, CLI |
| `themes/overrides.css` | Override targeting known keys within the multi-slide deck | manifest keys |

Dependency direction remains one-way: CLI writes files, `peitho-core` owns parsing, typestate, checking, rendering, manifest schema, and distribution index string generation. `manifest.json` is serialized from `peitho-core` types, not assembled as ad hoc CLI strings.

## Implementation Tasks

### Task 1 - Add Slide Context to BuildError

Goal: every parse/check/theme error can optionally report `slide N ('key')` while preserving existing line/help output.

Files:

- `crates/peitho-core/src/error.rs`

Test:

```rust
// crates/peitho-core/src/error.rs
#[test]
fn display_includes_slide_context_before_line() {
    let err = BuildError::new(
        ErrorKind::Arity,
        Some(12),
        "slot 'code' got 2 item(s), but layout 'title-body-code' allows 0..1",
        "use a layout with more code capacity or remove one code block",
    )
    .with_slide(2, Some("arch-1"));

    assert_eq!(err.slide, Some(ErrorSlide { number: 2, key: Some("arch-1".to_owned()) }));
    assert!(err.to_string().contains("slide 2 ('arch-1'), line 12"));
    assert!(err.to_string().contains("help: use a layout with more code capacity or remove one code block"));
}
```

Implementation:

```rust
// crates/peitho-core/src/error.rs
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorSlide {
    pub number: usize,
    pub key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildError {
    pub kind: ErrorKind,
    pub line: Option<usize>,
    pub message: String,
    pub help: String,
    pub slide: Option<ErrorSlide>,
}

impl BuildError {
    pub fn new(
        kind: ErrorKind,
        line: Option<usize>,
        message: impl Into<String>,
        help: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            line,
            message: message.into(),
            help: help.into(),
            slide: None,
        }
    }

    pub fn with_slide(mut self, number: usize, key: Option<&str>) -> Self {
        self.slide = Some(ErrorSlide {
            number,
            key: key.map(str::to_owned),
        });
        self
    }
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (&self.slide, self.line) {
            (Some(slide), Some(line)) => match &slide.key {
                Some(key) => write!(f, "slide {} ('{}'), line {}: {}\n  = help: {}", slide.number, key, line, self.message, self.help),
                None => write!(f, "slide {}, line {}: {}\n  = help: {}", slide.number, line, self.message, self.help),
            },
            (Some(slide), None) => match &slide.key {
                Some(key) => write!(f, "slide {} ('{}'): {}\n  = help: {}", slide.number, key, self.message, self.help),
                None => write!(f, "slide {}: {}\n  = help: {}", slide.number, self.message, self.help),
            },
            (None, Some(line)) => write!(f, "line {}: {}\n  = help: {}", line, self.message, self.help),
            (None, None) => write!(f, "{}\n  = help: {}", self.message, self.help),
        }
    }
}
```

Verification:

```bash
cargo test -p peitho-core error::tests::display_includes_slide_context_before_line
```

### Task 2 - Represent Parsed Slide Key Source

Goal: retain key origin and line so duplicate-key errors can distinguish explicit and derived keys.

Files:

- `crates/peitho-core/src/phase.rs`
- `crates/peitho-core/src/parser.rs`

Test:

```rust
// crates/peitho-core/src/parser.rs
#[test]
fn parsed_slide_records_explicit_and_derived_key_sources() {
    let deck = parse_markdown("<!-- {\"key\":\"arch-1\"} -->\n# Architecture\n\n---\n# Derived Title").unwrap();
    let slides = deck.parsed_slides();

    assert_eq!(slides[0].key.as_str(), "arch-1");
    assert_eq!(slides[0].key_source, KeySource::Explicit { line: 1 });
    assert_eq!(slides[1].key.as_str(), "derived-title");
    assert_eq!(slides[1].key_source, KeySource::Derived { line: Some(5) });
}

#[test]
fn derives_slide_number_key_when_slide_has_no_heading() {
    let deck = parse_markdown("Body only\n\n---\nSecond body").unwrap();

    assert_eq!(deck.parsed_slides()[0].key.as_str(), "slide-1");
    assert_eq!(deck.parsed_slides()[0].key_source, KeySource::Derived { line: None });
    assert_eq!(deck.parsed_slides()[1].key.as_str(), "slide-2");
    assert_eq!(deck.parsed_slides()[1].key_source, KeySource::Derived { line: None });
}
```

Implementation:

```rust
// crates/peitho-core/src/phase.rs
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeySource {
    Explicit { line: usize },
    Derived { line: Option<usize> },
}

#[derive(Debug, Clone)]
pub struct ParsedSlide {
    pub index: usize,
    pub key: SlideKey,
    pub key_source: KeySource,
    pub fragments: Vec<SourceFragment>,
}
```

```rust
// crates/peitho-core/src/parser.rs
fn derived_key_source(fragments: &[SourceFragment]) -> KeySource {
    let line = fragments
        .iter()
        .find(|fragment| matches!(fragment.kind(), FragmentKind::Heading { .. }))
        .map(SourceFragment::line);
    KeySource::Derived { line }
}

fn derive_key_from_fragments(fragments: &[SourceFragment], index: usize) -> SlideKey {
    let raw = fragments
        .iter()
        .find_map(SourceFragment::heading_text)
        .unwrap_or_else(|| format!("slide-{}", index + 1));
    let slug = raw
        .chars()
        .flat_map(char::to_lowercase)
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    SlideKey::new(if slug.is_empty() { format!("slide-{}", index + 1) } else { slug })
        .expect("derived slide keys use lowercase ascii, digits, or '-'")
}
```

Verification:

```bash
cargo test -p peitho-core parser::tests::parsed_slide_records_explicit_and_derived_key_sources
cargo test -p peitho-core parser::tests::derives_slide_number_key_when_slide_has_no_heading
```

### Task 3 - Split Markdown on Top-Level Thematic Breaks

Goal: support `---` as a slide delimiter while keeping other unsupported constructs as errors.

Files:

- `crates/peitho-core/src/parser.rs`

Test:

```rust
// crates/peitho-core/src/parser.rs
#[test]
fn splits_slides_on_thematic_break() {
    let deck = parse_markdown("# One\n\n---\n\n# Two\n\nBody").unwrap();
    let slides = deck.parsed_slides();

    assert_eq!(slides.len(), 2);
    assert_eq!(slides[0].index, 0);
    assert_eq!(slides[0].key.as_str(), "one");
    assert_eq!(slides[1].index, 1);
    assert_eq!(slides[1].key.as_str(), "two");
    assert_eq!(slides[1].fragments[0].line(), 5);
}

#[test]
fn rejects_empty_deck_after_splitting_delimiters() {
    let err = parse_markdown("  \n\n---\n\n---\n").unwrap_err();

    assert_eq!(err.kind, ErrorKind::Parse);
    assert_eq!(err.line, None);
    assert!(err.to_string().contains("deck has no slides"));
    assert_eq!(err.help, "add at least one slide with content before building");
}
```

Implementation:

```rust
// crates/peitho-core/src/parser.rs
#[derive(Debug, Clone, Copy)]
struct SlideRange {
    start: usize,
    end: usize,
}

fn split_slide_ranges(source: &str) -> Result<Vec<SlideRange>> {
    let mut ranges = Vec::new();
    let mut start = 0usize;
    let mut list_depth = 0usize;

    for (event, range) in Parser::new_ext(source, parser_options()).into_offset_iter() {
        match event {
            Event::Start(Tag::List(_)) => list_depth += 1,
            Event::End(TagEnd::List(_)) => list_depth = list_depth.saturating_sub(1),
            Event::Rule if list_depth == 0 => {
                ranges.push(SlideRange { start, end: range.start });
                start = range.end;
            }
            Event::Rule => {}
            _ => {}
        }
    }

    ranges.push(SlideRange { start, end: source.len() });
    Ok(ranges
        .into_iter()
        .filter(|range| !source[range.start..range.end].trim().is_empty())
        .collect())
}

pub fn parse_markdown(source: &str) -> Result<Deck<Parsed>> {
    let ranges = split_slide_ranges(source)?;
    if ranges.is_empty() {
        return Err(BuildError::new(
            ErrorKind::Parse,
            None,
            "deck has no slides",
            "add at least one slide with content before building",
        ));
    }

    let mut slides = Vec::new();
    for (index, range) in ranges.into_iter().enumerate() {
        slides.push(parse_slide(source, range, index)?);
    }
    Ok(Deck::parsed(slides))
}

fn parser_options() -> Options {
    Options::ENABLE_TABLES | Options::ENABLE_FOOTNOTES
}
```

Verification:

```bash
cargo test -p peitho-core parser::tests::splits_slides_on_thematic_break
cargo test -p peitho-core parser::tests::rejects_empty_deck_after_splitting_delimiters
```

### Task 4 - Parse Each Slide with Global Line Numbers

Goal: reuse the current pulldown parser per slide range, but keep all error and fragment line numbers relative to the full source. The accepted diff is only global offset correction, leading key ownership, and `Rule` rejection; the M1 event validation structure remains intact with `Html` checked before `_ if list_depth > 0`, `unsupported_tag` preserved, `text outside block` preserved, heading text trimmed, and the final `other => Err(...)` arm preserved.

Files:

- `crates/peitho-core/src/parser.rs`

Test:

```rust
// crates/peitho-core/src/parser.rs
#[test]
fn unsupported_construct_after_delimiter_reports_global_line_and_slide() {
    let err = parse_markdown("# One\n\n---\n\n# Two\n\n> quote").unwrap_err();

    assert_eq!(err.kind, ErrorKind::Parse);
    assert_eq!(err.line, Some(7));
    assert!(err.to_string().contains("slide 2 ('two'), line 7"));
    assert!(err.to_string().contains("unsupported construct 'blockquote'"));
}

#[test]
fn unsupported_image_after_delimiter_is_not_silently_dropped() {
    let err = parse_markdown("# One\n\n---\n\n![alt](image.png)").unwrap_err();

    assert_eq!(err.kind, ErrorKind::Parse);
    assert_eq!(err.line, Some(5));
    assert!(err.to_string().contains("slide 2 ('slide-2'), line 5"));
    assert!(err.to_string().contains("unsupported construct 'image'"));
}

#[test]
fn unsupported_table_after_delimiter_is_not_silently_dropped() {
    let err = parse_markdown("# One\n\n---\n\n| a |\n| - |").unwrap_err();

    assert_eq!(err.kind, ErrorKind::Parse);
    assert_eq!(err.line, Some(5));
    assert!(err.to_string().contains("slide 2 ('slide-2'), line 5"));
    assert!(err.to_string().contains("unsupported construct 'table'"));
}
```

Implementation:

```rust
// crates/peitho-core/src/parser.rs
fn parse_slide(source: &str, range: SlideRange, index: usize) -> Result<ParsedSlide> {
    let slice = &source[range.start..range.end];
    let mut explicit_key: Option<(SlideKey, usize)> = None;
    let mut fragments = Vec::new();
    let mut block: Option<OpenBlock> = None;
    let mut list_depth = 0usize;
    let mut list_start = None;
    let mut seen_content = false;

    for (event, local_range) in Parser::new_ext(slice, parser_options()).into_offset_iter() {
        let global_start = range.start + local_range.start;
        let global_end = range.start + local_range.end;
        let line = line_for_offset(source, global_start);
        match event {
            Event::Rule => {
                let err = unsupported_construct(line, "thematic break inside slide");
                return Err(attach_slide_context(err, index, explicit_key.as_ref(), &fragments));
            }
            Event::Start(Tag::List(_)) => {
                if list_depth == 0 {
                    list_start = Some(global_start);
                }
                list_depth += 1;
            }
            Event::End(TagEnd::List(_)) => {
                if list_depth == 0 {
                    let err = unsupported_construct(line, "list end without list start");
                    return Err(attach_slide_context(err, index, explicit_key.as_ref(), &fragments));
                }
                list_depth -= 1;
                if list_depth == 0 {
                    let Some(start) = list_start.take() else {
                        let err = unsupported_construct(line, "list end without list start");
                        return Err(attach_slide_context(err, index, explicit_key.as_ref(), &fragments));
                    };
                    fragments.push(SourceFragment::list(
                        line_for_offset(source, start),
                        source_slice(source, start, global_end),
                    ));
                    seen_content = true;
                }
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                let key = parse_key_comment(html.as_ref(), line)
                    .map_err(|err| attach_slide_context(err, index, explicit_key.as_ref(), &fragments))?;
                if let Some(key) = key {
                    if list_depth > 0 || seen_content {
                        let err = BuildError::new(
                            ErrorKind::Parse,
                            Some(line),
                            "slide key comment must appear before slide content",
                            "move the key comment to the first non-blank line of the slide",
                        );
                        return Err(attach_slide_context(err, index, explicit_key.as_ref(), &fragments));
                    }
                    explicit_key = Some((key, line));
                } else if !is_html_comment(html.as_ref()) && !html.trim().is_empty() {
                    let err = unsupported_construct(line, "html");
                    return Err(attach_slide_context(err, index, explicit_key.as_ref(), &fragments));
                }
            }
            _ if list_depth > 0 => {}
            Event::Start(Tag::Heading { level, .. }) => {
                block = Some(OpenBlock::Heading {
                    level: heading_level_to_u8(level),
                    start: global_start,
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
                        source_slice(source, start, global_end),
                        text.trim(),
                    ));
                    seen_content = true;
                }
            }
            Event::Start(Tag::Paragraph) => {
                block = Some(OpenBlock::Paragraph { start: global_start });
            }
            Event::End(TagEnd::Paragraph) => {
                if matches!(block, Some(OpenBlock::Paragraph { .. })) {
                    let Some(OpenBlock::Paragraph { start }) = block.take() else {
                        unreachable!();
                    };
                    fragments.push(SourceFragment::paragraph(
                        line_for_offset(source, start),
                        source_slice(source, start, global_end),
                    ));
                    seen_content = true;
                }
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let language = match kind {
                    CodeBlockKind::Fenced(language) if !language.is_empty() => Some(language.to_string()),
                    _ => None,
                };
                block = Some(OpenBlock::Code {
                    start: global_start,
                    language,
                    text: String::new(),
                });
            }
            Event::End(TagEnd::CodeBlock) => {
                if matches!(block, Some(OpenBlock::Code { .. })) {
                    let Some(OpenBlock::Code { start, language, text }) = block.take() else {
                        unreachable!();
                    };
                    fragments.push(SourceFragment::code(
                        line_for_offset(source, start),
                        language,
                        text,
                    ));
                    seen_content = true;
                }
            }
            Event::Text(text) | Event::Code(text) => match block.as_mut() {
                Some(OpenBlock::Heading { text: heading_text, .. }) => heading_text.push_str(text.as_ref()),
                Some(OpenBlock::Code { text: code_text, .. }) => code_text.push_str(text.as_ref()),
                Some(OpenBlock::Paragraph { .. }) => {}
                None if text.trim().is_empty() => {}
                None => {
                    let err = unsupported_construct(line, "text outside block");
                    return Err(attach_slide_context(err, index, explicit_key.as_ref(), &fragments));
                }
            },
            Event::SoftBreak | Event::HardBreak => {
                if let Some(OpenBlock::Code { text, .. }) = block.as_mut() {
                    text.push('\n');
                }
            }
            Event::Start(Tag::HtmlBlock) | Event::End(TagEnd::HtmlBlock) => {}
            Event::Start(tag) if unsupported_tag(&tag) => {
                let err = unsupported_construct(line, unsupported_tag_name(&tag));
                return Err(attach_slide_context(err, index, explicit_key.as_ref(), &fragments));
            }
            Event::Start(Tag::Item)
            | Event::End(TagEnd::Item)
            | Event::Start(Tag::Emphasis)
            | Event::End(TagEnd::Emphasis)
            | Event::Start(Tag::Strong)
            | Event::End(TagEnd::Strong)
            | Event::Start(Tag::Link { .. })
            | Event::End(TagEnd::Link) => {}
            other => {
                let err = unsupported_construct(line, event_name(&other));
                return Err(attach_slide_context(err, index, explicit_key.as_ref(), &fragments));
            }
        }
    }

    let (key, key_source) = explicit_key
        .map(|(key, line)| (key, KeySource::Explicit { line }))
        .unwrap_or_else(|| {
            let key = derive_key_from_fragments(&fragments, index);
            let key_source = derived_key_source(&fragments);
            (key, key_source)
        });

    Ok(ParsedSlide { index, key, key_source, fragments })
}

fn attach_slide_context(
    err: BuildError,
    index: usize,
    explicit_key: Option<&(SlideKey, usize)>,
    fragments: &[SourceFragment],
) -> BuildError {
    let key = explicit_key
        .map(|(key, _line)| key.clone())
        .unwrap_or_else(|| derive_key_from_fragments(fragments, index));
    err.with_slide(index + 1, Some(key.as_str()))
}
```

Verification:

```bash
cargo test -p peitho-core parser::tests::unsupported_construct_after_delimiter_reports_global_line_and_slide
cargo test -p peitho-core parser::tests::unsupported_image_after_delimiter_is_not_silently_dropped
cargo test -p peitho-core parser::tests::unsupported_table_after_delimiter_is_not_silently_dropped
```

### Task 5 - Attach Key Comments to the Following Slide

Goal: each slide's leading key comment owns that slide key, including the common `---` then key comment pattern.

Files:

- `crates/peitho-core/src/parser.rs`
- `examples/deck.md`

Test:

```rust
// crates/peitho-core/src/parser.rs
#[test]
fn key_comment_after_delimiter_belongs_to_next_slide() {
    let markdown = "# Intro\n\n---\n<!-- {\"key\":\"arch-1\"} -->\n# Architecture";
    let deck = parse_markdown(markdown).unwrap();

    assert_eq!(deck.parsed_slides()[0].key.as_str(), "intro");
    assert_eq!(deck.parsed_slides()[1].key.as_str(), "arch-1");
    assert_eq!(deck.parsed_slides()[1].key_source, KeySource::Explicit { line: 4 });
}
```

Implementation:

```rust
// crates/peitho-core/src/parser.rs
Event::Html(html) | Event::InlineHtml(html) => {
    let key = parse_key_comment(html.as_ref(), line)
        .map_err(|err| attach_slide_context(err, index, explicit_key.as_ref(), &fragments))?;
    if let Some(key) = key {
        if list_depth > 0 || seen_content {
            let err = BuildError::new(
                ErrorKind::Parse,
                Some(line),
                "slide key comment must appear before slide content",
                "move the key comment to the first non-blank line of the slide",
            );
            return Err(attach_slide_context(err, index, explicit_key.as_ref(), &fragments));
        }
        explicit_key = Some((key, line));
    } else if !is_html_comment(html.as_ref()) && !html.trim().is_empty() {
        let err = unsupported_construct(line, "html");
        return Err(attach_slide_context(err, index, explicit_key.as_ref(), &fragments));
    }
}
```

Verification:

```bash
cargo test -p peitho-core parser::tests::key_comment_after_delimiter_belongs_to_next_slide
```

### Task 6 - Reject Duplicate Slide Keys

Goal: duplicate keys across all slides are build errors whether explicit or derived.

Files:

- `crates/peitho-core/src/parser.rs`
- `crates/peitho-core/src/error.rs`

Test:

```rust
// crates/peitho-core/src/parser.rs
#[test]
fn rejects_duplicate_explicit_slide_keys() {
    let err = parse_markdown("<!-- {\"key\":\"same\"} -->\n# One\n\n---\n<!-- {\"key\":\"same\"} -->\n# Two").unwrap_err();

    assert_eq!(err.kind, ErrorKind::Parse);
    assert_eq!(err.line, Some(5));
    assert!(err.to_string().contains("slide 2 ('same'), line 5"));
    assert!(err.to_string().contains("duplicate slide key 'same'"));
    assert_eq!(err.help, "choose a unique explicit slide key");
}

#[test]
fn rejects_derived_slide_key_collision_with_explicit_key() {
    let err = parse_markdown("<!-- {\"key\":\"intro\"} -->\n# One\n\n---\n# Intro").unwrap_err();

    assert_eq!(err.line, Some(5));
    assert!(err.to_string().contains("duplicate slide key 'intro'"));
    assert_eq!(err.help, "add an explicit unique key comment before this slide");
}

#[test]
fn rejects_derived_slide_key_collision_with_derived_key() {
    let err = parse_markdown("# Intro\n\n---\n# Intro").unwrap_err();

    assert_eq!(err.line, Some(4));
    assert!(err.to_string().contains("duplicate slide key 'intro'"));
    assert_eq!(err.help, "add an explicit unique key comment before this slide");
}
```

Implementation:

```rust
// crates/peitho-core/src/parser.rs
fn validate_unique_keys(slides: &[ParsedSlide]) -> Result<()> {
    let mut seen = BTreeMap::<String, usize>::new();
    for slide in slides {
        let key = slide.key.as_str().to_owned();
        if seen.insert(key.clone(), slide.index).is_some() {
            let line = match slide.key_source {
                KeySource::Explicit { line } => Some(line),
                KeySource::Derived { line } => line,
            };
            let help = match slide.key_source {
                KeySource::Explicit { .. } => "choose a unique explicit slide key",
                KeySource::Derived { .. } => "add an explicit unique key comment before this slide",
            };
            return Err(BuildError::new(
                ErrorKind::Parse,
                line,
                format!("duplicate slide key '{key}'"),
                help,
            )
            .with_slide(slide.index + 1, Some(slide.key.as_str())));
        }
    }
    Ok(())
}

pub fn parse_markdown(source: &str) -> Result<Deck<Parsed>> {
    let ranges = split_slide_ranges(source)?;
    if ranges.is_empty() {
        return Err(BuildError::new(
            ErrorKind::Parse,
            None,
            "deck has no slides",
            "add at least one slide with content before building",
        ));
    }

    let mut slides = Vec::new();
    for (index, range) in ranges.into_iter().enumerate() {
        slides.push(parse_slide(source, range, index)?);
    }
    validate_unique_keys(&slides)?;
    Ok(Deck::parsed(slides))
}
```

Verification:

```bash
cargo test -p peitho-core parser::tests::rejects_duplicate_explicit_slide_keys
cargo test -p peitho-core parser::tests::rejects_derived_slide_key_collision_with_explicit_key
cargo test -p peitho-core parser::tests::rejects_derived_slide_key_collision_with_derived_key
```

### Task 7 - Confirm Mapping Handles Multiple Slides

Goal: convention mapping should map each parsed slide independently without introducing shared state.

Files:

- `crates/peitho-core/src/mapping.rs`

Test:

```rust
// crates/peitho-core/src/mapping.rs
#[test]
fn maps_each_slide_independently() {
    let markdown = "# Intro\n\nBody\n\n---\n# Architecture\n\n```rust\nfn main() {}\n```";
    let template = parse_template(
        "title-body-code",
        r#"<section>
           <slot name="title" accepts="inline" arity="1"></slot>
           <slot name="body" accepts="blocks" arity="0..*"></slot>
           <slot name="code" accepts="code" arity="0..1"></slot>
           </section>"#,
    )
    .unwrap();

    let mapped = map_by_convention(parse_markdown(markdown).unwrap(), &template).unwrap();

    assert_eq!(mapped.mapped_slides().len(), 2);
    assert_eq!(mapped.mapped_slides()[0].slots[&SlotName::new("body").unwrap()].fragments().len(), 1);
    assert_eq!(mapped.mapped_slides()[1].slots[&SlotName::new("code").unwrap()].fragments().len(), 1);
}
```

Implementation:

```rust
// crates/peitho-core/src/mapping.rs
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
                FragmentKind::Heading { .. }
                | FragmentKind::Paragraph
                | FragmentKind::List
                | FragmentKind::Image
                | FragmentKind::Text => "body",
            };
            let slot = SlotName::new(target).expect("conventional slot names are valid");
            if let Some(contract) = template.slot_by_name(&slot).cloned() {
                slots
                    .entry(slot.clone())
                    .or_insert_with(|| MappedSlot::new(contract))
                    .push(fragment);
            } else {
                unassigned.push(UnassignedFragment::new(slot, fragment));
            }
        }

        slides.push(MappedSlide { index: slide.index, key: slide.key, slots, unassigned });
    }
    Ok(Deck::mapped(slides))
}
```

Verification:

```bash
cargo test -p peitho-core mapping::tests::maps_each_slide_independently
```

### Task 8 - Add Slide Context to Check Errors

Goal: accepts, arity, and residual-content failures show the failing slide number and key.

Files:

- `crates/peitho-core/src/check.rs`

Test:

```rust
// crates/peitho-core/src/check.rs
#[test]
fn arity_error_in_second_slide_includes_slide_context() {
    let markdown = "# Intro\n\n---\n<!-- {\"key\":\"code-slide\"} -->\n# Code\n\n```rust\nfn a() {}\n```\n\n```rust\nfn b() {}\n```";
    let template = parse_template(
        "title-body-code",
        r#"<section>
           <slot name="title" accepts="inline" arity="1"></slot>
           <slot name="code" accepts="code" arity="0..1"></slot>
           </section>"#,
    )
    .unwrap();
    let mapped = map_by_convention(parse_markdown(markdown).unwrap(), &template).unwrap();

    let err = check_deck(mapped, &template).unwrap_err();

    assert_eq!(err.kind, ErrorKind::Arity);
    assert!(err.to_string().contains("slide 2 ('code-slide'), line 7"));
    assert!(err.to_string().contains("slot 'code' got 2 item(s)"));
}
```

Implementation:

```rust
// crates/peitho-core/src/check.rs
pub fn check_deck(deck: Deck<Mapped>, template: &Template) -> Result<Deck<Checked>> {
    let mut slides = Vec::new();
    for slide in deck.into_mapped_slides() {
        let slide_number = slide.index + 1;
        let slide_key = slide.key.as_str().to_owned();
        check_slide(&slide, template)
            .map_err(|err| err.with_slide(slide_number, Some(&slide_key)))?;
        let checked_slots = slide
            .slots
            .into_iter()
            .map(|(slot, mapped_slot)| (slot, mapped_slot.fragments().to_vec()))
            .collect();
        slides.push(CheckedSlide::new(slide.index, slide.key, checked_slots));
    }
    Ok(Deck::checked(slides))
}

fn check_slide(slide: &MappedSlide, template: &Template) -> Result<()> {
    check_accepts(&slide.slots)?;
    check_arity(&slide.slots, template)?;
    check_no_unassigned(&slide.unassigned)
}

fn check_accepts(slots: &BTreeMap<SlotName, MappedSlot>) -> Result<()> {
    for (slot, mapped_slot) in slots {
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
    Ok(())
}
```

Verification:

```bash
cargo test -p peitho-core check::tests::arity_error_in_second_slide_includes_slide_context
```

### Task 9 - Add Manifest Types as Core Contract

Goal: define §17 `manifest.json` schema in `peitho-core` with serde serialization.

Files:

- `crates/peitho-core/src/manifest.rs`
- `crates/peitho-core/src/lib.rs`
- `crates/peitho-core/Cargo.toml`
- `crates/peitho-core/src/domain.rs`
- `crates/peitho-core/src/error.rs`

Test:

```rust
// crates/peitho-core/src/manifest.rs
#[test]
fn serializes_manifest_schema_exactly() {
    let manifest = Manifest::new(
        "Peitho Architecture",
        vec![
            ManifestSlide::new(0, SlideKey::new("arch-1").unwrap(), "slides/000-arch-1.html", false),
            ManifestSlide::new(1, SlideKey::new("details").unwrap(), "slides/001-details.html", false),
        ],
    );

    let json = manifest_json(&manifest).unwrap();

    assert_eq!(
        json,
        concat!(
            "{\n",
            "  \"version\": 1,\n",
            "  \"peithoVersion\": \"0.1.0\",\n",
            "  \"title\": \"Peitho Architecture\",\n",
            "  \"slideCount\": 2,\n",
            "  \"slides\": [\n",
            "    {\n",
            "      \"index\": 0,\n",
            "      \"key\": \"arch-1\",\n",
            "      \"src\": \"slides/000-arch-1.html\",\n",
            "      \"hasNotes\": false\n",
            "    },\n",
            "    {\n",
            "      \"index\": 1,\n",
            "      \"key\": \"details\",\n",
            "      \"src\": \"slides/001-details.html\",\n",
            "      \"hasNotes\": false\n",
            "    }\n",
            "  ]\n",
            "}\n"
        )
    );
}
```

Implementation:

```rust
// crates/peitho-core/src/manifest.rs
use serde::Serialize;

use crate::{
    domain::SlideKey,
    error::{BuildError, ErrorKind, Result},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Manifest {
    version: u8,
    #[serde(rename = "peithoVersion")]
    peitho_version: String,
    title: String,
    #[serde(rename = "slideCount")]
    slide_count: usize,
    slides: Vec<ManifestSlide>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ManifestSlide {
    index: usize,
    key: SlideKey,
    src: String,
    #[serde(rename = "hasNotes")]
    has_notes: bool,
}

impl ManifestSlide {
    pub fn new(index: usize, key: SlideKey, src: impl Into<String>, has_notes: bool) -> Self {
        Self {
            index,
            key,
            src: src.into(),
            has_notes,
        }
    }
}

impl Manifest {
    pub fn new(title: impl Into<String>, slides: Vec<ManifestSlide>) -> Self {
        let slide_count = slides.len();
        Self {
            version: 1,
            peitho_version: env!("CARGO_PKG_VERSION").to_owned(),
            title: title.into(),
            slide_count,
            slides,
        }
    }
}

pub fn manifest_json(manifest: &Manifest) -> Result<String> {
    let mut json = serde_json::to_string_pretty(manifest).map_err(|err| {
        BuildError::new(ErrorKind::Manifest, None, format!("failed to serialize manifest: {err}"), "keep manifest fields serializable")
    })?;
    json.push('\n');
    Ok(json)
}
```

```rust
// crates/peitho-core/src/domain.rs
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct SlideKey(String);
```

```rust
// crates/peitho-core/src/error.rs
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    Parse,
    Template,
    Accepts,
    Arity,
    ResidualContent,
    Theme,
    Manifest,
}
```

```rust
// crates/peitho-core/src/lib.rs
pub mod manifest;
pub use manifest::{manifest_json, Manifest, ManifestSlide};
pub use phase::{require_checked_for_render, Deck, Mapped, Rendered};
```

Verification:

```bash
cargo test -p peitho-core manifest::tests::serializes_manifest_schema_exactly
```

### Task 10 - Build Manifest from Checked Deck

Goal: derive manifest title, slide count, fragment `src`, and `hasNotes=false` from `Deck<Checked>`.

Files:

- `crates/peitho-core/src/phase.rs`
- `crates/peitho-core/src/manifest.rs`
- `crates/peitho-core/src/domain.rs`
- `crates/peitho-core/src/lib.rs`

Test:

```rust
// crates/peitho-core/src/manifest.rs
#[test]
fn builds_manifest_from_checked_deck() {
    let checked = checked_deck(
        "<!-- {\"key\":\"arch-1\"} -->\n# Peitho Architecture\n\n---\n# Details",
        title_body_template(),
    );

    let manifest = build_manifest(&checked);
    let json = manifest_json(&manifest).unwrap();

    assert!(json.contains(r#""title": "Peitho Architecture""#));
    assert!(json.contains(r#""slideCount": 2"#));
    assert!(json.contains(r#""src": "slides/000-arch-1.html""#));
    assert!(json.contains(r#""src": "slides/001-details.html""#));
    assert!(json.contains(r#""hasNotes": false"#));
}
```

Implementation:

```rust
// crates/peitho-core/src/domain.rs
impl SourceFragment {
    pub(crate) fn heading_text_ref(&self) -> Option<&str> {
        matches!(self.kind, FragmentKind::Heading { .. }).then_some(self.plain_text())
    }
}
```

```rust
// crates/peitho-core/src/phase.rs
impl CheckedSlide {
    pub(crate) fn title_text(&self) -> Option<&str> {
        let title = SlotName::new("title").ok()?;
        self.slots
            .get(&title)?
            .iter()
            .find_map(|fragment| fragment.heading_text_ref())
    }
}

impl Deck<Checked> {
    pub(crate) fn checked_slides(&self) -> &[CheckedSlide] {
        &self.phase.slides
    }
}
```

```rust
// crates/peitho-core/src/manifest.rs
pub fn build_manifest(deck: &Deck<Checked>) -> Manifest {
    let title = deck
        .checked_slides()
        .first()
        .and_then(CheckedSlide::title_text)
        .filter(|title| !title.trim().is_empty())
        .unwrap_or("Untitled");

    let slides = deck
        .checked_slides()
        .iter()
        .map(|slide| ManifestSlide::new(
            slide.index(),
            slide.key().clone(),
            fragment_src(slide.index(), slide.key()),
            false,
        ))
        .collect();

    Manifest::new(title, slides)
}

pub fn fragment_src(index: usize, key: &SlideKey) -> String {
    format!("slides/{index:03}-{}.html", key.as_str())
}
```

```rust
// crates/peitho-core/src/lib.rs
pub use manifest::{build_manifest, fragment_src, manifest_json, Manifest, ManifestSlide};
```

Verification:

```bash
cargo test -p peitho-core manifest::tests::builds_manifest_from_checked_deck
```

### Task 11 - Render Fragment Source Metadata

Goal: expose deterministic per-slide fragment paths while keeping rendered slide fields private.

Files:

- `crates/peitho-core/src/domain.rs`
- `crates/peitho-core/src/render.rs`
- `crates/peitho-core/src/manifest.rs`

Test:

```rust
// crates/peitho-core/src/render.rs
#[test]
fn rendered_slides_have_manifest_fragment_sources() {
    let rendered = render_checked_deck("# Intro\n\n---\n# Details");

    assert_eq!(rendered.slides()[0].src(), "slides/000-intro.html");
    assert_eq!(rendered.slides()[1].src(), "slides/001-details.html");
}
```

Implementation:

```rust
// crates/peitho-core/src/domain.rs
impl RenderedSlide {
    pub fn src(&self) -> String {
        crate::manifest::fragment_src(self.index, &self.key)
    }
}
```

Verification:

```bash
cargo test -p peitho-core render::tests::rendered_slides_have_manifest_fragment_sources
```

### Task 12 - Generate Distribution index.html

Goal: replace embedded-slide `index.html` with a distribution entry that fetches `manifest.json` and each slide fragment.

Files:

- `crates/peitho-core/src/render.rs`
- `crates/peitho-core/src/lib.rs`

Test:

```rust
// crates/peitho-core/src/render.rs
#[test]
fn distribution_index_fetches_manifest_and_slide_sources_without_embedding_slides() {
    let html = render_distribution_index();

    assert!(html.contains(r#"<link rel="stylesheet" href="peitho.css">"#));
    assert!(html.contains("fetch('manifest.json')"));
    assert!(html.contains("fetch(slide.src)"));
    assert!(html.contains(r#"id="peitho-slides""#));
    assert!(!html.contains("data-slide-key="));
}
```

Implementation:

```rust
// crates/peitho-core/src/render.rs
pub fn render_distribution_index() -> String {
    r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <link rel="stylesheet" href="peitho.css">
  <title>Peitho Deck</title>
</head>
<body>
  <main id="peitho-slides"></main>
  <script>
    async function loadDeck() {
      const manifest = await fetch('manifest.json').then((response) => response.json());
      document.title = manifest.title || 'Peitho Deck';
      const root = document.getElementById('peitho-slides');
      for (const slide of manifest.slides) {
        const html = await fetch(slide.src).then((response) => response.text());
        const holder = document.createElement('div');
        holder.innerHTML = html;
        while (holder.firstChild) root.appendChild(holder.firstChild);
      }
    }
    loadDeck();
  </script>
</body>
</html>"#.to_owned()
}
```

Verification:

```bash
cargo test -p peitho-core render::tests::distribution_index_fetches_manifest_and_slide_sources_without_embedding_slides
```

### Task 13 - Write slides/ Fragments from the CLI

Goal: `peitho build` writes one fragment file per rendered slide under `dist/slides/`.

Files:

- `crates/peitho/src/main.rs`
- `crates/peitho/tests/build.rs`

Test:

```rust
// crates/peitho/tests/build.rs
#[test]
fn build_writes_slide_fragments_in_slides_directory() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    let out = dir.path().join("dist");
    write_multi_slide_fixture(&deck);
    let template = write_template(dir.path());
    let base = write_base_css(dir.path());
    let overrides = write_overrides_css(dir.path(), r#"[data-slide-key="arch-1"] .slot-code { outline: 3px solid #f40; }"#);

    Command::cargo_bin("peitho").unwrap()
        .args(["build", deck.to_str().unwrap(), "--template", template.to_str().unwrap(), "--base-css", base.to_str().unwrap(), "--overrides-css", overrides.to_str().unwrap(), "--out", out.to_str().unwrap()])
        .assert()
        .success();

    assert!(out.join("slides/000-arch-1.html").exists());
    assert!(out.join("slides/001-convention-mapping.html").exists());
    assert!(out.join("slides/002-dist-1.html").exists());
    assert!(fs::read_to_string(out.join("slides/000-arch-1.html")).unwrap().contains(r#"data-slide-key="arch-1""#));
}
```

Implementation:

```rust
// crates/peitho/src/main.rs
fn write_slide_fragments(out: &Path, rendered: &peitho_core::Deck<peitho_core::Rendered>) -> miette::Result<()> {
    let slides_dir = out.join("slides");
    fs::create_dir_all(&slides_dir).into_diagnostic()?;
    for slide in rendered.slides() {
        fs::write(out.join(slide.src()), slide.html()).into_diagnostic()?;
    }
    Ok(())
}
```

```rust
// crates/peitho/tests/build.rs
use tempfile::{tempdir, TempDir};

fn write_multi_slide_fixture(path: &Path) {
    fs::write(
        path,
        r#"<!-- {"key":"arch-1"} -->
# Peitho Architecture

Body

```rust
fn main() {}
```

---

# Convention Mapping

- title
- body

---
<!-- {"key":"dist-1"} -->
# Distribution

Fragments and manifest.
"#,
    )
    .unwrap();
}

fn write_template(dir: &Path) -> PathBuf {
    let path = dir.join("title-body-code.html");
    fs::write(
        &path,
        r#"<section><h1><slot name="title" accepts="inline" arity="1"></slot></h1><slot name="body" accepts="blocks" arity="0..*"></slot><slot name="code" accepts="code" arity="0..1"></slot></section>"#,
    )
    .unwrap();
    path
}

fn write_base_css(dir: &Path) -> PathBuf {
    let path = dir.join("base.css");
    fs::write(&path, ".slot-title { font-weight: 700; }").unwrap();
    path
}

fn write_overrides_css(dir: &Path, css: &str) -> PathBuf {
    let path = dir.join("overrides.css");
    fs::write(&path, css).unwrap();
    path
}

fn build_multi_slide_fixture() -> (TempDir, PathBuf) {
    build_multi_slide_fixture_with_override(
        r#"[data-slide-key="arch-1"] .slot-code { outline: 3px solid #f40; }"#,
    )
}

fn build_multi_slide_fixture_with_override(override_css: &str) -> (TempDir, PathBuf) {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    write_multi_slide_fixture(&deck);
    let template = write_template(dir.path());
    let base = write_base_css(dir.path());
    let overrides = write_overrides_css(dir.path(), override_css);
    let out = dir.path().join("dist");

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
        .success();

    (dir, out)
}
```

Verification:

```bash
cargo test -p peitho --test build build_writes_slide_fragments_in_slides_directory
```

### Task 14 - Write manifest.json from Core Manifest

Goal: CLI writes `manifest.json` using `peitho-core` manifest serialization, not string concatenation.

Files:

- `crates/peitho/src/main.rs`
- `crates/peitho/tests/build.rs`

Test:

```rust
// crates/peitho/tests/build.rs
#[test]
fn build_writes_manifest_json_with_refs_not_html() {
    let (_dir, out) = build_multi_slide_fixture();
    let manifest = fs::read_to_string(out.join("manifest.json")).unwrap();

    assert!(manifest.contains(r#""version": 1"#));
    assert!(manifest.contains(r#""peithoVersion": "0.1.0""#));
    assert!(manifest.contains(r#""title": "Peitho Architecture""#));
    assert!(manifest.contains(r#""slideCount": 3"#));
    assert!(manifest.contains(r#""src": "slides/000-arch-1.html""#));
    assert!(manifest.contains(r#""hasNotes": false"#));
    assert!(!manifest.contains("<section"));
}
```

Implementation:

```rust
// crates/peitho/src/main.rs
let checked = core(peitho_core::check_deck(mapped, &template))?;
let manifest = peitho_core::build_manifest(&checked);
let manifest_json = core(peitho_core::manifest_json(&manifest))?;
let rendered = core(peitho_core::render_deck(checked, &template))?;

fs::write(out.join("manifest.json"), manifest_json).into_diagnostic()?;
```

Verification:

```bash
cargo test -p peitho --test build build_writes_manifest_json_with_refs_not_html
```

### Task 15 - Write Distribution index.html without Embedded Slides

Goal: CLI writes an index that fetches manifest and fragments and does not duplicate slide bodies.

Files:

- `crates/peitho/src/main.rs`
- `crates/peitho/tests/build.rs`

Test:

```rust
// crates/peitho/tests/build.rs
#[test]
fn build_writes_fetching_index_without_embedded_slide_html() {
    let (_dir, out) = build_multi_slide_fixture();
    let index = fs::read_to_string(out.join("index.html")).unwrap();

    assert!(index.contains("fetch('manifest.json')"));
    assert!(index.contains("fetch(slide.src)"));
    assert!(index.contains(r#"<main id="peitho-slides"></main>"#));
    assert!(!index.contains("Peitho Architecture"));
    assert!(!index.contains("data-slide-key=\"arch-1\""));
}
```

Implementation:

```rust
// crates/peitho/src/main.rs
fs::write(out.join("index.html"), peitho_core::render_distribution_index()).into_diagnostic()?;
```

Verification:

```bash
cargo test -p peitho --test build build_writes_fetching_index_without_embedded_slide_html
```

### Task 16 - Validate Overrides Against All Slide Keys

Goal: theme override validation uses every checked slide key from a multi-slide deck.

Files:

- `crates/peitho/src/main.rs`
- `crates/peitho/tests/build.rs`

Test:

```rust
// crates/peitho/tests/build.rs
#[test]
fn build_accepts_override_targeting_derived_second_slide_key() {
    let (_dir, out) = build_multi_slide_fixture_with_override(
        r#"[data-slide-key="convention-mapping"] .slot-body { color: red; }"#,
    );

    let css = fs::read_to_string(out.join("peitho.css")).unwrap();
    assert!(css.contains(r#"[data-slide-key="convention-mapping"] .slot-body"#));
}
```

Implementation:

```rust
// crates/peitho/src/main.rs
let slide_keys = checked.slide_keys().collect::<Vec<_>>();
let css = core(peitho_core::build_theme_css(
    &base_css,
    &overrides_css,
    slide_keys.into_iter(),
    &template,
))?;
```

Verification:

```bash
cargo test -p peitho --test build build_accepts_override_targeting_derived_second_slide_key
```

### Task 17 - Surface Duplicate Key Failures Through CLI

Goal: CLI prints line/help for duplicate keys with slide context.

Files:

- `crates/peitho/tests/build.rs`

Test:

```rust
// crates/peitho/tests/build.rs
#[test]
fn build_fails_on_duplicate_slide_keys_with_line_help_and_slide_context() {
    let dir = tempdir().unwrap();
    let deck = dir.path().join("deck.md");
    fs::write(&deck, "# Intro\n\n---\n# Intro").unwrap();
    let template = write_template(dir.path());
    let base = write_base_css(dir.path());
    let overrides = write_overrides_css(dir.path(), "");
    let out = dir.path().join("dist");

    Command::cargo_bin("peitho").unwrap()
        .args(["build", deck.to_str().unwrap(), "--template", template.to_str().unwrap(), "--base-css", base.to_str().unwrap(), "--overrides-css", overrides.to_str().unwrap(), "--out", out.to_str().unwrap()])
        .assert()
        .failure()
        .stderr(predicate::str::contains("slide 2 ('intro'), line 4"))
        .stderr(predicate::str::contains("duplicate slide key 'intro'"))
        .stderr(predicate::str::contains("help: add an explicit unique key comment before this slide"));
}
```

Implementation:

```rust
// crates/peitho/src/main.rs
fn core<T>(result: peitho_core::Result<T>) -> miette::Result<T> {
    result.map_err(|err| miette::miette!("{err}"))
}
```

Verification:

```bash
cargo test -p peitho --test build build_fails_on_duplicate_slide_keys_with_line_help_and_slide_context
```

### Task 18 - Update Example Deck to Three Slides

Goal: repository example exercises explicit keys, derived keys, and a key comment immediately after a delimiter.

Files:

- `examples/deck.md`
- `themes/overrides.css`
- `crates/peitho/tests/build.rs`

Test:

```rust
// crates/peitho/tests/build.rs
#[test]
fn repository_example_builds_three_slide_distribution() {
    let out = tempdir().unwrap();

    Command::cargo_bin("peitho").unwrap()
        .current_dir(workspace_root())
        .args(["build", "examples/deck.md", "--template", "templates/title-body-code.html", "--base-css", "themes/base.css", "--overrides-css", "themes/overrides.css", "--out", out.path().to_str().unwrap()])
        .assert()
        .success()
        .stdout(predicate::str::contains("built 3 slide(s)"));

    assert!(out.path().join("slides/000-arch-1.html").exists());
    assert!(out.path().join("slides/001-convention-mapping.html").exists());
    assert!(out.path().join("slides/002-dist-1.html").exists());
    assert!(fs::read_to_string(out.path().join("manifest.json")).unwrap().contains(r#""slideCount": 3"#));
}
```

Implementation:

```markdown
<!-- examples/deck.md -->
<!-- {"key":"arch-1"} -->
# Peitho Architecture

Markdown is the source of truth, while HTML and CSS own layout.

```rust
enum Phase { Parsed, Mapped, Checked, Rendered }
```

---

# Convention Mapping

- Shallowest heading maps to title
- Code blocks map to code
- Remaining blocks map to body

---
<!-- {"key":"dist-1"} -->
# Distribution

The build output writes slide fragments plus a manifest.
```

```css
/* themes/overrides.css */
[data-slide-key="arch-1"] .slot-code { outline: 3px solid #f40; }
```

Verification:

```bash
cargo test -p peitho --test build repository_example_builds_three_slide_distribution
```

### Task 19 - Remove Obsolete Embedded Index Expectations

Goal: delete the old embedded-slide `render_index` API and update tests that expected `dist/index.html` to contain slide bodies.

Files:

- `crates/peitho/tests/build.rs`
- `crates/peitho-core/src/render.rs`
- `crates/peitho-core/src/lib.rs`
- `crates/peitho/src/main.rs`

Existing tests to update:

- `crates/peitho/tests/build.rs::build_writes_index_html_and_css`: keep the CSS assertion, but assert slide HTML exists under `slides/000-arch-1.html` and is not embedded in `index.html`.
- `crates/peitho/tests/build.rs::repository_example_builds`: replace with `repository_example_builds_three_slide_distribution` from Task 18.

Test:

```rust
// crates/peitho/tests/build.rs
#[test]
fn build_keeps_slide_html_only_in_fragment_files() {
    let (_dir, out) = build_multi_slide_fixture();
    let index = fs::read_to_string(out.join("index.html")).unwrap();
    let first = fs::read_to_string(out.join("slides/000-arch-1.html")).unwrap();

    assert!(!index.contains("data-slide-key"));
    assert!(first.contains(r#"data-slide-key="arch-1""#));
}
```

Implementation:

```rust
// crates/peitho/src/main.rs
fs::write(out.join("index.html"), peitho_core::render_distribution_index()).into_diagnostic()?;
```

```rust
// crates/peitho-core/src/lib.rs
pub use render::{render_deck, render_distribution_index};
```

```text
Delete `pub fn render_index(slides: &[RenderedSlide]) -> String` from `crates/peitho-core/src/render.rs`.
No caller may refer to `peitho_core::render_index`; `cargo clippy --workspace --all-targets -- -D warnings` verifies the old helper is gone instead of becoming dead code.
```

Verification:

```bash
cargo test -p peitho --test build build_keeps_slide_html_only_in_fragment_files
cargo clippy --workspace --all-targets -- -D warnings
```

### Task 20 - Final Verification

Goal: verify the milestone end-to-end contract and CI-facing commands.

Files:

- all files touched by Tasks 1-19

Test:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo run -p peitho -- build examples/deck.md --template templates/title-body-code.html --base-css themes/base.css --overrides-css themes/overrides.css --out dist
test -f dist/slides/000-arch-1.html
test -f dist/slides/001-convention-mapping.html
test -f dist/slides/002-dist-1.html
rg -n '"slideCount": 3|"src": "slides/000-arch-1.html"|"hasNotes": false' dist/manifest.json
rg -n "fetch\\('manifest.json'\\)|fetch\\(slide.src\\)" dist/index.html
! rg -n 'data-slide-key="arch-1"' dist/index.html
rg -n 'data-slide-key="arch-1"' dist/slides/000-arch-1.html
rg -n '\[data-slide-key="arch-1"\] \.slot-code' dist/peitho.css
```

Implementation:

```text
This task only runs commands; it does not modify repository files.
```

Verification:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo run -p peitho -- build examples/deck.md --template templates/title-body-code.html --base-css themes/base.css --overrides-css themes/overrides.css --out dist
```

## Summary

Across all 20 tasks, we first shape errors and parsed slide metadata for multiple slides, and add `---` delimiter splitting, slide key ownership, and duplicate key checking to the parser. Next, we lock down multi-slide error context for mapping/check, and add the manifest schema, JSON generation, fragment paths, and the fetch-only distribution index to `peitho-core`. Finally, we switch the CLI to emit `slides/` fragments + `manifest.json` + `index.html`, and confirm with a 3-slide example deck and integration tests that §14's "the entity lives in `slides/` exactly once" holds.
