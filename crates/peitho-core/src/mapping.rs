use std::collections::BTreeMap;

use crate::{
    check::check_slide,
    domain::{Accepts, FragmentKind, SlotName, SourceFragment},
    error::{BuildError, ErrorKind, Result},
    layout::{Layout, Layouts},
    phase::{Deck, Mapped, MappedSlide, MappedSlot, Parsed, ParsedSlide, UnassignedFragment},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchTrace {
    Explicit {
        layout: String,
        line: usize,
        result: DispatchResult,
    },
    SoleLayout {
        layout: String,
        result: DispatchResult,
    },
    StructuralMatch {
        candidates: Vec<Candidate>,
        result: DispatchResult,
    },
}

impl DispatchTrace {
    pub fn result(&self) -> &DispatchResult {
        match self {
            Self::Explicit { result, .. }
            | Self::SoleLayout { result, .. }
            | Self::StructuralMatch { result, .. } => result,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchResult {
    Matched(String),
    NoMatch { reason: Option<String> },
    Ambiguous(Vec<String>),
    UnknownLayout(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub layout: String,
    pub outcome: CandidateOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidateOutcome {
    Matched,
    Rejected { reason: String },
}

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
        let slide_number = slide.source_index + 1;
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

pub fn explain_dispatch(slide: &ParsedSlide, layouts: &Layouts) -> DispatchTrace {
    try_dispatch(slide, layouts).trace
}

struct DispatchAttempt {
    trace: DispatchTrace,
    mapped: Option<MappedSlide>,
    error: Option<BuildError>,
}

impl DispatchAttempt {
    fn matched(trace: DispatchTrace, mapped: MappedSlide) -> Self {
        Self {
            trace,
            mapped: Some(mapped),
            error: None,
        }
    }

    fn failed(trace: DispatchTrace, error: BuildError) -> Self {
        Self {
            trace,
            mapped: None,
            error: Some(error),
        }
    }
}

fn dispatch_slide(slide: ParsedSlide, layouts: &Layouts) -> Result<MappedSlide> {
    let attempt = try_dispatch(&slide, layouts);
    match (attempt.mapped, attempt.error) {
        (Some(mapped), None) => Ok(mapped),
        (None, Some(err)) => Err(err),
        _ => unreachable!("dispatch attempts are either matched or failed"),
    }
}

fn try_dispatch(slide: &ParsedSlide, layouts: &Layouts) -> DispatchAttempt {
    if let Some(request) = &slide.layout_request {
        let Some(layout) = layouts.get(&request.name) else {
            let trace = DispatchTrace::Explicit {
                layout: request.name.clone(),
                line: request.line,
                result: DispatchResult::UnknownLayout(request.name.clone()),
            };
            return DispatchAttempt::failed(trace, unknown_layout_error(request, layouts));
        };
        return match map_slide(slide, layout) {
            Ok(mapped) => {
                let trace = DispatchTrace::Explicit {
                    layout: request.name.clone(),
                    line: request.line,
                    result: DispatchResult::Matched(layout.name().to_owned()),
                };
                DispatchAttempt::matched(trace, mapped)
            }
            Err(err) => {
                let reason = err.message.clone();
                let trace = DispatchTrace::Explicit {
                    layout: request.name.clone(),
                    line: request.line,
                    result: DispatchResult::NoMatch {
                        reason: Some(reason),
                    },
                };
                DispatchAttempt::failed(trace, err)
            }
        };
    }

    if layouts.len() == 1 {
        let layout = layouts.iter().next().expect("single layout exists");
        return match map_slide(slide, layout) {
            Ok(mapped) => {
                let trace = DispatchTrace::SoleLayout {
                    layout: layout.name().to_owned(),
                    result: DispatchResult::Matched(layout.name().to_owned()),
                };
                DispatchAttempt::matched(trace, mapped)
            }
            Err(err) => {
                let reason = err.message.clone();
                let trace = DispatchTrace::SoleLayout {
                    layout: layout.name().to_owned(),
                    result: DispatchResult::NoMatch {
                        reason: Some(reason),
                    },
                };
                DispatchAttempt::failed(trace, err)
            }
        };
    }

    let mut matches = Vec::new();
    let mut rejections = Vec::new();
    let mut candidates = Vec::new();
    for layout in layouts.iter() {
        let mapped = match map_slide(slide, layout) {
            Ok(mapped) => mapped,
            Err(err) => {
                candidates.push(Candidate {
                    layout: layout.name().to_owned(),
                    outcome: CandidateOutcome::Rejected {
                        reason: err.message.clone(),
                    },
                });
                rejections.push(format!("{}: {}", layout.name(), err.message));
                continue;
            }
        };
        match check_slide(&mapped) {
            Ok(()) => {
                candidates.push(Candidate {
                    layout: layout.name().to_owned(),
                    outcome: CandidateOutcome::Matched,
                });
                matches.push(mapped);
            }
            Err(err) => {
                candidates.push(Candidate {
                    layout: layout.name().to_owned(),
                    outcome: CandidateOutcome::Rejected {
                        reason: err.message.clone(),
                    },
                });
                rejections.push(format!("{}: {}", layout.name(), err.message));
            }
        }
    }

    let line = slide.fragments.first().map(SourceFragment::line);
    match matches.len() {
        1 => {
            let mapped = matches.remove(0);
            let trace = DispatchTrace::StructuralMatch {
                candidates,
                result: DispatchResult::Matched(mapped.layout.name().to_owned()),
            };
            DispatchAttempt::matched(trace, mapped)
        }
        0 => {
            let trace = DispatchTrace::StructuralMatch {
                candidates,
                result: DispatchResult::NoMatch { reason: None },
            };
            DispatchAttempt::failed(
                trace,
                BuildError::new(
                    ErrorKind::Layout,
                    line,
                    format!("no layout matches this slide\n{}", rejections.join("\n")),
                    r#"adjust the slide content or pick a layout explicitly with <!-- {"layout":"…"} -->"#,
                ),
            )
        }
        _ => {
            let names = matches
                .iter()
                .map(|mapped| mapped.layout.name().to_owned())
                .collect::<Vec<_>>();
            let trace = DispatchTrace::StructuralMatch {
                candidates,
                result: DispatchResult::Ambiguous(names.clone()),
            };
            DispatchAttempt::failed(
                trace,
                BuildError::new(
                    ErrorKind::Layout,
                    line,
                    format!("slide matches multiple layouts: {}", names.join(", ")),
                    r#"pick one explicitly with <!-- {"layout":"…"} -->"#,
                ),
            )
        }
    }
}

fn unknown_layout_error(request: &crate::phase::LayoutRequest, layouts: &Layouts) -> BuildError {
    BuildError::new(
        ErrorKind::Layout,
        Some(request.line),
        format!("unknown layout '{}'", request.name),
        format!("use one of: {}", layouts.names().join(", ")),
    )
}

fn map_slide(slide: &ParsedSlide, layout: &Layout) -> Result<MappedSlide> {
    let title_line = shallowest_heading_line(&slide.fragments);
    let mut slots: BTreeMap<SlotName, MappedSlot> = BTreeMap::new();
    let mut unassigned = Vec::new();

    for fragment in slide.fragments.iter().cloned() {
        if let FragmentKind::SlotGroup { name, children } = fragment.kind() {
            let target = name.as_slot_name().clone();
            let Some(contract) = layout.slot_by_name(&target).cloned() else {
                return Err(unknown_explicit_slot_error(
                    &target,
                    fragment.line(),
                    layout,
                ));
            };
            for child in children.iter().cloned() {
                slots
                    .entry(target.clone())
                    .or_insert_with(|| MappedSlot::new(contract.clone()))
                    .push(child);
            }
            continue;
        }
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
            FragmentKind::Footnotes { .. } => {
                if layout.slot("footnotes").is_some() {
                    SlotName::new("footnotes").expect("conventional slot names are valid")
                } else {
                    SlotName::new("body").expect("conventional slot names are valid")
                }
            }
            FragmentKind::Heading { .. }
            | FragmentKind::Paragraph
            | FragmentKind::Math { .. }
            | FragmentKind::List
            | FragmentKind::Text => {
                SlotName::new("body").expect("conventional slot names are valid")
            }
            FragmentKind::SlotGroup { .. } => unreachable!("handled above"),
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
        source_index: slide.source_index,
        key: slide.key.clone(),
        layout: layout.clone(),
        slots,
        unassigned,
        skip: slide.skip,
        page_number_hidden: slide.page_number_hidden,
        notes: slide.notes.clone(),
    })
}

fn unknown_explicit_slot_error(target: &SlotName, line: usize, layout: &Layout) -> BuildError {
    let known = layout
        .slots()
        .keys()
        .map(SlotName::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    BuildError::new(
        ErrorKind::Layout,
        Some(line),
        format!(
            "unknown slot '{}' in explicit `::: {{slot=...}}` for layout '{}'",
            target.as_str(),
            layout.name(),
        ),
        format!("use one of: {known}"),
    )
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
            FragmentKind::Paragraph
            | FragmentKind::Text
            | FragmentKind::Code
            | FragmentKind::Math { .. }
            | FragmentKind::Footnotes { .. }
            | FragmentKind::Image { .. }
            | FragmentKind::List
            | FragmentKind::SlotGroup { .. } => None,
        })
        .min_by_key(|(level, line)| (*level, *line))
        .map(|(_level, line)| line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        check::check_deck,
        domain::{FootnoteEntry, SlideKey, SlotName, SourceFragment},
        layout::parse_layout,
        parser::{parse_frontmatter, parse_markdown as parse_markdown_impl},
        phase::{DeckSettings, KeySource, ParsedSlide},
    };

    fn parse_markdown(
        source: &str,
        highlighter: &crate::highlight::Highlighter,
    ) -> crate::error::Result<crate::phase::Deck<crate::phase::Parsed>> {
        let frontmatter = parse_frontmatter(source)?;
        parse_markdown_impl(source, frontmatter, highlighter)
    }

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
    fn mapping_carries_skip_flag_from_parsed_slide() {
        let layout = parse_layout(
            "cover",
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

        assert!(mapped.mapped_slides()[0].skip);
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
    fn explain_dispatch_records_explicit_layout_match() {
        let deck = parse_markdown(
            "<!-- {\"layout\":\"statement\"} -->\n# Title\n\nBody",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();
        let slide = &deck.parsed_slides()[0];

        let trace = explain_dispatch(slide, &cover_and_statement());

        assert_eq!(
            trace,
            DispatchTrace::Explicit {
                layout: "statement".to_owned(),
                line: 1,
                result: DispatchResult::Matched("statement".to_owned())
            }
        );
    }

    #[test]
    fn explain_dispatch_records_unknown_explicit_layout() {
        let deck = parse_markdown(
            "<!-- {\"layout\":\"missing\"} -->\n# Title",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();
        let slide = &deck.parsed_slides()[0];

        let trace = explain_dispatch(slide, &cover_and_statement());

        assert_eq!(
            trace,
            DispatchTrace::Explicit {
                layout: "missing".to_owned(),
                line: 1,
                result: DispatchResult::UnknownLayout("missing".to_owned())
            }
        );
    }

    #[test]
    fn explain_dispatch_records_explicit_map_failure_reason() {
        let deck = parse_markdown(
            "<!-- {\"layout\":\"cover\"} -->\n# Hello\n\n![alt](pic.png)",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();
        let slide = &deck.parsed_slides()[0];

        let trace = explain_dispatch(slide, &cover_and_statement());

        match trace {
            DispatchTrace::Explicit {
                layout,
                line,
                result:
                    DispatchResult::NoMatch {
                        reason: Some(reason),
                    },
            } => {
                assert_eq!(layout, "cover");
                assert_eq!(line, 1);
                assert!(reason.contains("no slot accepts image in layout 'cover'"));
            }
            other => panic!("expected explicit no-match trace, got {other:?}"),
        }
    }

    #[test]
    fn explain_dispatch_records_sole_layout_match() {
        let deck = parse_markdown(
            "# Title\n\nBody",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();
        let layout = parse_layout(
            "cover",
            r#"<section><slot name="title" accepts="inline" arity="1"></slot></section>"#,
        )
        .unwrap();
        let layouts = Layouts::single(layout);
        let slide = &deck.parsed_slides()[0];

        let trace = explain_dispatch(slide, &layouts);

        assert_eq!(
            trace,
            DispatchTrace::SoleLayout {
                layout: "cover".to_owned(),
                result: DispatchResult::Matched("cover".to_owned())
            }
        );
    }

    #[test]
    fn explain_dispatch_records_sole_layout_map_failure_reason() {
        let deck = parse_markdown(
            "# Hello\n\n![alt](pic.png)",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();
        let layout = parse_layout(
            "cover",
            r#"<section><slot name="title" accepts="inline" arity="1"></slot></section>"#,
        )
        .unwrap();
        let layouts = Layouts::single(layout);
        let slide = &deck.parsed_slides()[0];

        let trace = explain_dispatch(slide, &layouts);

        match trace {
            DispatchTrace::SoleLayout {
                layout,
                result:
                    DispatchResult::NoMatch {
                        reason: Some(reason),
                    },
            } => {
                assert_eq!(layout, "cover");
                assert!(reason.contains("no slot accepts image in layout 'cover'"));
            }
            other => panic!("expected sole-layout no-match trace, got {other:?}"),
        }
    }

    #[test]
    fn explain_dispatch_records_unique_structural_match() {
        let deck = parse_markdown(
            "# Statement\n\nBody paragraph",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();
        let slide = &deck.parsed_slides()[0];

        let trace = explain_dispatch(slide, &cover_and_statement());

        assert_eq!(
            trace,
            DispatchTrace::StructuralMatch {
                candidates: vec![
                    Candidate {
                        layout: "cover".to_owned(),
                        outcome: CandidateOutcome::Rejected {
                            reason: "unassigned content remains for missing 'body' slot".to_owned()
                        }
                    },
                    Candidate {
                        layout: "statement".to_owned(),
                        outcome: CandidateOutcome::Matched
                    }
                ],
                result: DispatchResult::Matched("statement".to_owned())
            }
        );
    }

    #[test]
    fn explain_dispatch_records_structural_no_match_with_candidate_reasons() {
        let deck = parse_markdown(
            "# Title\n\n```rust\nfn main() {}\n```",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();
        let slide = &deck.parsed_slides()[0];

        let trace = explain_dispatch(slide, &cover_and_statement());

        assert_eq!(
            trace,
            DispatchTrace::StructuralMatch {
                candidates: vec![
                    Candidate {
                        layout: "cover".to_owned(),
                        outcome: CandidateOutcome::Rejected {
                            reason: "unassigned content remains for missing 'code' slot".to_owned()
                        }
                    },
                    Candidate {
                        layout: "statement".to_owned(),
                        outcome: CandidateOutcome::Rejected {
                            reason: "slot 'body' got 0 item(s), but layout 'statement' allows 1..*"
                                .to_owned()
                        }
                    }
                ],
                result: DispatchResult::NoMatch { reason: None }
            }
        );
    }

    #[test]
    fn explain_dispatch_records_structural_ambiguity() {
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
        let deck =
            parse_markdown("# Title Only", &crate::highlight::Highlighter::defaults()).unwrap();
        let slide = &deck.parsed_slides()[0];

        let trace = explain_dispatch(slide, &layouts);

        assert_eq!(
            trace,
            DispatchTrace::StructuralMatch {
                candidates: vec![
                    Candidate {
                        layout: "cover".to_owned(),
                        outcome: CandidateOutcome::Matched
                    },
                    Candidate {
                        layout: "closing".to_owned(),
                        outcome: CandidateOutcome::Matched
                    }
                ],
                result: DispatchResult::Ambiguous(vec!["cover".to_owned(), "closing".to_owned()])
            }
        );
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
    fn maps_math_fragment_to_body_slot_by_convention() {
        let layout = parse_layout(
            "title-body",
            r#"<section>
               <slot name="title" accepts="inline" arity="1"></slot>
               <slot name="body" accepts="blocks" arity="0..*"></slot>
               </section>"#,
        )
        .unwrap();
        let parsed = Deck::parsed(
            DeckSettings::default(),
            vec![ParsedSlide {
                index: 0,
                source_index: 0,
                key: SlideKey::new("intro").unwrap(),
                key_source: KeySource::Derived { line: Some(1) },
                layout_request: None,
                fragments: vec![
                    SourceFragment::heading(1, 1, "# Intro", "Intro"),
                    SourceFragment::math(
                        7,
                        r#"<span class="katex-display">math html</span>"#,
                        r#"\frac{1}{2}"#,
                    ),
                ],
                skip: false,
                page_number_hidden: false,
                notes: None,
            }],
        );

        let mapped = map_by_convention(parsed, &layout).unwrap();
        let slide = &mapped.mapped_slides()[0];
        let body = SlotName::new("body").unwrap();

        assert_eq!(slide.slots[&body].fragments().len(), 1);
        match slide.slots[&body].fragments()[0].kind() {
            FragmentKind::Math { html } => {
                assert_eq!(html, r#"<span class="katex-display">math html</span>"#);
            }
            other => panic!("expected math fragment, got {other:?}"),
        }
        assert!(slide.unassigned.is_empty());
    }

    #[test]
    fn maps_footnotes_fragment_to_footnotes_slot_when_layout_declares_one() {
        let layout = parse_layout(
            "title-body-footnotes",
            r#"<section>
               <slot name="title" accepts="inline" arity="1"></slot>
               <slot name="body" accepts="blocks" arity="0..*"></slot>
               <slot name="footnotes" accepts="blocks" arity="0..1"></slot>
               </section>"#,
        )
        .unwrap();
        let parsed = Deck::parsed(
            DeckSettings::default(),
            vec![ParsedSlide {
                index: 0,
                source_index: 0,
                key: SlideKey::new("intro").unwrap(),
                key_source: KeySource::Derived { line: Some(1) },
                layout_request: None,
                fragments: vec![
                    SourceFragment::heading(1, 1, "# Intro", "Intro"),
                    SourceFragment::paragraph(3, "Body text."),
                    SourceFragment::footnotes(
                        7,
                        vec![FootnoteEntry::new(1, "note", "Footnote body.", 7)],
                    ),
                ],
                skip: false,
                page_number_hidden: false,
                notes: None,
            }],
        );

        let mapped = map_by_convention(parsed, &layout).unwrap();
        let slide = &mapped.mapped_slides()[0];
        let body = SlotName::new("body").unwrap();
        let footnotes = SlotName::new("footnotes").unwrap();

        assert_eq!(slide.slots[&body].fragments().len(), 1);
        assert!(matches!(
            slide.slots[&body].fragments()[0].kind(),
            FragmentKind::Paragraph
        ));
        assert_eq!(slide.slots[&footnotes].fragments().len(), 1);
        match slide.slots[&footnotes].fragments()[0].kind() {
            FragmentKind::Footnotes { entries } => {
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].label(), "note");
            }
            other => panic!("expected footnotes fragment, got {other:?}"),
        }
        assert!(slide.unassigned.is_empty());
    }

    #[test]
    fn maps_footnotes_fragment_to_body_slot_when_layout_has_no_footnotes_slot() {
        let layout = parse_layout(
            "title-body",
            r#"<section>
               <slot name="title" accepts="inline" arity="1"></slot>
               <slot name="body" accepts="blocks" arity="0..*"></slot>
               </section>"#,
        )
        .unwrap();
        let parsed = Deck::parsed(
            DeckSettings::default(),
            vec![ParsedSlide {
                index: 0,
                source_index: 0,
                key: SlideKey::new("intro").unwrap(),
                key_source: KeySource::Derived { line: Some(1) },
                layout_request: None,
                fragments: vec![
                    SourceFragment::heading(1, 1, "# Intro", "Intro"),
                    SourceFragment::paragraph(3, "Body text."),
                    SourceFragment::footnotes(
                        7,
                        vec![FootnoteEntry::new(1, "note", "Footnote body.", 7)],
                    ),
                ],
                skip: false,
                page_number_hidden: false,
                notes: None,
            }],
        );

        let mapped = map_by_convention(parsed, &layout).unwrap();
        let slide = &mapped.mapped_slides()[0];
        let body = SlotName::new("body").unwrap();

        assert_eq!(slide.slots[&body].fragments().len(), 2);
        assert!(matches!(
            slide.slots[&body].fragments()[0].kind(),
            FragmentKind::Paragraph
        ));
        match slide.slots[&body].fragments()[1].kind() {
            FragmentKind::Footnotes { entries } => {
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].label(), "note");
            }
            other => panic!("expected footnotes fragment, got {other:?}"),
        }
        assert!(slide.unassigned.is_empty());
    }

    #[test]
    fn layout_without_body_or_footnotes_slot_reports_footnotes_as_residual_content() {
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
    fn explicit_footnotes_slot_content_collides_with_collected_footnotes_by_arity() {
        let layout = parse_layout(
            "title-body-footnotes",
            r#"<section>
               <slot name="title" accepts="inline" arity="1"></slot>
               <slot name="body" accepts="blocks" arity="0..*"></slot>
               <slot name="footnotes" accepts="blocks" arity="0..1"></slot>
               </section>"#,
        )
        .unwrap();
        let mapped = map_by_convention(
            parse_markdown(
                "# Title\n\nClaim[^a].\n\n::: {slot=footnotes}\n\nManual footer paragraph.\n\n:::\n\n[^a]: Auto note.",
                &crate::highlight::Highlighter::defaults(),
            )
            .unwrap(),
            &layout,
        )
        .unwrap();

        let err = check_deck(mapped).unwrap_err();

        assert_eq!(err.kind, ErrorKind::Arity);
        assert_eq!(err.line, Some(7));
        assert!(err.to_string().contains(
            "slot 'footnotes' got 2 item(s), but layout 'title-body-footnotes' allows 0..1"
        ));
        assert_eq!(
            err.help,
            "use a layout with more footnotes capacity or remove one footnotes block"
        );
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

    // ----- Explicit slot syntax (::: {slot=name}) mapping tests -----

    fn two_column_layout() -> Layout {
        parse_layout(
            "two-column",
            r#"<section>
                <slot name="title" accepts="inline" arity="1"></slot>
                <slot name="left" accepts="blocks" arity="1..*"></slot>
                <slot name="right" accepts="blocks" arity="1..*"></slot>
            </section>"#,
        )
        .unwrap()
    }

    #[test]
    fn explicit_slot_routes_fragment_to_named_slot() {
        let markdown = "# Title\n\n::: {slot=left}\n\nleft body\n\n:::\n\n::: {slot=right}\n\nright body\n\n:::\n";
        let mapped = map_by_convention(
            parse_markdown(markdown, &crate::highlight::Highlighter::defaults()).unwrap(),
            &two_column_layout(),
        )
        .unwrap();
        let slots = &mapped.mapped_slides()[0].slots;
        assert_eq!(
            slots[&SlotName::new("left").unwrap()].fragments().len(),
            1,
            "left slot receives its explicit content"
        );
        assert_eq!(
            slots[&SlotName::new("right").unwrap()].fragments().len(),
            1,
            "right slot receives its explicit content"
        );
    }

    #[test]
    fn unknown_explicit_slot_is_error() {
        let markdown = "# Title\n\n::: {slot=middle}\n\nmiddle body\n\n:::\n";
        let err = map_by_convention(
            parse_markdown(markdown, &crate::highlight::Highlighter::defaults()).unwrap(),
            &two_column_layout(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("unknown slot 'middle'"));
        assert!(err.help.contains("left") && err.help.contains("right"));
    }

    #[test]
    fn explicit_slot_body_is_allowed() {
        // Author decision (2026-07-05): using a conventional slot name
        // explicitly is fine — intent-marking is not harmful.
        let layout = parse_layout(
            "statement",
            r#"<section>
                <slot name="title" accepts="inline" arity="1"></slot>
                <slot name="body" accepts="blocks" arity="1..*"></slot>
            </section>"#,
        )
        .unwrap();
        let markdown = "# Title\n\n::: {slot=body}\n\nexplicit body\n\n:::\n";
        let mapped = map_by_convention(
            parse_markdown(markdown, &crate::highlight::Highlighter::defaults()).unwrap(),
            &layout,
        )
        .unwrap();
        assert_eq!(
            mapped.mapped_slides()[0].slots[&SlotName::new("body").unwrap()]
                .fragments()
                .len(),
            1
        );
    }

    #[test]
    fn title_inferred_from_outside_slot_group() {
        // A heading inside an explicit slot group must not become the title:
        // the SlotGroup fragment is not a Heading, and its children are only
        // expanded during mapping into their target slot.
        let markdown = "::: {slot=left}\n\n# Not the title\n\n:::\n\n# Real title\n\nbody";
        let mapped = map_by_convention(
            parse_markdown(markdown, &crate::highlight::Highlighter::defaults()).unwrap(),
            &two_column_layout(),
        )
        .unwrap_or_else(|e| panic!("mapping failed: {}", e.message));
        let slots = &mapped.mapped_slides()[0].slots;
        // The outer '# Real title' lands in `title`; the inner heading lands in `left`.
        let title = &slots[&SlotName::new("title").unwrap()];
        assert_eq!(
            title.fragments()[0].plain_text(),
            "Real title",
            "outer heading remains the title candidate"
        );
    }

    #[test]
    fn explicit_and_conventional_share_slot_check_arity() {
        // Explicit slot=body plus a conventional body fragment: with an
        // ExactlyOne arity, the check phase must catch the overflow — no
        // silent drop.
        let layout = parse_layout(
            "one-body",
            r#"<section>
                <slot name="title" accepts="inline" arity="1"></slot>
                <slot name="body" accepts="blocks" arity="1"></slot>
            </section>"#,
        )
        .unwrap();
        let markdown = "# Title\n\n::: {slot=body}\n\nexplicit\n\n:::\n\nconventional\n";
        let mapped = map_by_convention(
            parse_markdown(markdown, &crate::highlight::Highlighter::defaults()).unwrap(),
            &layout,
        )
        .unwrap();
        let err = check_deck(mapped).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Arity);
    }

    #[test]
    fn accepts_violation_via_explicit_slot() {
        // A paragraph explicitly routed to a code slot must be caught by
        // check_accepts — validation rule #11.
        let layout = parse_layout(
            "code-only",
            r#"<section>
                <slot name="title" accepts="inline" arity="1"></slot>
                <slot name="snippet" accepts="code" arity="1"></slot>
            </section>"#,
        )
        .unwrap();
        let markdown = "# Title\n\n::: {slot=snippet}\n\nthis is a paragraph\n\n:::\n";
        let mapped = map_by_convention(
            parse_markdown(markdown, &crate::highlight::Highlighter::defaults()).unwrap(),
            &layout,
        )
        .unwrap();
        let err = check_deck(mapped).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Accepts);
    }

    #[test]
    fn dispatch_prefers_layout_with_explicit_slot_name() {
        // With a title-only cover layout and a two-column layout co-present,
        // a slide with ::: {slot=left} can only match two-column: the cover
        // layout has no 'left' slot so unknown-explicit-slot fails during
        // probing and the two-column layout wins uniquely.
        let cover = parse_layout(
            "cover",
            r#"<section><slot name="title" accepts="inline" arity="1"></slot></section>"#,
        )
        .unwrap();
        let layouts = Layouts::new(vec![cover, two_column_layout()]).unwrap();
        let markdown = "# Title\n\n::: {slot=left}\n\nL\n\n:::\n\n::: {slot=right}\n\nR\n\n:::\n";
        let mapped = dispatch_by_convention(
            parse_markdown(markdown, &crate::highlight::Highlighter::defaults()).unwrap(),
            &layouts,
        )
        .unwrap();
        assert_eq!(mapped.mapped_slides()[0].layout.name(), "two-column");
    }

    #[test]
    fn dispatch_rejects_when_no_layout_has_explicit_slot() {
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
        let layouts = Layouts::new(vec![cover, statement]).unwrap();
        let markdown = "# Title\n\n::: {slot=left}\n\nx\n\n:::\n";
        let err = dispatch_by_convention(
            parse_markdown(markdown, &crate::highlight::Highlighter::defaults()).unwrap(),
            &layouts,
        )
        .unwrap_err();
        assert!(err.to_string().contains("no layout matches"));
    }
}
