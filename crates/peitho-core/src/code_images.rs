use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

use sha2::{Digest, Sha256};

use crate::{
    domain::{CodeImageCommand, CodeImagesConfig, FragmentKind, RawImagePath, SourceFragment},
    error::{BuildError, ErrorKind, Result},
    highlight::Highlighter,
    parser::{parse_markdown, ParsedFrontmatter},
    phase::{Deck, Parsed},
};

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

pub trait SvgRunner {
    fn run(&self, command: &CodeImageCommand, stdin: &str) -> Result<Vec<u8>>;
}

pub fn parse_deck_and_transform<R: SvgRunner>(
    source: &str,
    frontmatter: ParsedFrontmatter,
    highlighter: &Highlighter,
    runner: &R,
    cache_dir: &Path,
) -> Result<Deck<Parsed>> {
    let config = frontmatter.settings().code_images().clone();
    let parsed = parse_markdown(source, frontmatter, highlighter)?;
    transform_code_images(parsed, &config, runner, cache_dir)
}

pub fn transform_code_images<R: SvgRunner>(
    deck: Deck<Parsed>,
    config: &CodeImagesConfig,
    runner: &R,
    cache_dir: &Path,
) -> Result<Deck<Parsed>> {
    let (settings, slides) = deck.into_parsed_parts();
    let mut transformed_slides = Vec::with_capacity(slides.len());

    for mut slide in slides {
        slide.fragments = slide
            .fragments
            .into_iter()
            .map(|fragment| transform_fragment(fragment, config, runner, cache_dir))
            .collect::<Result<Vec<_>>>()?;
        transformed_slides.push(slide);
    }

    Ok(Deck::parsed(settings, transformed_slides))
}

fn transform_fragment<R: SvgRunner>(
    fragment: SourceFragment,
    config: &CodeImagesConfig,
    runner: &R,
    cache_dir: &Path,
) -> Result<SourceFragment> {
    if let Some(tag) = fragment.language() {
        if matches!(fragment.kind(), FragmentKind::Code) {
            if let Some(command) = config.entries.get(tag) {
                let key = code_image_cache_key(command, fragment.code_text());
                let cache_path = cache_dir.join(format!("{key}.svg"));
                fs::create_dir_all(cache_dir).map_err(|err| {
                    code_image_error(
                        fragment.line(),
                        tag,
                        format!("failed to create code image cache directory: {err}"),
                        "make the .peitho directory writable and rebuild",
                    )
                })?;
                let cache_hit = valid_cached_svg(fragment.line(), tag, &cache_path);
                if !cache_hit {
                    let bytes = runner.run(command, fragment.code_text()).map_err(|err| {
                        code_image_error(fragment.line(), tag, err.message, err.help)
                    })?;
                    validate_svg_output(fragment.line(), tag, &bytes)?;
                    write_cache_file_atomic(&cache_path, &bytes).map_err(|err| {
                        code_image_error(
                            fragment.line(),
                            tag,
                            format!("failed to write code image cache file: {err}"),
                            "make the .peitho directory writable and rebuild",
                        )
                    })?;
                }
                let raw = RawImagePath::from_code_images_cache(&key);
                return Ok(SourceFragment::image(
                    fragment.line(),
                    format!("diagram ({tag})"),
                    raw,
                ));
            }
        }
    }

    match fragment.kind() {
        FragmentKind::SlotGroup { name, children } => {
            let children = children
                .clone()
                .into_iter()
                .map(|child| transform_fragment(child, config, runner, cache_dir))
                .collect::<Result<Vec<_>>>()?;
            Ok(SourceFragment::slot_group(
                fragment.line(),
                name.clone(),
                children,
            ))
        }
        FragmentKind::Heading { .. }
        | FragmentKind::Paragraph
        | FragmentKind::Text
        | FragmentKind::Code
        | FragmentKind::Image { .. }
        | FragmentKind::List => Ok(fragment),
    }
}

fn valid_cached_svg(line: usize, tag: &str, path: &Path) -> bool {
    let cache_hit = fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.len() > 0)
        .unwrap_or(false);
    if !cache_hit {
        return false;
    }
    fs::read(path)
        .map(|bytes| validate_svg_output(line, tag, &bytes).is_ok())
        .unwrap_or(false)
}

fn code_image_cache_key(command: &CodeImageCommand, code_text: &str) -> String {
    let mut hasher = Sha256::new();
    for arg in &command.argv {
        hasher.update(arg.as_bytes());
        hasher.update([0]);
    }
    hasher.update(code_text.as_bytes());
    hex_encode(&hasher.finalize())
}

