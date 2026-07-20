#![allow(clippy::result_large_err)]

use std::fs;

use peitho_core::{
    check_deck, code_images::SvgRunner, dispatch_by_convention, domain::CodeImageCommand,
    highlight::Highlighter, parse_deck_and_transform, parse_frontmatter, parse_layout, render_deck,
    resolve_image_paths, ResolvedImageAsset, ResolvedImagePath, Result, CODE_IMAGES_CACHE_DIR,
};

struct FakeRunner;

impl SvgRunner for FakeRunner {
    fn run(&self, _command: &CodeImageCommand, _stdin: &str) -> Result<Vec<u8>> {
        Ok(br#"<svg viewBox="0 0 10 10">diagram</svg>"#.to_vec())
    }
}

#[test]
fn renders_code_image_as_resolved_svg_img() {
    let markdown =
        "---\ncode_images:\n  mermaid: mmdc -i -\n---\n# Intro\n\n```mermaid\ngraph TD\n```";
    let frontmatter = parse_frontmatter(markdown).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let cache_dir = temp.path().join(CODE_IMAGES_CACHE_DIR);
    let transformed = parse_deck_and_transform(
        markdown,
        frontmatter,
        &Highlighter::defaults(),
        &FakeRunner,
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
