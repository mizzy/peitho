use std::{
    fmt,
    path::{Component, Path, PathBuf},
    str::FromStr,
};

use serde::{Deserialize, Deserializer, Serialize};

/// Slide canvas aspect ratio.
///
/// One of `16:9` (default) or `4:3`. Drives the logical width/height of
/// every slide and the presenter view's proportions. Wire form on
/// `manifest.json` is the label string; internal pixel dimensions are looked
/// up via [`AspectRatio::width`] / [`AspectRatio::height`].
#[cfg_attr(any(test, feature = "ts-bindings"), derive(ts_rs::TS))]
#[cfg_attr(
    any(test, feature = "ts-bindings"),
    ts(export, export_to = "../../bindings/")
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AspectRatio {
    #[serde(rename = "16:9")]
    #[default]
    Ratio16To9,
    #[serde(rename = "4:3")]
    Ratio4To3,
}

impl AspectRatio {
    /// Logical canvas width in pixels for this ratio.
    pub fn width(self) -> u32 {
        match self {
            Self::Ratio16To9 => 1280,
            Self::Ratio4To3 => 960,
        }
    }

    /// Logical canvas height in pixels for this ratio.
    pub fn height(self) -> u32 {
        match self {
            Self::Ratio16To9 | Self::Ratio4To3 => 720,
        }
    }

    /// CSS `aspect-ratio` value string for this ratio (e.g. `"16 / 9"`).
    pub(crate) fn css_aspect_value(self) -> &'static str {
        match self {
            Self::Ratio16To9 => "16 / 9",
            Self::Ratio4To3 => "4 / 3",
        }
    }
}

impl FromStr for AspectRatio {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "16:9" => Ok(Self::Ratio16To9),
            "4:3" => Ok(Self::Ratio4To3),
            _ => Err(format!("unknown aspect_ratio '{value}'")),
        }
    }
}

impl fmt::Display for AspectRatio {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Ratio16To9 => "16:9",
            Self::Ratio4To3 => "4:3",
        })
    }
}

/// A physical PDF page size in CSS pixels (96 dpi).
///
/// Constructed only via validated constructors, so raw `(u32, u32)` pairs
/// cannot masquerade as a checked PDF resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Resolution {
    width: u32,
    height: u32,
}

impl Resolution {
    /// Frontmatter parse: accepts only `WxH` pixel dimensions with non-zero
    /// `u32` width and height.
    pub fn from_frontmatter(raw: &str) -> Result<Self, String> {
        let (width_raw, height_raw) = raw
            .split_once('x')
            .filter(|(_, height)| !height.contains('x'))
            .ok_or_else(|| "resolution must use WxH pixel format".to_owned())?;
        if width_raw.is_empty() || height_raw.is_empty() {
            return Err("resolution must use WxH pixel format".to_owned());
        }
        let width = parse_resolution_dimension("width", width_raw)?;
        let height = parse_resolution_dimension("height", height_raw)?;

        Ok(Self { width, height })
    }

    pub fn from_aspect_ratio_default(ratio: AspectRatio) -> Self {
        match ratio {
            AspectRatio::Ratio16To9 => Self {
                width: 1920,
                height: 1080,
            },
            AspectRatio::Ratio4To3 => Self {
                width: 1440,
                height: 1080,
            },
        }
    }

    pub fn width(self) -> u32 {
        self.width
    }

    pub fn height(self) -> u32 {
        self.height
    }

    pub(crate) fn check_matches(self, ratio: AspectRatio) -> Result<(), String> {
        let resolution_width = u64::from(self.width);
        let resolution_height = u64::from(self.height);
        let ratio_width = u64::from(ratio.width());
        let ratio_height = u64::from(ratio.height());
        if resolution_width * ratio_height == resolution_height * ratio_width {
            return Ok(());
        }
        Err(format!(
            "resolution {}x{} does not match aspect_ratio {}",
            self.width, self.height, ratio
        ))
    }

    pub(crate) fn check_not_smaller_than_canvas(self, ratio: AspectRatio) -> Result<(), String> {
        if self.width >= ratio.width() && self.height >= ratio.height() {
            return Ok(());
        }
        Err(format!(
            "resolution {}x{} is smaller than the canvas logical size {}x{}; use at least the canvas dimensions",
            self.width,
            self.height,
            ratio.width(),
            ratio.height()
        ))
    }
}

