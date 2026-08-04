# Tweet embeds via build-time screenshots

<!-- constrained-by ../specs/2026-08-04-tweet-embeds-design.md -->

Issue: #395  
Date: 2026-08-04

This plan follows the approved design, the resolver invariant in
[`CLAUDE.md`](../../CLAUDE.md), and the existing
[`code_images` pipeline](./2026-07-12-issue-241-code-images.md) /
[`BuiltinMath` narrowing](./2026-07-18-builtin-math.md). v1 accepts one X
status URL in a bare `embed` fence, snapshots the official widget to PNG at
build time, and then uses the ordinary image pipeline. No script, wrapper
HTML, cache directory, or provider-specific fragment reaches output.

Fixed contracts used by every task:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbedTheme {
    Light,
    Dark,
}

impl EmbedTheme {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmbedRenderParams {
    pub width_css_px: u32,
    pub scale_factor: u32,
    pub theme: EmbedTheme,
}

impl EmbedRenderParams {
    pub(crate) const fn new(width_css_px: u32, scale_factor: u32, theme: EmbedTheme) -> Self {
        Self { width_css_px, scale_factor, theme }
    }
}

pub const BUILTIN_EMBED_PARAMS: EmbedRenderParams =
    EmbedRenderParams::new(550, 2, EmbedTheme::Light);

pub trait EmbedRenderer {
    fn render(
        &self,
        normalized_url: &str,
        params: EmbedRenderParams,
    ) -> crate::Result<Vec<u8>>;
}
```

`Dark` exists only so theme is typed and cache-key coverage is testable; v1
always passes `Light` and exposes no option syntax. The implementation adds no
crate dependency, no `FragmentKind`, no TypeScript contract, and no
README/site/example work.

## Task 1: Parse and normalize one X status URL

**Goal**: Turn the body of a built-in `embed` block into a canonical X URL,
with every rejected shape reported as an `ErrorKind::Asset` `BuildError` at the
fence line with actionable help.

**Files**: `crates/peitho-core/src/code_images.rs`

**Test**: Add the positive test and one distinct test function per error case.

```rust
#[test]
fn embed_block_accepts_x_and_twitter_status_urls() {
    let x = parse_embed_block(7, "\n  https://x.com/Gosukenator/status/2083825695709597710  \n")
        .unwrap();
    let twitter = parse_embed_block(
        7,
        "https://twitter.com/gosukenator/status/2083825695709597710\n",
    )
    .unwrap();
    assert_eq!(
        x.normalized_url(),
        "https://x.com/gosukenator/status/2083825695709597710"
    );
    assert_eq!(x, twitter);
}

#[test]
fn embed_block_accepts_case_insensitive_scheme_and_host() {
    let canonical = parse_embed_block(7, "https://x.com/a/status/1").unwrap();
    assert_eq!(parse_embed_block(7, "https://X.com/A/status/1").unwrap(), canonical);
    assert_eq!(parse_embed_block(7, "HTTPS://twitter.com/a/status/1").unwrap(), canonical);
}

fn assert_embed_error(body: &str, message: &str) {
    let err = parse_embed_block(7, body).unwrap_err();
    assert_eq!(err.kind, ErrorKind::Asset);
    assert_eq!(err.line, Some(7));
    assert!(err.message.contains(message), "{}", err.message);
    assert!(err.help.contains("https://x.com/<user>/status/<id>"));
    assert!(err.help.contains("https://twitter.com/<user>/status/<id>"));
}

#[test]
fn embed_block_rejects_empty_body() {
    assert_embed_error("\n \n", "empty");
}

#[test]
fn embed_block_rejects_multiple_non_blank_lines() {
    assert_embed_error(
        "https://x.com/a/status/1\nhttps://x.com/b/status/2\n",
        "exactly one non-blank line",
    );
}

#[test]
fn embed_block_rejects_surrounding_junk() {
    assert_embed_error("see https://x.com/a/status/1", "supported X status URL");
}

#[test]
fn embed_block_rejects_non_https_url() {
    assert_embed_error("http://x.com/a/status/1", "supported X status URL");
}

#[test]
fn embed_block_rejects_non_x_provider() {
    assert_embed_error("https://example.com/a/status/1", "supported X status URL");
}

#[test]
fn embed_block_rejects_malformed_status_path() {
    assert_embed_error("https://x.com/a/status/1/", "supported X status URL");
}

#[test]
fn embed_block_rejects_non_numeric_status_id() {
    assert_embed_error("https://x.com/a/status/not-a-number", "supported X status URL");
}

#[test]
fn embed_block_rejects_query() {
    assert_embed_error("https://x.com/a/status/1?s=20", "supported X status URL");
}

#[test]
fn embed_block_rejects_fragment() {
    assert_embed_error("https://x.com/a/status/1#quoted", "supported X status URL");
}

#[test]
fn embed_block_rejects_percent_encoding() {
    assert_embed_error("https://x.com/a%20b/status/1", "supported X status URL");
}
```

First run `cargo test -p peitho-core embed_block`; it must fail because
`parse_embed_block` does not exist.

**Implementation**: Add a private, `Debug + Clone + PartialEq + Eq`
`EmbedTarget::X(TweetStatusUrl { normalized_url, user, status_id })`. Trim each
line, discard blank lines, require exactly one remaining line, then send it
through `parse_embed_url`, the single URL→provider dispatcher. Its only v1
arm recognizes the `https://x.com/` and `https://twitter.com/` prefixes with
ASCII-case-insensitive scheme/host matching, then delegates the untouched,
case-sensitive path to the X parser; another host produces the
provider-follow-up error.
The X parser requires three path segments:
`<user>/status/<id>`. Require a nonempty ASCII alphanumeric/underscore user
and a nonempty ASCII-decimal ID; reject trailing slash, query, fragment,
percent escapes, extra segments, and text around the URL. Canonicalize both
hosts to `x.com` and lowercase the user. Every failure uses the same help text
naming both supported forms and stating that other providers belong to the
generic-oEmbed follow-up. Delegate `normalized_url()` and `user()` through
`EmbedTarget`; a future provider adds an enum arm instead of changing the
transform contract. Construct errors through `code_image_error(line,
"embed", message, embed_url_help())` so their prefix matches neighboring
built-ins.

**Verification**: `cargo test -p peitho-core embed_block` passes.

## Task 2: Freeze render parameters, cache identity, and raw path

**Goal**: Give every output-affecting parameter a typed cache-key input and
make cached PNGs constructible without weakening author-supplied image-path
validation.

**Files**: `crates/peitho-core/src/code_images.rs`,
`crates/peitho-core/src/domain.rs`, `crates/peitho-core/src/lib.rs`,
`crates/peitho-core/tests/support/embed_cache_key.rs` (new)

**Test**: Pin the complete byte stream and each parameter discriminator.

```rust
mod embed_cache_key_test_vector {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/support/embed_cache_key.rs"
    ));
}

#[test]
fn embed_cache_key_covers_url_width_scale_and_theme_without_crate_version() {
    let status = parse_embed_block(
        7,
        "https://x.com/gosukenator/status/2083825695709597710",
    )
    .unwrap();
    let base = EmbedRenderParams::new(550, 2, EmbedTheme::Light);
    assert_eq!(
        builtin_embed_cache_key(&status, base),
        embed_cache_key_test_vector::PINNED_BUILTIN_EMBED_CACHE_KEY
    );
    assert_ne!(builtin_embed_cache_key(&status, base), builtin_embed_cache_key(&status, EmbedRenderParams::new(551, 2, EmbedTheme::Light)));
    assert_ne!(builtin_embed_cache_key(&status, base), builtin_embed_cache_key(&status, EmbedRenderParams::new(550, 1, EmbedTheme::Light)));
    assert_ne!(builtin_embed_cache_key(&status, base), builtin_embed_cache_key(&status, EmbedRenderParams::new(550, 2, EmbedTheme::Dark)));
}

#[test]
fn embed_cache_raw_path_uses_png_cache_namespace() {
    let raw = RawImagePath::from_embeds_cache("abc123");
    assert_eq!(raw.as_str(), ".peitho/embeds-cache/abc123.png");
}
```

First run `cargo test -p peitho-core embed_cache`; it must fail on the missing
types, key function, constant, and constructor.

The pinned key is a compute → pin test vector, not a value chosen in this
plan. After the minimal key implementation makes the parameter-discriminator
assertions green, temporarily print its base-case result, run this focused
test once, copy the resulting 64-character lowercase hex value into
`PINNED_BUILTIN_EMBED_CACHE_KEY` in the shared test-support file, remove the
print, and rerun the equality assertion. Task 9 includes this same support
file rather than restating the captured value.

**Implementation**: Add the types shown above plus
`EMBEDS_CACHE_DIR: &str = ".peitho/embeds-cache"` next to
`CODE_IMAGES_CACHE_DIR`. Hash this exact byte sequence with SHA-256:
`\0peitho-builtin-embed\0`, canonical URL, `\0width=<decimal>`,
`\0scale=<decimal>`, `\0theme=<light|dark>`. Do not hash
`CARGO_PKG_VERSION`. Add `pub(crate) RawImagePath::from_embeds_cache`, which
formats `{EMBEDS_CACHE_DIR}/{key}.png`; leave `RawImagePath::new` and remote-URL
rejection unchanged.

**Verification**: `cargo test -p peitho-core embed_cache` passes.

## Task 3: Render a cache miss and write it atomically

**Goal**: Keep `peitho-core` pure by injecting PNG rendering, and place a
successful miss in the embed cache through the existing atomic writer.

**Files**: `crates/peitho-core/src/code_images.rs`

**Test**: Use a fixture renderer and assert both the call contract and the
filesystem end state.

```rust
#[test]
fn embed_cache_miss_renders_once_and_writes_atomically() {
    let temp = tempfile::tempdir().unwrap();
    let cache_dir = temp.path().join(crate::EMBEDS_CACHE_DIR);
    let renderer = FixtureEmbedRenderer::png(b"\x89PNG\r\n\x1a\nfixture".to_vec());
    let status = parse_embed_block(7, "https://x.com/a/status/1").unwrap();
    let key = cache_or_render_embed(
        7,
        &status,
        BUILTIN_EMBED_PARAMS,
        &renderer,
        &cache_dir,
    )
    .unwrap();

    assert_eq!(renderer.calls(), 1);
    assert_eq!(renderer.urls(), vec![status.normalized_url().to_owned()]);
    assert_eq!(renderer.params(), vec![BUILTIN_EMBED_PARAMS]);
    assert_eq!(fs::read(cache_dir.join(format!("{key}.png"))).unwrap(), b"\x89PNG\r\n\x1a\nfixture");
    assert_eq!(fs::read_dir(cache_dir).unwrap().count(), 1);
}
```

First run `cargo test -p peitho-core embed_cache_miss`; it must fail before
the renderer seam and cache function exist.

**Implementation**: Add `EmbedRenderer` with the fixed signature and a test
fixture that records URL/parameters. Add `cache_or_render_embed`: create the
cache directory, derive `{key}.png`, call the renderer only on a miss, require
PNG magic, and call the existing `write_cache_file_atomic` for the final
write. Return the key only after the final file exists. Do not duplicate the
temp-name counter, create-new open, flush, rename, or failed-temp cleanup.

**Verification**: `cargo test -p peitho-core embed_cache_miss` passes.

## Task 4: Make cache hits offline-safe and corrupt entries self-healing

**Goal**: A valid PNG hit performs no render work; empty and non-PNG entries
are misses that are replaced atomically.

**Files**: `crates/peitho-core/src/code_images.rs`

**Test**: Add separate hit and corruption tests.

```rust
#[test]
fn embed_valid_cache_hit_skips_renderer() {
    let renderer = PanicEmbedRenderer;
    let key = seed_embed_cache(b"\x89PNG\r\n\x1a\ncached");
    assert_eq!(cache_or_render_seeded_embed(&renderer).unwrap(), key);
}

#[test]
fn embed_cache_entry_empty_is_rerendered() {
    assert_embed_cache_self_heals(b"");
}

#[test]
fn embed_cache_entry_non_png_is_rerendered() {
    assert_embed_cache_self_heals(b"not a png");
}
```

`assert_embed_cache_self_heals` seeds the computed path, supplies a fixture
PNG renderer, asserts one call, and asserts the old bytes were replaced. Run
`cargo test -p peitho-core embed_valid_cache_hit` and
`cargo test -p peitho-core embed_cache_entry` separately; both must fail
before validity is checked.

**Implementation**: Add `valid_cached_png(&Path) -> bool`, mirroring
`valid_cached_svg`: require a regular file with length at least eight and
bytes beginning `89 50 4e 47 0d 0a 1a 0a`; I/O errors are misses. Perform this
check before `EmbedRenderer::render`. A corrupt path stays in place until a
new valid PNG has been produced, then the atomic rename replaces it. Do not
inspect mtime, add a TTL, or auto-refresh a valid snapshot.

**Verification**:

```text
cargo test -p peitho-core embed_valid_cache_hit
cargo test -p peitho-core embed_cache_entry
```

## Task 5: Report every miss failure at the fence line

**Goal**: Renderer, Chrome/network, invalid-output, directory, and write
failures are never silent and always explain the cache-only offline behavior.

**Files**: `crates/peitho-core/src/code_images.rs`

**Test**: Add one test per failure class; the renderer-error case pins the
offline/Chrome-less miss contract.

```rust
#[test]
fn embed_renderer_error_names_line_url_and_cache_path() {
    let err = cache_embed_with_renderer_error("Chrome not found").unwrap_err();
    assert_eq!(err.kind, ErrorKind::Asset);
    assert_eq!(err.line, Some(7));
    assert!(err.message.contains("https://x.com/a/status/1"));
    assert!(err.message.contains("Chrome not found"));
    assert!(err.help.contains(".peitho/embeds-cache/"));
    assert!(err.help.contains("cache hit works offline"));
    assert!(err.help.contains("delete the cache file to refresh"));
}

#[test]
fn embed_invalid_renderer_output_empty_is_line_numbered() {
    assert_invalid_embed_png(b"", "empty PNG output");
}

#[test]
fn embed_invalid_renderer_output_non_png_is_line_numbered() {
    assert_invalid_embed_png(b"<html>blocked</html>", "not PNG output");
}

#[test]
fn embed_cache_directory_failure_is_line_numbered() {
    assert_embed_cache_path_collision_is_asset_error_at_line(7);
}
```

Implement `assert_embed_cache_path_collision_is_asset_error_at_line`
cross-platform by creating the `.peitho` parent directory, writing a regular
file at the full `.peitho/embeds-cache` directory path, and then invoking the
cache path. `fs::create_dir_all` must fail because the requested directory is
already a file; assert that failure becomes an Asset error at line 7. Do not
use Unix permission bits to induce the failure.

First run `cargo test -p peitho-core embed_renderer_error`; it must fail until
miss errors are wrapped at the transform boundary.

**Implementation**: Wrap every renderer `BuildError` in a new Asset error
with the original fence line, canonical URL, and computed cache path. The help
must say that a valid cache hit needs neither Chrome nor network, that a miss
requires Chrome plus access to X, and that deleting the named file refreshes
the snapshot; append the renderer's original install/path/retry help. Give
empty bytes and wrong magic distinct messages. Map
`create_dir_all` and `write_cache_file_atomic` errors through the same helper;
preserve their I/O text and tell the author to make the deck's `.peitho`
directory writable. Never install an `_ => {}` arm.

**Verification**: `cargo test -p peitho-core embed_renderer_error`,
`cargo test -p peitho-core embed_invalid`, and
`cargo test -p peitho-core embed_cache_directory_failure` pass.

## Task 6: Generate the official-widget wrapper as a pure function

**Goal**: Produce measure and capture variants of a temporary 550-CSS-pixel
wrapper, with successful rendering gated by the official widget's `rendered`
event and measured height published through `document.title`.

**Files**: `crates/peitho/src/main.rs`

**Test**:

```rust
#[test]
fn embed_wrapper_html_splits_measure_failure_release_from_strict_capture() {
    let measure = embed_wrapper_html(
        "https://x.com/gosukenator/status/2083825695709597710",
        BUILTIN_EMBED_PARAMS,
        EmbedWrapperMode::Measure,
    );
    let capture = embed_wrapper_html(
        "https://x.com/gosukenator/status/2083825695709597710",
        BUILTIN_EMBED_PARAMS,
        EmbedWrapperMode::Capture,
    );
    for html in [&measure, &capture] {
        assert!(html.contains(r#"<iframe id="peitho-load-holder""#));
        assert!(html.contains("twttr.events.bind(\"rendered\""));
        assert!(html.contains("finally {\n      releaseLoad();\n    }"));
        assert!(html.contains("holder.contentDocument.close()"));
    }
    assert!(measure.contains("js.onerror = releaseLoad;"));
    assert!(measure.contains("setTimeout(releaseLoad, 15000);"));
    assert!(!capture.contains("js.onerror = releaseLoad;"));
    assert!(!capture.contains("setTimeout(releaseLoad, 15000);"));
}
```

First run `cargo test -p peitho embed_wrapper_html`; it must fail on the
missing function.

**Implementation**: Emit `<!doctype html>`, a white, marginless, overflow-hidden
550px page, one `<blockquote class="twitter-tweet">` containing an anchor to
the canonical URL, and the official `platform.x.com/widgets.js` loader. Set
the initial title to `peitho-embed-pending`. Inside `twttr.ready`, bind
`twttr.events` `rendered`; locate the rendered iframe, compute
`Math.ceil(getBoundingClientRect().height)`, and set
`document.title = "peitho-embed-height:" + height` only for a positive finite
height. `EmbedWrapperMode::{Measure,Capture}` both add a hidden zero-size load
holder iframe, open/write its child document before loading `widgets.js`, and
release it from the `rendered` handler's `finally` path. Measure additionally
sets `js.onerror = releaseLoad` and a 15-second `setTimeout(releaseLoad, 15000)`;
both failure paths leave the pending title so dump-dom completes and the
specific network/public-post error is returned. Capture deliberately has
neither fallback: releasing before `rendered` would cache a valid-but-blank
PNG, while a named 60-second timeout is safe. Exclude the holder from the
tweet-iframe selector. Attribute interpolation remains safe only under Task
1's strict URL grammar; loosening it must add escaping. Do not call
`image.decode()` or emit the wrapper.

**Verification**: `cargo test -p peitho embed_wrapper_html` passes.

## Task 7: Implement two-pass Chrome rendering through the one-shot runner

**Goal**: Measure widget height, capture a 2x PNG, and make every non-Chrome
piece unit-testable without launching a browser.

**Files**: `crates/peitho/src/main.rs`

**Test**: Pin title parsing, both argument vectors, and orchestration with a
fake invoker.

```rust
#[test]
fn embed_chrome_orchestration_measures_then_captures() {
    let temp = tempfile::tempdir().unwrap();
    let mut invocations = Vec::new();
    let png = render_embed_with_invoker(
        temp.path(),
        "https://x.com/a/status/1",
        BUILTIN_EMBED_PARAMS,
        |args, completion| {
            invocations.push(args.to_vec());
            fake_embed_chrome_output(args, completion, 742, b"\x89PNG\r\n\x1a\nfixture")
        },
    )
    .unwrap();
    assert_eq!(png, b"\x89PNG\r\n\x1a\nfixture");
    assert_eq!(invocations.len(), 2);
    assert!(has_arg(&invocations[0], "--dump-dom"));
    assert!(has_arg(&invocations[1], "--window-size=550,742"));
    assert!(has_arg(&invocations[1], "--force-device-scale-factor=2"));
    assert!(!has_arg_prefix(&invocations[0], "--virtual-time-budget"));
    assert!(!has_arg_prefix(&invocations[1], "--virtual-time-budget"));
    assert!(invocations[0].last().unwrap().to_string_lossy().ends_with("embed-measure.html"));
    assert!(invocations[1].last().unwrap().to_string_lossy().ends_with("embed-capture.html"));
    assert_ne!(user_data_dir(&invocations[0]), user_data_dir(&invocations[1]));
}

#[test]
fn embed_chrome_height_parser_enforces_exclusive_measurement_ceiling() {
    assert_eq!(parse_embed_height(b"<title>peitho-embed-height:9999</title>").unwrap(), 9999);
    assert!(parse_embed_height(b"<title>peitho-embed-pending</title>").is_err());
    assert!(parse_embed_height(b"<title>peitho-embed-height:0</title>").is_err());
    let err = parse_embed_height(b"<title>peitho-embed-height:10000</title>").unwrap_err();
    assert!(err.to_string().contains("reaches the 10000px measurement viewport"));
}
```

First run `cargo test -p peitho embed_chrome`; it must fail before the pure
argument/parsing/orchestration functions exist.

**Implementation**: Add pure `embed_measure_args`, `embed_capture_args`, and
`parse_embed_height` functions; accept heights `1..10_000`. A height at or
above the staging viewport is the distinct error `rendered embed height {h}
reaches the 10000px measurement viewport; the post is too tall to embed`.
Both argument sets include `--headless=new`, `--disable-gpu`, `--no-sandbox`, a throwaway
`--user-data-dir`, and the wrapper's `file://` URL. Neither pass may include a
`--virtual-time-budget` argument. Measurement adds `--dump-dom`, a 550px-wide
staging window, and scale 2; capture adds `--screenshot=<temp PNG>`,
`--window-size=550,<measured>`, and scale 2. Use distinct temp profiles for the
two passes and `CHROME_ONE_SHOT_TIMEOUT` for each.

Extend `ChromeCompletion` with exhaustive `EmbedMeasured` and `PngWritten`
arms. Keep `ChromeOutput.stdout` in non-test builds so the first pass can read
the title. Measurement completes while running only when stdout contains the
height prefix; a successful exit returns pending-title stdout to
`parse_embed_height` for the specific network/public-post diagnostic.
`PngWritten` completes on Chrome's `bytes written to file` needle alone
because that signal follows the write; read the file afterward so zero bytes
reach core's existing line-numbered `empty PNG output` error.
`render_embed_with_chrome` supplies an invoker that calls
`run_one_shot_chrome`, whose only process path is
`run_child_with_timeout`; never use `Command::output()`. Implement
`CliEmbedRenderer::render` by locating Chrome lazily inside the method,
creating a temp directory, writing the wrapper, running both passes, and
returning file bytes as an Asset error with no line. Core adds line/cache
context in Task 5.

**Measured Chrome pitfall**: `--virtual-time-budget=10000` expires while the X
iframe is still hidden at 0×0 and the title remains `peitho-embed-pending`;
raising the budget to 120000 can run for more than 90 seconds of wall time
because a pending resource stalls virtual time. Virtual time is therefore
unusable in both passes. Wall-clock headless Chrome renders the widget in
roughly 2–3 seconds, but plain `--dump-dom` and `--screenshot` run at the
parent page's load event, before the widget renders, yielding the raw “View
post on X” blockquote. Holding a child iframe document open until the widget's
`rendered` event gates that load event: measured `--dump-dom` then returned a
`<title>peitho-embed-height:321</title>` after roughly 3 seconds, and measured
`--screenshot` returned the complete official widget after roughly 3 seconds,
without virtual time. Chrome may linger after reporting bytes written; the
existing one-shot completion and kill path owns that case.

**Accepted residual risk**: The widget's `rendered` event can fire while an
inner-iframe subresource still races the screenshot. The parent load also
waits for the tweet iframe itself, so capture is gated on both the official
render signal and the iframe load. A deleted/blocked post or script failure is
rejected during measure via `onerror` or the 15-second fallback. Capture has
no non-rendered release; only a transient failure between passes pays the
named 60-second timeout instead of caching a blank PNG. v1 adds no pixel
inspection or image decoder for the remaining inner-resource race. Its
symptom is a partial cached tweet image; delete that block's named
`.peitho/embeds-cache/<key>.png` file and rebuild with network access to
refresh it. Do not add image decoding to close this residual risk.

**Verification**: `cargo test -p peitho embed_chrome` passes without Chrome.

## Task 8: Thread the renderer and cache dependencies through the pipeline

**Goal**: Grow the transform API and update every caller before enabling the
`embed` tag, so the final resolver change has no unrelated signature churn.

**Files**: `crates/peitho-core/src/code_images.rs`,
`crates/peitho-core/src/manifest.rs`,
`crates/peitho-core/tests/code_images.rs`, `crates/peitho/src/main.rs`

**Test**: Add a panic renderer to a plain-deck test and call the new signature.

```rust
struct PanicEmbedRenderer;

impl EmbedRenderer for PanicEmbedRenderer {
    fn render(&self, _url: &str, _params: EmbedRenderParams) -> Result<Vec<u8>> {
        panic!("a deck without embed blocks must not render an embed");
    }
}

#[test]
fn threads_embed_renderer_without_calling_it_for_plain_deck() {
    let temp = tempfile::tempdir().unwrap();
    let markdown = "# Plain\n\nParagraph\n";
    let deck = parse_deck_and_transform(
        markdown,
        parse_frontmatter(markdown).unwrap(),
        &Highlighter::defaults(),
        &NoSvgRunner,
        &PanicEmbedRenderer,
        &temp.path().join(crate::CODE_IMAGES_CACHE_DIR),
        &temp.path().join(crate::EMBEDS_CACHE_DIR),
    )
    .unwrap();
    assert_eq!(deck.slide_count(), 1);
}
```

First run `cargo test -p peitho-core threads_embed_renderer`; it must fail to
compile against the old entry-point signature.

**Implementation**: Change the core entry point to this exact shape, and add
the same `E: EmbedRenderer`, `embed_renderer`, and `embeds_cache_dir` inputs to
`transform_code_images` and recursive `transform_fragment` calls:

```rust
pub fn parse_deck_and_transform<S: SvgRunner, E: EmbedRenderer>(
    source: &str,
    frontmatter: ParsedFrontmatter,
    highlighter: &Highlighter,
    svg_runner: &S,
    embed_renderer: &E,
    code_images_cache_dir: &Path,
    embeds_cache_dir: &Path,
) -> Result<Deck<Parsed>>;
```

Do not add resolver behavior in this task. Update both
`crates/peitho/src/main.rs` call sites (`cmd_layouts --explain` and
`build_artifacts`), the manifest helper in
`crates/peitho-core/src/manifest.rs`, the integration test in
`crates/peitho-core/tests/code_images.rs`, and every direct
`transform_code_images` call in `crates/peitho-core/src/code_images.rs` tests.
CLI call sites pass `CliEmbedRenderer` plus
`deck_parent.join(EMBEDS_CACHE_DIR)`; core tests pass a fixture or panic
renderer and a separate temp embed-cache path.

**Verification**: `cargo test -p peitho-core threads_embed_renderer` passes,
then `cargo check -p peitho --bin peitho` compiles both updated CLI call sites.

## Task 9: Enable the resolver-backed vertical slice

**Goal**: Make bare `embed` fences become cached PNG image fragments through
every CLI entry point while explicit `code_images.embed:` remains the external
SVG override.

**Files**: `crates/peitho-core/src/domain.rs`,
`crates/peitho-core/src/parser.rs`,
`crates/peitho-core/src/code_images.rs`,
`crates/peitho-core/tests/code_images.rs`,
`crates/peitho/tests/tweet_embeds.rs` (new), `CLAUDE.md`

**Test**: Add all tests before the resolver arm and call-site wiring.

```rust
#[test]
fn resolver_selects_builtin_embed_but_explicit_override_wins() {
    let empty = CodeImagesConfig::default();
    assert_eq!(empty.renderer_for("embed"), Some(CodeImageRenderer::BuiltinEmbed));

    let command = CodeImageCommand { argv: vec!["embed-to-svg".to_owned()] };
    let configured = CodeImagesConfig {
        entries: BTreeMap::from([("embed".to_owned(), command.clone())]),
        key_line: Some(2),
    };
    assert_eq!(configured.renderer_for("embed"), Some(CodeImageRenderer::External(&command)));
}

#[test]
fn parser_accepts_bare_embed_and_rejects_its_line_emphasis() {
    parse_markdown("# T\n\n```embed\nhttps://x.com/a/status/1\n```", &highlighter()).unwrap();
    let err = parse_markdown("# T\n\n```embed {1}\nhttps://x.com/a/status/1\n```", &highlighter()).unwrap_err();
    assert_eq!(err.kind, ErrorKind::Parse);
    assert_eq!(err.line, Some(3));
    assert!(err.message.contains("line emphasis is not supported"));
}

