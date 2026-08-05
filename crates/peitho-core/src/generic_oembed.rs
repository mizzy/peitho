use std::{cell::RefCell, rc::Rc};

use lol_html::{element, HtmlRewriter, Settings};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use url::Url;

use crate::{
    domain::EmbedImageFormat,
    embed_card::hex_encode,
    error::{BuildError, ErrorKind, Result},
};

pub(crate) fn detect_thumbnail_format(bytes: &[u8]) -> Option<EmbedImageFormat> {
    if bytes.starts_with(b"\xff\xd8\xff") {
        Some(EmbedImageFormat::Jpeg)
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(EmbedImageFormat::Png)
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some(EmbedImageFormat::WebP)
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(EmbedImageFormat::Gif)
    } else {
        None
    }
}

pub(crate) fn generic_oembed_json_cache_key(normalized_page_url: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"\0peitho-generic-oembed\0");
    hasher.update(normalized_page_url.as_bytes());
    hex_encode(&hasher.finalize())
}

pub(crate) fn generic_oembed_thumbnail_cache_key(normalized_page_url: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"\0peitho-generic-oembed-thumbnail\0");
    hasher.update(normalized_page_url.as_bytes());
    hex_encode(&hasher.finalize())
}

pub(crate) fn discover_oembed_endpoint(line: usize, page_url: &Url, html: &[u8]) -> Result<Url> {
    let discovered = Rc::new(RefCell::new(None::<std::result::Result<String, String>>));
    let handler_state = Rc::clone(&discovered);
    let mut rewriter = HtmlRewriter::new(
        Settings {
            element_content_handlers: vec![element!("link", move |element| {
                if handler_state.borrow().is_some() {
                    return Ok(());
                }
                let rel = element
                    .get_attribute("rel")
                    .map(|value| html_escape::decode_html_entities(&value).into_owned());
                let content_type = element
                    .get_attribute("type")
                    .map(|value| html_escape::decode_html_entities(&value).into_owned());
                let matches_rel = rel.as_deref().is_some_and(|value| {
                    value
                        .split_ascii_whitespace()
                        .any(|token| token.eq_ignore_ascii_case("alternate"))
                });
                let matches_type = content_type.as_deref().is_some_and(|value| {
                    value.trim().eq_ignore_ascii_case("application/json+oembed")
                });
                if matches_rel && matches_type {
                    let href = element
                        .get_attribute("href")
                        .map(|value| html_escape::decode_html_entities(&value).into_owned())
                        .filter(|value| !value.trim().is_empty())
                        .ok_or_else(|| {
                            "first JSON oEmbed discovery link has no nonempty href".to_owned()
                        });
                    *handler_state.borrow_mut() = Some(href);
                }
                Ok(())
            })],
            ..Settings::default()
        },
        |_: &[u8]| {},
    );
    rewriter.write(html).map_err(|err| {
        discovery_error(
            line,
            page_url,
            format!("failed to parse page HTML for oEmbed discovery: {err}"),
        )
    })?;
    rewriter.end().map_err(|err| {
        discovery_error(
            line,
            page_url,
            format!("failed to finish page HTML parsing for oEmbed discovery: {err}"),
        )
    })?;

    let href = discovered
        .borrow()
        .clone()
        .ok_or_else(|| discovery_error(line, page_url, "no JSON oEmbed discovery link found"))?
        .map_err(|message| discovery_error(line, page_url, message))?;
    let endpoint = page_url.join(&href).map_err(|err| {
        discovery_error(
            line,
            page_url,
            format!("JSON oEmbed discovery href is invalid: {err}"),
        )
    })?;
    if !matches!(endpoint.scheme(), "http" | "https") {
        return Err(discovery_error(
            line,
            page_url,
            "JSON oEmbed discovery endpoint must use an HTTP(S) URL",
        ));
    }
    Ok(endpoint)
}

