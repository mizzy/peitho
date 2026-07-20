use std::{
    fs,
    path::{Component, Path, PathBuf},
};

use serde_json::Value;

use crate::{
    error::{BuildError, ErrorKind},
    Result,
};

const MAX_INCLUDE_DEPTH: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpandedSource {
    pub source: String,
    pub body_start: usize,
    pub line_map: LineMap,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineMap {
    origins: Vec<LineOrigin>,
}

impl LineMap {
    pub fn len(&self) -> usize {
        self.origins.len()
    }

    pub fn is_empty(&self) -> bool {
        self.origins.is_empty()
    }

    pub fn translate(&self, line: usize) -> (PathBuf, usize) {
        if line == 0 {
            return (PathBuf::new(), line);
        }

        let mut output_line = 0usize;
        let mut previous = None;
        for origin in &self.origins {
            if origin.original_line == 0 {
                if output_line + 1 == line && previous.is_none() {
                    return (origin.file.clone(), 1);
                }
                continue;
            }

            output_line += 1;
            if output_line == line {
                return (origin.file.clone(), origin.original_line);
            }
            previous = Some(origin);
        }

        let index = line - 1;
        if self
            .origins
            .get(index)
            .is_some_and(|origin| origin.original_line == 0)
        {
            return self.translate_synthetic_origin(index);
        }

        (PathBuf::new(), line)
    }

    pub fn origins(&self) -> &[LineOrigin] {
        &self.origins
    }

    fn translate_synthetic_origin(&self, synthetic_index: usize) -> (PathBuf, usize) {
        let synthetic = &self.origins[synthetic_index];
        self.origins
            .get(..synthetic_index)
            .unwrap_or_default()
            .iter()
            .rev()
            .find(|origin| origin.original_line != 0)
            .map(|origin| (origin.file.clone(), origin.original_line))
            .unwrap_or_else(|| (synthetic.file.clone(), 1))
    }

    fn for_source(source: &str, path: &Path) -> Self {
        Self {
            origins: (1..=line_count(source))
                .map(|line| LineOrigin {
                    file: path.to_path_buf(),
                    original_line: line,
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineOrigin {
    pub file: PathBuf,
    pub original_line: usize,
}

pub fn expand_includes(
    top_source: &str,
    top_body_start: usize,
    top_path: &Path,
) -> Result<ExpandedSource> {
    let mut stack = vec![path_key(top_path)];
    let deck_root = include_deck_root(top_path);
    expand_includes_for_source(top_source, top_body_start, top_path, &deck_root, &mut stack)
}

fn expand_includes_for_source(
    source_input: &str,
    body_start: usize,
    current_path: &Path,
    deck_root: &Path,
    stack: &mut Vec<PathBuf>,
) -> Result<ExpandedSource> {
    let regions = scan_slide_regions(source_input, body_start)
        .map_err(|err| err.with_origin_file(current_path))?;
    if regions.iter().all(|region| region.includes.is_empty()) {
        return Ok(ExpandedSource {
            source: source_input.to_owned(),
            body_start,
            line_map: LineMap::for_source(source_input, current_path),
        });
    }

    let mut source = String::new();
    let mut origins = Vec::new();
    let mut cursor = 0usize;
    for region in regions.iter().filter(|region| !region.includes.is_empty()) {
        append_chunk(
            &mut source,
            &mut origins,
            source_input,
            cursor,
            region.start,
            current_path,
        );
        // validate_include_region guarantees region.includes.len() == 1 before we reach here.
        let include = &region.includes[0];
        validate_include_target(current_path, &include.target, deck_root, include.line)
            .map_err(|err| err.with_origin_file(current_path))?;
        let include_path = resolve_include_path(current_path, &include.target);
        let include_key = path_key(&include_path);
        if let Some(position) = stack.iter().position(|path| path == &include_key) {
            return Err(
                include_cycle_error(include.line, stack, position, &include_key)
                    .with_origin_file(current_path),
            );
        }
        if stack.len() >= MAX_INCLUDE_DEPTH {
            return Err(include_depth_error(include.line).with_origin_file(current_path));
        }
        let included_source = fs::read_to_string(&include_path).map_err(|err| {
            include_read_error(&include.target, &include_path, include.line, err)
                .with_origin_file(current_path)
        })?;
        if source_has_no_content(&included_source) {
            return Err(
                included_file_has_no_slides_error(&include.target, include.line)
                    .with_origin_file(current_path),
            );
        }
        if let Some(line) = crate::parser::detect_frontmatter_present(&included_source) {
            return Err(included_frontmatter_error(line).with_origin_file(&include_path));
        }
        stack.push(include_key);
        let expanded =
            expand_includes_for_source(&included_source, 0, &include_path, deck_root, stack)?;
        stack.pop();
        append_region_leading_newline_if_needed(
            &mut source,
            &mut origins,
            source_input,
            region.start,
            &expanded,
            current_path,
        );
        source.push_str(&expanded.source);
        origins.extend(expanded.line_map.origins);
        append_separator_boundary_newline_if_needed(
            &mut source,
            &mut origins,
            source_input,
            region.end,
            current_path,
        );
        cursor = region.end;
    }
    append_chunk(
        &mut source,
        &mut origins,
        source_input,
        cursor,
        source_input.len(),
        current_path,
    );
    Ok(ExpandedSource {
        source,
        body_start,
        line_map: LineMap { origins },
    })
}

fn path_key(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| lexically_normalize(path))
}

fn lexically_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
            Component::RootDir | Component::Prefix(_) => normalized.push(component.as_os_str()),
        }
    }
    if normalized.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        normalized
    }
}

fn include_deck_root(top_path: &Path) -> PathBuf {
    normalize_existing_path_for_include_check(parent_dir_or_dot(top_path))
}

fn parent_dir_or_dot(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn normalize_existing_path_for_include_check(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| lexically_normalize_preserving_parent(path))
}

fn lexically_normalize_preserving_parent(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match normalized.components().next_back() {
                Some(Component::Normal(_)) => {
                    normalized.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => normalized.push(".."),
            },
            Component::Normal(part) => normalized.push(part),
            Component::RootDir | Component::Prefix(_) => normalized.push(component.as_os_str()),
        }
    }
    if normalized.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        normalized
    }
}

