use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use crate::{domain::FragmentKind, manifest::ManifestSlideText, phase::CheckedSlide};

pub(crate) fn slide_text<S>(slide: &CheckedSlide<S>) -> ManifestSlideText {
    let mut title = Vec::new();
    let mut body = Vec::new();
    let mut code = Vec::new();

    for (slot, fragments) in slide.slots() {
        match slot.as_str() {
            "title" => {
                title.extend(
                    fragments
                        .iter()
                        .filter(|fragment| matches!(fragment.kind(), FragmentKind::Heading { .. }))
                        .map(|fragment| fragment.plain_text().to_owned()),
                );
            }
            "body" => {
                body.extend(fragments.iter().filter_map(|fragment| {
                    let text = match fragment.kind() {
                        FragmentKind::Heading { .. } | FragmentKind::Text => {
                            fragment.plain_text().to_owned()
                        }
                        FragmentKind::Paragraph | FragmentKind::List => {
                            body_fragment_text(fragment.markdown())
                        }
                        FragmentKind::Image { .. }
                        | FragmentKind::Code
                        | FragmentKind::SlotGroup { .. } => return None,
                    };
                    if text.is_empty() {
                        None
                    } else {
                        Some(text)
                    }
                }));
            }
            "code" => {
                code.extend(
                    fragments
                        .iter()
                        .filter(|fragment| matches!(fragment.kind(), FragmentKind::Code))
                        .map(|fragment| fragment.code_text().to_owned()),
                );
            }
            _ => {}
        }
    }

    ManifestSlideText::new(title.join("\n"), body.join("\n"), code.join("\n"))
}

