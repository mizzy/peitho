use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
};

use pulldown_cmark::{
    CodeBlockKind, Event, HeadingLevel, MetadataBlockKind, Options, Parser, Tag, TagEnd,
};
use serde::Deserialize;

use crate::{
    domain::{
        AspectRatio, CodeImageCommand, CodeImagesConfig, ExplicitSlot, FragmentKind, RawImagePath,
        Resolution, SlideKey, SlotName, SourceFragment,
    },
    error::{BuildError, ErrorKind, Result},
    highlight::Highlighter,
    phase::{
        AssetPath, Deck, DeckSection, DeckSettings, KeySource, LayoutRequest, Parsed, ParsedSlide,
        PlannedTime,
    },
};

/// Page settings comment, deck-style:
/// `<!-- {"key":"...","layout":"...","section":"...","time":"..."} -->`.
/// Unknown fields are rejected so a typo never silently drops a setting.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PageComment {
    key: Option<String>,
    layout: Option<String>,
    section: Option<String>,
    time: Option<PlannedTime>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PageSectionMarker {
    name: String,
    planned: PlannedTime,
    line: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeckFrontmatter {
    #[serde(default, deserialize_with = "deserialize_optional_planned_time")]
    time: Option<PlannedTime>,
    #[serde(default)]
    aspect_ratio: Option<String>,
    #[serde(default)]
    resolution: Option<String>,
    #[serde(default)]
    breaks: bool,
    #[serde(default)]
    layouts: Option<AssetPath>,
    #[serde(default)]
    css: Option<AssetPath>,
    #[serde(default)]
    syntaxes: Option<AssetPath>,
    #[serde(default)]
    fonts: Option<AssetPath>,
    #[serde(default)]
    code_images: Option<HashMap<String, String>>,
}

#[derive(Debug)]
struct PageSettings {
    key: Option<SlideKey>,
    layout: Option<String>,
    section: Option<PageSectionMarker>,
}

#[derive(Debug)]
struct ParsedSlideDraft {
    slide: ParsedSlide,
    section: Option<PageSectionMarker>,
}

#[derive(Debug)]
struct ResolvedSection {
    section: DeckSection,
    line: usize,
}

enum OpenBlock {
    Heading {
        level: u8,
        start: usize,
        text: String,
    },
    Paragraph {
        start: usize,
        inline: ParagraphInline,
    },
    Code {
        start: usize,
        language: Option<String>,
        text: String,
    },
}

enum ParagraphInline {
    Empty,
    Text,
    Image {
        start: usize,
        alt: String,
        src: RawImagePath,
        open: bool,
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

pub struct ParsedFrontmatter {
    settings: DeckSettings,
    pub(crate) body_start: usize,
    key_lines: HashMap<&'static str, usize>,
}

impl ParsedFrontmatter {
    pub fn settings(&self) -> &DeckSettings {
        &self.settings
    }

    pub fn key_line(&self, key: &str) -> Option<usize> {
        self.key_lines.get(key).copied()
    }
}

pub fn parse_frontmatter(source: &str) -> Result<ParsedFrontmatter> {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let (raw, body_start) = leading_frontmatter(source)?;
    if raw.is_none() {
        reject_invalid_leading_frontmatter_start(source)?;
    }
    let settings = parse_deck_frontmatter(raw.as_ref())?;
    let key_lines = frontmatter_key_lines(raw.as_ref());

    Ok(ParsedFrontmatter {
        settings,
        body_start,
        key_lines,
    })
}

pub fn parse_markdown(
    source: &str,
    frontmatter: ParsedFrontmatter,
    highlighter: &Highlighter,
) -> Result<Deck<Parsed>> {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let ParsedFrontmatter {
        settings,
        body_start,
        ..
    } = frontmatter;
    let ranges = split_slide_ranges(source, body_start)?;
    if ranges.is_empty() {
        return Err(BuildError::new(
            ErrorKind::Parse,
            None,
            "deck has no slides",
            "add at least one slide with content before building",
        ));
    }

    let mut drafts = Vec::new();
    for (index, range) in ranges.into_iter().enumerate() {
        drafts.push(parse_slide(
            source,
            range,
            index,
            highlighter,
            settings.code_images(),
        )?);
    }
    let resolved_sections = resolve_deck_sections(&drafts)?;
    let settings = finalize_section_settings(settings, resolved_sections)?;
    let slides = drafts
        .into_iter()
        .map(|draft| draft.slide)
        .collect::<Vec<_>>();
    validate_unique_keys(&slides)?;
    Ok(Deck::parsed(settings, slides))
}

fn split_slide_ranges(source: &str, content_start: usize) -> Result<Vec<SlideRange>> {
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
    Ok(ranges)
}

fn resolve_deck_sections(drafts: &[ParsedSlideDraft]) -> Result<Vec<ResolvedSection>> {
    let markers = drafts
        .iter()
        .enumerate()
        .filter_map(|(index, draft)| draft.section.as_ref().map(|marker| (index, marker)))
        .collect::<Vec<_>>();

    if markers.is_empty() {
        return Ok(Vec::new());
    }
    if markers[0].0 != 0 {
        return Err(BuildError::new(
            ErrorKind::Parse,
            Some(markers[0].1.line),
            "first slide must declare a section",
            "add a section marker to the first slide or remove all section markers",
        ));
    }

    Ok(markers
        .iter()
        .enumerate()
        .map(|(marker_index, (start, marker))| {
            let end = markers
                .get(marker_index + 1)
                .map(|(next_start, _)| next_start - 1)
                .unwrap_or(drafts.len() - 1);
            ResolvedSection {
                section: DeckSection::new(marker.name.clone(), marker.planned, *start, end),
                line: marker.line,
            }
        })
        .collect())
}

fn finalize_section_settings(
    settings: DeckSettings,
    resolved_sections: Vec<ResolvedSection>,
) -> Result<DeckSettings> {
    if resolved_sections.is_empty() {
        return Ok(settings.with_sections(Vec::new()));
    }

    let first_line = resolved_sections[0].line;
    let total = section_total_from_millis(
        resolved_sections
            .iter()
            .map(|resolved| (resolved.line, resolved.section.planned().as_millis())),
    )?;
    let section_total =
        PlannedTime::from_millis(total).expect("section total was checked before conversion");
    let sections = resolved_sections
        .into_iter()
        .map(|resolved| resolved.section)
        .collect::<Vec<_>>();

    if let Some(frontmatter) = settings.planned_time() {
        if frontmatter.as_millis() != section_total.as_millis() {
            return Err(BuildError::new(
                ErrorKind::Parse,
                Some(first_line),
                format!(
                    "frontmatter time {}ms does not match section total {}ms",
                    frontmatter.as_millis(),
                    section_total.as_millis()
                ),
                "adjust frontmatter time or section times so the totals match",
            ));
        }
        return Ok(settings.with_sections(sections));
    }

    Ok(settings
        .with_planned_time(Some(section_total))
        .with_sections(sections))
}

fn section_total_from_millis<I>(items: I) -> Result<u64>
where
    I: IntoIterator<Item = (usize, u64)>,
{
    let mut total = 0_u64;
    for (line, millis) in items {
        total = total.checked_add(millis).ok_or_else(|| {
            BuildError::new(
                ErrorKind::Parse,
                Some(line),
                "section time total overflowed",
                "reduce section times so the total can be represented safely",
            )
        })?;
        if total > PlannedTime::MAX_SAFE_JAVASCRIPT_INTEGER_MILLIS {
            return Err(BuildError::new(
                ErrorKind::Parse,
                Some(line),
                "section time total is too large",
                "reduce section times so the total is at most Number.MAX_SAFE_INTEGER milliseconds",
            ));
        }
    }
    Ok(total)
}

fn deserialize_optional_planned_time<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<PlannedTime>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    PlannedTime::deserialize(deserializer).map(Some)
}

const ASSET_PATH_EMPTY_MESSAGE: &str = "value is empty";

fn parse_planned_time_text(input: &str) -> std::result::Result<u64, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("time must not be empty".to_owned());
    }
    if trimmed.bytes().all(|byte| byte.is_ascii_digit()) {
        let minutes = trimmed
            .parse::<u64>()
            .map_err(|_| PlannedTime::TOO_LARGE_MESSAGE.to_owned())?;
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
            .map_err(|_| PlannedTime::TOO_LARGE_MESSAGE.to_owned())?;
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
        .ok_or_else(|| PlannedTime::TOO_LARGE_MESSAGE.to_owned())?;
        total_seconds = total_seconds
            .checked_add(seconds)
            .ok_or_else(|| PlannedTime::TOO_LARGE_MESSAGE.to_owned())?;
        rest = &rest[1..];
    }

    seconds_to_millis(total_seconds)
}

fn minutes_to_millis(minutes: u64) -> std::result::Result<u64, String> {
    minutes
        .checked_mul(60_000)
        .ok_or_else(|| PlannedTime::TOO_LARGE_MESSAGE.to_owned())
}

fn seconds_to_millis(seconds: u64) -> std::result::Result<u64, String> {
    seconds
        .checked_mul(1000)
        .ok_or_else(|| PlannedTime::TOO_LARGE_MESSAGE.to_owned())
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
                minutes_to_millis(value)
                    .and_then(PlannedTime::from_millis)
                    .map_err(E::custom)
            }

            fn visit_i64<E>(self, value: i64) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if value < 0 {
                    return Err(E::custom("time must not be negative"));
                }
                self.visit_u64(value as u64)
            }

            fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                parse_planned_time_text(value)
                    .and_then(PlannedTime::from_millis)
                    .map_err(E::custom)
            }
        }

        deserializer.deserialize_any(PlannedTimeVisitor)
    }
}

impl<'de> Deserialize<'de> for AssetPath {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct AssetPathVisitor;

        impl<'de> serde::de::Visitor<'de> for AssetPathVisitor {
            type Value = AssetPath;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a non-empty asset path string")
            }

            fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                if value.trim().is_empty() {
                    return Err(E::custom(ASSET_PATH_EMPTY_MESSAGE));
                }
                Ok(AssetPath::new(PathBuf::from(value)))
            }
        }

        deserializer.deserialize_str(AssetPathVisitor)
    }
}

fn parse_deck_frontmatter(raw: Option<&RawFrontmatter>) -> Result<DeckSettings> {
    let Some(raw) = raw else {
        return Ok(DeckSettings::default());
    };
    if raw.yaml.trim().is_empty() {
        return Ok(DeckSettings::default());
    }
    validate_frontmatter_lines(raw)?;
    let yaml = normalize_frontmatter_yaml_for_parse(&raw.yaml);

    let value: serde_norway::Value =
        serde_norway::from_str(&yaml).map_err(|err| frontmatter_yaml_error(raw, &err))?;
    if !matches!(value, serde_norway::Value::Mapping(_)) {
        return Err(BuildError::new(
            ErrorKind::Parse,
            Some(raw.line + first_nonblank_yaml_line(&raw.yaml)),
            "invalid deck frontmatter: expected a YAML mapping of deck settings",
            "write valid YAML frontmatter before the first slide",
        ));
    }

    let key_lines = frontmatter_key_lines(Some(raw));
    let parsed: DeckFrontmatter =
        serde_norway::from_str(&yaml).map_err(|err| frontmatter_yaml_error(raw, &err))?;
    let aspect_ratio = parse_frontmatter_aspect_ratio(
        parsed.aspect_ratio,
        key_lines.get("aspect_ratio").copied(),
    )?;
    let resolution =
        parse_frontmatter_resolution(parsed.resolution, key_lines.get("resolution").copied())?;
    let code_images =
        parse_code_images_config(parsed.code_images, key_lines.get("code_images").copied())?;

    DeckSettings::new(
        parsed.time,
        aspect_ratio,
        resolution,
        parsed.breaks,
        Vec::new(),
        parsed.layouts,
        parsed.css,
        parsed.syntaxes,
        parsed.fonts,
        code_images,
    )
    .map_err(|message| {
        let help = deck_settings_resolution_error_help(&message);
        BuildError::new(
            ErrorKind::Parse,
            key_lines.get("resolution").copied(),
            message,
            help,
        )
    })
}

fn normalize_frontmatter_yaml_for_parse(yaml: &str) -> String {
    let mut normalized = String::with_capacity(yaml.len());
    for line in yaml.split_inclusive('\n') {
        let mut body_start = line.len();
        for (index, ch) in line.char_indices() {
            match ch {
                ' ' => normalized.push(' '),
                '\t' => normalized.push_str("  "),
                _ => {
                    body_start = index;
                    break;
                }
            }
        }
        if body_start < line.len() {
            normalized.push_str(&line[body_start..]);
        }
    }
    normalized
}

