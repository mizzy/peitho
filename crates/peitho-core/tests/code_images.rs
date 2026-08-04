#![allow(clippy::result_large_err)]

use std::fs;

use peitho_core::{
    check_deck,
    code_images::{EmbedRenderParams, EmbedRenderer, SvgRunner},
    dispatch_by_convention,
    domain::CodeImageCommand,
    highlight::Highlighter,
    parse_deck_and_transform, parse_frontmatter, parse_layout, render_deck, resolve_image_paths,
    ResolvedImageAsset, ResolvedImagePath, Result, CODE_IMAGES_CACHE_DIR, EMBEDS_CACHE_DIR,
};

struct FakeRunner;

impl SvgRunner for FakeRunner {
    fn run(&self, _command: &CodeImageCommand, _stdin: &str) -> Result<Vec<u8>> {
        Ok(br#"<svg viewBox="0 0 10 10">diagram</svg>"#.to_vec())
    }
}

struct PanicSvgRunner;

impl SvgRunner for PanicSvgRunner {
    fn run(&self, _command: &CodeImageCommand, _stdin: &str) -> Result<Vec<u8>> {
        panic!("built-in embed test must not invoke SVG runner");
    }
}

struct FixtureEmbedRenderer;

impl EmbedRenderer for FixtureEmbedRenderer {
    fn render(&self, _url: &str, _params: EmbedRenderParams) -> Result<Vec<u8>> {
        Ok(b"\x89PNG\r\n\x1a\nfixture tweet".to_vec())
    }
}

struct PanicEmbedRenderer;

impl EmbedRenderer for PanicEmbedRenderer {
    fn render(&self, _url: &str, _params: EmbedRenderParams) -> Result<Vec<u8>> {
        panic!("SVG integration test must not invoke embed renderer");
    }
}

#[test]
fn renders_code_image_as_resolved_svg_img() {
    let markdown =
        "---\ncode_images:\n  mermaid: mmdc -i -\n---\n# Intro\n\n```mermaid\ngraph TD\n```";
    let frontmatter = parse_frontmatter(markdown).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let cache_dir = temp.path().join(CODE_IMAGES_CACHE_DIR);
    let embeds_cache_dir = temp.path().join(EMBEDS_CACHE_DIR);
    let transformed = parse_deck_and_transform(
        markdown,
        frontmatter,
        &Highlighter::defaults(),
        &FakeRunner,
        &PanicEmbedRenderer,
        &cache_dir,
        &embeds_cache_dir,
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
    let layouts = peitho_core::Layouts::new(vec![layout]).unwrap();
    let checked = check_deck(dispatch_by_convention(transformed, &layouts).unwrap()).unwrap();
    let dist_rel = ResolvedImagePath::from_hashed_asset("0123456789abcdef", "diagram.svg").unwrap();

    let (resolved, _assets) = resolve_image_paths(checked, |request| {
        let source_abs = temp.path().join(request.raw.as_str());
        assert!(fs::metadata(&source_abs).unwrap().is_file());
        Ok(ResolvedImageAsset {
            source_abs,
            dist_rel: dist_rel.clone(),
        })
    })
    .unwrap();
    let rendered = render_deck(resolved, &Highlighter::defaults(), String::new()).unwrap();
    let html = rendered.slides()[0].html();

    assert!(html.contains("<img"));
    assert!(html.contains(r#"src="assets/0123456789abcdef-diagram.svg""#));
    assert!(html.contains(r#"alt="diagram (mermaid)""#));
}

#[test]
fn renders_builtin_embed_through_existing_png_image_pipeline() {
    let markdown = "# Intro\n\n```embed\nhttps://twitter.com/A/status/1\n```";
    let frontmatter = parse_frontmatter(markdown).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let code_images_cache_dir = temp.path().join(CODE_IMAGES_CACHE_DIR);
    let embeds_cache_dir = temp.path().join(EMBEDS_CACHE_DIR);
    let transformed = parse_deck_and_transform(
        markdown,
        frontmatter,
        &Highlighter::defaults(),
        &PanicSvgRunner,
        &FixtureEmbedRenderer,
        &code_images_cache_dir,
        &embeds_cache_dir,
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
    let layouts = peitho_core::Layouts::new(vec![layout]).unwrap();
    let checked = check_deck(dispatch_by_convention(transformed, &layouts).unwrap()).unwrap();
    let dist_rel = ResolvedImagePath::from_hashed_asset("0123456789abcdef", "tweet.png").unwrap();

    let (resolved, assets) = resolve_image_paths(checked, |request| {
        let source_abs = temp.path().join(request.raw.as_str());
        assert_eq!(&fs::read(&source_abs).unwrap()[..8], b"\x89PNG\r\n\x1a\n");
        Ok(ResolvedImageAsset {
            source_abs,
            dist_rel: dist_rel.clone(),
        })
    })
    .unwrap();
    assert_eq!(assets.len(), 1);
    let asset_path = assets[0].dist_rel.as_str().to_owned();
    let rendered = render_deck(resolved, &Highlighter::defaults(), String::new()).unwrap();
    let html = rendered.slides()[0].html();

    assert!(html.contains(&format!(r#"src="{asset_path}""#)));
    assert!(html.contains(r#"alt="X post by @a""#));
}
