use std::{cell::RefCell, collections::BTreeMap, error::Error, rc::Rc};

use lol_html::{element, errors::RewritingError, HtmlRewriter, Settings};

use crate::{
    domain::{Accepts, Arity, SlotContract, SlotName},
    error::{BuildError, ErrorKind, Result},
};

#[derive(Debug, Clone)]
pub struct Template {
    name: String,
    html: String,
    slots: BTreeMap<SlotName, SlotContract>,
}

impl Template {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn html(&self) -> &str {
        &self.html
    }

    pub fn slots(&self) -> &BTreeMap<SlotName, SlotContract> {
        &self.slots
    }

    pub fn slot(&self, name: &str) -> Option<&SlotContract> {
        SlotName::new(name)
            .ok()
            .and_then(|slot| self.slots.get(&slot))
    }

    pub fn slot_by_name(&self, name: &SlotName) -> Option<&SlotContract> {
        self.slots.get(name)
    }
}

pub fn parse_template(name: impl Into<String>, html: &str) -> Result<Template> {
    let slots = Rc::new(RefCell::new(BTreeMap::new()));
    let sink = slots.clone();
    let section_count = Rc::new(RefCell::new(0usize));
    let section_sink = section_count.clone();
    let mut rewriter = HtmlRewriter::new(
        Settings {
            element_content_handlers: vec![
                element!("section", move |_el| {
                    *section_sink.borrow_mut() += 1;
                    Ok(())
                }),
                element!("slot", move |el| {
                    let contract = SlotContract::from_element(el).map_err(box_build_error)?;
                    let key = contract.name.clone();
                    let mut slots = sink.borrow_mut();
                    if slots.contains_key(&key) {
                        return Err(box_build_error(BuildError::new(
                            ErrorKind::Template,
                            None,
                            format!("duplicate slot '{}'", key.as_str()),
                            "rename one slot so every slot contract has a unique name",
                        )));
                    }
                    slots.insert(key, contract);
                    Ok(())
                }),
            ],
            ..Settings::default()
        },
        |_chunk: &[u8]| {},
    );
    rewriter
        .write(html.as_bytes())
        .map_err(template_parse_error)?;
    rewriter.end().map_err(template_parse_error)?;

    let section_count = Rc::try_unwrap(section_count).unwrap().into_inner();
    if section_count != 1 {
        return Err(BuildError::new(
            ErrorKind::Template,
            None,
            format!("template must contain exactly one <section>, found {section_count}"),
            "wrap each slide template in one slide host <section> element",
        ));
    }

    Ok(Template {
        name: name.into(),
        html: html.to_owned(),
        slots: Rc::try_unwrap(slots).unwrap().into_inner(),
    })
}

impl SlotContract {
    fn from_element(el: &mut lol_html::html_content::Element<'_, '_>) -> Result<Self> {
        let raw_name = required_attr(el, "name")?;
        let raw_accepts = required_attr(el, "accepts")?;
        let raw_arity = required_attr(el, "arity")?;
        Ok(Self {
            name: SlotName::new(raw_name).map_err(|message| {
                BuildError::new(ErrorKind::Template, None, message, "rename the slot")
            })?,
            accepts: raw_accepts.parse::<Accepts>().map_err(|message| {
                BuildError::new(
                    ErrorKind::Template,
                    None,
                    message,
                    "use one of inline, blocks, text, code, image, list",
                )
            })?,
            arity: raw_arity.parse::<Arity>().map_err(|message| {
                BuildError::new(
                    ErrorKind::Template,
                    None,
                    message,
                    "use one of 1, 0..1, 1..*, 0..*",
                )
            })?,
        })
    }
}

fn required_attr(el: &lol_html::html_content::Element<'_, '_>, name: &str) -> Result<String> {
    el.get_attribute(name).ok_or_else(|| {
        BuildError::new(
            ErrorKind::Template,
            None,
            format!("slot is missing '{name}'"),
            r#"write <slot name="title" accepts="inline" arity="1"></slot>"#,
        )
    })
}

fn box_build_error(err: BuildError) -> Box<dyn Error + Send + Sync> {
    Box::new(err)
}

fn template_parse_error(err: RewritingError) -> BuildError {
    match err {
        RewritingError::ContentHandlerError(inner) => match inner.downcast::<BuildError>() {
            Ok(build_error) => *build_error,
            Err(inner) => BuildError::new(
                ErrorKind::Template,
                None,
                format!("template content handler failed: {inner}"),
                "keep the template HTML well-formed and slot attributes complete",
            ),
        },
        other => BuildError::new(
            ErrorKind::Template,
            None,
            format!("failed to parse template: {other}"),
            "keep the template HTML well-formed and slot attributes complete",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Accepts, Arity};

    #[test]
    fn extracts_title_body_code_slot_contracts() {
        let html = r#"<section class="slide">
  <h1><slot name="title" accepts="inline" arity="1"></slot></h1>
  <div><slot name="body" accepts="blocks" arity="0..*"></slot></div>
  <figure><slot name="code" accepts="code" arity="0..1"></slot></figure>
</section>"#;

        let template = parse_template("title-body-code", html).unwrap();

        assert_eq!(template.slot("title").unwrap().accepts, Accepts::Inline);
        assert_eq!(template.slot("body").unwrap().arity, Arity::ZeroOrMore);
        assert_eq!(template.slot("code").unwrap().accepts, Accepts::Code);
    }

    #[test]
    fn rejects_unknown_accepts_value_with_help() {
        let err = parse_template(
            "bad",
            r#"<section><slot name="title" accepts="heading" arity="1"></slot></section>"#,
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Template);
        assert!(err.to_string().contains("unknown accepts value 'heading'"));
        assert_eq!(
            err.help,
            "use one of inline, blocks, text, code, image, list"
        );
    }

    #[test]
    fn rejects_duplicate_slot_name() {
        let err = parse_template(
            "bad",
            r#"<section>
               <slot name="title" accepts="inline" arity="1"></slot>
               <slot name="title" accepts="blocks" arity="0..*"></slot>
               </section>"#,
        )
        .unwrap_err();

        assert!(err.to_string().contains("duplicate slot 'title'"));
    }

    #[test]
    fn rejects_template_without_section() {
        let err = parse_template(
            "bad",
            r#"<div><slot name="title" accepts="inline" arity="1"></slot></div>"#,
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Template);
        assert!(err
            .to_string()
            .contains("template must contain exactly one <section>, found 0"));
        assert_eq!(
            err.help,
            "wrap each slide template in one slide host <section> element"
        );
    }

    #[test]
    fn rejects_template_with_multiple_sections() {
        let err = parse_template(
            "bad",
            r#"<section><slot name="title" accepts="inline" arity="1"></slot></section><section></section>"#,
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Template);
        assert!(err
            .to_string()
            .contains("template must contain exactly one <section>, found 2"));
        assert_eq!(
            err.help,
            "wrap each slide template in one slide host <section> element"
        );
    }
}