fn parse_code_images_config(
    entries: Option<HashMap<String, String>>,
    key_line: Option<usize>,
) -> Result<CodeImagesConfig> {
    let mut parsed_entries = BTreeMap::new();
    for (tag, command) in entries.unwrap_or_default() {
        let argv = shlex::split(&command).ok_or_else(|| {
            BuildError::new(
                ErrorKind::Parse,
                key_line,
                format!("code_images entry '{tag}' has invalid command string"),
                "quote the command using shell-like syntax or remove the unmatched quote",
            )
        })?;
        if argv.is_empty() {
            return Err(BuildError::new(
                ErrorKind::Parse,
                key_line,
                format!("code_images entry '{tag}' has empty command"),
                format!("set code_images.{tag} to a command, like mmdc -i - -o - -e svg"),
            ));
        }
        parsed_entries.insert(tag, CodeImageCommand { argv });
    }

    Ok(CodeImagesConfig {
        entries: parsed_entries,
        key_line,
    })
}

fn deck_settings_resolution_error_help(message: &str) -> &'static str {
    if message.contains("smaller than the canvas logical size") {
        "set resolution to at least the canvas dimensions or remove resolution to use the default"
    } else {
        "set resolution to match aspect_ratio or remove resolution to use the default"
    }
}

fn frontmatter_key_lines(raw: Option<&RawFrontmatter>) -> HashMap<&'static str, usize> {
    let mut key_lines = HashMap::new();
    let Some(raw) = raw else {
        return key_lines;
    };

    for (index, line) in raw.yaml.lines().enumerate() {
        for key in [
            "time",
            "aspect_ratio",
            "resolution",
            "breaks",
            "layouts",
            "css",
            "syntaxes",
            "fonts",
            "code_images",
        ] {
            if line.starts_with(key) && line.as_bytes().get(key.len()) == Some(&b':') {
                key_lines.entry(key).or_insert(raw.line + index + 1);
            }
        }
    }

    key_lines
}

fn first_nonblank_yaml_line(yaml: &str) -> usize {
    yaml.lines()
        .position(|line| !line.trim().is_empty())
        .map(|index| index + 1)
        .unwrap_or(1)
}

fn parse_frontmatter_aspect_ratio(
    value: Option<String>,
    line: Option<usize>,
) -> Result<AspectRatio> {
    match (value.as_deref(), line) {
        (None, None) => Ok(AspectRatio::default()),
        (Some(value), line) => value
            .parse()
            .map_err(|_| unknown_aspect_ratio_error(value, line)),
        (None, Some(line)) => Err(BuildError::new(
            ErrorKind::Parse,
            Some(line),
            "aspect_ratio has no value",
            "set aspect_ratio to 16:9 or 4:3",
        )),
    }
}

fn unknown_aspect_ratio_error(value: &str, line: Option<usize>) -> BuildError {
    BuildError::new(
        ErrorKind::Parse,
        line,
        format!("unknown aspect_ratio '{value}'; use one of: 16:9, 4:3"),
        "set aspect_ratio to 16:9 or 4:3",
    )
}

fn parse_frontmatter_resolution(
    value: Option<String>,
    line: Option<usize>,
) -> Result<Option<Resolution>> {
    match (value.as_deref(), line) {
        (None, None) => Ok(None),
        (Some(value), line) => {
            let resolution = Resolution::from_frontmatter(value).map_err(|message| {
                BuildError::new(
                    ErrorKind::Parse,
                    line,
                    message,
                    "set resolution to WxH pixels, like 1920x1080",
                )
            })?;
            Ok(Some(resolution))
        }
        (None, Some(line)) => Err(BuildError::new(
            ErrorKind::Parse,
            Some(line),
            "resolution has no value",
            "set resolution to WxH pixels, like 1920x1080",
        )),
    }
}

fn validate_frontmatter_lines(raw: &RawFrontmatter) -> Result<()> {
    let lines = raw.yaml.lines().collect::<Vec<_>>();
    let content_len = lines
        .iter()
        .rposition(|line| !line.trim().is_empty())
        .map(|index| index + 1)
        .unwrap_or(0);

    let mut nested_mapping: Option<NestedFrontmatterMappingState> = None;
    for (index, line) in lines[..content_len].iter().enumerate() {
        if line.starts_with(char::is_whitespace) {
            if let Some(mapping) = nested_mapping.as_mut() {
                if let Some(entry) = nested_frontmatter_mapping_entry(line) {
                    match &mapping.indent {
                        None => {
                            mapping.indent = Some(entry.indent.to_owned());
                            mapping.previous_entry_had_value = entry.has_value;
                            continue;
                        }
                        Some(expected) if expected == entry.indent => {
                            mapping.previous_entry_had_value = entry.has_value;
                            continue;
                        }
                        Some(_) if mapping.previous_entry_had_value => {
                            return Err(BuildError::new(
                                ErrorKind::Parse,
                                Some(raw.line + index + 1),
                                "inconsistent indentation in code_images frontmatter",
                                "use the same indentation for every code_images entry",
                            ));
                        }
                        Some(_) => {
                            return Err(BuildError::new(
                                ErrorKind::Parse,
                                Some(raw.line + index + 1),
                                "unsupported nested mapping in code_images frontmatter",
                                "use code_images entries as tag: command strings, like mermaid: mmdc -i - -o - -e svg",
                            ));
                        }
                    }
                }
                if line.trim_start().contains(':') {
                    return Err(BuildError::new(
                        ErrorKind::Parse,
                        Some(raw.line + index + 1),
                        "unsupported nested mapping in code_images frontmatter",
                        "use code_images entries as tag: command strings, like mermaid: mmdc -i - -o - -e svg",
                    ));
                }
            }
            return Err(BuildError::new(
                ErrorKind::Parse,
                Some(raw.line + index + 1),
                "unexpected line in deck frontmatter (missing closing ---?)",
                "keep only key: value settings (like time: 15m) between the --- markers",
            ));
        }

        let Some((key, has_value)) = flat_yaml_key_and_value_presence(line) else {
            return Err(BuildError::new(
                ErrorKind::Parse,
                Some(raw.line + index + 1),
                "unexpected line in deck frontmatter (missing closing ---?)",
                "keep only key: value settings (like time: 15m) between the --- markers",
            ));
        };
        nested_mapping = (!has_value && allows_nested_frontmatter_mapping(key))
            .then_some(NestedFrontmatterMappingState::default());
    }

    Ok(())
}

#[derive(Default)]
struct NestedFrontmatterMappingState {
    indent: Option<String>,
    previous_entry_had_value: bool,
}

struct NestedFrontmatterMappingEntry<'a> {
    indent: &'a str,
    has_value: bool,
}

fn flat_yaml_key_and_value_presence(line: &str) -> Option<(&str, bool)> {
    let colon_index = line.find(':')?;
    let key = &line[..colon_index];
    let valid = !key.is_empty()
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
    valid.then(|| {
        let value = &line[colon_index + 1..];
        (key, !value.trim().is_empty())
    })
}

fn allows_nested_frontmatter_mapping(key: &str) -> bool {
    matches!(key, "code_images")
}

fn nested_frontmatter_mapping_entry(line: &str) -> Option<NestedFrontmatterMappingEntry<'_>> {
    let trimmed = line.trim_start();
    let indent = &line[..line.len() - trimmed.len()];
    if indent.is_empty() {
        return None;
    }
    flat_yaml_key_and_value_presence(trimmed)
        .map(|(_, has_value)| NestedFrontmatterMappingEntry { indent, has_value })
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
        "use only the supported deck frontmatter keys: time, aspect_ratio, resolution, breaks, layouts, css, syntaxes, fonts, code_images"
    } else if frontmatter_message_mentions_key(message, "aspect_ratio") {
        "set aspect_ratio to 16:9 or 4:3"
    } else if frontmatter_message_mentions_key(message, "resolution") {
        "set resolution to WxH pixels, like 1920x1080"
    } else if frontmatter_message_mentions_key(message, "breaks") {
        "set breaks to true or false"
    } else if frontmatter_message_mentions_key(message, "layouts") {
        "provide a path (relative to the deck file), or remove the layouts: key"
    } else if frontmatter_message_mentions_key(message, "css") {
        "provide a path (relative to the deck file), or remove the css: key"
    } else if frontmatter_message_mentions_key(message, "syntaxes") {
        "provide a path (relative to the deck file), or remove the syntaxes: key"
    } else if frontmatter_message_mentions_key(message, "fonts") {
        "provide a path (relative to the deck file), or remove the fonts: key"
    } else if frontmatter_message_mentions_key(message, "time")
        || message.contains("duration")
        || message.contains(PlannedTime::GREATER_THAN_ZERO_MESSAGE)
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

fn frontmatter_message_mentions_key(message: &str, key: &str) -> bool {
    message.contains(&format!("`{key}`")) || message.contains(&format!("{key}:"))
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

/// A `:::` line inside a slide, marking the start or end of an explicit
/// slot group.
#[derive(Debug, Clone)]
enum SlotDivMarker {
    Open(ExplicitSlot),
    Close,
}

/// Scan the slide's slice line by line and return `line -> SlotDivMarker`.
/// Lines inside fenced code blocks (` ``` ` or `~~~`) are excluded so the
/// author can write about the syntax itself in a code block.
///
/// Fences of four or more colons (`::::`) are rejected up front — spec's
/// forward-compatible reservation for future nested-div notation.
fn scan_slot_div_markers(
    slice: &str,
    slice_start_offset: usize,
    source: &str,
) -> Result<std::collections::HashMap<usize, SlotDivMarker>> {
    let mut markers = std::collections::HashMap::new();
    let mut in_code_fence: Option<(char, usize)> = None;
    let mut line_start = 0usize;

    for raw_line in slice.split_inclusive('\n') {
        let line_text = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        let trimmed = line_text.trim_end_matches('\r');
        let line_offset = slice_start_offset + line_start;
        let line_no = line_for_offset(source, line_offset);

        // Track fenced code blocks so ::: inside them is not a marker.
        if let Some((fence_char, fence_len)) = in_code_fence {
            if is_closing_code_fence(trimmed, fence_char, fence_len) {
                in_code_fence = None;
            }
            line_start += raw_line.len();
            continue;
        }
        if let Some((ch, len)) = opening_code_fence(trimmed) {
            in_code_fence = Some((ch, len));
            line_start += raw_line.len();
            continue;
        }

        // Reject long colon fences (four or more colons) explicitly.
        if trimmed.starts_with("::::") {
            return Err(BuildError::new(
                ErrorKind::Parse,
                Some(line_no),
                "explicit slot fence uses too many colons",
                "use exactly three colons: `::: {slot=name}` and `:::`",
            ));
        }

        // Not a `:::` line? move on.
        if !trimmed.starts_with(":::") {
            line_start += raw_line.len();
            continue;
        }
        // Reject a leading-space `:::` (must be at column 0).
        if line_text.starts_with(char::is_whitespace) {
            line_start += raw_line.len();
            continue;
        }

        let rest = trimmed[3..].trim_start();
        let marker = if rest.is_empty() {
            SlotDivMarker::Close
        } else {
            let slot = parse_slot_div_attributes(rest, line_no)?;
            SlotDivMarker::Open(slot)
        };
        markers.insert(line_no, marker);
        line_start += raw_line.len();
    }
    Ok(markers)
}

/// Extract the leading fence character (`\`` or `~`) and its length, if this
/// line opens a code fence. Only fences of three or more of the same character
/// count, matching pulldown-cmark semantics.
fn opening_code_fence(line: &str) -> Option<(char, usize)> {
    let trimmed = line.trim_start();
    let ch = trimmed.chars().next()?;
    if ch != '`' && ch != '~' {
        return None;
    }
    let count = trimmed.chars().take_while(|c| *c == ch).count();
    if count >= 3 {
        Some((ch, count))
    } else {
        None
    }
}

/// A closing fence must be the same character and at least as long.
fn is_closing_code_fence(line: &str, ch: char, min_len: usize) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }
    let count = trimmed.chars().take_while(|c| *c == ch).count();
    count >= min_len && trimmed.chars().skip(count).all(|c| c.is_whitespace())
}

