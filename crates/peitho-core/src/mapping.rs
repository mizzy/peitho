use std::collections::BTreeMap;

use crate::{
    check::check_slide,
    domain::{Accepts, FragmentKind, SlotName, SourceFragment},
    error::{BuildError, ErrorKind, Result},
    layout::{Layout, Layouts},
    phase::{Deck, Mapped, MappedSlide, MappedSlot, Parsed, ParsedSlide, UnassignedFragment},
};

/// Dispatch every slide to a layout, then map its fragments by convention.
///
/// The rules, in order and deterministic:
/// 1. an explicit `{"layout":"name"}` request wins; an unknown name is a
///    build error listing the known layouts
/// 2. with a single layout available it is used unconditionally, so contract
///    violations surface as the precise accepts/arity/residual errors
/// 3. with several layouts, the slide is probed against each in CLI order;
///    exactly one structural match is required — none or several are build
///    errors that name the candidates and how to disambiguate
pub fn dispatch_by_convention(deck: Deck<Parsed>, layouts: &Layouts) -> Result<Deck<Mapped>> {
    let (settings, parsed_slides) = deck.into_parsed_parts();
    let mut slides = Vec::new();
    for slide in parsed_slides {
        let slide_number = slide.index + 1;
        let slide_key = slide.key.as_str().to_owned();
        let mapped = dispatch_slide(slide, layouts)
            .map_err(|err| err.with_slide(slide_number, Some(&slide_key)))?;
        slides.push(mapped);
    }
    Ok(Deck::mapped(settings, slides))
}

/// Single-layout convenience.
pub fn map_by_convention(deck: Deck<Parsed>, layout: &Layout) -> Result<Deck<Mapped>> {
    dispatch_by_convention(deck, &Layouts::single(layout.clone()))
}

fn dispatch_slide(slide: ParsedSlide, layouts: &Layouts) -> Result<MappedSlide> {
    if let Some(request) = &slide.layout_request {
        let Some(layout) = layouts.get(&request.name) else {
            return Err(BuildError::new(
                ErrorKind::Layout,
                Some(request.line),
                format!("unknown layout '{}'", request.name),
                format!("use one of: {}", layouts.names().join(", ")),
            ));
        };
        return map_slide(&slide, layout);
    }

    if layouts.len() == 1 {
        let layout = layouts.iter().next().expect("single layout exists");
        return map_slide(&slide, layout);
    }

    let mut matches = Vec::new();
    let mut rejections = Vec::new();
    for layout in layouts.iter() {
        let mapped = match map_slide(&slide, layout) {
            Ok(mapped) => mapped,
            Err(err) => {
                rejections.push(format!("{}: {}", layout.name(), err.message));
                continue;
            }
        };
        match check_slide(&mapped) {
            Ok(()) => matches.push(mapped),
            Err(err) => rejections.push(format!("{}: {}", layout.name(), err.message)),
        }
    }

    let line = slide.fragments.first().map(SourceFragment::line);
    match matches.len() {
        1 => Ok(matches.remove(0)),
        0 => Err(BuildError::new(
            ErrorKind::Layout,
            line,
            format!("no layout matches this slide\n{}", rejections.join("\n")),
            r#"adjust the slide content or pick a layout explicitly with <!-- {"layout":"…"} -->"#,
        )),
        _ => {
            let names = matches
                .iter()
                .map(|mapped| mapped.layout.name().to_owned())
                .collect::<Vec<_>>()
                .join(", ");
            Err(BuildError::new(
                ErrorKind::Layout,
                line,
                format!("slide matches multiple layouts: {names}"),
                r#"pick one explicitly with <!-- {"layout":"…"} -->"#,
            ))
        }
    }
}