fn append_region_leading_newline_if_needed(
    output: &mut String,
    origins: &mut Vec<LineOrigin>,
    source: &str,
    region_start: usize,
    replacement: &ExpandedSource,
    current_path: &Path,
) {
    if replacement.source.is_empty() || output.is_empty() || output.ends_with('\n') {
        return;
    }
    if source[region_start..].starts_with('\n') {
        output.push('\n');
        origins.push(synthetic_boundary_origin(current_path));
    }
}

fn append_separator_boundary_newline_if_needed(
    output: &mut String,
    origins: &mut Vec<LineOrigin>,
    source: &str,
    next: usize,
    current_path: &Path,
) {
    if output.is_empty() || output.ends_with('\n') {
        return;
    }
    let next_line = source[next..]
        .split_inclusive('\n')
        .next()
        .unwrap_or_default()
        .trim_end_matches('\n')
        .trim_end_matches('\r');
    if is_slide_separator(next_line) {
        output.push('\n');
        origins.push(synthetic_boundary_origin(current_path));
    }
}

fn synthetic_boundary_origin(current_path: &Path) -> LineOrigin {
    LineOrigin {
        file: current_path.to_path_buf(),
        original_line: 0,
    }
}

fn include_cycle_error(
    line: usize,
    stack: &[PathBuf],
    cycle_start: usize,
    target: &Path,
) -> BuildError {
    let mut parts = stack[cycle_start..]
        .iter()
        .map(|path| path_label(path))
        .collect::<Vec<_>>();
    parts.push(path_label(target));
    BuildError::new(
        ErrorKind::Parse,
        Some(line),
        format!("include cycle detected: {}", parts.join(" -> ")),
        "remove one of the include comments in the cycle",
    )
}

fn include_depth_error(line: usize) -> BuildError {
    BuildError::new(
        ErrorKind::Parse,
        Some(line),
        format!("include chain exceeds max depth of {MAX_INCLUDE_DEPTH}"),
        "reduce include nesting",
    )
}

fn path_label(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn line_count(source: &str) -> usize {
    source.lines().count()
}

#[derive(Debug)]
struct SlideRegion<'a> {
    start: usize,
    end: usize,
    lines: Vec<&'a str>,
    includes: Vec<IncludeDirective>,
}

#[derive(Debug)]
struct IncludeDirective {
    target: PathBuf,
    line: usize,
}

#[derive(Debug)]
struct SourceLine<'a> {
    line: &'a str,
    line_no: usize,
    start: usize,
    end: usize,
}

