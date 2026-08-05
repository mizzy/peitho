use std::{
    any::Any,
    borrow::Cow,
    fs::{self, OpenOptions},
    io::{self, Write},
    panic::{catch_unwind, AssertUnwindSafe},
    path::{Path, PathBuf},
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
    embed_card::{
        build_embed_card_html, builtin_embed_card_cache_key, hex_encode, parse_oembed_document,
        OEmbedDocument,
    },
    error::{BuildError, ErrorKind, Result},
    highlight::Highlighter,
    math::{KatexRenderer, MathOutput, MathRenderer},
    parser::{parse_markdown, ParsedFrontmatter},
    phase::{Deck, Parsed},
    MAX_OEMBED_RESPONSE_BYTES,
};

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);
static BUILTIN_MERMAID_RENDERER: LazyLock<merman::render::HeadlessRenderer> =
    LazyLock::new(merman::render::HeadlessRenderer::new);
const PNG_MAGIC: &[u8; 8] = b"\x89PNG\r\n\x1a\n";

pub trait SvgRunner {
    fn run(&self, command: &CodeImageCommand, stdin: &str) -> Result<Vec<u8>>;
}

pub trait EmbedRenderer {
    fn render(&self, normalized_url: &str, params: EmbedRenderParams) -> crate::Result<Vec<u8>>;
}

