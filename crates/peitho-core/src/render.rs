use std::{collections::BTreeMap, error::Error};

use html_escape::encode_text;
use lol_html::{
    element, errors::RewritingError, html_content::ContentType, HtmlRewriter, Settings,
};
use pulldown_cmark::{html, Event, Options, Parser, Tag, TagEnd};

use crate::{
    domain::{FragmentKind, RenderedSlide, SlideKey, SlotName, SourceFragment},
    error::{BuildError, ErrorKind, Result},
    layout::Layout,
    phase::{Checked, Deck, Rendered},
};

pub fn render_deck(deck: Deck<Checked>) -> Result<Deck<Rendered>> {
    let (settings, checked_slides) = deck.into_checked_parts();
    let mut slides = Vec::new();
    for slide in checked_slides {
        let html = render_slide(slide.key(), slide.slots(), slide.layout())?;
        slides.push(RenderedSlide::new(slide.index(), slide.key().clone(), html));
    }
    Ok(Deck::rendered(settings, slides, String::new()))
}

fn render_slide(
    key: &SlideKey,
    slots: &BTreeMap<SlotName, Vec<SourceFragment>>,
    layout: &Layout,
) -> Result<String> {
    let mut output = Vec::new();
    let key_value = key.as_str().to_owned();
    let slot_values = slots.clone();
    let mut rewriter = HtmlRewriter::new(
        Settings {
            element_content_handlers: vec![
                element!("section", move |el| {
                    el.set_attribute("data-slide-key", &key_value)?;
                    let existing = el.get_attribute("class").unwrap_or_default();
                    let class = if existing
                        .split_whitespace()
                        .any(|part| part == "peitho-slide")
                    {
                        existing
                    } else if existing.is_empty() {
                        "peitho-slide".to_owned()
                    } else {
                        format!("{existing} peitho-slide")
                    };
                    el.set_attribute("class", &class)?;
                    Ok(())
                }),
                element!("slot", move |el| {
                    let raw_name = el.get_attribute("name").ok_or_else(|| {
                        box_build_error(BuildError::new(
                            ErrorKind::Layout,
                            None,
                            "slot is missing 'name'",
                            "add a name attribute to the slot",
                        ))
                    })?;
                    let slot = SlotName::new(raw_name).map_err(|message| {
                        box_build_error(BuildError::new(
                            ErrorKind::Layout,
                            None,
                            message,
                            "rename the slot",
                        ))
                    })?;
                    let fragments = slot_values.get(&slot).cloned().unwrap_or_default();
                    let html = render_slot(&slot, &fragments).map_err(box_build_error)?;
                    el.replace(&html, ContentType::Html);
                    Ok(())
                }),
            ],
            ..Settings::default()
        },
        |chunk: &[u8]| output.extend_from_slice(chunk),
    );
    rewriter
        .write(layout.html().as_bytes())
        .map_err(render_error)?;
    rewriter.end().map_err(render_error)?;
    String::from_utf8(output).map_err(|err| {
        BuildError::new(
            ErrorKind::Layout,
            None,
            format!("rendered HTML is not UTF-8: {err}"),
            "keep layouts and generated fragments as UTF-8",
        )
    })
}

