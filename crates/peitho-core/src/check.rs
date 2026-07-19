use std::collections::BTreeMap;

use crate::{
    domain::{Accepts, FragmentKind, SlotName, SourceFragment},
    error::{BuildError, ErrorKind, Result},
    layout::Layout,
    phase::{
        Checked, CheckedSlide, CheckedSlot, Deck, Mapped, MappedSlide, MappedSlot,
        UnassignedFragment,
    },
};

pub fn check_deck(deck: Deck<Mapped>) -> Result<Deck<Checked>> {
    let (settings, mapped_slides) = deck.into_mapped_parts();
    let mut slides = Vec::new();
    for slide in mapped_slides {
        let slide_number = slide.source_index + 1;
        let slide_key = slide.key.as_str().to_owned();
        check_slide(&slide).map_err(|err| err.with_slide(slide_number, Some(&slide_key)))?;
        let mut mapped_slots = slide.slots;
        let checked_slots = slide
            .layout
            .slots()
            .iter()
            .map(|(slot, contract)| {
                let (checked_contract, fragments) = mapped_slots
                    .remove(slot)
                    .map(|mapped_slot| {
                        (
                            mapped_slot.contract().clone(),
                            mapped_slot.fragments().to_vec(),
                        )
                    })
                    .unwrap_or_else(|| (contract.clone(), Vec::new()));
                (slot.clone(), CheckedSlot::new(checked_contract, fragments))
            })
            .collect();
        check_no_unknown_mapped_slots(&mapped_slots, &slide.layout)
            .map_err(|err| err.with_slide(slide_number, Some(&slide_key)))?;
        slides.push(CheckedSlide::new(
            slide.index,
            slide.source_index,
            slide.key,
            slide.layout,
            checked_slots,
            slide.skip,
            slide.page_number_hidden,
            slide.notes,
        ));
    }
    Ok(Deck::checked(settings, slides))
}