impl TryFrom<String> for Resolution {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::from_frontmatter(&value)
    }
}

impl From<Resolution> for String {
    fn from(resolution: Resolution) -> Self {
        format!("{}x{}", resolution.width, resolution.height)
    }
}

impl Default for Resolution {
    fn default() -> Self {
        Self::from_aspect_ratio_default(AspectRatio::default())
    }
}

fn parse_resolution_dimension(label: &str, raw: &str) -> Result<u32, String> {
    let value = raw
        .parse::<u32>()
        .map_err(|_| format!("resolution {label} `{raw}` is not a valid u32"))?;
    if value == 0 {
        return Err(format!("resolution {label} must be greater than zero"));
    }
    Ok(value)
}

#[cfg_attr(any(test, feature = "ts-bindings"), derive(ts_rs::TS))]
#[cfg_attr(any(test, feature = "ts-bindings"), ts(type = "string"))]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct SlideKey(String);

impl SlideKey {
    pub fn new(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        let valid = value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        if value.is_empty() || !valid {
            return Err("slide key must use lowercase ascii, digits, or '-'".to_owned());
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for SlideKey {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
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

/// A deck-relative image path exactly as accepted from Markdown.
///
/// `RawImagePath` is intentionally distinct from [`ResolvedImagePath`]:
/// raw paths may only be mapped and resolved, never rendered into HTML.
/// Construction validates that the path is local to the deck text and uses
/// one of Peitho's supported image extensions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawImagePath(String);

/// A distribution-relative image path that is safe to render as `<img src>`.
///
/// Values are produced by the image resolver after it has copied or otherwise
/// reserved the asset under `assets/<hash>-<basename>`. Rendering APIs accept
/// this type, not [`RawImagePath`], so callers cannot accidentally emit a raw
/// author-written path into generated HTML.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedImagePath(String);

impl ResolvedImagePath {
    // Test-only escape hatch. Do not make this public; it bypasses hashed
    // asset validation and the resolver boundary.
    #[cfg(test)]
    pub(crate) fn from_string(value: String) -> Self {
        Self(value)
    }

    /// Build a resolved `assets/<hash>-<basename>` path from resolver output.
    ///
    /// The hash must be the fixed 16-hex prefix chosen by the CLI resolver, and
    /// the basename must not contain path separators or URL delimiters.
    pub fn from_hashed_asset(hash: &str, basename: &str) -> Result<Self, String> {
        let valid_hash = hash.len() == 16 && hash.chars().all(|c| c.is_ascii_hexdigit());
        if !valid_hash {
            return Err("image asset hash must be 16 hex characters".to_owned());
        }
        let valid_basename = !basename.is_empty()
            && !basename.contains('/')
            && !basename.contains('\\')
            && !basename.contains('?')
            && !basename.contains('#')
            && Path::new(basename)
                .file_name()
                .and_then(|name| name.to_str())
                == Some(basename);
        if !valid_basename {
            return Err(
                "image asset basename must not contain path separators, queries, or fragments"
                    .to_owned(),
            );
        }
        Ok(Self(format!("assets/{hash}-{basename}")))
    }

    /// Return the distribution-relative path to use in generated HTML.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A resolved image asset that the CLI must materialize into the distribution.
///
/// Core records the source file chosen by the resolver and the typed
/// distribution-relative path used by rendered slides and `manifest.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedImageAsset {
    /// Canonical source path used by the CLI copy phase.
    pub source_abs: PathBuf,
    /// Typed `assets/...` path used in rendered HTML and the manifest.
    pub dist_rel: ResolvedImagePath,
}

#[cfg_attr(any(test, feature = "ts-bindings"), derive(ts_rs::TS))]
#[cfg_attr(
    any(test, feature = "ts-bindings"),
    ts(export, export_to = "../../bindings/")
)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeasuredDeck {
    pub canvas_width: f64,
    pub canvas_height: f64,
    pub slides: Vec<MeasuredSlide>,
}

#[cfg_attr(any(test, feature = "ts-bindings"), derive(ts_rs::TS))]
#[cfg_attr(
    any(test, feature = "ts-bindings"),
    ts(export, export_to = "../../bindings/")
)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeasuredSlide {
    pub key: String,
    pub background_color: String,
    pub boxes: Vec<MeasuredBox>,
    pub images: Vec<MeasuredImage>,
}

#[cfg_attr(any(test, feature = "ts-bindings"), derive(ts_rs::TS))]
#[cfg_attr(
    any(test, feature = "ts-bindings"),
    ts(export, export_to = "../../bindings/")
)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeasuredBox {
    pub slot: String,
    pub rect: MeasuredRect,
    pub style: MeasuredBoxStyle,
    pub paragraphs: Vec<MeasuredParagraph>,
}

#[cfg_attr(any(test, feature = "ts-bindings"), derive(ts_rs::TS))]
#[cfg_attr(
    any(test, feature = "ts-bindings"),
    ts(export, export_to = "../../bindings/")
)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeasuredBoxStyle {
    pub background_color: String,
    pub border_color: String,
    pub border_width: f64,
    pub border_radius: f64,
}

