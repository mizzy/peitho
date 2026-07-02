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
    Heading {
        level: u8,
        start: usize,
        text: String,
    },
    Paragraph {
        start: usize,
    },
    Code {
        start: usize,
        language: Option<String>,
        text: String,
    },
}

pub fn parse_markdown(source: &str) -> Result<Deck<Parsed>> {
    let mut explicit_key = None;
    let mut fragments = Vec::new();
    let mut block: Option<OpenBlock> = None;
    let mut list_depth = 0usize;
    let mut list_start = None;
    let options = Options::ENABLE_TABLES | Options::ENABLE_FOOTNOTES;

    for (event, range) in Parser::new_ext(source, options).into_offset_iter() {
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
            Event::Html(html) | Event::InlineHtml(html) => {
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
                    CodeBlockKind::Fenced(language) if !language.is_empty() => {
                        Some(language.to_string())
                    }
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
                    let Some(OpenBlock::Code {
                        start,
                        language,
                        text,
                    }) = block.take()
                    else {
                        unreachable!();
                    };
                    fragments.push(SourceFragment::code(
                        line_for_offset(source, start),
                        language,
                        text,
                    ));
                }
            }
            Event::Text(text) | Event::Code(text) => match block.as_mut() {
                Some(OpenBlock::Heading {
                    text: heading_text, ..
                }) => heading_text.push_str(&text),
                Some(OpenBlock::Code {
                    text: code_text, ..
                }) => code_text.push_str(&text),
                Some(OpenBlock::Paragraph { .. }) => {}
                None if text.trim().is_empty() => {}
                None => return Err(unsupported_construct(line, "text outside block")),
            },
            Event::SoftBreak | Event::HardBreak => {
                if let Some(OpenBlock::Code { text, .. }) = block.as_mut() {
                    text.push('\n');
                }
            }
            Event::Start(Tag::HtmlBlock) | Event::End(TagEnd::HtmlBlock) => {}
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
    let json = trimmed
        .trim_start_matches("<!--")
        .trim_end_matches("-->")
        .trim();
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
    SlideKey::new(if slug.is_empty() {
        format!("slide-{}", index + 1)
    } else {
        slug
    })
    .expect("derived slide keys are nonempty and attribute-safe")
}

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
        assert_eq!(
            slide.fragments[1].markdown(),
            "Markdown is the **source** of [truth](https://example.com)."
        );
        assert_eq!(slide.fragments[2].kind(), FragmentKind::List);
        assert_eq!(slide.fragments[2].line(), 6);
        assert!(slide.fragments[2]
            .markdown()
            .contains("- Markdown = source of truth"));
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
        assert!(err
            .to_string()
            .contains("unsupported construct 'blockquote'"));
        assert_eq!(
            err.help,
            "rewrite this slide using headings, paragraphs, lists, or fenced code blocks for milestone 1"
        );
    }

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
}
