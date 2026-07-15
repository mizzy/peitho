use std::{
    any::Any,
    borrow::Cow,
    fs::{self, OpenOptions},
    io::{self, Write},
    panic::{catch_unwind, AssertUnwindSafe},
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        LazyLock,
    },
};

use sha2::{Digest, Sha256};

use crate::{
    domain::{
        CodeImageCommand, CodeImageRenderer, CodeImagesConfig, FragmentKind, RawImagePath,
        SourceFragment,
    },
    error::{BuildError, ErrorKind, Result},
    highlight::Highlighter,
    parser::{parse_markdown, ParsedFrontmatter},
    phase::{Deck, Parsed},
};

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);
static BUILTIN_MERMAID_RENDERER: LazyLock<merman::render::HeadlessRenderer> =
    LazyLock::new(merman::render::HeadlessRenderer::new);

pub trait SvgRunner {
    fn run(&self, command: &CodeImageCommand, stdin: &str) -> Result<Vec<u8>>;
}

pub fn parse_deck_and_transform<R: SvgRunner>(
    source: &str,
    frontmatter: ParsedFrontmatter,
    highlighter: &Highlighter,
    runner: &R,
    cache_dir: &Path,
) -> Result<Deck<Parsed>> {
    let config = frontmatter.settings().code_images().clone();
    let parsed = parse_markdown(source, frontmatter, highlighter)?;
    transform_code_images(parsed, &config, runner, cache_dir)
}

pub fn transform_code_images<R: SvgRunner>(
    deck: Deck<Parsed>,
    config: &CodeImagesConfig,
    runner: &R,
    cache_dir: &Path,
) -> Result<Deck<Parsed>> {
    let (settings, slides) = deck.into_parsed_parts();
    let mut transformed_slides = Vec::with_capacity(slides.len());

    for mut slide in slides {
        slide.fragments = slide
            .fragments
            .into_iter()
            .map(|fragment| transform_fragment(fragment, config, runner, cache_dir))
            .collect::<Result<Vec<_>>>()?;
        transformed_slides.push(slide);
    }

    Ok(Deck::parsed(settings, transformed_slides))
}

fn transform_fragment<R: SvgRunner>(
    fragment: SourceFragment,
    config: &CodeImagesConfig,
    runner: &R,
    cache_dir: &Path,
) -> Result<SourceFragment> {
    if let Some(tag) = fragment.language() {
        if matches!(fragment.kind(), FragmentKind::Code) {
            if let Some(renderer) = config.renderer_for(tag) {
                let key = match &renderer {
                    CodeImageRenderer::External(command) => {
                        code_image_cache_key(command, fragment.code_text())
                    }
                    CodeImageRenderer::BuiltinMermaid => {
                        builtin_mermaid_cache_key(fragment.code_text())
                    }
                };
                let cache_path = cache_dir.join(format!("{key}.svg"));
                fs::create_dir_all(cache_dir).map_err(|err| {
                    code_image_error(
                        fragment.line(),
                        tag,
                        format!("failed to create code image cache directory: {err}"),
                        "make the .peitho directory writable and rebuild",
                    )
                })?;
                let cache_hit = valid_cached_svg(&cache_path);
                if !cache_hit {
                    let (bytes, output_context) = match &renderer {
                        CodeImageRenderer::External(command) => {
                            let bytes =
                                runner.run(command, fragment.code_text()).map_err(|err| {
                                    code_image_error(fragment.line(), tag, err.message, err.help)
                                })?;
                            (bytes, CodeImageOutputContext::ExternalCommand)
                        }
                        CodeImageRenderer::BuiltinMermaid => {
                            let bytes = render_builtin_mermaid(fragment.code_text()).map_err(
                                |message| {
                                    code_image_error(
                                        fragment.line(),
                                        tag,
                                        message,
                                        builtin_mermaid_override_help(),
                                    )
                                },
                            )?;
                            (bytes, CodeImageOutputContext::BuiltinMermaid)
                        }
                    };
                    validate_svg_output(fragment.line(), tag, &bytes, output_context)?;
                    let bytes =
                        normalize_svg_intrinsic_size(fragment.line(), tag, &bytes, output_context)?;
                    write_cache_file_atomic(&cache_path, bytes.as_ref()).map_err(|err| {
                        code_image_error(
                            fragment.line(),
                            tag,
                            format!("failed to write code image cache file: {err}"),
                            "make the .peitho directory writable and rebuild",
                        )
                    })?;
                }
                let raw = RawImagePath::from_code_images_cache(&key);
                return Ok(SourceFragment::image(
                    fragment.line(),
                    format!("diagram ({tag})"),
                    raw,
                ));
            }
        }
    }

    match fragment.kind() {
        FragmentKind::SlotGroup { name, children } => {
            let children = children
                .clone()
                .into_iter()
                .map(|child| transform_fragment(child, config, runner, cache_dir))
                .collect::<Result<Vec<_>>>()?;
            Ok(SourceFragment::slot_group(
                fragment.line(),
                name.clone(),
                children,
            ))
        }
        FragmentKind::Heading { .. }
        | FragmentKind::Paragraph
        | FragmentKind::Text
        | FragmentKind::Code
        | FragmentKind::Image { .. }
        | FragmentKind::List => Ok(fragment),
    }
}

fn render_builtin_mermaid(code_text: &str) -> std::result::Result<Vec<u8>, String> {
    render_builtin_mermaid_with(|| BUILTIN_MERMAID_RENDERER.render_svg_sync(code_text))
}

fn render_builtin_mermaid_with<F>(render: F) -> std::result::Result<Vec<u8>, String>
where
    F: FnOnce() -> std::result::Result<Option<String>, merman::render::HeadlessError>,
{
    // AssertUnwindSafe is limited to the captured static HeadlessRenderer: plain immutable data with no interior mutability in merman 0.7.0; re-verify on merman upgrades.
    let result = catch_unwind(AssertUnwindSafe(render)).map_err(|payload| {
        format!(
            "built-in mermaid renderer panicked: {}",
            panic_payload_message(payload.as_ref())
        )
    })?;
    let svg = match result {
        Ok(Some(svg)) => svg,
        Ok(None) => return Err(builtin_mermaid_non_diagram_message().to_owned()),
        Err(merman::render::HeadlessError::Parse(merman::Error::DetectType(_))) => {
            return Err(builtin_mermaid_non_diagram_message().to_owned());
        }
        Err(err) => return Err(err.to_string()),
    };
    Ok(svg.into_bytes())
}

fn panic_payload_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_owned()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic payload".to_owned()
    }
}

fn builtin_mermaid_non_diagram_message() -> &'static str {
    "built-in renderer did not detect a mermaid diagram"
}

fn builtin_mermaid_override_help() -> &'static str {
    "fix the mermaid source, or set code_images.mermaid to an external command like mmdc -i - -o - -e svg"
}

fn valid_cached_svg(path: &Path) -> bool {
    let cache_hit = fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.len() > 0)
        .unwrap_or(false);
    if !cache_hit {
        return false;
    }
    fs::read(path)
        .map(|bytes| is_valid_svg_bytes(&bytes) && svg_has_usable_intrinsic_size(&bytes))
        .unwrap_or(false)
}

fn code_image_cache_key(command: &CodeImageCommand, code_text: &str) -> String {
    let mut hasher = Sha256::new();
    for arg in &command.argv {
        hasher.update(arg.as_bytes());
        hasher.update([0]);
    }
    hasher.update(code_text.as_bytes());
    hex_encode(&hasher.finalize())
}

fn builtin_mermaid_cache_key(code_text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"\0peitho-builtin-mermaid\0");
    hasher.update(env!("CARGO_PKG_VERSION").as_bytes());
    hasher.update(b"\0");
    hasher.update(code_text.as_bytes());
    hex_encode(&hasher.finalize())
}

fn write_cache_file_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("code-image.svg");
    let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_path = dir.join(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        counter
    ));

    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)?;
        file.write_all(bytes)?;
        file.flush()?;
        fs::rename(&tmp_path, path)?;
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }
    result
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