fn discovery_error(line: usize, page_url: &Url, message: impl Into<String>) -> BuildError {
    BuildError::new(
        ErrorKind::Asset,
        Some(line),
        format!("{} ({page_url})", message.into()),
        "the page must contain <link rel=\"alternate\" type=\"application/json+oembed\" href=\"...\">; fix the page URL or use a provider that advertises JSON oEmbed",
    )
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum GenericOEmbedType {
    Photo,
    Video,
    Link,
    Rich,
}

#[derive(Debug, Deserialize)]
struct GenericOEmbedDocument {
    #[serde(rename = "type")]
    kind: Option<GenericOEmbedType>,
    title: Option<String>,
    author_name: Option<String>,
    provider_name: Option<String>,
    thumbnail_url: Option<String>,
    url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GenericOEmbedData {
    pub(crate) kind: Option<GenericOEmbedType>,
    pub(crate) title: Option<String>,
    pub(crate) author_name: Option<String>,
    pub(crate) provider_name: Option<String>,
    pub(crate) image_url: Option<Url>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GenericEmbedCardParts {
    pub(crate) image_alt_attr: String,
    pub(crate) title_html: Option<String>,
    pub(crate) author_html: Option<String>,
    pub(crate) provider_html: Option<String>,
    pub(crate) permalink_attr: String,
    pub(crate) plain_text: String,
}

pub(crate) fn parse_generic_oembed(
    line: usize,
    page_url: &Url,
    raw: &[u8],
) -> Result<GenericOEmbedData> {
    let document: GenericOEmbedDocument = serde_json::from_slice(raw).map_err(|err| {
        generic_oembed_error(
            line,
            page_url,
            format!("oEmbed response is invalid JSON data: {err}"),
            "the provider returned invalid oEmbed data; retry the build or check the author page URL",
        )
    })?;
    let title = trimmed_display_field(document.title);
    let author_name = trimmed_display_field(document.author_name);
    let provider_name = trimmed_display_field(document.provider_name);
    if title.is_none() && author_name.is_none() {
        return Err(generic_oembed_error(
            line,
            page_url,
            "oEmbed response has neither title nor author",
            "the provider returned incomplete oEmbed data; retry the build or check the author page URL; the response must contain a nonempty title or author_name",
        ));
    }

    let image_url = match document.thumbnail_url {
        Some(value) => Some(parse_image_url(line, page_url, "thumbnail_url", &value)?),
        None if document.kind == Some(GenericOEmbedType::Photo) => {
            let value = document.url.ok_or_else(|| {
                generic_oembed_error(
                    line,
                    page_url,
                    "photo oEmbed response has no thumbnail_url or required url image",
                    "the provider returned incomplete photo oEmbed data; retry the build or check the author page URL; thumbnail_url or the photo url must contain an HTTP(S) image URL",
                )
            })?;
            Some(parse_image_url(line, page_url, "photo url", &value)?)
        }
        None => None,
    };

    Ok(GenericOEmbedData {
        kind: document.kind,
        title,
        author_name,
        provider_name,
        image_url,
    })
}

pub(crate) fn build_generic_embed_card(
    page_url: &Url,
    data: &GenericOEmbedData,
) -> GenericEmbedCardParts {
    let title_html = data
        .title
        .as_deref()
        .map(|value| html_escape::encode_text(value).into_owned());
    let author_html = data
        .author_name
        .as_deref()
        .map(|value| html_escape::encode_text(value).into_owned());
    let provider_html = data
        .provider_name
        .as_deref()
        .map(|value| html_escape::encode_text(value).into_owned());
    let alt = data
        .title
        .as_deref()
        .or(data.author_name.as_deref())
        .expect("validated generic oEmbed has a title or author");
    let plain_text = [data.title.as_deref(), data.author_name.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n");
    GenericEmbedCardParts {
        image_alt_attr: html_escape::encode_double_quoted_attribute(alt).into_owned(),
        title_html,
        author_html,
        provider_html,
        permalink_attr: html_escape::encode_double_quoted_attribute(page_url.as_str()).into_owned(),
        plain_text,
    }
}

fn trimmed_display_field(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn parse_image_url(line: usize, page_url: &Url, field: &str, value: &str) -> Result<Url> {
    let parsed = Url::parse(value.trim()).map_err(|_| {
        generic_oembed_error(
            line,
            page_url,
            format!("oEmbed {field} image URL must use HTTP(S)"),
            "the provider returned an invalid oEmbed image URL; retry the build or check the author page URL; thumbnail_url (or the photo url) must be an absolute HTTP(S) URL",
        )
    })?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(generic_oembed_error(
            line,
            page_url,
            format!("oEmbed {field} image URL must use HTTP(S)"),
            "the provider returned an invalid oEmbed image URL; retry the build or check the author page URL; thumbnail_url (or the photo url) must be an absolute HTTP(S) URL",
        ));
    }
    Ok(parsed)
}

fn generic_oembed_error(
    line: usize,
    page_url: &Url,
    message: impl Into<String>,
    help: impl Into<String>,
) -> BuildError {
    BuildError::new(
        ErrorKind::Asset,
        Some(line),
        format!("{} ({page_url})", message.into()),
        help,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        build_generic_embed_card, discover_oembed_endpoint, parse_generic_oembed, GenericOEmbedType,
    };
    use crate::error::ErrorKind;
    use url::Url;

    const MASTODON_PAGE: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/mastodon-status-page.html"
    ));
    const MASTODON_URL: &str = "https://mastodon.social/@Mastodon/117004738413167978";
    const YOUTUBE_JSON: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/youtube-oembed-response.json"
    ));
    const MASTODON_JSON: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/mastodon-oembed-response.json"
    ));

    fn page_url(value: &str) -> Url {
        Url::parse(value).unwrap()
    }

    #[test]
    fn discovery_extracts_real_mastodon_oembed_link_and_decodes_ampersands() {
        let endpoint = discover_oembed_endpoint(7, &page_url(MASTODON_URL), MASTODON_PAGE).unwrap();

        assert_eq!(
            endpoint.as_str(),
            "https://mastodon.social/api/oembed?format=json&url=https%3A%2F%2Fmastodon.social%2F%40Mastodon%2F117004738413167978"
        );
        assert!(!endpoint.as_str().contains("amp;"));
    }

    #[test]
    fn discovery_matches_rel_and_type_in_any_attribute_order() {
        for html in [
            r#"<link href="/oembed?x=1&amp;y=2" rel="alternate" type="application/json+oembed">"#,
            r#"<link TYPE="APPLICATION/JSON+OEMBED" data-x="1" REL="alternate" HREF="/oembed">"#,
            r#"<link rel="help ALTERNATE next" href="/oembed" type="application/json+oembed">"#,
        ] {
            let endpoint = discover_oembed_endpoint(
                7,
                &page_url("https://example.com/posts/1"),
                html.as_bytes(),
            )
            .unwrap();
            assert!(endpoint.as_str().starts_with("https://example.com/oembed"));
        }
    }

    #[test]
    fn discovery_resolves_root_and_path_relative_hrefs_against_page_url() {
        let root = discover_oembed_endpoint(
            7,
            &page_url("https://example.com/posts/1"),
            br#"<link rel="alternate" type="application/json+oembed" href="/api/oembed">"#,
        )
        .unwrap();
        let relative = discover_oembed_endpoint(
            7,
            &page_url("https://example.com/posts/1"),
            br#"<link rel="alternate" type="application/json+oembed" href="../oembed?x=1">"#,
        )
        .unwrap();

        assert_eq!(root.as_str(), "https://example.com/api/oembed");
        assert_eq!(relative.as_str(), "https://example.com/oembed?x=1");
    }

    #[test]
    fn discovery_uses_first_matching_link() {
        let endpoint = discover_oembed_endpoint(
            7,
            &page_url("https://example.com/posts/1"),
            br#"<link rel="alternate" type="application/json+oembed" href="/first"><link rel="alternate" type="application/json+oembed" href="/second">"#,
        )
        .unwrap();

        assert_eq!(endpoint.as_str(), "https://example.com/first");
    }

    #[test]
    fn discovery_ignores_nonmatching_link_elements() {
        let endpoint = discover_oembed_endpoint(
            7,
            &page_url("https://example.com/posts/1"),
            br#"<link rel="stylesheet" type="application/json+oembed" href="/wrong-one"><link rel="alternate" type="text/xml+oembed" href="/wrong-two"><link rel="alternate" type="application/json+oembed" href="/right">"#,
        )
        .unwrap();

        assert_eq!(endpoint.as_str(), "https://example.com/right");
    }

    #[test]
    fn discovery_missing_link_is_line_numbered() {
        let err = discover_oembed_endpoint(
            7,
            &page_url("https://example.com/posts/1"),
            b"<html><head><title>No discovery</title></head></html>",
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Asset);
        assert_eq!(err.line, Some(7));
        assert!(err.message.contains("no JSON oEmbed discovery link"));
        assert!(err.message.contains("https://example.com/posts/1"));
        assert!(err.help.contains("rel=\"alternate\""));
    }

    #[test]
    fn discovery_rejects_missing_href_or_non_http_endpoint() {
        for (html, expected) in [
            (
                r#"<link rel="alternate" type="application/json+oembed">"#,
                "href",
            ),
            (
                r#"<link rel="alternate" type="application/json+oembed" href="javascript:alert(1)">"#,
                "HTTP(S)",
            ),
        ] {
            let err = discover_oembed_endpoint(
                11,
                &page_url("https://example.com/posts/1"),
                html.as_bytes(),
            )
            .unwrap_err();

            assert_eq!(err.kind, ErrorKind::Asset);
            assert_eq!(err.line, Some(11));
            assert!(err.message.contains(expected), "{}", err.message);
            assert!(err.help.contains("application/json+oembed"));
        }
    }

    #[test]
    fn youtube_video_oembed_builds_thumbnail_card_data() {
        let page = page_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ");
        let data = parse_generic_oembed(7, &page, YOUTUBE_JSON).unwrap();

        assert_eq!(data.kind, Some(GenericOEmbedType::Video));
        assert_eq!(
            data.title.as_deref(),
            Some("Rick Astley - Never Gonna Give You Up (Official Video) (4K Remaster)")
        );
        assert_eq!(data.author_name.as_deref(), Some("Rick Astley"));
        assert_eq!(data.provider_name.as_deref(), Some("YouTube"));
        assert_eq!(
            data.image_url.as_ref().map(Url::as_str),
            Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg")
        );
    }

    #[test]
    fn mastodon_rich_oembed_builds_text_card_without_reading_html() {
        let page = page_url(MASTODON_URL);
        let data = parse_generic_oembed(7, &page, MASTODON_JSON).unwrap();
        let card = build_generic_embed_card(&page, &data);

        assert_eq!(data.kind, Some(GenericOEmbedType::Rich));
        assert_eq!(data.title, None);
        assert_eq!(data.author_name.as_deref(), Some("Mastodon"));
        assert_eq!(data.provider_name.as_deref(), Some("mastodon.social"));
        assert_eq!(data.image_url, None);
        assert_eq!(card.plain_text, "Mastodon");
        assert!(!card.author_html.as_deref().unwrap().contains("blockquote"));
        assert!(!card.author_html.as_deref().unwrap().contains("script"));
    }

    #[test]
    fn generic_oembed_accepts_photo_video_link_and_rich_types() {
        for (kind, expected) in [
            ("photo", GenericOEmbedType::Photo),
            ("video", GenericOEmbedType::Video),
            ("link", GenericOEmbedType::Link),
            ("rich", GenericOEmbedType::Rich),
        ] {
            let image = if kind == "photo" {
                r#", "url": "https://cdn.example.com/photo.png""#
            } else {
                ""
            };
            let json = format!(r#"{{"type":"{kind}","title":"Title"{image}}}"#);
            let data =
                parse_generic_oembed(7, &page_url("https://example.com/post"), json.as_bytes())
                    .unwrap();
            assert_eq!(data.kind, Some(expected));
        }
    }

    #[test]
    fn photo_url_is_image_fallback_when_thumbnail_is_absent() {
        let data = parse_generic_oembed(
            7,
            &page_url("https://example.com/post"),
            br#"{"type":"photo","title":"Photo","url":"https://cdn.example.com/photo.webp"}"#,
        )
        .unwrap();

        assert_eq!(
            data.image_url.as_ref().map(Url::as_str),
            Some("https://cdn.example.com/photo.webp")
        );
    }

    #[test]
    fn generic_oembed_rejects_unknown_type_or_photo_without_image() {
        for (json, expected) in [
            (r#"{"type":"future","title":"Title"}"#, "unknown variant"),
            (r#"{"type":"photo","title":"Title"}"#, "photo"),
            (r#"{"type":7,"title":"Title"}"#, "invalid"),
        ] {
            let err =
                parse_generic_oembed(7, &page_url("https://example.com/post"), json.as_bytes())
                    .unwrap_err();
            assert_eq!(err.line, Some(7));
            assert!(err.message.contains(expected), "{}", err.message);
            assert!(err.help.contains("provider returned"), "{}", err.help);
            assert!(err.help.contains("retry the build"), "{}", err.help);
            assert!(!err.help.contains("cached oEmbed JSON"), "{}", err.help);
        }
    }

    #[test]
    fn generic_oembed_requires_nonempty_title_or_author() {
        for json in [
            r#"{"type":"rich","provider_name":"Example"}"#,
            r#"{"title":"  ","author_name":"\n","provider_name":"Example"}"#,
        ] {
            let err =
                parse_generic_oembed(13, &page_url("https://example.com/post"), json.as_bytes())
                    .unwrap_err();
            assert_eq!(err.line, Some(13));
            assert!(err.message.contains("neither title nor author"));
            assert!(err.help.contains("title or author_name"));
            assert!(err.help.contains("provider returned"));
            assert!(!err.help.contains("cached oEmbed JSON"));
        }
    }

    #[test]
    fn generic_oembed_rejects_non_http_thumbnail_and_photo_urls() {
        for json in [
            r#"{"type":"video","title":"Video","thumbnail_url":"javascript:alert(1)"}"#,
            r#"{"type":"photo","title":"Photo","url":"data:image/png;base64,AA=="}"#,
        ] {
            let err =
                parse_generic_oembed(5, &page_url("https://example.com/post"), json.as_bytes())
                    .unwrap_err();
            assert_eq!(err.line, Some(5));
            assert!(err.message.contains("image URL must use HTTP(S)"));
            assert!(err.help.contains("thumbnail_url"));
            assert!(err.help.contains("provider returned"));
            assert!(!err.help.contains("cached oEmbed JSON"));
        }
    }

    #[test]
    fn generic_card_escapes_hostile_title_author_provider_alt_and_permalink() {
        let page = page_url("https://example.com/post?a=1&b=2");
        let data = parse_generic_oembed(
            7,
            &page,
            br#"{"type":"rich","title":"<script>alert(1)</script>","author_name":"\" onmouseover=\"boom","provider_name":"<img src=x onerror=boom> & Co","html":"<iframe src=\"https://evil.example\"></iframe><script>boom()</script>","thumbnail_url":"https://cdn.example.com/thumb.jpg"}"#,
        )
        .unwrap();
        let card = build_generic_embed_card(&page, &data);

        assert_eq!(
            card.title_html.as_deref(),
            Some("&lt;script&gt;alert(1)&lt;/script&gt;")
        );
        assert_eq!(card.author_html.as_deref(), Some("\" onmouseover=\"boom"));
        assert_eq!(
            card.provider_html.as_deref(),
            Some("&lt;img src=x onerror=boom&gt; &amp; Co")
        );
        assert!(card.image_alt_attr.contains("&lt;script&gt;"));
        assert_eq!(card.permalink_attr, "https://example.com/post?a=1&amp;b=2");
        for part in [
            card.title_html.as_deref().unwrap(),
            card.author_html.as_deref().unwrap(),
            card.provider_html.as_deref().unwrap(),
            &card.image_alt_attr,
            &card.permalink_attr,
        ] {
            assert!(!part.contains("<script"));
            assert!(!part.contains("<iframe"));
        }
    }

    #[test]
    fn generic_card_permalink_is_always_normalized_author_url() {
        let page = page_url("HTTPS://EXAMPLE.COM/a/../post#fragment");
        let mut normalized = page.clone();
        normalized.set_fragment(None);
        let data = parse_generic_oembed(
            7,
            &normalized,
            br#"{"type":"link","title":"Title","url":"https://provider.example/other","author_url":"https://author.example/","provider_url":"https://provider.example/"}"#,
        )
        .unwrap();
        let card = build_generic_embed_card(&normalized, &data);

        assert_eq!(card.permalink_attr, "https://example.com/post");
        assert!(!card.permalink_attr.contains("provider.example"));
    }

    #[test]
    fn generic_card_plain_text_is_title_then_author() {
        let page = page_url("https://example.com/post");
        let data = parse_generic_oembed(
            7,
            &page,
            br#"{"title":"Title","author_name":"Author","provider_name":"Provider"}"#,
        )
        .unwrap();

        assert_eq!(
            build_generic_embed_card(&page, &data).plain_text,
            "Title\nAuthor"
        );
    }
}