#[test]
fn builtin_embed_becomes_png_image_and_preserves_reveal_span() {
    let transformed = transform_embed_fixture("https://twitter.com/A/status/1").unwrap();
    let fragment = &transformed.parsed_slides()[0].fragments[0];
    assert_eq!(fragment.line(), 7);
    assert_eq!(fragment.reveal_span(), Some(RevealSpan { start: 1, len: 1 }));
    match fragment.kind() {
        FragmentKind::Image { alt, src } => {
            assert_eq!(alt, "X post by @a");
            assert!(src.as_str().starts_with(".peitho/embeds-cache/"));
            assert!(src.as_str().ends_with(".png"));
        }
        kind => panic!("expected image, got {kind:?}"),
    }
}

#[test]
fn explicit_embed_override_uses_svg_runner_without_url_validation() {
    let result = transform_explicit_embed_override("plain external input").unwrap();
    assert_eq!(result.svg_calls(), 1);
    assert_eq!(result.embed_calls(), 0);
    assert!(result.image_src().starts_with(".peitho/code-images-cache/"));
    assert!(result.image_src().ends_with(".svg"));
}
```

In `crates/peitho-core/tests/code_images.rs`, add a fixture-renderer test that
runs parse → transform → dispatch → check → image resolution → render, reads
the one generated PNG asset's filename, and asserts the slide uses that exact
`assets/` filename in `<img>` with `alt="X post by @a"`.
In `crates/peitho/tests/tweet_embeds.rs`, add three CLI tests:

1. Include Task 2's shared `embed_cache_key.rs` test-support module, build a
   deck containing `https://x.com/gosukenator/status/2083825695709597710`,
   seed `embeds_cache_dir.join(format!("{}.png",
   PINNED_BUILTIN_EMBED_CACHE_KEY))`, point `PEITHO_CHROME_PATH` at a missing
   file, and assert `peitho build` succeeds,
   emits one PNG under `dist/assets/`, and emits no `platform.x.com`,
   `widgets.js`, or `twitter-tweet` text in output HTML. Assert `dist/.peitho`
   does not exist.