/// Parse the attribute chunk that follows `:::` on an opening line.
/// Only `{slot=name}` is accepted; anything else is a parse error.
fn parse_slot_div_attributes(rest: &str, line: usize) -> Result<ExplicitSlot> {
    let rest = rest.trim_end();
    let Some(inner) = rest.strip_prefix('{').and_then(|s| s.strip_suffix('}')) else {
        return Err(BuildError::new(
            ErrorKind::Parse,
            Some(line),
            "explicit slot fence must use `{slot=name}` attribute syntax",
            "write the opening as `::: {slot=<name>}`",
        ));
    };
    let inner = inner.trim();
    // Reject anything with a whitespace-separated second attribute.
    let key_value = inner.split_whitespace().collect::<Vec<_>>();
    if key_value.len() != 1 {
        return Err(BuildError::new(
            ErrorKind::Parse,
            Some(line),
            "explicit slot fence accepts only one attribute",
            "use exactly `{slot=<name>}`",
        ));
    }
    let attr = key_value[0];
    let Some((key, value)) = attr.split_once('=') else {
        return Err(BuildError::new(
            ErrorKind::Parse,
            Some(line),
            "explicit slot fence attribute must be `slot=<name>`",
            "write the opening as `::: {slot=<name>}`",
        ));
    };
    if key != "slot" {
        return Err(BuildError::new(
            ErrorKind::Parse,
            Some(line),
            format!("unknown attribute key '{key}' in explicit slot fence"),
            "only `slot=<name>` is accepted",
        ));
    }
    if value.is_empty() {
        return Err(BuildError::new(
            ErrorKind::Parse,
            Some(line),
            "explicit slot fence attribute needs a slot name",
            "write the opening as `::: {slot=<name>}`",
        ));
    }
    let slot = SlotName::new(value).map_err(|e| {
        BuildError::new(
            ErrorKind::Parse,
            Some(line),
            format!("invalid slot name '{value}': {e}"),
            "use lowercase ascii letters, digits, or '-'",
        )
    })?;
    Ok(ExplicitSlot::new(slot))
}

/// Push a fragment into either the outer fragment list or the currently-open
/// slot group's children, keeping call sites free of stack bookkeeping.
fn push_fragment(
    fragments: &mut Vec<SourceFragment>,
    slot_group_stack: &mut [(usize, ExplicitSlot, Vec<SourceFragment>)],
    fragment: SourceFragment,
) {
    if let Some(top) = slot_group_stack.last_mut() {
        top.2.push(fragment);
    } else {
        fragments.push(fragment);
    }
}

fn parse_slide(
    source: &str,
    range: SlideRange,
    index: usize,
    highlighter: &Highlighter,
    code_images: &CodeImagesConfig,
) -> Result<ParsedSlideDraft> {
    let slice = &source[range.start..range.end];
    let markers = scan_slot_div_markers(slice, range.start, source)?;
    let mut explicit_key: Option<(SlideKey, usize)> = None;
    let mut layout_request: Option<LayoutRequest> = None;
    let mut section_marker: Option<PageSectionMarker> = None;
    let mut page_settings_line: Option<usize> = None;
    let mut fragments = Vec::new();
    // Stack of open SlotGroups. Each frame owns the children collected so far
    // for that group; the top of stack is the current push target.
    let mut slot_group_stack: Vec<(usize, ExplicitSlot, Vec<SourceFragment>)> = Vec::new();
    let mut note_fragments: Vec<String> = Vec::new();
    let mut block: Option<OpenBlock> = None;
    let mut list_depth = 0usize;
    let mut list_start = None;
    let mut seen_content = false;
    // An HTML block can span multiple `Event::Html` events (one per source line
    // for a multi-line comment). We buffer them between Start(HtmlBlock)/End
    // and analyse the joined text once, so a multi-line `<!-- ... -->` isn't
    // mistaken for an "unsupported html" per line.
    let mut html_buf: Option<(String, usize)> = None;

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
                        push_fragment(
                            &mut fragments,
                            &mut slot_group_stack,
                            SourceFragment::list(
                                line_for_offset(source, start),
                                source_slice(source, start, global_end),
                            ),
                        );
                        seen_content = true;
                    }
                }
            }
            Event::Html(html) | Event::InlineHtml(html) => {
                if let Some((buf, _)) = html_buf.as_mut() {
                    buf.push_str(html.as_ref());
                } else {
                    // InlineHtml (or a stray Html outside a HtmlBlock): process
                    // as a single-event chunk immediately.
                    let ctx = explicit_key.clone();
                    process_html_chunk(
                        html.as_ref(),
                        line,
                        index,
                        &fragments,
                        &ctx,
                        list_depth,
                        seen_content,
                        &mut explicit_key,
                        &mut layout_request,
                        &mut section_marker,
                        &mut page_settings_line,
                        &mut note_fragments,
                    )?;
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
            Event::Start(Tag::Image { dest_url, .. }) => {
                if list_depth > 0 {
                    let err = unsupported_construct(line, "image inside list");
                    return Err(attach_slide_context(
                        err,
                        index,
                        explicit_key.as_ref(),
                        &fragments,
                    ));
                }
                let Some(OpenBlock::Paragraph { inline, .. }) = block.as_mut() else {
                    let err = unsupported_construct(line, "image outside paragraph");
                    return Err(attach_slide_context(
                        err,
                        index,
                        explicit_key.as_ref(),
                        &fragments,
                    ));
                };
                let src = RawImagePath::new(dest_url.to_string(), line).map_err(|err| {
                    attach_slide_context(err, index, explicit_key.as_ref(), &fragments)
                })?;
                if !start_paragraph_image(inline, global_start, src) {
                    let err = unsupported_construct(line, "mixed image paragraph");
                    return Err(attach_slide_context(
                        err,
                        index,
                        explicit_key.as_ref(),
                        &fragments,
                    ));
                }
            }
            Event::End(TagEnd::Image) => {
                let Some(OpenBlock::Paragraph { inline, .. }) = block.as_mut() else {
                    let err = unsupported_construct(line, "image end without image start");
                    return Err(attach_slide_context(
                        err,
                        index,
                        explicit_key.as_ref(),
                        &fragments,
                    ));
                };
                if !finish_paragraph_image(inline) {
                    let err = unsupported_construct(line, "image end without image start");
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
                    push_fragment(
                        &mut fragments,
                        &mut slot_group_stack,
                        SourceFragment::heading(
                            line_for_offset(source, start),
                            level,
                            source_slice(source, start, global_end),
                            text.trim(),
                        ),
                    );
                    seen_content = true;
                }
            }
            Event::Start(Tag::Paragraph) => {
                block = Some(OpenBlock::Paragraph {
                    start: global_start,
                    inline: ParagraphInline::Empty,
                });
            }
            Event::End(TagEnd::Paragraph) => {
                if matches!(block, Some(OpenBlock::Paragraph { .. })) {
                    let Some(OpenBlock::Paragraph { start, inline }) = block.take() else {
                        unreachable!();
                    };
                    let paragraph_line = line_for_offset(source, start);
                    // If this paragraph was actually a `:::` marker line, drop
                    // the paragraph and manipulate the slot group stack instead.
                    if let Some(marker) = markers.get(&paragraph_line).cloned() {
                        match marker {
                            SlotDivMarker::Open(name) => {
                                if !slot_group_stack.is_empty() {
                                    let err = BuildError::new(
                                        ErrorKind::Parse,
                                        Some(paragraph_line),
                                        "nested explicit slot fences are not supported",
                                        "close the current `:::` before opening another",
                                    );
                                    return Err(attach_slide_context(
                                        err,
                                        index,
                                        explicit_key.as_ref(),
                                        &fragments,
                                    ));
                                }
                                slot_group_stack.push((paragraph_line, name, Vec::new()));
                            }
                            SlotDivMarker::Close => {
                                let Some((open_line, name, children)) = slot_group_stack.pop()
                                else {
                                    let err = BuildError::new(
                                        ErrorKind::Parse,
                                        Some(paragraph_line),
                                        "closing `:::` has no matching opening fence",
                                        "add an opening `::: {slot=name}` before this line",
                                    );
                                    return Err(attach_slide_context(
                                        err,
                                        index,
                                        explicit_key.as_ref(),
                                        &fragments,
                                    ));
                                };
                                if children.is_empty() {
                                    let err = BuildError::new(
                                        ErrorKind::Parse,
                                        Some(open_line),
                                        "empty explicit slot fence",
                                        "add content between the opening and closing `:::`, or remove the fence",
                                    );
                                    return Err(attach_slide_context(
                                        err,
                                        index,
                                        explicit_key.as_ref(),
                                        &fragments,
                                    ));
                                }
                                let group = SourceFragment::slot_group(open_line, name, children);
                                fragments.push(group);
                                seen_content = true;
                            }
                        }
                        continue;
                    }

                    let fragment = match inline {
                        ParagraphInline::Empty | ParagraphInline::Text => {
                            SourceFragment::paragraph(
                                paragraph_line,
                                source_slice(source, start, global_end),
                            )
                        }
                        ParagraphInline::Image {
                            start,
                            alt,
                            src,
                            open: false,
                        } => SourceFragment::image(line_for_offset(source, start), alt, src),
                        ParagraphInline::Image { open: true, .. } => {
                            let err = unsupported_construct(line, "image without image end");
                            return Err(attach_slide_context(
                                err,
                                index,
                                explicit_key.as_ref(),
                                &fragments,
                            ));
                        }
                    };
                    push_fragment(&mut fragments, &mut slot_group_stack, fragment);
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
                        if !code_images.entries.contains_key(language) {
                            highlighter
                                .validate_language(language, code_line)
                                .map_err(|err| {
                                    attach_slide_context(
                                        err,
                                        index,
                                        explicit_key.as_ref(),
                                        &fragments,
                                    )
                                })?;
                        }
                    }
                    push_fragment(
                        &mut fragments,
                        &mut slot_group_stack,
                        SourceFragment::code(code_line, language, text),
                    );
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
                Some(OpenBlock::Paragraph { inline, .. }) => {
                    if !push_paragraph_text(inline, &text) {
                        let err = unsupported_construct(line, "mixed image paragraph");
                        return Err(attach_slide_context(
                            err,
                            index,
                            explicit_key.as_ref(),
                            &fragments,
                        ));
                    }
                }
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
            Event::SoftBreak | Event::HardBreak => match block.as_mut() {
                Some(OpenBlock::Code { text, .. }) => text.push('\n'),
                Some(OpenBlock::Paragraph { inline, .. }) => {
                    match push_paragraph_text(inline, "\n") {
                        true => {}
                        false => {
                            let err = unsupported_construct(line, "mixed image paragraph");
                            return Err(attach_slide_context(
                                err,
                                index,
                                explicit_key.as_ref(),
                                &fragments,
                            ));
                        }
                    }
                }
                _ => {}
            },
            Event::Start(Tag::HtmlBlock) => {
                html_buf = Some((String::new(), line));
            }
            Event::End(TagEnd::HtmlBlock) => {
                if let Some((buf, start_line)) = html_buf.take() {
                    let ctx = explicit_key.clone();
                    process_html_chunk(
                        &buf,
                        start_line,
                        index,
                        &fragments,
                        &ctx,
                        list_depth,
                        seen_content,
                        &mut explicit_key,
                        &mut layout_request,
                        &mut section_marker,
                        &mut page_settings_line,
                        &mut note_fragments,
                    )?;
                }
            }
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
            | Event::End(TagEnd::Link) => {
                if matches!(
                    block,
                    Some(OpenBlock::Paragraph {
                        inline: ParagraphInline::Image { open: true, .. },
                        ..
                    })
                ) {
                    let err = unsupported_construct(line, "formatted image alt text");
                    return Err(attach_slide_context(
                        err,
                        index,
                        explicit_key.as_ref(),
                        &fragments,
                    ));
                }
            }
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

    if let Some((open_line, _, _)) = slot_group_stack.first() {
        let err = BuildError::new(
            ErrorKind::Parse,
            Some(*open_line),
            "unclosed explicit slot fence",
            "add a closing `:::` line before the end of the slide",
        );
        return Err(attach_slide_context(
            err,
            index,
            explicit_key.as_ref(),
            &fragments,
        ));
    }

    let (key, key_source) = explicit_key
        .map(|(key, line)| (key, KeySource::Explicit { line }))
        .unwrap_or_else(|| {
            let key = derive_key_from_fragments(&fragments, index);
            let key_source = derived_key_source(&fragments);
            (key, key_source)
        });

    let notes = if note_fragments.is_empty() {
        None
    } else {
        Some(note_fragments.join("\n\n"))
    };

    Ok(ParsedSlideDraft {
        slide: ParsedSlide {
            index,
            key,
            key_source,
            layout_request,
            fragments,
            notes,
        },
        section: section_marker,
    })
}

fn start_paragraph_image(inline: &mut ParagraphInline, start: usize, src: RawImagePath) -> bool {
    match inline {
        ParagraphInline::Empty => {
            *inline = ParagraphInline::Image {
                start,
                alt: String::new(),
                src,
                open: true,
            };
            true
        }
        ParagraphInline::Text | ParagraphInline::Image { .. } => false,
    }
}

fn finish_paragraph_image(inline: &mut ParagraphInline) -> bool {
    match inline {
        ParagraphInline::Image { open, .. } if *open => {
            *open = false;
            true
        }
        ParagraphInline::Empty | ParagraphInline::Text | ParagraphInline::Image { .. } => false,
    }
}

fn push_paragraph_text(inline: &mut ParagraphInline, text: &str) -> bool {
    match inline {
        ParagraphInline::Empty => {
            if !text.trim().is_empty() {
                *inline = ParagraphInline::Text;
            }
            true
        }
        ParagraphInline::Text => true,
        ParagraphInline::Image {
            alt, open: true, ..
        } => {
            alt.push_str(text);
            true
        }
        ParagraphInline::Image { open: false, .. } => text.trim().is_empty(),
    }
}

/// Handle one whole HTML chunk (a completed HtmlBlock's joined text, or one
/// stray inline HTML event). Dispatches to page-settings parsing, note
/// collection, or an "unsupported html" error.
#[allow(clippy::too_many_arguments)]
fn process_html_chunk(
    raw: &str,
    line: usize,
    index: usize,
    fragments: &[SourceFragment],
    explicit_key_ctx: &Option<(SlideKey, usize)>,
    list_depth: usize,
    seen_content: bool,
    explicit_key: &mut Option<(SlideKey, usize)>,
    layout_request: &mut Option<LayoutRequest>,
    section_marker: &mut Option<PageSectionMarker>,
    page_settings_line: &mut Option<usize>,
    note_fragments: &mut Vec<String>,
) -> Result<()> {
    if let Some(settings) = parse_page_comment(raw, line)
        .map_err(|err| attach_slide_context(err, index, explicit_key_ctx.as_ref(), fragments))?
    {
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
                explicit_key_ctx.as_ref(),
                fragments,
            ));
        }
        if page_settings_line.is_some() {
            let err = BuildError::new(
                ErrorKind::Parse,
                Some(line),
                "duplicate page settings comment",
                "merge the settings into the first page settings comment",
            );
            return Err(attach_slide_context(
                err,
                index,
                explicit_key_ctx.as_ref(),
                fragments,
            ));
        }
        *page_settings_line = Some(line);
        if let Some(key) = settings.key {
            *explicit_key = Some((key, line));
        }
        if let Some(name) = settings.layout {
            *layout_request = Some(LayoutRequest { name, line });
        }
        if let Some(section) = settings.section {
            *section_marker = Some(section);
        }
        return Ok(());
    }
    if is_html_comment(raw) {
        if let Some(text) = extract_html_comment_body(raw) {
            note_fragments.push(text);
        }
        return Ok(());
    }
    if !raw.trim().is_empty() {
        let err = unsupported_construct(line, "html");
        return Err(attach_slide_context(
            err,
            index,
            explicit_key_ctx.as_ref(),
            fragments,
        ));
    }
    Ok(())
}

