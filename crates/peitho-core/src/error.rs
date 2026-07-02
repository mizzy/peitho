use std::{error::Error, fmt};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorKind {
    Parse,
    Template,
    Accepts,
    Arity,
    ResidualContent,
    Theme,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildError {
    pub kind: ErrorKind,
    pub line: Option<usize>,
    pub message: String,
    pub help: String,
    rendered: String,
}

impl BuildError {
    pub fn new(
        kind: ErrorKind,
        line: Option<usize>,
        message: impl Into<String>,
        help: impl Into<String>,
    ) -> Self {
        let message = message.into();
        let help = help.into();
        let rendered = match line {
            Some(line) => format!("line {line}: {message}\n  = help: {help}"),
            None => format!("{message}\n  = help: {help}"),
        };
        Self {
            kind,
            line,
            message,
            help,
            rendered,
        }
    }
}

impl fmt::Display for BuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.rendered)
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
}