#[cfg_attr(any(test, feature = "ts-bindings"), derive(ts_rs::TS))]
#[cfg_attr(
    any(test, feature = "ts-bindings"),
    ts(export, export_to = "../../bindings/")
)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeasuredParagraph {
    pub align: String,
    pub bullet_level: Option<u8>,
    #[serde(default)]
    pub numbered: bool,
    #[serde(default)]
    pub bullet_continuation: bool,
    #[serde(default)]
    pub numbering_start_at: Option<u16>,
    pub runs: Vec<MeasuredRun>,
}

#[cfg_attr(any(test, feature = "ts-bindings"), derive(ts_rs::TS))]
#[cfg_attr(
    any(test, feature = "ts-bindings"),
    ts(export, export_to = "../../bindings/")
)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeasuredRun {
    pub text: String,
    pub color: String,
    pub font_family: String,
    pub font_size_px: f64,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    #[serde(default)]
    pub breaks_before: u32,
}

#[cfg_attr(any(test, feature = "ts-bindings"), derive(ts_rs::TS))]
#[cfg_attr(
    any(test, feature = "ts-bindings"),
    ts(export, export_to = "../../bindings/")
)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeasuredImage {
    pub src: String,
    pub alt: String,
    pub rect: MeasuredRect,
}

#[cfg_attr(any(test, feature = "ts-bindings"), derive(ts_rs::TS))]
#[cfg_attr(
    any(test, feature = "ts-bindings"),
    ts(export, export_to = "../../bindings/")
)]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeasuredRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

const SUPPORTED_IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp"];
const SUPPORTED_IMAGE_EXTENSIONS_TEXT: &str = "png, jpg, jpeg, gif, webp";

impl RawImagePath {
    /// Image extensions accepted by Markdown parsing.
    pub const SUPPORTED_EXTENSIONS: &'static [&'static str] = SUPPORTED_IMAGE_EXTENSIONS;

    /// Validate a Markdown image path and keep it in raw deck-relative form.
    ///
    /// Remote URLs, absolute paths, parent-directory escapes, empty paths, and
    /// unsupported extensions are rejected with a line-numbered parse error.
    pub fn new(raw: impl Into<String>, line: usize) -> crate::error::Result<Self> {
        let value = raw.into();
        if value.is_empty() {
            return Err(image_path_error(
                line,
                "empty image path",
                "write a local deck-relative image path",
            ));
        }
        if is_remote_image_url(&value) {
            return Err(image_path_error(
                line,
                "remote image URLs are not supported",
                "store the image next to the deck and reference it with a relative path",
            ));
        }
        if is_absolute_image_path(&value) {
            return Err(image_path_error(
                line,
                "absolute image paths are not supported",
                "reference the image with a path relative to the deck file",
            ));
        }
        if value.contains('\\') {
            return Err(image_path_error(
                line,
                "image paths must use forward slashes",
                "write deck-relative paths like img/diagram.png",
            ));
        }
        if value.contains('?') {
            return Err(image_path_error(
                line,
                "image path query strings are not supported",
                "reference the local image file without URL query parameters",
            ));
        }
        if value.contains('#') {
            return Err(image_path_error(
                line,
                "image path fragments are not supported",
                "reference the local image file without URL fragments",
            ));
        }
        let path = Path::new(&value);
        if path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(image_path_error(
                line,
                "image path escapes deck directory",
                "keep image paths inside the deck directory",
            ));
        }
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default();
        let extension_lower = extension.to_ascii_lowercase();
        if !Self::SUPPORTED_EXTENSIONS.contains(&extension_lower.as_str()) {
            return Err(image_path_error(
                line,
                format!(
                    "unsupported image extension '{extension}'; supported: {SUPPORTED_IMAGE_EXTENSIONS_TEXT}"
                ),
                "use a supported local image file",
            ));
        }
        Ok(Self(value))
    }

    // Test-only escape hatch. Do not make this public; parser entry points
    // must use `new()` so raw Markdown paths are validated before they enter
    // the pipeline.
    #[cfg(test)]
    pub(crate) fn new_unchecked(value: String) -> Self {
        Self(value)
    }

    /// Return the original deck-relative path.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn image_path_error(
    line: usize,
    message: impl Into<String>,
    help: impl Into<String>,
) -> crate::error::BuildError {
    crate::error::BuildError::new(crate::error::ErrorKind::Parse, Some(line), message, help)
}