2. Leave that cache absent with the same missing Chrome path and assert
   failure names the fence line, normalized URL, exact cache path,
   `Chrome not found`, `PEITHO_CHROME_PATH`, and refresh help.
3. Mark `build_renders_official_tweet_embed_png` with `#[ignore]`; use
   `util::test_chrome_path`, a fresh cache, and the approved-design status URL.
   Assert the cache and dist asset start with PNG magic and the slide contains
   only the ordinary image reference.

Run `cargo test -p peitho-core embed` and
`cargo test -p peitho --test tweet_embeds` first; they must fail because bare
`embed` is still an unknown language and the transform is not wired.

**Implementation**: Add `CodeImageRenderer::BuiltinEmbed` after
`BuiltinMath`; keep the explicit-entry lookup first in `renderer_for`. Do not
add a production parser branch: the existing resolver lookup at
`crates/peitho-core/src/parser.rs` excludes bare `embed` from unknown-language
validation and rejects emphasis.

In `transform_fragment`, match all four renderer variants. Narrow only
`External` and `BuiltinMermaid` into the existing SVG tail; keep
`BuiltinMath`'s early return; give `BuiltinEmbed` a distinct early return that
parses the block, calls `cache_or_render_embed`, and builds
`SourceFragment::image(line, format!("X post by @{}", status.user()),
RawImagePath::from_embeds_cache(&key))`. The existing outer reveal-span
strip/re-attach remains the sole annotation path. Keep the fall-through match
exhaustive over `SlotGroup`, `Heading`, `Paragraph`, `Text`, `Code`, `Math`,
`Footnotes`, `Image`, and `List`; add no wildcard and no fragment kind.
Update `SvgCodeImageRenderer`'s narrowing comment to name both excluded
non-SVG built-ins. Keep the existing synchronous slide/fragment loop, so
multiple misses render serially and each valid hit skips Chrome. Do not add
the embed cache to preview watch roots. Make no mapping, contract-check,
image-resolution, render, PDF-flatten, or publish-contamination branch: the
new raw PNG must traverse those existing image paths unchanged.

