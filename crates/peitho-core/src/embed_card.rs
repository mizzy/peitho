use std::{cell::RefCell, rc::Rc};

use lol_html::{element, end_tag, text, HtmlRewriter, Settings};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::error::{BuildError, ErrorKind, Result};

const EMBED_CARD_CSS: &str = include_str!("../assets/embed-card.css");
const GENERIC_EMBED_CARD_CSS: &str = include_str!("../assets/generic-embed-card.css");

pub(crate) fn generic_embed_card_css() -> &'static str {
    GENERIC_EMBED_CARD_CSS
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmbedCardAssets {
    _private: (),
}

impl EmbedCardAssets {
    pub fn builtin() -> Self {
        Self { _private: () }
    }

    pub fn css(&self) -> &'static str {
        EMBED_CARD_CSS
    }
}

const OEMBED_ENDPOINT: &str = "https://publish.x.com/oembed";
pub(crate) const OEMBED_REQUEST_PARAMS: &[(&str, &str)] =
    &[("omit_script", "1"), ("dnt", "1"), ("hide_thread", "1")];

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub(crate) struct OEmbedDocument {
    pub(crate) html: String,
    pub(crate) author_name: String,
    pub(crate) url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextDirection {
    Ltr,
    Rtl,
    Auto,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TweetPart {
    Text(String),
    Break,
    Link { href: String, text: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TweetParagraph {
    pub(crate) lang: Option<String>,
    pub(crate) dir: Option<TextDirection>,
    pub(crate) parts: Vec<TweetPart>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EmbedCardData {
    pub(crate) paragraphs: Vec<TweetParagraph>,
    pub(crate) author_name: String,
    pub(crate) handle: String,
    pub(crate) date_label: String,
    pub(crate) date_href: String,
    pub(crate) permalink: String,
}

#[derive(Debug)]
enum LinkKind {
    Tweet,
    Date,
}

#[derive(Debug)]
struct PendingLink {
    kind: LinkKind,
    href: String,
    text: String,
}

#[derive(Debug, Default)]
struct ExtractionState {
    blockquotes: usize,
    current_paragraph: Option<TweetParagraph>,
    current_link: Option<PendingLink>,
    paragraphs: Vec<TweetParagraph>,
    pending_blockquote_text: bool,
    date_anchor_started: bool,
    date_link: Option<(String, String)>,
    error: Option<String>,
}

pub(crate) fn parse_oembed_document(raw: &str) -> serde_json::Result<OEmbedDocument> {
    serde_json::from_str(raw)
}

pub(crate) fn extract_embed_card(
    line: usize,
    normalized_url: &str,
    document: &OEmbedDocument,
) -> Result<EmbedCardData> {
    let state = Rc::new(RefCell::new(ExtractionState::default()));
    let blockquote_state = Rc::clone(&state);
    let paragraph_state = Rc::clone(&state);
    let break_state = Rc::clone(&state);
    let link_state = Rc::clone(&state);
    let text_state = Rc::clone(&state);
    let structure_state = Rc::clone(&state);
    let mut rewriter = HtmlRewriter::new(
        Settings {
            element_content_handlers: vec![
                element!("blockquote.twitter-tweet", move |_| {
                    blockquote_state.borrow_mut().blockquotes += 1;
                    Ok(())
                }),
                element!("blockquote.twitter-tweet p", move |el| {
                    let mut state = paragraph_state.borrow_mut();
                    if state.pending_blockquote_text {
                        state.error.get_or_insert_with(|| {
                            "tweet blockquote has text between tweet paragraphs".to_owned()
                        });
                    }
                    if state.date_anchor_started {
                        state.error.get_or_insert_with(|| {
                            "tweet paragraph appears after the trailing date anchor".to_owned()
                        });
                    }
                    if state.current_paragraph.is_some() {
                        state.error.get_or_insert_with(|| {
                            "unexpected nested <p> in tweet blockquote".to_owned()
                        });
                        return Ok(());
                    }
                    let dir_value = el
                        .get_attribute("dir")
                        .map(|value| html_escape::decode_html_entities(&value).into_owned());
                    let dir = match dir_value.as_deref() {
                        Some("ltr") => Some(TextDirection::Ltr),
                        Some("rtl") => Some(TextDirection::Rtl),
                        Some("auto") => Some(TextDirection::Auto),
                        Some(value) => {
                            state.error.get_or_insert_with(|| {
                                format!("tweet paragraph has invalid dir '{value}'")
                            });
                            None
                        }
                        None => None,
                    };
                    let lang = el
                        .get_attribute("lang")
                        .map(|value| html_escape::decode_html_entities(&value).into_owned());
                    if let Some(value) = &lang {
                        if !valid_language_tag(value) {
                            state.error.get_or_insert_with(|| {
                                format!("tweet paragraph has invalid lang '{value}'")
                            });
                        }
                    }
                    state.current_paragraph = Some(TweetParagraph {
                        lang,
                        dir,
                        parts: Vec::new(),
                    });
                    let end_state = Rc::clone(&paragraph_state);
                    el.on_end_tag(end_tag!(move |_| {
                        let mut state = end_state.borrow_mut();
                        if state.current_link.is_some() {
                            state.error.get_or_insert_with(|| {
                                "tweet link was not closed before </p>".to_owned()
                            });
                        }
                        if let Some(paragraph) = state.current_paragraph.take() {
                            state.paragraphs.push(paragraph);
                        }
                        Ok(())
                    }))?;
                    Ok(())
                }),
                element!("blockquote.twitter-tweet p br", move |_| {
                    let mut state = break_state.borrow_mut();
                    if state.current_link.is_some() {
                        state.error.get_or_insert_with(|| {
                            "unexpected <br> inside a tweet link".to_owned()
                        });
                    } else if let Some(paragraph) = &mut state.current_paragraph {
                        paragraph.parts.push(TweetPart::Break);
                    } else {
                        state.error.get_or_insert_with(|| {
                            "unexpected <br> outside tweet paragraph".to_owned()
                        });
                    }
                    Ok(())
                }),
                element!("blockquote.twitter-tweet a", move |el| {
                    let mut state = link_state.borrow_mut();
                    if state.current_link.is_some() {
                        state
                            .error
                            .get_or_insert_with(|| "unexpected nested tweet link".to_owned());
                        return Ok(());
                    }
                    let Some(href) = el.get_attribute("href") else {
                        state
                            .error
                            .get_or_insert_with(|| "tweet link is missing href".to_owned());
                        return Ok(());
                    };
                    let kind = if state.current_paragraph.is_some() {
                        LinkKind::Tweet
                    } else {
                        state.pending_blockquote_text = false;
                        if state.paragraphs.is_empty() {
                            state.error.get_or_insert_with(|| {
                                "trailing date anchor appears before the tweet paragraph".to_owned()
                            });
                        }
                        if state.date_anchor_started {
                            state.error.get_or_insert_with(|| {
                                "tweet blockquote has more than one date anchor".to_owned()
                            });
                        }
                        state.date_anchor_started = true;
                        LinkKind::Date
                    };
                    state.current_link = Some(PendingLink {
                        kind,
                        href: html_escape::decode_html_entities(&href).into_owned(),
                        text: String::new(),
                    });
                    let end_state = Rc::clone(&link_state);
                    el.on_end_tag(end_tag!(move |_| {
                        let mut state = end_state.borrow_mut();
                        let Some(link) = state.current_link.take() else {
                            return Ok(());
                        };
                        match link.kind {
                            LinkKind::Tweet => {
                                if let Some(paragraph) = &mut state.current_paragraph {
                                    paragraph.parts.push(TweetPart::Link {
                                        href: link.href,
                                        text: link.text,
                                    });
                                } else {
                                    state.error.get_or_insert_with(|| {
                                        "tweet link ended outside its paragraph".to_owned()
                                    });
                                }
                            }
                            LinkKind::Date => {
                                if state.date_link.is_some() {
                                    state.error.get_or_insert_with(|| {
                                        "tweet blockquote has more than one date anchor".to_owned()
                                    });
                                } else {
                                    state.date_link = Some((link.href, link.text));
                                }
                            }
                        }
                        Ok(())
                    }))?;
                    Ok(())
                }),
                text!("blockquote.twitter-tweet", move |text| {
                    let value = html_escape::decode_html_entities(text.as_str());
                    if value.is_empty() {
                        return Ok(());
                    }
                    let mut state = text_state.borrow_mut();
                    if let Some(link) = &mut state.current_link {
                        link.text.push_str(&value);
                    } else if let Some(paragraph) = &mut state.current_paragraph {
                        match paragraph.parts.last_mut() {
                            Some(TweetPart::Text(existing)) => existing.push_str(&value),
                            _ => paragraph.parts.push(TweetPart::Text(value.into_owned())),
                        }
                    } else {
                        if value.trim().is_empty() {
                            return Ok(());
                        }
                        if state.date_link.is_some() {
                            state.error.get_or_insert_with(|| {
                                "tweet blockquote has content after its trailing date anchor"
                                    .to_owned()
                            });
                        } else if state.paragraphs.is_empty() {
                            state.error.get_or_insert_with(|| {
                                "tweet blockquote has text before the first tweet paragraph"
                                    .to_owned()
                            });
                        } else {
                            state.pending_blockquote_text = true;
                        }
                    }
                    Ok(())
                }),
                element!("blockquote.twitter-tweet *", move |el| {
                    let tag = el.tag_name();
                    let mut state = structure_state.borrow_mut();
                    let unexpected = match tag.as_str() {
                        "p" | "a" => false,
                        "br" => state.current_paragraph.is_none() || state.current_link.is_some(),
                        _ => true,
                    };
                    if unexpected {
                        state.error.get_or_insert_with(|| {
                            format!("unexpected <{tag}> in tweet blockquote")
                        });
                    }
                    Ok(())
                }),
            ],
            ..Settings::default()
        },
        |_: &[u8]| {},
    );
    rewriter
        .write(document.html.as_bytes())
        .and_then(|()| rewriter.end())
        .map_err(|err| embed_card_structure_error(line, normalized_url, err.to_string()))?;

    let state = Rc::try_unwrap(state)
        .expect("all oEmbed extraction handlers are dropped after rewriting")
        .into_inner();
    if let Some(message) = state.error {
        return Err(embed_card_structure_error(line, normalized_url, message));
    }
    if state.blockquotes != 1 {
        return Err(embed_card_structure_error(
            line,
            normalized_url,
            format!(
                "oEmbed HTML must contain exactly one tweet blockquote, found {}",
                state.blockquotes
            ),
        ));
    }
    if state.paragraphs.is_empty() {
        return Err(embed_card_structure_error(
            line,
            normalized_url,
            "oEmbed HTML is missing a tweet paragraph",
        ));
    }
    let Some((date_href, date_label)) = state.date_link else {
        return Err(embed_card_structure_error(
            line,
            normalized_url,
            "oEmbed HTML is missing its trailing date anchor",
        ));
    };
    let Some(handle) = normalized_url
        .strip_prefix("https://x.com/")
        .and_then(|path| path.split('/').next())
        .filter(|handle| !handle.is_empty())
    else {
        return Err(embed_card_structure_error(
            line,
            normalized_url,
            "normalized X status URL is missing its handle",
        ));
    };
    Ok(EmbedCardData {
        paragraphs: state.paragraphs,
        author_name: document.author_name.clone(),
        handle: handle.to_owned(),
        date_label,
        date_href,
        permalink: document.url.clone(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EmbedCardMarkup {
    pub(crate) html: String,
    pub(crate) plain_text: String,
}

pub(crate) fn build_embed_card_html(
    line: usize,
    normalized_url: &str,
    document: &OEmbedDocument,
) -> Result<EmbedCardMarkup> {
    let data = extract_embed_card(line, normalized_url, document)?;
    require_http_url(line, normalized_url, "oEmbed permalink", &data.permalink)?;
    require_http_url(line, normalized_url, "date link", &data.date_href)?;
    for paragraph in &data.paragraphs {
        for part in &paragraph.parts {
            if let TweetPart::Link { href, .. } = part {
                require_http_url(line, normalized_url, "tweet link", href)?;
            }
        }
    }

    let mut html = String::new();
    html.push_str(r#"<article class="peitho-embed-card__content">"#);
    html.push_str(r#"<a class="peitho-embed-card__author" href=""#);
    html.push_str(&html_escape::encode_double_quoted_attribute(
        &data.permalink,
    ));
    html.push_str(r#"" rel="noopener noreferrer"><span class="peitho-embed-card__author-name">"#);
    html.push_str(&html_escape::encode_text(&data.author_name));
    html.push_str(r#"</span><span class="peitho-embed-card__handle">@"#);
    html.push_str(&html_escape::encode_text(&data.handle));
    html.push_str("</span></a>");

    let mut plain_text = String::new();
    for (paragraph_index, paragraph) in data.paragraphs.iter().enumerate() {
        if paragraph_index > 0 {
            plain_text.push('\n');
        }
        html.push_str(r#"<p class="peitho-embed-card__tweet-text""#);
        if let Some(lang) = &paragraph.lang {
            html.push_str(r#" lang=""#);
            html.push_str(&html_escape::encode_double_quoted_attribute(lang));
            html.push('"');
        }
        if let Some(dir) = paragraph.dir {
            html.push_str(r#" dir=""#);
            html.push_str(dir.as_str());
            html.push('"');
        }
        html.push('>');
        for part in &paragraph.parts {
            match part {
                TweetPart::Text(text) => {
                    html.push_str(&html_escape::encode_text(text));
                    plain_text.push_str(text);
                }
                TweetPart::Break => {
                    html.push_str("<br>");
                    plain_text.push('\n');
                }
                TweetPart::Link { href, text } => {
                    html.push_str(r#"<a class="peitho-embed-card__link" href=""#);
                    html.push_str(&html_escape::encode_double_quoted_attribute(href));
                    html.push_str(r#"" rel="noopener noreferrer">"#);
                    html.push_str(&html_escape::encode_text(text));
                    html.push_str("</a>");
                    plain_text.push_str(text);
                }
            }
        }
        html.push_str("</p>");
    }
    html.push_str(r#"<a class="peitho-embed-card__date" href=""#);
    html.push_str(&html_escape::encode_double_quoted_attribute(
        &data.date_href,
    ));
    html.push_str(r#"" rel="noopener noreferrer">"#);
    html.push_str(&html_escape::encode_text(&data.date_label));
    html.push_str("</a></article>");

    Ok(EmbedCardMarkup { html, plain_text })
}

impl TextDirection {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Ltr => "ltr",
            Self::Rtl => "rtl",
            Self::Auto => "auto",
        }
    }
}

fn valid_language_tag(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=35).contains(&bytes.len())
        && bytes[0].is_ascii_alphabetic()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
}

fn require_http_url(line: usize, normalized_url: &str, field: &str, value: &str) -> Result<()> {
    let scheme = value.split_once("://").map(|(scheme, _)| scheme);
    if scheme.is_some_and(|scheme| {
        scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
    }) {
        return Ok(());
    }
    Err(embed_card_structure_error(
        line,
        normalized_url,
        format!("{field} must use an HTTP(S) URL"),
    ))
}

fn embed_card_structure_error(
    line: usize,
    normalized_url: &str,
    message: impl Into<String>,
) -> BuildError {
    BuildError::new(
        ErrorKind::Asset,
        Some(line),
        format!("{} ({normalized_url})", message.into()),
        "delete the cached oEmbed JSON and rebuild; if the error persists, X changed its oEmbed structure",
    )
}

pub fn builtin_oembed_request_url(normalized_url: &str) -> String {
    let mut url = format!(
        "{OEMBED_ENDPOINT}?url={}",
        encode_query_component(normalized_url)
    );
    for (name, value) in OEMBED_REQUEST_PARAMS {
        url.push('&');
        url.push_str(name);
        url.push('=');
        url.push_str(&encode_query_component(value));
    }
    url
}

pub(crate) fn builtin_embed_card_cache_key(normalized_url: &str) -> String {
    builtin_embed_card_cache_key_with_params(normalized_url, OEMBED_REQUEST_PARAMS)
}

pub(crate) fn builtin_embed_card_cache_key_with_params(
    normalized_url: &str,
    params: &[(&str, &str)],
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"\0peitho-builtin-embed-card\0");
    hasher.update(normalized_url.as_bytes());
    for (name, value) in params {
        hasher.update(b"\0");
        hasher.update(name.as_bytes());
        hasher.update(b"=");
        hasher.update(value.as_bytes());
    }
    hex_encode(&hasher.finalize())
}

fn encode_query_component(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push(HEX[(byte >> 4) as usize] as char);
            encoded.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    encoded
}

pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::{
        build_embed_card_html, builtin_embed_card_cache_key,
        builtin_embed_card_cache_key_with_params, builtin_oembed_request_url, extract_embed_card,
        parse_oembed_document, EmbedCardData, OEmbedDocument, TextDirection, TweetParagraph,
        TweetPart, OEMBED_REQUEST_PARAMS,
    };
    use crate::error::ErrorKind;

    const STATUS_URL: &str = "https://x.com/gosukenator/status/2074821309259973046";

    mod cache_key_test_vector {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/support/embed_card_cache_key.rs"
        ));
    }

    #[test]
    fn embed_card_request_url_targets_publish_x_and_encodes_fixed_parameters() {
        assert_eq!(
            builtin_oembed_request_url(STATUS_URL),
            "https://publish.x.com/oembed?url=https%3A%2F%2Fx.com%2Fgosukenator%2Fstatus%2F2074821309259973046&omit_script=1&dnt=1&hide_thread=1"
        );
        assert!(!builtin_oembed_request_url(STATUS_URL).contains("publish.twitter.com"));
    }

    #[test]
    fn embed_card_cache_key_covers_url_and_every_request_parameter() {
        let key = builtin_embed_card_cache_key(STATUS_URL);
        assert_eq!(
            key,
            cache_key_test_vector::PINNED_BUILTIN_EMBED_CARD_CACHE_KEY
        );
        assert_ne!(
            key,
            builtin_embed_card_cache_key("https://x.com/a/status/1")
        );

        for index in 0..OEMBED_REQUEST_PARAMS.len() {
            let mut changed = OEMBED_REQUEST_PARAMS.to_vec();
            changed[index].1 = "changed";
            assert_ne!(
                key,
                builtin_embed_card_cache_key_with_params(STATUS_URL, &changed),
                "parameter {} must contribute to the key",
                OEMBED_REQUEST_PARAMS[index].0
            );
        }
    }

    #[test]
    fn captured_x_oembed_extracts_paragraphs_lang_dir_links_author_handle_and_date() {
        let raw = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/x-oembed-response.json"
        ));
        let document = parse_oembed_document(raw).unwrap();

        let extracted = extract_embed_card(12, STATUS_URL, &document).unwrap();

        assert_eq!(
            extracted,
            EmbedCardData {
                paragraphs: vec![TweetParagraph {
                    lang: Some("ja".to_owned()),
                    dir: Some(TextDirection::Ltr),
                    parts: vec![
                        TweetPart::Text("自前プレゼンツール、スライド作成中に表示確認したり、スライド一覧をグリッドで表示したり、Markdownの修正をリアルタイムに反映したりする機能を入れた".to_owned()),
                        TweetPart::Link {
                            href: "https://t.co/c3iJNbu3uw".to_owned(),
                            text: "https://t.co/c3iJNbu3uw".to_owned(),
                        },
                        TweetPart::Text(" ".to_owned()),
                        TweetPart::Link {
                            href: "https://t.co/mhIvL0JQFA".to_owned(),
                            text: "pic.twitter.com/mhIvL0JQFA".to_owned(),
                        },
                    ],
                }],
                author_name: "mizzy".to_owned(),
                handle: "gosukenator".to_owned(),
                date_label: "July 8, 2026".to_owned(),
                date_href: "https://x.com/gosukenator/status/2074821309259973046?ref_src=twsrc%5Etfw".to_owned(),
                permalink: "https://x.com/gosukenator/status/2074821309259973046".to_owned(),
            }
        );
    }

    #[test]
    fn oembed_html_preserves_br_boundaries() {
        let document = fixture_document(
            r#"<blockquote class="twitter-tweet"><p lang="en" dir="auto">first<br>second<br/>third</p>&mdash; A (@a) <a href="https://x.com/a/status/1">August 5, 2026</a></blockquote>"#,
        );

        let extracted = extract_embed_card(22, "https://x.com/a/status/1", &document).unwrap();

        assert_eq!(
            extracted.paragraphs[0].parts,
            vec![
                TweetPart::Text("first".to_owned()),
                TweetPart::Break,
                TweetPart::Text("second".to_owned()),
                TweetPart::Break,
                TweetPart::Text("third".to_owned()),
            ]
        );
        assert_eq!(extracted.paragraphs[0].dir, Some(TextDirection::Auto));
    }

    #[test]
    fn oembed_html_blockquote_level_text_before_first_paragraph_is_error() {
        assert_structure_error(
            r#"<blockquote class="twitter-tweet">stray text<p>a</p>&mdash; A (@a) <a href="https://x.com/a/status/1">date</a></blockquote>"#,
            "text before the first tweet paragraph",
        );
    }

    #[test]
    fn oembed_html_blockquote_level_text_between_paragraphs_is_error() {
        assert_structure_error(
            r#"<blockquote class="twitter-tweet"><p>a</p>stray text<p>b</p>&mdash; A (@a) <a href="https://x.com/a/status/1">date</a></blockquote>"#,
            "text between tweet paragraphs",
        );
    }

    #[test]
    fn oembed_html_blockquote_level_text_allows_multi_paragraph_attribution() {
        let document = fixture_document(
            r#"<blockquote class="twitter-tweet"><p>a</p><p>b</p>&mdash; A (@a) <a href="https://x.com/a/status/1">date</a></blockquote>"#,
        );

        let extracted = extract_embed_card(22, "https://x.com/a/status/1", &document).unwrap();

        assert_eq!(
            extracted.paragraphs,
            vec![
                TweetParagraph {
                    lang: None,
                    dir: None,
                    parts: vec![TweetPart::Text("a".to_owned())],
                },
                TweetParagraph {
                    lang: None,
                    dir: None,
                    parts: vec![TweetPart::Text("b".to_owned())],
                },
            ]
        );
        assert_eq!(extracted.date_label, "date");
    }

    #[test]
    fn oembed_html_without_tweet_paragraph_is_line_numbered() {
        assert_structure_error(
            r#"<blockquote class="twitter-tweet">text <a href="https://x.com/a/status/1">August 5, 2026</a></blockquote>"#,
            "tweet paragraph",
        );
    }

    #[test]
    fn oembed_html_without_trailing_date_anchor_is_line_numbered() {
        assert_structure_error(
            r#"<blockquote class="twitter-tweet"><p>hello</p>&mdash; A (@a)</blockquote>"#,
            "date anchor",
        );
        assert_structure_error(
            r#"<blockquote class="twitter-tweet"><a href="https://x.com/a/status/1">August 5, 2026</a><p>hello</p>&mdash; A (@a)</blockquote>"#,
            "date anchor",
        );
        assert_structure_error(
            r#"<blockquote class="twitter-tweet"><p>hello</p>&mdash; A (@a) <a href="https://x.com/a/status/1">August 5, 2026</a> trailing text</blockquote>"#,
            "date anchor",
        );
    }

    #[test]
    fn oembed_html_with_unexpected_blockquote_structure_is_line_numbered() {
        assert_structure_error(
            r#"<blockquote class="twitter-tweet"><p>hello <span>surprise</span></p>&mdash; A (@a) <a href="https://x.com/a/status/1">August 5, 2026</a></blockquote>"#,
            "unexpected <span>",
        );
        assert_structure_error(
            r#"<blockquote class="twitter-tweet"><p>hello</p><br>&mdash; A (@a) <a href="https://x.com/a/status/1">August 5, 2026</a></blockquote>"#,
            "unexpected <br>",
        );
    }

    fn fixture_document(html: &str) -> OEmbedDocument {
        OEmbedDocument {
            html: html.to_owned(),
            author_name: "A".to_owned(),
            url: "https://x.com/a/status/1".to_owned(),
        }
    }

    fn assert_structure_error(html: &str, message: &str) {
        let url = "https://x.com/a/status/1";
        let err = extract_embed_card(22, url, &fixture_document(html)).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Asset);
        assert_eq!(err.line, Some(22));
        assert!(err.message.contains(message), "{}", err.message);
        assert!(err.message.contains(url), "{}", err.message);
        assert!(err.help.contains("oEmbed"), "{}", err.help);
    }

    #[test]
    fn embed_card_escapes_hostile_oembed_fields_and_never_injects_provider_html() {
        let document = OEmbedDocument {
            html: concat!(
                r#"<script>outside()</script><style>.outside{}</style>"#,
                r#"<blockquote class="twitter-tweet" data-provider="raw"><p lang="en-US" dir="rtl" onclick="inside()">Hello &lt;script&gt;<br><a href="https://example.com/?q=&quot;x&quot;" onmouseover="inside()">&lt;img src=x onerror=inside()&gt;</a></p>&mdash; A (@a) <a href="https://x.com/a/status/1?ref=%22bad%22" onclick="inside()">&lt;date&gt;</a></blockquote>"#,
            )
            .to_owned(),
            author_name: r#""><script>author()</script>"#.to_owned(),
            url: r#"https://x.com/a/status/1?quote="bad"#.to_owned(),
        };

        let markup = build_embed_card_html(31, "https://x.com/a/status/1", &document).unwrap();

        assert!(!markup.html.contains("<script"), "{}", markup.html);
        assert!(!markup.html.contains("<style"), "{}", markup.html);
        assert!(!markup.html.contains("<blockquote"), "{}", markup.html);
        assert!(!markup.html.contains("onclick"), "{}", markup.html);
        assert!(!markup.html.contains("onmouseover"), "{}", markup.html);
        assert!(!markup.html.contains("twitter-tweet"), "{}", markup.html);
        assert!(markup.html.contains(r#"lang="en-US" dir="rtl""#));
        assert!(markup.html.contains("&lt;script&gt;"), "{}", markup.html);
        assert!(
            markup.html.contains("&lt;img src=x onerror=inside()&gt;"),
            "{}",
            markup.html
        );
        assert!(markup.html.contains("&lt;date&gt;"), "{}", markup.html);
        assert!(
            markup.html.contains("\"&gt;&lt;script&gt;author()"),
            "{}",
            markup.html
        );
        assert!(markup.html.contains("quote=&quot;bad"), "{}", markup.html);
        assert!(markup.html.contains("q=&quot;x&quot;"), "{}", markup.html);
        assert_eq!(
            markup.plain_text,
            "Hello <script>\n<img src=x onerror=inside()>"
        );
    }

    #[test]
    fn embed_card_rejects_non_http_links() {
        for (html, permalink) in [
            (
                r#"<blockquote class="twitter-tweet"><p><a href="javascript:alert(1)">bad</a></p>&mdash; A (@a) <a href="https://x.com/a/status/1">date</a></blockquote>"#,
                "https://x.com/a/status/1",
            ),
            (
                r#"<blockquote class="twitter-tweet"><p><a href="data:text/html,bad">bad</a></p>&mdash; A (@a) <a href="https://x.com/a/status/1">date</a></blockquote>"#,
                "https://x.com/a/status/1",
            ),
            (
                r#"<blockquote class="twitter-tweet"><p>ok</p>&mdash; A (@a) <a href="javascript:alert(1)">date</a></blockquote>"#,
                "https://x.com/a/status/1",
            ),
            (
                r#"<blockquote class="twitter-tweet"><p>ok</p>&mdash; A (@a) <a href="https://x.com/a/status/1">date</a></blockquote>"#,
                "javascript:alert(1)",
            ),
        ] {
            let document = OEmbedDocument {
                html: html.to_owned(),
                author_name: "A".to_owned(),
                url: permalink.to_owned(),
            };
            let err = build_embed_card_html(32, "https://x.com/a/status/1", &document).unwrap_err();
            assert_eq!(err.kind, ErrorKind::Asset);
            assert_eq!(err.line, Some(32));
            assert!(err.message.contains("https://x.com/a/status/1"));
            assert!(err.message.contains("HTTP(S)"), "{}", err.message);
            assert!(err.help.contains("delete"), "{}", err.help);
        }
    }

    #[test]
    fn embed_card_omits_absent_tweet_lang_and_dir() {
        let markup = build_embed_card_html(
            33,
            "https://x.com/a/status/1",
            &fixture_document(
                r#"<blockquote class="twitter-tweet"><p>hello</p>&mdash; A (@a) <a href="https://x.com/a/status/1">date</a></blockquote>"#,
            ),
        )
        .unwrap();
        let tweet_text = markup
            .html
            .split("peitho-embed-card__tweet-text")
            .nth(1)
            .unwrap()
            .split('>')
            .next()
            .unwrap();
        assert!(!tweet_text.contains("lang="), "{}", markup.html);
        assert!(!tweet_text.contains("dir="), "{}", markup.html);
    }

    #[test]
    fn embed_card_rejects_invalid_tweet_lang_or_dir() {
        for attributes in [
            r#"lang="""#,
            r#"lang="en_US""#,
            r#"lang="abcdefghijklmnopqrstuvwxyzabcdefghij""#,
            r#"dir="RTL""#,
            r#"dir="sideways""#,
            r#"dir="""#,
        ] {
            let html = format!(
                r#"<blockquote class="twitter-tweet"><p {attributes}>hello</p>&mdash; A (@a) <a href="https://x.com/a/status/1">date</a></blockquote>"#
            );
            let err =
                build_embed_card_html(34, "https://x.com/a/status/1", &fixture_document(&html))
                    .unwrap_err();
            assert_eq!(err.kind, ErrorKind::Asset);
            assert_eq!(err.line, Some(34));
            assert!(err.message.contains("https://x.com/a/status/1"));
            assert!(err.message.contains("invalid"), "{}", err.message);
            assert!(err.help.contains("oEmbed"), "{}", err.help);
        }
    }
}