/// Extract the inner text of an HTML comment (between `<!--` and `-->`).
/// Trims surrounding whitespace and returns `None` if the body is empty,
/// so `<!-- -->` and `<!--\n\n-->` are silently ignored (not treated as notes).
fn extract_html_comment_body(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let body = trimmed.strip_prefix("<!--")?.strip_suffix("-->")?.trim();
    if body.is_empty() {
        None
    } else {
        Some(body.to_owned())
    }
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
            r#"use <!-- {"key":"arch-1","layout":"cover","section":"Setup","time":"1m"} --> (key/layout optional; section and time must appear together; no other fields)"#,
        )
    })?;
    if parsed.key.is_none()
        && parsed.layout.is_none()
        && parsed.section.is_none()
        && parsed.time.is_none()
    {
        return Err(BuildError::new(
            ErrorKind::Parse,
            Some(line),
            "page settings comment has no settings",
            r#"set "key", "layout", and/or a "section" with "time", or remove the comment"#,
        ));
    }
    let section = match (parsed.section.as_deref(), parsed.time) {
        (Some(name), Some(planned)) if !name.trim().is_empty() => Some(PageSectionMarker {
            name: name.trim().to_owned(),
            planned,
            line,
        }),
        (Some(_), Some(_)) => {
            return Err(BuildError::new(
                ErrorKind::Parse,
                Some(line),
                "section name must not be empty",
                r#"set "section" to the agenda label shown in presenter view"#,
            ));
        }
        (Some(_), None) => {
            return Err(BuildError::new(
                ErrorKind::Parse,
                Some(line),
                "section marker is missing time",
                r#"add "time":"1m" to this section marker"#,
            ));
        }
        (None, Some(_)) => {
            return Err(BuildError::new(
                ErrorKind::Parse,
                Some(line),
                "section time requires a section name",
                "put deck-wide time in YAML frontmatter instead",
            ));
        }
        (None, None) => None,
    };
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
        section,
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
            | Tag::FootnoteDefinition(_)
    )
}

