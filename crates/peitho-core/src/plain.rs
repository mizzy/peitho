use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use crate::{
    domain::FragmentKind, manifest::ManifestSlideText, phase::CheckedSlide,
    render::BODY_MARKDOWN_OPTIONS,
};

pub(crate) fn slide_text<S>(slide: &CheckedSlide<S>) -> ManifestSlideText {
    let mut title = Vec::new();
    let mut body = Vec::new();
    let mut code = Vec::new();

    for (slot, checked_slot) in slide.slots() {
        let fragments = checked_slot.fragments();
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
                        FragmentKind::Paragraph
                        | FragmentKind::List
                        | FragmentKind::Blockquote
                        | FragmentKind::Table => body_fragment_text(fragment.markdown()),
                        FragmentKind::Math { .. } => fragment.code_text().trim_end().to_owned(),
                        FragmentKind::EmbedCard { .. } => fragment.plain_text().to_owned(),
                        FragmentKind::GenericEmbedCard { .. } => fragment.plain_text().to_owned(),
                        FragmentKind::Footnotes { entries } => entries
                            .iter()
                            .map(|entry| body_fragment_text(entry.markdown()))
                            .filter(|text| !text.is_empty())
                            .collect::<Vec<_>>()
                            .join("\n"),
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
    let mut at_first_cell_of_table_row = false;

    for event in Parser::new_ext(markdown, BODY_MARKDOWN_OPTIONS) {
        match event {
            Event::Start(Tag::Image { .. }) => in_image = true,
            Event::End(TagEnd::Image) => in_image = false,
            Event::FootnoteReference(_) => {}
            Event::Start(Tag::Item) => push_block_separator(&mut text),
            Event::Start(Tag::Paragraph) => push_block_separator(&mut text),
            Event::Start(Tag::Table(_alignments)) => {
                push_block_separator(&mut text);
                at_first_cell_of_table_row = true;
            }
            Event::Start(Tag::TableRow) => {
                push_block_separator(&mut text);
                at_first_cell_of_table_row = true;
            }
            Event::Start(Tag::TableCell) => {
                if !at_first_cell_of_table_row
                    && !text.is_empty()
                    && !text.ends_with(char::is_whitespace)
                {
                    text.push(' ');
                }
                at_first_cell_of_table_row = false;
            }
            Event::End(TagEnd::Table) => at_first_cell_of_table_row = false,
            Event::Text(_) | Event::Code(_) | Event::InlineMath(_) | Event::DisplayMath(_)
                if in_image => {}
            Event::Text(value)
            | Event::Code(value)
            | Event::InlineMath(value)
            | Event::DisplayMath(value) => text.push_str(&value),
            Event::SoftBreak | Event::HardBreak
                if !in_image && !text.is_empty() && !text.ends_with(char::is_whitespace) =>
            {
                text.push(' ');
            }
            Event::Start(Tag::Heading { .. })
            | Event::Start(Tag::CodeBlock(_))
            | Event::Start(Tag::HtmlBlock)
            | Event::Start(Tag::List(_))
            | Event::Start(Tag::BlockQuote(_))
            | Event::Start(Tag::FootnoteDefinition(_))
            | Event::Start(Tag::DefinitionList)
            | Event::Start(Tag::DefinitionListTitle)
            | Event::Start(Tag::DefinitionListDefinition)
            | Event::Start(Tag::Emphasis)
            | Event::Start(Tag::Strong)
            | Event::Start(Tag::Strikethrough)
            | Event::Start(Tag::Superscript)
            | Event::Start(Tag::Subscript)
            | Event::Start(Tag::Link { .. })
            | Event::Start(Tag::MetadataBlock(_))
            | Event::Start(Tag::TableHead)
            | Event::End(TagEnd::Paragraph)
            | Event::End(TagEnd::Heading(_))
            | Event::End(TagEnd::CodeBlock)
            | Event::End(TagEnd::HtmlBlock)
            | Event::End(TagEnd::List(_))
            | Event::End(TagEnd::BlockQuote(_))
            | Event::End(TagEnd::Item)
            | Event::End(TagEnd::FootnoteDefinition)
            | Event::End(TagEnd::DefinitionList)
            | Event::End(TagEnd::DefinitionListTitle)
            | Event::End(TagEnd::DefinitionListDefinition)
            | Event::End(TagEnd::TableHead)
            | Event::End(TagEnd::TableRow)
            | Event::End(TagEnd::TableCell)
            | Event::End(TagEnd::Emphasis)
            | Event::End(TagEnd::Strong)
            | Event::End(TagEnd::Strikethrough)
            | Event::End(TagEnd::Superscript)
            | Event::End(TagEnd::Subscript)
            | Event::End(TagEnd::Link)
            | Event::End(TagEnd::MetadataBlock(_))
            | Event::Html(_)
            | Event::InlineHtml(_)
            | Event::SoftBreak
            | Event::HardBreak
            | Event::Rule
            | Event::TaskListMarker(_) => {}
        }
    }

    text
}

fn push_block_separator(text: &mut String) {
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::{
        check::check_deck,
        domain::{RawImagePath, SlideKey, SlotName, SourceFragment},
        highlight::Highlighter,
        layout::{parse_layout, Layout},
        mapping::map_by_convention,
        parser::{parse_frontmatter, parse_markdown},
        phase::{Checked, CheckedSlot, Deck},
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
    fn body_slot_separates_paragraphs_in_loose_list_item() {
        let text = checked_slide_text("# Title\n\n- first para\n\n  second para");

        assert_eq!(text.body(), "first para\nsecond para");
    }

    #[test]
    fn manifest_body_text_contains_blockquote_text_without_markers() {
        let text = checked_slide_text(
            "# Title\n\n> First **quoted** paragraph.\n>\n> Second paragraph with `code`.",
        );

        assert_eq!(
            text.body(),
            "First quoted paragraph.\nSecond paragraph with code."
        );
        assert!(!text.body().contains('>'));
    }

    #[test]
    fn manifest_body_text_flattens_table_with_cell_and_row_separators() {
        let text = checked_slide_text(
            "# Title\n\n| Name | Score |\n| --- | --- |\n| Ada | 10 |\n| Lin | 20 |",
        );

        assert_eq!(text.body(), "Name Score\nAda 10\nLin 20");
        assert!(!text.body().contains('|'));
    }

    #[test]
    fn manifest_body_text_does_not_prefix_nonempty_header_after_empty_cell() {
        let text = checked_slide_text("# Title\n\n|  | X |\n| --- | --- |\n| 1 | 2 |");

        assert_eq!(text.body(), "X\n1 2");
    }

    #[test]
    fn body_slot_with_inline_code() {
        let text = checked_slide_text("# Title\n\n`foo` bar");

        assert_eq!(text.body(), "foo bar");
    }

    #[test]
    fn body_slot_drops_footnote_reference_marker_and_keeps_footnote_body() {
        let text = checked_slide_text("# Title\n\nClaim[^a].\n\n[^a]: Supporting note.");

        assert_eq!(text.body(), "Claim.\nSupporting note.");
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
    fn manifest_body_text_unaffected_by_breaks() {
        let text = checked_slide_text("---\nbreaks: true\n---\n# Title\n\nfirst\nsecond");

        assert_eq!(text.body(), "first second");
    }

    #[test]
    fn body_slot_image_markdown_produces_no_text() {
        let layout = all_slots_layout();
        let body = SlotName::new("body").unwrap();
        let contract = layout.slot("body").unwrap().clone();
        let mut slots = BTreeMap::new();
        slots.insert(
            body,
            CheckedSlot::new(
                contract,
                vec![SourceFragment::paragraph(1, "![Alt text](x.png)")],
            ),
        );
        let slide = CheckedSlide::new(
            0,
            0,
            SlideKey::new("intro").unwrap(),
            layout,
            slots,
            false,
            0,
            false,
            None,
        );

        let text = slide_text(&slide);

        assert_eq!(text.body(), "");
    }

    #[test]
    fn embed_card_text_enters_manifest_body() {
        let layout = all_slots_layout();
        let body = SlotName::new("body").unwrap();
        let contract = layout.slot("body").unwrap().clone();
        let mut slots = BTreeMap::new();
        slots.insert(
            body,
            CheckedSlot::new(
                contract,
                vec![SourceFragment::embed_card(
                    7,
                    "<article>generated card</article>",
                    "selectable tweet text",
                )],
            ),
        );
        let slide = CheckedSlide::new(
            0,
            0,
            SlideKey::new("card").unwrap(),
            layout,
            slots,
            false,
            0,
            false,
            None,
        );

        let text = slide_text(&slide);

        assert_eq!(text.body(), "selectable tweet text");
    }

    #[test]
    fn generic_card_manifest_text_contains_title_and_author_only() {
        let layout = all_slots_layout();
        let body = SlotName::new("body").unwrap();
        let contract = layout.slot("body").unwrap().clone();
        let mut slots = BTreeMap::new();
        slots.insert(
            body,
            CheckedSlot::new(
                contract,
                vec![SourceFragment::generic_embed_card(
                    7,
                    None::<RawImagePath>,
                    "Title",
                    Some("Title".to_owned()),
                    Some("Author".to_owned()),
                    Some("Provider must stay out".to_owned()),
                    "https://example.com/post",
                    "Title\nAuthor",
                )],
            ),
        );
        let slide = CheckedSlide::new(
            0,
            0,
            SlideKey::new("generic-card").unwrap(),
            layout,
            slots,
            false,
            0,
            false,
            None,
        );

        let text = slide_text(&slide);

        assert_eq!(text.body(), "Title\nAuthor");
        assert!(!text.body().contains("Provider"));
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
