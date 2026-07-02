use std::{error::Error, fmt};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    Parse,
    Template,
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
            message: message.into(),
            help: help.into(),
            slide: None,
        }
    }

    pub fn with_slide(mut self, number: usize, key: Option<&str>) -> Self {
        self.slide = Some(ErrorSlide {
            number,
            key: key.map(str::to_owned),
        });
        self
    }
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (&self.slide, self.line) {
            (Some(slide), Some(line)) => match &slide.key {
                Some(key) => write!(
                    f,
                    "slide {} ('{}'), line {}: {}\n  = help: {}",
                    slide.number, key, line, self.message, self.help
                ),
                None => write!(
                    f,
                    "slide {}, line {}: {}\n  = help: {}",
                    slide.number, line, self.message, self.help
                ),
            },
            (Some(slide), None) => match &slide.key {
                Some(key) => write!(
                    f,
                    "slide {} ('{}'): {}\n  = help: {}",
                    slide.number, key, self.message, self.help
                ),
                None => write!(
                    f,
                    "slide {}: {}\n  = help: {}",
                    slide.number, self.message, self.help
                ),
            },
            (None, Some(line)) => write!(
                f,
                "line {}: {}\n  = help: {}",
                line, self.message, self.help
            ),
            (None, None) => write!(f, "{}\n  = help: {}", self.message, self.help),
        }
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
}
