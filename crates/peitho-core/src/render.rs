use std::{collections::BTreeMap, error::Error};

use html_escape::{encode_double_quoted_attribute, encode_text};
use lol_html::{
    element, errors::RewritingError, html_content::ContentType, HtmlRewriter, Settings,
};
use pulldown_cmark::{html, Event, Options, Parser, Tag, TagEnd};

use crate::{
    domain::{
        Accepts, AspectRatio, FragmentKind, RenderedSlide, ResolvedImagePath, SlideKey, SlotName,
        SourceFragment,
    },
    error::{BuildError, ErrorKind, Result},
    highlight::Highlighter,
    layout::Layout,
    phase::{Checked, CheckedSlot, Deck, Rendered},
};

const PDF_FLATTEN_JS: &str = include_str!("pdf_flatten.js");

/// Render a checked deck whose image paths have already been resolved.
///
/// The `ResolvedImagePath` type parameter is part of the safety boundary:
/// callers cannot pass `Deck<Checked<RawImagePath>>`, so raw Markdown paths
/// cannot reach `<img src>` generation by convention or accident.
pub fn render_deck(
    deck: Deck<Checked<ResolvedImagePath>>,
    highlighter: &Highlighter,
) -> Result<Deck<Rendered>> {
    let (settings, checked_slides) = deck.into_checked_parts();
    let mut slides = Vec::new();
    for slide in checked_slides {
        let html = render_slide(slide.key(), slide.slots(), slide.layout(), highlighter)?;
        let notes = slide.notes().map(|s| s.to_owned());
        slides.push(RenderedSlide::new(
            slide.index(),
            slide.key().clone(),
            html,
            notes,
        ));
    }
    Ok(Deck::rendered(settings, slides, String::new()))
}