pub trait OEmbedFetcher {
    fn fetch(&self, normalized_url: &str) -> crate::Result<String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbedTheme {
    Light,
    Dark,
}

impl EmbedTheme {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmbedRenderParams {
    pub width_css_px: u32,
    pub scale_factor: u32,
    pub theme: EmbedTheme,
}

impl EmbedRenderParams {
    pub(crate) const fn new(width_css_px: u32, scale_factor: u32, theme: EmbedTheme) -> Self {
        Self {
            width_css_px,
            scale_factor,
            theme,
        }
    }
}

pub const BUILTIN_EMBED_PARAMS: EmbedRenderParams =
    EmbedRenderParams::new(550, 2, EmbedTheme::Light);

#[derive(Debug, Clone, PartialEq, Eq)]
enum EmbedTarget {
    X(TweetStatusUrl),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EmbedMode {
    Screenshot,
    Card,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedEmbedBlock {
    target: EmbedTarget,
    mode: EmbedMode,
}

impl EmbedTarget {
    fn normalized_url(&self) -> &str {
        match self {
            Self::X(status) => status.normalized_url(),
        }
    }

    fn user(&self) -> &str {
        match self {
            Self::X(status) => &status.user,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TweetStatusUrl {
    normalized_url: String,
    user: String,
    status_id: String,
}

impl TweetStatusUrl {
    fn normalized_url(&self) -> &str {
        &self.normalized_url
    }
}

fn parse_embed_block(line: usize, body: &str) -> Result<ParsedEmbedBlock> {
    let mut non_blank_lines = body
        .lines()
        .enumerate()
        .map(|(index, text)| (line + index + 1, text.trim()))
        .filter(|(_, text)| !text.is_empty());
    let Some((url_line, url)) = non_blank_lines.next() else {
        return Err(code_image_error(
            line,
            "embed",
            "embed block is empty",
            format!("{}; {}", embed_option_help(), embed_url_help()),
        ));
    };

    if let Some((key, _)) = url
        .split_once(':')
        .filter(|(_, value)| !value.trim_start().starts_with('/'))
    {
        return Err(embed_option_error(
            url_line,
            format!("option '{}' must follow the URL", key.trim()),
        ));
    }

    let target = parse_embed_url(line, url)?;
    let mut mode = EmbedMode::Screenshot;
    let mut mode_seen = false;
    for (option_line, option) in non_blank_lines {
        let Some((key, value)) = option.split_once(':') else {
            return Err(embed_option_error(
                option_line,
                "embed option must use 'key: value' syntax",
            ));
        };
        let key = key.trim();
        let value = value.trim();
        if key.is_empty() {
            return Err(embed_option_error(option_line, "embed option key is empty"));
        }
        if value.is_empty() {
            return Err(embed_option_error(
                option_line,
                "embed option value is empty",
            ));
        }
        if key != "mode" {
            return Err(embed_option_error(
                option_line,
                format!("unknown embed option '{key}'"),
            ));
        }
        if mode_seen {
            return Err(embed_option_error(
                option_line,
                "duplicate embed option 'mode'",
            ));
        }
        mode_seen = true;
        mode = match value {
            "card" => EmbedMode::Card,
            "screenshot" => EmbedMode::Screenshot,
            _ => {
                return Err(embed_option_error(
                    option_line,
                    format!("unknown embed mode '{value}'"),
                ));
            }
        };
    }

    Ok(ParsedEmbedBlock { target, mode })
}

fn embed_option_error(line: usize, message: impl Into<String>) -> BuildError {
    code_image_error(line, "embed", message, embed_option_help())
}

fn embed_option_help() -> &'static str {
    "put the X status URL first, then use at most one option: mode: card or mode: screenshot"
}

fn parse_embed_url(line: usize, url: &str) -> Result<EmbedTarget> {
    for prefix in ["https://x.com/", "https://twitter.com/"] {
        if url
            .get(..prefix.len())
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(prefix))
        {
            return parse_x_status_url(line, &url[prefix.len()..]);
        }
    }
    Err(unsupported_embed_url_error(line))
}

fn parse_x_status_url(line: usize, path: &str) -> Result<EmbedTarget> {
    if path.contains(['%', '?', '#']) {
        return Err(unsupported_embed_url_error(line));
    }
    let mut segments = path.split('/');
    let (Some(user), Some("status"), Some(status_id), None) = (
        segments.next(),
        segments.next(),
        segments.next(),
        segments.next(),
    ) else {
        return Err(unsupported_embed_url_error(line));
    };
    if user.is_empty()
        || !user
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        || status_id.is_empty()
        || !status_id.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(unsupported_embed_url_error(line));
    }

    let user = user.to_ascii_lowercase();
    Ok(EmbedTarget::X(TweetStatusUrl {
        normalized_url: format!("https://x.com/{user}/status/{status_id}"),
        user,
        status_id: status_id.to_owned(),
    }))
}

fn unsupported_embed_url_error(line: usize) -> BuildError {
    code_image_error(
        line,
        "embed",
        "embed block must contain a supported X status URL",
        embed_url_help(),
    )
}

fn embed_url_help() -> &'static str {
    "use https://x.com/<user>/status/<id> or https://twitter.com/<user>/status/<id>; other providers belong to the generic-oEmbed follow-up"
}

/// Typed narrowing that keeps the SVG tail unable to see BuiltinMath or
/// BuiltinEmbed; a future SVG-emitting built-in must extend this seam explicitly.
enum SvgCodeImageRenderer<'a> {
    External(&'a CodeImageCommand),
    BuiltinMermaid,
}

#[allow(clippy::too_many_arguments)]
pub fn parse_deck_and_transform<S: SvgRunner, E: EmbedRenderer, F: OEmbedFetcher>(
    source: &str,
    frontmatter: ParsedFrontmatter,
    highlighter: &Highlighter,
    svg_runner: &S,
    embed_renderer: &E,
    oembed_fetcher: &F,
    code_images_cache_dir: &Path,
    embeds_cache_dir: &Path,
) -> Result<Deck<Parsed>> {
    let config = frontmatter.settings().code_images().clone();
    let parsed = parse_markdown(source, frontmatter, highlighter)?;
    transform_code_images(
        parsed,
        &config,
        svg_runner,
        embed_renderer,
        oembed_fetcher,
        code_images_cache_dir,
        embeds_cache_dir,
    )
}

pub fn transform_code_images<S: SvgRunner, E: EmbedRenderer, F: OEmbedFetcher>(
    deck: Deck<Parsed>,
    config: &CodeImagesConfig,
    svg_runner: &S,
    embed_renderer: &E,
    oembed_fetcher: &F,
    code_images_cache_dir: &Path,
    embeds_cache_dir: &Path,
) -> Result<Deck<Parsed>> {
    let (settings, slides) = deck.into_parsed_parts();
    let mut transformed_slides = Vec::with_capacity(slides.len());

    for mut slide in slides {
        slide.fragments = slide
            .fragments
            .into_iter()
            .map(|fragment| {
                transform_fragment(
                    fragment,
                    config,
                    svg_runner,
                    embed_renderer,
                    oembed_fetcher,
                    code_images_cache_dir,
                    embeds_cache_dir,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        transformed_slides.push(slide);
    }

    Ok(Deck::parsed(settings, transformed_slides))
}

fn transform_fragment<S: SvgRunner, E: EmbedRenderer, F: OEmbedFetcher>(
    fragment: SourceFragment,
    config: &CodeImagesConfig,
    svg_runner: &S,
    embed_renderer: &E,
    oembed_fetcher: &F,
    code_images_cache_dir: &Path,
    embeds_cache_dir: &Path,
) -> Result<SourceFragment> {
    // Every code fragment passes through here, and one without a renderer is
    // returned by value with its annotations intact — so carrying emphasis is
    // normal. What must never happen is emphasis on a fragment that gets
    // *rebuilt* into an image or math node: that rebuild would drop the
    // annotation silently, which pillar ③ forbids. The parser rejects that
    // combination at the `renderer_for` seam; this asserts the ordering holds.
    debug_assert!(
        fragment.emphasis().is_none()
            || fragment
                .language()
                .is_none_or(|tag| config.renderer_for(tag).is_none()),
        "line emphasis on a rendered code image must be rejected at parse time",
    );
    let reveal_span = fragment.reveal_span();
    let transformed = (|| -> Result<SourceFragment> {
        if let Some(tag) = fragment.language() {
            if matches!(fragment.kind(), FragmentKind::Code) {
                if let Some(renderer) = config.renderer_for(tag) {
                    let renderer = match renderer {
                        CodeImageRenderer::External(command) => {
                            SvgCodeImageRenderer::External(command)
                        }
                        CodeImageRenderer::BuiltinMermaid => SvgCodeImageRenderer::BuiltinMermaid,
                        CodeImageRenderer::BuiltinMath => {
                            return transform_builtin_math_fragment(
                                fragment.line(),
                                tag,
                                fragment.code_text(),
                            );
                        }
                        CodeImageRenderer::BuiltinEmbed => {
                            let parsed = parse_embed_block(fragment.line(), fragment.code_text())?;
                            let status = parsed.target;
                            return match parsed.mode {
                                EmbedMode::Screenshot => {
                                    let key = cache_or_render_embed(
                                        fragment.line(),
                                        &status,
                                        BUILTIN_EMBED_PARAMS,
                                        embed_renderer,
                                        embeds_cache_dir,
                                    )?;
                                    Ok(SourceFragment::image(
                                        fragment.line(),
                                        format!("X post by @{}", status.user()),
                                        RawImagePath::from_embeds_cache(&key),
                                    ))
                                }
                                EmbedMode::Card => {
                                    let cache_path = oembed_cache_path(embeds_cache_dir, &status);
                                    let document = cache_or_fetch_oembed(
                                        fragment.line(),
                                        &status,
                                        oembed_fetcher,
                                        embeds_cache_dir,
                                    )?;
                                    let markup = build_embed_card_html(
                                        fragment.line(),
                                        status.normalized_url(),
                                        &document,
                                    )
                                    .map_err(|err| {
                                        with_oembed_cache_refresh_help(err, &cache_path)
                                    })?;
                                    Ok(SourceFragment::embed_card(
                                        fragment.line(),
                                        markup.html,
                                        markup.plain_text,
                                    ))
                                }
                            };
                        }
                    };
                    let key = match &renderer {
                        SvgCodeImageRenderer::External(command) => {
                            code_image_cache_key(command, fragment.code_text())
                        }
                        SvgCodeImageRenderer::BuiltinMermaid => {
                            builtin_mermaid_cache_key(fragment.code_text())
                        }
                    };
                    let cache_path = code_images_cache_dir.join(format!("{key}.svg"));
                    fs::create_dir_all(code_images_cache_dir).map_err(|err| {
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
                            SvgCodeImageRenderer::External(command) => {
                                let bytes = svg_runner.run(command, fragment.code_text()).map_err(
                                    |err| {
                                        code_image_error(
                                            fragment.line(),
                                            tag,
                                            err.message,
                                            err.help,
                                        )
                                    },
                                )?;
                                (bytes, CodeImageOutputContext::ExternalCommand)
                            }
                            SvgCodeImageRenderer::BuiltinMermaid => {
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
                        let bytes = normalize_svg_intrinsic_size(
                            fragment.line(),
                            tag,
                            &bytes,
                            output_context,
                        )?;
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
                    .map(|child| {
                        transform_fragment(
                            child,
                            config,
                            svg_runner,
                            embed_renderer,
                            oembed_fetcher,
                            code_images_cache_dir,
                            embeds_cache_dir,
                        )
                    })
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
            | FragmentKind::Math { .. }
            | FragmentKind::EmbedCard { .. }
            | FragmentKind::Footnotes { .. }
            | FragmentKind::Image { .. }
            | FragmentKind::List => Ok(fragment),
        }
    })()?;

    Ok(match reveal_span {
        Some(span) => transformed.with_reveal_span(span),
        None => transformed,
    })
}

fn render_builtin_mermaid(code_text: &str) -> std::result::Result<Vec<u8>, String> {
    render_builtin_mermaid_with(|| BUILTIN_MERMAID_RENDERER.render_svg_sync(code_text))
}

fn transform_builtin_math_fragment(line: usize, tag: &str, latex: &str) -> Result<SourceFragment> {
    if latex.trim().is_empty() {
        return Err(code_image_error(
            line,
            tag,
            "math block is empty",
            builtin_math_override_help(),
        ));
    }
    let html = render_builtin_math(latex)
        .map_err(|message| code_image_error(line, tag, message, builtin_math_override_help()))?;
    Ok(SourceFragment::math(line, html, latex.to_owned()))
}

fn render_builtin_math(latex: &str) -> std::result::Result<String, String> {
    render_builtin_math_with(|| KatexRenderer.render(latex, true))
}

fn render_builtin_math_with<F>(render: F) -> std::result::Result<String, String>
where
    F: FnOnce() -> std::result::Result<MathOutput, crate::math::MathError>,
{
    // AssertUnwindSafe is limited to the captured static KatexContext: no interior mutability in katex-rs 0.2.4 (the only RefCell in the render path is the per-call Settings.macros, constructed inside the closure and dropped on unwind); re-verify on katex-rs upgrades.
    let result = catch_unwind(AssertUnwindSafe(render)).map_err(|payload| {
        format!(
            "built-in math renderer panicked: {}",
            panic_payload_message(payload.as_ref())
        )
    })?;
    match result {
        Ok(MathOutput::HtmlFragment(html)) => Ok(html),
        Err(err) => Err(err.to_string()),
    }
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

fn builtin_math_override_help() -> &'static str {
    "fix the LaTeX source, or set code_images.math to an external command"
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

fn builtin_embed_cache_key(target: &EmbedTarget, params: EmbedRenderParams) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"\0peitho-builtin-embed\0");
    hasher.update(target.normalized_url().as_bytes());
    hasher.update(format!("\0width={}", params.width_css_px).as_bytes());
    hasher.update(format!("\0scale={}", params.scale_factor).as_bytes());
    hasher.update(format!("\0theme={}", params.theme.as_str()).as_bytes());
    hex_encode(&hasher.finalize())
}

fn valid_cached_oembed_json(path: &Path) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.len() <= MAX_OEMBED_RESPONSE_BYTES as u64)
        .unwrap_or(false)
}

fn oembed_cache_path(cache_dir: &Path, target: &EmbedTarget) -> PathBuf {
    let key = builtin_embed_card_cache_key(target.normalized_url());
    cache_dir.join(format!("{key}.json"))
}

fn cache_or_fetch_oembed<F: OEmbedFetcher>(
    line: usize,
    target: &EmbedTarget,
    fetcher: &F,
    cache_dir: &Path,
) -> Result<OEmbedDocument> {
    let cache_path = oembed_cache_path(cache_dir, target);
    fs::create_dir_all(cache_dir).map_err(|err| {
        oembed_cache_error(
            line,
            target,
            &cache_path,
            format!("failed to create embed cache directory: {err}"),
            "make the deck's .peitho directory writable and rebuild",
        )
    })?;

    if valid_cached_oembed_json(&cache_path) {
        if let Ok(raw) = fs::read_to_string(&cache_path) {
            if let Ok(document) = parse_oembed_document(&raw) {
                return Ok(document);
            }
        }
    }

    let raw = fetcher.fetch(target.normalized_url()).map_err(|err| {
        oembed_cache_error(
            line,
            target,
            &cache_path,
            format!("failed to fetch oEmbed data: {}", err.message),
            &err.help,
        )
    })?;
    if raw.len() > MAX_OEMBED_RESPONSE_BYTES {
        return Err(oembed_cache_error(
            line,
            target,
            &cache_path,
            format!(
                "oEmbed response size {} bytes exceeds the maximum of {MAX_OEMBED_RESPONSE_BYTES} bytes",
                raw.len()
            ),
            "retry the oEmbed fetch; X must return a bounded JSON response",
        ));
    }
    let document = parse_oembed_document(&raw).map_err(|err| {
        oembed_cache_error(
            line,
            target,
            &cache_path,
            format!("oEmbed response was not valid data: {err}"),
            "retry with network access to X",
        )
    })?;
    write_cache_file_atomic(&cache_path, raw.as_bytes()).map_err(|err| {
        oembed_cache_error(
            line,
            target,
            &cache_path,
            format!("failed to write oEmbed cache file: {err}"),
            "make the deck's .peitho directory writable and rebuild",
        )
    })?;
    Ok(document)
}

fn oembed_cache_error(
    line: usize,
    target: &EmbedTarget,
    cache_path: &Path,
    message: impl Into<String>,
    extra_help: &str,
) -> BuildError {
    let cache_help = format!(
        "a valid JSON cache hit works offline without curl or network; {}",
        oembed_cache_refresh_help(cache_path)
    );
    let help = if extra_help.is_empty() {
        cache_help
    } else {
        format!("{cache_help}; {extra_help}")
    };
    code_image_error(
        line,
        "embed",
        format!("{} ({})", message.into(), target.normalized_url()),
        help,
    )
}

fn oembed_cache_refresh_help(cache_path: &Path) -> String {
    format!(
        "cache file: {}; delete the cache file to refresh",
        cache_path.display()
    )
}

fn with_oembed_cache_refresh_help(mut error: BuildError, cache_path: &Path) -> BuildError {
    error.help = format!("{}; {}", error.help, oembed_cache_refresh_help(cache_path));
    error
}

fn cache_or_render_embed<R: EmbedRenderer>(
    line: usize,
    target: &EmbedTarget,
    params: EmbedRenderParams,
    renderer: &R,
    cache_dir: &Path,
) -> Result<String> {
    let key = builtin_embed_cache_key(target, params);
    let cache_path = cache_dir.join(format!("{key}.png"));
    fs::create_dir_all(cache_dir).map_err(|err| {
        embed_cache_error(
            line,
            target,
            &cache_path,
            format!("failed to create embed cache directory: {err}"),
            "make the deck's .peitho directory writable and rebuild",
        )
    })?;
    if valid_cached_png(&cache_path) {
        return Ok(key);
    }

    let bytes = renderer
        .render(target.normalized_url(), params)
        .map_err(|err| {
            embed_cache_error(
                line,
                target,
                &cache_path,
                format!(
                    "failed to render {}: {}",
                    target.normalized_url(),
                    err.message
                ),
                &err.help,
            )
        })?;
    if bytes.is_empty() {
        return Err(embed_cache_error(
            line,
            target,
            &cache_path,
            format!(
                "renderer returned empty PNG output for {}",
                target.normalized_url()
            ),
            "retry with Chrome and network access to X",
        ));
    }
    if !bytes.starts_with(PNG_MAGIC) {
        return Err(embed_cache_error(
            line,
            target,
            &cache_path,
            format!(
                "renderer returned not PNG output for {}",
                target.normalized_url()
            ),
            "retry with Chrome and network access to X",
        ));
    }
    write_cache_file_atomic(&cache_path, &bytes).map_err(|err| {
        embed_cache_error(
            line,
            target,
            &cache_path,
            format!("failed to write embed cache file: {err}"),
            "make the deck's .peitho directory writable and rebuild",
        )
    })?;
    Ok(key)
}

fn embed_cache_error(
    line: usize,
    target: &EmbedTarget,
    cache_path: &Path,
    message: impl Into<String>,
    extra_help: &str,
) -> BuildError {
    let cache_help = format!(
        "a valid cache hit works offline without Chrome or network; this cache miss requires Chrome and network access to X; cache file: {}; delete the cache file to refresh",
        cache_path.display()
    );
    let help = if extra_help.is_empty() {
        cache_help
    } else {
        format!("{cache_help}; {extra_help}")
    };
    code_image_error(
        line,
        "embed",
        format!("{} ({})", message.into(), target.normalized_url()),
        help,
    )
}

fn valid_cached_png(path: &Path) -> bool {
    let cache_hit = fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.len() >= PNG_MAGIC.len() as u64)
        .unwrap_or(false);
    if !cache_hit {
        return false;
    }
    fs::read(path)
        .map(|bytes| bytes.starts_with(PNG_MAGIC))
        .unwrap_or(false)
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
        builtin_embed_cache_key, builtin_math_override_help, builtin_mermaid_cache_key,
        builtin_mermaid_override_help, cache_or_fetch_oembed, cache_or_render_embed,
        code_image_cache_key, hex_encode, is_svg_output, parse_deck_and_transform,
        parse_embed_block, render_builtin_math_with, render_builtin_mermaid_with,
        svg_empty_output_error, svg_has_usable_intrinsic_size, svg_intrinsic_size_error,
        svg_not_document_error, svg_root_not_found_error, transform_code_images,
        valid_cached_oembed_json, valid_cached_png, CodeImageOutputContext, EmbedMode,
        EmbedRenderParams, EmbedRenderer, EmbedTarget, EmbedTheme, OEmbedFetcher, SvgRunner,
        BUILTIN_EMBED_PARAMS,
    };
    use crate::embed_card::builtin_embed_card_cache_key;
    use crate::error::ErrorKind;
    use crate::{
        check::check_deck,
        domain::{
            CodeImageCommand, CodeImagesConfig, FragmentKind, RawImagePath, ResolvedImageAsset,
            ResolvedImagePath, RevealSpan, SlotName, SourceFragment,
        },
        layout::{parse_layout, Layouts},
        mapping::{dispatch_by_convention, map_by_convention},
        parser::{parse_frontmatter, parse_markdown},
        phase::{resolve_image_paths, Deck, DeckSettings, KeySource, Parsed, ParsedSlide},
        BuildError, Result,
    };
    use sha2::{Digest, Sha256};
    use std::{
        cell::{Cell, RefCell},
        collections::BTreeMap,
        fs,
        path::{Path, PathBuf},
    };

    const MERMAID_KEY: &str = "4dba32c8d19de69fc2671719f51c327b802adf382763f36d20c1bffd972745f1";

    mod embed_cache_key_test_vector {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/embed_cache_key.rs"
        ));
    }

    #[test]
    fn embed_cache_key_covers_url_width_scale_and_theme_without_crate_version() {
        let status =
            parse_embed_block(7, "https://x.com/gosukenator/status/2083825695709597710").unwrap();
        let base = EmbedRenderParams::new(550, 2, EmbedTheme::Light);
        assert_eq!(
            builtin_embed_cache_key(&status.target, base),
            embed_cache_key_test_vector::PINNED_BUILTIN_EMBED_CACHE_KEY
        );
        assert_ne!(
            builtin_embed_cache_key(&status.target, base),
            builtin_embed_cache_key(
                &status.target,
                EmbedRenderParams::new(551, 2, EmbedTheme::Light)
            )
        );
        assert_ne!(
            builtin_embed_cache_key(&status.target, base),
            builtin_embed_cache_key(
                &status.target,
                EmbedRenderParams::new(550, 1, EmbedTheme::Light)
            )
        );
        assert_ne!(
            builtin_embed_cache_key(&status.target, base),
            builtin_embed_cache_key(
                &status.target,
                EmbedRenderParams::new(550, 2, EmbedTheme::Dark)
            )
        );
    }

    #[test]
    fn embed_cache_raw_path_uses_png_cache_namespace() {
        let raw = RawImagePath::from_embeds_cache("abc123");
        assert_eq!(raw.as_str(), ".peitho/embeds-cache/abc123.png");
    }

    #[test]
    fn embed_block_accepts_x_and_twitter_status_urls() {
        let x = parse_embed_block(
            7,
            "\n  https://x.com/Gosukenator/status/2083825695709597710  \n",
        )
        .unwrap();
        let twitter = parse_embed_block(
            7,
            "https://twitter.com/gosukenator/status/2083825695709597710\n",
        )
        .unwrap();
        assert_eq!(
            x.target.normalized_url(),
            "https://x.com/gosukenator/status/2083825695709597710"
        );
        assert_eq!(x, twitter);
    }

    #[test]
    fn embed_block_accepts_case_insensitive_scheme_and_host() {
        let canonical =
            parse_embed_block(7, "https://x.com/gosukenator/status/2083825695709597710").unwrap();
        let uppercase_x_host =
            parse_embed_block(7, "https://X.com/Gosukenator/status/2083825695709597710").unwrap();
        let uppercase_twitter_scheme = parse_embed_block(
            7,
            "HTTPS://twitter.com/gosukenator/status/2083825695709597710",
        )
        .unwrap();

        assert_eq!(uppercase_x_host, canonical);
        assert_eq!(uppercase_twitter_scheme, canonical);
    }

    #[test]
    fn embed_block_defaults_to_screenshot_and_accepts_explicit_modes() {
        let implicit = parse_embed_block(7, "https://x.com/a/status/1\n").unwrap();
        let screenshot =
            parse_embed_block(7, "https://x.com/a/status/1\nmode: screenshot\n").unwrap();
        let card = parse_embed_block(7, "https://x.com/a/status/1\nmode: card\n").unwrap();

        assert_eq!(implicit.mode, EmbedMode::Screenshot);
        assert_eq!(screenshot.mode, EmbedMode::Screenshot);
        assert_eq!(card.mode, EmbedMode::Card);
        assert_eq!(implicit.target, screenshot.target);
        assert_eq!(implicit.target, card.target);
    }

    #[test]
    fn embed_block_allows_blank_lines_between_url_and_options() {
        let parsed =
            parse_embed_block(7, "\nhttps://x.com/a/status/1\n\n   \nmode: card\n\n").unwrap();

        assert_eq!(parsed.mode, EmbedMode::Card);
        assert_eq!(parsed.target.normalized_url(), "https://x.com/a/status/1");
    }

    fn assert_embed_option_error(body: &str, line: usize, message: &str) {
        let err = parse_embed_block(7, body).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Asset);
        assert_eq!(err.line, Some(line));
        assert!(err.message.contains(message), "{}", err.message);
        assert!(err.help.contains("mode: card"), "{}", err.help);
        assert!(err.help.contains("mode: screenshot"), "{}", err.help);
    }

    #[test]
    fn embed_block_rejects_option_before_url_at_source_line() {
        assert_embed_option_error(
            "\nmode: card\nhttps://x.com/a/status/1\n",
            9,
            "option 'mode' must follow the URL",
        );
    }

    #[test]
    fn embed_block_distinguishes_malformed_url_from_option_before_url() {
        let err = parse_embed_block(7, "https:/x.com/a/status/1\n").unwrap_err();
        assert_eq!(err.kind, ErrorKind::Asset);
        assert_eq!(err.line, Some(7));
        assert!(
            err.message.contains("supported X status URL"),
            "{}",
            err.message
        );
        assert!(
            !err.message.contains("must follow the URL"),
            "{}",
            err.message
        );
        assert!(err.help.contains("https://x.com/<user>/status/<id>"));
        assert!(err.help.contains("https://twitter.com/<user>/status/<id>"));

        assert_embed_option_error(
            "mode: card\nhttps://x.com/a/status/1\n",
            8,
            "option 'mode' must follow the URL",
        );
    }

    #[test]
    fn embed_block_rejects_unknown_option_key_at_source_line() {
        assert_embed_option_error(
            "https://x.com/a/status/1\ntheme: dark\n",
            9,
            "unknown embed option 'theme'",
        );
    }

    #[test]
    fn embed_block_rejects_unknown_mode_value_at_source_line() {
        assert_embed_option_error(
            "https://x.com/a/status/1\nmode: compact\n",
            9,
            "unknown embed mode 'compact'",
        );
    }

    #[test]
    fn embed_block_rejects_duplicate_or_malformed_options_at_source_line() {
        assert_embed_option_error(
            "https://x.com/a/status/1\nmode: card\nmode: screenshot\n",
            10,
            "duplicate embed option 'mode'",
        );
        assert_embed_option_error(
            "https://x.com/a/status/1\nmode card\n",
            9,
            "embed option must use 'key: value' syntax",
        );
        assert_embed_option_error(
            "https://x.com/a/status/1\nmode:\n",
            9,
            "embed option value is empty",
        );
    }

    fn assert_embed_error(body: &str, message: &str) {
        let err = parse_embed_block(7, body).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Asset);
        assert_eq!(err.line, Some(7));
        assert!(err.message.contains(message), "{}", err.message);
        assert!(err.help.contains("https://x.com/<user>/status/<id>"));
        assert!(err.help.contains("https://twitter.com/<user>/status/<id>"));
    }