fn check_no_unknown_mapped_slots(
    mapped_slots: &BTreeMap<SlotName, MappedSlot>,
    layout: &Layout,
) -> Result<()> {
    if mapped_slots.is_empty() {
        return Ok(());
    }
    let names = mapped_slots
        .keys()
        .map(SlotName::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    Err(BuildError::new(
        ErrorKind::Layout,
        None,
        format!(
            "mapped slide for layout '{}' contains slot(s) not declared by layout: {names}",
            layout.name()
        ),
        "keep mapping output synchronized with the layout slot contracts",
    ))
}

/// Validate one mapped slide against the layout it carries. Also used by
/// dispatch to probe whether a slide structurally fits a candidate layout,
/// so there is exactly one source of contract truth.
pub(crate) fn check_slide(slide: &MappedSlide) -> Result<()> {
    check_accepts(&slide.slots)?;
    check_arity(&slide.slots, &slide.layout)?;
    check_no_unassigned(&slide.unassigned)
}

fn check_accepts(slots: &BTreeMap<SlotName, MappedSlot>) -> Result<()> {
    for (slot, mapped_slot) in slots {
        let contract = mapped_slot.contract();
        for fragment in mapped_slot.fragments() {
            if !accepts_fragment(contract.accepts, fragment) {
                return Err(BuildError::new(
                    ErrorKind::Accepts,
                    Some(fragment.line()),
                    format!(
                        "slot '{}' accepts {}, but got {}",
                        slot.as_str(),
                        contract.accepts,
                        fragment.kind()
                    ),
                    format!(
                        "change the layout accepts to '{}' or move this content to a {} slot",
                        fragment.kind().default_accepts(),
                        fragment.kind().default_accepts()
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn accepts_fragment(accepts: Accepts, fragment: &SourceFragment) -> bool {
    matches!(
        (accepts, fragment.kind()),
        (Accepts::Inline, FragmentKind::Heading { .. })
            | (Accepts::Blocks, FragmentKind::Heading { .. })
            | (Accepts::Blocks, FragmentKind::Paragraph)
            | (Accepts::Blocks, FragmentKind::List)
            | (Accepts::Blocks, FragmentKind::Math { .. })
            | (Accepts::Blocks, FragmentKind::Footnotes { .. })
            | (Accepts::Text, FragmentKind::Text)
            | (Accepts::Code, FragmentKind::Code)
            | (Accepts::Image, FragmentKind::Image { .. })
            | (Accepts::List, FragmentKind::List)
    )
}

fn check_arity(slots: &BTreeMap<SlotName, MappedSlot>, layout: &Layout) -> Result<()> {
    for (slot, contract) in layout.slots() {
        let count = slots
            .get(slot)
            .map(|mapped| mapped.fragments().len())
            .unwrap_or(0);
        if !contract.arity.allows(count) {
            let line = slots
                .get(slot)
                .and_then(|mapped| mapped.fragments().first())
                .map(SourceFragment::line);
            let help = if count == 0 && slot.as_str() == "title" {
                "add a heading for the title slot".to_owned()
            } else if count == 0 {
                format!("add content for the {} slot", slot.as_str())
            } else {
                format!(
                    "use a layout with more {} capacity or remove one {} block",
                    slot.as_str(),
                    slot.as_str()
                )
            };
            return Err(BuildError::new(
                ErrorKind::Arity,
                line,
                format!(
                    "slot '{}' got {} item(s), but layout '{}' allows {}",
                    slot.as_str(),
                    count,
                    layout.name(),
                    contract.arity
                ),
                help,
            ));
        }
    }
    Ok(())
}

fn check_no_unassigned(unassigned: &[UnassignedFragment]) -> Result<()> {
    if let Some(unassigned) = unassigned.first() {
        let fragment = unassigned.fragment();
        let target = unassigned.expected_slot().as_str();
        return Err(BuildError::new(
            ErrorKind::ResidualContent,
            Some(fragment.line()),
            format!("unassigned content remains for missing '{target}' slot"),
            format!(
                "add a '{target}' slot to the layout or remove the {}",
                fragment.kind().removal_noun()
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        domain::{Arity, FootnoteEntry, RawImagePath, SlideKey, SlotContract},
        error::ErrorKind,
        layout::parse_layout,
        mapping::map_by_convention,
        parser::{parse_frontmatter, parse_markdown as parse_markdown_impl},
        phase::{DeckSettings, MappedSlide, MappedSlot},
    };

    fn parse_markdown(
        source: &str,
        highlighter: &crate::highlight::Highlighter,
    ) -> crate::error::Result<crate::phase::Deck<crate::phase::Parsed>> {
        let frontmatter = parse_frontmatter(source)?;
        parse_markdown_impl(source, frontmatter, highlighter)
    }

    fn mapped_deck_with_math_slot(
        slot_name: &str,
        accepts: &str,
    ) -> crate::phase::Deck<crate::phase::Mapped> {
        let layout = parse_layout(
            "math-layout",
            &format!(
                r#"<section><slot name="{slot_name}" accepts="{accepts}" arity="1"></slot></section>"#
            ),
        )
        .unwrap();
        let slot = SlotName::new(slot_name).unwrap();
        let mut mapped_slot = MappedSlot::new(layout.slot(slot_name).unwrap().clone());
        mapped_slot.push(SourceFragment::math(
            7,
            r#"<span class="katex-display">math html</span>"#,
            r#"\frac{1}{2}"#,
        ));
        let mut slots = BTreeMap::new();
        slots.insert(slot, mapped_slot);

        Deck::mapped(
            DeckSettings::default(),
            vec![MappedSlide {
                index: 0,
                source_index: 0,
                key: SlideKey::new("intro").unwrap(),
                layout,
                slots,
                unassigned: Vec::new(),
                skip: false,
                page_number_hidden: false,
                notes: None,
            }],
        )
    }

    fn mapped_deck_with_footnotes_slot(
        slot_name: &str,
        accepts: &str,
    ) -> crate::phase::Deck<crate::phase::Mapped> {
        let layout = parse_layout(
            "footnotes-layout",
            &format!(
                r#"<section><slot name="{slot_name}" accepts="{accepts}" arity="1"></slot></section>"#
            ),
        )
        .unwrap();
        let slot = SlotName::new(slot_name).unwrap();
        let mut mapped_slot = MappedSlot::new(layout.slot(slot_name).unwrap().clone());
        mapped_slot.push(SourceFragment::footnotes(
            7,
            vec![FootnoteEntry::new(1, "note", "Footnote body.", 7)],
        ));
        let mut slots = BTreeMap::new();
        slots.insert(slot, mapped_slot);

        Deck::mapped(
            DeckSettings::default(),
            vec![MappedSlide {
                index: 0,
                source_index: 0,
                key: SlideKey::new("intro").unwrap(),
                layout,
                slots,
                unassigned: Vec::new(),
                skip: false,
                page_number_hidden: false,
                notes: None,
            }],
        )
    }

    #[test]
    fn rejects_paragraph_in_inline_slot_with_line_and_help() {
        let layout = parse_layout(
            "bad-body",
            r#"<section>
               <slot name="title" accepts="inline" arity="1"></slot>
               <slot name="body" accepts="inline" arity="0..*"></slot>
               </section>"#,
        )
        .unwrap();
        let mapped = map_by_convention(
            parse_markdown(
                "# Title\n\nBody paragraph",
                &crate::highlight::Highlighter::defaults(),
            )
            .unwrap(),
            &layout,
        )
        .unwrap();

        let err = check_deck(mapped).unwrap_err();

        assert_eq!(err.kind, ErrorKind::Accepts);
        assert_eq!(err.line, Some(3));
        assert!(err.to_string().contains("slot 'body' accepts inline"));
        assert_eq!(
            err.help,
            "change the layout accepts to 'blocks' or move this content to a blocks slot"
        );
    }

    #[test]
    fn check_deck_carries_skip_flag_to_checked_slide() {
        let layout = parse_layout(
            "title-only",
            r#"<section><slot name="title" accepts="inline" arity="1"></slot></section>"#,
        )
        .unwrap();
        let mapped = map_by_convention(
            parse_markdown(
                "<!-- {\"skip\":true} -->\n# Appendix",
                &crate::highlight::Highlighter::defaults(),
            )
            .unwrap(),
            &layout,
        )
        .unwrap();
        let checked = check_deck(mapped).unwrap();

        assert!(checked.checked_slides()[0].skip());
    }

    #[test]
    fn accepts_math_fragment_in_blocks_slot() {
        let checked = check_deck(mapped_deck_with_math_slot("body", "blocks")).unwrap();

        assert_eq!(checked.checked_slides()[0].slots().len(), 1);
        let body = SlotName::new("body").unwrap();
        assert!(matches!(
            checked.checked_slides()[0].slots()[&body].fragments()[0].kind(),
            FragmentKind::Math { .. }
        ));
    }

    #[test]
    fn rejects_math_fragment_in_code_slot_with_contract_error() {
        let err = check_deck(mapped_deck_with_math_slot("code", "code")).unwrap_err();

        assert_eq!(err.kind, ErrorKind::Accepts);
        assert_eq!(err.line, Some(7));
        assert!(err.to_string().contains("slot 'code' accepts code"));
        assert_eq!(
            err.help,
            "change the layout accepts to 'blocks' or move this content to a blocks slot"
        );
    }

    #[test]
    fn accepts_footnotes_fragment_in_blocks_slot() {
        let checked = check_deck(mapped_deck_with_footnotes_slot("body", "blocks")).unwrap();

        assert_eq!(checked.checked_slides()[0].slots().len(), 1);
        let body = SlotName::new("body").unwrap();
        assert!(matches!(
            checked.checked_slides()[0].slots()[&body].fragments()[0].kind(),
            FragmentKind::Footnotes { .. }
        ));
    }

    #[test]
    fn rejects_footnotes_fragment_in_code_slot_with_contract_error() {
        let err = check_deck(mapped_deck_with_footnotes_slot("code", "code")).unwrap_err();

        assert_eq!(err.kind, ErrorKind::Accepts);
        assert_eq!(err.line, Some(7));
        assert!(err.to_string().contains("slot 'code' accepts code"));
        assert_eq!(
            err.help,
            "change the layout accepts to 'blocks' or move this content to a blocks slot"
        );
    }

    #[test]
    fn layout_without_body_slot_reports_footnotes_as_residual_content() {
        let layout = parse_layout(
            "title-only",
            r#"<section><slot name="title" accepts="inline" arity="1"></slot></section>"#,
        )
        .unwrap();
        let mapped = map_by_convention(
            parse_markdown(
                "# Title[^note]\n\n[^note]: Footnote body.",
                &crate::highlight::Highlighter::defaults(),
            )
            .unwrap(),
            &layout,
        )
        .unwrap();

        let err = check_deck(mapped).unwrap_err();

        assert_eq!(err.kind, ErrorKind::ResidualContent);
        assert_eq!(err.line, Some(3));
        assert!(err
            .to_string()
            .contains("unassigned content remains for missing 'body' slot"));
        assert_eq!(
            err.help,
            "add a 'body' slot to the layout or remove the footnote block"
        );
    }

    #[test]
    fn check_errors_use_source_slide_number_after_draft_drop() {
        let layout = parse_layout(
            "title-only",
            r#"<section><slot name="title" accepts="inline" arity="1"></slot></section>"#,
        )
        .unwrap();
        let mapped = map_by_convention(
            parse_markdown(
                "# Live\n\n---\n\
                 <!-- {\"draft\":true} -->\n# Draft\n\n---\n\
                 Body only",
                &crate::highlight::Highlighter::defaults(),
            )
            .unwrap(),
            &layout,
        )
        .unwrap();

        let err = check_deck(mapped).unwrap_err();

        assert_eq!(err.kind, ErrorKind::Arity);
        assert!(err.to_string().contains("slide 3 ('slide-3')"));
        assert!(err.to_string().contains("slot 'title' got 0 item(s)"));
    }

    #[test]
    fn rejects_two_code_blocks_for_zero_or_one_code_slot() {
        let markdown = "# Title\n\n```rust\nfn a() {}\n```\n\n```rust\nfn b() {}\n```";
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

        let err = check_deck(mapped).unwrap_err();

        assert_eq!(err.kind, ErrorKind::Arity);
        assert_eq!(err.line, Some(3));
        assert!(err.to_string().contains("slot 'code' got 2 item(s)"));
        assert_eq!(
            err.help,
            "use a layout with more code capacity or remove one code block"
        );
    }

    #[test]
    fn rejects_missing_required_title_slot() {
        let layout = parse_layout(
            "title-only",
            r#"<section><slot name="title" accepts="inline" arity="1"></slot></section>"#,
        )
        .unwrap();
        let mapped = map_by_convention(
            parse_markdown("Body only", &crate::highlight::Highlighter::defaults()).unwrap(),
            &layout,
        )
        .unwrap();

        let err = check_deck(mapped).unwrap_err();

        assert_eq!(err.kind, ErrorKind::Arity);
        assert!(err.to_string().contains("slot 'title' got 0 item(s)"));
        assert_eq!(err.help, "add a heading for the title slot");
    }

    #[test]
    fn rejects_unassigned_code_when_layout_has_no_code_slot() {
        let layout = parse_layout(
            "title-body",
            r#"<section>
               <slot name="title" accepts="inline" arity="1"></slot>
               <slot name="body" accepts="blocks" arity="0..*"></slot>
               </section>"#,
        )
        .unwrap();
        let markdown = "# Title\n\n```rust\nfn lost() {}\n```";
        let mapped = map_by_convention(
            parse_markdown(markdown, &crate::highlight::Highlighter::defaults()).unwrap(),
            &layout,
        )
        .unwrap();

        let err = check_deck(mapped).unwrap_err();

        assert_eq!(err.kind, ErrorKind::ResidualContent);
        assert_eq!(err.line, Some(3));
        assert!(err.to_string().contains("unassigned content remains"));
        assert_eq!(
            err.help,
            "add a 'code' slot to the layout or remove the code block"
        );
    }

    #[test]
    fn rejects_unassigned_secondary_heading_as_body_content() {
        let layout = parse_layout(
            "title-only",
            r#"<section><slot name="title" accepts="inline" arity="1"></slot></section>"#,
        )
        .unwrap();
        let markdown = "# Title\n\n## Detail";
        let mapped = map_by_convention(
            parse_markdown(markdown, &crate::highlight::Highlighter::defaults()).unwrap(),
            &layout,
        )
        .unwrap();

        let err = check_deck(mapped).unwrap_err();

        assert_eq!(err.kind, ErrorKind::ResidualContent);
        assert_eq!(err.line, Some(3));
        assert_eq!(
            err.help,
            "add a 'body' slot to the layout or remove the heading"
        );
    }

    #[test]
    fn check_accepts_image_fragment_in_image_slot() {
        let layout = parse_layout(
            "image",
            r#"<section><slot name="hero" accepts="image" arity="1"></slot></section>"#,
        )
        .unwrap();
        let hero = SlotName::new("hero").unwrap();
        let image = SourceFragment::image(
            7,
            "Architecture",
            RawImagePath::new_unchecked("x.png".into()),
        );

        assert!(accepts_fragment(Accepts::Image, &image));
        assert!(!accepts_fragment(Accepts::Blocks, &image));

        let contract = layout.slot_by_name(&hero).unwrap().clone();
        let mut mapped_slot = MappedSlot::new(contract);
        mapped_slot.push(image);
        let mut slots = BTreeMap::new();
        slots.insert(hero, mapped_slot);
        let slide = MappedSlide {
            index: 0,
            source_index: 0,
            key: SlideKey::new("image").unwrap(),
            layout,
            slots,
            unassigned: Vec::new(),
            skip: false,
            page_number_hidden: false,
            notes: None,
        };

        check_slide(&slide).unwrap();
    }

    #[test]
    fn rejects_mapped_slot_not_declared_by_layout_before_checked_conversion() {
        let layout = parse_layout(
            "title-only",
            r#"<section><slot name="title" accepts="inline" arity="1"></slot></section>"#,
        )
        .unwrap();
        let title = SlotName::new("title").unwrap();
        let extra = SlotName::new("extra").unwrap();
        let mut title_slot = MappedSlot::new(layout.slot_by_name(&title).unwrap().clone());
        title_slot.push(SourceFragment::heading(1, 1, "# Title", "Title"));
        let mut extra_slot = MappedSlot::new(SlotContract {
            name: extra.clone(),
            accepts: Accepts::Blocks,
            arity: Arity::ZeroOrMore,
        });
        extra_slot.push(SourceFragment::paragraph(3, "Should not disappear"));
        let mut slots = BTreeMap::new();
        slots.insert(title, title_slot);
        slots.insert(extra, extra_slot);
        let mapped = Deck::mapped(
            crate::phase::DeckSettings::default(),
            vec![MappedSlide {
                index: 0,
                source_index: 0,
                key: SlideKey::new("title").unwrap(),
                layout,
                slots,
                unassigned: Vec::new(),
                skip: false,
                page_number_hidden: false,
                notes: None,
            }],
        );

        let err = check_deck(mapped).unwrap_err();

        assert_eq!(err.kind, ErrorKind::Layout);
        assert!(err.to_string().contains("slide 1 ('title')"));
        assert!(err
            .to_string()
            .contains("slot(s) not declared by layout: extra"));
        assert_eq!(
            err.help,
            "keep mapping output synchronized with the layout slot contracts"
        );
    }

    #[test]
    fn rejects_heading_in_image_slot_named_title() {
        let layout = parse_layout(
            "bad-title-image",
            r#"<section><slot name="title" accepts="image" arity="1"></slot></section>"#,
        )
        .unwrap();
        let mapped = map_by_convention(
            parse_markdown("# Title", &crate::highlight::Highlighter::defaults()).unwrap(),
            &layout,
        )
        .unwrap();

        let err = check_deck(mapped).unwrap_err();

        assert_eq!(err.kind, ErrorKind::Accepts);
        assert!(err.to_string().contains("line 1"));
        assert!(err
            .to_string()
            .contains("slot 'title' accepts image, but got heading"));
        assert!(err.help.contains("change the layout accepts to 'inline'"));
    }

    #[test]
    fn arity_error_in_second_slide_includes_slide_context() {
        let markdown = "# Intro\n\n---\n<!-- {\"key\":\"code-slide\"} -->\n# Code\n\n```rust\nfn a() {}\n```\n\n```rust\nfn b() {}\n```";
        let layout = parse_layout(
            "title-body-code",
            r#"<section>
               <slot name="title" accepts="inline" arity="1"></slot>
               <slot name="code" accepts="code" arity="0..1"></slot>
               </section>"#,
        )
        .unwrap();
        let mapped = map_by_convention(
            parse_markdown(markdown, &crate::highlight::Highlighter::defaults()).unwrap(),
            &layout,
        )
        .unwrap();

        let err = check_deck(mapped).unwrap_err();

        assert_eq!(err.kind, ErrorKind::Arity);
        assert!(err.to_string().contains("slide 2 ('code-slide'), line 7"));
        assert!(err.to_string().contains("slot 'code' got 2 item(s)"));
    }
}
