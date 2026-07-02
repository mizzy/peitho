use std::collections::BTreeMap;

use crate::{
    domain::{FragmentKind, SlotName, SourceFragment},
    error::Result,
    phase::{Deck, Mapped, MappedSlide, MappedSlot, Parsed, UnassignedFragment},
    template::Template,
};

pub fn map_by_convention(deck: Deck<Parsed>, template: &Template) -> Result<Deck<Mapped>> {
    let mut slides = Vec::new();
    for slide in deck.into_parsed_slides() {
        let title_line = shallowest_heading_line(&slide.fragments);
        let mut slots: BTreeMap<SlotName, MappedSlot> = BTreeMap::new();
        let mut unassigned = Vec::new();

        for fragment in slide.fragments {
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
            if let Some(contract) = template.slot_by_name(&slot).cloned() {
                slots
                    .entry(slot.clone())
                    .or_insert_with(|| MappedSlot::new(contract))
                    .push(fragment);
            } else {
                unassigned.push(UnassignedFragment::new(slot, fragment));
            }
        }

        slides.push(MappedSlide {
            index: slide.index,
            key: slide.key,
            slots,
            unassigned,
        });
    }
    Ok(Deck::mapped(slides))
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
    use crate::{domain::SlotName, parser::parse_markdown, template::parse_template};

    #[test]
    fn maps_title_body_and_code_slots_by_convention() {
        let markdown =
            "<!-- {\"key\":\"arch-1\"} -->\n# Architecture\n\nBody\n\n```rust\nfn main() {}\n```";
        let template = parse_template(
            "title-body-code",
            r#"<section>
               <slot name="title" accepts="inline" arity="1"></slot>
               <slot name="body" accepts="blocks" arity="0..*"></slot>
               <slot name="code" accepts="code" arity="0..1"></slot>
               </section>"#,
        )
        .unwrap();

        let mapped = map_by_convention(parse_markdown(markdown).unwrap(), &template).unwrap();
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
}