fn body_fragment_text(markdown: &str) -> String {
    let mut text = String::new();
    let mut in_image = false;

    for event in Parser::new_ext(markdown, Options::empty()) {
        match event {
            Event::Start(Tag::Image { .. }) => in_image = true,
            Event::End(TagEnd::Image) => in_image = false,
            _ if in_image => {}
            Event::Start(Tag::Item) if !text.is_empty() && !text.ends_with('\n') => {
                text.push('\n');
            }
            Event::Text(value) | Event::Code(value) => text.push_str(&value),
            Event::SoftBreak | Event::HardBreak
                if !text.is_empty() && !text.ends_with(char::is_whitespace) =>
            {
                text.push(' ');
            }
            _ => {}
        }
    }

    text
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::{
        check::check_deck,
        domain::{SlideKey, SlotName, SourceFragment},
        highlight::Highlighter,
        layout::{parse_layout, Layout},
        mapping::map_by_convention,
        parser::{parse_frontmatter, parse_markdown},
        phase::{Checked, Deck},
    };

    #[test]
    fn title_slot_with_single_heading() {
        let text = checked_slide_text("# Peitho");

        assert_eq!(text.title(), "Peitho");
        assert_eq!(text.body(), "");
        assert_eq!(text.code(), "");
    }

    #[test]
    fn title_slot_with_markdown_inline() {
        let text = checked_slide_text("# **Bold** heading");

        assert_eq!(text.title(), "Bold heading");
    }

    #[test]
    fn title_slot_missing_is_empty() {
        let text = checked_slide_text("Body only");

        assert_eq!(text.title(), "");
    }

    #[test]
    fn body_slot_with_two_paragraphs() {
        let text = checked_slide_text("# Title\n\nFirst paragraph\n\nSecond paragraph");

        assert_eq!(text.body(), "First paragraph\nSecond paragraph");
    }

    #[test]
    fn body_slot_includes_subheading() {
        let text = checked_slide_text("# Title\n\n## Subheading\n\nBody paragraph");

        assert_eq!(text.body(), "Subheading\nBody paragraph");
    }

    #[test]
    fn body_slot_includes_heading_and_paragraph_in_order() {
        let text = checked_slide_text("# Title\n\n## Before\n\nMiddle paragraph\n\n## After");

        assert_eq!(text.body(), "Before\nMiddle paragraph\nAfter");
    }

    #[test]
    fn body_slot_with_list() {
        let text = checked_slide_text("# Title\n\n- item1\n- item2");

        assert_eq!(text.body(), "item1\nitem2");
    }

    #[test]
    fn body_slot_with_inline_code() {
        let text = checked_slide_text("# Title\n\n`foo` bar");

        assert_eq!(text.body(), "foo bar");
    }

    #[test]
    fn body_slot_with_link_keeps_link_text_only() {
        let text = checked_slide_text("# Title\n\n[click](https://example.com)");

        assert_eq!(text.body(), "click");
    }

    #[test]
    fn body_slot_with_soft_break_uses_single_space() {
        let text = checked_slide_text("# Title\n\nfirst\nsecond");

        assert_eq!(text.body(), "first second");
    }

    #[test]
    fn body_slot_with_hard_break_uses_single_space() {
        let text = checked_slide_text("# Title\n\nfirst\\\nsecond");

        assert_eq!(text.body(), "first second");
    }

    #[test]
    fn body_slot_image_markdown_produces_no_text() {
        let body = SlotName::new("body").unwrap();
        let mut slots = BTreeMap::new();
        slots.insert(
            body,
            vec![SourceFragment::paragraph(1, "![Alt text](x.png)")],
        );
        let slide = CheckedSlide::new(
            0,
            SlideKey::new("intro").unwrap(),
            all_slots_layout(),
            slots,
            None,
        );

        let text = slide_text(&slide);

        assert_eq!(text.body(), "");
    }

    #[test]
    fn body_slot_missing_is_empty() {
        let text = checked_slide_text("# Title");

        assert_eq!(text.body(), "");
    }

    #[test]
    fn code_slot_with_one_code_block_preserves_newline() {
        let text = checked_slide_text("# Title\n\n```rust\nfn main() {}\n```");

        assert_eq!(text.code(), "fn main() {}\n");
    }

    #[test]
    fn code_slot_with_two_code_blocks_is_blank_line_separated() {
        let text =
            checked_slide_text("# Title\n\n```rust\nfn one() {}\n```\n\n```rust\nfn two() {}\n```");

        assert_eq!(text.code(), "fn one() {}\n\nfn two() {}\n");
    }

    #[test]
    fn code_slot_missing_is_empty() {
        let text = checked_slide_text("# Title");

        assert_eq!(text.code(), "");
    }

    #[test]
    fn explicit_nonstandard_slot_is_ignored() {
        let text = checked_slide_text("::: {slot=aside}\n\n# Aside\n\nAside body\n\n:::");

        assert_eq!(text.title(), "");
        assert_eq!(text.body(), "");
        assert_eq!(text.code(), "");
    }

    #[test]
    fn mixed_title_body_and_code_slide() {
        let text = checked_slide_text(
            "# Peitho\n\nFirst paragraph\n\n- item1\n- item2\n\n```rust\nfn main() {}\n```",
        );

        assert_eq!(text.title(), "Peitho");
        assert_eq!(text.body(), "First paragraph\nitem1\nitem2");
        assert_eq!(text.code(), "fn main() {}\n");
    }

    fn checked_slide_text(markdown: &str) -> ManifestSlideText {
        let checked = checked_deck(markdown, all_slots_layout());
        slide_text(&checked.checked_slides()[0])
    }

    fn checked_deck(markdown: &str, layout: Layout) -> Deck<Checked> {
        let frontmatter = parse_frontmatter(markdown).unwrap();
        check_deck(
            map_by_convention(
                parse_markdown(markdown, frontmatter, &Highlighter::defaults()).unwrap(),
                &layout,
            )
            .unwrap(),
        )
        .unwrap()
    }

    fn all_slots_layout() -> Layout {
        parse_layout(
            "all-slots",
            r#"<section>
               <slot name="title" accepts="inline" arity="0..1"></slot>
               <slot name="body" accepts="blocks" arity="0..*"></slot>
               <slot name="code" accepts="code" arity="0..*"></slot>
               <slot name="aside" accepts="blocks" arity="0..*"></slot>
               </section>"#,
        )
        .unwrap()
    }
}