#[derive(Clone, Copy)]
enum CodeImageOutputContext {
    ExternalCommand,
    BuiltinMermaid,
}

fn validate_svg_output(
    line: usize,
    tag: &str,
    bytes: &[u8],
    context: CodeImageOutputContext,
) -> Result<()> {
    if bytes.is_empty() {
        return Err(svg_empty_output_error(line, tag, context));
    }
    if !is_valid_svg_bytes(bytes) {
        return Err(svg_not_document_error(line, tag, context));
    }
    Ok(())
}

fn is_valid_svg_bytes(bytes: &[u8]) -> bool {
    !bytes.is_empty() && is_svg_output(bytes)
}

fn normalize_svg_intrinsic_size<'a>(
    line: usize,
    tag: &str,
    bytes: &'a [u8],
    context: CodeImageOutputContext,
) -> Result<Cow<'a, [u8]>> {
    let Some(root) = find_root_svg_tag(bytes) else {
        return Err(svg_root_not_found_error(line, tag, context));
    };
    let attrs = parse_svg_root_attributes(bytes, root);

    if svg_root_has_usable_dimensions(bytes, attrs) {
        return Ok(Cow::Borrowed(bytes));
    }

    let Some(view_box) = attrs
        .view_box
        .and_then(|attr| parse_view_box_dimensions(&bytes[attr.value_start..attr.value_end]))
    else {
        return Err(svg_intrinsic_size_error(line, tag, context));
    };

    Ok(Cow::Owned(apply_dimension_edits(
        bytes, root, attrs, view_box,
    )))
}

fn svg_has_usable_intrinsic_size(bytes: &[u8]) -> bool {
    let Some(root) = find_root_svg_tag(bytes) else {
        return false;
    };
    let attrs = parse_svg_root_attributes(bytes, root);
    svg_root_has_usable_dimensions(bytes, attrs)
}

fn svg_root_has_usable_dimensions(bytes: &[u8], attrs: SvgRootAttributes) -> bool {
    attrs
        .width
        .is_some_and(|attr| is_usable_svg_length(&bytes[attr.value_start..attr.value_end]))
        && attrs
            .height
            .is_some_and(|attr| is_usable_svg_length(&bytes[attr.value_start..attr.value_end]))
}

#[derive(Clone, Copy)]
struct SvgRootTag {
    attrs_start: usize,
    insert_before: usize,
}

#[derive(Clone, Copy)]
struct SvgAttribute {
    value_start: usize,
    value_end: usize,
    full_end: usize,
}

#[derive(Clone, Copy, Default)]
struct SvgRootAttributes {
    width: Option<SvgAttribute>,
    height: Option<SvgAttribute>,
    view_box: Option<SvgAttribute>,
}

#[derive(Clone, Copy)]
struct ViewBoxDimensions<'a> {
    width: &'a [u8],
    height: &'a [u8],
}

struct SvgEdit {
    start: usize,
    end: usize,
    replacement: Vec<u8>,
}

fn find_root_svg_tag(bytes: &[u8]) -> Option<SvgRootTag> {
    let mut pos = 0;
    if bytes.starts_with(b"\xef\xbb\xbf") {
        pos = b"\xef\xbb\xbf".len();
    }
    pos = skip_ascii_whitespace(bytes, pos);

    loop {
        pos = skip_ascii_whitespace(bytes, pos);
        if pos >= bytes.len() {
            return None;
        }

        if starts_with_ascii_case_insensitive(&bytes[pos..], b"<?xml") {
            let end = find_subsequence(bytes, pos + b"<?xml".len(), b"?>")?;
            pos = end + b"?>".len();
            continue;
        }

        if bytes[pos..].starts_with(b"<!--") {
            let end = find_subsequence(bytes, pos + b"<!--".len(), b"-->")?;
            pos = end + b"-->".len();
            continue;
        }

        if starts_with_ascii_case_insensitive(&bytes[pos..], b"<!doctype") {
            let end = find_doctype_end(bytes, pos + b"<!doctype".len())?;
            pos = end + 1;
            continue;
        }

        if is_svg_start_tag_at(bytes, pos) {
            let end = find_tag_like_end(bytes, pos + b"<svg".len())?;
            return Some(SvgRootTag {
                attrs_start: pos + b"<svg".len(),
                insert_before: svg_root_insert_before(bytes, pos, end),
            });
        }

        return None;
    }
}

fn parse_svg_root_attributes(bytes: &[u8], root: SvgRootTag) -> SvgRootAttributes {
    let mut attrs = SvgRootAttributes::default();
    let mut pos = root.attrs_start;
    let end = root.insert_before;

    while pos < end {
        pos = skip_ascii_whitespace_until(bytes, pos, end);
        if pos >= end || bytes[pos] == b'/' {
            break;
        }

        let name_start = pos;
        while pos < end && !is_svg_attribute_name_delimiter(bytes[pos]) {
            pos += 1;
        }
        let name_end = pos;
        if name_start == name_end {
            pos += 1;
            continue;
        }

        pos = skip_ascii_whitespace_until(bytes, pos, end);
        if pos >= end || bytes[pos] != b'=' {
            continue;
        }
        pos += 1;
        pos = skip_ascii_whitespace_until(bytes, pos, end);
        if pos >= end {
            break;
        }

        let quote = bytes[pos];
        let value_start;
        let value_end;
        if quote == b'\'' || quote == b'"' {
            pos += 1;
            value_start = pos;
            while pos < end && bytes[pos] != quote {
                pos += 1;
            }
            if pos >= end {
                break;
            }
            value_end = pos;
            pos += 1;
        } else {
            value_start = pos;
            while pos < end && !bytes[pos].is_ascii_whitespace() {
                pos += 1;
            }
            value_end = pos;
        }

        let attr = SvgAttribute {
            value_start,
            value_end,
            full_end: pos,
        };
        let name = &bytes[name_start..name_end];
        if name == b"width" {
            attrs.width.get_or_insert(attr);
        } else if name == b"height" {
            attrs.height.get_or_insert(attr);
        } else if name == b"viewBox" {
            attrs.view_box.get_or_insert(attr);
        }
    }

    attrs
}

fn apply_dimension_edits(
    bytes: &[u8],
    root: SvgRootTag,
    attrs: SvgRootAttributes,
    view_box: ViewBoxDimensions<'_>,
) -> Vec<u8> {
    let mut edits = Vec::new();

    if let Some(width) = attrs.width {
        edits.push(replace_attr_value(width, view_box.width));
    }
    if let Some(height) = attrs.height {
        edits.push(replace_attr_value(height, view_box.height));
    }

    match (attrs.width, attrs.height) {
        (None, None) => edits.push(insert_dimensions(
            root.insert_before,
            view_box.width,
            view_box.height,
        )),
        (Some(width), None) => {
            edits.push(insert_dimension(width.full_end, b"height", view_box.height))
        }
        (None, Some(height)) => {
            edits.push(insert_dimension(height.full_end, b"width", view_box.width))
        }
        (Some(_), Some(_)) => {}
    }

    apply_svg_edits(bytes, edits)
}

fn replace_attr_value(attr: SvgAttribute, replacement: &[u8]) -> SvgEdit {
    SvgEdit {
        start: attr.value_start,
        end: attr.value_end,
        replacement: replacement.to_vec(),
    }
}

fn insert_dimension(start: usize, name: &[u8], value: &[u8]) -> SvgEdit {
    let mut replacement = Vec::with_capacity(name.len() + value.len() + 5);
    replacement.push(b' ');
    replacement.extend_from_slice(name);
    replacement.push(b'=');
    replacement.push(b'"');
    replacement.extend_from_slice(value);
    replacement.push(b'"');
    SvgEdit {
        start,
        end: start,
        replacement,
    }
}

