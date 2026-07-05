use std::{
    fmt,
    path::{Component, Path, PathBuf},
    str::FromStr,
};

use serde::{Deserialize, Deserializer, Serialize};

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
    // Only the image resolver may construct this from an already-copied dist
    // path. Do not make this public; it bypasses hashed asset validation and
    // exists only for crate-internal tests and transforms.
    #[allow(dead_code)]
    pub(crate) fn from_string(value: String) -> Self {
        Self(value)
    }

    /// Build a resolved `assets/<hash>-<basename>` path from resolver output.
    ///
    /// The hash must be the fixed 16-hex prefix chosen by the CLI resolver, and
    /// the basename must not contain path separators.
    pub fn from_hashed_asset(hash: &str, basename: &str) -> Result<Self, String> {
        let valid_hash = hash.len() == 16 && hash.chars().all(|c| c.is_ascii_hexdigit());
        if !valid_hash {
            return Err("image asset hash must be 16 hex characters".to_owned());
        }
        let valid_basename = !basename.is_empty()
            && !basename.contains('/')
            && !basename.contains('\\')
            && Path::new(basename)
                .file_name()
                .and_then(|name| name.to_str())
                == Some(basename);
        if !valid_basename {
            return Err("image asset basename must not contain path separators".to_owned());
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

    // TDD-only escape hatch for tests/internal construction. Do not make this
    // public; parser entry points must use `new()` so raw Markdown paths are
    // validated before they can enter the pipeline.
    #[allow(dead_code)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FragmentKind<S = RawImagePath> {
    Heading { level: u8 },
    Paragraph,
    Text,
    Code,
    Image { alt: String, src: S },
    List,
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
        f: F,
    ) -> std::result::Result<SourceFragment<T>, E>
    where
        F: FnOnce(S) -> std::result::Result<T, E>,
    {
        let kind = match self.kind {
            FragmentKind::Heading { level } => FragmentKind::Heading { level },
            FragmentKind::Paragraph => FragmentKind::Paragraph,
            FragmentKind::Text => FragmentKind::Text,
            FragmentKind::Code => FragmentKind::Code,
            FragmentKind::Image { alt, src } => FragmentKind::Image { alt, src: f(src)? },
            FragmentKind::List => FragmentKind::List,
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
}
