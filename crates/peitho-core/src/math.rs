use std::{fmt, sync::LazyLock};

static KATEX_CONTEXT: LazyLock<katex::KatexContext> = LazyLock::new(katex::KatexContext::default);
static KATEX_CSS: LazyLock<String> =
    LazyLock::new(|| rewrite_katex_css(include_str!("../assets/katex/katex.min.css")));

macro_rules! katex_font {
    ($name:literal) => {
        MathFontAsset::new(
            $name,
            include_bytes!(concat!("../assets/katex/fonts/", $name)),
        )
    };
}

const KATEX_FONTS: &[MathFontAsset] = &[
    katex_font!("KaTeX_AMS-Regular.woff2"),
    katex_font!("KaTeX_Caligraphic-Bold.woff2"),
    katex_font!("KaTeX_Caligraphic-Regular.woff2"),
    katex_font!("KaTeX_Fraktur-Bold.woff2"),
    katex_font!("KaTeX_Fraktur-Regular.woff2"),
    katex_font!("KaTeX_Main-Bold.woff2"),
    katex_font!("KaTeX_Main-BoldItalic.woff2"),
    katex_font!("KaTeX_Main-Italic.woff2"),
    katex_font!("KaTeX_Main-Regular.woff2"),
    katex_font!("KaTeX_Math-BoldItalic.woff2"),
    katex_font!("KaTeX_Math-Italic.woff2"),
    katex_font!("KaTeX_SansSerif-Bold.woff2"),
    katex_font!("KaTeX_SansSerif-Italic.woff2"),
    katex_font!("KaTeX_SansSerif-Regular.woff2"),
    katex_font!("KaTeX_Script-Regular.woff2"),
    katex_font!("KaTeX_Size1-Regular.woff2"),
    katex_font!("KaTeX_Size2-Regular.woff2"),
    katex_font!("KaTeX_Size3-Regular.woff2"),
    katex_font!("KaTeX_Size4-Regular.woff2"),
    katex_font!("KaTeX_Typewriter-Regular.woff2"),
];

pub(crate) trait MathRenderer {
    fn render(&self, latex: &str, display: bool) -> Result<MathOutput, MathError>;
}

/// Math rendering currently emits KaTeX HTML fragments only. Add an SVG variant
/// when a real SVG math engine is introduced; exhaustive matches will force
/// consumers to handle it then.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MathOutput {
    HtmlFragment(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MathAssets {
    _private: (),
}

impl MathAssets {
    pub fn katex() -> Self {
        Self { _private: () }
    }

    pub fn css(&self) -> &'static str {
        &KATEX_CSS
    }

    pub fn fonts(&self) -> &'static [MathFontAsset] {
        KATEX_FONTS
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MathFontAsset {
    file_name: &'static str,
    bytes: &'static [u8],
}

impl MathFontAsset {
    const fn new(file_name: &'static str, bytes: &'static [u8]) -> Self {
        Self { file_name, bytes }
    }

    pub fn file_name(&self) -> &'static str {
        self.file_name
    }

    pub fn bytes(&self) -> &'static [u8] {
        self.bytes
    }
}

fn rewrite_katex_css(css: &str) -> String {
    let mut rewritten = css.replace("url(fonts/", "url(katex-fonts/");
    for font in KATEX_FONTS {
        let stem = font
            .file_name()
            .strip_suffix(".woff2")
            .expect("embedded KaTeX font names are woff2");
        let woff = format!(r#",url(katex-fonts/{stem}.woff) format("woff")"#);
        let ttf = format!(r#",url(katex-fonts/{stem}.ttf) format("truetype")"#);
        rewritten = rewritten.replace(&woff, "");
        rewritten = rewritten.replace(&ttf, "");
    }
    rewritten
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MathError {
    message: String,
}

impl MathError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for MathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for MathError {}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct KatexRenderer;

impl MathRenderer for KatexRenderer {
    fn render(&self, latex: &str, display: bool) -> Result<MathOutput, MathError> {
        let settings = katex::Settings::builder().display_mode(display).build();
        katex::render_to_string(&KATEX_CONTEXT, latex, &settings)
            .map(MathOutput::HtmlFragment)
            .map_err(|err| MathError::new(err.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn katex_renderer_renders_display_math_as_html_fragment() {
        let renderer = KatexRenderer;

        let output = renderer.render(r#"\frac{1}{2}"#, true).unwrap();

        match output {
            MathOutput::HtmlFragment(html) => {
                assert!(html.starts_with(r#"<span class="katex-display""#), "{html}");
                assert!(html.contains("mfrac"), "{html}");
            }
        }
    }

    #[test]
    fn katex_renderer_returns_parse_error_message() {
        let renderer = KatexRenderer;

        let err = renderer.render(r#"\frac{1}{"#, true).unwrap_err();
        let message = err.to_string();

        assert!(message.contains("KaTeX parse error"));
        assert!(message.contains("expected '}'"));
    }

    #[test]
    fn math_assets_rewrite_katex_font_urls_and_expose_embedded_fonts() {
        let assets = MathAssets::katex();

        assert!(assets.css().contains("url(katex-fonts/"));
        assert!(!assets.css().contains("url(fonts/"));
        assert_eq!(assets.css().matches("url(katex-fonts/").count(), 20);
        assert!(!assets.css().contains(".woff)"));
        assert!(!assets.css().contains(".ttf)"));
        assert_eq!(assets.fonts().len(), 20);
        assert!(assets
            .fonts()
            .iter()
            .any(|font| font.file_name() == "KaTeX_Main-Regular.woff2"
                && font.bytes().starts_with(b"wOF2")));
    }
}