fn scan_slide_regions(source: &str, body_start: usize) -> Result<Vec<SlideRegion<'_>>> {
    let mut regions = Vec::new();
    let mut current = SlideRegion {
        start: body_start,
        end: body_start,
        lines: Vec::new(),
        includes: Vec::new(),
    };
    let mut in_code_fence: Option<(char, usize)> = None;

    for source_line in source_lines_from(source, body_start) {
        let trimmed = source_line.line.trim_end_matches('\r');

        if in_code_fence.is_none() && source_line.start >= body_start && is_slide_separator(trimmed)
        {
            current.end = source_line.start;
            validate_include_region(&current)?;
            regions.push(current);
            current = SlideRegion {
                start: source_line.end,
                end: source_line.end,
                lines: Vec::new(),
                includes: Vec::new(),
            };
            continue;
        }

        let include = if in_code_fence.is_none() {
            parse_include_comment(trimmed, source_line.line_no)?
        } else {
            None
        };

        current.lines.push(source_line.line);
        if let Some(include) = include {
            current.includes.push(include);
        }

        if let Some((fence_char, fence_len)) = in_code_fence {
            if crate::parser::is_closing_code_fence(trimmed, fence_char, fence_len) {
                in_code_fence = None;
            }
        } else if let Some((fence_char, fence_len)) = crate::parser::opening_code_fence(trimmed) {
            in_code_fence = Some((fence_char, fence_len));
        }
    }

    current.end = source.len();
    validate_include_region(&current)?;
    regions.push(current);
    Ok(regions)
}

fn source_lines_from(source: &str, body_start: usize) -> Vec<SourceLine<'_>> {
    let mut lines = Vec::new();
    let mut offset = 0usize;
    for (index, raw_line) in source.split_inclusive('\n').enumerate() {
        let start = offset;
        let end = start + raw_line.len();
        offset = end;
        if end <= body_start {
            continue;
        }
        let slice_start = body_start.saturating_sub(start);
        let line = raw_line[slice_start..]
            .strip_suffix('\n')
            .unwrap_or(&raw_line[slice_start..]);
        lines.push(SourceLine {
            line,
            line_no: index + 1,
            start,
            end,
        });
    }
    lines
}

fn validate_include_region(region: &SlideRegion<'_>) -> Result<()> {
    if region.includes.is_empty() {
        return Ok(());
    }
    let include = &region.includes[0];
    let significant_lines = region
        .lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .count();
    if region.includes.len() != 1 || significant_lines != 1 {
        return Err(include_container_error(include.line));
    }
    Ok(())
}

fn parse_include_comment(line: &str, line_no: usize) -> Result<Option<IncludeDirective>> {
    let trimmed = line.trim();
    let Some(json) = trimmed
        .strip_prefix("<!--")
        .and_then(|comment| comment.strip_suffix("-->"))
        .map(str::trim)
    else {
        return Ok(None);
    };
    if !json.contains("\"include\"") {
        return Ok(None);
    }

    let parsed: Value =
        serde_json::from_str(json).map_err(|err| invalid_include_comment_error(line_no, err))?;
    let Value::Object(object) = parsed else {
        return Err(BuildError::new(
            ErrorKind::Parse,
            Some(line_no),
            "include comment must be a JSON object",
            r#"use <!-- {"include":"path/to/slides.md"} -->"#,
        ));
    };
    if !object.contains_key("include") {
        return Ok(None);
    }
    if object.len() != 1 {
        return Err(BuildError::new(
            ErrorKind::Parse,
            Some(line_no),
            "include comment accepts only include",
            r#"use <!-- {"include":"path/to/slides.md"} --> on a slide with no other settings"#,
        ));
    }
    let Some(include) = object.get("include").and_then(Value::as_str) else {
        return Err(include_value_error(line_no));
    };
    if include.trim().is_empty() {
        return Err(include_value_error(line_no));
    }
    Ok(Some(IncludeDirective {
        target: PathBuf::from(include),
        line: line_no,
    }))
}

fn include_value_error(line: usize) -> BuildError {
    BuildError::new(
        ErrorKind::Parse,
        Some(line),
        "include value must be a non-empty string",
        r#"set "include" to a deck-relative Markdown file path"#,
    )
}

fn invalid_include_comment_error(line: usize, err: serde_json::Error) -> BuildError {
    BuildError::new(
        ErrorKind::Parse,
        Some(line),
        format!("invalid include comment: {err}"),
        r#"use <!-- {"include":"path/to/slides.md"} -->"#,
    )
}