fn is_remote_image_url(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.starts_with("//")
        || lower.starts_with("http:")
        || lower.starts_with("https:")
        || lower.starts_with("file:")
        || lower.starts_with("data:")
        || lower.starts_with("javascript:")
}

fn is_absolute_image_path(value: &str) -> bool {
    value.starts_with('/') || value.starts_with('~') || has_windows_drive_prefix(value)
}

fn has_windows_drive_prefix(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
}

/// A slot name that was produced by the explicit `::: {slot=name}` syntax.
///
/// The type is public so it can appear in `FragmentKind::SlotGroup`, but the
/// constructor is deliberately `pub(crate)`: outside callers cannot fabricate
/// one, so any future consumer that needs to route by explicit intent must go
/// through the parser. This preserves the invariant that convention-derived
/// and author-declared slot names cannot be silently interchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplicitSlot(SlotName);

impl ExplicitSlot {
    pub(crate) fn new(name: SlotName) -> Self {
        Self(name)
    }

    pub fn as_slot_name(&self) -> &SlotName {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FragmentKind<S = RawImagePath> {
    Heading {
        level: u8,
    },
    Paragraph,
    Text,
    Code,
    Image {
        alt: String,
        src: S,
    },
    List,
    /// A `::: {slot=name}` fenced block. Mapping expands the children into
    /// the named slot; SlotGroup itself never leaves the Mapped phase.
    SlotGroup {
        name: ExplicitSlot,
        children: Vec<SourceFragment<S>>,
    },
}

impl<S> FragmentKind<S> {
    pub fn default_accepts(&self) -> Accepts {
        match self {
            Self::Heading { .. } => Accepts::Inline,
            Self::Paragraph => Accepts::Blocks,
            Self::Text => Accepts::Text,
            Self::Code => Accepts::Code,
            Self::Image { .. } => Accepts::Image,
            Self::List => Accepts::List,
            // SlotGroup is expanded in mapping, so its own accepts value never
            // reaches contract checks. Report a conservative blocks default so
            // any accidental reader gets a stable answer instead of unreachable!.
            Self::SlotGroup { .. } => Accepts::Blocks,
        }
    }

    pub fn removal_noun(&self) -> &'static str {
        match self {
            Self::Heading { .. } => "heading",
            Self::Paragraph => "paragraph",
            Self::Text => "text block",
            Self::Code => "code block",
            Self::Image { .. } => "image",
            Self::List => "list",
            Self::SlotGroup { .. } => "slot group",
        }
    }
}

impl<S> fmt::Display for FragmentKind<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Heading { .. } => "heading",
            Self::Paragraph => "paragraph",
            Self::Text => "text",
            Self::Code => "code",
            Self::Image { .. } => "image",
            Self::List => "list",
            Self::SlotGroup { .. } => "slot group",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFragment<S = RawImagePath> {
    line: usize,
    kind: FragmentKind<S>,
    markdown: String,
    text: String,
    code: String,
    language: Option<String>,
}

impl SourceFragment<RawImagePath> {
    pub fn heading(
        line: usize,
        level: u8,
        markdown: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        Self {
            line,
            kind: FragmentKind::Heading { level },
            markdown: markdown.into(),
            text: text.into(),
            code: String::new(),
            language: None,
        }
    }