fn render_slot(slot: &SlotName, fragments: &[SourceFragment]) -> Result<String> {
    if fragments.is_empty() {
        return Ok(String::new());
    }

    let class_name = slot.class_name();
    Ok(match fragments.first().map(SourceFragment::kind) {
        Some(FragmentKind::Heading { .. }) => {
            let body = fragments
                .iter()
                .map(|fragment| render_heading_inline(fragment.markdown()))
                .collect::<Vec<_>>()
                .join(" ");
            format!(r#"<span class="{class_name}">{body}</span>"#)
        }
        Some(FragmentKind::Code) => {
            let body = fragments
                .iter()
                .map(render_code_fragment)
                .collect::<Result<Vec<_>>>()?
                .join("\n");
            format!(r#"<pre class="{class_name}"><code>{body}</code></pre>"#)
        }
        _ => {
            let markdown = fragments
                .iter()
                .map(SourceFragment::markdown)
                .collect::<Vec<_>>()
                .join("\n\n");
            let mut body = String::new();
            html::push_html(&mut body, Parser::new_ext(&markdown, Options::empty()));
            format!(r#"<div class="{class_name}">{body}</div>"#)
        }
    })
}

/// A tagged code block is highlighted at build time into `hl-*` classed
/// spans (colors live in theme CSS); an untagged block stays escaped plain
/// text.
fn render_code_fragment(fragment: &SourceFragment) -> Result<String> {
    match fragment.language() {
        Some(language) => {
            crate::highlight::highlight_html(fragment.code_text(), language, fragment.line())
                .map(|html| html.trim_end().to_owned())
        }
        None => Ok(encode_text(fragment.code_text()).into_owned()),
    }
}

fn render_heading_inline(markdown: &str) -> String {
    let mut events = Vec::new();
    let mut in_heading = false;
    for event in Parser::new_ext(markdown, Options::empty()) {
        match event {
            Event::Start(Tag::Heading { .. }) => in_heading = true,
            Event::End(TagEnd::Heading(_)) => break,
            event if in_heading => events.push(event),
            _ => {}
        }
    }
    let mut rendered = String::new();
    html::push_html(&mut rendered, events.into_iter());
    rendered
}

fn box_build_error(err: BuildError) -> Box<dyn Error + Send + Sync> {
    Box::new(err)
}

fn render_error(err: RewritingError) -> BuildError {
    match err {
        RewritingError::ContentHandlerError(inner) => match inner.downcast::<BuildError>() {
            Ok(build_error) => *build_error,
            Err(inner) => BuildError::new(
                ErrorKind::Layout,
                None,
                format!("render content handler failed: {inner}"),
                "keep slot elements well-formed and avoid malformed HTML in the layout",
            ),
        },
        other => BuildError::new(
            ErrorKind::Layout,
            None,
            format!("failed to render layout: {other}"),
            "keep slot elements well-formed and avoid malformed HTML in the layout",
        ),
    }
}

pub fn render_distribution_index() -> String {
    r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <link rel="stylesheet" href="peitho.css">
  <title>Peitho Deck</title>
  <style>
    html, body { margin: 0; width: 100%; height: 100%; background: #000; overflow: hidden; }
    #peitho-slides { position: fixed; inset: 0; overflow: hidden; background: #000; }
    #peitho-canvas { position: absolute; left: 0; top: 0; width: 1280px; height: 720px; transform-origin: top left; }
  </style>
</head>
<body>
  <main id="peitho-slides">
    <div id="peitho-canvas"></div>
  </main>
  <script>
    const CANVAS_WIDTH = 1280;
    const CANVAS_HEIGHT = 720;
    let slides = [];
    let currentIndex = 0;

    function showError(message) {
      const root = document.getElementById('peitho-slides');
      root.textContent = message;
    }

    async function fetchOk(url) {
      const response = await fetch(url);
      if (!response.ok) {
        throw new Error(`Failed to load ${url}: ${response.status}`);
      }
      return response;
    }

    function resizeCanvas() {
      const canvas = document.getElementById('peitho-canvas');
      const scale = Math.min(window.innerWidth / CANVAS_WIDTH, window.innerHeight / CANVAS_HEIGHT);
      const width = CANVAS_WIDTH * scale;
      const height = CANVAS_HEIGHT * scale;
      const left = (window.innerWidth - width) / 2;
      const top = (window.innerHeight - height) / 2;
      canvas.style.transform = `translate(${left}px, ${top}px) scale(${scale})`;
    }

    function showSlide(index) {
      if (slides.length === 0) {
        document.getElementById('peitho-canvas').replaceChildren();
        return;
      }
      const next = Math.max(0, Math.min(index, slides.length - 1));
      currentIndex = next;
      const canvas = document.getElementById('peitho-canvas');
      canvas.innerHTML = slides[next].html;
    }

    function navigate(to) {
      if (to === 'next') showSlide(currentIndex + 1);
      if (to === 'prev') showSlide(currentIndex - 1);
      if (to === 'first') showSlide(0);
      if (to === 'last') showSlide(slides.length - 1);
    }

    async function loadDeck() {
      try {
        const manifest = await fetchOk('manifest.json').then((response) => response.json());
        document.title = manifest.title || 'Peitho Deck';
        slides = await Promise.all(
          manifest.slides.map(async (slide) => ({
            key: slide.key,
            html: await fetchOk(slide.src).then((response) => response.text())
          }))
        );
        showSlide(0);
        resizeCanvas();
      } catch (error) {
        showError(error.message);
      }
    }

    document.addEventListener('keydown', (event) => {
      if (event.key === 'ArrowRight' || event.key === 'PageDown' || event.key === ' ') {
        event.preventDefault();
        navigate('next');
      }
      if (event.key === 'ArrowLeft' || event.key === 'PageUp') {
        event.preventDefault();
        navigate('prev');
      }
      if (event.key === 'Home') navigate('first');
      if (event.key === 'End') navigate('last');
    });
    document.addEventListener('click', (event) => {
      navigate(event.clientX < window.innerWidth / 4 ? 'prev' : 'next');
    });
    window.addEventListener('resize', resizeCanvas);
    loadDeck();
  </script>
</body>
</html>"#
        .to_owned()
}

pub fn render_present_index() -> String {
    r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Peitho Present</title>
  <style>
    html, body { margin: 0; width: 100%; height: 100%; background: #000; overflow: hidden; }
    #peitho-present-root { position: fixed; inset: 0; overflow: hidden; background: #000; }
    .peitho-control-bar { position: fixed; left: 16px; bottom: 16px; z-index: 10; display: flex; gap: 8px; align-items: center; padding: 8px; background: rgba(0, 0, 0, 0.72); color: #fff; border-radius: 6px; }
    .peitho-control-bar[hidden] { display: none; }
  </style>
</head>
<body>
  <main id="peitho-present-root"></main>
  <!-- Runtime controls include data-peitho-action="close". -->
  <script type="module">
    import {
      installCanvasClickNavigation,
      installCloseOnEscape,
      installFullscreenShortcut,
      installKeyboardNavigation,
      installPresentationControls,
      installSyncBridge,
      mountPresentShell,
      serverSyncChannelFactory
    } from './shell.js';

    function showError(message) {
      const root = document.getElementById('peitho-present-root');
      root.textContent = message;
    }

    async function fetchOk(url) {
      const response = await fetch(url);
      if (!response.ok) throw new Error(`Failed to load ${url}: ${response.status}`);
      return response;
    }

    async function main() {
      const root = document.getElementById('peitho-present-root');
      try {
        window.peithoNotes = await fetchOk('notes.json').then((response) => response.json());
        installCloseOnEscape(window);
        installKeyboardNavigation(window);
        installSyncBridge(window, serverSyncChannelFactory());
        installPresentationControls({ root, window, document });
        installCanvasClickNavigation({ root, window });
        installFullscreenShortcut({ window, document });
        await mountPresentShell({ root });
      } catch (error) {
        showError(error.message);
      }
    }

    main();
  </script>
</body>
</html>"#
        .to_owned()
}

pub fn render_presenter_index() -> String {
    r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Peitho Presenter</title>
  <style>
    html, body { margin: 0; width: 100%; min-height: 100%; background: #111; color: #f5f5f5; }
    body { font: 14px system-ui, sans-serif; }
    #peitho-presenter-root { min-height: 100vh; }
    .peitho-presenter { display: grid; grid-layout-columns: minmax(0, 2fr) minmax(320px, 1fr); gap: 16px; padding: 16px; box-sizing: border-box; min-height: 100vh; }
    .peitho-presenter-pane { position: relative; overflow: hidden; background: #000; min-height: 180px; }
    [data-peitho-presenter="current"] { min-height: calc(100vh - 32px); }
    [data-peitho-presenter="preview"] { aspect-ratio: 16 / 9; }
    [data-peitho-presenter="notes"] { white-space: pre-wrap; line-height: 1.5; margin-top: 16px; }
    [data-peitho-presenter="timer"] { display: block; font-size: 40px; font-variant-numeric: tabular-nums; margin: 16px 0; }
    .peitho-presenter-controls { display: flex; flex-wrap: wrap; gap: 8px; }
  </style>
</head>
<body>
  <main id="peitho-presenter-root"></main>
  <!-- Runtime presenter controls include data-peitho-action="close". -->
  <script type="module">
    import { installCloseOnEscape, mountPresenterView, serverSyncChannelFactory } from './shell.js';

    function showError(message) {
      const root = document.getElementById('peitho-presenter-root');
      root.textContent = message;
    }

    async function fetchOk(url) {
      const response = await fetch(url);
      if (!response.ok) throw new Error(`Failed to load ${url}: ${response.status}`);
      return response;
    }

    async function main() {
      const root = document.getElementById('peitho-presenter-root');
      try {
        const notes = await fetchOk('notes.json').then((response) => response.json());
        installCloseOnEscape(window);
        await mountPresenterView({
          root,
          notes,
          syncChannelFactory: serverSyncChannelFactory()
        });
      } catch (error) {
        showError(error.message);
      }
    }

    main();
  </script>
</body>
</html>"#
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        check::check_deck, layout::parse_layout, mapping::map_by_convention,
        parser::parse_markdown, phase::PlannedTime,
    };

    #[test]
    fn renders_checked_slide_with_key_and_slot_classes() {
        let markdown = "<!-- {\"key\":\"arch-1\"} -->\n# **Architecture** `Phase`\n\nBody\n\n```rust\nfn main() {}\n```";
        let layout = parse_layout(
            "title-body-code",
            r#"<section class="slide">
  <h1><slot name="title" accepts="inline" arity="1"></slot></h1>
  <div class="body"><slot name="body" accepts="blocks" arity="0..*"></slot></div>
  <figure class="code"><slot name="code" accepts="code" arity="0..1"></slot></figure>
</section>"#,
        )
        .unwrap();
        let checked =
            check_deck(map_by_convention(parse_markdown(markdown).unwrap(), &layout).unwrap())
                .unwrap();

        let rendered = render_deck(checked).unwrap();
        let html = rendered.slides()[0].html();

        assert!(html.contains(r#"data-slide-key="arch-1""#));
        assert!(html.contains(r#"class="slot-title""#));
        assert!(html.contains(r#"class="slot-body""#));
        assert!(html.contains(r#"class="slot-code""#));
        assert!(html.contains("<strong>Architecture</strong>"));
        assert!(html.contains("<code>Phase</code>"));
        // The tagged rust block is highlighted into hl-* classed spans.
        assert!(html.contains("hl-"));
        assert!(html.contains("main"));
    }

    #[test]
    fn renders_empty_optional_slot_as_no_slot_markup() {
        let markdown = "# Architecture\n\nBody without code.";
        let layout = parse_layout(
            "title-body-code",
            r#"<section class="slide">
  <h1><slot name="title" accepts="inline" arity="1"></slot></h1>
  <div class="body"><slot name="body" accepts="blocks" arity="0..*"></slot></div>
  <figure class="code"><slot name="code" accepts="code" arity="0..1"></slot></figure>
</section>"#,
        )
        .unwrap();
        let checked =
            check_deck(map_by_convention(parse_markdown(markdown).unwrap(), &layout).unwrap())
                .unwrap();

        let rendered = render_deck(checked).unwrap();
        let html = rendered.slides()[0].html();

        assert!(html.contains(r#"<figure class="code"></figure>"#));
        assert!(!html.contains(r#"class="slot-code""#));
    }

    #[test]
    fn renders_inline_markup_in_heading_slot() {
        let markdown = "# **Architecture** `Phase` [docs](https://example.com)";
        let layout = parse_layout(
            "title-only",
            r#"<section><h1><slot name="title" accepts="inline" arity="1"></slot></h1></section>"#,
        )
        .unwrap();
        let checked =
            check_deck(map_by_convention(parse_markdown(markdown).unwrap(), &layout).unwrap())
                .unwrap();

        let rendered = render_deck(checked).unwrap();
        let html = rendered.slides()[0].html();

        assert!(html.contains("<strong>Architecture</strong>"));
        assert!(html.contains("<code>Phase</code>"));
        assert!(html.contains(r#"<a href="https://example.com">docs</a>"#));
        assert!(!html.contains("<p><strong>Architecture</strong>"));
    }

    #[test]
    fn renders_setext_and_atx_closing_hash_headings_as_inline_html() {
        let layout = parse_layout(
            "title-only",
            r#"<section><h1><slot name="title" accepts="inline" arity="1"></slot></h1></section>"#,
        )
        .unwrap();

        let setext = check_deck(
            map_by_convention(
                parse_markdown("**Architecture** `Phase`\n====").unwrap(),
                &layout,
            )
            .unwrap(),
        )
        .unwrap();
        let setext_html = render_deck(setext).unwrap().slides()[0].html().to_owned();
        assert!(setext_html.contains("<strong>Architecture</strong>"));
        assert!(setext_html.contains("<code>Phase</code>"));
        assert!(!setext_html.contains(r#"<span class="slot-title"><h1>"#));

        let atx = check_deck(
            map_by_convention(parse_markdown("# Architecture #").unwrap(), &layout).unwrap(),
        )
        .unwrap();
        let atx_html = render_deck(atx).unwrap().slides()[0].html().to_owned();
        assert!(atx_html.contains(r#"<span class="slot-title">Architecture</span>"#));
        assert!(!atx_html.contains("Architecture #"));
    }

    #[test]
    fn rendered_slides_have_manifest_fragment_sources() {
        let rendered = render_checked_deck("# Intro\n\n---\n# Details");

        assert_eq!(rendered.slides()[0].src(), "slides/000-intro.html");
        assert_eq!(rendered.slides()[1].src(), "slides/001-details.html");
    }

    #[test]
    fn deck_settings_survive_all_typestate_transitions() {
        let layout = parse_layout(
            "title-only",
            r#"<section><slot name="title" accepts="inline" arity="1"></slot></section>"#,
        )
        .unwrap();
        let parsed = parse_markdown("---\ntime: 15m\n---\n# Intro").unwrap();
        let mapped = map_by_convention(parsed, &layout).unwrap();
        let checked = check_deck(mapped).unwrap();
        let rendered = render_deck(checked).unwrap();

        assert_eq!(
            rendered
                .settings()
                .planned_time()
                .map(PlannedTime::as_millis),
            Some(900_000)
        );
    }

    #[test]
    fn distribution_index_uses_one_slide_canvas_without_shell_bundle() {
        let html = render_distribution_index();

        assert!(html.contains(r#"<link rel="stylesheet" href="peitho.css">"#));
        assert!(html.contains(r#"id="peitho-canvas""#));
        assert!(html.contains("const CANVAS_WIDTH = 1280"));
        assert!(html.contains("const CANVAS_HEIGHT = 720"));
        assert!(html.contains("function resizeCanvas()"));
        assert!(html.contains("function showSlide(index)"));
        assert!(html.contains("document.addEventListener('keydown'"));
        assert!(html.contains("document.addEventListener('click'"));
        assert!(html.contains("fetchOk('manifest.json')"));
        assert!(html.contains("fetchOk(slide.src)"));
        assert!(html.contains("response.ok"));
        assert!(!html.contains("shell.js"));
        assert!(!html.contains("installPresentationControls"));
        assert!(!html.contains("data-slide-key="));
    }

    #[test]
    fn present_index_mounts_shell_controls_keyboard_sync_and_notes() {
        let html = render_present_index();

        assert!(html.contains(r#"<main id="peitho-present-root"></main>"#));
        assert!(html.contains("installPresentationControls"));
        assert!(html.contains("installCanvasClickNavigation"));
        assert!(html.contains("installFullscreenShortcut"));
        assert!(html.contains("installCloseOnEscape(window)"));
        assert!(html.contains("fetchOk('notes.json')"));
        assert!(html.contains("await mountPresentShell({ root })"));
        assert!(html.contains("installKeyboardNavigation(window)"));
        assert!(html.contains("installSyncBridge(window, serverSyncChannelFactory())"));
        assert!(html.contains(r#"data-peitho-action="close""#));
        let controls_index = html
            .find("installPresentationControls({ root, window, document })")
            .unwrap();
        let mount_index = html.find("await mountPresentShell({ root })").unwrap();
        assert!(controls_index < mount_index);
        assert!(!html.contains("peitho-presenter-link"));
        assert!(!html.contains(">Presenter view</a>"));
        assert!(!html.contains("fetchOk(slide.src)"));
    }

    #[test]
    fn presenter_index_mounts_presenter_view_with_canvas_panes_and_notes() {
        let html = render_presenter_index();

        assert!(html.contains(r#"<main id="peitho-presenter-root"></main>"#));
        assert!(html.contains(
            r#"import { installCloseOnEscape, mountPresenterView, serverSyncChannelFactory } from './shell.js';"#
        ));
        assert!(html.contains("fetchOk('notes.json')"));
        assert!(html.contains("installCloseOnEscape(window)"));
        assert!(html.contains("await mountPresenterView({"));
        assert!(html.contains("syncChannelFactory: serverSyncChannelFactory()"));
        assert!(html.contains(r#"data-peitho-action="close""#));
        assert!(html.contains(".peitho-presenter-pane"));
        assert!(html.contains("overflow: hidden"));
        assert!(html.contains("Failed to load"));
        assert!(!html.contains("fetchOk(slide.src)"));
    }

    #[test]
    fn present_index_uses_server_sync_factory() {
        let html = render_present_index();

        assert!(html.contains("serverSyncChannelFactory"));
        assert!(html.contains("installSyncBridge(window, serverSyncChannelFactory())"));
        assert!(!html.contains("installSyncBridge(window);"));
    }

    #[test]
    fn presenter_index_passes_server_sync_factory_to_presenter_view() {
        let html = render_presenter_index();

        assert!(html.contains("serverSyncChannelFactory"));
        assert!(html.contains("syncChannelFactory: serverSyncChannelFactory()"));
    }

    #[test]
    fn present_index_has_no_static_presenter_link() {
        let html = render_present_index();

        assert!(html.contains(r#"<main id="peitho-present-root"></main>"#));
        assert!(!html.contains("peitho-presenter-link"));
        assert!(!html.contains(">Presenter view</a>"));
        assert!(!html.contains("mountPresenterView"));
    }

    #[test]
    fn present_index_keeps_controls_default_display_management_before_mount() {
        let html = render_present_index();

        let controls_index = html
            .find("installPresentationControls({ root, window, document })")
            .unwrap();
        let mount_index = html.find("await mountPresentShell({ root })").unwrap();
        assert!(controls_index < mount_index);
        assert!(html.contains("installPresentationControls({ root, window, document })"));
        assert!(!html.contains("openPresenter"));
    }

    fn render_checked_deck(markdown: &str) -> Deck<Rendered> {
        let layout = parse_layout(
            "title-only",
            r#"<section><h1><slot name="title" accepts="inline" arity="1"></slot></h1></section>"#,
        )
        .unwrap();
        let checked =
            check_deck(map_by_convention(parse_markdown(markdown).unwrap(), &layout).unwrap())
                .unwrap();
        render_deck(checked).unwrap()
    }
}
