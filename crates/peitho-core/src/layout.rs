use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    error::Error,
    rc::Rc,
};

use lol_html::{element, errors::RewritingError, HtmlRewriter, Settings};

use crate::{
    domain::{Accepts, Arity, SlotContract, SlotName},
    error::{BuildError, ErrorKind, Result},
};

#[derive(Debug, Clone)]
pub struct Layout {
    name: String,
    html: String,
    slots: BTreeMap<SlotName, SlotContract>,
    root_classes: BTreeSet<String>,
}

impl Layout {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn html(&self) -> &str {
        &self.html
    }

    pub fn slots(&self) -> &BTreeMap<SlotName, SlotContract> {
        &self.slots
    }

    /// Class attribute tokens from this layout's single root `<section>`;
    /// `build_theme_css` validates root-class `width`/`height` overrides
    /// against these classes.
    pub fn root_classes(&self) -> &BTreeSet<String> {
        &self.root_classes
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

/// The ordered set of layouts a deck can dispatch to. Names are unique;
/// the order is the CLI order and keeps dispatch reporting deterministic.
#[derive(Debug, Clone)]
pub struct Layouts {
    layouts: Vec<Layout>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutSummary {
    pub name: String,
    pub slots: Vec<SlotSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotSummary {
    pub name: String,
    pub accepts: String,
    pub arity: String,
}

impl Layouts {
    pub fn new(layouts: Vec<Layout>) -> Result<Self> {
        if layouts.is_empty() {
            return Err(BuildError::new(
                ErrorKind::Layout,
                None,
                "no layouts provided",
                "pass at least one layout HTML",
            ));
        }
        for (index, layout) in layouts.iter().enumerate() {
            if layouts[..index].iter().any(|l| l.name() == layout.name()) {
                return Err(BuildError::new(
                    ErrorKind::Layout,
                    None,
                    format!("duplicate layout name '{}'", layout.name()),
                    "layout names come from file stems; rename one file",
                ));
            }
        }
        Ok(Self { layouts })
    }

    pub fn single(layout: Layout) -> Self {
        Self {
            layouts: vec![layout],
        }
    }

    pub fn get(&self, name: &str) -> Option<&Layout> {
        self.layouts.iter().find(|layout| layout.name() == name)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Layout> {
        self.layouts.iter()
    }

    pub fn len(&self) -> usize {
        self.layouts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.layouts.is_empty()
    }

    pub fn names(&self) -> Vec<&str> {
        self.layouts.iter().map(Layout::name).collect()
    }

    /// Slot class names across all provided layouts; bare `.slot-*`
    /// selectors in theme CSS are validated against this union.
    pub fn slot_classes(&self) -> std::collections::BTreeSet<String> {
        self.layouts
            .iter()
            .flat_map(|layout| layout.slots().keys().map(SlotName::class_name))
            .collect()
    }

    /// Class attribute tokens from the single root `<section>` of every
    /// provided layout; `build_theme_css` validates root-class `width`/`height`
    /// overrides against this union.
    pub fn root_classes(&self) -> BTreeSet<String> {
        self.layouts
            .iter()
            .flat_map(|layout| layout.root_classes().iter().cloned())
            .collect()
    }
}

pub fn describe_layouts(layouts: &Layouts) -> Vec<LayoutSummary> {
    layouts
        .iter()
        .map(|layout| LayoutSummary {
            name: layout.name().to_owned(),
            slots: layout
                .slots()
                .values()
                .map(|slot| SlotSummary {
                    name: slot.name.as_str().to_owned(),
                    accepts: slot.accepts.to_string(),
                    arity: slot.arity.to_string(),
                })
                .collect(),
        })
        .collect()
}

pub fn parse_layout(name: impl Into<String>, html: &str) -> Result<Layout> {
    let slots = Rc::new(RefCell::new(BTreeMap::new()));
    let sink = slots.clone();
    let section_count = Rc::new(RefCell::new(0usize));
    let section_sink = section_count.clone();
    let root_classes = Rc::new(RefCell::new(BTreeSet::new()));
    let root_classes_sink = root_classes.clone();
    let mut rewriter = HtmlRewriter::new(
        Settings {
            element_content_handlers: vec![
                element!("section", move |el| {
                    *section_sink.borrow_mut() += 1;
                    if let Some(classes) = el.get_attribute("class") {
                        root_classes_sink
                            .borrow_mut()
                            .extend(classes.split_whitespace().map(str::to_owned));
                    }
                    Ok(())
                }),
                element!("slot", move |el| {
                    let contract = SlotContract::from_element(el).map_err(box_build_error)?;
                    let key = contract.name.clone();
                    let mut slots = sink.borrow_mut();
                    if slots.contains_key(&key) {
                        return Err(box_build_error(BuildError::new(
                            ErrorKind::Layout,
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
        .map_err(layout_parse_error)?;
    rewriter.end().map_err(layout_parse_error)?;

    let section_count = Rc::try_unwrap(section_count).unwrap().into_inner();
    if section_count != 1 {
        return Err(BuildError::new(
            ErrorKind::Layout,
            None,
            format!("layout must contain exactly one <section>, found {section_count}"),
            "wrap each slide layout in one slide host <section> element",
        ));
    }

    Ok(Layout {
        name: name.into(),
        html: html.to_owned(),
        slots: Rc::try_unwrap(slots).unwrap().into_inner(),
        root_classes: Rc::try_unwrap(root_classes).unwrap().into_inner(),
    })
}

impl SlotContract {
    fn from_element(el: &mut lol_html::html_content::Element<'_, '_>) -> Result<Self> {
        let raw_name = required_attr(el, "name")?;
        let raw_accepts = required_attr(el, "accepts")?;
        let raw_arity = required_attr(el, "arity")?;
        Ok(Self {
            name: SlotName::new(raw_name).map_err(|message| {
                BuildError::new(ErrorKind::Layout, None, message, "rename the slot")
            })?,
            accepts: raw_accepts.parse::<Accepts>().map_err(|message| {
                BuildError::new(
                    ErrorKind::Layout,
                    None,
                    message,
                    "use one of inline, blocks, text, code, image, list",
                )
            })?,
            arity: raw_arity.parse::<Arity>().map_err(|message| {
                BuildError::new(
                    ErrorKind::Layout,
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
            ErrorKind::Layout,
            None,
            format!("slot is missing '{name}'"),
            r#"write <slot name="title" accepts="inline" arity="1"></slot>"#,
        )
    })
}

fn box_build_error(err: BuildError) -> Box<dyn Error + Send + Sync> {
    Box::new(err)
}

fn layout_parse_error(err: RewritingError) -> BuildError {
    match err {
        RewritingError::ContentHandlerError(inner) => match inner.downcast::<BuildError>() {
            Ok(build_error) => *build_error,
            Err(inner) => BuildError::new(
                ErrorKind::Layout,
                None,
                format!("layout content handler failed: {inner}"),
                "keep the layout HTML well-formed and slot attributes complete",
            ),
        },
        other => BuildError::new(
            ErrorKind::Layout,
            None,
            format!("failed to parse layout: {other}"),
            "keep the layout HTML well-formed and slot attributes complete",
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

        let layout = parse_layout("title-body-code", html).unwrap();

        assert_eq!(layout.slot("title").unwrap().accepts, Accepts::Inline);
        assert_eq!(layout.slot("body").unwrap().arity, Arity::ZeroOrMore);
        assert_eq!(layout.slot("code").unwrap().accepts, Accepts::Code);
    }

    #[test]
    fn extracts_root_section_classes() {
        let layout = parse_layout(
            "root",
            r#"<section class="cover code-images"><slot name="title" accepts="inline" arity="1"></slot></section>"#,
        )
        .unwrap();

        assert_eq!(
            layout
                .root_classes()
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["code-images", "cover"]
        );
    }

    #[test]
    fn root_section_classes_are_empty_without_class_attribute() {
        let layout = parse_layout(
            "root",
            r#"<section><slot name="title" accepts="inline" arity="1"></slot></section>"#,
        )
        .unwrap();

        assert!(layout.root_classes().is_empty());
    }

    #[test]
    fn root_section_classes_split_whitespace() {
        let layout = parse_layout(
            "root",
            "<section class=\"  cover\tstatement\nwide  \"><slot name=\"title\" accepts=\"inline\" arity=\"1\"></slot></section>",
        )
        .unwrap();

        assert_eq!(
            layout
                .root_classes()
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["cover", "statement", "wide"]
        );
    }

    #[test]
    fn layouts_root_classes_are_union_across_layouts() {
        let cover = parse_layout(
            "cover",
            r#"<section class="cover shared"><slot name="title" accepts="inline" arity="1"></slot></section>"#,
        )
        .unwrap();
        let body = parse_layout(
            "body",
            r#"<section class="body shared"><slot name="title" accepts="inline" arity="1"></slot></section>"#,
        )
        .unwrap();
        let layouts = Layouts::new(vec![cover, body]).unwrap();

        assert_eq!(
            layouts
                .root_classes()
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["body", "cover", "shared"]
        );
    }

    #[test]
    fn rejects_unknown_accepts_value_with_help() {
        let err = parse_layout(
            "bad",
            r#"<section><slot name="title" accepts="heading" arity="1"></slot></section>"#,
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Layout);
        assert!(err.to_string().contains("unknown accepts value 'heading'"));
        assert_eq!(
            err.help,
            "use one of inline, blocks, text, code, image, list"
        );
    }

    #[test]
    fn rejects_duplicate_slot_name() {
        let err = parse_layout(
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
    fn rejects_layout_without_section() {
        let err = parse_layout(
            "bad",
            r#"<div><slot name="title" accepts="inline" arity="1"></slot></div>"#,
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Layout);
        assert!(err
            .to_string()
            .contains("layout must contain exactly one <section>, found 0"));
        assert_eq!(
            err.help,
            "wrap each slide layout in one slide host <section> element"
        );
    }

    #[test]
    fn rejects_layout_with_multiple_sections() {
        let err = parse_layout(
            "bad",
            r#"<section><slot name="title" accepts="inline" arity="1"></slot></section><section></section>"#,
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Layout);
        assert!(err
            .to_string()
            .contains("layout must contain exactly one <section>, found 2"));
        assert_eq!(
            err.help,
            "wrap each slide layout in one slide host <section> element"
        );
    }

    #[test]
    fn describes_layouts_in_cli_order_with_sorted_slots() {
        let cover = parse_layout(
            "cover",
            r#"<section><slot name="title" accepts="inline" arity="1"></slot></section>"#,
        )
        .unwrap();
        let statement = parse_layout(
            "statement",
            r#"<section>
               <slot name="title" accepts="inline" arity="1"></slot>
               <slot name="body" accepts="blocks" arity="1..*"></slot>
               </section>"#,
        )
        .unwrap();
        let title_body_code = parse_layout(
            "title-body-code",
            r#"<section>
               <slot name="title" accepts="inline" arity="1"></slot>
               <slot name="body" accepts="blocks" arity="0..*"></slot>
               <slot name="code" accepts="code" arity="0..1"></slot>
               </section>"#,
        )
        .unwrap();
        let layouts = Layouts::new(vec![cover, statement, title_body_code]).unwrap();

        let summaries = describe_layouts(&layouts);

        assert_eq!(
            summaries
                .iter()
                .map(|summary| summary.name.as_str())
                .collect::<Vec<_>>(),
            vec!["cover", "statement", "title-body-code"]
        );
        assert_eq!(
            summaries[2]
                .slots
                .iter()
                .map(|slot| (
                    slot.name.as_str(),
                    slot.accepts.as_str(),
                    slot.arity.as_str()
                ))
                .collect::<Vec<_>>(),
            vec![
                ("body", "blocks", "0..*"),
                ("code", "code", "0..1"),
                ("title", "inline", "1")
            ]
        );
    }
}
