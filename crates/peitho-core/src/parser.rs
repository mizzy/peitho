use std::collections::BTreeMap;

use pulldown_cmark::{
    CodeBlockKind, Event, HeadingLevel, MetadataBlockKind, Options, Parser, Tag, TagEnd,
};
use serde::Deserialize;

use crate::{
    domain::{FragmentKind, SlideKey, SourceFragment},
    error::{BuildError, ErrorKind, Result},
    phase::{Deck, KeySource, LayoutRequest, Parsed, ParsedSlide},
};

/// Page settings comment, deck-style: `<!-- {"key":"...","layout":"..."} -->`.
/// Unknown fields are rejected so a typo never silently drops a setting.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PageComment {
    key: Option<String>,
    layout: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeckFrontmatter {
    #[serde(default, deserialize_with = "deserialize_optional_planned_time")]
    time: Option<PlannedTime>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PlannedTime(u64);

impl PlannedTime {
    #[allow(dead_code)] // Converted at the future Deck/manifest consumption boundary.
    fn as_millis(self) -> u64 {
        self.0
    }
}

#[allow(dead_code)] // Carried on Deck in Task 5.
struct ParsedDeckSettings {
    time: Option<PlannedTime>,
}

struct PageSettings {
    key: Option<SlideKey>,
    layout: Option<String>,
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

struct RawFrontmatter {
    line: usize,
    yaml: String,
}

struct SplitSlides {
    frontmatter: Option<RawFrontmatter>,
    ranges: Vec<SlideRange>,
}

pub fn parse_markdown(source: &str) -> Result<Deck<Parsed>> {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let SplitSlides {
        frontmatter,
        ranges,
    } = split_slide_ranges(source)?;
    let _settings = parse_deck_frontmatter(frontmatter.as_ref())?;
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

fn split_slide_ranges(source: &str) -> Result<SplitSlides> {
    let (frontmatter, content_start) = leading_frontmatter(source)?;
    if frontmatter.is_none() {
        reject_invalid_leading_frontmatter_start(source)?;
    }

    let mut ranges = Vec::new();
    let mut start = content_start;
    let mut list_depth = 0usize;
    let content = &source[content_start..];

    for (event, range) in Parser::new_ext(content, slide_split_options()).into_offset_iter() {
        let global_start = content_start + range.start;
        let global_end = content_start + range.end;
        match event {
            Event::Start(Tag::List(_)) => list_depth += 1,
            Event::End(TagEnd::List(_)) => list_depth = list_depth.saturating_sub(1),
            Event::Rule if list_depth == 0 => {
                ranges.push(SlideRange {
                    start,
                    end: global_start,
                });
                start = global_end;
            }
            Event::Rule => {}
            _ => {}
        }
    }

    ranges.push(SlideRange {
        start,
        end: source.len(),
    });
    let ranges = ranges
        .into_iter()
        .filter(|range| !source[range.start..range.end].trim().is_empty())
        .collect();
    Ok(SplitSlides {
        frontmatter,
        ranges,
    })
}

fn deserialize_optional_planned_time<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<PlannedTime>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    PlannedTime::deserialize(deserializer).map(Some)
}

fn parse_planned_time_text(input: &str) -> std::result::Result<u64, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("time must not be empty".to_owned());
    }
    if trimmed.bytes().all(|byte| byte.is_ascii_digit()) {
        let minutes = trimmed
            .parse::<u64>()
            .map_err(|_| "time is too large".to_owned())?;
        return minutes_to_millis(minutes);
    }

    let mut rest = trimmed;
    let mut total_seconds = 0_u64;
    while !rest.is_empty() {
        let digit_bytes = rest
            .bytes()
            .take_while(|byte| byte.is_ascii_digit())
            .count();
        if digit_bytes == 0 {
            return Err("time must use h, m, or s units".to_owned());
        }
        let value = rest[..digit_bytes]
            .parse::<u64>()
            .map_err(|_| "time is too large".to_owned())?;
        rest = &rest[digit_bytes..];
        let unit = rest
            .bytes()
            .next()
            .ok_or_else(|| "time string is missing a unit".to_owned())?;
        let seconds = match unit {
            b'h' => value.checked_mul(3600),
            b'm' => value.checked_mul(60),
            b's' => Some(value),
            _ => return Err("time must use h, m, or s units".to_owned()),
        }
        .ok_or_else(|| "time is too large".to_owned())?;
        total_seconds = total_seconds
            .checked_add(seconds)
            .ok_or_else(|| "time is too large".to_owned())?;
        rest = &rest[1..];
    }

    seconds_to_millis(total_seconds)
}

