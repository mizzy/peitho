use std::{collections::BTreeMap, error::Error};

use html_escape::encode_text;
use lol_html::{
    element, errors::RewritingError, html_content::ContentType, HtmlRewriter, Settings,
};
use pulldown_cmark::{html, Options, Parser};

use crate::{
    domain::{FragmentKind, RenderedSlide, SlideKey, SlotName, SourceFragment},
    error::{BuildError, ErrorKind, Result},
    phase::{Checked, Deck, Rendered},
    template::Template,
};

pub fn render_deck(deck: Deck<Checked>, template: &Template) -> Result<Deck<Rendered>> {
    let mut slides = Vec::new();
    for slide in deck.into_checked_slides() {
        let html = render_slide(slide.key(), slide.slots(), template)?;
        slides.push(RenderedSlide::new(slide.index(), slide.key().clone(), html));
    }
    Ok(Deck::rendered(slides, String::new()))
}

fn render_slide(
    key: &SlideKey,
    slots: &BTreeMap<SlotName, Vec<SourceFragment>>,
    template: &Template,
) -> Result<String> {
    let mut output = Vec::new();
    let key_value = key.as_str().to_owned();
    let slot_values = slots.clone();
    let mut rewriter = HtmlRewriter::new(
        Settings {
            element_content_handlers: vec![
                element!("section", move |el| {
                    el.set_attribute("data-slide-key", &key_value)?;
                    let existing = el.get_attribute("class").unwrap_or_default();
                    let class = if existing
                        .split_whitespace()
                        .any(|part| part == "peitho-slide")
                    {
                        existing
                    } else if existing.is_empty() {
                        "peitho-slide".to_owned()
                    } else {
                        format!("{existing} peitho-slide")
                    };
                    el.set_attribute("class", &class)?;
                    Ok(())
                }),
                element!("slot", move |el| {
                    let raw_name = el.get_attribute("name").ok_or_else(|| {
                        box_build_error(BuildError::new(
                            ErrorKind::Template,
                            None,
                            "slot is missing 'name'",
                            "add a name attribute to the slot",
                        ))
                    })?;
                    let slot = SlotName::new(raw_name).map_err(|message| {
                        box_build_error(BuildError::new(
                            ErrorKind::Template,
                            None,
                            message,
                            "rename the slot",
                        ))
                    })?;
                    let fragments = slot_values.get(&slot).cloned().unwrap_or_default();
                    el.replace(&render_slot(&slot, &fragments), ContentType::Html);
                    Ok(())
                }),
            ],
            ..Settings::default()
        },
        |chunk: &[u8]| output.extend_from_slice(chunk),
    );
    rewriter
        .write(template.html.as_bytes())
        .map_err(render_error)?;
    rewriter.end().map_err(render_error)?;
    String::from_utf8(output).map_err(|err| {
        BuildError::new(
            ErrorKind::Template,
            None,
            format!("rendered HTML is not UTF-8: {err}"),
            "keep templates and generated fragments as UTF-8",
        )
    })
}

fn render_slot(slot: &SlotName, fragments: &[SourceFragment]) -> String {
    let class_name = slot.class_name();
    match fragments.first().map(SourceFragment::kind) {
        Some(FragmentKind::Heading { .. }) => {
            let text = fragments
                .iter()
                .map(SourceFragment::plain_text)
                .collect::<Vec<_>>()
                .join(" ");
            format!(r#"<span class="{class_name}">{}</span>"#, encode_text(&text))
        }
        Some(FragmentKind::Code) => {
            let code = fragments
                .iter()
                .map(SourceFragment::code_text)
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                r#"<pre class="{class_name}"><code>{}</code></pre>"#,
                encode_text(&code)
            )
        }
        _ => {
            let markdown = fragments
                .iter()
                .map(SourceFragment::markdown)
                .collect::<Vec<_>>()
                .join("\n\n");
            let mut body = String::new();
            html::push_html(&mut body, Parser::new_ext(&markdown, Options::empty()));
            format!(r#"<div class="{class_name}">{body}</div>"#)
        }
    }
}

fn box_build_error(err: BuildError) -> Box<dyn Error + Send + Sync> {
    Box::new(err)
}

fn render_error(err: RewritingError) -> BuildError {
    match err {
        RewritingError::ContentHandlerError(inner) => match inner.downcast::<BuildError>() {
            Ok(build_error) => *build_error,
            Err(inner) => BuildError::new(
                ErrorKind::Template,
                None,
                format!("render content handler failed: {inner}"),
                "keep slot elements well-formed and avoid malformed HTML in the template",
            ),
        },
        other => BuildError::new(
            ErrorKind::Template,
            None,
            format!("failed to render template: {other}"),
            "keep slot elements well-formed and avoid malformed HTML in the template",
        ),
    }
}

pub fn render_index(slides: &[RenderedSlide]) -> String {
    let body = slides
        .iter()
        .map(RenderedSlide::html)
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <link rel="stylesheet" href="peitho.css">
  <title>Peitho Deck</title>
</head>
<body>
{body}
</body>
</html>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        check::check_deck, mapping::map_by_convention, parser::parse_markdown,
        template::parse_template,
    };

    #[test]
    fn renders_checked_slide_with_key_and_slot_classes() {
        let markdown = "<!-- {\"key\":\"arch-1\"} -->\n# Architecture\n\nBody\n\n```rust\nfn main() {}\n```";
        let template = parse_template(
            "title-body-code",
            r#"<section class="slide">
  <h1><slot name="title" accepts="inline" arity="1"></slot></h1>
  <div class="body"><slot name="body" accepts="blocks" arity="0..*"></slot></div>
  <figure class="code"><slot name="code" accepts="code" arity="0..1"></slot></figure>
</section>"#,
        )
        .unwrap();
        let checked = check_deck(
            map_by_convention(parse_markdown(markdown).unwrap(), &template).unwrap(),
            &template,
        )
        .unwrap();

        let rendered = render_deck(checked, &template).unwrap();
        let html = rendered.slides()[0].html();

        assert!(html.contains(r#"data-slide-key="arch-1""#));
        assert!(html.contains(r#"class="slot-title""#));
        assert!(html.contains(r#"class="slot-body""#));
        assert!(html.contains(r#"class="slot-code""#));
        assert!(html.contains("fn main() {}"));
    }
}
