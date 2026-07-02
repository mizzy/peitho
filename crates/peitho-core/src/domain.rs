use std::{fmt, str::FromStr};

use serde::Serialize;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FragmentKind {
    Heading { level: u8 },
    Paragraph,
    Text,
    Code,
    Image,
    List,
}

impl FragmentKind {
    pub fn default_accepts(self) -> Accepts {
        match self {
            Self::Heading { .. } => Accepts::Inline,
            Self::Paragraph => Accepts::Blocks,
            Self::Text => Accepts::Text,
            Self::Code => Accepts::Code,
            Self::Image => Accepts::Image,
            Self::List => Accepts::List,
        }
    }

    pub fn removal_noun(self) -> &'static str {
        match self {
            Self::Heading { .. } => "heading",
            Self::Paragraph => "paragraph",
            Self::Text => "text block",
            Self::Code => "code block",
            Self::Image => "image",
            Self::List => "list",
        }
    }
}

impl fmt::Display for FragmentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Heading { .. } => "heading",
            Self::Paragraph => "paragraph",
            Self::Text => "text",
            Self::Code => "code",
            Self::Image => "image",
            Self::List => "list",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFragment {
    line: usize,
    kind: FragmentKind,
    markdown: String,
    text: String,
    code: String,
    language: Option<String>,
}

impl SourceFragment {
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

    pub fn line(&self) -> usize {
        self.line
    }

    pub fn kind(&self) -> FragmentKind {
        self.kind
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
        matches!(self.kind, FragmentKind::Heading { .. }).then(|| self.text.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedSlide {
    index: usize,
    key: SlideKey,
    html: String,
}

impl RenderedSlide {
    pub(crate) fn new(index: usize, key: SlideKey, html: String) -> Self {
        Self { index, key, html }
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