fn minutes_to_millis(minutes: u64) -> std::result::Result<u64, String> {
    if minutes == 0 {
        return Err("time must be greater than zero".to_owned());
    }
    minutes
        .checked_mul(60_000)
        .ok_or_else(|| "time is too large".to_owned())
}

fn seconds_to_millis(seconds: u64) -> std::result::Result<u64, String> {
    seconds
        .checked_mul(1000)
        .filter(|millis| *millis > 0)
        .ok_or_else(|| {
            if seconds == 0 {
                "time must be greater than zero".to_owned()
            } else {
                "time is too large".to_owned()
            }
        })
}

impl<'de> Deserialize<'de> for PlannedTime {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct PlannedTimeVisitor;

        impl<'de> serde::de::Visitor<'de> for PlannedTimeVisitor {
            type Value = PlannedTime;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a duration like 15m, 90s, 1h30m, or an integer minute count")
            }

            fn visit_u64<E>(self, value: u64) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                minutes_to_millis(value).map(PlannedTime).map_err(E::custom)
            }

            fn visit_i64<E>(self, value: i64) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if value <= 0 {
                    return Err(E::custom("time must be greater than zero"));
                }
                self.visit_u64(value as u64)
            }

            fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                parse_planned_time_text(value)
                    .map(PlannedTime)
                    .map_err(E::custom)
            }
        }

        deserializer.deserialize_any(PlannedTimeVisitor)
    }
}

fn parse_deck_frontmatter(raw: Option<&RawFrontmatter>) -> Result<ParsedDeckSettings> {
    let Some(raw) = raw else {
        return Ok(ParsedDeckSettings { time: None });
    };
    if raw.yaml.trim().is_empty() {
        return Ok(ParsedDeckSettings { time: None });
    }
    validate_frontmatter_lines(raw)?;

    let value: serde_norway::Value =
        serde_norway::from_str(&raw.yaml).map_err(|err| frontmatter_yaml_error(raw, &err))?;
    if !matches!(value, serde_norway::Value::Mapping(_)) {
        return Err(BuildError::new(
            ErrorKind::Parse,
            Some(raw.line + first_nonblank_yaml_line(&raw.yaml)),
            "invalid deck frontmatter: expected a YAML mapping of deck settings",
            "write valid YAML frontmatter before the first slide",
        ));
    }

    let parsed: DeckFrontmatter =
        serde_norway::from_str(&raw.yaml).map_err(|err| frontmatter_yaml_error(raw, &err))?;

    Ok(ParsedDeckSettings { time: parsed.time })
}

fn first_nonblank_yaml_line(yaml: &str) -> usize {
    yaml.lines()
        .position(|line| !line.trim().is_empty())
        .map(|index| index + 1)
        .unwrap_or(1)
}

fn validate_frontmatter_lines(raw: &RawFrontmatter) -> Result<()> {
    let lines = raw.yaml.lines().collect::<Vec<_>>();
    let content_len = lines
        .iter()
        .rposition(|line| !line.trim().is_empty())
        .map(|index| index + 1)
        .unwrap_or(0);

    for (index, line) in lines[..content_len].iter().enumerate() {
        if !starts_with_flat_yaml_key(line) {
            return Err(BuildError::new(
                ErrorKind::Parse,
                Some(raw.line + index + 1),
                "unexpected line in deck frontmatter (missing closing ---?)",
                "keep only key: value settings (like time: 15m) between the --- markers",
            ));
        }
    }

    Ok(())
}