fn insert_dimensions(start: usize, width: &[u8], height: &[u8]) -> SvgEdit {
    let mut replacement = Vec::with_capacity(width.len() + height.len() + 18);
    replacement.extend_from_slice(b" width=\"");
    replacement.extend_from_slice(width);
    replacement.extend_from_slice(b"\" height=\"");
    replacement.extend_from_slice(height);
    replacement.push(b'"');
    SvgEdit {
        start,
        end: start,
        replacement,
    }
}

fn apply_svg_edits(bytes: &[u8], mut edits: Vec<SvgEdit>) -> Vec<u8> {
    edits.sort_by_key(|edit| (edit.start, edit.end));
    let replacement_len = edits
        .iter()
        .map(|edit| edit.replacement.len())
        .sum::<usize>();
    let removed_len = edits
        .iter()
        .map(|edit| edit.end - edit.start)
        .sum::<usize>();
    let mut out = Vec::with_capacity(bytes.len() + replacement_len.saturating_sub(removed_len));
    let mut copied_until = 0;

    for edit in edits {
        out.extend_from_slice(&bytes[copied_until..edit.start]);
        out.extend_from_slice(&edit.replacement);
        copied_until = edit.end;
    }

    out.extend_from_slice(&bytes[copied_until..]);
    out
}

fn parse_view_box_dimensions(value: &[u8]) -> Option<ViewBoxDimensions<'_>> {
    let mut tokens = Vec::with_capacity(4);
    let mut pos = 0;
    while pos < value.len() {
        while pos < value.len() && (value[pos].is_ascii_whitespace() || value[pos] == b',') {
            pos += 1;
        }
        if pos >= value.len() {
            break;
        }
        let start = pos;
        while pos < value.len() && !value[pos].is_ascii_whitespace() && value[pos] != b',' {
            pos += 1;
        }
        tokens.push(&value[start..pos]);
    }

    if tokens.len() != 4 {
        return None;
    }

    let numbers = tokens
        .iter()
        .map(|token| std::str::from_utf8(token).ok()?.parse::<f64>().ok())
        .collect::<Option<Vec<_>>>()?;
    if numbers.iter().any(|number| !number.is_finite()) {
        return None;
    }
    if numbers[2] <= 0.0 || numbers[3] <= 0.0 {
        return None;
    }

    Some(ViewBoxDimensions {
        width: tokens[2],
        height: tokens[3],
    })
}

fn is_usable_svg_length(value: &[u8]) -> bool {
    let Ok(value) = std::str::from_utf8(value) else {
        return false;
    };
    let value = value.trim();
    let Some(number_end) = svg_number_prefix_len(value) else {
        return false;
    };
    let Ok(number) = value[..number_end].parse::<f64>() else {
        return false;
    };
    if !number.is_finite() || number <= 0.0 {
        return false;
    }

    let unit = value[number_end..].trim();
    if unit.contains('%') {
        return false;
    }
    matches!(
        unit,
        "" | "px" | "pt" | "pc" | "mm" | "cm" | "in" | "em" | "ex"
    )
}

fn svg_number_prefix_len(value: &str) -> Option<usize> {
    let bytes = value.as_bytes();
    let mut pos = 0;
    if matches!(bytes.get(pos), Some(b'+' | b'-')) {
        pos += 1;
    }

    let digits_start = pos;
    while matches!(bytes.get(pos), Some(b'0'..=b'9')) {
        pos += 1;
    }
    let digits_before_decimal = pos - digits_start;

    let mut digits_after_decimal = 0;
    if matches!(bytes.get(pos), Some(b'.')) {
        pos += 1;
        let decimal_start = pos;
        while matches!(bytes.get(pos), Some(b'0'..=b'9')) {
            pos += 1;
        }
        digits_after_decimal = pos - decimal_start;
    }

    if digits_before_decimal == 0 && digits_after_decimal == 0 {
        return None;
    }

    if matches!(bytes.get(pos), Some(b'e' | b'E')) {
        let exponent_start = pos;
        let mut exponent_pos = pos + 1;
        if matches!(bytes.get(exponent_pos), Some(b'+' | b'-')) {
            exponent_pos += 1;
        }
        let exponent_digits_start = exponent_pos;
        while matches!(bytes.get(exponent_pos), Some(b'0'..=b'9')) {
            exponent_pos += 1;
        }
        if exponent_pos > exponent_digits_start {
            pos = exponent_pos;
        } else {
            pos = exponent_start;
        }
    }

    Some(pos)
}

fn is_svg_start_tag_at(bytes: &[u8], pos: usize) -> bool {
    let significant = &bytes[pos..];
    significant.len() >= b"<svg".len()
        && significant[..b"<svg".len()].eq_ignore_ascii_case(b"<svg")
        && significant
            .get(b"<svg".len())
            .is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b'>' || *byte == b'/')
}

fn find_tag_like_end(bytes: &[u8], mut pos: usize) -> Option<usize> {
    let mut quote = None;
    while pos < bytes.len() {
        let byte = bytes[pos];
        if let Some(quote_byte) = quote {
            if byte == quote_byte {
                quote = None;
            }
        } else if byte == b'\'' || byte == b'"' {
            quote = Some(byte);
        } else if byte == b'>' {
            return Some(pos);
        }
        pos += 1;
    }
    None
}

fn find_doctype_end(bytes: &[u8], mut pos: usize) -> Option<usize> {
    let mut quote = None;
    while pos < bytes.len() {
        let byte = bytes[pos];
        if let Some(quote_byte) = quote {
            if byte == quote_byte {
                quote = None;
            }
        } else if byte == b'\'' || byte == b'"' {
            quote = Some(byte);
        } else if byte == b'[' {
            pos = find_doctype_internal_subset_end(bytes, pos + 1)? + 1;
            continue;
        } else if byte == b'>' {
            return Some(pos);
        }
        pos += 1;
    }
    None
}

fn find_doctype_internal_subset_end(bytes: &[u8], mut pos: usize) -> Option<usize> {
    let mut quote = None;
    while pos < bytes.len() {
        let byte = bytes[pos];
        if let Some(quote_byte) = quote {
            if byte == quote_byte {
                quote = None;
            }
        } else if byte == b'\'' || byte == b'"' {
            quote = Some(byte);
        } else if byte == b']' {
            return Some(pos);
        }
        pos += 1;
    }
    None
}

fn svg_root_insert_before(bytes: &[u8], root_start: usize, tag_end: usize) -> usize {
    let mut insert_before = tag_end;
    while insert_before > root_start && bytes[insert_before - 1].is_ascii_whitespace() {
        insert_before -= 1;
    }
    if insert_before > root_start && bytes[insert_before - 1] == b'/' {
        insert_before -= 1;
        while insert_before > root_start && bytes[insert_before - 1].is_ascii_whitespace() {
            insert_before -= 1;
        }
    }
    insert_before
}

fn is_svg_attribute_name_delimiter(byte: u8) -> bool {
    byte.is_ascii_whitespace() || byte == b'=' || byte == b'/' || byte == b'>'
}

fn skip_ascii_whitespace(bytes: &[u8], mut pos: usize) -> usize {
    while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
        pos += 1;
    }
    pos
}

fn skip_ascii_whitespace_until(bytes: &[u8], mut pos: usize, end: usize) -> usize {
    while pos < end && bytes[pos].is_ascii_whitespace() {
        pos += 1;
    }
    pos
}

fn find_subsequence(bytes: &[u8], start: usize, needle: &[u8]) -> Option<usize> {
    bytes[start..]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|offset| start + offset)
}

fn svg_empty_output_error(line: usize, tag: &str, context: CodeImageOutputContext) -> BuildError {
    match context {
        CodeImageOutputContext::ExternalCommand => code_image_error(
            line,
            tag,
            "command wrote empty stdout",
            format!("make code_images.{tag} write an SVG document to stdout"),
        ),
        CodeImageOutputContext::BuiltinMermaid => code_image_error(
            line,
            tag,
            "built-in renderer produced empty SVG output",
            builtin_mermaid_override_help(),
        ),
    }
}