    #[test]
    fn embed_block_rejects_empty_body() {
        let err = parse_embed_block(7, "\n \n").unwrap_err();
        assert_eq!(err.kind, ErrorKind::Asset);
        assert_eq!(err.line, Some(7));
        assert!(err.message.contains("empty"), "{}", err.message);
        assert!(err.help.contains("https://x.com/<user>/status/<id>"));
        assert!(err.help.contains("https://twitter.com/<user>/status/<id>"));
        assert!(err.help.contains("URL first"), "{}", err.help);
        assert!(err.help.contains("mode: card"), "{}", err.help);
        assert!(err.help.contains("mode: screenshot"), "{}", err.help);
    }

    #[test]
    fn embed_block_rejects_second_url_as_unknown_option() {
        assert_embed_option_error(
            "https://x.com/a/status/1\nhttps://x.com/b/status/2\n",
            9,
            "unknown embed option 'https'",
        );
    }

    #[test]
    fn embed_block_rejects_surrounding_junk() {
        assert_embed_error("see https://x.com/a/status/1", "supported X status URL");
    }

    #[test]
    fn embed_block_rejects_non_https_url() {
        assert_embed_error("http://x.com/a/status/1", "supported X status URL");
    }

    #[test]
    fn embed_block_rejects_non_x_provider() {
        assert_embed_error("https://example.com/a/status/1", "supported X status URL");
    }