fn render_slide(
    key: &SlideKey,
    slots: &BTreeMap<SlotName, CheckedSlot<ResolvedImagePath>>,
    layout: &Layout,
    highlighter: &Highlighter,
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
                    let checked_slot = slot_values.get(&slot).ok_or_else(|| {
                        box_build_error(BuildError::new(
                            ErrorKind::Layout,
                            None,
                            format!("checked slot '{}' is missing its contract", slot.as_str()),
                            "keep checked slides synchronized with layout slots",
                        ))
                    })?;
                    let html = render_slot(
                        &slot,
                        checked_slot.contract().accepts,
                        checked_slot.fragments(),
                        highlighter,
                    )
                    .map_err(box_build_error)?;
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

fn render_slot(
    slot: &SlotName,
    accepts: Accepts,
    fragments: &[SourceFragment<ResolvedImagePath>],
    highlighter: &Highlighter,
) -> Result<String> {
    if fragments.is_empty() {
        return Ok(String::new());
    }

    let class_name = slot.class_name();
    Ok(match accepts {
        Accepts::Inline => {
            let body = fragments
                .iter()
                .map(render_heading_inline_fragment)
                .collect::<Result<Vec<_>>>()?
                .join(" ");
            format!(r#"<span class="{class_name}">{body}</span>"#)
        }
        Accepts::Code => {
            let body = fragments
                .iter()
                .map(|fragment| render_code_fragment(fragment, highlighter))
                .collect::<Result<Vec<_>>>()?
                .join("\n");
            format!(r#"<pre class="{class_name}"><code>{body}</code></pre>"#)
        }
        Accepts::Image => fragments
            .iter()
            .map(render_image_fragment)
            .collect::<Result<Vec<_>>>()?
            .join("\n"),
        Accepts::Blocks | Accepts::Text | Accepts::List => {
            render_block_slot(&class_name, accepts, fragments)?
        }
    })
}

fn render_block_slot(
    class_name: &str,
    accepts: Accepts,
    fragments: &[SourceFragment<ResolvedImagePath>],
) -> Result<String> {
    for fragment in fragments {
        ensure_fragment_matches_contract(accepts, fragment)?;
    }
    let markdown = fragments
        .iter()
        .map(SourceFragment::markdown)
        .collect::<Vec<_>>()
        .join("\n\n");
    let mut body = String::new();
    html::push_html(&mut body, Parser::new_ext(&markdown, Options::empty()));
    Ok(format!(r#"<div class="{class_name}">{body}</div>"#))
}

fn render_heading_inline_fragment(fragment: &SourceFragment<ResolvedImagePath>) -> Result<String> {
    ensure_fragment_matches_contract(Accepts::Inline, fragment)?;
    Ok(render_heading_inline(fragment.markdown()))
}

/// A tagged code block is highlighted at build time into `hl-*` classed
/// spans (colors live in theme CSS); an untagged block stays escaped plain
/// text.
fn render_code_fragment(
    fragment: &SourceFragment<ResolvedImagePath>,
    highlighter: &Highlighter,
) -> Result<String> {
    ensure_fragment_matches_contract(Accepts::Code, fragment)?;
    match fragment.language() {
        Some(language) => highlighter
            .highlight_html(fragment.code_text(), language, fragment.line())
            .map(|html| html.trim_end().to_owned()),
        None => Ok(encode_text(fragment.code_text().trim_end()).into_owned()),
    }
}

fn render_image_fragment(fragment: &SourceFragment<ResolvedImagePath>) -> Result<String> {
    ensure_fragment_matches_contract(Accepts::Image, fragment)?;
    match fragment.kind() {
        FragmentKind::Image { alt, src } => Ok(format!(
            r#"<img src="{}" alt="{}">"#,
            encode_double_quoted_attribute(src.as_str()),
            encode_double_quoted_attribute(alt),
        )),
        FragmentKind::Heading { .. }
        | FragmentKind::Paragraph
        | FragmentKind::Text
        | FragmentKind::Code
        | FragmentKind::List
        | FragmentKind::SlotGroup { .. } => unreachable!("validated by contract guard"),
    }
}

fn ensure_fragment_matches_contract(
    accepts: Accepts,
    fragment: &SourceFragment<ResolvedImagePath>,
) -> Result<()> {
    if accepts_fragment(accepts, fragment) {
        return Ok(());
    }
    let expected = expected_fragment_label(accepts);
    Err(BuildError::new(
        ErrorKind::Layout,
        Some(fragment.line()),
        format!("expected {expected} fragment, got {}", fragment.kind()),
        format!("keep {accepts} slots mapped only to {expected} fragments"),
    ))
}

fn expected_fragment_label(accepts: Accepts) -> &'static str {
    match accepts {
        Accepts::Inline => "heading",
        Accepts::Blocks => "block",
        Accepts::Text => "text",
        Accepts::Code => "code",
        Accepts::Image => "image",
        Accepts::List => "list",
    }
}

fn accepts_fragment(accepts: Accepts, fragment: &SourceFragment<ResolvedImagePath>) -> bool {
    matches!(
        (accepts, fragment.kind()),
        (Accepts::Inline, FragmentKind::Heading { .. })
            | (Accepts::Blocks, FragmentKind::Heading { .. })
            | (Accepts::Blocks, FragmentKind::Paragraph)
            | (Accepts::Blocks, FragmentKind::List)
            | (Accepts::Text, FragmentKind::Text)
            | (Accepts::Code, FragmentKind::Code)
            | (Accepts::Image, FragmentKind::Image { .. })
            | (Accepts::List, FragmentKind::List)
    )
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

pub fn render_distribution_index(aspect_ratio: AspectRatio) -> String {
    const TEMPLATE: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <link rel="stylesheet" href="peitho.css">
  <title>Peitho Deck</title>
  <style>
    :root { --peitho-canvas-width: __PEITHO_CANVAS_WIDTH__px; --peitho-canvas-height: __PEITHO_CANVAS_HEIGHT__px; --peitho-canvas-aspect: __PEITHO_CANVAS_ASPECT__; }
    html, body { margin: 0; width: 100%; height: 100%; background: #000; overflow: hidden; touch-action: pan-y; }
    #peitho-slides { position: fixed; inset: 0; overflow: hidden; background: #000; touch-action: pan-y; }
    #peitho-canvas { position: absolute; left: 0; top: 0; width: __PEITHO_CANVAS_WIDTH__px; height: __PEITHO_CANVAS_HEIGHT__px; transform-origin: top left; }
  </style>
</head>
<body>
  <main id="peitho-slides">
    <div id="peitho-canvas"></div>
  </main>
  <script>
    const CANVAS_WIDTH = __PEITHO_CANVAS_WIDTH__;
    const CANVAS_HEIGHT = __PEITHO_CANVAS_HEIGHT__;
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

    function parsePositiveInt(raw) {
      const match = raw.trim().match(/^\d+$/);
      return match === null ? 1 : Number(match[0]);
    }

    function readSlideIndexFromUrl() {
      const searchSlide = new URLSearchParams(location.search).get('slide');
      if (searchSlide !== null) return parsePositiveInt(searchSlide);
      const hash = location.hash;
      if (hash.startsWith('#slide=')) {
        return parsePositiveInt(hash.slice('#slide='.length).split('&')[0]);
      }
      if (hash.startsWith('#') && hash.length > 1) {
        return parsePositiveInt(hash.slice(1).split('&')[0]);
      }
      return 1;
    }

    function showSlide(index) {
      if (slides.length === 0) {
        document.getElementById('peitho-canvas').replaceChildren();
        return;
      }
      const next = Math.max(0, Math.min(index, slides.length - 1));
      currentIndex = next;
      writeSlideIndexToUrl(currentIndex + 1);
      const canvas = document.getElementById('peitho-canvas');
      canvas.innerHTML = slides[next].html;
    }

    function writeSlideIndexToUrl(oneBased) {
      const params = new URLSearchParams(location.search);
      params.set('slide', String(oneBased));
      history.replaceState(null, '', location.pathname + '?' + params.toString() + location.hash);
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
        showSlide(readSlideIndexFromUrl() - 1);
        resizeCanvas();
      } catch (error) {
        showError(error.message);
      }
    }

    function exitToIndex() {
      const referrer = document.referrer;
      if (referrer !== '') {
        try {
          if (new URL(referrer).origin === window.location.origin) {
            window.history.back();
            return;
          }
        } catch (_error) {}
      }
      window.location.assign('/');
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
      if (event.key === 'Escape' && !event.metaKey && !event.ctrlKey && !event.altKey) {
        event.preventDefault();
        exitToIndex();
      }
    });
    // Click-selection guard. Kept in sync with
    // packages/peitho-present/src/clickNavigationGuard.ts.
    let __clickStart = null;
    document.addEventListener('mousedown', (event) => {
      __clickStart = { x: event.clientX, y: event.clientY };
    });
    function hasNonCollapsedSelection() {
      const selection = window.getSelection();
      return selection !== null && !selection.isCollapsed;
    }
    function shouldIgnoreNavigationClick(event) {
      if (hasNonCollapsedSelection()) {
        __clickStart = null;
        return true;
      }
      if (__clickStart === null) return false;
      const moved = Math.hypot(event.clientX - __clickStart.x, event.clientY - __clickStart.y) > 5;
      __clickStart = null;
      return moved;
    }
    document.addEventListener('click', (event) => {
      if (shouldIgnoreNavigationClick(event)) return;
      navigate(event.clientX < window.innerWidth / 4 ? 'prev' : 'next');
    });
    let __swipeState = null;
    document.addEventListener('touchstart', (event) => {
      if (__swipeState !== null) return;
      if (event.touches.length !== 1) return;
      const t = event.touches[0];
      __swipeState = { x: t.clientX, y: t.clientY, t: performance.now() };
    }, { passive: true });
    document.addEventListener('touchend', (event) => {
      if (__swipeState === null) return;
      const state = __swipeState;
      __swipeState = null;
      const t = event.changedTouches[0];
      if (!t) return;
      const dx = t.clientX - state.x;
      const dy = t.clientY - state.y;
      const dt = performance.now() - state.t;
      // Click-suppress: half the swipe threshold. Kept in sync with
      // installSwipeNavigation's `clickSuppressPx = minHorizontalPx / 2`.
      if (Math.abs(dx) >= 25) event.preventDefault();
      if (Math.abs(dx) < 50) return;
      if (Math.abs(dx) / Math.max(Math.abs(dy), 1) <= 1.5) return;
      if (dt > 800) return;
      navigate(dx < 0 ? 'next' : 'prev');
    }, { passive: false });
    document.addEventListener('touchcancel', () => { __swipeState = null; });
    // `#slide=N` / `#N` are load-time fallbacks only; after load, query is canonical.
    window.addEventListener('popstate', () => {
      showSlide(readSlideIndexFromUrl() - 1);
    });
    window.addEventListener('resize', resizeCanvas);
    loadDeck();
  </script>
</body>
</html>"#;

    fill_canvas_tokens(TEMPLATE, aspect_ratio)
}

pub fn render_pdf_document(deck: &Deck<Rendered>) -> String {
    let settings = deck.settings();
    let aspect_ratio = settings.aspect_ratio();
    let resolution = settings.resolution();
    let scale = format_pdf_scale(resolution.width(), aspect_ratio.width());
    let mut slides = String::new();
    for slide in deck.slides() {
        slides.push_str(r#"  <div class="peitho-slide-wrap">"#);
        slides.push_str(slide.html());
        slides.push_str("</div>\n");
    }

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <link rel="stylesheet" href="peitho.css">
  <title>Peitho PDF Export</title>
  <style>
    @page {{ size: {page_width}px {page_height}px; margin: 0; }}
    :root {{ --peitho-canvas-width: {canvas_width}px; --peitho-canvas-height: {canvas_height}px; --peitho-canvas-aspect: {canvas_aspect}; }}
    html, body {{ margin: 0; padding: 0; background: #fff; }}
    body {{ width: {page_width}px; }}
    .peitho-slide-wrap {{ width: {page_width}px; height: {page_height}px; page-break-after: always; page-break-inside: avoid; break-after: page; break-inside: avoid; overflow: hidden; }}
    .peitho-slide-wrap:last-child {{ page-break-after: auto; break-after: auto; }}
    .peitho-slide {{ transform: scale({scale}); transform-origin: top left; }}
  </style>
</head>
<body>
{slides}  <script>
{pdf_flatten_js}
  </script>
</body>
</html>"#,
        page_width = resolution.width(),
        page_height = resolution.height(),
        canvas_width = aspect_ratio.width(),
        canvas_height = aspect_ratio.height(),
        canvas_aspect = aspect_ratio.css_aspect_value(),
        pdf_flatten_js = PDF_FLATTEN_JS,
    )
}

fn format_pdf_scale(page_width: u32, canvas_width: u32) -> String {
    let scale = f64::from(page_width) / f64::from(canvas_width);
    scale.to_string()
}

fn fill_canvas_tokens(template: &str, aspect_ratio: AspectRatio) -> String {
    template
        .replace("__PEITHO_CANVAS_WIDTH__", &aspect_ratio.width().to_string())
        .replace(
            "__PEITHO_CANVAS_HEIGHT__",
            &aspect_ratio.height().to_string(),
        )
        .replace("__PEITHO_CANVAS_ASPECT__", aspect_ratio.css_aspect_value())
}

pub fn render_present_index(aspect_ratio: AspectRatio) -> String {
    const TEMPLATE: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Peitho Present</title>
  <style>
    :root { --peitho-canvas-width: __PEITHO_CANVAS_WIDTH__px; --peitho-canvas-height: __PEITHO_CANVAS_HEIGHT__px; --peitho-canvas-aspect: __PEITHO_CANVAS_ASPECT__; }
    html, body { margin: 0; width: 100%; height: 100%; background: #000; overflow: hidden; touch-action: pan-y; }
    #peitho-present-root { position: fixed; inset: 0; overflow: hidden; background: #000; touch-action: pan-y; }
    .peitho-control-bar { position: fixed; left: 16px; bottom: 16px; z-index: 10; display: flex; gap: 8px; align-items: center; padding: 8px; background: rgba(0, 0, 0, 0.72); color: #fff; border-radius: 6px; }
    .peitho-control-bar[hidden] { display: none; }
    .peitho-time-tracker { position: absolute; left: 0; right: 0; bottom: 0; height: 6px; z-index: 5; pointer-events: none; background: rgba(255, 255, 255, 0.18); }
    .peitho-time-tracker [data-peitho-marker] { position: absolute; transition: left 120ms linear, transform 120ms linear; font-size: 18px; line-height: 1; }
    .peitho-time-tracker [data-peitho-marker="rabbit"],
    .peitho-time-tracker [data-peitho-marker="turtle"] { bottom: 8px; }
    .peitho-time-tracker[data-peitho-overrun] { background: rgba(255, 92, 92, 0.35); }
  </style>
</head>
<body>
  <main id="peitho-present-root"></main>
  <!-- Runtime controls include data-peitho-action="close". -->
  <script type="module">
    import * as peitho from './shell.js';

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
        const configPromise = fetchOk('present.json')
          .then((response) => response.json())
          .catch((_error) => {
            console.error("failed to load present.json; time tracker and display swap disabled");
            return null;
          });
        peitho.installCloseOnEscape(window);
        peitho.installKeyboardNavigation(window);
        peitho.installPresentationControls({ root, window, document });
        peitho.installCanvasClickNavigation({ root, window });
        peitho.installSwipeNavigation({ root, window });
        peitho.installFullscreenShortcut({ window, document });
        const shell = await peitho.mountPresentShell({ root });
        peitho.installSyncBridge(window, peitho.serverSyncChannelFactory());
        const config = await configPromise;
        if (config != null && config.presenterOpen) {
          if (typeof peitho.installSwapShortcut === 'function') {
            peitho.installSwapShortcut(window);
          } else {
            console.error("shell bundle does not provide installSwapShortcut; display swap disabled");
          }
        }
        const rawPlannedDurationMs = shell.manifest?.plannedDurationMs ?? null;
        const plannedDurationMs =
          rawPlannedDurationMs != null && peitho.isValidDurationMs(rawPlannedDurationMs)
            ? rawPlannedDurationMs
            : null;
        if (rawPlannedDurationMs != null && plannedDurationMs == null) {
          console.error("Invalid plannedDurationMs in manifest.json");
        }
        if (plannedDurationMs != null) {
          if (typeof peitho.installTimeTracker === 'function') {
            if (config != null && !config.presenterOpen) {
              peitho.installTimeTracker({
                root,
                shell,
                plannedDurationMs,
                window,
                document,
                variant: 'present'
              });
            }
          } else {
            console.error("shell bundle does not provide installTimeTracker; time tracker disabled");
          }
        }
      } catch (error) {
        showError(error.message);
      }
    }

    main();
  </script>
</body>
</html>"#;

    fill_canvas_tokens(TEMPLATE, aspect_ratio)
}

pub fn render_preview_index(aspect_ratio: AspectRatio) -> String {
    const TEMPLATE: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Peitho Preview</title>
  <style>
    :root { --peitho-canvas-width: __PEITHO_CANVAS_WIDTH__px; --peitho-canvas-height: __PEITHO_CANVAS_HEIGHT__px; --peitho-canvas-aspect: __PEITHO_CANVAS_ASPECT__; }
    html, body { margin: 0; width: 100%; height: 100%; background: #000; overflow: hidden; }
    #peitho-preview-root { position: fixed; inset: 0; overflow: hidden; background: #000; }
  </style>
</head>
<body>
  <main id="peitho-preview-root"></main>
  <script type="module">
    import * as peitho from './preview.js';

    function showError(message) {
      const root = document.getElementById('peitho-preview-root');
      root.textContent = message;
    }

    async function main() {
      const root = document.getElementById('peitho-preview-root');
      try {
        peitho.installPreviewKeyboard(window);
        const shell = await peitho.mountPreviewShell({ root });
        peitho.installPreviewReload(shell);
      } catch (error) {
        showError(error.message);
      }
    }

    main();
  </script>
</body>
</html>"#;

    fill_canvas_tokens(TEMPLATE, aspect_ratio)
}

pub fn render_preview_error_index(generation: u64, error: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Peitho Preview Error</title>
  <style>
    html, body {{ margin: 0; min-height: 100%; background: #111; color: #f5f5f5; font: 16px/1.5 ui-monospace, SFMono-Regular, Menlo, monospace; }}
    main {{ box-sizing: border-box; min-height: 100vh; padding: 24px; white-space: pre-wrap; }}
  </style>
</head>
<body>
  <main>{}</main>
  <script>
    const baselineGeneration = {};
    let seq = null;
    async function poll() {{
      try {{
        const response = seq === null ? await fetch('/sync') : await fetch(`/sync?seq=${{seq}}`);
        if (response.status === 204) {{
          poll();
          return;
        }}
        const body = await response.json();
        seq = body.seq;
        if (body.generation !== baselineGeneration) {{
          location.reload();
          return;
        }}
      }} catch (_error) {{
        await new Promise((resolve) => setTimeout(resolve, 1000));
      }}
      poll();
    }}
    poll();
  </script>
</body>
</html>"#,
        encode_text(error),
        generation
    )
}

pub fn render_presenter_index(aspect_ratio: AspectRatio) -> String {
    const TEMPLATE: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <link rel="preconnect" href="https://fonts.googleapis.com">
  <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
  <link href="https://fonts.googleapis.com/css2?family=Geist:wght@400;500;600;700&family=Geist+Mono:wght@400;500;600&display=swap" rel="stylesheet">
  <title>Peitho Presenter</title>
  <style>
    :root {
      --bg: oklch(15% 0.01 250);
      --bg-elev: oklch(19% 0.012 250);
      --bg-slide: oklch(8% 0.005 250);
      --line: oklch(28% 0.015 250);
      --line-soft: oklch(24% 0.012 250);
      --fg: oklch(96% 0.005 250);
      --fg-mute: oklch(72% 0.01 250);
      --fg-dim: oklch(52% 0.012 250);
      --accent: oklch(78% 0.14 195);
      --accent-soft: oklch(78% 0.14 195 / 0.14);
      --warn: oklch(72% 0.19 30);
      --warn-soft: oklch(72% 0.19 30 / 0.18);
      --pause: oklch(82% 0.15 90);
      --stage-gap: 12px;
      --colhead-h: 18px;
      --kbdbar-h: 22px;
      --notes-h: 24vh;
      --peitho-canvas-width: __PEITHO_CANVAS_WIDTH__px;
      --peitho-canvas-height: __PEITHO_CANVAS_HEIGHT__px;
      --peitho-canvas-aspect: __PEITHO_CANVAS_ASPECT__;
    }
    html, body { margin: 0; width: 100%; height: 100%; background: var(--bg); color: var(--fg); overflow: hidden; }
    body { font-family: "Geist", ui-sans-serif, system-ui, -apple-system, "Hiragino Kaku Gothic ProN", sans-serif; font-size: 14px; letter-spacing: 0; }
    [hidden] { display: none !important; }
    .mono { font-family: "Geist Mono", ui-monospace, monospace; font-variant-numeric: tabular-nums; }
    #peitho-presenter-root { min-height: 100vh; height: 100vh; }
    .app { display: grid; grid-template-columns: minmax(0, 1.7fr) minmax(400px, 1fr); gap: 20px; padding: 20px; box-sizing: border-box; height: 100vh; max-height: 100vh; }
    .left { container-type: size; min-height: 0; min-width: 0; display: flex; justify-content: center; }
    .stage { display: flex; flex-direction: column; gap: var(--stage-gap); height: 100%; min-width: 0; width: max(280px, min(100%, calc((100cqh - var(--colhead-h) - var(--kbdbar-h) - var(--notes-h) - 3 * var(--stage-gap)) * var(--peitho-canvas-aspect)))); }
    .right { display: grid; grid-template-rows: auto minmax(0, 1fr); gap: 16px; min-height: 0; min-width: 0; }
    .colhead { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 0 2px; min-width: 0; height: var(--colhead-h); }
    .status-line { display: inline-flex; align-items: center; gap: 10px; color: var(--fg-dim); font-size: 11px; letter-spacing: 0.14em; text-transform: uppercase; min-width: 0; }
    .status-line .sep { width: 3px; height: 3px; border-radius: 50%; background: var(--line); flex: 0 0 auto; }
    .deck-title { font-size: 12px; color: var(--fg-mute); letter-spacing: 0; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
    .slide-frame { min-width: 0; }
    .slide-pane { position: relative; width: 100%; box-sizing: border-box; aspect-ratio: var(--peitho-canvas-aspect); background: var(--bg-slide); border: 1px solid var(--line-soft); box-shadow: 0 20px 60px -30px rgba(0, 0, 0, 0.6); overflow: hidden; }
    .peitho-presenter-pane { position: relative; overflow: hidden; background: var(--bg-slide); min-height: 0; }
    .kbdbar { display: flex; align-items: center; justify-content: space-between; gap: 12px; padding: 0 2px; color: var(--fg-dim); font-size: 12px; height: var(--kbdbar-h); flex: 0 0 auto; }
    .kbdbar .pos { color: var(--fg-mute); font-family: "Geist Mono", ui-monospace, monospace; font-variant-numeric: tabular-nums; }
    .kbd { display: inline-flex; align-items: center; padding: 2px 6px; border: 1px solid var(--line); border-bottom-width: 2px; border-radius: 4px; font-family: "Geist Mono", ui-monospace, monospace; font-size: 11px; color: var(--fg-mute); line-height: 1.2; }
    .kbdbar .grp { display: inline-flex; align-items: center; gap: 6px; margin-left: 14px; white-space: nowrap; }
    .kbdbar .grp:first-of-type { margin-left: 0; }
    .notes { background: var(--bg-elev); border: 1px solid var(--line-soft); display: grid; grid-template-rows: auto minmax(0, 1fr); flex: 1 0 var(--notes-h); max-height: 42vh; overflow: hidden; }
    .notes-head { display: flex; align-items: center; justify-content: space-between; padding: 6px 14px; border-bottom: 1px solid var(--line-soft); color: var(--fg-dim); font-size: 10px; letter-spacing: 0.16em; text-transform: uppercase; }
    .notes-head .badge { color: var(--fg-mute); letter-spacing: 0; text-transform: none; font-size: 11px; }
    .notes-body { overflow: auto; padding: 10px 16px 12px; font-size: 16px; line-height: 1.4; color: var(--fg); white-space: pre-wrap; }
    .notes-body.is-empty { color: var(--fg-dim); font-style: italic; }
    .card { background: var(--bg-elev); border: 1px solid var(--line-soft); display: flex; flex-direction: column; min-height: 0; }
    .card-head { display: flex; align-items: center; justify-content: space-between; padding: 8px 14px; border-bottom: 1px solid var(--line-soft); color: var(--fg-dim); font-size: 10px; letter-spacing: 0.16em; text-transform: uppercase; }
    .card-head .badge { color: var(--fg-mute); letter-spacing: 0; text-transform: none; font-size: 11px; }
    .next-wrap { padding: 12px; }
    .next-preview { position: relative; aspect-ratio: var(--peitho-canvas-aspect); background: var(--bg-slide); border: 1px solid var(--line-soft); overflow: hidden; container-type: inline-size; }
    .next-preview > [data-peitho-presenter="preview"] { position: absolute; inset: 0; }
    [data-peitho-presenter="preview-end"] { position: absolute; inset: 0; margin: 0; box-sizing: border-box; padding: 9.6% 13.5%; display: flex; flex-direction: column; background: var(--bg-slide); color: var(--fg); }
    [data-peitho-presenter="preview-end"] .eod-top,
    [data-peitho-presenter="preview-end"] .eod-bottom { display: flex; justify-content: space-between; font-size: 10px; line-height: 1; letter-spacing: 0.16em; text-transform: uppercase; color: var(--fg-dim); }
    [data-peitho-presenter="preview-end"] .eod-center { flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 4.5%; }
    [data-peitho-presenter="preview-end"] .eod-fin { display: flex; align-items: center; gap: 16px; font-size: 10px; line-height: 1; letter-spacing: 0.24em; text-transform: uppercase; color: var(--fg-dim); }
    [data-peitho-presenter="preview-end"] .eod-rule { display: block; width: 42px; height: 1px; background: var(--line); }
    [data-peitho-presenter="preview-end"] .eod-title { font-size: 8.3cqi; font-weight: 600; line-height: 1; letter-spacing: -0.015em; color: var(--fg); }
    @media (prefers-reduced-motion: no-preference) {
      [data-peitho-presenter="preview-end"]:not([hidden]) { animation: eod-fade-in 500ms ease-out both; }
      [data-peitho-presenter="preview-end"]:not([hidden]) .eod-rule:first-child { transform-origin: right center; animation: eod-rule-in 500ms cubic-bezier(0.6, 0, 0.2, 1) 150ms both; }
      [data-peitho-presenter="preview-end"]:not([hidden]) .eod-rule:last-child { transform-origin: left center; animation: eod-rule-in 500ms cubic-bezier(0.6, 0, 0.2, 1) 150ms both; }
      [data-peitho-presenter="preview-end"]:not([hidden]) .eod-title { animation: eod-title-in 550ms cubic-bezier(0.2, 0, 0.2, 1) 350ms both; }
    }
    @keyframes eod-fade-in { from { opacity: 0; } to { opacity: 1; } }
    @keyframes eod-rule-in { from { transform: scaleX(0); } to { transform: scaleX(1); } }
    @keyframes eod-title-in { from { opacity: 0; transform: translateY(4px); } to { opacity: 1; transform: none; } }
    .clock { display: flex; flex-direction: column; min-height: 0; }
    .clock-row { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: end; gap: 12px; padding: 12px 16px 6px; }
    .timer { display: block; font-size: 48px; font-weight: 500; letter-spacing: 0; line-height: 1; color: var(--fg); transition: color 200ms ease; font-variant-numeric: tabular-nums; }
    .timer .planned { color: var(--fg-dim); font-weight: 400; margin-left: 8px; font-size: 18px; letter-spacing: 0; transition: color 200ms ease; }
    .timer .overrun { color: var(--warn); font-weight: 500; margin-left: 8px; font-size: 18px; letter-spacing: 0; }
    .clock[data-peitho-state="paused"] .timer { color: var(--fg-dim); }
    .clock[data-peitho-state="stopped"] .timer { color: var(--fg-dim); }
    .clock[data-peitho-state="running"][data-peitho-urgency="warning"] .timer,
    .clock[data-peitho-state="running"][data-peitho-urgency="warning"] .timer .planned { color: var(--pause); }
    .clock[data-peitho-state="running"][data-peitho-urgency="urgent"] .timer,
    .clock[data-peitho-state="running"][data-peitho-urgency="urgent"] .timer .planned { color: var(--warn); }
    .clock[data-peitho-state][data-peitho-urgency="overrun"] .timer,
    .clock[data-peitho-state][data-peitho-urgency="overrun"] .timer .planned { color: var(--warn); }
    .state-pill { display: inline-flex; align-items: center; gap: 8px; padding: 6px 10px; border: 1px solid var(--line); font-size: 10px; letter-spacing: 0.16em; text-transform: uppercase; color: var(--fg-mute); transition: color 150ms ease, border-color 150ms ease; white-space: nowrap; }
    .state-dot { width: 6px; height: 6px; background: var(--fg-dim); border-radius: 50%; transition: background 150ms ease, box-shadow 150ms ease; }
    .state-pill[data-peitho-state="running"] { color: var(--accent); border-color: color-mix(in oklch, var(--accent) 45%, var(--line)); }
    .state-pill[data-peitho-state="running"] .state-dot { background: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); animation: pulse 1.4s ease-in-out infinite; }
    .state-pill[data-peitho-state="paused"] { color: var(--pause); border-color: color-mix(in oklch, var(--pause) 45%, var(--line)); }
    .state-pill[data-peitho-state="paused"] .state-dot { background: var(--pause); animation: none; box-shadow: none; }
    .state-pill[data-peitho-state="stopped"] { color: var(--fg-dim); }
    .state-pill[data-peitho-state="stopped"] .state-dot { background: var(--fg-dim); animation: none; box-shadow: none; }
    @keyframes pulse { 50% { box-shadow: 0 0 0 6px transparent; } }
    .tracker-wrap { padding: 4px 16px 14px; }
    .tracker-wrap:empty { display: none; }
    .peitho-time-tracker[data-peitho-time-tracker="presenter"] { display: block; pointer-events: none; }
    .tracker-legend { display: flex; justify-content: space-between; color: var(--fg-dim); font-size: 10px; letter-spacing: 0.14em; text-transform: uppercase; margin-bottom: 6px; }
    .tracker { position: relative; height: 30px; background: color-mix(in oklch, var(--fg) 8%, transparent); border: 1px solid var(--line-soft); }
    .tracker-fill { position: absolute; top: 0; bottom: 0; left: 0; background: linear-gradient(90deg, color-mix(in oklch, var(--accent) 22%, transparent), color-mix(in oklch, var(--accent) 8%, transparent)); }
    .peitho-time-tracker[data-peitho-overrun] .tracker { border-color: color-mix(in oklch, var(--warn) 45%, var(--line)); background: var(--warn-soft); }
    .peitho-time-tracker[data-peitho-overrun] .tracker-fill { background: linear-gradient(90deg, color-mix(in oklch, var(--warn) 24%, transparent), color-mix(in oklch, var(--warn) 10%, transparent)); }
    .tracker [data-peitho-marker="rabbit"],
    .tracker [data-peitho-marker="turtle"] { position: absolute; transition: left 120ms linear, transform 120ms linear; font-size: 18px; line-height: 1; }
    .tracker [data-peitho-marker="rabbit"] { top: -6px; }
    .tracker [data-peitho-marker="turtle"] { bottom: -6px; }
    .tracker-scale { position: relative; height: 12px; margin-top: 6px; color: var(--fg-dim); font-size: 10px; letter-spacing: 0.08em; }
    .tracker-scale span { position: absolute; top: 0; white-space: nowrap; }
    [data-peitho-agenda] { overflow: hidden; padding: 0 16px 14px; }
    [data-peitho-agenda-head] { display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-bottom: 4px; color: var(--fg-dim); font-size: 10px; letter-spacing: 0.14em; text-transform: uppercase; }
    [data-peitho-agenda-title] { color: var(--fg-mute); }
    [data-peitho-agenda-hint] { white-space: nowrap; }
    [data-peitho-presenter="agenda-slot"] { min-height: 0; overflow: hidden; }
    [data-peitho-agenda-list] { display: grid; }
    [data-peitho-agenda-row] { display: grid; grid-template-columns: 10px minmax(0, 1fr) auto auto; gap: 8px; align-items: center; min-height: 28px; padding: 6px 0; }
    [data-peitho-agenda-row] + [data-peitho-agenda-row] { border-top: 1px solid var(--line-soft); }
    [data-peitho-agenda-marker] { width: 8px; height: 8px; border-radius: 50%; border: 1px solid var(--fg-dim); box-sizing: border-box; }
    [data-peitho-agenda-state="done"] [data-peitho-agenda-marker] { background: var(--fg-dim); border-color: var(--fg-dim); }
    [data-peitho-agenda-state="current"] [data-peitho-agenda-marker] { background: var(--accent); border-color: var(--accent); box-shadow: 0 0 0 3px var(--accent-soft); }
    [data-peitho-agenda-state="upcoming"] [data-peitho-agenda-marker] { background: transparent; border-color: var(--fg-dim); }
    [data-peitho-agenda-label] { min-width: 0; display: flex; align-items: baseline; gap: 8px; }
    [data-peitho-agenda-name] { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--fg-mute); }
    [data-peitho-agenda-state="done"] [data-peitho-agenda-name] { color: var(--fg-dim); }
    [data-peitho-agenda-state="current"] [data-peitho-agenda-name] { color: var(--fg); font-weight: 600; }
    [data-peitho-agenda-range] { color: var(--fg-dim); font-size: 10px; letter-spacing: 0.08em; flex-shrink: 0; white-space: nowrap; }
    [data-peitho-agenda-time],
    [data-peitho-agenda-delta] { font-family: "Geist Mono", ui-monospace, monospace; font-variant-numeric: tabular-nums; white-space: nowrap; color: var(--fg-dim); }
    [data-peitho-agenda-delta] { min-width: 6ch; text-align: right; }
    [data-peitho-agenda-state="current"] [data-peitho-agenda-time] { color: var(--accent); }
    [data-peitho-agenda-state="done"][data-peitho-agenda-outcome="under"] [data-peitho-agenda-time],
    [data-peitho-agenda-state="done"][data-peitho-agenda-outcome="under"] [data-peitho-agenda-delta] { color: color-mix(in oklch, var(--accent) 72%, var(--fg-mute)); }
    [data-peitho-agenda-state][data-peitho-agenda-outcome="over"] [data-peitho-agenda-time],
    [data-peitho-agenda-state][data-peitho-agenda-outcome="over"] [data-peitho-agenda-delta] { color: var(--warn); }
    .controls { display: grid; grid-template-columns: minmax(max-content, 1fr) auto auto auto auto auto; align-items: center; gap: 6px; padding: 10px 8px; border-top: 1px solid var(--line-soft); background: color-mix(in oklch, var(--bg-elev) 92%, transparent); margin-top: auto; }
    .btn { appearance: none; border: 1px solid var(--line); background: transparent; color: var(--fg-mute); padding: 4px 8px; font: inherit; font-size: 11px; letter-spacing: 0.06em; text-transform: uppercase; cursor: pointer; display: inline-flex; align-items: center; justify-content: center; gap: 6px; white-space: nowrap; transition: background 90ms ease, color 90ms ease, border-color 90ms ease, transform 60ms ease, box-shadow 90ms ease; min-width: 0; position: relative; overflow: hidden; -webkit-tap-highlight-color: transparent; }
    .btn .k { color: var(--fg-dim); font-family: "Geist Mono", ui-monospace, monospace; font-size: 10px; letter-spacing: 0.04em; text-transform: none; }
    .btn:hover { color: var(--fg); border-color: color-mix(in oklch, var(--fg) 35%, var(--line)); background: color-mix(in oklch, var(--fg) 6%, transparent); }
    .btn:focus-visible { outline: none; border-color: var(--accent); box-shadow: 0 0 0 2px var(--accent-soft); }
    .btn:active { transform: translateY(2px); background: var(--accent); border-color: var(--accent); color: var(--bg); box-shadow: 0 0 24px var(--accent-soft), inset 0 2px 4px rgba(0, 0, 0, 0.2); }
    .btn:active .k { color: var(--bg); }
    .btn.primary { color: var(--bg); background: var(--accent); border-color: var(--accent); font-weight: 600; min-width: max-content; }
    .btn.primary .k { color: color-mix(in oklch, var(--bg) 60%, var(--accent)); }
    .btn.primary:hover { background: color-mix(in oklch, var(--accent) 88%, white); }
    .btn.primary:active { background: color-mix(in oklch, var(--accent) 60%, black); border-color: color-mix(in oklch, var(--accent) 60%, black); color: white; box-shadow: inset 0 3px 6px rgba(0, 0, 0, 0.4); }
    .btn.primary:active .k { color: color-mix(in oklch, white 60%, var(--accent)); }
    .clock[data-peitho-state="paused"] .btn.play.primary { background: var(--pause); border-color: var(--pause); color: var(--bg); }
    .clock[data-peitho-state="paused"] .btn.play.primary .k { color: color-mix(in oklch, var(--bg) 60%, var(--pause)); }
    .btn.danger { color: var(--warn); border-color: color-mix(in oklch, var(--warn) 45%, var(--line)); }
    .btn.danger:hover { background: color-mix(in oklch, var(--warn) 12%, transparent); color: var(--warn); }
    .btn.danger:active { background: var(--warn); color: var(--bg); border-color: var(--warn); box-shadow: 0 0 24px color-mix(in oklch, var(--warn) 30%, transparent); }
    .btn::before { content: ""; position: absolute; inset: 0; background: radial-gradient(circle at var(--rx, 50%) var(--ry, 50%), color-mix(in oklch, white 60%, transparent) 0%, transparent 45%); opacity: 0; pointer-events: none; transform: scale(0.3); }
    .btn.primary::before { background: radial-gradient(circle at var(--rx, 50%) var(--ry, 50%), color-mix(in oklch, black 55%, transparent) 0%, transparent 45%); }
    .btn.danger::before { background: radial-gradient(circle at var(--rx, 50%) var(--ry, 50%), color-mix(in oklch, white 55%, transparent) 0%, transparent 45%); }
    .btn.pressed::before { animation: btn-ripple 500ms ease-out; }
    @keyframes btn-ripple { 0% { opacity: 0.85; transform: scale(0.3); } 100% { opacity: 0; transform: scale(1.6); } }
    @media (max-width: 1100px) {
      html, body { overflow: auto; }
      .app { grid-template-columns: 1fr; grid-template-rows: minmax(0, 1fr) auto; min-height: 100vh; height: auto; }
      .right { grid-template-rows: auto auto; }
    }
    @media (prefers-reduced-motion: reduce) {
      *, *::before, *::after { animation-duration: 0.001ms !important; animation-iteration-count: 1 !important; transition-duration: 0.001ms !important; scroll-behavior: auto !important; }
      .btn:active { transform: none; }
      .btn.pressed::before { animation: none; }
      .tracker [data-peitho-marker] { transition: none; }
    }
  </style>
</head>
<body>
  <main id="peitho-presenter-root"></main>
  <!-- Runtime presenter controls include data-peitho-action="playpause" and data-peitho-action="close". -->
  <script type="module">
    import * as peitho from './shell.js';

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
        peitho.installCloseOnEscape(window);
        await peitho.mountPresenterView({
          root,
          notes,
          syncChannelFactory: peitho.serverSyncChannelFactory()
        });
        if (typeof peitho.installSwapShortcut === 'function') {
          peitho.installSwapShortcut(window);
        } else {
          console.error("shell bundle does not provide installSwapShortcut; display swap disabled");
        }
      } catch (error) {
        showError(error.message);
      }
    }

    main();
  </script>
</body>
</html>"#;

    fill_canvas_tokens(TEMPLATE, aspect_ratio)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        check::check_deck,
        domain::AspectRatio,
        layout::parse_layout,
        mapping::map_by_convention,
        parser::{parse_frontmatter, parse_markdown as parse_markdown_impl},
        phase::{CheckedSlide, CheckedSlot, DeckSettings, PlannedTime},
    };

    fn parse_markdown(
        source: &str,
        highlighter: &crate::highlight::Highlighter,
    ) -> crate::error::Result<Deck<crate::phase::Parsed>> {
        let frontmatter = parse_frontmatter(source)?;
        parse_markdown_impl(source, frontmatter, highlighter)
    }

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
        let checked = check_deck(
            map_by_convention(
                parse_markdown(markdown, &crate::highlight::Highlighter::defaults()).unwrap(),
                &layout,
            )
            .unwrap(),
        )
        .unwrap();

        let rendered = render_checked(checked);
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
        let checked = check_deck(
            map_by_convention(
                parse_markdown(markdown, &crate::highlight::Highlighter::defaults()).unwrap(),
                &layout,
            )
            .unwrap(),
        )
        .unwrap();

        let rendered = render_checked(checked);
        let html = rendered.slides()[0].html();

        assert!(html.contains(r#"<figure class="code"></figure>"#));
        assert!(!html.contains(r#"class="slot-code""#));
    }

    #[test]
    fn untagged_code_slot_trims_trailing_newline_like_highlighted_code() {
        let markdown = "```\nlet value = 1;\n```";
        let layout = parse_layout(
            "code-only",
            r#"<section><slot name="code" accepts="code" arity="1"></slot></section>"#,
        )
        .unwrap();
        let checked = check_deck(
            map_by_convention(
                parse_markdown(markdown, &crate::highlight::Highlighter::defaults()).unwrap(),
                &layout,
            )
            .unwrap(),
        )
        .unwrap();

        let rendered = render_checked(checked);
        let html = rendered.slides()[0].html();

        assert!(html.contains(r#"<pre class="slot-code"><code>let value = 1;</code></pre>"#));
        assert!(!html.contains("let value = 1;\n</code>"));
    }

    #[test]
    fn renders_inline_markup_in_heading_slot() {
        let markdown = "# **Architecture** `Phase` [docs](https://example.com)";
        let layout = parse_layout(
            "title-only",
            r#"<section><h1><slot name="title" accepts="inline" arity="1"></slot></h1></section>"#,
        )
        .unwrap();
        let checked = check_deck(
            map_by_convention(
                parse_markdown(markdown, &crate::highlight::Highlighter::defaults()).unwrap(),
                &layout,
            )
            .unwrap(),
        )
        .unwrap();

        let rendered = render_checked(checked);
        let html = rendered.slides()[0].html();

        assert!(html.contains("<strong>Architecture</strong>"));
        assert!(html.contains("<code>Phase</code>"));
        assert!(html.contains(r#"<a href="https://example.com">docs</a>"#));
        assert!(!html.contains("<p><strong>Architecture</strong>"));
    }

    #[test]
    fn renders_image_with_resolved_src_and_escaped_alt() {
        let layout = parse_layout(
            "visual",
            r#"<section><figure><slot name="hero" accepts="image" arity="1"></slot></figure></section>"#,
        )
        .unwrap();
        let hero = SlotName::new("hero").unwrap();
        let contract = layout.slot("hero").unwrap().clone();
        let mut slots = BTreeMap::new();
        slots.insert(
            hero,
            CheckedSlot::new(
                contract,
                vec![SourceFragment::image(
                    3,
                    "<Diagram>, \"Notes\" & emoji 🎉",
                    ResolvedImagePath::from_string("assets/xxx.png".to_owned()),
                )],
            ),
        );
        let checked = Deck::checked(
            DeckSettings::default(),
            vec![CheckedSlide::new(
                0,
                SlideKey::new("visual").unwrap(),
                layout,
                slots,
                None,
            )],
        );

        let rendered = render_deck(checked, &crate::highlight::Highlighter::defaults()).unwrap();
        let html = rendered.slides()[0].html();

        assert!(html.contains(
            r#"<img src="assets/xxx.png" alt="&lt;Diagram&gt;, &quot;Notes&quot; &amp; emoji 🎉">"#
        ));
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
                parse_markdown(
                    "**Architecture** `Phase`\n====",
                    &crate::highlight::Highlighter::defaults(),
                )
                .unwrap(),
                &layout,
            )
            .unwrap(),
        )
        .unwrap();
        let setext_html = render_checked(setext).slides()[0].html().to_owned();
        assert!(setext_html.contains("<strong>Architecture</strong>"));
        assert!(setext_html.contains("<code>Phase</code>"));
        assert!(!setext_html.contains(r#"<span class="slot-title"><h1>"#));

        let atx = check_deck(
            map_by_convention(
                parse_markdown(
                    "# Architecture #",
                    &crate::highlight::Highlighter::defaults(),
                )
                .unwrap(),
                &layout,
            )
            .unwrap(),
        )
        .unwrap();
        let atx_html = render_checked(atx).slides()[0].html().to_owned();
        assert!(atx_html.contains(r#"<span class="slot-title">Architecture</span>"#));
        assert!(!atx_html.contains("Architecture #"));
    }

    #[test]
    fn renders_heading_paragraph_and_list_in_blocks_slot_as_block_html() {
        let markdown = r#"# Title

::: {slot=left}

## Block Heading

Paragraph after heading.

- First
- Second

:::
"#;
        let layout = parse_layout(
            "two-column",
            r#"<section>
  <h1><slot name="title" accepts="inline" arity="1"></slot></h1>
  <slot name="left" accepts="blocks" arity="1..*"></slot>
</section>"#,
        )
        .unwrap();
        let checked = check_deck(
            map_by_convention(
                parse_markdown(markdown, &crate::highlight::Highlighter::defaults()).unwrap(),
                &layout,
            )
            .unwrap(),
        )
        .unwrap();

        let rendered = render_checked(checked);
        let html = rendered.slides()[0].html();

        assert!(html.contains(r#"<div class="slot-left">"#));
        assert!(html.contains("<h2>Block Heading</h2>"));
        assert!(html.contains("<p>Paragraph after heading.</p>"));
        assert!(html.contains("<ul>"));
        assert!(html.contains("<li>First</li>"));
        assert!(html.contains("<li>Second</li>"));
        assert!(!html.contains(r#"<span class="slot-left">"#));
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
        let parsed = parse_markdown(
            "---\ntime: 15m\nlayouts: ./layouts\ncss: ./css\nsyntaxes: ./syntaxes\n---\n# Intro",
            &crate::highlight::Highlighter::defaults(),
        )
        .unwrap();
        let mapped = map_by_convention(parsed, &layout).unwrap();
        let checked = check_deck(mapped).unwrap();
        let rendered = render_checked(checked);

        assert_eq!(
            rendered
                .settings()
                .planned_time()
                .map(PlannedTime::as_millis),
            Some(900_000)
        );
        assert_eq!(
            rendered.settings().layouts().map(|path| path.as_path()),
            Some(std::path::Path::new("./layouts"))
        );
        assert_eq!(
            rendered.settings().css().map(|path| path.as_path()),
            Some(std::path::Path::new("./css"))
        );
        assert_eq!(
            rendered.settings().syntaxes().map(|path| path.as_path()),
            Some(std::path::Path::new("./syntaxes"))
        );
    }

    #[test]
    fn distribution_index_uses_one_slide_canvas_without_shell_bundle() {
        let html = render_distribution_index(AspectRatio::Ratio16To9);

        assert!(html.contains(r#"<link rel="stylesheet" href="peitho.css">"#));
        assert!(html.contains(r#"id="peitho-canvas""#));
        assert!(html.contains("--peitho-canvas-width: 1280px;"));
        assert!(html.contains("--peitho-canvas-height: 720px;"));
        assert!(html.contains("width: 1280px; height: 720px;"));
        assert!(html.contains(
            "html, body { margin: 0; width: 100%; height: 100%; background: #000; overflow: hidden; touch-action: pan-y; }"
        ));
        assert!(html.contains(
            "#peitho-slides { position: fixed; inset: 0; overflow: hidden; background: #000; touch-action: pan-y; }"
        ));
        assert!(html.contains("const CANVAS_WIDTH = 1280"));
        assert!(html.contains("const CANVAS_HEIGHT = 720"));
        assert!(html.contains("function resizeCanvas()"));
        assert!(html.contains("function showSlide(index)"));
        assert!(html.contains("document.addEventListener('keydown'"));
        assert!(html.contains("document.addEventListener('click'"));
        assert!(html.contains("__swipeState"));
        assert!(html.contains("document.addEventListener('touchstart'"));
        assert!(html.contains("fetchOk('manifest.json')"));
        assert!(html.contains("fetchOk(slide.src)"));
        assert!(html.contains("response.ok"));
        assert!(!html.contains("shell.js"));
        assert!(!html.contains("installPresentationControls"));
        assert!(!html.contains("data-slide-key="));
    }

    #[test]
    fn distribution_index_click_navigation_ignores_selection_gestures() {
        let html = render_distribution_index(AspectRatio::Ratio16To9);

        assert!(html.contains("window.getSelection()"));
        assert!(html.contains("__clickStart"));
        assert!(html.contains("Math.hypot(event.clientX - __clickStart.x"));
        assert!(html.contains("return selection !== null && !selection.isCollapsed"));
    }

    #[test]
    fn distribution_index_reads_slide_query_on_load() {
        let html = render_distribution_index(AspectRatio::Ratio16To9);

        assert!(html.contains("URLSearchParams(location.search)"));
        assert!(html.contains(".get('slide')"));
        assert!(html.contains("showSlide(readSlideIndexFromUrl() - 1)"));
    }

    #[test]
    fn distribution_index_supports_hash_fallback() {
        let html = render_distribution_index(AspectRatio::Ratio16To9);

        assert!(html.contains("location.hash"));
        assert!(html.contains("hash.startsWith('#slide=')"));
        assert!(html.contains("hash.startsWith('#')"));
        assert!(html.contains(".split('&')[0]"));
    }

    #[test]
    fn distribution_index_updates_url_on_slide_change() {
        let html = render_distribution_index(AspectRatio::Ratio16To9);

        assert!(html.contains("history.replaceState"));
        let show_slide_index = html.find("function showSlide(index)").unwrap();
        let current_index = html.find("currentIndex = next;").unwrap();
        let write_call_index = html
            .find("writeSlideIndexToUrl(currentIndex + 1);")
            .unwrap();
        let replace_state_index = html[current_index..]
            .find("history.replaceState")
            .map(|index| current_index + index)
            .unwrap();

        assert!(show_slide_index < current_index);
        assert!(current_index < write_call_index);
        assert!(current_index < replace_state_index);
    }

    #[test]
    fn distribution_index_never_uses_pushstate() {
        let html = render_distribution_index(AspectRatio::Ratio16To9);

        assert!(!html.contains("history.pushState"));
    }

    #[test]
    fn distribution_index_handles_popstate() {
        let html = render_distribution_index(AspectRatio::Ratio16To9);

        assert!(html.contains("window.addEventListener('popstate'"));
    }

    #[test]
    fn distribution_index_preserves_query_and_hash_when_writing_slide() {
        let html = render_distribution_index(AspectRatio::Ratio16To9);

        assert!(html.contains("location.pathname"));
        assert!(html.contains("params.set('slide'"));
        assert!(html.contains("location.hash"));
        assert!(html.contains("location.pathname + '?' + params.toString() + location.hash"));
    }

    #[test]
    fn distribution_index_url_deep_link_leaves_present_and_presenter_alone() {
        let present_html = render_present_index(AspectRatio::Ratio16To9);
        let presenter_html = render_presenter_index(AspectRatio::Ratio16To9);

        assert!(!present_html.contains("?slide="));
        assert!(!present_html.contains("URLSearchParams"));
        assert!(!presenter_html.contains("?slide="));
        assert!(!presenter_html.contains("URLSearchParams"));
    }

    #[test]
    fn distribution_index_uses_deck_aspect_ratio_canvas_dimensions() {
        let html = render_distribution_index(AspectRatio::Ratio4To3);

        assert!(html.contains("--peitho-canvas-width: 960px;"));
        assert!(html.contains("--peitho-canvas-height: 720px;"));
        assert!(html.contains("width: 960px; height: 720px;"));
        assert!(html.contains("const CANVAS_WIDTH = 960"));
        assert!(html.contains("const CANVAS_HEIGHT = 720"));
    }

    #[test]
    fn present_index_mounts_shell_controls_keyboard_sync_and_notes() {
        let html = render_present_index(AspectRatio::Ratio16To9);

        assert!(html.contains(r#"<main id="peitho-present-root"></main>"#));
        assert!(html.contains(
            "html, body { margin: 0; width: 100%; height: 100%; background: #000; overflow: hidden; touch-action: pan-y; }"
        ));
        assert!(html.contains(
            "#peitho-present-root { position: fixed; inset: 0; overflow: hidden; background: #000; touch-action: pan-y; }"
        ));
        assert!(html.contains("installPresentationControls"));
        assert!(html.contains("installCanvasClickNavigation"));
        assert!(html.contains("installSwipeNavigation"));
        assert!(html.contains("installFullscreenShortcut"));
        assert!(html.contains("installCloseOnEscape(window)"));
        assert!(html.contains("fetchOk('notes.json')"));
        assert!(html.contains("await peitho.mountPresentShell({ root })"));
        assert!(html.contains("installKeyboardNavigation(window)"));
        assert!(html.contains("installSyncBridge(window, peitho.serverSyncChannelFactory())"));
        assert!(html.contains("typeof peitho.installSwapShortcut === 'function'"));
        assert!(html.contains("config != null && config.presenterOpen"));
        assert!(html.contains("peitho.installSwapShortcut(window)"));
        assert!(html.contains(
            r#""shell bundle does not provide installSwapShortcut; display swap disabled""#
        ));
        assert!(html.contains(r#"data-peitho-action="close""#));
        let controls_index = html
            .find("peitho.installPresentationControls({ root, window, document })")
            .unwrap();
        let mount_index = html
            .find("await peitho.mountPresentShell({ root })")
            .unwrap();
        let sync_index = html
            .find("peitho.installSyncBridge(window, peitho.serverSyncChannelFactory())")
            .unwrap();
        let swap_index = html.find("peitho.installSwapShortcut(window)").unwrap();
        assert!(controls_index < mount_index);
        assert!(mount_index < sync_index);
        assert!(sync_index < swap_index);
        assert!(!html.contains("peitho-presenter-link"));
        assert!(!html.contains(">Presenter view</a>"));
        assert!(!html.contains("fetchOk(slide.src)"));
    }

    #[test]
    fn present_index_fetches_present_config_and_mounts_time_tracker_conditionally() {
        let html = render_present_index(AspectRatio::Ratio16To9);

        assert!(html.contains("import * as peitho from './shell.js';"));
        assert!(html.contains("fetchOk('present.json')"));
        assert!(html.contains("typeof peitho.installTimeTracker === 'function'"));
        assert!(html.contains("peitho.installTimeTracker"));
        assert!(html.contains("!config.presenterOpen"));
        assert!(html.contains("rawPlannedDurationMs = shell.manifest?.plannedDurationMs ?? null"));
        assert!(html.contains("peitho.isValidDurationMs(rawPlannedDurationMs)"));
        assert!(html.contains(r#""Invalid plannedDurationMs in manifest.json""#));
        assert!(html.contains(
            r#""shell bundle does not provide installTimeTracker; time tracker disabled""#
        ));
        assert!(html.contains(r#"const configPromise = fetchOk('present.json')"#));
        assert!(html
            .contains(r#""failed to load present.json; time tracker and display swap disabled""#));
        assert!(html.contains("variant: 'present'"));
        assert!(html.contains(".peitho-time-tracker"));
        assert!(html.contains("z-index: 5"));
        assert!(!html.contains("transform: translateX(-50%)"));
        assert!(html.contains("transition: left 120ms linear, transform 120ms linear"));
        assert!(html.contains(concat!(
            r#".peitho-time-tracker [data-peitho-marker="rabbit"],"#,
            "\n",
            r#"    .peitho-time-tracker [data-peitho-marker="turtle"] { bottom: 8px; }"#
        )));
        assert!(!html.contains(r#"[data-peitho-marker="rabbit"] { bottom: 20px; }"#));
        assert!(!html.contains(r#"[data-peitho-marker="turtle"] { bottom: 2px; }"#));
        assert!(!html.contains(r#"[data-peitho-time-tracker="presenter"]"#));
        let mount_index = html
            .find("await peitho.mountPresentShell({ root })")
            .unwrap();
        let sync_index = html
            .find("peitho.installSyncBridge(window, peitho.serverSyncChannelFactory())")
            .unwrap();
        let config_fetch_index = html
            .find("const configPromise = fetchOk('present.json')")
            .unwrap();
        let config_await_index = html.find("const config = await configPromise").unwrap();
        assert!(config_fetch_index < mount_index);
        assert!(mount_index < sync_index);
        assert!(sync_index < config_await_index);
    }

    #[test]
    fn presenter_index_mounts_presenter_view_with_canvas_panes_and_notes() {
        let html = render_presenter_index(AspectRatio::Ratio16To9);

        assert!(html.contains(r#"<main id="peitho-presenter-root"></main>"#));
        assert!(html.contains("import * as peitho from './shell.js';"));
        assert!(html.contains("fetchOk('notes.json')"));
        assert!(html.contains("installCloseOnEscape(window)"));
        assert!(html.contains("typeof peitho.installSwapShortcut === 'function'"));
        assert!(html.contains("peitho.installSwapShortcut(window)"));
        assert!(html.contains(
            r#""shell bundle does not provide installSwapShortcut; display swap disabled""#
        ));
        assert!(html.contains("await peitho.mountPresenterView({"));
        assert!(html.contains("syncChannelFactory: peitho.serverSyncChannelFactory()"));
        assert!(html.contains(r#"data-peitho-action="close""#));
        assert!(html.contains(r#"data-peitho-action="playpause""#));
        assert!(html.contains("fonts.googleapis.com"));
        assert!(html.contains("family=Geist:wght@400;500;600;700"));
        assert!(html.contains("family=Geist+Mono:wght@400;500;600"));
        assert!(html.contains("display=swap"));
        assert!(html.contains(r#"--accent: oklch(78% 0.14 195)"#));
        assert!(html.contains(
            ".app { display: grid; grid-template-columns: minmax(0, 1.7fr) minmax(400px, 1fr);"
        ));
        assert!(html.contains(".slide-pane"));
        assert!(html.contains(".next-preview"));
        assert!(html.contains("--peitho-canvas-aspect: 16 / 9;"));
        assert!(html.contains(".stage { display: flex; flex-direction: column; gap: var(--stage-gap); height: 100%; min-width: 0; width: max(280px, min(100%, calc((100cqh - var(--colhead-h) - var(--kbdbar-h) - var(--notes-h) - 3 * var(--stage-gap)) * var(--peitho-canvas-aspect)))); }"));
        assert!(html.contains("aspect-ratio: var(--peitho-canvas-aspect);"));
        assert!(
            html.contains(".slide-pane { position: relative; width: 100%; box-sizing: border-box;")
        );
        assert!(html.contains(".notes { background: var(--bg-elev); border: 1px solid var(--line-soft); display: grid; grid-template-rows: auto minmax(0, 1fr); flex: 1 0 var(--notes-h); max-height: 42vh; overflow: hidden; }"));
        assert!(html.contains(".notes-body"));
        assert!(html.contains(".clock { display: flex; flex-direction: column;"));
        assert!(html.contains(".controls {"));
        assert!(html
            .contains("grid-template-columns: minmax(max-content, 1fr) auto auto auto auto auto"));
        assert!(html.contains("margin-top: auto"));
        assert!(html.contains(
            ".btn.primary .k { color: color-mix(in oklch, var(--bg) 60%, var(--accent)); }"
        ));
        assert!(html.contains(".btn.pressed::before"));
        assert!(html.contains("@media (prefers-reduced-motion: reduce)"));
        assert!(!html.contains("grid-layout-columns"));
        assert!(html.contains("overflow: hidden"));
        assert!(html.contains("Failed to load"));
        assert!(!html.contains("fetchOk(slide.src)"));
        assert!(!html.contains(".agenda"));
        assert!(!html.contains("Section —"));
        assert!(!html.contains("data-omelette-injected"));
        let mount_index = html.find("await peitho.mountPresenterView({").unwrap();
        let swap_index = html.find("peitho.installSwapShortcut(window)").unwrap();
        assert!(mount_index < swap_index);
    }

    #[test]
    fn presenter_index_uses_deck_aspect_ratio_css_variable() {
        let html = render_presenter_index(AspectRatio::Ratio4To3);

        assert!(html.contains("--peitho-canvas-aspect: 4 / 3;"));
        assert!(html.contains("aspect-ratio: var(--peitho-canvas-aspect);"));
        assert!(html.contains(
            "calc((100cqh - var(--colhead-h) - var(--kbdbar-h) - var(--notes-h) - 3 * var(--stage-gap)) * var(--peitho-canvas-aspect))"
        ));
        assert!(!html.contains("aspect-ratio: 16 / 9"));
    }

    #[test]
    fn present_index_emits_deck_aspect_ratio_css_variable() {
        let html = render_present_index(AspectRatio::Ratio4To3);

        assert!(html.contains("--peitho-canvas-width: 960px;"));
        assert!(html.contains("--peitho-canvas-height: 720px;"));
        assert!(html.contains("--peitho-canvas-aspect: 4 / 3;"));
    }

    #[test]
    fn presenter_index_includes_time_tracker_css() {
        let html = render_presenter_index(AspectRatio::Ratio16To9);

        assert!(html.contains(".peitho-time-tracker"));
        assert!(html.contains(r#"[data-peitho-time-tracker="presenter"]"#));
        assert!(html.contains(".tracker-wrap"));
        assert!(html.contains(".tracker-wrap:empty { display: none; }"));
        assert!(html.contains(".tracker-legend"));
        assert!(html.contains(".tracker { position: relative; height: 30px;"));
        assert!(html.contains(".tracker-fill"));
        assert!(html.contains(".tracker-scale"));
        assert!(html.contains(".tracker-scale { position: relative;"));
        assert!(html.contains(".tracker-scale span { position: absolute;"));
        assert!(!html.contains(".tracker-scale { display: grid;"));
        assert!(!html.contains("transform: translateX(-50%)"));
        assert!(html.contains("transition: left 120ms linear, transform 120ms linear"));
        assert!(html.contains(concat!(
            r#".tracker [data-peitho-marker="rabbit"],"#,
            "\n",
            r#"    .tracker [data-peitho-marker="turtle"] { position: absolute; transition: left 120ms linear, transform 120ms linear; font-size: 18px; line-height: 1; }"#
        )));
        assert!(html.contains(r#".tracker [data-peitho-marker="rabbit"] { top: -6px; }"#));
        assert!(html.contains(r#".tracker [data-peitho-marker="turtle"] { bottom: -6px; }"#));
        assert!(!html.contains(".mark"));
        assert!(html.contains(r#"[data-peitho-urgency="urgent"]"#));
        assert!(!html.contains(r#"[data-peitho-presenter="timer"][data-peitho-overrun]"#));
        assert!(!html.contains(".peitho-time-tracker { position: absolute"));
        assert!(!html.contains("bottom: 0; height: 6px"));
    }

    #[test]
    fn presenter_index_includes_agenda_css_with_data_selectors() {
        let html = render_presenter_index(AspectRatio::Ratio16To9);

        assert!(html.contains(r#"[data-peitho-agenda] { overflow: hidden;"#));
        assert!(html.contains(r#"[data-peitho-agenda-head]"#));
        assert!(html.contains(r#"[data-peitho-agenda-list]"#));
        assert!(html.contains(
            r#"[data-peitho-presenter="agenda-slot"] { min-height: 0; overflow: hidden; }"#
        ));
        assert!(html.contains(r#"[data-peitho-agenda-row]"#));
        assert!(html.contains("grid-template-columns: 10px minmax(0, 1fr) auto auto"));
        assert!(html.contains(r#"[data-peitho-agenda-row] + [data-peitho-agenda-row]"#));
        assert!(html.contains(r#"[data-peitho-agenda-marker]"#));
        assert!(html.contains(r#"[data-peitho-agenda-state="done"]"#));
        assert!(html.contains(r#"[data-peitho-agenda-state="current"]"#));
        assert!(html.contains(r#"[data-peitho-agenda-state="upcoming"]"#));
        assert!(html.contains(r#"[data-peitho-agenda-state="done"] [data-peitho-agenda-marker]"#));
        assert!(
            html.contains(r#"[data-peitho-agenda-state="current"] [data-peitho-agenda-marker]"#)
        );
        assert!(
            html.contains(r#"[data-peitho-agenda-state="upcoming"] [data-peitho-agenda-marker]"#)
        );
        assert!(html.contains(r#"[data-peitho-agenda-label]"#));
        assert!(html.contains(r#"[data-peitho-agenda-name]"#));
        assert!(html.contains(r#"[data-peitho-agenda-range]"#));
        assert!(html.contains("flex-shrink: 0"));
        assert!(html.contains(r#"[data-peitho-agenda-time]"#));
        assert!(html.contains(r#"[data-peitho-agenda-delta]"#));
        assert!(html.contains(r#"[data-peitho-agenda-state="done"] [data-peitho-agenda-name] { color: var(--fg-dim); }"#));
        assert!(html.contains("min-width: 6ch"));
        assert!(html
            .contains(r#"[data-peitho-agenda-state="done"][data-peitho-agenda-outcome="under"]"#));
        assert!(html.contains(
            r#"[data-peitho-agenda-state][data-peitho-agenda-outcome="over"] [data-peitho-agenda-time]"#
        ));
        assert!(html.contains(".clock { display: flex; flex-direction: column;"));
        assert!(
            html.contains(r#".clock[data-peitho-state="paused"] .timer { color: var(--fg-dim); }"#)
        );
        assert!(
            !html.contains(r#".clock[data-peitho-state="paused"] .timer { color: var(--pause); }"#)
        );
        assert!(html.contains(".controls {"));
        assert!(html.contains("margin-top: auto"));
        assert!(!html.contains(".agenda"));
    }

    #[test]
    fn distribution_index_does_not_include_time_tracker() {
        let html = render_distribution_index(AspectRatio::Ratio16To9);

        assert!(!html.contains("installTimeTracker"));
        assert!(!html.contains("peitho-time-tracker"));
        assert!(!html.contains("present.json"));
    }

    #[test]
    fn distribution_index_maps_escape_to_index_navigation() {
        let html = render_distribution_index(AspectRatio::Ratio16To9);

        assert!(html.contains("event.key === 'Escape'"));
        assert!(html.contains("function exitToIndex()"));
        assert!(html.contains("window.history.back()"));
        assert!(html.contains("window.location.assign('/')"));
        assert!(html.contains("new URL(referrer).origin === window.location.origin"));
    }

    #[test]
    fn present_index_uses_server_sync_factory() {
        let html = render_present_index(AspectRatio::Ratio16To9);

        assert!(html.contains("serverSyncChannelFactory"));
        assert!(html.contains("installSyncBridge(window, peitho.serverSyncChannelFactory())"));
        assert!(!html.contains("installSyncBridge(window);"));
    }

    #[test]
    fn preview_error_index_escapes_error_and_handshakes_before_polling() {
        let html = render_preview_error_index(3, "bad <deck> & broken");

        assert!(html.contains("bad &lt;deck&gt; &amp; broken"));
        assert!(html.contains("const baselineGeneration = 3;"));
        assert!(html.contains("fetch('/sync')"));
        assert!(html.contains("fetch(`/sync?seq=${seq}`)"));
        assert!(html.contains("body.generation !== baselineGeneration"));
        assert!(!html.contains("seq=now"));
    }

    #[test]
    fn presenter_index_passes_server_sync_factory_to_presenter_view() {
        let html = render_presenter_index(AspectRatio::Ratio16To9);

        assert!(html.contains("serverSyncChannelFactory"));
        assert!(html.contains("syncChannelFactory: peitho.serverSyncChannelFactory()"));
    }

    #[test]
    fn present_index_has_no_static_presenter_link() {
        let html = render_present_index(AspectRatio::Ratio16To9);

        assert!(html.contains(r#"<main id="peitho-present-root"></main>"#));
        assert!(!html.contains("peitho-presenter-link"));
        assert!(!html.contains(">Presenter view</a>"));
        assert!(!html.contains("mountPresenterView"));
    }

    #[test]
    fn present_index_keeps_controls_default_display_management_before_mount() {
        let html = render_present_index(AspectRatio::Ratio16To9);

        let controls_index = html
            .find("peitho.installPresentationControls({ root, window, document })")
            .unwrap();
        let mount_index = html
            .find("await peitho.mountPresentShell({ root })")
            .unwrap();
        assert!(controls_index < mount_index);
        assert!(html.contains("peitho.installPresentationControls({ root, window, document })"));
        assert!(!html.contains("openPresenter"));
    }

    #[test]
    fn pdf_document_inlines_all_slides_with_print_page_css() {
        let rendered =
            render_checked_deck("---\nresolution: 1920x1080\n---\n# Intro\n\n---\n# Details");

        let html = render_pdf_document(&rendered);

        assert!(html.contains(r#"<!doctype html>"#));
        assert!(html.contains(r#"<link rel="stylesheet" href="peitho.css">"#));
        assert!(html.contains("@page { size: 1920px 1080px; margin: 0; }"));
        assert!(html.contains("--peitho-canvas-width: 1280px;"));
        assert!(html.contains("--peitho-canvas-height: 720px;"));
        assert!(html.contains(".peitho-slide-wrap { width: 1920px; height: 1080px;"));
        assert!(html.contains("page-break-after: always;"));
        assert!(html.contains("break-after: page;"));
        assert!(html.contains("transform: scale(1.5);"));
        assert!(html.contains(r#"data-slide-key="intro""#));
        assert!(html.contains(r#"data-slide-key="details""#));
        assert!(html.contains(">Intro</span>"));
        assert!(html.contains(">Details</span>"));
        assert!(!html.contains("fetch("));
        assert!(!html.contains("manifest.json"));
        assert!(!html.contains("notes.json"));
    }

    #[test]
    fn pdf_document_embeds_pdf_flattening_script_after_slides() {
        let rendered = render_checked_deck("# Intro");

        let html = render_pdf_document(&rendered);

        let slide_index = html.find(r#"data-slide-key="intro""#).unwrap();
        let script_index = html.find("<script>").unwrap();
        assert!(script_index > slide_index);
        assert!(html.contains("flattenGradients"));
        assert!(html.contains("flattenBoxShadows"));
    }

    #[test]
    fn pdf_flatten_script_cannot_close_its_embedding_script_tag() {
        assert!(!PDF_FLATTEN_JS.to_ascii_lowercase().contains("</script"));
    }

    #[test]
    fn pdf_document_uses_four_by_three_resolution_and_scale() {
        let rendered = render_checked_deck("---\naspect_ratio: 4:3\n---\n# Intro");

        let html = render_pdf_document(&rendered);

        assert!(html.contains("@page { size: 1440px 1080px; margin: 0; }"));
        assert!(html.contains("--peitho-canvas-width: 960px;"));
        assert!(html.contains("--peitho-canvas-height: 720px;"));
        assert!(html.contains(".peitho-slide-wrap { width: 1440px; height: 1080px;"));
        assert!(html.contains("transform: scale(1.5);"));
    }

    #[test]
    fn pdf_document_does_not_include_speaker_notes() {
        let rendered = render_checked_deck("# Intro\n\n<!-- speaker secret -->");

        assert_eq!(rendered.slides()[0].notes(), Some("speaker secret"));

        let html = render_pdf_document(&rendered);

        assert!(!html.contains("speaker secret"));
    }

    fn render_checked_deck(markdown: &str) -> Deck<Rendered> {
        let layout = parse_layout(
            "title-only",
            r#"<section><h1><slot name="title" accepts="inline" arity="1"></slot></h1></section>"#,
        )
        .unwrap();
        let checked = check_deck(
            map_by_convention(
                parse_markdown(markdown, &crate::highlight::Highlighter::defaults()).unwrap(),
                &layout,
            )
            .unwrap(),
        )
        .unwrap();
        render_checked(checked)
    }

    fn render_checked(checked: Deck<Checked>) -> Deck<Rendered> {
        let (resolved, assets) = crate::phase::resolve_image_paths(checked, |request| {
            panic!(
                "unexpected image resolver call for {} on slide {}",
                request.raw.as_str(),
                request.slide_key.as_str()
            )
        })
        .unwrap();
        assert!(assets.is_empty());
        render_deck(resolved, &crate::highlight::Highlighter::defaults()).unwrap()
    }
}