fn svg_not_document_error(line: usize, tag: &str, context: CodeImageOutputContext) -> BuildError {
    match context {
        CodeImageOutputContext::ExternalCommand => code_image_error(
            line,
            tag,
            "command stdout is not an SVG document",
            format!("make code_images.{tag} write an SVG document to stdout"),
        ),
        CodeImageOutputContext::BuiltinMermaid => code_image_error(
            line,
            tag,
            "built-in renderer output is not an SVG document",
            builtin_mermaid_override_help(),
        ),
    }
}

fn svg_intrinsic_size_error(line: usize, tag: &str, context: CodeImageOutputContext) -> BuildError {
    match context {
        CodeImageOutputContext::ExternalCommand => code_image_error(
            line,
            tag,
            "command's SVG has no usable intrinsic size (no absolute width/height and no viewBox)",
            format!(
                "make code_images.{tag} emit an SVG with a viewBox (width/height are derived from it) or absolute width/height attributes"
            ),
        ),
        CodeImageOutputContext::BuiltinMermaid => code_image_error(
            line,
            tag,
            "built-in renderer's SVG has no usable intrinsic size (no absolute width/height and no viewBox)",
            builtin_mermaid_override_help(),
        ),
    }
}

fn svg_root_not_found_error(line: usize, tag: &str, context: CodeImageOutputContext) -> BuildError {
    match context {
        CodeImageOutputContext::ExternalCommand => code_image_error(
            line,
            tag,
            "could not locate the root <svg> element in the command's SVG output",
            format!("make code_images.{tag} write a standalone SVG document to stdout"),
        ),
        CodeImageOutputContext::BuiltinMermaid => code_image_error(
            line,
            tag,
            "could not locate the root <svg> element in the built-in renderer's SVG output",
            builtin_mermaid_override_help(),
        ),
    }
}

fn is_svg_output(bytes: &[u8]) -> bool {
    const SVG_SCAN_LIMIT: usize = 1024;

    let scan = &bytes[..bytes.len().min(SVG_SCAN_LIMIT)];
    let Some(first_token) = scan.iter().position(|byte| !byte.is_ascii_whitespace()) else {
        return false;
    };
    let significant = &scan[first_token..];
    if starts_with_ascii_case_insensitive(significant, b"<html")
        || starts_with_ascii_case_insensitive(significant, b"<!doctype html")
    {
        return false;
    }

    significant
        .windows(b"<svg".len())
        .any(|window| window == b"<svg")
}

fn starts_with_ascii_case_insensitive(bytes: &[u8], prefix: &[u8]) -> bool {
    bytes.len() >= prefix.len() && bytes[..prefix.len()].eq_ignore_ascii_case(prefix)
}

