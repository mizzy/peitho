use std::collections::BTreeMap;

use crate::{
    check::check_slide,
    domain::{FragmentKind, SlotName, SourceFragment},
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
        return Ok(map_slide(&slide, layout));
    }

    if layouts.len() == 1 {
        let layout = layouts.iter().next().expect("single layout exists");
        return Ok(map_slide(&slide, layout));
    }

    let mut matches = Vec::new();
    let mut rejections = Vec::new();
    for layout in layouts.iter() {
        let mapped = map_slide(&slide, layout);
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

fn map_slide(slide: &ParsedSlide, layout: &Layout) -> MappedSlide {
    let title_line = shallowest_heading_line(&slide.fragments);
    let mut slots: BTreeMap<SlotName, MappedSlot> = BTreeMap::new();
    let mut unassigned = Vec::new();

    for fragment in slide.fragments.iter().cloned() {
        let target = match fragment.kind() {
            FragmentKind::Heading { .. } if Some(fragment.line()) == title_line => "title",
            FragmentKind::Code => "code",
            FragmentKind::Heading { .. }
            | FragmentKind::Paragraph
            | FragmentKind::List
            | FragmentKind::Image
            | FragmentKind::Text => "body",
        };
        let slot = SlotName::new(target).expect("conventional slot names are valid");
        if let Some(contract) = layout.slot_by_name(&slot).cloned() {
            slots
                .entry(slot.clone())
                .or_insert_with(|| MappedSlot::new(contract))
                .push(fragment);
        } else {
            unassigned.push(UnassignedFragment::new(slot, fragment));
        }
    }

    MappedSlide {
        index: slide.index,
        key: slide.key.clone(),
        layout: layout.clone(),
        slots,
        unassigned,
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
    use crate::{domain::SlotName, layout::parse_layout, parser::parse_markdown};

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

        let mapped =
            dispatch_by_convention(parse_markdown(markdown).unwrap(), &cover_and_statement())
                .unwrap();

        assert_eq!(mapped.mapped_slides()[0].layout.name(), "cover");
        assert_eq!(mapped.mapped_slides()[1].layout.name(), "statement");
    }

    #[test]
    fn explicit_layout_request_wins_over_dispatch() {
        let markdown = "<!-- {\"layout\":\"statement\"} -->\n# Title\n\nBody";

        let mapped =
            dispatch_by_convention(parse_markdown(markdown).unwrap(), &cover_and_statement())
                .unwrap();

        assert_eq!(mapped.mapped_slides()[0].layout.name(), "statement");
    }

    #[test]
    fn unknown_explicit_layout_is_an_error_listing_known_names() {
        let markdown = "<!-- {\"layout\":\"missing\"} -->\n# Title";

        let err = dispatch_by_convention(parse_markdown(markdown).unwrap(), &cover_and_statement())
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

        let err =
            dispatch_by_convention(parse_markdown("# Title Only").unwrap(), &layouts).unwrap_err();

        assert!(err
            .to_string()
            .contains("slide matches multiple layouts: cover, closing"));
        assert!(err.help.contains(r#"{"layout":"…"}"#));
    }

    #[test]
    fn unmatched_slide_error_lists_each_layouts_reason() {
        let markdown = "# Title\n\n```rust\nfn main() {}\n```";

        let err = dispatch_by_convention(parse_markdown(markdown).unwrap(), &cover_and_statement())
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
        let mapped =
            dispatch_by_convention(parse_markdown("# Title\n\nBody").unwrap(), &layouts).unwrap();

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

        let mapped = map_by_convention(parse_markdown(markdown).unwrap(), &layout).unwrap();
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

        let mapped = map_by_convention(parse_markdown(markdown).unwrap(), &layout).unwrap();

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