fn map_slide(slide: &ParsedSlide, layout: &Layout) -> Result<MappedSlide> {
    let title_line = shallowest_heading_line(&slide.fragments);
    let mut slots: BTreeMap<SlotName, MappedSlot> = BTreeMap::new();
    let mut unassigned = Vec::new();

    for fragment in slide.fragments.iter().cloned() {
        let target = match fragment.kind() {
            FragmentKind::Heading { .. } if Some(fragment.line()) == title_line => {
                SlotName::new("title").expect("conventional slot names are valid")
            }
            FragmentKind::Code => SlotName::new("code").expect("conventional slot names are valid"),
            FragmentKind::Image { .. } => {
                // Images must be claimed by an explicit image slot; never fall
                // through to body/block mapping.
                image_slot_name(layout, fragment.line())?
            }
            FragmentKind::Heading { .. }
            | FragmentKind::Paragraph
            | FragmentKind::List
            | FragmentKind::Text => {
                SlotName::new("body").expect("conventional slot names are valid")
            }
        };
        if let Some(contract) = layout.slot_by_name(&target).cloned() {
            slots
                .entry(target.clone())
                .or_insert_with(|| MappedSlot::new(contract))
                .push(fragment);
        } else {
            unassigned.push(UnassignedFragment::new(target, fragment));
        }
    }

    Ok(MappedSlide {
        index: slide.index,
        key: slide.key.clone(),
        layout: layout.clone(),
        slots,
        unassigned,
        notes: slide.notes.clone(),
    })
}