    pub fn paragraph(line: usize, markdown: impl Into<String>) -> Self {
        Self {
            line,
            kind: FragmentKind::Paragraph,
            markdown: markdown.into(),
            text: String::new(),
            code: String::new(),
            language: None,
        }
    }

    pub fn list(line: usize, markdown: impl Into<String>) -> Self {
        Self {
            line,
            kind: FragmentKind::List,
            markdown: markdown.into(),
            text: String::new(),
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

    pub(crate) fn slot_group(
        line: usize,
        name: ExplicitSlot,
        children: Vec<SourceFragment<RawImagePath>>,
    ) -> Self {
        Self {
            line,
            kind: FragmentKind::SlotGroup { name, children },
            markdown: String::new(),
            text: String::new(),
            code: String::new(),
            language: None,
        }
    }
}

impl<S> SourceFragment<S> {
    pub fn image(line: usize, alt: impl Into<String>, src: S) -> Self {
        Self {
            line,
            kind: FragmentKind::Image {
                alt: alt.into(),
                src,
            },
            markdown: String::new(),
            text: String::new(),
            code: String::new(),
            language: None,
        }
    }

    pub(crate) fn try_map_image_src<T, E, F>(
        self,
        mut f: F,
    ) -> std::result::Result<SourceFragment<T>, E>
    where
        F: FnMut(S) -> std::result::Result<T, E>,
    {
        self.try_map_image_src_inner(&mut f)
    }

    fn try_map_image_src_inner<T, E>(
        self,
        f: &mut dyn FnMut(S) -> std::result::Result<T, E>,
    ) -> std::result::Result<SourceFragment<T>, E> {
        let kind = match self.kind {
            FragmentKind::Heading { level } => FragmentKind::Heading { level },
            FragmentKind::Paragraph => FragmentKind::Paragraph,
            FragmentKind::Text => FragmentKind::Text,
            FragmentKind::Code => FragmentKind::Code,
            FragmentKind::Image { alt, src } => FragmentKind::Image { alt, src: f(src)? },
            FragmentKind::List => FragmentKind::List,
            FragmentKind::SlotGroup { name, children } => {
                let mut mapped_children = Vec::with_capacity(children.len());
                for child in children {
                    mapped_children.push(child.try_map_image_src_inner(f)?);
                }
                FragmentKind::SlotGroup {
                    name,
                    children: mapped_children,
                }
            }
        };
        Ok(SourceFragment {
            line: self.line,
            kind,
            markdown: self.markdown,
            text: self.text,
            code: self.code,
            language: self.language,
        })
    }

    pub fn line(&self) -> usize {
        self.line
    }

    pub fn kind(&self) -> &FragmentKind<S> {
        &self.kind
    }

    pub fn markdown(&self) -> &str {
        &self.markdown
    }

    pub fn plain_text(&self) -> &str {
        &self.text
    }

    pub fn code_text(&self) -> &str {
        &self.code
    }

    pub fn language(&self) -> Option<&str> {
        self.language.as_deref()
    }

    pub fn heading_text(&self) -> Option<String> {
        matches!(&self.kind, FragmentKind::Heading { .. }).then(|| self.text.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedSlide {
    index: usize,
    key: SlideKey,
    html: String,
    notes: Option<String>,
}

impl RenderedSlide {
    pub(crate) fn new(index: usize, key: SlideKey, html: String, notes: Option<String>) -> Self {
        Self {
            index,
            key,
            html,
            notes,
        }
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

    pub fn notes(&self) -> Option<&str> {
        self.notes.as_deref()
    }

    pub fn src(&self) -> String {
        crate::manifest::fragment_src(self.index, &self.key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aspect_ratio_constants_define_canvas_dimensions() {
        assert_eq!(AspectRatio::Ratio16To9.width(), 1280);
        assert_eq!(AspectRatio::Ratio16To9.height(), 720);
        assert_eq!(AspectRatio::Ratio4To3.width(), 960);
        assert_eq!(AspectRatio::Ratio4To3.height(), 720);
        assert_eq!(AspectRatio::default(), AspectRatio::Ratio16To9);
    }

    #[test]
    fn aspect_ratio_serializes_as_semantic_string() {
        assert_eq!(
            serde_json::to_string(&AspectRatio::Ratio16To9).unwrap(),
            r#""16:9""#
        );
        assert_eq!(
            serde_json::to_string(&AspectRatio::Ratio4To3).unwrap(),
            r#""4:3""#
        );
    }

    #[test]
    fn aspect_ratio_deserializes_semantic_string() {
        assert_eq!(
            serde_json::from_str::<AspectRatio>(r#""16:9""#).unwrap(),
            AspectRatio::Ratio16To9
        );
        assert_eq!(
            serde_json::from_str::<AspectRatio>(r#""4:3""#).unwrap(),
            AspectRatio::Ratio4To3
        );
    }

    #[test]
    fn aspect_ratio_parses_labels_from_str() {
        assert_eq!(
            "16:9".parse::<AspectRatio>().unwrap(),
            AspectRatio::Ratio16To9
        );
        assert_eq!(
            "4:3".parse::<AspectRatio>().unwrap(),
            AspectRatio::Ratio4To3
        );
        assert!("16:10".parse::<AspectRatio>().is_err());
    }

    #[test]
    fn aspect_ratio_rejects_unknown_wire_string() {
        let err = serde_json::from_str::<AspectRatio>(r#""16:10""#).unwrap_err();

        assert!(err.to_string().contains("unknown variant"));
    }

    #[test]
    fn aspect_ratio_rejects_pixel_dimension_object() {
        assert!(serde_json::from_str::<AspectRatio>(r#"{"width":1280,"height":720}"#).is_err());
    }

    #[test]
    fn aspect_ratio_display_matches_frontmatter_label() {
        assert_eq!(AspectRatio::Ratio16To9.to_string(), "16:9");
        assert_eq!(AspectRatio::Ratio4To3.to_string(), "4:3");
    }

    #[test]
    fn resolution_from_frontmatter_accepts_wxh_pixels() {
        let resolution = Resolution::from_frontmatter("1920x1080").unwrap();

        assert_eq!(resolution.width(), 1920);
        assert_eq!(resolution.height(), 1080);
    }

    #[test]
    fn resolution_from_frontmatter_rejects_invalid_shape() {
        for raw in ["", "1920", "1920x1080x2"] {
            let err = Resolution::from_frontmatter(raw).unwrap_err();

            assert_eq!(err, "resolution must use WxH pixel format");
        }
    }

    #[test]
    fn resolution_from_frontmatter_rejects_non_numeric_dimensions() {
        let err = Resolution::from_frontmatter("abcx1080").unwrap_err();
        assert_eq!(err, "resolution width `abc` is not a valid u32");

        let err = Resolution::from_frontmatter("1920xdef").unwrap_err();
        assert_eq!(err, "resolution height `def` is not a valid u32");
    }

    #[test]
    fn resolution_from_frontmatter_rejects_u32_overflow() {
        let err = Resolution::from_frontmatter("9999999999x1080").unwrap_err();
        assert_eq!(err, "resolution width `9999999999` is not a valid u32");

        let err = Resolution::from_frontmatter("1080x9999999999").unwrap_err();
        assert_eq!(err, "resolution height `9999999999` is not a valid u32");
    }

    #[test]
    fn resolution_from_frontmatter_rejects_zero_dimensions() {
        let err = Resolution::from_frontmatter("0x1080").unwrap_err();
        assert_eq!(err, "resolution width must be greater than zero");

        let err = Resolution::from_frontmatter("1920x0").unwrap_err();
        assert_eq!(err, "resolution height must be greater than zero");
    }

    #[test]
    fn resolution_defaults_to_high_dpi_canvas_for_aspect_ratio() {
        let widescreen = Resolution::from_aspect_ratio_default(AspectRatio::Ratio16To9);
        assert_eq!(widescreen.width(), 1920);
        assert_eq!(widescreen.height(), 1080);

        let classic = Resolution::from_aspect_ratio_default(AspectRatio::Ratio4To3);
        assert_eq!(classic.width(), 1440);
        assert_eq!(classic.height(), 1080);
    }

    #[test]
    fn resolution_default_uses_default_aspect_ratio() {
        let resolution = Resolution::default();

        assert_eq!(resolution.width(), 1920);
        assert_eq!(resolution.height(), 1080);
    }

    #[test]
    fn resolution_check_matches_accepts_same_aspect_ratio() {
        Resolution::from_frontmatter("1920x1080")
            .unwrap()
            .check_matches(AspectRatio::Ratio16To9)
            .unwrap();
        Resolution::from_frontmatter("1440x1080")
            .unwrap()
            .check_matches(AspectRatio::Ratio4To3)
            .unwrap();
    }

    #[test]
    fn resolution_check_matches_rejects_mismatched_aspect_ratio() {
        let err = Resolution::from_frontmatter("1024x768")
            .unwrap()
            .check_matches(AspectRatio::Ratio16To9)
            .unwrap_err();

        assert_eq!(err, "resolution 1024x768 does not match aspect_ratio 16:9");
    }

    #[test]
    fn resolution_serializes_as_wxh_string() {
        let resolution = Resolution::from_frontmatter("1920x1080").unwrap();

        assert_eq!(
            serde_json::to_string(&resolution).unwrap(),
            r#""1920x1080""#
        );
    }

    #[test]
    fn resolution_deserializes_through_validated_wxh_string() {
        let resolution = serde_json::from_str::<Resolution>(r#""1440x1080""#).unwrap();

        assert_eq!(resolution.width(), 1440);
        assert_eq!(resolution.height(), 1080);
    }

    #[test]
    fn resolution_deserialization_rejects_invalid_wxh_string() {
        let err = serde_json::from_str::<Resolution>(r#""9999999999x1080""#).unwrap_err();

        assert!(err
            .to_string()
            .contains("resolution width `9999999999` is not a valid u32"));
    }

    #[test]
    fn measured_deck_serializes_with_browser_contract_field_names() {
        let deck = MeasuredDeck {
            canvas_width: 1280.0,
            canvas_height: 720.0,
            slides: vec![MeasuredSlide {
                key: "intro".to_owned(),
                background_color: "rgb(255, 255, 255)".to_owned(),
                boxes: vec![MeasuredBox {
                    slot: "title".to_owned(),
                    rect: MeasuredRect {
                        x: 96.0,
                        y: 80.0,
                        w: 640.0,
                        h: 120.0,
                    },
                    style: MeasuredBoxStyle {
                        background_color: "rgba(0, 0, 0, 0)".to_owned(),
                        border_color: "rgb(0, 0, 0)".to_owned(),
                        border_width: 2.0,
                        border_radius: 8.0,
                    },
                    paragraphs: vec![MeasuredParagraph {
                        align: "center".to_owned(),
                        bullet_level: Some(1),
                        numbered: true,
                        bullet_continuation: false,
                        numbering_start_at: Some(3),
                        runs: vec![MeasuredRun {
                            text: "Intro".to_owned(),
                            color: "rgb(34, 34, 34)".to_owned(),
                            font_family: "Inter".to_owned(),
                            font_size_px: 56.0,
                            bold: true,
                            italic: false,
                            underline: true,
                            breaks_before: 2,
                        }],
                    }],
                }],
                images: vec![MeasuredImage {
                    src: "assets/0123456789abcdef-arch.png".to_owned(),
                    alt: "Architecture".to_owned(),
                    rect: MeasuredRect {
                        x: 700.0,
                        y: 120.0,
                        w: 420.0,
                        h: 240.0,
                    },
                }],
            }],
        };

        let json = serde_json::to_value(&deck).unwrap();

        assert_eq!(json["canvasWidth"], 1280.0);
        assert_eq!(json["canvasHeight"], 720.0);
        assert_eq!(json["slides"][0]["backgroundColor"], "rgb(255, 255, 255)");
        assert_eq!(json["slides"][0]["boxes"][0]["style"]["borderWidth"], 2.0);
        assert_eq!(
            json["slides"][0]["boxes"][0]["paragraphs"][0]["bulletLevel"],
            1
        );
        assert_eq!(
            json["slides"][0]["boxes"][0]["paragraphs"][0]["numbered"],
            true
        );
        assert_eq!(
            json["slides"][0]["boxes"][0]["paragraphs"][0]["bulletContinuation"],
            false
        );
        assert_eq!(
            json["slides"][0]["boxes"][0]["paragraphs"][0]["numberingStartAt"],
            3
        );
        assert_eq!(
            json["slides"][0]["boxes"][0]["paragraphs"][0]["runs"][0]["fontFamily"],
            "Inter"
        );
        assert_eq!(
            json["slides"][0]["boxes"][0]["paragraphs"][0]["runs"][0]["fontSizePx"],
            56.0
        );
        assert_eq!(
            json["slides"][0]["boxes"][0]["paragraphs"][0]["runs"][0]["breaksBefore"],
            2
        );

        let round_tripped: MeasuredDeck = serde_json::from_value(json).unwrap();
        assert_eq!(
            round_tripped.slides[0].boxes[0].paragraphs[0].bullet_level,
            Some(1)
        );
        assert!(round_tripped.slides[0].boxes[0].paragraphs[0].numbered);
        assert!(!round_tripped.slides[0].boxes[0].paragraphs[0].bullet_continuation);
        assert_eq!(
            round_tripped.slides[0].boxes[0].paragraphs[0].numbering_start_at,
            Some(3)
        );
    }

    #[test]
    fn exports_measured_bindings_with_camel_case_contract() {
        use std::{fs, path::Path};

        use ts_rs::{Config, TS};

        let cfg = Config::from_env();
        MeasuredDeck::export_all(&cfg).unwrap();
        MeasuredSlide::export_all(&cfg).unwrap();
        MeasuredBox::export_all(&cfg).unwrap();
        MeasuredBoxStyle::export_all(&cfg).unwrap();
        MeasuredParagraph::export_all(&cfg).unwrap();
        MeasuredRun::export_all(&cfg).unwrap();
        MeasuredImage::export_all(&cfg).unwrap();
        MeasuredRect::export_all(&cfg).unwrap();

        let root_bindings = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bindings");
        let deck = fs::read_to_string(root_bindings.join("MeasuredDeck.ts")).unwrap();
        let slide = fs::read_to_string(root_bindings.join("MeasuredSlide.ts")).unwrap();
        let box_binding = fs::read_to_string(root_bindings.join("MeasuredBox.ts")).unwrap();
        let box_style = fs::read_to_string(root_bindings.join("MeasuredBoxStyle.ts")).unwrap();
        let paragraph = fs::read_to_string(root_bindings.join("MeasuredParagraph.ts")).unwrap();
        let run = fs::read_to_string(root_bindings.join("MeasuredRun.ts")).unwrap();

        assert!(deck.contains(r#"import type { MeasuredSlide } from "./MeasuredSlide";"#));
        assert!(deck.contains("canvasWidth: number"));
        assert!(deck.contains("canvasHeight: number"));
        assert!(deck.contains("slides: Array<MeasuredSlide>"));
        assert!(slide.contains("backgroundColor: string"));
        assert!(slide.contains("boxes: Array<MeasuredBox>"));
        assert!(slide.contains("images: Array<MeasuredImage>"));
        assert!(box_binding.contains("style: MeasuredBoxStyle"));
        assert!(box_style.contains("borderRadius: number"));
        assert!(paragraph.contains("bulletLevel: number | null"));
        assert!(paragraph.contains("numbered: boolean"));
        assert!(paragraph.contains("bulletContinuation: boolean"));
        assert!(paragraph.contains("numberingStartAt: number | null"));
        assert!(run.contains("fontFamily: string"));
        assert!(run.contains("fontSizePx: number"));
        assert!(run.contains("breaksBefore: number"));
        assert!(!run.contains("breakBefore"));
        assert!(!run.contains("monospace"));
    }

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

    #[test]
    fn rejects_invalid_slide_key_characters() {
        assert!(SlideKey::new("arch-1").is_ok());
        let err = SlideKey::new("Arch 1]").unwrap_err();
        assert_eq!(err, "slide key must use lowercase ascii, digits, or '-'");
    }

    #[test]
    fn resolved_image_path_rejects_url_delimiters_in_basename() {
        let err =
            ResolvedImagePath::from_hashed_asset("0123456789abcdef", "arch.png#frag").unwrap_err();
        assert_eq!(
            err,
            "image asset basename must not contain path separators, queries, or fragments"
        );

        let err =
            ResolvedImagePath::from_hashed_asset("0123456789abcdef", "arch.png?v=1").unwrap_err();
        assert_eq!(
            err,
            "image asset basename must not contain path separators, queries, or fragments"
        );
    }
}