fn write_cache_file_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("code-image.svg");
    let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_path = dir.join(format!(
        ".{file_name}.{}.{}.tmp",
        std::process::id(),
        counter
    ));

    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp_path)?;
        file.write_all(bytes)?;
        file.flush()?;
        fs::rename(&tmp_path, path)?;
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&tmp_path);
    }
    result
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn validate_svg_output(line: usize, tag: &str, bytes: &[u8]) -> Result<()> {
    if bytes.is_empty() {
        return Err(code_image_error(
            line,
            tag,
            "command wrote empty stdout",
            format!("make code_images.{tag} write an SVG document to stdout"),
        ));
    }
    if !is_svg_output(bytes) {
        return Err(code_image_error(
            line,
            tag,
            "command stdout is not an SVG document",
            format!("make code_images.{tag} write an SVG document to stdout"),
        ));
    }
    Ok(())
}

fn is_svg_output(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let trimmed = text.trim_start();
    if trimmed.starts_with("<svg") {
        return true;
    }
    let Some(after_xml_open) = trimmed.strip_prefix("<?xml") else {
        return false;
    };
    let Some(end) = after_xml_open.find("?>") else {
        return false;
    };
    after_xml_open[end + 2..].trim_start().starts_with("<svg")
}

fn code_image_error(
    line: usize,
    tag: &str,
    message: impl Into<String>,
    help: impl Into<String>,
) -> BuildError {
    BuildError::new(
        ErrorKind::Asset,
        Some(line),
        format!("code_images '{tag}' failed: {}", message.into()),
        help,
    )
}

#[cfg(test)]
mod tests {
    use super::{transform_code_images, SvgRunner};
    use crate::error::ErrorKind;
    use crate::{
        check::check_deck,
        domain::{
            CodeImageCommand, CodeImagesConfig, FragmentKind, ResolvedImageAsset,
            ResolvedImagePath, SourceFragment,
        },
        layout::{parse_layout, Layouts},
        mapping::dispatch_by_convention,
        parser::{parse_frontmatter, parse_markdown},
        phase::{resolve_image_paths, Deck, DeckSettings, KeySource, Parsed, ParsedSlide},
        BuildError, Result,
    };
    use std::{cell::Cell, collections::BTreeMap, fs, path::PathBuf};

    const MERMAID_KEY: &str = "4dba32c8d19de69fc2671719f51c327b802adf382763f36d20c1bffd972745f1";

    struct FakeRunner {
        calls: Cell<usize>,
        result: Result<Vec<u8>>,
    }

    impl FakeRunner {
        fn svg(output: impl Into<Vec<u8>>) -> Self {
            Self {
                calls: Cell::new(0),
                result: Ok(output.into()),
            }
        }

        fn err(message: &str) -> Self {
            Self {
                calls: Cell::new(0),
                result: Err(BuildError::new(
                    ErrorKind::Asset,
                    None,
                    message,
                    "check the code_images command",
                )),
            }
        }
    }

    impl SvgRunner for FakeRunner {
        fn run(&self, _command: &CodeImageCommand, _stdin: &str) -> Result<Vec<u8>> {
            self.calls.set(self.calls.get() + 1);
            self.result.clone()
        }
    }

    fn config() -> CodeImagesConfig {
        CodeImagesConfig {
            entries: BTreeMap::from([(
                "mermaid".to_owned(),
                CodeImageCommand {
                    argv: vec!["mmdc".to_owned(), "-i".to_owned(), "-".to_owned()],
                },
            )]),
            key_line: Some(2),
        }
    }

    fn deck_with_mermaid(code: &str) -> Deck<Parsed> {
        Deck::parsed(
            DeckSettings::default(),
            vec![ParsedSlide {
                index: 0,
                key: crate::domain::SlideKey::new("intro").unwrap(),
                key_source: KeySource::Derived { line: Some(1) },
                layout_request: None,
                fragments: vec![SourceFragment::code(
                    7,
                    Some("mermaid".to_owned()),
                    code.to_owned(),
                )],
                notes: None,
            }],
        )
    }

    #[test]
    fn transforms_matching_code_block_to_cached_image() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        let runner = FakeRunner::svg("<svg>diagram</svg>");

        let deck = transform_code_images(
            deck_with_mermaid("graph TD"),
            &config(),
            &runner,
            &cache_dir,
        )
        .unwrap();
        let fragment = &deck.parsed_slides()[0].fragments[0];