fn image_slot_name(layout: &Layout, line: usize) -> Result<SlotName> {
    let image_slots = layout
        .slots()
        .values()
        .filter(|contract| contract.accepts == Accepts::Image)
        .collect::<Vec<_>>();
    match image_slots.as_slice() {
        [] => Err(BuildError::new(
            ErrorKind::Layout,
            Some(line),
            format!("no slot accepts image in layout '{}'", layout.name()),
            "add exactly one slot with accepts=\"image\" or remove the image",
        )),
        [slot] => Ok(slot.name.clone()),
        many => {
            let names = many
                .iter()
                .map(|slot| slot.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            Err(BuildError::new(
                ErrorKind::Layout,
                Some(line),
                format!(
                    "layout '{}' has multiple slots accepting image: {names}",
                    layout.name()
                ),
                "keep exactly one image slot in the layout",
            ))
        }
    }
}

fn shallowest_heading_line(fragments: &[SourceFragment]) -> Option<usize> {
    fragments
        .iter()
        .filter_map(|fragment| match fragment.kind() {
            FragmentKind::Heading { level } => Some((level, fragment.line())),
            _ => None,
        })
        .min_by_key(|(level, line)| (*level, *line))
        .map(|(_level, line)| line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        check::check_deck, domain::SlotName, layout::parse_layout, parser::parse_markdown,
    };

    fn cover_and_statement() -> Layouts {
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
        Layouts::new(vec![cover, statement]).unwrap()
    }

    #[test]
    fn dispatches_each_slide_to_the_unique_structural_match() {
        let markdown = "# Cover Only\n\n---\n# Statement\n\nBody paragraph";

        let mapped = dispatch_by_convention(
            parse_markdown(markdown, &crate::highlight::Highlighter::defaults()).unwrap(),
            &cover_and_statement(),
        )
        .unwrap();

        assert_eq!(mapped.mapped_slides()[0].layout.name(), "cover");
        assert_eq!(mapped.mapped_slides()[1].layout.name(), "statement");
    }

    #[test]
    fn explicit_layout_request_wins_over_dispatch() {
        let markdown = "<!-- {\"layout\":\"statement\"} -->\n# Title\n\nBody";

        let mapped = dispatch_by_convention(
            parse_markdown(markdown, &crate::highlight::Highlighter::defaults()).unwrap(),
            &cover_and_statement(),
        )
        .unwrap();

        assert_eq!(mapped.mapped_slides()[0].layout.name(), "statement");
    }

    #[test]
    fn unknown_explicit_layout_is_an_error_listing_known_names() {
        let markdown = "<!-- {\"layout\":\"missing\"} -->\n# Title";

        let err = dispatch_by_convention(
            parse_markdown(markdown, &crate::highlight::Highlighter::defaults()).unwrap(),
            &cover_and_statement(),
        )
        .unwrap_err();

        assert_eq!(err.line, Some(1));
        assert!(err.to_string().contains("unknown layout 'missing'"));
        assert_eq!(err.help, "use one of: cover, statement");
    }

    #[test]
    fn ambiguous_slide_is_an_error_naming_the_candidates() {
        let cover = parse_layout(
            "cover",
            r#"<section><slot name="title" accepts="inline" arity="1"></slot></section>"#,
        )
        .unwrap();
        let closing = parse_layout(
            "closing",
            r#"<section><slot name="title" accepts="inline" arity="1"></slot></section>"#,
        )
        .unwrap();
        let layouts = Layouts::new(vec![cover, closing]).unwrap();

        let err = dispatch_by_convention(
            parse_markdown("# Title Only", &crate::highlight::Highlighter::defaults()).unwrap(),
            &layouts,
        )
        .unwrap_err();

        assert!(err
            .to_string()
            .contains("slide matches multiple layouts: cover, closing"));
        assert!(err.help.contains(r#"{"layout":"…"}"#));
    }

    #[test]
    fn unmatched_slide_error_lists_each_layouts_reason() {
        let markdown = "# Title\n\n```rust\nfn main() {}\n```";

        let err = dispatch_by_convention(
            parse_markdown(markdown, &crate::highlight::Highlighter::defaults()).unwrap(),
            &cover_and_statement(),
        )
        .unwrap_err();

        let message = err.to_string();
        assert!(message.contains("no layout matches this slide"));
        assert!(message.contains("cover:"));
        assert!(message.contains("statement:"));
        assert!(err.help.contains(r#"{"layout":"…"}"#));
    }

    #[test]
    fn single_layout_is_used_unconditionally_so_check_reports_precise_errors() {
        let cover = parse_layout(
            "cover",
            r#"<section><slot name="title" accepts="inline" arity="1"></slot></section>"#,
        )
        .unwrap();
        let layouts = Layouts::new(vec![cover]).unwrap();

        // Body would not fit `cover`, but with a single layout dispatch does
        // not reject the slide; the check phase reports the residual error.
        let mapped = dispatch_by_convention(
            parse_markdown(
                "# Title\n\nBody",
                &crate::highlight::Highlighter::defaults(),
            )
            .unwrap(),
            &layouts,
        )
        .unwrap();

        assert_eq!(mapped.mapped_slides()[0].layout.name(), "cover");
        assert_eq!(mapped.mapped_slides()[0].unassigned.len(), 1);
    }

    #[test]
    fn maps_title_body_and_code_slots_by_convention() {
        let markdown =
            "<!-- {\"key\":\"arch-1\"} -->\n# Architecture\n\nBody\n\n```rust\nfn main() {}\n```";
        let layout = parse_layout(
            "title-body-code",
            r#"<section>
               <slot name="title" accepts="inline" arity="1"></slot>
               <slot name="body" accepts="blocks" arity="0..*"></slot>
               <slot name="code" accepts="code" arity="0..1"></slot>
               </section>"#,
        )
        .unwrap();

        let mapped = map_by_convention(
            parse_markdown(markdown, &crate::highlight::Highlighter::defaults()).unwrap(),
            &layout,
        )
        .unwrap();
        let slide = &mapped.mapped_slides()[0];

        assert_eq!(
            slide.slots[&SlotName::new("title").unwrap()]
                .fragments()
                .len(),
            1
        );
        assert_eq!(
            slide.slots[&SlotName::new("body").unwrap()]
                .fragments()
                .len(),
            1
        );
        assert_eq!(
            slide.slots[&SlotName::new("code").unwrap()]
                .fragments()
                .len(),
            1
        );
        assert!(slide.unassigned.is_empty());
    }

    #[test]
    fn maps_image_to_unique_image_accepting_slot() {
        let layout = parse_layout(
            "hero",
            r#"<section>
               <slot name="title" accepts="inline" arity="1"></slot>
               <slot name="hero" accepts="image" arity="1"></slot>
               </section>"#,
        )
        .unwrap();

        let mapped = map_by_convention(
            parse_markdown(
                "# Title\n\n![Architecture](x.png)",
                &crate::highlight::Highlighter::defaults(),
            )
            .unwrap(),
            &layout,
        )
        .unwrap();
        let slide = &mapped.mapped_slides()[0];
        let title = SlotName::new("title").unwrap();
        let hero = SlotName::new("hero").unwrap();

        assert_eq!(slide.slots[&title].fragments().len(), 1);
        assert_eq!(
            slide.slots[&title].fragments()[0].kind(),
            &FragmentKind::Heading { level: 1 }
        );
        assert_eq!(slide.slots[&hero].fragments().len(), 1);
        match slide.slots[&hero].fragments()[0].kind() {
            FragmentKind::Image { alt, src } => {
                assert_eq!(alt, "Architecture");
                assert_eq!(src.as_str(), "x.png");
            }
            other => panic!("expected image fragment, got {other:?}"),
        }
        assert!(slide.unassigned.is_empty());
    }

    #[test]
    fn rejects_image_when_layout_has_no_image_slot() {
        let layout = parse_layout(
            "title-body",
            r#"<section>
               <slot name="title" accepts="inline" arity="1"></slot>
               <slot name="body" accepts="blocks" arity="0..*"></slot>
               </section>"#,
        )
        .unwrap();

        let err = map_by_convention(
            parse_markdown(
                "# Title\n\n![Architecture](x.png)",
                &crate::highlight::Highlighter::defaults(),
            )
            .unwrap(),
            &layout,
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Layout);
        assert_eq!(err.line, Some(3));
        assert!(err.to_string().contains("no slot accepts image"));
    }

    #[test]
    fn rejects_image_when_multiple_image_slots_are_ambiguous() {
        let layout = parse_layout(
            "double-image",
            r#"<section>
               <slot name="title" accepts="inline" arity="1"></slot>
               <slot name="hero" accepts="image" arity="1"></slot>
               <slot name="diagram" accepts="image" arity="1"></slot>
               </section>"#,
        )
        .unwrap();

        let err = map_by_convention(
            parse_markdown(
                "# Title\n\n![Architecture](x.png)",
                &crate::highlight::Highlighter::defaults(),
            )
            .unwrap(),
            &layout,
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Layout);
        assert_eq!(err.line, Some(3));
        assert!(err
            .to_string()
            .contains("layout 'double-image' has multiple slots accepting image"));
    }

    #[test]
    fn dispatch_selects_layout_with_image_slot_as_unique_structural_match() {
        let cover = parse_layout(
            "cover",
            r#"<section><slot name="title" accepts="inline" arity="1"></slot></section>"#,
        )
        .unwrap();
        let visual = parse_layout(
            "visual",
            r#"<section>
               <slot name="title" accepts="inline" arity="1"></slot>
               <slot name="hero" accepts="image" arity="1"></slot>
               </section>"#,
        )
        .unwrap();
        let layouts = Layouts::new(vec![cover, visual]).unwrap();

        let mapped = dispatch_by_convention(
            parse_markdown(
                "# Title\n\n![Architecture](x.png)",
                &crate::highlight::Highlighter::defaults(),
            )
            .unwrap(),
            &layouts,
        )
        .unwrap();

        assert_eq!(mapped.mapped_slides()[0].layout.name(), "visual");
    }

    #[test]
    fn dispatch_rejects_two_image_layout_matches() {
        let visual = parse_layout(
            "visual",
            r#"<section>
               <slot name="title" accepts="inline" arity="1"></slot>
               <slot name="hero" accepts="image" arity="1"></slot>
               </section>"#,
        )
        .unwrap();
        let diagram = parse_layout(
            "diagram",
            r#"<section>
               <slot name="title" accepts="inline" arity="1"></slot>
               <slot name="hero" accepts="image" arity="1"></slot>
               </section>"#,
        )
        .unwrap();
        let layouts = Layouts::new(vec![visual, diagram]).unwrap();

        let err = dispatch_by_convention(
            parse_markdown(
                "# Title\n\n![Architecture](x.png)",
                &crate::highlight::Highlighter::defaults(),
            )
            .unwrap(),
            &layouts,
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Layout);
        assert!(err
            .to_string()
            .contains("slide matches multiple layouts: visual, diagram"));
        assert!(err.help.contains(r#"{"layout":"…"}"#));
    }

    #[test]
    fn explicit_layout_without_image_slot_rejects_image_without_fallback() {
        let cover = parse_layout(
            "cover",
            r#"<section><slot name="title" accepts="inline" arity="1"></slot></section>"#,
        )
        .unwrap();
        let visual = parse_layout(
            "visual",
            r#"<section>
               <slot name="title" accepts="inline" arity="1"></slot>
               <slot name="hero" accepts="image" arity="1"></slot>
               </section>"#,
        )
        .unwrap();
        let layouts = Layouts::new(vec![cover, visual]).unwrap();

        let err = dispatch_by_convention(
            parse_markdown(
                "<!-- {\"layout\":\"cover\"} -->\n# Title\n\n![A](x.png)",
                &crate::highlight::Highlighter::defaults(),
            )
            .unwrap(),
            &layouts,
        )
        .unwrap_err();

        assert_eq!(err.kind, ErrorKind::Layout);
        assert_eq!(err.line, Some(4));
        assert!(err
            .to_string()
            .contains("no slot accepts image in layout 'cover'"));
    }

    #[test]
    fn explicit_image_layout_without_image_content_obeys_image_slot_arity() {
        let optional = parse_layout(
            "visual-optional",
            r#"<section>
               <slot name="title" accepts="inline" arity="1"></slot>
               <slot name="hero" accepts="image" arity="0..*"></slot>
               </section>"#,
        )
        .unwrap();
        let required = parse_layout(
            "visual-required",
            r#"<section>
               <slot name="title" accepts="inline" arity="1"></slot>
               <slot name="hero" accepts="image" arity="1"></slot>
               </section>"#,
        )
        .unwrap();

        let optional_mapped = dispatch_by_convention(
            parse_markdown(
                "<!-- {\"layout\":\"visual-optional\"} -->\n# Title",
                &crate::highlight::Highlighter::defaults(),
            )
            .unwrap(),
            &Layouts::single(optional),
        )
        .unwrap();
        check_deck(optional_mapped).unwrap();

        let required_mapped = dispatch_by_convention(
            parse_markdown(
                "<!-- {\"layout\":\"visual-required\"} -->\n# Title",
                &crate::highlight::Highlighter::defaults(),
            )
            .unwrap(),
            &Layouts::single(required),
        )
        .unwrap();
        let err = check_deck(required_mapped).unwrap_err();

        assert_eq!(err.kind, ErrorKind::Arity);
        assert!(err
            .to_string()
            .contains("slot 'hero' got 0 item(s), but layout 'visual-required' allows 1"));
    }

    #[test]
    fn maps_each_slide_independently() {
        let markdown = "# Intro\n\nBody\n\n---\n# Architecture\n\n```rust\nfn main() {}\n```";
        let layout = parse_layout(
            "title-body-code",
            r#"<section>
               <slot name="title" accepts="inline" arity="1"></slot>
               <slot name="body" accepts="blocks" arity="0..*"></slot>
               <slot name="code" accepts="code" arity="0..1"></slot>
               </section>"#,
        )
        .unwrap();

        let mapped = map_by_convention(
            parse_markdown(markdown, &crate::highlight::Highlighter::defaults()).unwrap(),
            &layout,
        )
        .unwrap();

        assert_eq!(mapped.mapped_slides().len(), 2);
        assert_eq!(
            mapped.mapped_slides()[0].slots[&SlotName::new("body").unwrap()]
                .fragments()
                .len(),
            1
        );
        assert_eq!(
            mapped.mapped_slides()[1].slots[&SlotName::new("code").unwrap()]
                .fragments()
                .len(),
            1
        );
    }
}