fn starts_with_flat_yaml_key(line: &str) -> bool {
    let Some(colon_index) = line.find(':') else {
        return false;
    };
    let key = &line[..colon_index];
    !key.is_empty()
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

fn frontmatter_yaml_error(raw: &RawFrontmatter, err: &serde_norway::Error) -> BuildError {
    let yaml_line = err
        .location()
        .map(|location| location.line())
        .unwrap_or_else(|| first_nonblank_yaml_line(&raw.yaml));
    let err = err.to_string();
    let message = format!(
        "invalid deck frontmatter: {}",
        err.split(" at line ").next().unwrap_or(&err)
    );
    let help = frontmatter_help(&message);
    BuildError::new(ErrorKind::Parse, Some(raw.line + yaml_line), message, help)
}

fn frontmatter_help(message: &str) -> &'static str {
    if message.contains("unknown field") || message.contains("duplicate entry") {
        "use only the supported deck frontmatter key: time"
    } else if message.contains("time")
        || message.contains("duration")
        || message.contains("greater than zero")
        || message.contains("unit")
        || message.contains("integer")
        || message.contains("u128")
        || message.contains("too large")
    {
        "set time to 15m, 90s, 1h, 1h30m, or an integer minute count"
    } else {
        "write valid YAML frontmatter before the first slide"
    }
}

fn leading_frontmatter(source: &str) -> Result<(Option<RawFrontmatter>, usize)> {
    let mut events = Parser::new_ext(source, parser_options()).into_offset_iter();
    let Some((event, range)) = events.next() else {
        return Ok((None, 0));
    };

    let frontmatter_line = match event {
        Event::Start(Tag::MetadataBlock(MetadataBlockKind::YamlStyle))
            if source[..range.start].chars().all(char::is_whitespace) =>
        {
            line_for_offset(source, range.start)
        }
        _ => return Ok((None, 0)),
    };

    let mut frontmatter_yaml = String::new();

    for (event, range) in events {
        match event {
            Event::Text(text) => {
                frontmatter_yaml.push_str(&text);
            }
            Event::End(TagEnd::MetadataBlock(MetadataBlockKind::YamlStyle)) => {
                return Ok((
                    Some(RawFrontmatter {
                        line: frontmatter_line,
                        yaml: frontmatter_yaml,
                    }),
                    range.end,
                ));
            }
            Event::Start(Tag::MetadataBlock(_)) | Event::End(TagEnd::MetadataBlock(_)) => {
                return Err(metadata_block_position_error(source, range.start));
            }
            _ => return Err(unexpected_frontmatter_content_error(source, range.start)),
        }
    }

    Err(unexpected_frontmatter_content_error(source, source.len()))
}

fn reject_invalid_leading_frontmatter_start(source: &str) -> Result<()> {
    let Some((offset, line)) = first_nonblank_source_line(source) else {
        return Ok(());
    };
    if line.trim() != "---" {
        return Ok(());
    }

    Err(BuildError::new(
        ErrorKind::Parse,
        Some(line_for_offset(source, offset)),
        "deck starts with --- but no valid YAML frontmatter was found",
        "put --- on the first line, settings on the following lines, and close with --- without blank lines",
    ))
}

fn first_nonblank_source_line(source: &str) -> Option<(usize, &str)> {
    let mut line_start = 0usize;
    for line in source.split_inclusive('\n') {
        let line_without_newline = line.strip_suffix('\n').unwrap_or(line);
        if !line_without_newline.trim().is_empty() {
            return Some((line_start, line_without_newline));
        }
        line_start += line.len();
    }
    None
}