fn include_container_error(line: usize) -> BuildError {
    BuildError::new(
        ErrorKind::Parse,
        Some(line),
        "include comment must be the only content of its slide",
        "place the include comment in its own slide bounded by `---`",
    )
}

fn resolve_include_path(current_path: &Path, target: &Path) -> PathBuf {
    parent_dir_or_dot(current_path).join(target)
}

fn validate_include_target(
    current_path: &Path,
    target: &Path,
    deck_root: &Path,
    line: usize,
) -> Result<()> {
    if target.is_absolute() {
        return Err(absolute_include_path_error(line));
    }

    let current_dir = normalize_existing_path_for_include_check(parent_dir_or_dot(current_path));
    let resolved = lexically_normalize_preserving_parent(&current_dir.join(target));
    if !resolved.starts_with(deck_root) {
        return Err(escaping_include_path_error(line));
    }
    Ok(())
}

fn absolute_include_path_error(line: usize) -> BuildError {
    BuildError::new(
        ErrorKind::Parse,
        Some(line),
        "include path must be deck-relative, not absolute",
        "use a path relative to the including file (e.g. `shared/intro.md`)",
    )
}

fn escaping_include_path_error(line: usize) -> BuildError {
    BuildError::new(
        ErrorKind::Parse,
        Some(line),
        "include path must not escape the deck directory",
        "keep includes within the deck's directory or a subdirectory",
    )
}

fn include_read_error(
    target: &Path,
    resolved: &Path,
    line: usize,
    err: std::io::Error,
) -> BuildError {
    match err.kind() {
        std::io::ErrorKind::NotFound => BuildError::new(
            ErrorKind::Parse,
            Some(line),
            format!("include file not found: {}", target.display()),
            "create the included file or fix the include path",
        ),
        std::io::ErrorKind::IsADirectory => include_directory_error(target, line),
        _ if resolved.is_dir() => include_directory_error(target, line),
        _ => BuildError::new(
            ErrorKind::Parse,
            Some(line),
            format!("failed to read include file {}: {err}", target.display()),
            "make the included file readable or fix the include path",
        ),
    }
}

fn include_directory_error(target: &Path, line: usize) -> BuildError {
    BuildError::new(
        ErrorKind::Parse,
        Some(line),
        format!(
            "include target is a directory, not a file: {}",
            target.display()
        ),
        "point include at a Markdown file, not a directory",
    )
}

fn included_file_has_no_slides_error(target: &Path, line: usize) -> BuildError {
    BuildError::new(
        ErrorKind::Parse,
        Some(line),
        format!("included file has no slides: {}", target.display()),
        "add at least one slide to the included file or remove the include comment",
    )
}

fn included_frontmatter_error(line: usize) -> BuildError {
    BuildError::new(
        ErrorKind::Parse,
        Some(line),
        "included file has frontmatter, which is not supported",
        "move frontmatter to the top-level deck; included files may only contain slides",
    )
}

fn append_chunk(
    output: &mut String,
    origins: &mut Vec<LineOrigin>,
    source: &str,
    start: usize,
    end: usize,
    path: &Path,
) {
    if start == end {
        return;
    }
    let chunk = &source[start..end];
    let first_line = crate::parser::line_for_offset(source, start);
    output.push_str(chunk);
    origins.extend((0..line_count(chunk)).map(|index| LineOrigin {
        file: path.to_path_buf(),
        original_line: first_line + index,
    }));
}

fn is_slide_separator(line: &str) -> bool {
    line.trim() == "---"
}