    #[test]
    fn embed_block_rejects_malformed_status_path() {
        assert_embed_error("https://x.com/a/status/1/", "supported X status URL");
    }

    #[test]
    fn embed_block_rejects_non_numeric_status_id() {
        assert_embed_error(
            "https://x.com/a/status/not-a-number",
            "supported X status URL",
        );
    }

    #[test]
    fn embed_block_rejects_query() {
        assert_embed_error("https://x.com/a/status/1?s=20", "supported X status URL");
    }

    #[test]
    fn embed_block_rejects_fragment() {
        assert_embed_error("https://x.com/a/status/1#quoted", "supported X status URL");
    }

    #[test]
    fn embed_block_rejects_percent_encoding() {
        assert_embed_error("https://x.com/a%20b/status/1", "supported X status URL");
    }

    struct FakeRunner {
        calls: Cell<usize>,
        result: Result<Vec<u8>>,
    }

    struct NoSvgRunner;

    impl SvgRunner for NoSvgRunner {
        fn run(&self, _command: &CodeImageCommand, _stdin: &str) -> Result<Vec<u8>> {
            panic!("a plain deck must not run an SVG renderer");
        }
    }

    fn embed_cache_dir(code_images_cache_dir: &Path) -> PathBuf {
        code_images_cache_dir
            .parent()
            .expect("code image cache has a .peitho parent")
            .join("embeds-cache")
    }

    struct FixtureEmbedRenderer {
        calls: Cell<usize>,
        urls: RefCell<Vec<String>>,
        params: RefCell<Vec<EmbedRenderParams>>,
        result: Result<Vec<u8>>,
    }

    impl FixtureEmbedRenderer {
        fn png(output: Vec<u8>) -> Self {
            Self {
                calls: Cell::new(0),
                urls: RefCell::new(Vec::new()),
                params: RefCell::new(Vec::new()),
                result: Ok(output),
            }
        }

        fn calls(&self) -> usize {
            self.calls.get()
        }

        fn urls(&self) -> Vec<String> {
            self.urls.borrow().clone()
        }

        fn params(&self) -> Vec<EmbedRenderParams> {
            self.params.borrow().clone()
        }

        fn err(message: &str) -> Self {
            Self {
                calls: Cell::new(0),
                urls: RefCell::new(Vec::new()),
                params: RefCell::new(Vec::new()),
                result: Err(BuildError::new(
                    ErrorKind::Asset,
                    None,
                    message,
                    "set PEITHO_CHROME_PATH to a Chrome executable and retry",
                )),
            }
        }
    }

    impl EmbedRenderer for FixtureEmbedRenderer {
        fn render(&self, normalized_url: &str, params: EmbedRenderParams) -> Result<Vec<u8>> {
            self.calls.set(self.calls.get() + 1);
            self.urls.borrow_mut().push(normalized_url.to_owned());
            self.params.borrow_mut().push(params);
            self.result.clone()
        }
    }

    const OEMBED_JSON: &str = concat!(
        r#"{"html":"<blockquote><p>hello</p><a href=\"https://x.com/a/status/1\">January 1, 2026</a></blockquote>","author_name":"A","url":"https://x.com/a/status/1","extra":true}"#,
        "\n"
    );

    fn oversized_oembed_json() -> String {
        format!(
            r#"{{"html":"x","author_name":"Oversized","url":"https://x.com/a/status/1","padding":"{}"}}"#,
            "x".repeat(crate::MAX_OEMBED_RESPONSE_BYTES)
        )
    }

    struct FixtureOEmbedFetcher {
        calls: Cell<usize>,
        urls: RefCell<Vec<String>>,
        result: Result<String>,
    }

    impl FixtureOEmbedFetcher {
        fn json(response: &str) -> Self {
            Self {
                calls: Cell::new(0),
                urls: RefCell::new(Vec::new()),
                result: Ok(response.to_owned()),
            }
        }

        fn err(message: &str) -> Self {
            Self {
                calls: Cell::new(0),
                urls: RefCell::new(Vec::new()),
                result: Err(BuildError::new(
                    ErrorKind::Asset,
                    None,
                    message,
                    "install curl and retry with network access",
                )),
            }
        }
    }

    impl OEmbedFetcher for FixtureOEmbedFetcher {
        fn fetch(&self, normalized_url: &str) -> Result<String> {
            self.calls.set(self.calls.get() + 1);
            self.urls.borrow_mut().push(normalized_url.to_owned());
            self.result.clone()
        }
    }

    struct PanicOEmbedFetcher;

    impl OEmbedFetcher for PanicOEmbedFetcher {
        fn fetch(&self, _normalized_url: &str) -> Result<String> {
            panic!("a valid oEmbed cache hit must not invoke the fetcher");
        }
    }

    #[test]
    fn oembed_cache_miss_fetches_once_and_writes_raw_json_atomically() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::EMBEDS_CACHE_DIR);
        let target = parse_embed_block(7, "https://x.com/a/status/1")
            .unwrap()
            .target;
        let fetcher = FixtureOEmbedFetcher::json(OEMBED_JSON);

        let document = cache_or_fetch_oembed(7, &target, &fetcher, &cache_dir).unwrap();

        assert_eq!(fetcher.calls.get(), 1);
        assert_eq!(fetcher.urls.borrow().as_slice(), [target.normalized_url()]);
        assert_eq!(document.author_name, "A");
        let key = builtin_embed_card_cache_key(target.normalized_url());
        assert_eq!(
            fs::read(cache_dir.join(format!("{key}.json"))).unwrap(),
            OEMBED_JSON.as_bytes()
        );
        assert_eq!(fs::read_dir(cache_dir).unwrap().count(), 1);
    }