fn parse_slide(source: &str, range: SlideRange, index: usize) -> Result<ParsedSlide> {
    let slice = &source[range.start..range.end];
    let mut explicit_key: Option<(SlideKey, usize)> = None;
    let mut layout_request: Option<LayoutRequest> = None;
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
                if let Some(settings) = parse_page_comment(html.as_ref(), line).map_err(|err| {
                    attach_slide_context(err, index, explicit_key.as_ref(), &fragments)
                })? {
                    if list_depth > 0 || seen_content {
                        let err = BuildError::new(
                            ErrorKind::Parse,
                            Some(line),
                            "page settings comment must appear before slide content",
                            "move the settings comment to the first non-blank line of the slide",
                        );
                        return Err(attach_slide_context(
                            err,
                            index,
                            explicit_key.as_ref(),
                            &fragments,
                        ));
                    }
                    if let Some(key) = settings.key {
                        explicit_key = Some((key, line));
                    }
                    if let Some(name) = settings.layout {
                        layout_request = Some(LayoutRequest { name, line });
                    }
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
            Event::Start(Tag::MetadataBlock(_)) => {
                let err = metadata_block_position_error(source, global_start);
                return Err(attach_slide_context(
                    err,
                    index,
                    explicit_key.as_ref(),
                    &fragments,
                ));
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
                    let code_line = line_for_offset(source, start);
                    if let Some(language) = &language {
                        crate::highlight::validate_language(language, code_line).map_err(
                            |err| {
                                attach_slide_context(err, index, explicit_key.as_ref(), &fragments)
                            },
                        )?;
                    }
                    fragments.push(SourceFragment::code(code_line, language, text));
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
        layout_request,
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
    Options::ENABLE_TABLES | Options::ENABLE_FOOTNOTES | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
}

fn slide_split_options() -> Options {
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

fn parse_page_comment(raw: &str, line: usize) -> Result<Option<PageSettings>> {
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
    let parsed: PageComment = serde_json::from_str(json).map_err(|err| {
        BuildError::new(
            ErrorKind::Parse,
            Some(line),
            format!("invalid page settings comment: {err}"),
            r#"use <!-- {"key":"arch-1","layout":"cover"} --> (both fields optional, no other fields)"#,
        )
    })?;
    if parsed.key.is_none() && parsed.layout.is_none() {
        return Err(BuildError::new(
            ErrorKind::Parse,
            Some(line),
            "page settings comment has no settings",
            r#"set "key" and/or "layout", or remove the comment"#,
        ));
    }
    let key = parsed
        .key
        .map(|key| {
            SlideKey::new(key).map_err(|message| {
                BuildError::new(
                    ErrorKind::Parse,
                    Some(line),
                    message,
                    "use lowercase ascii, digits, or '-' in the key string",
                )
            })
        })
        .transpose()?;
    Ok(Some(PageSettings {
        key,
        layout: parsed.layout,
    }))
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

fn metadata_block_position_error(source: &str, offset: usize) -> BuildError {
    BuildError::new(
        ErrorKind::Parse,
        Some(line_for_offset(source, offset)),
        "YAML frontmatter is only allowed at the top of the deck",
        "move deck settings before the first slide or replace this block with slide content",
    )
}

fn unexpected_frontmatter_content_error(source: &str, offset: usize) -> BuildError {
    BuildError::new(
        ErrorKind::Parse,
        Some(line_for_offset(source, offset)),
        "unexpected content in deck frontmatter",
        "write plain YAML frontmatter before the first slide",
    )
}

fn unsupported_tag(tag: &Tag<'_>) -> bool {
    matches!(
        tag,
        Tag::BlockQuote(_)
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
        Tag::BlockQuote(_) => "blockquote",
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
    fn deck_frontmatter_yaml_dependency_is_available() {
        #[derive(Debug, serde::Deserialize)]
        struct Probe {
            time: String,
        }

        let parsed: Probe = serde_norway::from_str("time: 15m\n").unwrap();

        assert_eq!(parsed.time, "15m");
    }

    #[test]
    fn parses_planned_time_values_from_frontmatter() {
        let cases = [
            ("15m", 900_000),
            ("90s", 90_000),
            ("1h", 3_600_000),
            ("1h30m", 5_400_000),
            ("15", 900_000),
        ];

        for (value, expected) in cases {
            let yaml = format!("time: {value}\n");
            let parsed: DeckFrontmatter = serde_norway::from_str(&yaml).unwrap();

            assert_eq!(parsed.time.map(PlannedTime::as_millis), Some(expected));
        }
    }

    #[test]
    fn parsed_deck_settings_keep_planned_time_until_consumed() {
        let raw = RawFrontmatter {
            line: 1,
            yaml: "time: 15m\n".to_owned(),
        };

        let settings = parse_deck_frontmatter(Some(&raw)).unwrap();

        assert_eq!(settings.time.map(PlannedTime::as_millis), Some(900_000));
    }

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
    fn parses_layout_request_from_page_settings_comment() {
        let deck =
            parse_markdown("<!-- {\"key\":\"cover\",\"layout\":\"cover\"} -->\n# Title").unwrap();

        let slide = &deck.parsed_slides()[0];
        assert_eq!(slide.key.as_str(), "cover");
        assert_eq!(
            slide.layout_request,
            Some(LayoutRequest {
                name: "cover".to_owned(),
                line: 1,
            })
        );
    }

    #[test]
    fn layout_only_comment_keeps_derived_key() {
        let deck = parse_markdown("<!-- {\"layout\":\"cover\"} -->\n# My Title").unwrap();

        let slide = &deck.parsed_slides()[0];
        assert_eq!(slide.key.as_str(), "my-title");
        assert_eq!(slide.layout_request.as_ref().unwrap().name, "cover");
    }

    #[test]
    fn rejects_unknown_page_settings_fields() {
        let err = parse_markdown("<!-- {\"key\":\"a\",\"freeze\":true} -->\n# Title").unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(1));
        assert!(err.to_string().contains("invalid page settings comment"));
    }

    #[test]
    fn rejects_empty_page_settings_comment() {
        let err = parse_markdown("<!-- {} -->\n# Title").unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert!(err
            .to_string()
            .contains("page settings comment has no settings"));
    }

    #[test]
    fn rejects_unknown_code_language_tag_with_line() {
        let err = parse_markdown("# Title\n\n```notalang\nx\n```").unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(3));
        assert!(err.to_string().contains("unknown code language 'notalang'"));
    }

    #[test]
    fn untagged_code_block_needs_no_language() {
        let deck = parse_markdown("# Title\n\n```\nplain\n```").unwrap();

        assert_eq!(deck.parsed_slides()[0].fragments[1].language(), None);
    }

    #[test]
    fn rejects_broken_json_key_comments() {
        let err = parse_markdown("<!-- {\"kye\":\"arch-1\"} -->\n# Title").unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(1));
        assert!(err.to_string().contains("invalid page settings comment"));
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
    fn frontmatter_is_not_a_slide_and_later_rules_still_split_slides() {
        let markdown = "---\ntime: 15m\n---\n\n# Intro\n\n---\n# Details";
        let split = split_slide_ranges(markdown).unwrap();
        let frontmatter = split.frontmatter.as_ref().unwrap();

        assert_eq!(frontmatter.line, 1);
        assert_eq!(frontmatter.yaml, "time: 15m\n");

        let deck = parse_markdown(markdown).unwrap();
        assert_eq!(deck.parsed_slides().len(), 2);
        assert_eq!(deck.parsed_slides()[0].fragments[0].line(), 5);
        assert_eq!(deck.parsed_slides()[1].fragments[0].line(), 8);
    }

    #[test]
    fn rejects_leading_dense_rule_pair_as_invalid_frontmatter_not_dropped_slide() {
        let err = parse_markdown("---\n# Title\n---\n# Next").unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(2));
        assert!(err
            .to_string()
            .contains("unexpected line in deck frontmatter"));
        assert_eq!(
            err.help,
            "keep only key: value settings (like time: 15m) between the --- markers"
        );
    }

    #[test]
    fn blank_prefixed_frontmatter_is_accepted_without_dropping_content() {
        let deck = parse_markdown("\n---\ntime: 15m\n---\n\n# A").unwrap();
        let slides = deck.parsed_slides();

        assert_eq!(slides.len(), 1);
        assert_eq!(slides[0].key.as_str(), "a");
        assert_eq!(slides[0].fragments[0].line(), 6);
    }

    #[test]
    fn nonblank_prefix_before_rule_pair_keeps_existing_markdown_semantics() {
        let deck = parse_markdown("# Z\n\n---\ntitle: x\n---\n\n# A").unwrap();
        let slides = deck.parsed_slides();

        assert_eq!(slides.len(), 2);
        assert_eq!(slides[0].key.as_str(), "z");
        assert_eq!(slides[1].key.as_str(), "title-x");
        assert_eq!(slides[1].fragments[1].line(), 7);
        assert_eq!(slides[1].fragments[1].markdown(), "# A");
    }

    #[test]
    fn rejects_unknown_frontmatter_key_with_line_and_help() {
        let err = parse_markdown("---\ntime: 15m\nunknown: true\n---\n# Intro").unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(3));
        assert!(err.to_string().contains("invalid deck frontmatter"));
        assert_eq!(
            err.help,
            "use only the supported deck frontmatter key: time"
        );
    }

    #[test]
    fn rejects_invalid_frontmatter_time_with_line_and_help() {
        for value in ["0", "-1", "abc", ""] {
            let markdown = format!("---\ntime: {value}\n---\n# Intro");
            let err = parse_markdown(&markdown).unwrap_err();
            assert_eq!(err.kind, ErrorKind::Parse);
            assert_eq!(err.line, Some(2));
            assert!(err.to_string().contains("invalid deck frontmatter"));
            assert_eq!(
                err.help,
                "set time to 15m, 90s, 1h, 1h30m, or an integer minute count"
            );
        }
    }

    #[test]
    fn rejects_overflowing_planned_time_with_specific_message() {
        for yaml in [
            "time: 18446744073709551615\n",
            "time: 18446744073709551615m\n",
        ] {
            let err = serde_norway::from_str::<DeckFrontmatter>(yaml).unwrap_err();

            assert!(err.to_string().contains("time is too large"));
        }

        let err = serde_norway::from_str::<DeckFrontmatter>("time: 0\n").unwrap_err();
        assert!(err.to_string().contains("time must be greater than zero"));
    }

    #[test]
    fn huge_integer_time_uses_time_format_help() {
        let err = parse_markdown("---\ntime: 999999999999999999999\n---\n# Intro").unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(2));
        assert!(err.to_string().contains("invalid deck frontmatter"));
        assert_eq!(
            err.help,
            "set time to 15m, 90s, 1h, 1h30m, or an integer minute count"
        );
    }

    #[test]
    fn duplicate_frontmatter_time_reports_frontmatter_line() {
        let err = parse_markdown("---\ntime: 15m\ntime: 20m\n---\n# Intro").unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(2));
        assert!(err.to_string().contains("duplicate entry"));
        assert_eq!(
            err.help,
            "use only the supported deck frontmatter key: time"
        );
    }

    #[test]
    fn rejects_broken_frontmatter_yaml_with_line_and_help() {
        let err = parse_markdown("---\ntime: [\n---\n# Intro").unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(3));
        assert!(err.to_string().contains("invalid deck frontmatter"));
        assert!(!err.to_string().contains(" at line "));
        assert_eq!(
            err.help,
            "write valid YAML frontmatter before the first slide"
        );
    }

    #[test]
    fn rejects_metadata_block_inside_a_slide() {
        let source = "---\ntime: 15m\n---\n# Details";
        let err = parse_slide(
            source,
            SlideRange {
                start: 0,
                end: source.len(),
            },
            1,
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(1));
        assert!(err
            .to_string()
            .contains("YAML frontmatter is only allowed at the top of the deck"));
    }

    #[test]
    fn frontmatter_keeps_raw_yaml_for_error_line_mapping() {
        let err = parse_markdown("\n---\nunknown: true\n---\n# Intro").unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(3));
        assert!(err.to_string().contains("invalid deck frontmatter"));
        assert_eq!(
            err.help,
            "use only the supported deck frontmatter key: time"
        );
    }

    #[test]
    fn rejects_frontmatter_heading_line_as_missing_closing_delimiter() {
        let err = parse_markdown("---\ntime: 15m\n# Intro\n---\n# Details").unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(3));
        assert!(err
            .to_string()
            .contains("unexpected line in deck frontmatter"));
        assert_eq!(
            err.help,
            "keep only key: value settings (like time: 15m) between the --- markers"
        );
    }

    #[test]
    fn allows_trailing_blank_line_before_frontmatter_close() {
        let raw = RawFrontmatter {
            line: 1,
            yaml: "time: 15m\n\n".to_owned(),
        };

        let settings = parse_deck_frontmatter(Some(&raw)).unwrap();
        let deck = parse_markdown("---\ntime: 15m\n\n---\n\n# A").unwrap();

        assert_eq!(settings.time.map(PlannedTime::as_millis), Some(900_000));
        assert_eq!(deck.parsed_slides().len(), 1);
        assert_eq!(deck.parsed_slides()[0].key.as_str(), "a");
    }

    #[test]
    fn rejects_frontmatter_blank_line_as_missing_closing_delimiter() {
        let err = parse_markdown("---\ntime: 15m\n\n# A\n\n---\n\n# B").unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(3));
        assert!(err
            .to_string()
            .contains("unexpected line in deck frontmatter"));
        assert_eq!(
            err.help,
            "keep only key: value settings (like time: 15m) between the --- markers"
        );
    }

    #[test]
    fn bom_prefixed_frontmatter_is_parsed_normally() {
        let deck = parse_markdown("\u{feff}---\ntime: 15m\n---\n\n# A").unwrap();
        let slides = deck.parsed_slides();

        assert_eq!(slides.len(), 1);
        assert_eq!(slides[0].key.as_str(), "a");
    }

    #[test]
    fn rejects_leading_rule_when_no_valid_frontmatter_was_found() {
        for markdown in ["---\n\ntime: 15m\n---\n\n# A", "---\n---\n\n# A"] {
            let err = parse_markdown(markdown).unwrap_err();

            assert_eq!(err.kind, ErrorKind::Parse);
            assert_eq!(err.line, Some(1));
            assert!(err
                .to_string()
                .contains("deck starts with --- but no valid YAML frontmatter was found"));
            assert_eq!(
                err.help,
                "put --- on the first line, settings on the following lines, and close with --- without blank lines"
            );
        }
    }

    #[test]
    fn indented_leading_rule_does_not_hang_and_reports_invalid_frontmatter_start() {
        let err = parse_markdown("  ---\ntime: 15m\n---\n\n# A").unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(1));
        assert!(err
            .to_string()
            .contains("deck starts with --- but no valid YAML frontmatter was found"));
    }

    #[test]
    fn yaml_like_slide_content_between_rules_still_splits_as_slides() {
        let markdown = "# A\n\n---\n# Cfg\nport: 8080\n\n---\n\n# C";
        let deck = parse_markdown(markdown).unwrap();
        let slides = deck.parsed_slides();

        assert_eq!(slides.len(), 3);
        assert_eq!(slides[0].key.as_str(), "a");
        assert_eq!(slides[1].key.as_str(), "cfg");
        assert_eq!(slides[1].fragments[0].line(), 4);
        assert_eq!(slides[1].fragments[1].line(), 5);
        assert_eq!(slides[2].key.as_str(), "c");
    }

    #[test]
    fn rejects_empty_deck_after_splitting_delimiters() {
        let err = parse_markdown("  \n\n***\n\n***\n").unwrap_err();

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