Update the resolver paragraph in `CLAUDE.md` to include bare `embed`, the
`EmbedRenderer` seam, PNG cache validation, and the external SVG override.
This is the invariant record, not a guide/example task; make no `site/`,
`examples/`, README, `bindings/`, package, or generated-shell change.

**Verification**:

```text
cargo test -p peitho-core embed
cargo test -p peitho --test tweet_embeds
cargo test -p peitho --test tweet_embeds -- --ignored --nocapture
```

The third command is the explicit Chrome/network E2E and is never part of a
normal unignored test run.

## Summary

<!-- derived-from #task-1-parse-and-normalize-one-x-status-url -->
<!-- derived-from #task-3-render-a-cache-miss-and-write-it-atomically -->
<!-- derived-from #task-7-implement-two-pass-chrome-rendering-through-the-one-shot-runner -->
<!-- derived-from #task-9-enable-the-resolver-backed-vertical-slice -->

Tasks 1–5 lock down the pure input/cache contract, Tasks 6–7 isolate Chrome
behind tested pure functions and the existing one-shot process runner, and
Task 8 threads the dependencies without changing behavior. Task 9 enables the
typed resolver path and proves that downstream output is a plain cached image.

## Full gates

Run the workspace race-sensitive test gate three separate times, then the
remaining Rust and drift checks:

```text
cargo test --workspace
cargo test --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
git diff --exit-code bindings/
git diff --exit-code packages/peitho-present/dist/shell.js
git diff --exit-code packages/peitho-present/dist/preview.js
git diff --exit-code packages/peitho-present/dist/remote.js
```

`bindings/` must remain unchanged: all new types are Rust-only and the final
fragment is the existing image kind. No TypeScript source changes are expected,
so do not run npm build/test/typecheck or regenerate bundles; the three bundle
drift checks above confirm the committed outputs stayed untouched.