        assert_eq!(runner.calls.get(), 1);
        assert_eq!(fragment.line(), 7);
        match fragment.kind() {
            FragmentKind::Image { alt, src } => {
                assert_eq!(alt, "diagram (mermaid)");
                assert_eq!(
                    src.as_str(),
                    format!("{}/{MERMAID_KEY}.svg", crate::CODE_IMAGES_CACHE_DIR)
                );
            }
            other => panic!("expected image fragment, got {other:?}"),
        }
        assert_eq!(
            fs::read(cache_dir.join(format!("{MERMAID_KEY}.svg"))).unwrap(),
            b"<svg>diagram</svg>"
        );
    }

    #[test]
    fn uses_non_empty_cache_hit_without_running_command() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        fs::create_dir_all(&cache_dir).unwrap();
        fs::write(
            cache_dir.join(format!("{MERMAID_KEY}.svg")),
            b"<svg>cached</svg>",
        )
        .unwrap();
        let runner = FakeRunner::svg("<svg>new</svg>");

        let deck = transform_code_images(
            deck_with_mermaid("graph TD"),
            &config(),
            &runner,
            &cache_dir,
        )
        .unwrap();
        let fragment = &deck.parsed_slides()[0].fragments[0];

        assert_eq!(runner.calls.get(), 0);
        match fragment.kind() {
            FragmentKind::Image { src, .. } => {
                assert_eq!(
                    src.as_str(),
                    format!("{}/{MERMAID_KEY}.svg", crate::CODE_IMAGES_CACHE_DIR)
                );
            }
            other => panic!("expected image fragment, got {other:?}"),
        }
        assert_eq!(
            fs::read(cache_dir.join(format!("{MERMAID_KEY}.svg"))).unwrap(),
            b"<svg>cached</svg>"
        );
    }

    #[test]
    fn corrupt_cache_hit_is_replaced_by_runner_output() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        fs::create_dir_all(&cache_dir).unwrap();
        fs::write(cache_dir.join(format!("{MERMAID_KEY}.svg")), b"not svg").unwrap();
        let runner = FakeRunner::svg("<svg>new</svg>");

        transform_code_images(
            deck_with_mermaid("graph TD"),
            &config(),
            &runner,
            &cache_dir,
        )
        .unwrap();

        assert_eq!(runner.calls.get(), 1);
        assert_eq!(
            fs::read(cache_dir.join(format!("{MERMAID_KEY}.svg"))).unwrap(),
            b"<svg>new</svg>"
        );
    }

    #[test]
    fn runner_failure_reports_code_block_line_and_stderr_excerpt() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        let runner = FakeRunner::err("command exited with status 1; stderr: boom");

        let err = match transform_code_images(
            deck_with_mermaid("graph TD"),
            &config(),
            &runner,
            &cache_dir,
        ) {
            Ok(_) => panic!("expected runner failure"),
            Err(err) => err,
        };

        assert_eq!(runner.calls.get(), 1);
        assert_eq!(err.kind, ErrorKind::Asset);
        assert_eq!(err.line, Some(7));
        assert!(err.message.contains("code_images 'mermaid' failed"));
        assert!(err.message.contains("boom"));
        assert_eq!(err.help, "check the code_images command");
    }

    #[test]
    fn empty_stdout_reports_code_block_line() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        let runner = FakeRunner::svg(Vec::new());

        let err = match transform_code_images(
            deck_with_mermaid("graph TD"),
            &config(),
            &runner,
            &cache_dir,
        ) {
            Ok(_) => panic!("expected empty stdout failure"),
            Err(err) => err,
        };

        assert_eq!(runner.calls.get(), 1);
        assert_eq!(err.kind, ErrorKind::Asset);
        assert_eq!(err.line, Some(7));
        assert_eq!(
            err.message,
            "code_images 'mermaid' failed: command wrote empty stdout"
        );
        assert_eq!(
            err.help,
            "make code_images.mermaid write an SVG document to stdout"
        );
        assert!(!cache_dir.join(format!("{MERMAID_KEY}.svg")).exists());
    }

    #[test]
    fn non_svg_stdout_reports_code_block_line() {
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        let runner = FakeRunner::svg("<html>not svg</html>");

        let err = match transform_code_images(
            deck_with_mermaid("graph TD"),
            &config(),
            &runner,
            &cache_dir,
        ) {
            Ok(_) => panic!("expected non-SVG stdout failure"),
            Err(err) => err,
        };

        assert_eq!(runner.calls.get(), 1);
        assert_eq!(err.kind, ErrorKind::Asset);
        assert_eq!(err.line, Some(7));
        assert_eq!(
            err.message,
            "code_images 'mermaid' failed: command stdout is not an SVG document"
        );
        assert_eq!(
            err.help,
            "make code_images.mermaid write an SVG document to stdout"
        );
        assert!(!cache_dir.join(format!("{MERMAID_KEY}.svg")).exists());
    }

    #[test]
    fn code_images_entry_wins_over_known_syntect_language() {
        let markdown =
            "---\ncode_images:\n  json: json-to-svg\n---\n# Intro\n\n```json\n{\"ok\": true}\n```";
        let frontmatter = parse_frontmatter(markdown).unwrap();
        let config = frontmatter.settings().code_images().clone();
        let parsed = parse_markdown(
            markdown,
            frontmatter,
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();
        let temp = tempfile::tempdir().unwrap();
        let runner = FakeRunner::svg("<svg>json</svg>");

        let deck = transform_code_images(
            parsed,
            &config,
            &runner,
            &temp.path().join(crate::CODE_IMAGES_CACHE_DIR),
        )
        .unwrap();
        let fragment = &deck.parsed_slides()[0].fragments[1];

        assert_eq!(runner.calls.get(), 1);
        match fragment.kind() {
            FragmentKind::Image { alt, .. } => {
                assert_eq!(alt, "diagram (json)");
            }
            other => panic!("expected image fragment, got {other:?}"),
        }
    }

    #[test]
    fn transforms_code_images_inside_slot_group() {
        let markdown = "---\ncode_images:\n  mermaid: mmdc -i -\n---\n# Intro\n\n::: {slot=main}\n\n```mermaid\ngraph TD\n```\n:::\n";
        let frontmatter = parse_frontmatter(markdown).unwrap();
        let config = frontmatter.settings().code_images().clone();
        let parsed = parse_markdown(
            markdown,
            frontmatter,
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();
        let temp = tempfile::tempdir().unwrap();
        let transformed = transform_code_images(
            parsed,
            &config,
            &FakeRunner::svg("<svg>slot</svg>"),
            &temp.path().join(crate::CODE_IMAGES_CACHE_DIR),
        )
        .unwrap();

        let slot_group = transformed.parsed_slides()[0]
            .fragments
            .iter()
            .find_map(|fragment| match fragment.kind() {
                FragmentKind::SlotGroup { name, children } => Some((name, children)),
                _ => None,
            })
            .expect("expected transformed slide to contain a slot group");

        assert_eq!(slot_group.0.as_slot_name().as_str(), "main");
        assert_eq!(slot_group.1.len(), 1);
        match slot_group.1[0].kind() {
            FragmentKind::Image { alt, .. } => assert_eq!(alt, "diagram (mermaid)"),
            other => panic!("expected slot group child image, got {other:?}"),
        }
    }

    #[test]
    fn duplicate_diagrams_share_one_cache_file_and_one_dist_asset() {
        let markdown = "---\ncode_images:\n  mermaid: mmdc -i -\n---\n# One\n\n```mermaid\ngraph TD\n```\n\n---\n# Two\n\n```mermaid\ngraph TD\n```";
        let frontmatter = parse_frontmatter(markdown).unwrap();
        let config = frontmatter.settings().code_images().clone();
        let parsed = parse_markdown(
            markdown,
            frontmatter,
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();
        let temp = tempfile::tempdir().unwrap();
        let cache_dir = temp.path().join(crate::CODE_IMAGES_CACHE_DIR);
        let transformed = transform_code_images(
            parsed,
            &config,
            &FakeRunner::svg("<svg>same</svg>"),
            &cache_dir,
        )
        .unwrap();
        let layout = parse_layout(
            "title-image",
            r#"<section>
               <slot name="title" accepts="inline" arity="1"></slot>
               <slot name="image" accepts="image" arity="1"></slot>
               </section>"#,
        )
        .unwrap();
        let layouts = Layouts::new(vec![layout]).unwrap();
        let checked = check_deck(dispatch_by_convention(transformed, &layouts).unwrap()).unwrap();
        let dist_rel = ResolvedImagePath::from_string("assets/same.svg".to_owned());
        let mut resolve_calls = 0;

        let (_resolved, assets) = resolve_image_paths(checked, |_request| {
            resolve_calls += 1;
            Ok(ResolvedImageAsset {
                source_abs: PathBuf::from("/tmp/code-image.svg"),
                dist_rel: dist_rel.clone(),
            })
        })
        .unwrap();

        assert_eq!(fs::read_dir(&cache_dir).unwrap().count(), 1);
        assert_eq!(resolve_calls, 2);
        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].dist_rel.as_str(), "assets/same.svg");
    }
}
