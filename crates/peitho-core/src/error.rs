use std::{
    error::Error,
    fmt,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    Parse,
    Layout,
    Asset,
    Accepts,
    Arity,
    ResidualContent,
    Theme,
    Manifest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorSlide {
    pub number: usize,
    pub key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildError {
    pub kind: ErrorKind,
    pub line: Option<usize>,
    pub origin_file: Option<PathBuf>,
    pub message: String,
    pub help: String,
    pub slide: Option<ErrorSlide>,
}

impl BuildError {
    pub fn new(
        kind: ErrorKind,
        line: Option<usize>,
        message: impl Into<String>,
        help: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            line,
            origin_file: None,
            message: message.into(),
            help: help.into(),
            slide: None,
        }
    }

    pub fn with_origin_file(mut self, path: impl AsRef<Path>) -> Self {
        self.origin_file = Some(path.as_ref().to_path_buf());
        self
    }

    pub fn with_slide(mut self, number: usize, key: Option<&str>) -> Self {
        self.slide = Some(ErrorSlide {
            number,
            key: key.map(str::to_owned),
        });
        self
    }

    /// The location-prefixed message without the help tail — what a renderer
    /// that styles help separately should print as the error's first block.
    pub fn headline(&self) -> String {
        match (&self.slide, self.line, &self.origin_file) {
            (Some(slide), Some(line), Some(file)) => match &slide.key {
                Some(key) => format!(
                    "{}:{}, slide {} ('{}'): {}",
                    file.display(),
                    line,
                    slide.number,
                    key,
                    self.message
                ),
                None => format!(
                    "{}:{}, slide {}: {}",
                    file.display(),
                    line,
                    slide.number,
                    self.message
                ),
            },
            (Some(slide), Some(line), None) => match &slide.key {
                Some(key) => format!(
                    "slide {} ('{}'), line {}: {}",
                    slide.number, key, line, self.message
                ),
                None => format!("slide {}, line {}: {}", slide.number, line, self.message),
            },
            (Some(slide), None, _) => match &slide.key {
                Some(key) => format!("slide {} ('{}'): {}", slide.number, key, self.message),
                None => format!("slide {}: {}", slide.number, self.message),
            },
            (None, Some(line), Some(file)) => {
                format!("{}:{}: {}", file.display(), line, self.message)
            }
            (None, Some(line), None) => format!("line {}: {}", line, self.message),
            (None, None, _) => self.message.clone(),
        }
    }
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}\n  = help: {}", self.headline(), self.help)
    }
}

impl Error for BuildError {}

pub type Result<T> = std::result::Result<T, BuildError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_includes_line_and_help() {
        let err = BuildError::new(
            ErrorKind::Arity,
            Some(12),
            "slot 'code' got 2 item(s), but layout 'title-body-code' allows 0..1",
            "use a layout with two code slots or remove one code block",
        );

        assert_eq!(err.line, Some(12));
        assert_eq!(
            err.help,
            "use a layout with two code slots or remove one code block"
        );
        assert!(err.to_string().contains("line 12"));
        assert!(err.to_string().contains("slot 'code' got 2 item(s)"));
    }

    #[test]
    fn display_includes_slide_context_before_line() {
        let err = BuildError::new(
            ErrorKind::Arity,
            Some(12),
            "slot 'code' got 2 item(s), but layout 'title-body-code' allows 0..1",
            "use a layout with more code capacity or remove one code block",
        )
        .with_slide(2, Some("arch-1"));

        assert_eq!(
            err.slide,
            Some(ErrorSlide {
                number: 2,
                key: Some("arch-1".to_owned())
            })
        );
        assert!(err.to_string().contains("slide 2 ('arch-1'), line 12"));
        assert!(err
            .to_string()
            .contains("help: use a layout with more code capacity or remove one code block"));
    }

    #[test]
    fn headline_is_display_without_the_help_tail() {
        let err = BuildError::new(
            ErrorKind::Parse,
            Some(3),
            "invalid deck frontmatter: unknown field `fontss`",
            "use only the supported deck frontmatter keys",
        )
        .with_origin_file("deck.md");

        assert_eq!(
            err.headline(),
            "deck.md:3: invalid deck frontmatter: unknown field `fontss`"
        );
        assert_eq!(
            err.to_string(),
            format!("{}\n  = help: {}", err.headline(), err.help)
        );
        assert!(!err.headline().contains("help"));
    }
}