fn unsupported_tag_name(tag: &Tag<'_>) -> &'static str {
    match tag {
        Tag::BlockQuote(_) => "blockquote",
        Tag::Table(_) | Tag::TableHead | Tag::TableRow | Tag::TableCell => "table",
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
    use crate::{
        domain::{AspectRatio, FragmentKind},
        error::ErrorKind,
        phase::KeySource,
    };
    use std::path::Path;

    fn parse_markdown(
        source: &str,
        highlighter: &crate::highlight::Highlighter,
    ) -> Result<Deck<Parsed>> {
        let frontmatter = parse_frontmatter(source)?;
        super::parse_markdown(source, frontmatter, highlighter)
    }

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

        assert_eq!(
            settings.planned_time().map(PlannedTime::as_millis),
            Some(900_000)
        );
    }

    #[test]
    fn parses_code_images_frontmatter_to_shlex_argv() {
        let frontmatter =
            parse_frontmatter("---\ncode_images: { mermaid: mmdc -i - -o - -e svg }\n---\n# Intro")
                .unwrap();
        let config = frontmatter.settings().code_images();
        let command = config.entries.get("mermaid").unwrap();

        assert_eq!(config.key_line, Some(2));
        assert_eq!(command.argv, ["mmdc", "-i", "-", "-o", "-", "-e", "svg"]);
    }

    #[test]
    fn parses_code_images_frontmatter_with_two_space_indent() {
        let frontmatter =
            parse_frontmatter("---\ncode_images:\n  mermaid: mmdc -i -\n---\n# Intro").unwrap();
        let command = frontmatter
            .settings()
            .code_images()
            .entries
            .get("mermaid")
            .unwrap();

        assert_eq!(command.argv, ["mmdc", "-i", "-"]);
    }

    #[test]
    fn parses_code_images_frontmatter_with_four_space_indent() {
        let frontmatter =
            parse_frontmatter("---\ncode_images:\n    mermaid: mmdc -i -\n---\n# Intro").unwrap();
        let command = frontmatter
            .settings()
            .code_images()
            .entries
            .get("mermaid")
            .unwrap();

        assert_eq!(command.argv, ["mmdc", "-i", "-"]);
    }

    #[test]
    fn parses_code_images_frontmatter_with_tab_indent() {
        let frontmatter =
            parse_frontmatter("---\ncode_images:\n\tmermaid: mmdc -i -\n---\n# Intro").unwrap();
        let command = frontmatter
            .settings()
            .code_images()
            .entries
            .get("mermaid")
            .unwrap();

        assert_eq!(command.argv, ["mmdc", "-i", "-"]);
    }

    #[test]
    fn rejects_inconsistent_code_images_indent_with_line_and_help() {
        let err = match parse_frontmatter(
            "---\ncode_images:\n  mermaid: mmdc -i -\n    dot: dot -Tsvg\n---\n# Intro",
        ) {
            Ok(_) => panic!("expected inconsistent code_images indentation to fail"),
            Err(err) => err,
        };

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(4));
        assert_eq!(
            err.message,
            "inconsistent indentation in code_images frontmatter"
        );
        assert_eq!(
            err.help,
            "use the same indentation for every code_images entry"
        );
    }

    #[test]
    fn rejects_empty_code_images_command_with_line_and_help() {
        let err = match parse_frontmatter("---\ncode_images: { mermaid: \"\" }\n---\n# Intro") {
            Ok(_) => panic!("expected empty code_images command to fail"),
            Err(err) => err,
        };

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(2));
        assert_eq!(err.message, "code_images entry 'mermaid' has empty command");
        assert_eq!(
            err.help,
            "set code_images.mermaid to a command, like mmdc -i - -o - -e svg"
        );
    }

    #[test]
    fn rejects_shlex_invalid_code_images_command_with_line_and_help() {
        let err = match parse_frontmatter(
            "---\ncode_images: { mermaid: \"mmdc 'unterminated\" }\n---\n# Intro",
        ) {
            Ok(_) => panic!("expected invalid code_images command to fail"),
            Err(err) => err,
        };

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(2));
        assert_eq!(
            err.message,
            "code_images entry 'mermaid' has invalid command string"
        );
        assert_eq!(
            err.help,
            "quote the command using shell-like syntax or remove the unmatched quote"
        );
    }

    #[test]
    fn rejects_three_level_code_images_nesting_with_line_and_help() {
        let err = match parse_frontmatter(
            "---\ncode_images:\n  mermaid:\n    command: mmdc\n---\n# Intro",
        ) {
            Ok(_) => panic!("expected three-level code_images nesting to fail"),
            Err(err) => err,
        };

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(4));
        assert_eq!(
            err.message,
            "unsupported nested mapping in code_images frontmatter"
        );
        assert_eq!(
            err.help,
            "use code_images entries as tag: command strings, like mermaid: mmdc -i - -o - -e svg"
        );
    }

    #[test]
    fn parses_frontmatter_aspect_ratio_16_9_as_default() {
        let deck = parse_markdown(
            "---\naspect_ratio: 16:9\n---\n# Intro",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();

        assert_eq!(deck.settings().aspect_ratio(), AspectRatio::Ratio16To9);
    }

    #[test]
    fn parses_frontmatter_aspect_ratio_4_3() {
        let deck = parse_markdown(
            "---\naspect_ratio: 4:3\n---\n# Intro",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();

        assert_eq!(deck.settings().aspect_ratio().width(), 960);
        assert_eq!(deck.settings().aspect_ratio().height(), 720);
    }

    #[test]
    fn missing_frontmatter_aspect_ratio_defaults_to_16_9() {
        let frontmatter = parse_frontmatter("# Intro").unwrap();

        assert_eq!(
            frontmatter.settings().aspect_ratio(),
            AspectRatio::Ratio16To9
        );
    }

    #[test]
    fn parses_frontmatter_resolution() {
        let deck = parse_markdown(
            "---\naspect_ratio: 16:9\nresolution: 1920x1080\n---\n# Intro",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();

        assert_eq!(deck.settings().resolution().width(), 1920);
        assert_eq!(deck.settings().resolution().height(), 1080);
    }

    #[test]
    fn missing_frontmatter_resolution_defaults_from_aspect_ratio() {
        let widescreen = parse_frontmatter("# Intro").unwrap();
        assert_eq!(widescreen.settings().resolution().width(), 1920);
        assert_eq!(widescreen.settings().resolution().height(), 1080);

        let classic = parse_frontmatter("---\naspect_ratio: 4:3\n---\n# Intro").unwrap();
        assert_eq!(classic.settings().resolution().width(), 1440);
        assert_eq!(classic.settings().resolution().height(), 1080);
    }

    #[test]
    fn rejects_frontmatter_resolution_that_mismatches_aspect_ratio() {
        let markdown = "---\naspect_ratio: 16:9\nresolution: 1024x768\n---\n# Intro";
        let err = parse_markdown(markdown, &crate::highlight::Highlighter::defaults()).unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(3));
        assert_eq!(
            err.message,
            "resolution 1024x768 does not match aspect_ratio 16:9"
        );
        assert_eq!(
            err.help,
            "set resolution to match aspect_ratio or remove resolution to use the default"
        );
    }

    #[test]
    fn rejects_frontmatter_resolution_smaller_than_canvas() {
        let markdown = "---\naspect_ratio: 16:9\nresolution: 16x9\n---\n# Intro";
        let err = parse_markdown(markdown, &crate::highlight::Highlighter::defaults()).unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(3));
        assert_eq!(
            err.message,
            "resolution 16x9 is smaller than the canvas logical size 1280x720; use at least the canvas dimensions"
        );
        assert_eq!(
            err.help,
            "set resolution to at least the canvas dimensions or remove resolution to use the default"
        );
    }

    #[test]
    fn accepts_frontmatter_resolution_equal_to_canvas() {
        let deck = parse_markdown(
            "---\naspect_ratio: 16:9\nresolution: 1280x720\n---\n# Intro",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();

        assert_eq!(deck.settings().resolution().width(), 1280);
        assert_eq!(deck.settings().resolution().height(), 720);
    }

    #[test]
    fn rejects_invalid_frontmatter_resolution_with_line_and_message() {
        let cases = [
            (
                "resolution:",
                "resolution has no value",
                "set resolution to WxH pixels, like 1920x1080",
            ),
            (
                "resolution: abc",
                "resolution must use WxH pixel format",
                "set resolution to WxH pixels, like 1920x1080",
            ),
            (
                "resolution: 1920",
                "resolution must use WxH pixel format",
                "set resolution to WxH pixels, like 1920x1080",
            ),
            (
                "resolution: abcxdef",
                "resolution width `abc` is not a valid u32",
                "set resolution to WxH pixels, like 1920x1080",
            ),
            (
                "resolution: 9999999999x1080",
                "resolution width `9999999999` is not a valid u32",
                "set resolution to WxH pixels, like 1920x1080",
            ),
            (
                "resolution: 1080x9999999999",
                "resolution height `9999999999` is not a valid u32",
                "set resolution to WxH pixels, like 1920x1080",
            ),
            (
                "resolution: 0x1080",
                "resolution width must be greater than zero",
                "set resolution to WxH pixels, like 1920x1080",
            ),
            (
                "resolution: 1080x0",
                "resolution height must be greater than zero",
                "set resolution to WxH pixels, like 1920x1080",
            ),
        ];

        for (frontmatter_line, message, help) in cases {
            let markdown = format!("---\ntime: 15m\n{frontmatter_line}\n---\n# Intro");
            let err =
                parse_markdown(&markdown, &crate::highlight::Highlighter::defaults()).unwrap_err();

            assert_eq!(err.kind, ErrorKind::Parse);
            assert_eq!(err.line, Some(3), "case: {frontmatter_line}");
            assert_eq!(err.message, message, "case: {frontmatter_line}");
            assert_eq!(err.help, help, "case: {frontmatter_line}");
        }
    }

    #[test]
    fn rejects_unknown_frontmatter_aspect_ratio_with_line_and_message() {
        for value in ["16:10", "1920x1080"] {
            let markdown = format!("---\ntime: 15m\naspect_ratio: {value}\n---\n# Intro");
            let err =
                parse_markdown(&markdown, &crate::highlight::Highlighter::defaults()).unwrap_err();

            assert_eq!(err.kind, ErrorKind::Parse);
            assert_eq!(err.line, Some(3));
            assert_eq!(
                err.message,
                format!("unknown aspect_ratio '{value}'; use one of: 16:9, 4:3")
            );
        }
    }

    #[test]
    fn rejects_empty_frontmatter_aspect_ratio_with_line_and_message() {
        let markdown = "---\naspect_ratio:\n---\n# Intro";
        let err = parse_markdown(markdown, &crate::highlight::Highlighter::defaults()).unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(2));
        assert_eq!(err.message, "aspect_ratio has no value");
        assert_eq!(err.help, "set aspect_ratio to 16:9 or 4:3");
    }

    #[test]
    fn parse_frontmatter_returns_default_settings_when_no_frontmatter() {
        let frontmatter = parse_frontmatter("# Intro").unwrap();

        assert!(frontmatter.settings().planned_time().is_none());
        assert!(!frontmatter.settings().breaks());
        assert!(frontmatter.settings().layouts().is_none());
        assert!(frontmatter.settings().css().is_none());
        assert!(frontmatter.settings().syntaxes().is_none());
        assert!(frontmatter.settings().fonts().is_none());
        assert_eq!(frontmatter.body_start, 0);
    }

    #[test]
    fn breaks_frontmatter_true_flips_setting() {
        let frontmatter = parse_frontmatter("---\nbreaks: true\n---\n# Intro").unwrap();

        assert!(frontmatter.settings().breaks());
    }

    #[test]
    fn breaks_frontmatter_false_or_missing_defaults_to_false() {
        let explicit_false = parse_frontmatter("---\nbreaks: false\n---\n# Intro").unwrap();
        let missing = parse_frontmatter("# Intro").unwrap();

        assert!(!explicit_false.settings().breaks());
        assert!(!missing.settings().breaks());
    }

    #[test]
    fn breaks_frontmatter_invalid_value_is_line_numbered_error() {
        let err = parse_markdown(
            "---\nbreaks: not-a-bool\n---\n# Intro",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(2));
        assert!(err.to_string().contains("invalid deck frontmatter"));
        assert_eq!(err.help, "set breaks to true or false");
    }

    #[test]
    fn parse_frontmatter_rejects_empty_pair_as_invalid_leading_frontmatter() {
        let err = match parse_frontmatter("---\n---\n# Intro") {
            Ok(_) => panic!("expected empty frontmatter pair to be rejected"),
            Err(err) => err,
        };

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

    #[test]
    fn parse_frontmatter_records_key_line_for_layouts() {
        let frontmatter =
            parse_frontmatter("---\ntime: 15m\nlayouts: ./layouts\n---\n# Intro").unwrap();

        assert_eq!(frontmatter.key_line("layouts"), Some(3));
    }

    #[test]
    fn parse_frontmatter_records_key_line_for_css() {
        let frontmatter = parse_frontmatter("---\ntime: 15m\ncss: ./css\n---\n# Intro").unwrap();

        assert_eq!(frontmatter.key_line("css"), Some(3));
    }

    #[test]
    fn parse_frontmatter_records_key_line_for_syntaxes() {
        let frontmatter =
            parse_frontmatter("---\ntime: 15m\nsyntaxes: ./syntaxes\n---\n# Intro").unwrap();

        assert_eq!(frontmatter.key_line("syntaxes"), Some(3));
    }

    #[test]
    fn parse_frontmatter_records_key_line_for_fonts() {
        let frontmatter =
            parse_frontmatter("---\ntime: 15m\nfonts: ./fonts\n---\n# Intro").unwrap();

        assert_eq!(frontmatter.key_line("fonts"), Some(3));
    }

    #[test]
    fn parse_frontmatter_records_key_line_for_resolution() {
        let frontmatter =
            parse_frontmatter("---\ntime: 15m\nresolution: 1920x1080\n---\n# Intro").unwrap();

        assert_eq!(frontmatter.key_line("resolution"), Some(3));
    }

    #[test]
    fn parse_frontmatter_records_key_line_for_breaks() {
        let frontmatter = parse_frontmatter("---\ntime: 15m\nbreaks: true\n---\n# Intro").unwrap();

        assert_eq!(frontmatter.key_line("breaks"), Some(3));
    }

    #[test]
    fn parse_frontmatter_records_key_line_for_time() {
        let frontmatter =
            parse_frontmatter("---\ntime: 15m\nlayouts: ./layouts\n---\n# Intro").unwrap();

        assert_eq!(frontmatter.key_line("time"), Some(2));
    }

    #[test]
    fn parse_frontmatter_key_line_returns_none_for_absent_key() {
        let frontmatter = parse_frontmatter("---\nlayouts: ./layouts\n---\n# Intro").unwrap();

        assert_eq!(frontmatter.key_line("css"), None);
    }

    #[test]
    fn parse_markdown_slices_body_from_parsed_frontmatter_offset() {
        let markdown = "---\ntime: 15m\nlayouts: ./layouts\ncss: ./css\n---\n# Intro";
        let frontmatter = parse_frontmatter(markdown).unwrap();
        let deck = super::parse_markdown(
            markdown,
            frontmatter,
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();
        let slide = &deck.parsed_slides()[0];

        assert_eq!(slide.fragments.len(), 1);
        assert_eq!(slide.fragments[0].markdown(), "# Intro");
        assert_eq!(
            slide.fragments[0].kind(),
            &FragmentKind::Heading { level: 1 }
        );
        assert_eq!(slide.fragments[0].line(), 6);
    }

    #[test]
    fn parses_frontmatter_layouts_key_carries_to_settings() {
        let deck = parse_markdown(
            "---\nlayouts: ./layouts\n---\n# Intro",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();

        assert_eq!(
            deck.settings().layouts().map(|path| path.as_path()),
            Some(Path::new("./layouts"))
        );
    }

    #[test]
    fn parses_frontmatter_css_key_carries_to_settings() {
        let deck = parse_markdown(
            "---\ncss: ./css\n---\n# Intro",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();

        assert_eq!(
            deck.settings().css().map(|path| path.as_path()),
            Some(Path::new("./css"))
        );
    }

    #[test]
    fn parses_frontmatter_syntaxes_key_carries_to_settings() {
        let deck = parse_markdown(
            "---\nsyntaxes: ./syntaxes\n---\n# Intro",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();

        assert_eq!(
            deck.settings().syntaxes().map(|path| path.as_path()),
            Some(Path::new("./syntaxes"))
        );
    }

    #[test]
    fn parses_frontmatter_fonts_key_carries_to_settings() {
        let deck = parse_markdown(
            "---\nfonts: ./fonts\n---\n# Intro",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();

        assert_eq!(
            deck.settings().fonts().map(|path| path.as_path()),
            Some(Path::new("./fonts"))
        );
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

        let deck = parse_markdown(markdown, &crate::highlight::Highlighter::defaults()).unwrap();
        let slide = &deck.parsed_slides()[0];

        assert_eq!(slide.key.as_str(), "arch-1");
        assert_eq!(
            slide.fragments[0].kind(),
            &FragmentKind::Heading { level: 1 }
        );
        assert_eq!(slide.fragments[0].line(), 2);
        assert_eq!(slide.fragments[0].markdown(), "# **Architecture** `Phase`");
        assert_eq!(slide.fragments[1].kind(), &FragmentKind::Paragraph);
        assert_eq!(
            slide.fragments[1].markdown(),
            "Markdown is the **source** of [truth](https://example.com)."
        );
        assert_eq!(slide.fragments[2].kind(), &FragmentKind::List);
        assert_eq!(slide.fragments[2].line(), 6);
        assert!(slide.fragments[2]
            .markdown()
            .contains("- Markdown = source of truth"));
        assert_eq!(slide.fragments[3].kind(), &FragmentKind::Code);
        assert_eq!(slide.fragments[3].line(), 9);
    }

    #[test]
    fn parses_standalone_image_paragraph_as_image_fragment() {
        let deck = parse_markdown(
            "# Title\n\n![Architecture diagram](images/arch.png)",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();
        let slide = &deck.parsed_slides()[0];

        assert_eq!(slide.fragments.len(), 2);
        assert_eq!(slide.fragments[1].line(), 3);
        match slide.fragments[1].kind() {
            FragmentKind::Image { alt, src } => {
                assert_eq!(alt, "Architecture diagram");
                assert_eq!(src.as_str(), "images/arch.png");
            }
            other => panic!("expected image fragment, got {other:?}"),
        }
    }

    #[test]
    fn parses_empty_alt_image_as_image_fragment() {
        let deck = parse_markdown(
            "# Title\n\n![](images/arch.png)",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();
        let slide = &deck.parsed_slides()[0];

        match slide.fragments[1].kind() {
            FragmentKind::Image { alt, src } => {
                assert_eq!(alt, "");
                assert_eq!(src.as_str(), "images/arch.png");
            }
            other => panic!("expected image fragment, got {other:?}"),
        }
    }

    #[test]
    fn parses_current_dir_and_nested_image_paths() {
        let deck = parse_markdown(
            "# Title\n\n![Nested](./img/deep/nested/path.png)",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();
        let slide = &deck.parsed_slides()[0];

        match slide.fragments[1].kind() {
            FragmentKind::Image { src, .. } => {
                assert_eq!(src.as_str(), "./img/deep/nested/path.png");
            }
            other => panic!("expected image fragment, got {other:?}"),
        }
    }

    #[test]
    fn parses_supported_image_extension_case_insensitively() {
        let deck = parse_markdown(
            "# Title\n\n![Architecture diagram](images/Arch.PNG)",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();
        let slide = &deck.parsed_slides()[0];

        match slide.fragments[1].kind() {
            FragmentKind::Image { src, .. } => assert_eq!(src.as_str(), "images/Arch.PNG"),
            other => panic!("expected image fragment, got {other:?}"),
        }
    }

    #[test]
    fn rejects_remote_image_url_with_line() {
        let err = parse_markdown(
            "# Title\n\n![Remote](https://example.com/x.png)",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(3));
        assert!(err
            .to_string()
            .contains("remote image URLs are not supported"));
    }

    #[test]
    fn rejects_absolute_image_path_with_line() {
        let err = parse_markdown(
            "# Title\n\n![Absolute](/abs/x.png)",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(3));
        assert!(err
            .to_string()
            .contains("absolute image paths are not supported"));
    }

    #[test]
    fn rejects_parent_directory_escape_in_image_path() {
        let err = parse_markdown(
            "# Title\n\n![Escape](../foo.png)",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(3));
        assert!(err
            .to_string()
            .contains("image path escapes deck directory"));
    }

    #[test]
    fn rejects_image_path_query_string() {
        let err = parse_markdown(
            "# Title\n\n![Query](img/x.png?v=1)",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(3));
        assert!(err
            .to_string()
            .contains("image path query strings are not supported"));
    }

    #[test]
    fn rejects_image_path_fragment() {
        let err = parse_markdown(
            "# Title\n\n![Fragment](img/x.png#frag)",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(3));
        assert!(err
            .to_string()
            .contains("image path fragments are not supported"));
    }

    #[test]
    fn rejects_backslash_image_path() {
        let err = parse_markdown(
            r"# Title

![Backslash](img\arch.png)",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(3));
        assert!(err
            .to_string()
            .contains("image paths must use forward slashes"));
    }

    #[test]
    fn rejects_empty_image_path() {
        let err = parse_markdown(
            "# Title\n\n![]()",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(3));
        assert!(err.to_string().contains("empty image path"));
    }

    #[test]
    fn rejects_image_without_supported_extension() {
        let err = parse_markdown(
            "# Title\n\n![Binary](foo.exe)",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(3));
        assert!(err
            .to_string()
            .contains("unsupported image extension 'exe'; supported: png, jpg, jpeg, gif, webp"));
    }

    #[test]
    fn rejects_image_without_supported_extension_preserves_extension_case() {
        let err = parse_markdown(
            "# Title\n\n![Binary](foo.EXE)",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(3));
        assert!(err
            .to_string()
            .contains("unsupported image extension 'EXE'; supported: png, jpg, jpeg, gif, webp"));
    }

    #[test]
    fn rejects_svg_until_policy_is_decided() {
        let err = parse_markdown(
            "# Title\n\n![Icon](icon.svg)",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(3));
        assert!(err
            .to_string()
            .contains("unsupported image extension 'svg'; supported: png, jpg, jpeg, gif, webp"));
    }

    #[test]
    fn rejects_text_and_image_mixed_in_one_paragraph() {
        let err = parse_markdown(
            "# Title\n\nprefix ![Architecture](x.png)",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(3));
        assert!(err
            .to_string()
            .contains("unsupported construct 'mixed image paragraph'"));

        let err = parse_markdown(
            "# Title\n\n![Architecture](x.png) suffix",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(3));
        assert!(err
            .to_string()
            .contains("unsupported construct 'mixed image paragraph'"));
    }

    #[test]
    fn rejects_two_images_in_one_paragraph_until_inline_design_exists() {
        let err = parse_markdown(
            "# Title\n\n![A](x.png)![B](y.png)",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(3));
        assert!(err
            .to_string()
            .contains("unsupported construct 'mixed image paragraph'"));
    }

    #[test]
    fn rejects_image_inside_list_before_markdown_rerender() {
        let err = parse_markdown(
            "# Title\n\n- ![Architecture](x.png)",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(3));
        assert!(err
            .to_string()
            .contains("unsupported construct 'image inside list'"));
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

        let deck = parse_markdown(markdown, &crate::highlight::Highlighter::defaults()).unwrap();
        let slide = &deck.parsed_slides()[0];

        assert_eq!(slide.fragments[1].kind(), &FragmentKind::List);
        assert!(slide.fragments[1].markdown().contains("- loose a"));
        assert!(slide.fragments[1].markdown().contains("  - nested"));
        assert!(slide.fragments[1].markdown().contains("fn inside_item()"));
        assert_eq!(slide.fragments[2].kind(), &FragmentKind::Paragraph);
        assert_eq!(slide.fragments[2].markdown(), "After list");
    }

    #[test]
    fn rejects_unsupported_construct_with_line_and_help() {
        let err = parse_markdown(
            "# Title\n\n> quoted",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

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
        let err = parse_markdown(
            "# Title\n\n- item <b>raw</b>",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(3));
        assert!(err.to_string().contains("unsupported construct 'html'"));
    }

    #[test]
    fn collects_speaker_note_from_html_comment() {
        let deck = parse_markdown(
            "# Title\n\n<!-- speaker note body -->",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();

        let slide = &deck.parsed_slides()[0];
        assert_eq!(slide.key.as_str(), "title");
        assert_eq!(slide.fragments.len(), 1);
        assert_eq!(slide.notes.as_deref(), Some("speaker note body"));
    }

    #[test]
    fn joins_multiple_html_comments_with_blank_line() {
        let deck = parse_markdown(
            "# Title\n\n<!-- first note -->\n\nbody\n\n<!-- second note -->",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();

        let slide = &deck.parsed_slides()[0];
        assert_eq!(slide.notes.as_deref(), Some("first note\n\nsecond note"));
    }

    #[test]
    fn empty_html_comment_is_ignored_as_note() {
        let deck = parse_markdown(
            "# Title\n\n<!-- -->\n\n<!--\n\n-->",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();

        let slide = &deck.parsed_slides()[0];
        assert!(slide.notes.is_none());
    }

    #[test]
    fn html_comment_before_content_is_still_a_note() {
        let deck = parse_markdown(
            "<!-- pre-title note -->\n# Title",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();

        let slide = &deck.parsed_slides()[0];
        assert_eq!(slide.key.as_str(), "title");
        assert_eq!(slide.notes.as_deref(), Some("pre-title note"));
    }

    #[test]
    fn page_settings_comment_and_note_coexist() {
        let deck = parse_markdown(
            "<!-- {\"key\":\"cover\"} -->\n# Title\n\n<!-- this is a note -->",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();

        let slide = &deck.parsed_slides()[0];
        assert_eq!(slide.key.as_str(), "cover");
        assert_eq!(slide.notes.as_deref(), Some("this is a note"));
    }

    #[test]
    fn multiline_html_comment_preserves_internal_newlines() {
        let deck = parse_markdown(
            "# Title\n\n<!--\nline one\nline two\n-->",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();

        let slide = &deck.parsed_slides()[0];
        assert_eq!(slide.notes.as_deref(), Some("line one\nline two"));
    }

    #[test]
    fn parses_layout_request_from_page_settings_comment() {
        let deck = parse_markdown(
            "<!-- {\"key\":\"cover\",\"layout\":\"cover\"} -->\n# Title",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();

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
        let deck = parse_markdown(
            "<!-- {\"layout\":\"cover\"} -->\n# My Title",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();

        let slide = &deck.parsed_slides()[0];
        assert_eq!(slide.key.as_str(), "my-title");
        assert_eq!(slide.layout_request.as_ref().unwrap().name, "cover");
    }

    #[test]
    fn parses_section_page_comment_with_time_key_and_layout() {
        let settings = parse_page_comment(
            r#"<!-- {"key":"cover","layout":"cover","section":"Setup","time":"1m"} -->"#,
            7,
        )
        .unwrap()
        .unwrap();

        let section = settings.section.unwrap();
        assert_eq!(section.name, "Setup");
        assert_eq!(section.planned.as_millis(), 60_000);
        assert_eq!(section.line, 7);
        assert_eq!(settings.layout.as_deref(), Some("cover"));
        assert_eq!(settings.key.unwrap().as_str(), "cover");
    }

    #[test]
    fn rejects_invalid_section_page_comments_with_line_and_help() {
        let cases = [
            (
                r#"<!-- {"section":"Setup"} -->"#,
                "section marker is missing time",
                r#"add "time":"1m" to this section marker"#,
            ),
            (
                r#"<!-- {"time":"1m"} -->"#,
                "section time requires a section name",
                "put deck-wide time in YAML frontmatter instead",
            ),
            (
                r#"<!-- {"section":"","time":"1m"} -->"#,
                "section name must not be empty",
                r#"set "section" to the agenda label shown in presenter view"#,
            ),
        ];

        for (raw, message, help) in cases {
            let err = parse_page_comment(raw, 3).unwrap_err();
            assert_eq!(err.kind, ErrorKind::Parse);
            assert_eq!(err.line, Some(3));
            assert!(err.to_string().contains(message));
            assert_eq!(err.help, help);
        }
    }

    #[test]
    fn rejects_second_page_settings_comment_that_would_override_section() {
        let err = parse_markdown(
            "<!-- {\"section\":\"Setup\",\"time\":\"1m\"} -->\n\
             <!-- {\"section\":\"Demo\",\"time\":\"1m\"} -->\n\
             # Title",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(2));
        assert!(err.to_string().contains("duplicate page settings comment"));
        assert_eq!(
            err.help,
            "merge the settings into the first page settings comment"
        );
    }

    #[test]
    fn rejects_second_page_settings_comment_that_only_sets_key() {
        let err = parse_markdown(
            "<!-- {\"layout\":\"cover\"} -->\n\
             <!-- {\"key\":\"cover\"} -->\n\
             # Title",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(2));
        assert!(err.to_string().contains("duplicate page settings comment"));
        assert_eq!(
            err.help,
            "merge the settings into the first page settings comment"
        );
    }

    #[test]
    fn rejects_second_page_settings_comment_after_content_as_position_error() {
        let err = parse_markdown(
            "<!-- {\"layout\":\"cover\"} -->\n\
             # Title\n\
             <!-- {\"key\":\"late\"} -->",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(3));
        assert!(err
            .to_string()
            .contains("page settings comment must appear before slide content"));
        assert_eq!(
            err.help,
            "move the settings comment to the first non-blank line of the slide"
        );
    }

    #[test]
    fn rejects_unknown_page_settings_fields() {
        let err = parse_markdown(
            "<!-- {\"key\":\"a\",\"freeze\":true} -->\n# Title",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(1));
        assert!(err.to_string().contains("invalid page settings comment"));
        assert_eq!(
            err.help,
            r#"use <!-- {"key":"arch-1","layout":"cover","section":"Setup","time":"1m"} --> (key/layout optional; section and time must appear together; no other fields)"#
        );
    }

    #[test]
    fn rejects_empty_page_settings_comment() {
        let err = parse_markdown(
            "<!-- {} -->\n# Title",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert!(err
            .to_string()
            .contains("page settings comment has no settings"));
    }

    #[test]
    fn rejects_unknown_code_language_tag_with_line() {
        let err = parse_markdown(
            "# Title\n\n```notalang\nx\n```",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(3));
        assert!(err.to_string().contains("unknown code language 'notalang'"));
    }

    #[test]
    fn accepts_code_images_language_tag_unknown_to_highlighter() {
        let markdown = "---\ncode_images:\n  mermaid: mmdc -i - -o - -e svg\n---\n# Intro\n\n```mermaid\ngraph TD\n```";

        let deck = parse_markdown(markdown, &crate::highlight::Highlighter::defaults()).unwrap();
        let fragment = &deck.parsed_slides()[0].fragments[1];

        assert_eq!(fragment.kind(), &FragmentKind::Code);
        assert_eq!(fragment.language(), Some("mermaid"));
    }

    #[test]
    fn untagged_code_block_needs_no_language() {
        let deck = parse_markdown(
            "# Title\n\n```\nplain\n```",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();

        assert_eq!(deck.parsed_slides()[0].fragments[1].language(), None);
    }

    #[test]
    fn rejects_broken_json_key_comments() {
        let err = parse_markdown(
            "<!-- {\"kye\":\"arch-1\"} -->\n# Title",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(1));
        assert!(err.to_string().contains("invalid page settings comment"));
    }

    #[test]
    fn rejects_explicit_keys_with_invalid_characters_and_line() {
        let err = parse_markdown(
            "<!-- {\"key\":\"bad key]\"} -->\n# Title",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

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
        let deck = parse_markdown(
            "# Architecture Overview\n\nBody",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();
        assert_eq!(
            deck.parsed_slides()[0].key.as_str(),
            "architecture-overview"
        );
    }

    #[test]
    fn explicit_key_wins_over_derived_key() {
        let deck = parse_markdown(
            "<!-- {\"key\":\"arch-1\"} -->\n# Renamed Title",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();
        assert_eq!(deck.parsed_slides()[0].key.as_str(), "arch-1");
    }

    #[test]
    fn parsed_slide_records_explicit_and_derived_key_sources() {
        let deck = parse_markdown(
            "<!-- {\"key\":\"arch-1\"} -->\n# Architecture\n\n---\n# Derived Title",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();
        let slides = deck.parsed_slides();

        assert_eq!(slides[0].key.as_str(), "arch-1");
        assert_eq!(slides[0].key_source, KeySource::Explicit { line: 1 });
        assert_eq!(slides[1].key.as_str(), "derived-title");
        assert_eq!(slides[1].key_source, KeySource::Derived { line: Some(5) });
    }

    #[test]
    fn derives_slide_number_key_when_slide_has_no_heading() {
        let deck = parse_markdown(
            "Body only\n\n---\nSecond body",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();

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
        let deck = parse_markdown(
            "# One\n\n---\n\n# Two\n\nBody",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();
        let slides = deck.parsed_slides();

        assert_eq!(slides.len(), 2);
        assert_eq!(slides[0].index, 0);
        assert_eq!(slides[0].key.as_str(), "one");
        assert_eq!(slides[1].index, 1);
        assert_eq!(slides[1].key.as_str(), "two");
        assert_eq!(slides[1].fragments[0].line(), 5);
    }

    #[test]
    fn resolves_section_ranges_from_marker_positions() {
        let deck = parse_markdown(
            "---\ntime: 3m\n---\n\
             <!-- {\"section\":\"Setup\",\"time\":\"1m\"} -->\n# A\n\n---\n# B\n\n---\n\
             <!-- {\"section\":\"Demo\",\"time\":\"2m\"} -->\n# C",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();

        let sections = deck.settings().sections();
        assert_eq!(sections.len(), 2);
        assert_eq!(
            (sections[0].name(), sections[0].start(), sections[0].end()),
            ("Setup", 0, 1)
        );
        assert_eq!(
            (sections[1].name(), sections[1].start(), sections[1].end()),
            ("Demo", 2, 2)
        );
    }

    #[test]
    fn allows_duplicate_section_names_as_separate_ranges() {
        let deck = parse_markdown(
            "<!-- {\"section\":\"Repeat\",\"time\":\"1m\"} -->\n# A\n\n---\n# B\n\n---\n\
             <!-- {\"section\":\"Repeat\",\"time\":\"2m\"} -->\n# C\n\n---\n# D",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();

        let sections = deck.settings().sections();
        assert_eq!(sections.len(), 2);
        assert_eq!(
            (
                sections[0].name(),
                sections[0].start(),
                sections[0].end(),
                sections[0].planned().as_millis()
            ),
            ("Repeat", 0, 1, 60_000)
        );
        assert_eq!(
            (
                sections[1].name(),
                sections[1].start(),
                sections[1].end(),
                sections[1].planned().as_millis()
            ),
            ("Repeat", 2, 3, 120_000)
        );
    }

    #[test]
    fn single_section_marker_covers_the_whole_deck_and_derives_planned_time() {
        let deck = parse_markdown(
            "<!-- {\"section\":\"Full\",\"time\":\"2m\"} -->\n# A\n\n---\n# B\n\n---\n# C",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();

        assert_eq!(deck.settings().planned_time().unwrap().as_millis(), 120_000);
        let sections = deck.settings().sections();
        assert_eq!(sections.len(), 1);
        assert_eq!(
            (
                sections[0].name(),
                sections[0].start(),
                sections[0].end(),
                sections[0].planned().as_millis()
            ),
            ("Full", 0, 2, 120_000)
        );
    }

    #[test]
    fn decks_without_section_markers_keep_settings_unchanged() {
        let deck = parse_markdown(
            "# A\n\n---\n# B",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();

        assert_eq!(deck.settings().planned_time(), None);
        assert!(deck.settings().sections().is_empty());
    }

    #[test]
    fn rejects_section_markers_when_first_slide_has_no_marker() {
        let err = parse_markdown(
            "# A\n\n---\n<!-- {\"section\":\"Late\",\"time\":\"1m\"} -->\n# B",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(4));
        assert!(err
            .to_string()
            .contains("first slide must declare a section"));
        assert_eq!(
            err.help,
            "add a section marker to the first slide or remove all section markers"
        );
    }

    #[test]
    fn derives_deck_time_from_section_sum_when_frontmatter_time_is_absent() {
        let deck = parse_markdown(
            "<!-- {\"section\":\"Setup\",\"time\":\"1m\"} -->\n# A\n\n---\n\
             <!-- {\"section\":\"Demo\",\"time\":\"2m\"} -->\n# B",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();

        assert_eq!(
            deck.settings().planned_time().map(PlannedTime::as_millis),
            Some(180_000)
        );
    }

    #[test]
    fn rejects_frontmatter_time_that_differs_from_section_total() {
        let err = parse_markdown(
            "---\ntime: 5m\n---\n\
             <!-- {\"section\":\"Setup\",\"time\":\"1m\"} -->\n# A\n\n---\n\
             <!-- {\"section\":\"Demo\",\"time\":\"2m\"} -->\n# B",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(4));
        assert!(err
            .to_string()
            .contains("frontmatter time 300000ms does not match section total 180000ms"));
        assert_eq!(
            err.help,
            "adjust frontmatter time or section times so the totals match"
        );
    }

    #[test]
    fn rejects_section_time_total_above_manifest_safe_integer_limit() {
        let err = parse_markdown(
            "<!-- {\"section\":\"A\",\"time\":\"9007199254740s\"} -->\n# A\n\n---\n\
             <!-- {\"section\":\"B\",\"time\":\"9007199254740s\"} -->\n# B",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(5));
        assert!(err.to_string().contains("section time total is too large"));
        assert_eq!(
            err.help,
            "reduce section times so the total is at most Number.MAX_SAFE_INTEGER milliseconds"
        );
    }

    #[test]
    fn checked_add_overflow_in_section_sum_reports_line_and_help() {
        let err = section_total_from_millis([
            (2, PlannedTime::MAX_SAFE_JAVASCRIPT_INTEGER_MILLIS),
            (5, u64::MAX),
        ])
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(5));
        assert!(err.to_string().contains("section time total overflowed"));
        assert_eq!(
            err.help,
            "reduce section times so the total can be represented safely"
        );
    }

    #[test]
    fn frontmatter_is_not_a_slide_and_later_rules_still_split_slides() {
        let markdown = "---\ntime: 15m\n---\n\n# Intro\n\n---\n# Details";
        let frontmatter = parse_frontmatter(markdown).unwrap();
        let ranges = split_slide_ranges(markdown, frontmatter.body_start).unwrap();

        assert_eq!(
            frontmatter
                .settings()
                .planned_time()
                .map(PlannedTime::as_millis),
            Some(900_000)
        );
        assert_eq!(ranges.len(), 2);

        let deck = parse_markdown(markdown, &crate::highlight::Highlighter::defaults()).unwrap();
        assert_eq!(deck.parsed_slides().len(), 2);
        assert_eq!(deck.parsed_slides()[0].fragments[0].line(), 5);
        assert_eq!(deck.parsed_slides()[1].fragments[0].line(), 8);
    }

    #[test]
    fn breaks_does_not_affect_slide_splitting() {
        for breaks in [true, false] {
            let markdown = format!("---\nbreaks: {breaks}\n---\n\nfirst\nsecond\n\n---\n\n# Next");
            let deck =
                parse_markdown(&markdown, &crate::highlight::Highlighter::defaults()).unwrap();

            assert_eq!(deck.parsed_slides().len(), 2, "breaks: {breaks}");
        }
    }

    #[test]
    fn rejects_leading_dense_rule_pair_as_invalid_frontmatter_not_dropped_slide() {
        let err = parse_markdown(
            "---\n# Title\n---\n# Next",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

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
        let deck = parse_markdown(
            "\n---\ntime: 15m\n---\n\n# A",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();
        let slides = deck.parsed_slides();

        assert_eq!(slides.len(), 1);
        assert_eq!(slides[0].key.as_str(), "a");
        assert_eq!(slides[0].fragments[0].line(), 6);
    }

    #[test]
    fn nonblank_prefix_before_rule_pair_keeps_existing_markdown_semantics() {
        let deck = parse_markdown(
            "# Z\n\n---\ntitle: x\n---\n\n# A",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();
        let slides = deck.parsed_slides();

        assert_eq!(slides.len(), 2);
        assert_eq!(slides[0].key.as_str(), "z");
        assert_eq!(slides[1].key.as_str(), "title-x");
        assert_eq!(slides[1].fragments[1].line(), 7);
        assert_eq!(slides[1].fragments[1].markdown(), "# A");
    }

    #[test]
    fn unknown_key_help_mentions_breaks() {
        let err = parse_markdown(
            "---\ntime: 15m\nunknown: true\n---\n# Intro",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(3));
        assert!(err.to_string().contains("invalid deck frontmatter"));
        assert_eq!(
            err.help,
            "use only the supported deck frontmatter keys: time, aspect_ratio, resolution, breaks, layouts, css, syntaxes, fonts, code_images"
        );
    }

    #[test]
    fn rejects_empty_layouts_string_with_line_and_help() {
        let err = parse_markdown(
            "---\nlayouts: \"\"\n---\n# Intro",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(2));
        assert!(err.to_string().contains("layouts: value is empty"), "{err}");
        assert_eq!(
            err.help,
            "provide a path (relative to the deck file), or remove the layouts: key"
        );
    }

    #[test]
    fn rejects_empty_css_string_with_line_and_help() {
        let err = parse_markdown(
            "---\ncss: \"\"\n---\n# Intro",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(2));
        assert!(err.to_string().contains("css: value is empty"), "{err}");
        assert_eq!(
            err.help,
            "provide a path (relative to the deck file), or remove the css: key"
        );
    }

    #[test]
    fn rejects_empty_syntaxes_string_with_line_and_help() {
        let err = parse_markdown(
            "---\nsyntaxes: \"\"\n---\n# Intro",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(2));
        assert!(
            err.to_string().contains("syntaxes: value is empty"),
            "{err}"
        );
        assert_eq!(
            err.help,
            "provide a path (relative to the deck file), or remove the syntaxes: key"
        );
    }

    #[test]
    fn rejects_empty_fonts_string_with_line_and_help() {
        let err = parse_markdown(
            "---\nfonts: \"\"\n---\n# Intro",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(2));
        assert!(err.to_string().contains("fonts: value is empty"), "{err}");
        assert_eq!(
            err.help,
            "provide a path (relative to the deck file), or remove the fonts: key"
        );
    }

    #[test]
    fn rejects_invalid_frontmatter_time_with_line_and_help() {
        for value in ["0", "0s", "-1", "abc", ""] {
            let markdown = format!("---\ntime: {value}\n---\n# Intro");
            let err =
                parse_markdown(&markdown, &crate::highlight::Highlighter::defaults()).unwrap_err();
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
    fn frontmatter_help_routes_invalid_values_by_key() {
        let cases = [
            (
                "---\nlayouts: \"\"\n---\n# Intro",
                "provide a path (relative to the deck file), or remove the layouts: key",
            ),
            (
                "---\ncss: \"\"\n---\n# Intro",
                "provide a path (relative to the deck file), or remove the css: key",
            ),
            (
                "---\nsyntaxes: \"\"\n---\n# Intro",
                "provide a path (relative to the deck file), or remove the syntaxes: key",
            ),
            (
                "---\nfonts: \"\"\n---\n# Intro",
                "provide a path (relative to the deck file), or remove the fonts: key",
            ),
            (
                "---\nbreaks: not-a-bool\n---\n# Intro",
                "set breaks to true or false",
            ),
            (
                "---\ntime: bad-value\n---\n# Intro",
                "set time to 15m, 90s, 1h, 1h30m, or an integer minute count",
            ),
        ];

        for (markdown, help) in cases {
            let err = parse_markdown(markdown, &crate::highlight::Highlighter::defaults())
                .expect_err("invalid frontmatter value should fail");

            assert_eq!(err.help, help);
        }
    }

    #[test]
    fn frontmatter_help_boolean_error_without_breaks_key_falls_through() {
        assert_eq!(
            frontmatter_help("invalid deck frontmatter: expected a boolean"),
            "write valid YAML frontmatter before the first slide"
        );
    }

    #[test]
    fn rejects_overflowing_planned_time_with_specific_message() {
        for yaml in [
            "time: 18446744073709551615\n",
            "time: 18446744073709551615m\n",
        ] {
            let err = serde_norway::from_str::<DeckFrontmatter>(yaml).unwrap_err();

            assert!(err.to_string().contains(PlannedTime::TOO_LARGE_MESSAGE));
        }

        let err = serde_norway::from_str::<DeckFrontmatter>("time: 0\n").unwrap_err();
        assert!(err
            .to_string()
            .contains(PlannedTime::GREATER_THAN_ZERO_MESSAGE));
    }

    #[test]
    fn rejects_planned_time_above_javascript_safe_integer_with_line_and_help() {
        let err = parse_markdown(
            "---\ntime: 9007199254740993s\n---\n# Intro",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(2));
        assert!(err.to_string().contains("invalid deck frontmatter"));
        assert!(err.to_string().contains(PlannedTime::TOO_LARGE_MESSAGE));
        assert_eq!(
            err.help,
            "set time to 15m, 90s, 1h, 1h30m, or an integer minute count"
        );
    }

    #[test]
    fn huge_integer_time_uses_time_format_help() {
        let err = parse_markdown(
            "---\ntime: 999999999999999999999\n---\n# Intro",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

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
        let err = parse_markdown(
            "---\ntime: 15m\ntime: 20m\n---\n# Intro",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(2));
        assert!(err.to_string().contains("duplicate entry"));
        assert_eq!(
            err.help,
            "use only the supported deck frontmatter keys: time, aspect_ratio, resolution, breaks, layouts, css, syntaxes, fonts, code_images"
        );
    }

    #[test]
    fn rejects_broken_frontmatter_yaml_with_line_and_help() {
        let err = parse_markdown(
            "---\ntime: [\n---\n# Intro",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

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
            &crate::highlight::Highlighter::defaults(),
            &crate::domain::CodeImagesConfig::default(),
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
        let err = parse_markdown(
            "\n---\nunknown: true\n---\n# Intro",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(3));
        assert!(err.to_string().contains("invalid deck frontmatter"));
        assert_eq!(
            err.help,
            "use only the supported deck frontmatter keys: time, aspect_ratio, resolution, breaks, layouts, css, syntaxes, fonts, code_images"
        );
    }

    #[test]
    fn rejects_frontmatter_heading_line_as_missing_closing_delimiter() {
        let err = parse_markdown(
            "---\ntime: 15m\n# Intro\n---\n# Details",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

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
        let deck = parse_markdown(
            "---\ntime: 15m\n\n---\n\n# A",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();

        assert_eq!(
            settings.planned_time().map(PlannedTime::as_millis),
            Some(900_000)
        );
        assert_eq!(deck.parsed_slides().len(), 1);
        assert_eq!(deck.parsed_slides()[0].key.as_str(), "a");
    }

    #[test]
    fn rejects_frontmatter_blank_line_as_missing_closing_delimiter() {
        let err = parse_markdown(
            "---\ntime: 15m\n\n# A\n\n---\n\n# B",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

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
        let deck = parse_markdown(
            "\u{feff}---\ntime: 15m\n---\n\n# A",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();
        let slides = deck.parsed_slides();

        assert_eq!(slides.len(), 1);
        assert_eq!(slides[0].key.as_str(), "a");
    }

    #[test]
    fn rejects_leading_rule_when_no_valid_frontmatter_was_found() {
        for markdown in ["---\n\ntime: 15m\n---\n\n# A", "---\n---\n\n# A"] {
            let err =
                parse_markdown(markdown, &crate::highlight::Highlighter::defaults()).unwrap_err();

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
        let err = parse_markdown(
            "  ---\ntime: 15m\n---\n\n# A",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(1));
        assert!(err
            .to_string()
            .contains("deck starts with --- but no valid YAML frontmatter was found"));
    }

    #[test]
    fn yaml_like_slide_content_between_rules_still_splits_as_slides() {
        let markdown = "# A\n\n---\n# Cfg\nport: 8080\n\n---\n\n# C";
        let deck = parse_markdown(markdown, &crate::highlight::Highlighter::defaults()).unwrap();
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
        let err = parse_markdown(
            "  \n\n***\n\n***\n",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

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
        let err = parse_markdown(
            "# One\n\n---\n\n# Two\n\n> quote",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(7));
        assert!(err.to_string().contains("slide 2 ('two'), line 7"));
        assert!(err
            .to_string()
            .contains("unsupported construct 'blockquote'"));
    }

    #[test]
    fn image_after_delimiter_is_not_silently_dropped() {
        let deck = parse_markdown(
            "# One\n\n---\n\n![alt](image.png)",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();
        let slide = &deck.parsed_slides()[1];

        assert_eq!(slide.fragments.len(), 1);
        assert_eq!(slide.fragments[0].line(), 5);
        match slide.fragments[0].kind() {
            FragmentKind::Image { alt, src } => {
                assert_eq!(alt, "alt");
                assert_eq!(src.as_str(), "image.png");
            }
            other => panic!("expected image fragment, got {other:?}"),
        }
    }

    #[test]
    fn unsupported_table_after_delimiter_is_not_silently_dropped() {
        let err = parse_markdown(
            "# One\n\n---\n\n| a |\n| - |",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(5));
        assert!(err.to_string().contains("slide 2 ('slide-2'), line 5"));
        assert!(err.to_string().contains("unsupported construct 'table'"));
    }

    #[test]
    fn key_comment_after_delimiter_belongs_to_next_slide() {
        let markdown = "# Intro\n\n---\n<!-- {\"key\":\"arch-1\"} -->\n# Architecture";
        let deck = parse_markdown(markdown, &crate::highlight::Highlighter::defaults()).unwrap();

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
            &crate::highlight::Highlighter::defaults(),
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
        let err = parse_markdown(
            "<!-- {\"key\":\"intro\"} -->\n# One\n\n---\n# Intro",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.line, Some(5));
        assert!(err.to_string().contains("duplicate slide key 'intro'"));
        assert_eq!(
            err.help,
            "add an explicit unique key comment before this slide"
        );
    }

    #[test]
    fn rejects_derived_slide_key_collision_with_derived_key() {
        let err = parse_markdown(
            "# Intro\n\n---\n# Intro",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();

        assert_eq!(err.line, Some(4));
        assert!(err.to_string().contains("duplicate slide key 'intro'"));
        assert_eq!(
            err.help,
            "add an explicit unique key comment before this slide"
        );
    }

    // ----- Explicit slot syntax (::: {slot=name}) -----

    fn parse_first_slide(source: &str) -> ParsedSlide {
        let deck = parse_markdown(source, &crate::highlight::Highlighter::defaults()).unwrap();
        deck.parsed_slides()[0].clone()
    }

    #[test]
    fn slot_group_open_close_produces_fragment() {
        let slide = parse_first_slide("# Title\n\n::: {slot=left}\n\nleft body\n\n:::\n");
        let group = slide
            .fragments
            .iter()
            .find(|f| matches!(f.kind(), FragmentKind::SlotGroup { .. }))
            .expect("SlotGroup fragment present");
        let FragmentKind::SlotGroup { name, children } = group.kind() else {
            unreachable!();
        };
        assert_eq!(name.as_slot_name().as_str(), "left");
        assert_eq!(children.len(), 1);
        assert!(matches!(children[0].kind(), FragmentKind::Paragraph));
        assert_eq!(group.line(), 3);
    }

    #[test]
    fn slot_group_children_are_parsed() {
        let slide = parse_first_slide(
            "# Title\n\n::: {slot=left}\n\n## Sub\n\n- one\n- two\n\n```rust\nfn a(){}\n```\n\n:::\n",
        );
        let group = slide
            .fragments
            .iter()
            .find(|f| matches!(f.kind(), FragmentKind::SlotGroup { .. }))
            .expect("SlotGroup fragment present");
        let FragmentKind::SlotGroup { children, .. } = group.kind() else {
            unreachable!();
        };
        let kinds: Vec<_> = children.iter().map(|c| c.kind().to_string()).collect();
        assert!(kinds.iter().any(|k| k == "heading"));
        assert!(kinds.iter().any(|k| k == "list"));
        assert!(kinds.iter().any(|k| k == "code"));
    }

    #[test]
    fn nested_slot_group_is_error() {
        let err = parse_markdown(
            "# T\n\n::: {slot=a}\n\n::: {slot=b}\n\nx\n\n:::\n\n:::\n",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();
        assert_eq!(err.kind, ErrorKind::Parse);
        assert!(err.to_string().contains("nested"));
    }

    #[test]
    fn long_fence_four_colons_is_error() {
        let err = parse_markdown(
            "# T\n\n:::: {slot=a}\n\nx\n\n::::\n",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();
        assert_eq!(err.kind, ErrorKind::Parse);
        assert!(err.to_string().to_lowercase().contains("colon"));
    }

    #[test]
    fn unclosed_slot_group_is_error() {
        let err = parse_markdown(
            "# T\n\n::: {slot=a}\n\nx\n",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();
        assert_eq!(err.kind, ErrorKind::Parse);
        assert!(
            err.to_string().to_lowercase().contains("unclosed")
                || err.to_string().to_lowercase().contains("no closing")
        );
    }

    #[test]
    fn slot_group_missing_attr_is_error() {
        let err = parse_markdown(
            "# T\n\n:::\n\nx\n\n:::\n",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();
        // With no opening attr, the first ::: is treated as an unmatched close.
        assert_eq!(err.kind, ErrorKind::Parse);
    }

    #[test]
    fn slot_group_unknown_attr_key_is_error() {
        let err = parse_markdown(
            "# T\n\n::: {layout=x}\n\nx\n\n:::\n",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();
        assert_eq!(err.kind, ErrorKind::Parse);
        assert!(err.to_string().to_lowercase().contains("slot="));
    }

    #[test]
    fn slot_group_multi_attr_is_error() {
        let err = parse_markdown(
            "# T\n\n::: {slot=a slot=b}\n\nx\n\n:::\n",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();
        assert_eq!(err.kind, ErrorKind::Parse);
    }

    #[test]
    fn slot_group_invalid_slot_name_is_error() {
        let err = parse_markdown(
            "# T\n\n::: {slot=Foo}\n\nx\n\n:::\n",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();
        assert_eq!(err.kind, ErrorKind::Parse);
    }

    #[test]
    fn close_marker_with_attr_is_error() {
        let err = parse_markdown(
            "# T\n\n::: {slot=a}\n\nx\n\n::: {slot=a}\n",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();
        assert_eq!(err.kind, ErrorKind::Parse);
    }

    #[test]
    fn empty_slot_group_is_error() {
        let err = parse_markdown(
            "# T\n\n::: {slot=a}\n\n:::\n",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();
        assert_eq!(err.kind, ErrorKind::Parse);
        assert!(err.to_string().to_lowercase().contains("empty"));
    }

    #[test]
    fn slot_group_in_code_fence_is_ignored() {
        // Author wants to write about the syntax itself in a code block; the
        // ::: inside must not be picked up as a marker.
        let slide = parse_first_slide("# T\n\n```\n::: {slot=left}\nbody\n:::\n```\n");
        assert!(!slide
            .fragments
            .iter()
            .any(|f| matches!(f.kind(), FragmentKind::SlotGroup { .. })));
    }

    #[test]
    fn slot_group_split_across_thematic_break_is_error() {
        // The div opens in slide 1 but the closing ::: is in slide 2. From
        // slide 1's perspective, the group is unclosed.
        let err = parse_markdown(
            "# T\n\n::: {slot=a}\n\nx\n\n---\n\n# T2\n\n:::\n",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap_err();
        assert_eq!(err.kind, ErrorKind::Parse);
    }
}