fn code_image_error(
    line: usize,
    tag: &str,
    message: impl Into<String>,
    help: impl Into<String>,
) -> BuildError {
    BuildError::new(
        ErrorKind::Asset,
        Some(line),
        format!("code_images '{tag}' failed: {}", message.into()),
        help,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        builtin_mermaid_cache_key, builtin_mermaid_override_help, code_image_cache_key, hex_encode,
        is_svg_output, render_builtin_mermaid_with, svg_empty_output_error,
        svg_has_usable_intrinsic_size, svg_intrinsic_size_error, svg_not_document_error,
        svg_root_not_found_error, transform_code_images, CodeImageOutputContext, SvgRunner,
    };
    use crate::error::ErrorKind;
    use crate::{
        check::check_deck,
        domain::{
            CodeImageCommand, CodeImagesConfig, FragmentKind, ResolvedImageAsset,
            ResolvedImagePath, SourceFragment,
        },
        layout::{parse_layout, Layouts},
        mapping::dispatch_by_convention,
        parser::{parse_frontmatter, parse_markdown},
        phase::{resolve_image_paths, Deck, DeckSettings, KeySource, Parsed, ParsedSlide},
        BuildError, Result,
    };
    use sha2::{Digest, Sha256};
    use std::{cell::Cell, collections::BTreeMap, fs, path::PathBuf};

    const MERMAID_KEY: &str = "4dba32c8d19de69fc2671719f51c327b802adf382763f36d20c1bffd972745f1";

    struct FakeRunner {
        calls: Cell<usize>,
        result: Result<Vec<u8>>,
    }

    impl FakeRunner {
        fn svg(output: impl Into<Vec<u8>>) -> Self {
            Self {
                calls: Cell::new(0),
                result: Ok(output.into()),
            }
        }

        fn err(message: &str) -> Self {
            Self {
                calls: Cell::new(0),
                result: Err(BuildError::new(
                    ErrorKind::Asset,
                    None,
                    message,
                    "check the code_images command",
                )),
            }
        }
    }

    impl SvgRunner for FakeRunner {
        fn run(&self, _command: &CodeImageCommand, _stdin: &str) -> Result<Vec<u8>> {
            self.calls.set(self.calls.get() + 1);
            self.result.clone()
        }
    }

    fn config() -> CodeImagesConfig {
        CodeImagesConfig {
            entries: BTreeMap::from([(
                "mermaid".to_owned(),
                CodeImageCommand {
                    argv: vec!["mmdc".to_owned(), "-i".to_owned(), "-".to_owned()],
                },
            )]),
            key_line: Some(2),
        }
    }

    fn deck_with_mermaid(code: &str) -> Deck<Parsed> {
        Deck::parsed(
            DeckSettings::default(),
            vec![ParsedSlide {
                index: 0,
                source_index: 0,
                key: crate::domain::SlideKey::new("intro").unwrap(),
                key_source: KeySource::Derived { line: Some(1) },
                layout_request: None,
                fragments: vec![SourceFragment::code(
                    7,
                    Some("mermaid".to_owned()),
                    code.to_owned(),
                )],
                skip: false,
                notes: None,
            }],
        )
    }

    fn normalize_runner_output(input: impl Into<Vec<u8>>) -> Vec<u8> {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        let runner = FakeRunner::svg(input);

        transform_code_images(
            deck_with_mermaid("graph TD"),
            &config(),
            &runner,
            &cache_dir,
        )
        .unwrap();

        assert_eq!(runner.calls.get(), 1);
        fs::read(cache_dir.join(format!("{MERMAID_KEY}.svg"))).unwrap()
    }

    #[test]
    fn transforms_matching_code_block_to_cached_image() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        let runner = FakeRunner::svg(r#"<svg viewBox="0 0 10 10">diagram</svg>"#);

        let deck = transform_code_images(
            deck_with_mermaid("graph TD"),
            &config(),
            &runner,
            &cache_dir,
        )
        .unwrap();
        let fragment = &deck.parsed_slides()[0].fragments[0];

        assert_eq!(runner.calls.get(), 1);
        assert_eq!(fragment.line(), 7);
        match fragment.kind() {
            FragmentKind::Image { alt, src } => {
                assert_eq!(alt, "diagram (mermaid)");
                assert_eq!(
                    src.as_str(),
                    format!("{}/{MERMAID_KEY}.svg", crate::CODE_IMAGES_CACHE_DIR)
                );
            }
            other => panic!("expected image fragment, got {other:?}"),
        }
        assert_eq!(
            fs::read(cache_dir.join(format!("{MERMAID_KEY}.svg"))).unwrap(),
            br#"<svg viewBox="0 0 10 10" width="10" height="10">diagram</svg>"#
        );
    }

    #[test]
    fn uses_valid_normalized_cache_hit_without_running_command() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        fs::create_dir_all(&cache_dir).unwrap();
        fs::write(
            cache_dir.join(format!("{MERMAID_KEY}.svg")),
            br#"<svg width="10" height="10" viewBox="0 0 10 10">cached</svg>"#,
        )
        .unwrap();
        let runner = FakeRunner::svg(r#"<svg width="1" height="1">new</svg>"#);

        let deck = transform_code_images(
            deck_with_mermaid("graph TD"),
            &config(),
            &runner,
            &cache_dir,
        )
        .unwrap();
        let fragment = &deck.parsed_slides()[0].fragments[0];

        assert_eq!(runner.calls.get(), 0);
        match fragment.kind() {
            FragmentKind::Image { src, .. } => {
                assert_eq!(
                    src.as_str(),
                    format!("{}/{MERMAID_KEY}.svg", crate::CODE_IMAGES_CACHE_DIR)
                );
            }
            other => panic!("expected image fragment, got {other:?}"),
        }
        assert_eq!(
            fs::read(cache_dir.join(format!("{MERMAID_KEY}.svg"))).unwrap(),
            br#"<svg width="10" height="10" viewBox="0 0 10 10">cached</svg>"#
        );
    }

    #[test]
    fn corrupt_cache_hit_is_replaced_by_runner_output() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        fs::create_dir_all(&cache_dir).unwrap();
        fs::write(cache_dir.join(format!("{MERMAID_KEY}.svg")), b"not svg").unwrap();
        let runner = FakeRunner::svg(r#"<svg viewBox="0 0 10 10">new</svg>"#);

        transform_code_images(
            deck_with_mermaid("graph TD"),
            &config(),
            &runner,
            &cache_dir,
        )
        .unwrap();

        assert_eq!(runner.calls.get(), 1);
        assert_eq!(
            fs::read(cache_dir.join(format!("{MERMAID_KEY}.svg"))).unwrap(),
            br#"<svg viewBox="0 0 10 10" width="10" height="10">new</svg>"#
        );
    }

    #[test]
    fn builtin_mermaid_cache_key_uses_discriminator_version_and_code() {
        let code = "graph TD";
        let expected_input = format!(
            "\0peitho-builtin-mermaid\0{}\0{}",
            env!("CARGO_PKG_VERSION"),
            code
        );
        let mut hasher = Sha256::new();
        hasher.update(expected_input.as_bytes());
        let expected = hex_encode(&hasher.finalize());
        let external = code_image_cache_key(config().entries.get("mermaid").unwrap(), code);

        assert_eq!(builtin_mermaid_cache_key(code), expected);
        assert_ne!(
            builtin_mermaid_cache_key(code),
            builtin_mermaid_cache_key("graph TD\n  A-->B\n")
        );
        assert_ne!(builtin_mermaid_cache_key(code), external);
    }

    #[test]
    fn builtin_mermaid_uses_valid_cache_hit_without_rewriting() {
        let code = "graph TD\n  A-->B\n";
        let key = builtin_mermaid_cache_key(code);
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        let cache_path = cache_dir.join(format!("{key}.svg"));
        fs::create_dir_all(&cache_dir).unwrap();
        fs::write(
            &cache_path,
            br#"<svg width="10" height="10" viewBox="0 0 10 10">cached builtin</svg>"#,
        )
        .unwrap();
        let runner = FakeRunner::svg(r#"<svg viewBox="0 0 1 1">external</svg>"#);

        let deck = transform_code_images(
            deck_with_mermaid(code),
            &CodeImagesConfig::default(),
            &runner,
            &cache_dir,
        )
        .unwrap();

        assert_eq!(runner.calls.get(), 0);
        match deck.parsed_slides()[0].fragments[0].kind() {
            FragmentKind::Image { src, .. } => {
                assert_eq!(
                    src.as_str(),
                    format!("{}/{key}.svg", crate::CODE_IMAGES_CACHE_DIR)
                );
            }
            other => panic!("expected image fragment, got {other:?}"),
        }
        assert_eq!(fs::read_dir(&cache_dir).unwrap().count(), 1);
        assert_eq!(
            fs::read(cache_path).unwrap(),
            br#"<svg width="10" height="10" viewBox="0 0 10 10">cached builtin</svg>"#
        );
    }

    #[test]
    fn transforms_bare_mermaid_with_builtin_renderer_without_running_command() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        let runner = FakeRunner::svg(r#"<svg viewBox="0 0 1 1">external</svg>"#);

        let deck = transform_code_images(
            deck_with_mermaid("graph TD\n  A-->B\n"),
            &CodeImagesConfig::default(),
            &runner,
            &cache_dir,
        )
        .unwrap();
        let fragment = &deck.parsed_slides()[0].fragments[0];

        assert_eq!(runner.calls.get(), 0);
        match fragment.kind() {
            FragmentKind::Image { alt, src } => {
                assert_eq!(alt, "diagram (mermaid)");
                assert!(src.as_str().starts_with(crate::CODE_IMAGES_CACHE_DIR));
            }
            other => panic!("expected image fragment, got {other:?}"),
        }

        let cache_files = fs::read_dir(&cache_dir).unwrap().collect::<Vec<_>>();
        assert_eq!(cache_files.len(), 1);
        let bytes = fs::read(cache_files[0].as_ref().unwrap().path()).unwrap();
        assert!(is_svg_output(&bytes));
        assert!(svg_has_usable_intrinsic_size(&bytes));
    }

    #[test]
    fn explicit_mermaid_entry_overrides_builtin_renderer() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        let runner = FakeRunner::svg(r#"<svg viewBox="0 0 10 10">external override</svg>"#);

        transform_code_images(
            deck_with_mermaid("graph TD"),
            &config(),
            &runner,
            &cache_dir,
        )
        .unwrap();

        assert_eq!(runner.calls.get(), 1);
        assert_eq!(
            fs::read(cache_dir.join(format!("{MERMAID_KEY}.svg"))).unwrap(),
            br#"<svg viewBox="0 0 10 10" width="10" height="10">external override</svg>"#
        );
        assert_eq!(fs::read_dir(&cache_dir).unwrap().count(), 1);
    }

    #[test]
    fn builtin_mermaid_render_error_reports_line_and_override_help() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        let runner = FakeRunner::svg(r#"<svg viewBox="0 0 1 1">external</svg>"#);

        let err = match transform_code_images(
            deck_with_mermaid("flowchart TD\n  A[unterminated\n"),
            &CodeImagesConfig::default(),
            &runner,
            &cache_dir,
        ) {
            Ok(_) => panic!("expected built-in mermaid render failure"),
            Err(err) => err,
        };

        assert_eq!(runner.calls.get(), 0);
        assert_eq!(err.kind, ErrorKind::Asset);
        assert_eq!(err.line, Some(7));
        assert!(err.message.contains("code_images 'mermaid' failed"));
        assert!(err.message.contains("Unterminated node label"));
        assert_eq!(err.help, builtin_mermaid_override_help());
    }

    #[test]
    fn builtin_mermaid_non_diagram_reports_line_and_override_help() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        let runner = FakeRunner::svg(r#"<svg viewBox="0 0 1 1">external</svg>"#);

        let err = match transform_code_images(
            deck_with_mermaid("this is not a diagram"),
            &CodeImagesConfig::default(),
            &runner,
            &cache_dir,
        ) {
            Ok(_) => panic!("expected built-in mermaid non-diagram failure"),
            Err(err) => err,
        };

        assert_eq!(runner.calls.get(), 0);
        assert_eq!(err.kind, ErrorKind::Asset);
        assert_eq!(err.line, Some(7));
        assert_eq!(
            err.message,
            "code_images 'mermaid' failed: built-in renderer did not detect a mermaid diagram"
        );
        assert_eq!(err.help, builtin_mermaid_override_help());
    }

    #[test]
    fn builtin_mermaid_renderer_panic_becomes_error_message() {
        let err = render_builtin_mermaid_with(
            || -> std::result::Result<Option<String>, merman::render::HeadlessError> {
                panic!("boom");
            },
        )
        .unwrap_err();

        assert_eq!(err, "built-in mermaid renderer panicked: boom");
    }

    #[test]
    fn builtin_svg_output_errors_use_builtin_renderer_context() {
        let empty = svg_empty_output_error(7, "mermaid", CodeImageOutputContext::BuiltinMermaid);
        assert_eq!(
            empty.message,
            "code_images 'mermaid' failed: built-in renderer produced empty SVG output"
        );
        assert_eq!(empty.help, builtin_mermaid_override_help());

        let not_svg = svg_not_document_error(7, "mermaid", CodeImageOutputContext::BuiltinMermaid);
        assert_eq!(
            not_svg.message,
            "code_images 'mermaid' failed: built-in renderer output is not an SVG document"
        );
        assert_eq!(not_svg.help, builtin_mermaid_override_help());

        let no_root =
            svg_root_not_found_error(7, "mermaid", CodeImageOutputContext::BuiltinMermaid);
        assert_eq!(
            no_root.message,
            "code_images 'mermaid' failed: could not locate the root <svg> element in the built-in renderer's SVG output"
        );
        assert_eq!(no_root.help, builtin_mermaid_override_help());

        let no_size =
            svg_intrinsic_size_error(7, "mermaid", CodeImageOutputContext::BuiltinMermaid);
        assert_eq!(
            no_size.message,
            "code_images 'mermaid' failed: built-in renderer's SVG has no usable intrinsic size (no absolute width/height and no viewBox)"
        );
        assert_eq!(no_size.help, builtin_mermaid_override_help());
    }

    #[test]
    fn external_svg_output_errors_keep_command_context() {
        let empty = svg_empty_output_error(7, "mermaid", CodeImageOutputContext::ExternalCommand);
        assert_eq!(
            empty.message,
            "code_images 'mermaid' failed: command wrote empty stdout"
        );
        assert_eq!(
            empty.help,
            "make code_images.mermaid write an SVG document to stdout"
        );

        let not_svg = svg_not_document_error(7, "mermaid", CodeImageOutputContext::ExternalCommand);
        assert_eq!(
            not_svg.message,
            "code_images 'mermaid' failed: command stdout is not an SVG document"
        );
        assert_eq!(
            not_svg.help,
            "make code_images.mermaid write an SVG document to stdout"
        );

        let no_root =
            svg_root_not_found_error(7, "mermaid", CodeImageOutputContext::ExternalCommand);
        assert_eq!(
            no_root.message,
            "code_images 'mermaid' failed: could not locate the root <svg> element in the command's SVG output"
        );
        assert_eq!(
            no_root.help,
            "make code_images.mermaid write a standalone SVG document to stdout"
        );

        let no_size =
            svg_intrinsic_size_error(7, "mermaid", CodeImageOutputContext::ExternalCommand);
        assert_eq!(
            no_size.message,
            "code_images 'mermaid' failed: command's SVG has no usable intrinsic size (no absolute width/height and no viewBox)"
        );
        assert_eq!(
            no_size.help,
            "make code_images.mermaid emit an SVG with a viewBox (width/height are derived from it) or absolute width/height attributes"
        );
    }

    #[test]
    fn normalizes_mermaid_style_svg_from_viewbox_dimensions() {
        let input = br#"<svg id="my-svg" width="100%" xmlns="http://www.w3.org/2000/svg" style="max-width: 524.594px;" viewBox="0 0 524.59375 70" role="graphics-document document"><g>diagram</g></svg>"#;
        let expected = br#"<svg id="my-svg" width="524.59375" height="70" xmlns="http://www.w3.org/2000/svg" style="max-width: 524.594px;" viewBox="0 0 524.59375 70" role="graphics-document document"><g>diagram</g></svg>"#;

        assert_eq!(normalize_runner_output(input.to_vec()), expected);
    }

    #[test]
    fn normalizes_bom_prefixed_svg_and_preserves_bom_bytes() {
        let input = b"\xef\xbb\xbf<svg width=\"100%\" viewBox=\"0 0 10 10\"></svg>";
        let expected = b"\xef\xbb\xbf<svg width=\"10\" height=\"10\" viewBox=\"0 0 10 10\"></svg>";

        assert_eq!(normalize_runner_output(input.to_vec()), expected);
    }

    #[test]
    fn rejects_stray_prefix_before_root_svg_with_root_not_found_error() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        let runner = FakeRunner::svg(r#"junk<svg viewBox="0 0 10 10"></svg>"#);

        let err = match transform_code_images(
            deck_with_mermaid("graph TD"),
            &config(),
            &runner,
            &cache_dir,
        ) {
            Ok(_) => panic!("expected root-not-found failure"),
            Err(err) => err,
        };

        assert_eq!(runner.calls.get(), 1);
        assert_eq!(err.kind, ErrorKind::Asset);
        assert_eq!(err.line, Some(7));
        assert_eq!(
            err.message,
            "code_images 'mermaid' failed: could not locate the root <svg> element in the command's SVG output"
        );
        assert_eq!(
            err.help,
            "make code_images.mermaid write a standalone SVG document to stdout"
        );
        assert!(!cache_dir.join(format!("{MERMAID_KEY}.svg")).exists());
    }

    #[test]
    fn graphviz_svg_with_intrinsic_size_passes_through_byte_identical() {
        let graphviz_svg = b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>\n\
            <!-- Generated by graphviz version 12.0.0. Comment mentions <svg but is not the root. -->\n\
            <!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \
            \"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\">\n\
            <svg width=\"181pt\" height=\"293pt\" viewBox=\"0.00 0.00 181.00 293.00\" \
            xmlns=\"http://www.w3.org/2000/svg\"></svg>";

        assert_eq!(normalize_runner_output(graphviz_svg.to_vec()), graphviz_svg);
    }

    #[test]
    fn graphviz_svg_with_doctype_internal_subset_passes_through_byte_identical() {
        let graphviz_svg = b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>\n\
            <!DOCTYPE svg [<!ENTITY a \"quoted > value\"><!ENTITY b \"c\">]>\n\
            <svg width=\"181pt\" height=\"293pt\" xmlns=\"http://www.w3.org/2000/svg\"></svg>";

        assert_eq!(normalize_runner_output(graphviz_svg.to_vec()), graphviz_svg);
    }

    #[test]
    fn normalizes_missing_or_unusable_dimensions_from_viewbox() {
        let cases: &[(&[u8], &[u8])] = &[
            (
                br#"<svg height="12.5px" viewBox="0 0 20 30"></svg>"#,
                br#"<svg height="30" width="20" viewBox="0 0 20 30"></svg>"#,
            ),
            (
                br#"<svg width="50pt" viewBox="0 0 40 60"></svg>"#,
                br#"<svg width="40" height="60" viewBox="0 0 40 60"></svg>"#,
            ),
            (
                br#"<svg width="10" height="0%" viewBox="0 0 10 15"></svg>"#,
                br#"<svg width="10" height="15" viewBox="0 0 10 15"></svg>"#,
            ),
            (
                br#"<svg width="0" height="5" viewBox="0 0 7 5"></svg>"#,
                br#"<svg width="7" height="5" viewBox="0 0 7 5"></svg>"#,
            ),
        ];

        for (input, expected) in cases {
            assert_eq!(normalize_runner_output((*input).to_vec()), *expected);
        }
    }

    #[test]
    fn usable_dimension_next_to_unusable_one_is_also_replaced_from_viewbox() {
        // Keep intrinsic dimensions aligned with the viewBox aspect ratio.
        assert_eq!(
            normalize_runner_output(
                br#"<svg width="200" height="100%" viewBox="0 0 50 25"></svg>"#.to_vec()
            ),
            br#"<svg width="50" height="25" viewBox="0 0 50 25"></svg>"#
        );
    }

    #[test]
    fn normalizes_self_closing_root_tags_before_closing_slash() {
        let cases: &[(&[u8], &[u8])] = &[
            (
                br#"<svg viewBox="0 0 10 10"/>"#,
                br#"<svg viewBox="0 0 10 10" width="10" height="10"/>"#,
            ),
            (
                br#"<svg viewBox="0 0 10 10" />"#,
                br#"<svg viewBox="0 0 10 10" width="10" height="10" />"#,
            ),
        ];

        for (input, expected) in cases {
            assert_eq!(normalize_runner_output((*input).to_vec()), *expected);
        }
    }

    #[test]
    fn parses_single_quoted_svg_attributes() {
        assert_eq!(
            normalize_runner_output(br#"<svg width='100%' viewBox='0 0 42 24'></svg>"#.to_vec()),
            br#"<svg width='42' height="24" viewBox='0 0 42 24'></svg>"#
        );
    }

    #[test]
    fn usable_unquoted_width_and_height_pass_through_byte_identical() {
        let svg = br#"<svg width=100 height=50 viewBox="0 0 10 10"></svg>"#;

        assert_eq!(normalize_runner_output(svg.to_vec()), svg);
    }

    #[test]
    fn unusable_unquoted_width_is_replaced_without_duplicate_attribute() {
        assert_eq!(
            normalize_runner_output(br#"<svg width=100% viewBox="0 0 10 10"></svg>"#.to_vec()),
            br#"<svg width=10 height="10" viewBox="0 0 10 10"></svg>"#
        );
    }

    #[test]
    fn uppercase_width_and_height_do_not_satisfy_xml_svg_dimensions() {
        assert_eq!(
            normalize_runner_output(
                br#"<svg WIDTH="100" HEIGHT="50" viewBox="0 0 10 10"></svg>"#.to_vec()
            ),
            br#"<svg WIDTH="100" HEIGHT="50" viewBox="0 0 10 10" width="10" height="10"></svg>"#
        );
    }

    #[test]
    fn lowercase_viewbox_does_not_supply_svg_dimensions() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        let runner = FakeRunner::svg(r#"<svg width="100%" viewbox="0 0 10 10"></svg>"#);

        let err = match transform_code_images(
            deck_with_mermaid("graph TD"),
            &config(),
            &runner,
            &cache_dir,
        ) {
            Ok(_) => panic!("expected intrinsic size failure"),
            Err(err) => err,
        };

        assert_eq!(runner.calls.get(), 1);
        assert_eq!(err.kind, ErrorKind::Asset);
        assert_eq!(err.line, Some(7));
        assert_eq!(
            err.message,
            "code_images 'mermaid' failed: command's SVG has no usable intrinsic size (no absolute width/height and no viewBox)"
        );
        assert_eq!(
            err.help,
            "make code_images.mermaid emit an SVG with a viewBox (width/height are derived from it) or absolute width/height attributes"
        );
        assert!(!cache_dir.join(format!("{MERMAID_KEY}.svg")).exists());
    }

    #[test]
    fn scientific_notation_lengths_pass_through_byte_identical() {
        let svg = br#"<svg width="1e3px" height="0.5e2" viewBox="0 0 10 10"></svg>"#;

        assert_eq!(normalize_runner_output(svg.to_vec()), svg);
    }

    #[test]
    fn scientific_notation_viewbox_dimensions_converge_as_cache_hit() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        let first_runner = FakeRunner::svg(r#"<svg width="100%" viewBox="0 0 1e3 70"></svg>"#);

        transform_code_images(
            deck_with_mermaid("graph TD"),
            &config(),
            &first_runner,
            &cache_dir,
        )
        .unwrap();

        let cache_path = cache_dir.join(format!("{MERMAID_KEY}.svg"));
        assert_eq!(first_runner.calls.get(), 1);
        assert_eq!(
            fs::read(&cache_path).unwrap(),
            br#"<svg width="1e3" height="70" viewBox="0 0 1e3 70"></svg>"#
        );

        let second_runner = FakeRunner::svg(r#"<svg width="1" height="1"></svg>"#);
        transform_code_images(
            deck_with_mermaid("graph TD"),
            &config(),
            &second_runner,
            &cache_dir,
        )
        .unwrap();

        assert_eq!(second_runner.calls.get(), 0);
        assert_eq!(
            fs::read(cache_path).unwrap(),
            br#"<svg width="1e3" height="70" viewBox="0 0 1e3 70"></svg>"#
        );
    }

    #[test]
    fn ignores_svg_text_inside_comment_before_root_tag() {
        let input = b"<!-- This comment contains <svg width=\"100\" height=\"100\"></svg>. -->\n\
            <svg width=\"100%\" viewBox=\"0 0 10 10\"><g /></svg>";
        let expected =
            b"<!-- This comment contains <svg width=\"100\" height=\"100\"></svg>. -->\n\
            <svg width=\"10\" height=\"10\" viewBox=\"0 0 10 10\"><g /></svg>";

        assert_eq!(normalize_runner_output(input.to_vec()), expected);
    }

    #[test]
    fn rejects_svg_without_intrinsic_size_or_viewbox() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        let runner = FakeRunner::svg(r#"<svg width="100%"></svg>"#);

        let err = match transform_code_images(
            deck_with_mermaid("graph TD"),
            &config(),
            &runner,
            &cache_dir,
        ) {
            Ok(_) => panic!("expected missing intrinsic size failure"),
            Err(err) => err,
        };

        assert_eq!(runner.calls.get(), 1);
        assert_eq!(err.kind, ErrorKind::Asset);
        assert_eq!(err.line, Some(7));
        assert_eq!(
            err.message,
            "code_images 'mermaid' failed: command's SVG has no usable intrinsic size (no absolute width/height and no viewBox)"
        );
        assert_eq!(
            err.help,
            "make code_images.mermaid emit an SVG with a viewBox (width/height are derived from it) or absolute width/height attributes"
        );
        assert!(!cache_dir.join(format!("{MERMAID_KEY}.svg")).exists());
    }

    #[test]
    fn rejects_viewbox_with_non_positive_dimensions() {
        for svg in [
            r#"<svg viewBox="0 0 0 10"></svg>"#,
            r#"<svg viewBox="0 0 10 -1"></svg>"#,
        ] {
            let temp = tempfile::tempdir().unwrap();
            let cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
            let runner = FakeRunner::svg(svg);

            let err = match transform_code_images(
                deck_with_mermaid("graph TD"),
                &config(),
                &runner,
                &cache_dir,
            ) {
                Ok(_) => panic!("expected invalid viewBox failure"),
                Err(err) => err,
            };

            assert_eq!(runner.calls.get(), 1);
            assert_eq!(err.kind, ErrorKind::Asset);
            assert_eq!(err.line, Some(7));
            assert_eq!(
                err.message,
                "code_images 'mermaid' failed: command's SVG has no usable intrinsic size (no absolute width/height and no viewBox)"
            );
            assert_eq!(
                err.help,
                "make code_images.mermaid emit an SVG with a viewBox (width/height are derived from it) or absolute width/height attributes"
            );
            assert!(!cache_dir.join(format!("{MERMAID_KEY}.svg")).exists());
        }
    }

    #[test]
    fn unnormalized_cached_svg_is_miss_and_gets_rewritten() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        fs::create_dir_all(&cache_dir).unwrap();
        fs::write(
            cache_dir.join(format!("{MERMAID_KEY}.svg")),
            br#"<svg width="100%" viewBox="0 0 10 10">old</svg>"#,
        )
        .unwrap();
        let runner = FakeRunner::svg(r#"<svg width="100%" viewBox="0 0 10 10">new</svg>"#);

        transform_code_images(
            deck_with_mermaid("graph TD"),
            &config(),
            &runner,
            &cache_dir,
        )
        .unwrap();

        assert_eq!(runner.calls.get(), 1);
        assert_eq!(
            fs::read(cache_dir.join(format!("{MERMAID_KEY}.svg"))).unwrap(),
            br#"<svg width="10" height="10" viewBox="0 0 10 10">new</svg>"#
        );
    }

    #[test]
    fn already_normalized_cached_svg_is_hit() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        fs::create_dir_all(&cache_dir).unwrap();
        fs::write(
            cache_dir.join(format!("{MERMAID_KEY}.svg")),
            br#"<svg width="10" height="10" viewBox="0 0 10 10">cached</svg>"#,
        )
        .unwrap();
        let runner = FakeRunner::svg(r#"<svg width="1" height="1">new</svg>"#);

        transform_code_images(
            deck_with_mermaid("graph TD"),
            &config(),
            &runner,
            &cache_dir,
        )
        .unwrap();

        assert_eq!(runner.calls.get(), 0);
        assert_eq!(
            fs::read(cache_dir.join(format!("{MERMAID_KEY}.svg"))).unwrap(),
            br#"<svg width="10" height="10" viewBox="0 0 10 10">cached</svg>"#
        );
    }

    #[test]
    fn runner_failure_reports_code_block_line_and_stderr_excerpt() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        let runner = FakeRunner::err("command exited with status 1; stderr: boom");

        let err = match transform_code_images(
            deck_with_mermaid("graph TD"),
            &config(),
            &runner,
            &cache_dir,
        ) {
            Ok(_) => panic!("expected runner failure"),
            Err(err) => err,
        };

        assert_eq!(runner.calls.get(), 1);
        assert_eq!(err.kind, ErrorKind::Asset);
        assert_eq!(err.line, Some(7));
        assert!(err.message.contains("code_images 'mermaid' failed"));
        assert!(err.message.contains("boom"));
        assert_eq!(err.help, "check the code_images command");
    }

    #[test]
    fn empty_stdout_reports_code_block_line() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        let runner = FakeRunner::svg(Vec::new());

        let err = match transform_code_images(
            deck_with_mermaid("graph TD"),
            &config(),
            &runner,
            &cache_dir,
        ) {
            Ok(_) => panic!("expected empty stdout failure"),
            Err(err) => err,
        };

        assert_eq!(runner.calls.get(), 1);
        assert_eq!(err.kind, ErrorKind::Asset);
        assert_eq!(err.line, Some(7));
        assert_eq!(
            err.message,
            "code_images 'mermaid' failed: command wrote empty stdout"
        );
        assert_eq!(
            err.help,
            "make code_images.mermaid write an SVG document to stdout"
        );
        assert!(!cache_dir.join(format!("{MERMAID_KEY}.svg")).exists());
    }

    #[test]
    fn non_svg_stdout_reports_code_block_line() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        let runner = FakeRunner::svg("<html>not svg</html>");

        let err = match transform_code_images(
            deck_with_mermaid("graph TD"),
            &config(),
            &runner,
            &cache_dir,
        ) {
            Ok(_) => panic!("expected non-SVG stdout failure"),
            Err(err) => err,
        };

        assert_eq!(runner.calls.get(), 1);
        assert_eq!(err.kind, ErrorKind::Asset);
        assert_eq!(err.line, Some(7));
        assert_eq!(
            err.message,
            "code_images 'mermaid' failed: command stdout is not an SVG document"
        );
        assert_eq!(
            err.help,
            "make code_images.mermaid write an SVG document to stdout"
        );
        assert!(!cache_dir.join(format!("{MERMAID_KEY}.svg")).exists());
    }

    #[test]
    fn accepts_svg_with_graphviz_preamble() {
        let graphviz_svg = b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>\n\
            <!-- Generated by graphviz version 12.0.0 -->\n\
            <!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \
            \"http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd\">\n\
            <svg xmlns=\"http://www.w3.org/2000/svg\"></svg>";

        assert!(is_svg_output(graphviz_svg));
    }

    #[test]
    fn accepts_svg_with_xml_declaration() {
        assert!(is_svg_output(b"<?xml version=\"1.0\"?>\n<svg></svg>"));
    }

    #[test]
    fn accepts_bare_svg() {
        assert!(is_svg_output(b"<svg></svg>"));
    }

    #[test]
    fn rejects_html_page_with_embedded_svg() {
        assert!(!is_svg_output(
            b"<html><body><svg xmlns=\"http://www.w3.org/2000/svg\"></svg></body></html>"
        ));
    }

    #[test]
    fn rejects_whitespace_only_svg_output() {
        assert!(!is_svg_output(b" \n\t "));
    }

    #[test]
    fn rejects_text_without_svg_in_first_kib() {
        let mut output = vec![b'a'; 1024];
        output.extend_from_slice(b"<svg></svg>");

        assert!(!is_svg_output(&output));
    }

    #[test]
    fn code_images_entry_wins_over_known_syntect_language() {
        let markdown =
            "---\ncode_images:\n  json: json-to-svg\n---\n# Intro\n\n```json\n{\"ok\": true}\n```";
        let frontmatter = parse_frontmatter(markdown).unwrap();
        let config = frontmatter.settings().code_images().clone();
        let parsed = parse_markdown(
            markdown,
            frontmatter,
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();
        let temp = tempfile::tempdir().unwrap();
        let runner = FakeRunner::svg(r#"<svg viewBox="0 0 10 10">json</svg>"#);

        let deck = transform_code_images(
            parsed,
            &config,
            &runner,
            &temp.path().join(crate::CODE_IMAGES_CACHE_DIR),
        )
        .unwrap();
        let fragment = &deck.parsed_slides()[0].fragments[1];

        assert_eq!(runner.calls.get(), 1);
        match fragment.kind() {
            FragmentKind::Image { alt, .. } => {
                assert_eq!(alt, "diagram (json)");
            }
            other => panic!("expected image fragment, got {other:?}"),
        }
    }

    #[test]
    fn transforms_code_images_inside_slot_group() {
        let markdown = "---\ncode_images:\n  mermaid: mmdc -i -\n---\n# Intro\n\n::: {slot=main}\n\n```mermaid\ngraph TD\n```\n:::\n";
        let frontmatter = parse_frontmatter(markdown).unwrap();
        let config = frontmatter.settings().code_images().clone();
        let parsed = parse_markdown(
            markdown,
            frontmatter,
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();
        let temp = tempfile::tempdir().unwrap();
        let transformed = transform_code_images(
            parsed,
            &config,
            &FakeRunner::svg(r#"<svg viewBox="0 0 10 10">slot</svg>"#),
            &temp.path().join(crate::CODE_IMAGES_CACHE_DIR),
        )
        .unwrap();

        let slot_group = transformed.parsed_slides()[0]
            .fragments
            .iter()
            .find_map(|fragment| match fragment.kind() {
                FragmentKind::SlotGroup { name, children } => Some((name, children)),
                _ => None,
            })
            .expect("expected transformed slide to contain a slot group");

        assert_eq!(slot_group.0.as_slot_name().as_str(), "main");
        assert_eq!(slot_group.1.len(), 1);
        match slot_group.1[0].kind() {
            FragmentKind::Image { alt, .. } => assert_eq!(alt, "diagram (mermaid)"),
            other => panic!("expected slot group child image, got {other:?}"),
        }
    }

    #[test]
    fn duplicate_diagrams_share_one_cache_file_and_one_dist_asset() {
        let markdown = "---\ncode_images:\n  mermaid: mmdc -i -\n---\n# One\n\n```mermaid\ngraph TD\n```\n\n---\n# Two\n\n```mermaid\ngraph TD\n```";
        let frontmatter = parse_frontmatter(markdown).unwrap();
        let config = frontmatter.settings().code_images().clone();
        let parsed = parse_markdown(
            markdown,
            frontmatter,
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        let transformed = transform_code_images(
            parsed,
            &config,
            &FakeRunner::svg(r#"<svg viewBox="0 0 10 10">same</svg>"#),
            &cache_dir,
        )
        .unwrap();
        let layout = parse_layout(
            "title-image",
            r#"<section>
               <slot name="title" accepts="inline" arity="1"></slot>
               <slot name="image" accepts="image" arity="1"></slot>
               </section>"#,
        )
        .unwrap();
        let layouts = Layouts::new(vec![layout]).unwrap();
        let checked = check_deck(dispatch_by_convention(transformed, &layouts).unwrap()).unwrap();
        let dist_rel = ResolvedImagePath::from_string("assets/same.svg".to_owned());
        let mut resolve_calls = 0;

        let (_resolved, assets) = resolve_image_paths(checked, |_request| {
            resolve_calls += 1;
            Ok(ResolvedImageAsset {
                source_abs: PathBuf::from("/tmp/code-image.svg"),
                dist_rel: dist_rel.clone(),
            })
        })
        .unwrap();

        assert_eq!(fs::read_dir(&cache_dir).unwrap().count(), 1);
        assert_eq!(resolve_calls, 2);
        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].dist_rel.as_str(), "assets/same.svg");
    }
}