fn source_has_no_content(source: &str) -> bool {
    source
        .strip_prefix('\u{feff}')
        .unwrap_or(source)
        .trim()
        .is_empty()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use crate::domain::{FragmentKind, SlotName};

    use super::*;

    #[test]
    fn expand_includes_returns_source_unchanged_when_no_include_comment_is_present() {
        let source = "---\ntime: 1m\n---\n# Intro\n\nBody\n";
        let expanded = expand_includes(source, 17, Path::new("deck.md")).unwrap();

        assert_eq!(expanded.source, source);
        assert_eq!(expanded.body_start, 17);
        assert_eq!(expanded.line_map.len(), source.lines().count());
        assert_eq!(
            expanded.line_map.translate(4),
            (Path::new("deck.md").to_path_buf(), 4)
        );
    }

    #[test]
    fn include_comment_with_sibling_heading_is_an_error() {
        let source = "# Container\n\n<!-- {\"include\":\"shared.md\"} -->\n";
        let err = expand_includes(source, 0, Path::new("deck.md")).unwrap_err();

        assert_eq!(err.line, Some(3));
        assert!(err
            .message
            .contains("include comment must be the only content of its slide"));
        assert!(err
            .help
            .contains("place the include comment in its own slide"));
    }

    #[test]
    fn include_comment_with_sibling_page_settings_comment_is_an_error() {
        let source = "<!-- {\"key\":\"container\"} -->\n<!-- {\"include\":\"shared.md\"} -->\n";
        let err = expand_includes(source, 0, Path::new("deck.md")).unwrap_err();

        assert_eq!(err.line, Some(2));
        assert!(err
            .message
            .contains("include comment must be the only content of its slide"));
    }

    #[test]
    fn include_comment_with_sibling_speaker_note_is_an_error() {
        let source = "<!-- presenter note -->\n<!-- {\"include\":\"shared.md\"} -->\n";
        let err = expand_includes(source, 0, Path::new("deck.md")).unwrap_err();

        assert_eq!(err.line, Some(2));
        assert!(err
            .message
            .contains("include comment must be the only content of its slide"));
    }

    #[test]
    fn two_include_comments_in_the_same_slide_are_an_error() {
        let source = "<!-- {\"include\":\"a.md\"} -->\n<!-- {\"include\":\"b.md\"} -->\n";
        let err = expand_includes(source, 0, Path::new("deck.md")).unwrap_err();

        assert_eq!(err.line, Some(1));
        assert!(err
            .message
            .contains("include comment must be the only content of its slide"));
    }

    #[test]
    fn include_comment_rejects_page_settings_keys_on_the_same_comment() {
        for (name, extra) in [
            ("draft", r#""draft":true"#),
            ("skip", r#""skip":true"#),
            ("section", r#""section":"Intro","time":"1m""#),
            ("layout", r#""layout":"cover""#),
            ("key", r#""key":"intro""#),
            ("page_number", r#""page_number":false"#),
        ] {
            let source = format!(r#"<!-- {{"include":"shared.md",{extra}}} -->"#);
            let err = expand_includes(&source, 0, Path::new("deck.md")).unwrap_err();

            assert_eq!(err.line, Some(1), "{name}");
            assert!(
                err.message.contains("include comment accepts only include"),
                "{name}: {}",
                err.message
            );
        }
    }

    #[test]
    fn include_comment_rejects_non_string_include_value() {
        let source = r#"<!-- {"include":42} -->"#;
        let err = expand_includes(source, 0, Path::new("deck.md")).unwrap_err();

        assert_eq!(err.line, Some(1));
        assert!(err
            .message
            .contains("include value must be a non-empty string"));
    }

    #[test]
    fn include_comment_rejects_empty_include_value() {
        let source = r#"<!-- {"include":""} -->"#;
        let err = expand_includes(source, 0, Path::new("deck.md")).unwrap_err();

        assert_eq!(err.line, Some(1));
        assert!(err
            .message
            .contains("include value must be a non-empty string"));
    }

    #[test]
    fn include_comment_rejects_invalid_json() {
        let source = r#"<!-- {"include":} -->"#;
        let err = expand_includes(source, 0, Path::new("deck.md")).unwrap_err();

        assert_eq!(err.line, Some(1));
        assert!(err.message.contains("invalid include comment"));
    }

    #[test]
    fn include_comment_inside_fenced_code_block_is_preserved() {
        let source =
            "```markdown\n<!-- {\"include\":\"shared.md\",\"layout\":\"cover\"} -->\n```\n";
        let expanded = expand_includes(source, 0, Path::new("deck.md")).unwrap();

        assert_eq!(expanded.source, source);
    }

    #[test]
    fn malformed_non_include_page_comment_is_left_for_the_parser() {
        let source = r#"<!-- {"key":} -->"#;
        let expanded = expand_includes(source, 0, Path::new("deck.md")).unwrap();

        assert_eq!(expanded.source, source);
    }

    #[test]
    fn missing_include_target_is_a_line_numbered_error() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let source = "# Before\n\n---\n<!-- {\"include\":\"missing.md\"} -->\n---\n# After\n";

        let err = expand_includes(source, 0, &deck).unwrap_err();

        assert_eq!(err.line, Some(4));
        assert!(err.message.contains("include file not found"));
        assert!(err.message.contains("missing.md"));
    }

    #[test]
    fn directory_include_target_is_a_friendly_line_numbered_error() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        fs::create_dir(dir.path().join("shared")).unwrap();
        let source = "<!-- {\"include\":\"shared\"} -->\n";

        let err = expand_includes(source, 0, &deck).unwrap_err();

        assert_eq!(err.line, Some(1));
        assert_eq!(err.origin_file, Some(deck));
        assert_eq!(
            err.message,
            "include target is a directory, not a file: shared"
        );
        assert_eq!(
            err.help,
            "point include at a Markdown file, not a directory"
        );
    }

    #[test]
    fn included_file_with_frontmatter_is_a_line_numbered_error() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        fs::write(
            dir.path().join("shared.md"),
            "---\ntime: 1m\n---\n# Included\n",
        )
        .unwrap();
        let source = "<!-- {\"include\":\"shared.md\"} -->\n";

        let err = expand_includes(source, 0, &deck).unwrap_err();

        assert_eq!(err.line, Some(1));
        assert_eq!(
            err.message,
            "included file has frontmatter, which is not supported"
        );
        assert!(err.help.contains("move frontmatter to the top-level deck"));
    }

    #[test]
    fn included_file_with_malformed_frontmatter_is_a_line_numbered_error() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        fs::write(dir.path().join("shared.md"), "---\ntime: 1m").unwrap();
        let source = "<!-- {\"include\":\"shared.md\"} -->\n";

        let err = expand_includes(source, 0, &deck).unwrap_err();

        assert_eq!(err.line, Some(1));
        assert_eq!(err.origin_file, Some(dir.path().join("shared.md")));
        assert_eq!(
            err.message,
            "included file has frontmatter, which is not supported"
        );
    }

    #[test]
    fn empty_included_file_is_a_line_numbered_error() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        fs::write(dir.path().join("shared.md"), " \n\t\n").unwrap();
        let source = "# Before\n\n---\n<!-- {\"include\":\"shared.md\"} -->\n---\n# After\n";

        let err = expand_includes(source, 0, &deck).unwrap_err();

        assert_eq!(err.line, Some(4));
        assert_eq!(err.origin_file, Some(deck));
        assert_eq!(err.message, "included file has no slides: shared.md");
        assert!(err.help.contains("add at least one slide"));
    }

    #[test]
    fn absolute_include_target_is_a_line_numbered_error() {
        let source = "# Before\n\n---\n<!-- {\"include\":\"/etc/passwd\"} -->\n";

        let err = expand_includes(source, 0, Path::new("deck.md")).unwrap_err();

        assert_eq!(err.line, Some(4));
        assert_eq!(err.origin_file, Some(Path::new("deck.md").to_path_buf()));
        assert_eq!(
            err.message,
            "include path must be deck-relative, not absolute"
        );
        assert_eq!(
            err.help,
            "use a path relative to the including file (e.g. `shared/intro.md`)"
        );
    }

    #[test]
    fn parent_directory_include_escape_is_a_line_numbered_error() {
        let dir = tempfile::tempdir().unwrap();
        let deck_dir = dir.path().join("dir");
        fs::create_dir(&deck_dir).unwrap();
        let deck = deck_dir.join("deck.md");
        fs::write(dir.path().join("secret.md"), "# Secret\n").unwrap();
        let source = "<!-- {\"include\":\"../secret.md\"} -->\n";

        let err = expand_includes(source, 0, &deck).unwrap_err();

        assert_eq!(err.line, Some(1));
        assert_eq!(err.origin_file, Some(deck));
        assert_eq!(
            err.message,
            "include path must not escape the deck directory"
        );
        assert_eq!(
            err.help,
            "keep includes within the deck's directory or a subdirectory"
        );
    }

    #[test]
    fn subdirectory_include_under_deck_root_is_allowed() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let subdir = dir.path().join("subdir");
        fs::create_dir(&subdir).unwrap();
        fs::write(subdir.join("foo.md"), "# Included\n").unwrap();
        let source = "<!-- {\"include\":\"subdir/foo.md\"} -->\n";

        let expanded = expand_includes(source, 0, &deck).unwrap();

        assert_eq!(expanded.source, "# Included\n");
        assert_eq!(
            expanded.line_map.translate(1),
            (dir.path().join("subdir/foo.md"), 1)
        );
    }

    #[test]
    fn nested_parent_directory_include_escape_uses_top_deck_root() {
        let dir = tempfile::tempdir().unwrap();
        let deck_dir = dir.path().join("dir");
        fs::create_dir(&deck_dir).unwrap();
        let deck = deck_dir.join("deck.md");
        let included = deck_dir.join("a.md");
        fs::write(&included, "<!-- {\"include\":\"../B.md\"} -->\n").unwrap();
        fs::write(dir.path().join("B.md"), "# B\n").unwrap();
        let source = "<!-- {\"include\":\"a.md\"} -->\n";

        let err = expand_includes(source, 0, &deck).unwrap_err();

        assert_eq!(err.line, Some(1));
        assert_eq!(err.origin_file, Some(included));
        assert_eq!(
            err.message,
            "include path must not escape the deck directory"
        );
        assert_eq!(
            err.help,
            "keep includes within the deck's directory or a subdirectory"
        );
    }

    #[test]
    fn single_file_include_splices_slides_and_removes_container_slide() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        let included = dir.path().join("shared.md");
        fs::write(
            &included,
            "# Included One\n\n---\n<!-- {\"section\":\"Shared\",\"time\":\"1m\"} -->\n# Included Two\n",
        )
        .unwrap();
        let source = "# Before\n\n---\n<!-- {\"include\":\"shared.md\"} -->\n---\n# After\n";

        let expanded = expand_includes(source, 0, &deck).unwrap();

        assert_eq!(
            expanded.source,
            "# Before\n\n---\n# Included One\n\n---\n<!-- {\"section\":\"Shared\",\"time\":\"1m\"} -->\n# Included Two\n---\n# After\n"
        );
        assert_eq!(
            expanded.line_map.translate(4),
            (included.clone(), 1),
            "included slide should retain its source file line"
        );
    }

    #[test]
    fn include_without_trailing_newline_keeps_following_separator_on_new_line() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        fs::write(dir.path().join("shared.md"), "# Included").unwrap();
        let source = "# Before\n\n---\n<!-- {\"include\":\"shared.md\"} -->\n---\n# After\n";

        let expanded = expand_includes(source, 0, &deck).unwrap();

        assert_eq!(
            expanded.source,
            "# Before\n\n---\n# Included\n---\n# After\n"
        );
        assert_eq!(expanded.line_map.translate(6), (deck, 6));
    }

    #[test]
    fn include_after_top_frontmatter_keeps_frontmatter_separator_on_its_own_line() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        fs::write(dir.path().join("shared.md"), "# Included\n").unwrap();
        let source = "---\ntime: 1m\n---\n<!-- {\"include\":\"shared.md\"} -->\n";
        let frontmatter = crate::parser::parse_frontmatter(source).unwrap();

        let expanded = expand_includes(source, frontmatter.body_start(), &deck).unwrap();

        assert_eq!(expanded.source, "---\ntime: 1m\n---\n# Included\n");
        assert_eq!(
            expanded.line_map.translate(4),
            (dir.path().join("shared.md"), 1)
        );
        assert!(expanded
            .line_map
            .origins
            .iter()
            .any(|origin| origin.file == deck && origin.original_line == 0));
    }

    #[test]
    fn expanded_source_with_included_section_marker_parses_as_one_deck() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        fs::write(
            dir.path().join("shared.md"),
            "<!-- {\"section\":\"Shared\",\"time\":\"1m\"} -->\n# Included\n",
        )
        .unwrap();
        let source = "<!-- {\"include\":\"shared.md\"} -->\n---\n# After\n";

        let expanded = expand_includes(source, 0, &deck).unwrap();
        let frontmatter = crate::parser::parse_frontmatter(&expanded.source).unwrap();
        let parsed = crate::parser::parse_markdown(
            &expanded.source,
            frontmatter,
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();

        assert_eq!(parsed.parsed_slides().len(), 2);
        assert_eq!(parsed.settings().sections()[0].name(), "Shared");
        assert_eq!(parsed.settings().sections()[0].start(), 0);
        assert_eq!(parsed.settings().sections()[0].end(), 1);
    }

    #[test]
    fn included_explicit_body_slot_survives_expand_parse_and_map() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        fs::write(
            dir.path().join("shared.md"),
            "# Included title\n\n::: {slot=body}\n\nIncluded body content\n\n:::\n",
        )
        .unwrap();
        let source = "# Deck slide\n\n---\n<!-- {\"include\":\"shared.md\"} -->\n";
        let top_frontmatter = crate::parser::parse_frontmatter(source).unwrap();

        let expanded = expand_includes(source, top_frontmatter.body_start(), &deck).unwrap();

        assert_eq!(
            expanded.source,
            "# Deck slide\n\n---\n# Included title\n\n::: {slot=body}\n\nIncluded body content\n\n:::\n"
        );

        let frontmatter = crate::parser::parse_frontmatter(&expanded.source).unwrap();
        let parsed = crate::parser::parse_markdown(
            &expanded.source,
            frontmatter,
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();

        assert_eq!(parsed.parsed_slides().len(), 2);
        let included = &parsed.parsed_slides()[1];
        let slot_group = included
            .fragments
            .iter()
            .find(|fragment| matches!(fragment.kind(), FragmentKind::SlotGroup { .. }))
            .expect("included slide should keep its explicit slot group");
        let FragmentKind::SlotGroup { name, children } = slot_group.kind() else {
            unreachable!();
        };
        assert_eq!(name.as_slot_name().as_str(), "body");
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].kind(), &FragmentKind::Paragraph);
        assert_eq!(children[0].markdown(), "Included body content");

        let layout = crate::layout::parse_layout(
            "title-body",
            r#"<section>
                <slot name="title" accepts="inline" arity="1"></slot>
                <slot name="body" accepts="blocks" arity="0..*"></slot>
            </section>"#,
        )
        .unwrap();
        let mapped = crate::mapping::map_by_convention(parsed, &layout).unwrap();
        let body_slot = SlotName::new("body").unwrap();
        let body_fragments = mapped.mapped_slides()[1].slots[&body_slot].fragments();

        assert_eq!(body_fragments.len(), 1);
        assert_eq!(body_fragments[0].kind(), &FragmentKind::Paragraph);
        assert_eq!(body_fragments[0].markdown(), "Included body content");
    }

    #[test]
    fn nested_include_chain_splices_all_slides_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        fs::write(
            dir.path().join("a.md"),
            "# A\n\n---\n<!-- {\"include\":\"b.md\"} -->\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("b.md"),
            "# B\n\n---\n<!-- {\"include\":\"c.md\"} -->\n",
        )
        .unwrap();
        fs::write(dir.path().join("c.md"), "# C\n").unwrap();
        let source = "<!-- {\"include\":\"a.md\"} -->\n";

        let expanded = expand_includes(source, 0, &deck).unwrap();

        assert_eq!(expanded.source, "# A\n\n---\n# B\n\n---\n# C\n");
        assert_eq!(expanded.line_map.translate(7), (dir.path().join("c.md"), 1));
    }

    #[test]
    fn acyclic_include_chain_over_max_depth_is_a_line_numbered_error() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        for index in 1..=70 {
            let path = dir.path().join(format!("{index}.md"));
            let source = if index == 70 {
                "# Leaf\n".to_owned()
            } else {
                format!("\n<!-- {{\"include\":\"{}.md\"}} -->\n", index + 1)
            };
            fs::write(path, source).unwrap();
        }
        let source = "\n<!-- {\"include\":\"1.md\"} -->\n";

        let err = expand_includes(source, 0, &deck).unwrap_err();

        assert_eq!(err.line, Some(2));
        assert_eq!(err.message, "include chain exceeds max depth of 64");
        assert_eq!(err.help, "reduce include nesting");
    }

    #[test]
    fn self_include_cycle_is_a_line_numbered_error() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        fs::write(
            dir.path().join("shared.md"),
            "<!-- {\"include\":\"shared.md\"} -->\n",
        )
        .unwrap();
        let source = "<!-- {\"include\":\"shared.md\"} -->\n";

        let err = expand_includes(source, 0, &deck).unwrap_err();

        assert_eq!(err.line, Some(1));
        assert!(err.message.contains("include cycle detected"));
        assert!(err.message.contains("shared.md -> shared.md"));
    }

    #[test]
    fn multi_file_include_cycle_is_a_line_numbered_error() {
        let dir = tempfile::tempdir().unwrap();
        let deck = dir.path().join("deck.md");
        fs::write(dir.path().join("a.md"), "<!-- {\"include\":\"b.md\"} -->\n").unwrap();
        fs::write(dir.path().join("b.md"), "<!-- {\"include\":\"a.md\"} -->\n").unwrap();
        let source = "<!-- {\"include\":\"a.md\"} -->\n";

        let err = expand_includes(source, 0, &deck).unwrap_err();

        assert_eq!(err.line, Some(1));
        assert!(err.message.contains("include cycle detected"));
        assert!(err.message.contains("a.md -> b.md -> a.md"));
    }
}
