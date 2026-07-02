use std::collections::BTreeMap;

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use serde::Deserialize;

use crate::{
    domain::{FragmentKind, SlideKey, SourceFragment},
    error::{BuildError, ErrorKind, Result},
    phase::{Deck, KeySource, Parsed, ParsedSlide},
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

#[derive(Debug, Clone, Copy)]
struct SlideRange {
    start: usize,
    end: usize,
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

fn split_slide_ranges(source: &str) -> Result<Vec<SlideRange>> {
    let mut ranges = Vec::new();
    let mut start = 0usize;
    let mut list_depth = 0usize;

    for (event, range) in Parser::new_ext(source, parser_options()).into_offset_iter() {
        match event {
            Event::Start(Tag::List(_)) => list_depth += 1,
            Event::End(TagEnd::List(_)) => list_depth = list_depth.saturating_sub(1),
            Event::Rule if list_depth == 0 => {
                ranges.push(SlideRange {
                    start,
                    end: range.start,
                });
                start = range.end;
            }
            Event::Rule => {}
            _ => {}
        }
    }

    ranges.push(SlideRange {
        start,
        end: source.len(),
    });
    Ok(ranges
        .into_iter()
        .filter(|range| !source[range.start..range.end].trim().is_empty())
        .collect())
}

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
                return Err(attach_slide_context(
                    err,
                    index,
                    explicit_key.as_ref(),
                    &fragments,
                ));
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
                    return Err(attach_slide_context(
                        err,
                        index,
                        explicit_key.as_ref(),
                        &fragments,
                    ));
                }
                list_depth -= 1;
                if list_depth == 0 {
                    if let Some(start) = list_start.take() {
                        fragments.push(SourceFragment::list(
                            line_for_offset(source, start),
                            source_slice(source, start, global_end),
                        ));
                        seen_content = true;
                    }
                }
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                if let Some(key) = parse_key_comment(html.as_ref(), line).map_err(|err| {
                    attach_slide_context(err, index, explicit_key.as_ref(), &fragments)
                })? {
                    if list_depth > 0 || seen_content {
                        let err = BuildError::new(
                            ErrorKind::Parse,
                            Some(line),
                            "slide key comment must appear before slide content",
                            "move the key comment to the first non-blank line of the slide",
                        );
                        return Err(attach_slide_context(
                            err,
                            index,
                            explicit_key.as_ref(),
                            &fragments,
                        ));
                    }
                    explicit_key = Some((key, line));
                } else if !is_html_comment(html.as_ref()) && !html.trim().is_empty() {
                    let err = unsupported_construct(line, "html");
                    return Err(attach_slide_context(
                        err,
                        index,
                        explicit_key.as_ref(),
                        &fragments,
                    ));
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
                block = Some(OpenBlock::Paragraph {
                    start: global_start,
                });
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
                    CodeBlockKind::Fenced(language) if !language.is_empty() => {
                        Some(language.to_string())
                    }
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
                    seen_content = true;
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
                None => {
                    let err = unsupported_construct(line, "text outside block");
                    return Err(attach_slide_context(
                        err,
                        index,
                        explicit_key.as_ref(),
                        &fragments,
                    ));
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
                return Err(attach_slide_context(
                    err,
                    index,
                    explicit_key.as_ref(),
                    &fragments,
                ));
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
                return Err(attach_slide_context(
                    err,
                    index,
                    explicit_key.as_ref(),
                    &fragments,
                ));
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

    Ok(ParsedSlide {
        index,
        key,
        key_source,
        fragments,
    })
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

fn parser_options() -> Options {
    Options::ENABLE_TABLES | Options::ENABLE_FOOTNOTES
}

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

fn parse_key_comment(raw: &str, line: usize) -> Result<Option<SlideKey>> {
    let trimmed = raw.trim();
    if !trimmed.starts_with("<!--") || !trimmed.ends_with("-->") {
        return Ok(None);
    }
    let json = trimmed
        .trim_start_matches("<!--")
        .trim_end_matches("-->")
        .trim();
    if !json.starts_with('{') {
        return Ok(None);
    }
    let parsed: KeyComment = serde_json::from_str(json).map_err(|err| {
        BuildError::new(
            ErrorKind::Parse,
            Some(line),
            format!("invalid slide key comment: {err}"),
            r#"use <!-- {"key":"arch-1"} --> with a lowercase ascii, digit, or '-' key"#,
        )
    })?;
    SlideKey::new(parsed.key).map(Some).map_err(|message| {
        BuildError::new(
            ErrorKind::Parse,
            Some(line),
            message,
            "use lowercase ascii, digits, or '-' in the key string",
        )
    })
}

fn is_html_comment(raw: &str) -> bool {
    let trimmed = raw.trim();
    trimmed.starts_with("<!--") && trimmed.ends_with("-->")
}

fn line_for_offset(source: &str, offset: usize) -> usize {
    source[..offset]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1
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

fn derived_key_source(fragments: &[SourceFragment]) -> KeySource {
    let line = fragments
        .iter()
        .find(|fragment| matches!(fragment.kind(), FragmentKind::Heading { .. }))
        .map(SourceFragment::line);
    KeySource::Derived { line }
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
    use crate::{domain::FragmentKind, error::ErrorKind, phase::KeySource};

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
        assert_eq!(
            slide.fragments[0].kind(),
            FragmentKind::Heading { level: 1 }
        );
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
    fn rejects_inline_html_inside_list_items() {
        let err = parse_markdown("# Title\n\n- item <b>raw</b>").unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(3));
        assert!(err.to_string().contains("unsupported construct 'html'"));
    }

    #[test]
    fn ignores_plain_html_comments() {
        let deck = parse_markdown("<!-- TODO: polish copy -->\n# Title").unwrap();

        let slide = &deck.parsed_slides()[0];
        assert_eq!(slide.key.as_str(), "title");
        assert_eq!(slide.fragments.len(), 1);
        assert_eq!(slide.fragments[0].line(), 2);
    }

    #[test]
    fn rejects_broken_json_key_comments() {
        let err = parse_markdown("<!-- {\"kye\":\"arch-1\"} -->\n# Title").unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(1));
        assert!(err.to_string().contains("invalid slide key comment"));
    }

    #[test]
    fn rejects_explicit_keys_with_invalid_characters_and_line() {
        let err = parse_markdown("<!-- {\"key\":\"bad key]\"} -->\n# Title").unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(1));
        assert!(err
            .to_string()
            .contains("slide key must use lowercase ascii, digits, or '-'"));
        assert_eq!(
            err.help,
            "use lowercase ascii, digits, or '-' in the key string"
        );
    }

    #[test]
    fn derives_key_from_first_heading_when_comment_is_absent() {
        let deck = parse_markdown("# Architecture Overview\n\nBody").unwrap();
        assert_eq!(
            deck.parsed_slides()[0].key.as_str(),
            "architecture-overview"
        );
    }

    #[test]
    fn explicit_key_wins_over_derived_key() {
        let deck = parse_markdown("<!-- {\"key\":\"arch-1\"} -->\n# Renamed Title").unwrap();
        assert_eq!(deck.parsed_slides()[0].key.as_str(), "arch-1");
    }

    #[test]
    fn parsed_slide_records_explicit_and_derived_key_sources() {
        let deck =
            parse_markdown("<!-- {\"key\":\"arch-1\"} -->\n# Architecture\n\n---\n# Derived Title")
                .unwrap();
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
        assert_eq!(
            deck.parsed_slides()[0].key_source,
            KeySource::Derived { line: None }
        );
        assert_eq!(deck.parsed_slides()[1].key.as_str(), "slide-2");
        assert_eq!(
            deck.parsed_slides()[1].key_source,
            KeySource::Derived { line: None }
        );
    }

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
        assert_eq!(
            err.help,
            "add at least one slide with content before building"
        );
    }

    #[test]
    fn unsupported_construct_after_delimiter_reports_global_line_and_slide() {
        let err = parse_markdown("# One\n\n---\n\n# Two\n\n> quote").unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(7));
        assert!(err.to_string().contains("slide 2 ('two'), line 7"));
        assert!(err
            .to_string()
            .contains("unsupported construct 'blockquote'"));
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

    #[test]
    fn key_comment_after_delimiter_belongs_to_next_slide() {
        let markdown = "# Intro\n\n---\n<!-- {\"key\":\"arch-1\"} -->\n# Architecture";
        let deck = parse_markdown(markdown).unwrap();

        assert_eq!(deck.parsed_slides()[0].key.as_str(), "intro");
        assert_eq!(deck.parsed_slides()[1].key.as_str(), "arch-1");
        assert_eq!(
            deck.parsed_slides()[1].key_source,
            KeySource::Explicit { line: 4 }
        );
    }

    #[test]
    fn rejects_duplicate_explicit_slide_keys() {
        let err = parse_markdown(
            "<!-- {\"key\":\"same\"} -->\n# One\n\n---\n<!-- {\"key\":\"same\"} -->\n# Two",
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(5));
        assert!(err.to_string().contains("slide 2 ('same'), line 5"));
        assert!(err.to_string().contains("duplicate slide key 'same'"));
        assert_eq!(err.help, "choose a unique explicit slide key");
    }

    #[test]
    fn rejects_derived_slide_key_collision_with_explicit_key() {
        let err =
            parse_markdown("<!-- {\"key\":\"intro\"} -->\n# One\n\n---\n# Intro").unwrap_err();

        assert_eq!(err.line, Some(5));
        assert!(err.to_string().contains("duplicate slide key 'intro'"));
        assert_eq!(
            err.help,
            "add an explicit unique key comment before this slide"
        );
    }

    #[test]
    fn rejects_derived_slide_key_collision_with_derived_key() {
        let err = parse_markdown("# Intro\n\n---\n# Intro").unwrap_err();

        assert_eq!(err.line, Some(4));
        assert!(err.to_string().contains("duplicate slide key 'intro'"));
        assert_eq!(
            err.help,
            "add an explicit unique key comment before this slide"
        );
    }
}