    #[test]
    fn oembed_valid_cache_hit_skips_fetcher() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::EMBEDS_CACHE_DIR);
        let target = parse_embed_block(7, "https://x.com/a/status/1")
            .unwrap()
            .target;
        let key = builtin_embed_card_cache_key(target.normalized_url());
        fs::create_dir_all(&cache_dir).unwrap();
        fs::write(cache_dir.join(format!("{key}.json")), OEMBED_JSON).unwrap();

        let document = cache_or_fetch_oembed(7, &target, &PanicOEmbedFetcher, &cache_dir).unwrap();

        assert_eq!(document.html, "<blockquote><p>hello</p><a href=\"https://x.com/a/status/1\">January 1, 2026</a></blockquote>");
        assert_eq!(document.url, "https://x.com/a/status/1");
    }

    #[test]
    fn oembed_non_regular_cache_path_is_a_miss_and_fetches() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::EMBEDS_CACHE_DIR);
        let target = parse_embed_block(7, "https://x.com/a/status/1")
            .unwrap()
            .target;
        let key = builtin_embed_card_cache_key(target.normalized_url());
        let cache_path = cache_dir.join(format!("{key}.json"));
        fs::create_dir_all(&cache_path).unwrap();
        let fetcher = FixtureOEmbedFetcher::err("expected cache miss");

        assert!(!valid_cached_oembed_json(&cache_path));
        let err = cache_or_fetch_oembed(7, &target, &fetcher, &cache_dir).unwrap_err();

        assert_eq!(fetcher.calls.get(), 1);
        assert!(
            err.message.contains("expected cache miss"),
            "{}",
            err.message
        );
    }

    #[test]
    fn oembed_oversized_cache_is_a_miss_and_self_heals() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::EMBEDS_CACHE_DIR);
        let target = parse_embed_block(7, "https://x.com/a/status/1")
            .unwrap()
            .target;
        let key = builtin_embed_card_cache_key(target.normalized_url());
        let cache_path = cache_dir.join(format!("{key}.json"));
        fs::create_dir_all(&cache_dir).unwrap();
        fs::write(&cache_path, oversized_oembed_json()).unwrap();
        let fetcher = FixtureOEmbedFetcher::json(OEMBED_JSON);

        let document = cache_or_fetch_oembed(7, &target, &fetcher, &cache_dir).unwrap();

        assert_eq!(fetcher.calls.get(), 1);
        assert_eq!(document.author_name, "A");
        assert_eq!(fs::read(&cache_path).unwrap(), OEMBED_JSON.as_bytes());
    }

    #[test]
    fn oembed_oversized_fetch_is_line_numbered_error_and_not_cached() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::EMBEDS_CACHE_DIR);
        let target = parse_embed_block(41, "https://x.com/a/status/1")
            .unwrap()
            .target;
        let key = builtin_embed_card_cache_key(target.normalized_url());
        let cache_path = cache_dir.join(format!("{key}.json"));
        let oversized = oversized_oembed_json();
        let fetcher = FixtureOEmbedFetcher::json(&oversized);

        let err = cache_or_fetch_oembed(41, &target, &fetcher, &cache_dir).unwrap_err();

        assert_eq!(err.kind, ErrorKind::Asset);
        assert_eq!(err.line, Some(41));
        assert!(err.message.contains("exceeds"), "{}", err.message);
        assert!(
            err.message
                .contains(&crate::MAX_OEMBED_RESPONSE_BYTES.to_string()),
            "{}",
            err.message
        );
        assert!(!cache_path.exists());
    }

    #[test]
    fn oembed_invalid_cache_self_heals() {
        for invalid in [
            "",
            "{",
            "{}",
            r#"{"html":1,"author_name":"A","url":"https://x.com/a/status/1"}"#,
            r#"{"html":"x","author_name":false,"url":"https://x.com/a/status/1"}"#,
            r#"{"html":"x","author_name":"A","url":null}"#,
        ] {
            let temp = tempfile::tempdir().unwrap();
            let cache_dir = temp.path().join(crate::EMBEDS_CACHE_DIR);
            let target = parse_embed_block(7, "https://x.com/a/status/1")
                .unwrap()
                .target;
            let key = builtin_embed_card_cache_key(target.normalized_url());
            let cache_path = cache_dir.join(format!("{key}.json"));
            fs::create_dir_all(&cache_dir).unwrap();
            fs::write(&cache_path, invalid).unwrap();
            let fetcher = FixtureOEmbedFetcher::json(OEMBED_JSON);

            cache_or_fetch_oembed(7, &target, &fetcher, &cache_dir).unwrap();

            assert_eq!(fetcher.calls.get(), 1, "invalid cache: {invalid:?}");
            assert_eq!(fs::read(&cache_path).unwrap(), OEMBED_JSON.as_bytes());
            assert_eq!(fs::read_dir(&cache_dir).unwrap().count(), 1);
        }
    }

    #[test]
    fn oembed_fetch_failure_names_line_url_cache_and_refresh() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::EMBEDS_CACHE_DIR);
        let target = parse_embed_block(41, "https://x.com/a/status/1")
            .unwrap()
            .target;

        let err = cache_or_fetch_oembed(
            41,
            &target,
            &FixtureOEmbedFetcher::err("curl exited with status 22"),
            &cache_dir,
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Asset);
        assert_eq!(err.line, Some(41));
        assert!(
            err.message.contains(target.normalized_url()),
            "{}",
            err.message
        );
        assert!(err.help.contains(".peitho/embeds-cache/"), "{}", err.help);
        assert!(err.help.contains(".json"), "{}", err.help);
        assert!(err.help.contains("delete"), "{}", err.help);
        assert!(err.help.contains("offline"), "{}", err.help);
    }

    struct PanicEmbedRenderer;

    impl EmbedRenderer for PanicEmbedRenderer {
        fn render(&self, _normalized_url: &str, _params: EmbedRenderParams) -> Result<Vec<u8>> {
            panic!("a valid embed cache hit must not invoke the renderer");
        }
    }

    struct SeededEmbedCache {
        _temp: tempfile::TempDir,
        cache_dir: PathBuf,
        target: EmbedTarget,
    }

    thread_local! {
        static SEEDED_EMBED_CACHE: RefCell<Option<SeededEmbedCache>> = const { RefCell::new(None) };
    }

    fn seed_embed_cache(bytes: &[u8]) -> String {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::EMBEDS_CACHE_DIR);
        let target = parse_embed_block(7, "https://x.com/a/status/1")
            .unwrap()
            .target;
        let key = builtin_embed_cache_key(&target, BUILTIN_EMBED_PARAMS);
        let cache_path = cache_dir.join(format!("{key}.png"));
        fs::create_dir_all(&cache_dir).unwrap();
        fs::write(&cache_path, bytes).unwrap();
        assert!(valid_cached_png(&cache_path));
        SEEDED_EMBED_CACHE.with(|seeded| {
            seeded.replace(Some(SeededEmbedCache {
                _temp: temp,
                cache_dir,
                target,
            }));
        });
        key
    }

    fn cache_or_render_seeded_embed<R: EmbedRenderer>(renderer: &R) -> Result<String> {
        SEEDED_EMBED_CACHE.with(|seeded| {
            let seeded = seeded.borrow();
            let seeded = seeded.as_ref().expect("seed an embed cache first");
            cache_or_render_embed(
                7,
                &seeded.target,
                BUILTIN_EMBED_PARAMS,
                renderer,
                &seeded.cache_dir,
            )
        })
    }

    fn assert_embed_cache_self_heals(old_bytes: &[u8]) {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::EMBEDS_CACHE_DIR);
        let target = parse_embed_block(7, "https://x.com/a/status/1")
            .unwrap()
            .target;
        let key = builtin_embed_cache_key(&target, BUILTIN_EMBED_PARAMS);
        let cache_path = cache_dir.join(format!("{key}.png"));
        fs::create_dir_all(&cache_dir).unwrap();
        fs::write(&cache_path, old_bytes).unwrap();
        assert!(!valid_cached_png(&cache_path));
        let renderer = FixtureEmbedRenderer::png(b"\x89PNG\r\n\x1a\nreplacement".to_vec());

        let returned_key =
            cache_or_render_embed(7, &target, BUILTIN_EMBED_PARAMS, &renderer, &cache_dir).unwrap();

        assert_eq!(returned_key, key);
        assert_eq!(renderer.calls(), 1);
        assert_eq!(
            fs::read(cache_path).unwrap(),
            b"\x89PNG\r\n\x1a\nreplacement"
        );
    }

    fn cache_embed_with_renderer_error(message: &str) -> Result<String> {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::EMBEDS_CACHE_DIR);
        let target = parse_embed_block(7, "https://x.com/a/status/1")
            .unwrap()
            .target;
        cache_or_render_embed(
            7,
            &target,
            BUILTIN_EMBED_PARAMS,
            &FixtureEmbedRenderer::err(message),
            &cache_dir,
        )
    }

    fn assert_invalid_embed_png(bytes: &[u8], message: &str) {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::EMBEDS_CACHE_DIR);
        let target = parse_embed_block(7, "https://x.com/a/status/1")
            .unwrap()
            .target;
        let err = cache_or_render_embed(
            7,
            &target,
            BUILTIN_EMBED_PARAMS,
            &FixtureEmbedRenderer::png(bytes.to_vec()),
            &cache_dir,
        )
        .unwrap_err();
        assert_eq!(err.kind, ErrorKind::Asset);
        assert_eq!(err.line, Some(7));
        assert!(err.message.contains(message), "{}", err.message);
        assert!(err.help.contains(".peitho/embeds-cache/"));
    }

    fn assert_embed_cache_path_collision_is_asset_error_at_line(line: usize) {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::EMBEDS_CACHE_DIR);
        fs::create_dir_all(cache_dir.parent().unwrap()).unwrap();
        fs::write(&cache_dir, b"regular file blocks cache directory").unwrap();
        let target = parse_embed_block(line, "https://x.com/a/status/1")
            .unwrap()
            .target;

        let err = cache_or_render_embed(
            line,
            &target,
            BUILTIN_EMBED_PARAMS,
            &PanicEmbedRenderer,
            &cache_dir,
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Asset);
        assert_eq!(err.line, Some(line));
        assert!(err
            .message
            .contains("failed to create embed cache directory"));
        assert!(err.help.contains(".peitho"));
        assert!(err.help.contains("writable"));
    }

    #[test]
    fn embed_renderer_error_names_line_url_and_cache_path() {
        let err = cache_embed_with_renderer_error("Chrome not found").unwrap_err();
        assert_eq!(err.kind, ErrorKind::Asset);
        assert_eq!(err.line, Some(7));
        assert!(err.message.contains("https://x.com/a/status/1"));
        assert!(err.message.contains("Chrome not found"));
        assert!(err.help.contains(".peitho/embeds-cache/"));
        assert!(err.help.contains("cache hit works offline"));
        assert!(err.help.contains("delete the cache file to refresh"));
    }

    #[test]
    fn embed_invalid_renderer_output_empty_is_line_numbered() {
        assert_invalid_embed_png(b"", "empty PNG output");
    }

    #[test]
    fn embed_invalid_renderer_output_non_png_is_line_numbered() {
        assert_invalid_embed_png(b"<html>blocked</html>", "not PNG output");
    }

    #[test]
    fn embed_cache_directory_failure_is_line_numbered() {
        assert_embed_cache_path_collision_is_asset_error_at_line(7);
    }

    #[test]
    fn threads_embed_renderer_without_calling_it_for_plain_deck() {
        let temp = tempfile::tempdir().unwrap();
        let markdown = "# Plain\n\nParagraph\n";
        let deck = parse_deck_and_transform(
            markdown,
            parse_frontmatter(markdown).unwrap(),
            &crate::highlight::Highlighter::defaults(),
            &NoSvgRunner,
            &PanicEmbedRenderer,
            &PanicOEmbedFetcher,
            &temp.path().join(crate::CODE_IMAGES_CACHE_DIR),
            &temp.path().join(crate::EMBEDS_CACHE_DIR),
        )
        .unwrap();
        assert_eq!(deck.parsed_slides().len(), 1);
    }

    fn transform_embed_fixture(url: &str) -> Result<Deck<Parsed>> {
        let temp = tempfile::tempdir().unwrap();
        let code_images_cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        let embeds_cache_dir = temp.path().join(crate::EMBEDS_CACHE_DIR);
        transform_code_images(
            deck_with_spanned_code("embed", url, RevealSpan { start: 1, len: 1 }),
            &CodeImagesConfig::default(),
            &NoSvgRunner,
            &FixtureEmbedRenderer::png(b"\x89PNG\r\n\x1a\nfixture".to_vec()),
            &PanicOEmbedFetcher,
            &code_images_cache_dir,
            &embeds_cache_dir,
        )
    }

    #[test]
    fn builtin_embed_becomes_png_image_and_preserves_reveal_span() {
        let transformed = transform_embed_fixture("https://twitter.com/A/status/1").unwrap();
        let fragment = &transformed.parsed_slides()[0].fragments[0];
        assert_eq!(fragment.line(), 7);
        assert_eq!(
            fragment.reveal_span(),
            Some(RevealSpan { start: 1, len: 1 })
        );
        match fragment.kind() {
            FragmentKind::Image { alt, src } => {
                assert_eq!(alt, "X post by @a");
                assert!(src.as_str().starts_with(".peitho/embeds-cache/"));
                assert!(src.as_str().ends_with(".png"));
            }
            kind => panic!("expected image, got {kind:?}"),
        }
    }

    #[test]
    fn builtin_card_mode_fetches_without_rendering_png() {
        let temp = tempfile::tempdir().unwrap();
        let code_images_cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        let embeds_cache_dir = temp.path().join(crate::EMBEDS_CACHE_DIR);
        let fetcher = FixtureOEmbedFetcher::json(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/x-oembed-response.json"
        )));
        let span = RevealSpan { start: 1, len: 1 };

        let transformed = transform_code_images(
            deck_with_spanned_code(
                "embed",
                "https://x.com/gosukenator/status/2074821309259973046\nmode: card\n",
                span,
            ),
            &CodeImagesConfig::default(),
            &NoSvgRunner,
            &PanicEmbedRenderer,
            &fetcher,
            &code_images_cache_dir,
            &embeds_cache_dir,
        )
        .unwrap();

        assert_eq!(fetcher.calls.get(), 1);
        assert_eq!(
            fetcher.urls.borrow().as_slice(),
            ["https://x.com/gosukenator/status/2074821309259973046"]
        );
        assert_eq!(fs::read_dir(&embeds_cache_dir).unwrap().count(), 1);
        assert!(fs::read_dir(&embeds_cache_dir).unwrap().all(|entry| entry
            .unwrap()
            .path()
            .extension()
            .unwrap()
            == "json"));
        let fragment = &transformed.parsed_slides()[0].fragments[0];
        assert_eq!(fragment.reveal_span(), Some(span));
        assert_eq!(fragment.plain_text(), "自前プレゼンツール、スライド作成中に表示確認したり、スライド一覧をグリッドで表示したり、Markdownの修正をリアルタイムに反映したりする機能を入れたhttps://t.co/c3iJNbu3uw pic.twitter.com/mhIvL0JQFA");
        match fragment.kind() {
            FragmentKind::EmbedCard { html } => {
                assert!(html.contains("peitho-embed-card__tweet-text"));
                assert!(!html.contains("twitter-tweet"));
            }
            kind => panic!("expected embed card, got {kind:?}"),
        }
    }

    #[test]
    fn builtin_card_structure_error_names_cache_path() {
        let temp = tempfile::tempdir().unwrap();
        let code_images_cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        let embeds_cache_dir = temp.path().join(crate::EMBEDS_CACHE_DIR);
        let normalized_url = "https://x.com/a/status/1";
        let key = builtin_embed_card_cache_key(normalized_url);
        let cache_path = embeds_cache_dir.join(format!("{key}.json"));
        fs::create_dir_all(&embeds_cache_dir).unwrap();
        fs::write(
            &cache_path,
            r#"{"html":"<blockquote class=\"twitter-tweet\"><p>hello</p></blockquote>","author_name":"A","url":"https://x.com/a/status/1"}"#,
        )
        .unwrap();

        let err = transform_code_images(
            deck_with_spanned_code(
                "embed",
                "https://x.com/a/status/1\nmode: card\n",
                RevealSpan { start: 1, len: 1 },
            ),
            &CodeImagesConfig::default(),
            &NoSvgRunner,
            &PanicEmbedRenderer,
            &PanicOEmbedFetcher,
            &code_images_cache_dir,
            &embeds_cache_dir,
        )
        .unwrap_err();

        assert_eq!(err.line, Some(7));
        assert!(err.message.contains("date anchor"), "{}", err.message);
        assert!(
            err.help
                .contains(&format!("cache file: {}", cache_path.display())),
            "{}",
            err.help
        );
        assert!(
            err.help.contains("delete the cache file to refresh"),
            "{}",
            err.help
        );
    }

    #[test]
    fn builtin_screenshot_modes_never_fetch_oembed() {
        for body in [
            "https://x.com/a/status/1\n",
            "https://x.com/a/status/1\nmode: screenshot\n",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let renderer = FixtureEmbedRenderer::png(b"\x89PNG\r\n\x1a\nfixture".to_vec());
            let transformed = transform_code_images(
                deck_with_spanned_code("embed", body, RevealSpan { start: 1, len: 1 }),
                &CodeImagesConfig::default(),
                &NoSvgRunner,
                &renderer,
                &PanicOEmbedFetcher,
                &temp.path().join(crate::CODE_IMAGES_CACHE_DIR),
                &temp.path().join(crate::EMBEDS_CACHE_DIR),
            )
            .unwrap();

            assert_eq!(renderer.calls(), 1);
            assert!(matches!(
                transformed.parsed_slides()[0].fragments[0].kind(),
                FragmentKind::Image { .. }
            ));
        }
    }

    #[test]
    fn legacy_embed_without_options_matches_explicit_screenshot_bytes() {
        fn transform(body: &str) -> (SourceFragment, Vec<u8>) {
            let temp = tempfile::tempdir().unwrap();
            let embeds_cache_dir = temp.path().join(crate::EMBEDS_CACHE_DIR);
            let transformed = transform_code_images(
                deck_with_spanned_code("embed", body, RevealSpan { start: 1, len: 1 }),
                &CodeImagesConfig::default(),
                &NoSvgRunner,
                &FixtureEmbedRenderer::png(b"\x89PNG\r\n\x1a\nfixture".to_vec()),
                &PanicOEmbedFetcher,
                &temp.path().join(crate::CODE_IMAGES_CACHE_DIR),
                &embeds_cache_dir,
            )
            .unwrap();
            let bytes = fs::read(
                fs::read_dir(&embeds_cache_dir)
                    .unwrap()
                    .next()
                    .unwrap()
                    .unwrap()
                    .path(),
            )
            .unwrap();
            (transformed.parsed_slides()[0].fragments[0].clone(), bytes)
        }

        let implicit = transform("https://x.com/a/status/1\n");
        let explicit = transform("https://x.com/a/status/1\nmode: screenshot\n");

        assert_eq!(implicit, explicit);
    }

    struct RecordingSvgRunner {
        inputs: RefCell<Vec<String>>,
    }

    impl SvgRunner for RecordingSvgRunner {
        fn run(&self, _command: &CodeImageCommand, stdin: &str) -> Result<Vec<u8>> {
            self.inputs.borrow_mut().push(stdin.to_owned());
            Ok(br#"<svg viewBox="0 0 10 10">external embed</svg>"#.to_vec())
        }
    }

    #[test]
    fn explicit_embed_override_receives_option_lines_verbatim() {
        let temp = tempfile::tempdir().unwrap();
        let body = "\n  https://x.com/A/status/1  \n\nmode: card\n  future: exact whitespace  \n";
        let runner = RecordingSvgRunner {
            inputs: RefCell::new(Vec::new()),
        };
        let config = CodeImagesConfig {
            entries: BTreeMap::from([(
                "embed".to_owned(),
                CodeImageCommand {
                    argv: vec!["embed-to-svg".to_owned()],
                },
            )]),
            key_line: Some(2),
        };

        let transformed = transform_code_images(
            deck_with_spanned_code("embed", body, RevealSpan { start: 1, len: 1 }),
            &config,
            &runner,
            &PanicEmbedRenderer,
            &PanicOEmbedFetcher,
            &temp.path().join(crate::CODE_IMAGES_CACHE_DIR),
            &temp.path().join(crate::EMBEDS_CACHE_DIR),
        )
        .unwrap();

        assert_eq!(runner.inputs.borrow().as_slice(), [body]);
        assert!(matches!(
            transformed.parsed_slides()[0].fragments[0].kind(),
            FragmentKind::Image { .. }
        ));
    }

    #[test]
    fn explicit_embed_override_uses_svg_runner_without_url_validation() {
        let temp = tempfile::tempdir().unwrap();
        let code_images_cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        let embeds_cache_dir = temp.path().join(crate::EMBEDS_CACHE_DIR);
        let svg_runner = FakeRunner::svg(r#"<svg viewBox="0 0 10 10">embed</svg>"#);
        let embed_renderer = FixtureEmbedRenderer::png(b"\x89PNG\r\n\x1a\nunused".to_vec());
        let config = CodeImagesConfig {
            entries: BTreeMap::from([(
                "embed".to_owned(),
                CodeImageCommand {
                    argv: vec!["embed-to-svg".to_owned()],
                },
            )]),
            key_line: Some(2),
        };

        let transformed = transform_code_images(
            deck_with_spanned_code(
                "embed",
                "plain external input",
                RevealSpan { start: 1, len: 1 },
            ),
            &config,
            &svg_runner,
            &embed_renderer,
            &PanicOEmbedFetcher,
            &code_images_cache_dir,
            &embeds_cache_dir,
        )
        .unwrap();

        assert_eq!(svg_runner.calls.get(), 1);
        assert_eq!(embed_renderer.calls(), 0);
        match transformed.parsed_slides()[0].fragments[0].kind() {
            FragmentKind::Image { src, .. } => {
                assert!(src.as_str().starts_with(".peitho/code-images-cache/"));
                assert!(src.as_str().ends_with(".svg"));
            }
            kind => panic!("expected image, got {kind:?}"),
        }
    }

    #[test]
    fn embed_valid_cache_hit_skips_renderer() {
        let renderer = PanicEmbedRenderer;
        let key = seed_embed_cache(b"\x89PNG\r\n\x1a\ncached");
        assert_eq!(cache_or_render_seeded_embed(&renderer).unwrap(), key);
    }

    #[test]
    fn embed_cache_entry_empty_is_rerendered() {
        assert_embed_cache_self_heals(b"");
    }

    #[test]
    fn embed_cache_entry_non_png_is_rerendered() {
        assert_embed_cache_self_heals(b"not a png");
    }

    #[test]
    fn embed_cache_miss_renders_once_and_writes_atomically() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::EMBEDS_CACHE_DIR);
        let renderer = FixtureEmbedRenderer::png(b"\x89PNG\r\n\x1a\nfixture".to_vec());
        let status = parse_embed_block(7, "https://x.com/a/status/1")
            .unwrap()
            .target;
        let key =
            cache_or_render_embed(7, &status, BUILTIN_EMBED_PARAMS, &renderer, &cache_dir).unwrap();

        assert_eq!(renderer.calls(), 1);
        assert_eq!(renderer.urls(), vec![status.normalized_url().to_owned()]);
        assert_eq!(renderer.params(), vec![BUILTIN_EMBED_PARAMS]);
        assert_eq!(
            fs::read(cache_dir.join(format!("{key}.png"))).unwrap(),
            b"\x89PNG\r\n\x1a\nfixture"
        );
        assert_eq!(fs::read_dir(cache_dir).unwrap().count(), 1);
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

    fn math_config() -> CodeImagesConfig {
        CodeImagesConfig {
            entries: BTreeMap::from([(
                "math".to_owned(),
                CodeImageCommand {
                    argv: vec!["math-to-svg".to_owned()],
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
                step_count: 0,
                page_number_hidden: false,
                notes: None,
            }],
        )
    }

    fn deck_with_math(latex: &str) -> Deck<Parsed> {
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
                    Some("math".to_owned()),
                    latex.to_owned(),
                )],
                skip: false,
                step_count: 0,
                page_number_hidden: false,
                notes: None,
            }],
        )
    }

    fn deck_with_spanned_code(language: &str, body: &str, span: RevealSpan) -> Deck<Parsed> {
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
                    Some(language.to_owned()),
                    body.to_owned(),
                )
                .with_reveal_span(span)],
                skip: false,
                step_count: span.len,
                page_number_hidden: false,
                notes: None,
            }],
        )
    }

    fn transform_markdown_with_code_images(markdown: &str) -> Result<Deck<Parsed>> {
        let frontmatter = parse_frontmatter(markdown).unwrap();
        let config = frontmatter.settings().code_images().clone();
        let parsed = parse_markdown(
            markdown,
            frontmatter,
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();
        let temp = tempfile::tempdir().unwrap();
        transform_code_images(
            parsed,
            &config,
            &FakeRunner::svg(r#"<svg viewBox="0 0 1 1">external</svg>"#),
            &PanicEmbedRenderer,
            &PanicOEmbedFetcher,
            &temp.path().join(crate::CODE_IMAGES_CACHE_DIR),
            &temp.path().join(crate::EMBEDS_CACHE_DIR),
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
            &PanicEmbedRenderer,
            &PanicOEmbedFetcher,
            &cache_dir,
            &embed_cache_dir(&cache_dir),
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
            &PanicEmbedRenderer,
            &PanicOEmbedFetcher,
            &cache_dir,
            &embed_cache_dir(&cache_dir),
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
    fn transformed_mermaid_code_preserves_reveal_span() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        let runner = FakeRunner::svg(r#"<svg viewBox="0 0 10 10">diagram</svg>"#);
        let span = RevealSpan { start: 1, len: 1 };

        let deck = transform_code_images(
            deck_with_spanned_code("mermaid", "graph TD", span),
            &config(),
            &runner,
            &PanicEmbedRenderer,
            &PanicOEmbedFetcher,
            &cache_dir,
            &embed_cache_dir(&cache_dir),
        )
        .unwrap();
        let fragment = &deck.parsed_slides()[0].fragments[0];

        assert!(matches!(fragment.kind(), FragmentKind::Image { .. }));
        assert_eq!(fragment.reveal_span(), Some(span));
    }

    #[test]
    fn transformed_builtin_math_code_preserves_reveal_span() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        let runner = FakeRunner::svg(r#"<svg viewBox="0 0 10 10">unused</svg>"#);
        let span = RevealSpan { start: 1, len: 1 };

        let deck = transform_code_images(
            deck_with_spanned_code("math", r#"\frac{1}{2}"#, span),
            &CodeImagesConfig::default(),
            &runner,
            &PanicEmbedRenderer,
            &PanicOEmbedFetcher,
            &cache_dir,
            &embed_cache_dir(&cache_dir),
        )
        .unwrap();
        let fragment = &deck.parsed_slides()[0].fragments[0];

        assert!(matches!(fragment.kind(), FragmentKind::Math { .. }));
        assert_eq!(fragment.reveal_span(), Some(span));
    }

    #[test]
    fn code_carrying_line_emphasis_passes_through_untouched() {
        // Regression: every code fragment goes through `transform_fragment`,
        // not only diagram ones. A plain highlighted block with emphasis has
        // no renderer, so it must pass through with its annotation intact —
        // the invariant is only that emphasis never rides a fragment that gets
        // rebuilt into an image or math node.
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        let runner = FakeRunner::svg(r#"<svg viewBox="0 0 10 10">unused</svg>"#);
        let markdown = "# T\n\n```rust {1|2}\nlet a = 1;\nlet b = 2;\n```";
        let highlighter = crate::highlight::Highlighter::defaults();
        let frontmatter = parse_frontmatter(markdown).unwrap();
        let parsed = parse_markdown(markdown, frontmatter, &highlighter).unwrap();

        let deck = transform_code_images(
            parsed,
            &config(),
            &runner,
            &PanicEmbedRenderer,
            &PanicOEmbedFetcher,
            &cache_dir,
            &embed_cache_dir(&cache_dir),
        )
        .unwrap();
        let fragment = deck.parsed_slides()[0]
            .fragments
            .iter()
            .find(|fragment| matches!(fragment.kind(), FragmentKind::Code))
            .expect("the code fragment survives transformation");

        assert!(fragment.emphasis().is_some());
        // Stepped emphasis also keeps the span that places it in step space.
        assert_eq!(
            fragment.reveal_span(),
            Some(RevealSpan { start: 1, len: 2 })
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
            &PanicEmbedRenderer,
            &PanicOEmbedFetcher,
            &cache_dir,
            &embed_cache_dir(&cache_dir),
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
            &PanicEmbedRenderer,
            &PanicOEmbedFetcher,
            &cache_dir,
            &embed_cache_dir(&cache_dir),
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
            &PanicEmbedRenderer,
            &PanicOEmbedFetcher,
            &cache_dir,
            &embed_cache_dir(&cache_dir),
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
            &PanicEmbedRenderer,
            &PanicOEmbedFetcher,
            &cache_dir,
            &embed_cache_dir(&cache_dir),
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
    fn transforms_bare_math_with_builtin_renderer_to_html_fragment_without_cache() {
        let latex = r#"\frac{1}{2}"#;
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        let runner = FakeRunner::svg(r#"<svg viewBox="0 0 1 1">external</svg>"#);

        let deck = transform_code_images(
            deck_with_math(latex),
            &CodeImagesConfig::default(),
            &runner,
            &PanicEmbedRenderer,
            &PanicOEmbedFetcher,
            &cache_dir,
            &embed_cache_dir(&cache_dir),
        )
        .unwrap();
        let fragment = &deck.parsed_slides()[0].fragments[0];

        assert_eq!(runner.calls.get(), 0);
        assert_eq!(fragment.line(), 7);
        assert_eq!(fragment.markdown(), latex);
        assert_eq!(fragment.plain_text(), "");
        assert_eq!(fragment.code_text(), latex);
        match fragment.kind() {
            FragmentKind::Math { html } => {
                assert!(html.starts_with(r#"<span class="katex-display""#), "{html}");
                assert!(html.contains("mfrac"), "{html}");
            }
            other => panic!("expected math fragment, got {other:?}"),
        }
        assert!(!cache_dir.exists());
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
            &PanicEmbedRenderer,
            &PanicOEmbedFetcher,
            &cache_dir,
            &embed_cache_dir(&cache_dir),
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
    fn explicit_math_entry_overrides_builtin_renderer_with_external_svg_path() {
        let latex = r#"\frac{1}{2}"#;
        let key = code_image_cache_key(math_config().entries.get("math").unwrap(), latex);
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        let runner = FakeRunner::svg(r#"<svg viewBox="0 0 10 10">external math</svg>"#);

        let deck = transform_code_images(
            deck_with_math(latex),
            &math_config(),
            &runner,
            &PanicEmbedRenderer,
            &PanicOEmbedFetcher,
            &cache_dir,
            &embed_cache_dir(&cache_dir),
        )
        .unwrap();

        assert_eq!(runner.calls.get(), 1);
        assert_eq!(
            fs::read(cache_dir.join(format!("{key}.svg"))).unwrap(),
            br#"<svg viewBox="0 0 10 10" width="10" height="10">external math</svg>"#
        );
        match deck.parsed_slides()[0].fragments[0].kind() {
            FragmentKind::Image { alt, src } => {
                assert_eq!(alt, "diagram (math)");
                assert_eq!(
                    src.as_str(),
                    format!("{}/{key}.svg", crate::CODE_IMAGES_CACHE_DIR)
                );
            }
            other => panic!("expected image fragment, got {other:?}"),
        }
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
            &PanicEmbedRenderer,
            &PanicOEmbedFetcher,
            &cache_dir,
            &embed_cache_dir(&cache_dir),
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
    fn builtin_math_render_error_reports_line_and_override_help() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        let runner = FakeRunner::svg(r#"<svg viewBox="0 0 1 1">external</svg>"#);

        let err = match transform_code_images(
            deck_with_math(r#"\frac{1}{"#),
            &CodeImagesConfig::default(),
            &runner,
            &PanicEmbedRenderer,
            &PanicOEmbedFetcher,
            &cache_dir,
            &embed_cache_dir(&cache_dir),
        ) {
            Ok(_) => panic!("expected built-in math render failure"),
            Err(err) => err,
        };

        assert_eq!(runner.calls.get(), 0);
        assert_eq!(err.kind, ErrorKind::Asset);
        assert_eq!(err.line, Some(7));
        assert!(err.message.contains("code_images 'math' failed"));
        assert!(err.message.contains("KaTeX parse error"));
        assert!(err.message.contains("expected '}'"));
        assert_eq!(err.help, builtin_math_override_help());
        assert!(!cache_dir.exists());
    }

    #[test]
    fn empty_builtin_math_fences_are_line_numbered_errors_but_comment_input_is_allowed() {
        for markdown in [
            "# Intro\n\n```math\n```\n",
            "# Intro\n\n```math\n  \n\t\n```\n",
        ] {
            let err = match transform_markdown_with_code_images(markdown) {
                Ok(_) => panic!("expected empty built-in math render failure"),
                Err(err) => err,
            };

            assert_eq!(err.kind, ErrorKind::Asset);
            assert_eq!(err.line, Some(3));
            assert_eq!(
                err.message,
                "code_images 'math' failed: math block is empty"
            );
            assert_eq!(err.help, builtin_math_override_help());
        }

        transform_markdown_with_code_images("# Intro\n\n```math\n% note\n```\n").unwrap();
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
            &PanicEmbedRenderer,
            &PanicOEmbedFetcher,
            &cache_dir,
            &embed_cache_dir(&cache_dir),
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
    fn builtin_math_renderer_panic_becomes_error_message() {
        let err = render_builtin_math_with(|| {
            panic!("boom");
        })
        .unwrap_err();

        assert_eq!(err, "built-in math renderer panicked: boom");
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
            &PanicEmbedRenderer,
            &PanicOEmbedFetcher,
            &cache_dir,
            &embed_cache_dir(&cache_dir),
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
            &PanicEmbedRenderer,
            &PanicOEmbedFetcher,
            &cache_dir,
            &embed_cache_dir(&cache_dir),
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
            &PanicEmbedRenderer,
            &PanicOEmbedFetcher,
            &cache_dir,
            &embed_cache_dir(&cache_dir),
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
            &PanicEmbedRenderer,
            &PanicOEmbedFetcher,
            &cache_dir,
            &embed_cache_dir(&cache_dir),
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
            &PanicEmbedRenderer,
            &PanicOEmbedFetcher,
            &cache_dir,
            &embed_cache_dir(&cache_dir),
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
                &PanicEmbedRenderer,
                &PanicOEmbedFetcher,
                &cache_dir,
                &embed_cache_dir(&cache_dir),
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
            &PanicEmbedRenderer,
            &PanicOEmbedFetcher,
            &cache_dir,
            &embed_cache_dir(&cache_dir),
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
            &PanicEmbedRenderer,
            &PanicOEmbedFetcher,
            &cache_dir,
            &embed_cache_dir(&cache_dir),
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
            &PanicEmbedRenderer,
            &PanicOEmbedFetcher,
            &cache_dir,
            &embed_cache_dir(&cache_dir),
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
            &PanicEmbedRenderer,
            &PanicOEmbedFetcher,
            &cache_dir,
            &embed_cache_dir(&cache_dir),
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
            &PanicEmbedRenderer,
            &PanicOEmbedFetcher,
            &cache_dir,
            &embed_cache_dir(&cache_dir),
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
            &PanicEmbedRenderer,
            &PanicOEmbedFetcher,
            &temp.path().join(crate::CODE_IMAGES_CACHE_DIR),
            &temp.path().join(crate::EMBEDS_CACHE_DIR),
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
            &PanicEmbedRenderer,
            &PanicOEmbedFetcher,
            &temp.path().join(crate::CODE_IMAGES_CACHE_DIR),
            &temp.path().join(crate::EMBEDS_CACHE_DIR),
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
    fn math_inside_explicit_slot_group_maps_and_checks_as_math_fragment() {
        let layout = parse_layout(
            "explicit-slots",
            r#"<section>
               <slot name="title" accepts="inline" arity="1"></slot>
               <slot name="body" accepts="blocks" arity="0..*"></slot>
               <slot name="code" accepts="code" arity="0..1"></slot>
               </section>"#,
        )
        .unwrap();
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        let runner = FakeRunner::svg(r#"<svg viewBox="0 0 10 10">unused</svg>"#);

        let body_markdown = "# Intro\n\n::: {slot=body}\n\n```math\n\\frac{1}{2}\n```\n:::\n";
        let body_frontmatter = parse_frontmatter(body_markdown).unwrap();
        let body_config = body_frontmatter.settings().code_images().clone();
        let body_parsed = parse_markdown(
            body_markdown,
            body_frontmatter,
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();
        let body_transformed = transform_code_images(
            body_parsed,
            &body_config,
            &runner,
            &PanicEmbedRenderer,
            &PanicOEmbedFetcher,
            &cache_dir,
            &embed_cache_dir(&cache_dir),
        )
        .unwrap();
        let body_checked =
            check_deck(map_by_convention(body_transformed, &layout).unwrap()).unwrap();
        let body = SlotName::new("body").unwrap();

        match body_checked.checked_slides()[0].slots()[&body].fragments()[0].kind() {
            FragmentKind::Math { html } => {
                assert!(html.starts_with(r#"<span class="katex-display""#));
            }
            other => panic!("expected body slot math fragment, got {other:?}"),
        }

        let code_markdown = "# Intro\n\n::: {slot=code}\n\n```math\n\\frac{1}{2}\n```\n:::\n";
        let code_frontmatter = parse_frontmatter(code_markdown).unwrap();
        let code_config = code_frontmatter.settings().code_images().clone();
        let code_parsed = parse_markdown(
            code_markdown,
            code_frontmatter,
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();
        let code_transformed = transform_code_images(
            code_parsed,
            &code_config,
            &runner,
            &PanicEmbedRenderer,
            &PanicOEmbedFetcher,
            &cache_dir,
            &embed_cache_dir(&cache_dir),
        )
        .unwrap();
        let err = check_deck(map_by_convention(code_transformed, &layout).unwrap()).unwrap_err();

        assert_eq!(err.kind, ErrorKind::Accepts);
        assert_eq!(err.line, Some(5));
        assert!(err.to_string().contains("slot 'code' accepts code"));
        assert_eq!(
            err.help,
            "change the layout accepts to 'blocks' or move this content to a blocks slot"
        );
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
            &PanicEmbedRenderer,
            &PanicOEmbedFetcher,
            &cache_dir,
            &embed_cache_dir(&cache_dir),
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
