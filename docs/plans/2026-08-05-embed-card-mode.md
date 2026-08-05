# Opt-in oEmbed card mode for tweet embeds

<!-- constrained-by ../specs/2026-08-04-tweet-embeds-design.md -->
<!-- derived-from ./2026-08-04-tweet-embeds.md -->

Date: 2026-08-05

## Goal

Add an opt-in `mode: card` form for built-in `embed` fences. Card mode fetches
X oEmbed data at build time, caches the raw JSON, and regenerates escaped,
selectable, deck-styled static HTML on every build without Chrome. A block with
no options, or with `mode: screenshot`, must keep the v1 PNG path and existing
output bytes.

## Decisions

1. The author revised the trigger to typed `key=value` tokens on the opening
   fence; the body is exactly one non-blank X status URL. No option means
   screenshot; v1 accepts `mode=card` and `mode=screenshot`. See
   [Syntax revision](#syntax-revision-author-decision-2026-08-05), which
   supersedes the original body-option grammar retained elsewhere as plan
   history. Future `theme` or `width` keys require no regrammar.
2. An explicit `code_images.embed: <command>` remains the external SVG path.
   It rejects `mode=` fence options. With a bare `embed` fence, the complete
   body and whitespace go to the command verbatim on stdin.
3. Core adds `OEmbedFetcher`, with one method taking the normalized status URL
   and returning the raw JSON response body as `String`. The CLI implementation
   runs system
   `curl -fsS --max-time 30 --max-filesize 1048576 <endpoint>` through the
   existing spawn + piped readers + timeout + kill/reap one-shot discipline,
   never `Command::output()`. This preserves the v1 no-new-dependency decision;
   curl ships on macOS, Linux CI images, and Windows 10+, while `-f` surfaces
   HTTP 4xx/5xx responses and exit status plus stderr remain legible.
4. The only endpoint is
   `https://publish.x.com/oembed?url=<encoded-status-url>&omit_script=1&dnt=1&hide_thread=1`,
   requested directly without redirect following. **Measured 2026-08-05:**
   `https://publish.twitter.com/oembed` returns HTTP 301 with an empty body and
   a `Location` header pointing to `https://publish.x.com/oembed?...`.
   `curl -f` without `-L` does not treat that redirect as a failure; it exits
   successfully with zero bytes. Do not add `-L`: silently following a
   provider-controlled redirect to an arbitrary host is outside the fixed
   single-provider contract. Input stays limited to the same normalized X
   status URLs as v1.
5. Card data uses `.peitho/embeds-cache/<key>.json`, beside PNG entries, with
   the existing atomic temp-file + rename writer and per-deck cache resolution.
   The SHA-256 byte stream is `\0peitho-builtin-embed-card\0`, the normalized
   URL, `\0omit_script=1`, `\0dnt=1`, then `\0hide_thread=1`. No crate version or
   card-markup bytes enter the key. Only a regular file no larger than
   `MAX_OEMBED_RESPONSE_BYTES` (1 MiB / 1048576 bytes) is read; it is a hit only
   when it is UTF-8 JSON with string fields `html`, `author_name`, and `url`.
   Non-regular, oversized, or invalid entries miss and self-heal. There is no
   TTL or auto-refresh. Delete the named file to refresh; cache data never
   enters `dist/`.
6. The cached artifact is the raw oEmbed JSON, not trusted renderable HTML.
   Following the cache-security boundary in the
   [built-in math plan](./2026-07-18-builtin-math.md), cached bytes never flow
   verbatim into slide markup. `html` is parsed only as data with the existing
   `lol_html` dependency:
   extract tweet `<p>` content while retaining generated `<br>` elements and
   HTTP(S) link href/text pairs, take `author_name` from JSON, derive the handle
   from the normalized status URL, and take the date label from the trailing
   blockquote anchor. Retain optional `lang` and `dir` from each tweet `<p>` on
   the generated tweet-text element: `dir` accepts only `ltr`, `rtl`, or `auto`,
   and `lang` must fully match `[A-Za-z][A-Za-z0-9-]{0,34}`. Omit an absent
   attribute; reject a present-but-invalid value as a line-numbered structural
   error rather than dropping it. The provider HTML is never injected. Every
   retained text and attribute value, including `lang` and `dir`, is escaped
   while building fresh card markup; a non-HTTP(S) href or a missing
   paragraph/date anchor is a line-numbered error naming the URL.
7. Card mode produces `FragmentKind::EmbedCard { html }`, mirroring `Math`.
   It routes to conventional body slots, is accepted only by
   `Accepts::Blocks`, and renders inside `<div class="peitho-embed-card">`.
   Screenshot mode remains an image fragment. The routing difference is
   deliberate and incompatible layouts fail through the ordinary typed check.
8. Card CSS is embedded in the binary and prepended to `peitho.css` only when
   at least one card fragment exists, following `MathAssets::katex()` because a
   deck CSS source replaces the built-in theme wholesale. Every color and font
   is controlled by `--peitho-embed-card-*` properties with inherited text/font
   and a light border as defaults. Add no fonts or images; omit an X logo.
9. Card mode never invokes Chrome. An all-card deck builds without Chrome;
   screenshot cache/render behavior and `EmbedRenderParams` stay untouched.
   A valid JSON cache hit works offline. Missing curl or any fetch failure on a
   miss is a line-numbered error naming the URL, cache path, and delete-to-refresh
   story.

## Syntax revision (author decision, 2026-08-05)

The original plan put a mixed grammar in the body: a bare URL followed by a
`key: value` option line.

````markdown
```embed
https://x.com/gosukenator/status/2074821309259973046
mode: card
```
````

The implemented grammar keeps the v1 body pure—exactly one non-blank URL—and
puts whitespace-separated `key=value` modifiers on the fence info string:

````markdown
```embed mode=card
https://x.com/gosukenator/status/2074821309259973046
```
````

Only bare `embed` resolves these tokens; v1 accepts `mode=card` and
`mode=screenshot`. Unknown or duplicate keys, unknown values, bare tokens, and
empty keys/values are line-numbered errors with fence-syntax help. A braced
tail remains line emphasis and therefore keeps the rendered-code-image error.
An explicit `code_images.embed` override rejects `mode=` tokens; its bare fence
still sends the body verbatim to the external command.

The parser owns the single interpretation point: `emphasis::split_info_string`
returns a non-braced tail, `CodeImagesConfig::renderer_for` identifies built-in
and overridden embeds with typed variants, and `SourceFragment` carries the
parsed `EmbedMode` to `code_images::transform_fragment`. The transform parses
only the URL and never re-parses mode text.

Rationale: the old body mixed a bare URL line with `key: value` options. Fence
modifiers instead occupy the same syntactic position as language tags and
`{2-4}` emphasis, while `key=value` matches the existing `::: {slot=name}`
fence-attribute vocabulary. This token grammar can accept future `theme` or
`width` keys without another grammar change. **Measured 2026-08-05:** the
opening-fence form with info string `embed mode=card` was a hard error—
"unexpected text after the code language"—so the syntax slot was unclaimed and
no existing deck changes meaning.

When later historical task or edge-case text conflicts with this section, this
revision controls; the original task record is otherwise left intact.

## Approach

### Built-in grammar and dispatch

In [`crates/peitho-core/src/code_images.rs`](../../crates/peitho-core/src/code_images.rs),
change `parse_embed_block` to return a `ParsedEmbedBlock { target, mode }`, with
`EmbedMode::{Screenshot, Card}`. Enumerate original body lines before trimming
blanks so an offending option reports its physical source line; an empty block
still reports the fence line. Recognize an option-shaped first line only to emit
the specific options-before-URL error; otherwise validate it through the
unchanged `parse_embed_url` / `TweetStatusUrl` path. Collect later lines through
one generic `key: value` parser with centralized duplicate detection.

Keep `CodeImagesConfig::renderer_for("embed") -> BuiltinEmbed` as the single
typed resolver. In `transform_fragment`, match the parsed mode only inside the
`CodeImageRenderer::BuiltinEmbed` arm:

- `Screenshot` calls the existing `cache_or_render_embed` with
  `BUILTIN_EMBED_PARAMS` and rebuilds the same `SourceFragment::image`.
- `Card` calls new `cache_or_fetch_oembed`, then `build_embed_card_html`, and
  returns `SourceFragment::embed_card`.
- `External` continues into the existing SVG tail with
  `fragment.code_text()` unchanged; it never calls `parse_embed_block`.

Thread `&impl OEmbedFetcher` through `parse_deck_and_transform`,
`transform_code_images`, and recursive `transform_fragment` beside the existing
`EmbedRenderer`. Panic spies in tests make the mode boundary executable:
screenshots cannot fetch, cards cannot render PNGs, and unrelated decks call
neither seam.

The public core seam is intentionally one method:

```rust
pub trait OEmbedFetcher {
    fn fetch(&self, normalized_url: &str) -> crate::Result<String>;
}
```

### oEmbed data, cache, and safe card builder

Register a private `embed_card` module in `crates/peitho-core/src/lib.rs`, and
add `crates/peitho-core/src/embed_card.rs` plus
`crates/peitho-core/assets/embed-card.css`:

- `OEmbedDocument { html, author_name, url }` is the private serde shape. Extra
  provider fields are ignored; the three required strings define cache
  validity.
- One ordered request-parameter constant feeds both
  `builtin_oembed_request_url` and `builtin_embed_card_cache_key`, preventing
  request-parameter/cache drift. `builtin_oembed_request_url` always targets
  `publish.x.com` directly. Query-component percent encoding is implemented as
  a small pure helper; no URL or HTTP crate is added.
- `cache_or_fetch_oembed(line, target, fetcher, cache_dir)` in
  `code_images.rs` gates `<key>.json` reads on `metadata.is_file()` and
  `metadata.len() <= MAX_OEMBED_RESPONSE_BYTES`, then fetches only on a miss,
  validates the fetched JSON before caching, writes the exact response string
  atomically, and returns the parsed document. Non-regular and oversized cache
  entries are misses. A fetched body over the 1 MiB bound is a line-numbered
  error and is not cached; invalid fetched data is also not cached.
- `extract_embed_card` uses `lol_html` handlers and an explicit safe model such
  as `TweetParagraph { lang, dir, parts }` plus `Text`, `Break`, and `Link`
  parts, rather than copying DOM substrings. It requires one tweet blockquote,
  at least one `<p>`, and a trailing out-of-paragraph date anchor. Parse `dir`
  to a typed `TextDirection::{Ltr, Rtl, Auto}` and validate optional `lang`
  as a full match against `[A-Za-z][A-Za-z0-9-]{0,34}`; invalid present values
  are structural errors and absent values remain `None`. Within the blockquote, only the
  expected text / `<p>` / `<br>` / `<a>` structure is accepted; an unknown
  element is a structural error rather than a partial card. Provider markup
  outside the blockquote and nonessential attributes on allowed elements are
  never retained.
- `build_embed_card_html(line, normalized_url, handle, document)` validates the
  JSON permalink and every retained tweet link as HTTP(S), then emits only
  peitho-owned tags/classes. It escapes text with `html_escape::encode_text`
  and hrefs with `encode_double_quoted_attribute`. It returns
  `EmbedCardMarkup { html, plain_text }` so `ManifestSlideText` receives the
  visible tweet text without reparsing generated HTML. Generated tweet-text
  elements copy validated `lang` / `dir` when present, escaping both as
  attributes, and omit them when absent.
- `EmbedCardAssets::builtin().css()` exposes the embedded stylesheet. The
  stylesheet uses `--peitho-embed-card-color`, `-background`, `-border-color`,
  `-link-color`, `-muted-color`, and `-font-family`; geometry may remain fixed.

The oEmbed `url` field supplies the card/date permalink after scheme validation;
the normalized input URL remains the request/cache identity and the source of
the lowercase `@handle`. A card-builder change never invalidates JSON because
the card is rebuilt from data on every build.

### Typed phase routing and rendering

In [`domain.rs`](../../crates/peitho-core/src/domain.rs), add
`FragmentKind::EmbedCard { html: String }` and
`SourceFragment::embed_card(line, html, plain_text)`. Its default accepts value
is `Blocks`; `removal_noun`, `Display`, and `try_map_image_src` preserve it
exhaustively. `plain.rs` reads the constructor's extracted plain text into the
manifest body. Add explicit arms anywhere the compiler exposes the new variant,
including the reveal/footnote bookkeeping in `parser.rs` and recursive handling
in `code_images.rs`; do not add wildcard arms or a TypeScript-facing type.

Route `EmbedCard` beside `Math` in
[`mapping.rs`](../../crates/peitho-core/src/mapping.rs), and accept only
`(Accepts::Blocks, FragmentKind::EmbedCard { .. })` in both contract matrices in
[`check.rs`](../../crates/peitho-core/src/check.rs) and
[`render.rs`](../../crates/peitho-core/src/render.rs). `render_block_slot`
flushes the current Markdown run, emits the peitho wrapper plus trusted
builder output, then resumes Markdown. `render_revealed_fragment` emits the same
wrapper with `data-reveal-step`, preserving the transform's existing reveal
span behavior.

`render_deck` tracks `uses_embed_card` beside `uses_math`. Preserve the current
no-card branches byte-for-byte. For a card deck, prepend embedded card CSS before
the deck/theme CSS; when math is also present, pin the deterministic order as
KaTeX CSS, card CSS, then deck/theme CSS. No new `Deck<Rendered>` asset field is
needed because cards have no companion files: the completed CSS already rides
the Rendered typestate.

### CLI curl implementation

In [`crates/peitho/src/main.rs`](../../crates/peitho/src/main.rs), add
`CliOEmbedFetcher` and pass it at both `parse_deck_and_transform` call sites.
It builds the fixed endpoint with core's `builtin_oembed_request_url` and runs:

```text
curl -fsS --max-time 30 --max-filesize 1048576 <endpoint>
```

`<endpoint>` must begin with `https://publish.x.com/oembed?`; the argv must not
contain `-L`, `--location`, or any other redirect-following flag. This encodes
the measured `publish.twitter.com` empty-301 pitfall at command construction,
rather than relying on curl defaults.

Use `run_child_with_timeout(..., |_, _| false)` so success is defined by normal
process exit after both pipes drain. Give the outer runner a small margin beyond
curl's 30-second deadline. Convert successful UTF-8 stdout to `String`; map
spawn/capture/wait/kill errors, nonzero status with `stderr_excerpt`, timeout,
and non-UTF-8 stdout to `BuildError` without a line. `cache_or_fetch_oembed`
attaches the fence/body line, normalized URL, `.json` path, offline-hit rule,
and refresh help. Curl exit code 22 leads with help that the X post may have
been deleted or made private before the network/retry advice; other exit codes
keep the generic network/curl help. Do not call `locate_chrome` from this path.

## TDD task breakdown

Every task starts with the named red test(s), then adds only enough production
code to make that task green.

1. **Option grammar — `code_images.rs`.** Red:
   `embed_block_defaults_to_screenshot_and_accepts_explicit_modes`,
   `embed_block_allows_blank_lines_between_url_and_options`,
   `embed_block_rejects_option_before_url_at_source_line`,
   `embed_block_rejects_unknown_option_key_at_source_line`,
   `embed_block_rejects_unknown_mode_value_at_source_line`, and
   `embed_block_rejects_duplicate_or_malformed_options_at_source_line`.
   Implement `EmbedMode`, `ParsedEmbedBlock`, source-line retention, and the
   future-key-ready option parser. Keep all existing X URL cases green.
2. **Request identity, fetch seam, and JSON cache — `embed_card.rs`,
   `code_images.rs`, and `crates/peitho-core/tests/support/embed_card_cache_key.rs`.**
   Red: `embed_card_request_url_targets_publish_x_and_encodes_fixed_parameters`,
   `embed_card_cache_key_covers_url_and_every_request_parameter`,
   `oembed_cache_miss_fetches_once_and_writes_raw_json_atomically`,
   `oembed_valid_cache_hit_skips_fetcher`,
   `oembed_invalid_cache_self_heals`, and
   `oembed_fetch_failure_names_line_url_cache_and_refresh`. Add
   `FixtureOEmbedFetcher` that records normalized URLs. Pin the exact SHA-256
   vector; cover empty/malformed JSON, missing/wrong-type required fields,
   exact raw-byte preservation, no temp-file residue, and no fetch on a valid
   hit.
3. **Captured-response extraction — `embed_card.rs` and
   `crates/peitho-core/tests/fixtures/x-oembed-response.json`.** Red:
   `captured_x_oembed_extracts_paragraphs_lang_dir_links_author_handle_and_date`,
   `oembed_html_preserves_br_boundaries`,
   `oembed_html_without_tweet_paragraph_is_line_numbered`,
   `oembed_html_without_trailing_date_anchor_is_line_numbered`, and
   `oembed_html_with_unexpected_blockquote_structure_is_line_numbered`. Use the
   existing 971-byte, unedited fixture fetched 2026-08-05 from `publish.x.com`
   for `https://x.com/gosukenator/status/2074821309259973046`; implementation
   requires no network capture. Its `html` is a tweet blockquote shaped as
   `<p lang="ja" dir="ltr">…text…<a href>…</a></p>&mdash; mizzy
   (@gosukenator) <a href="…?ref_src=…">July 8, 2026</a>`. Assert the complete
   extracted model, including `lang = "ja"`, `dir = Ltr`, multiple text chunks,
   and links; use a synthetic companion only for the `<br>` case absent from
   the captured response.
4. **Escaping and href defense — `embed_card.rs`.** Red:
   `embed_card_escapes_hostile_oembed_fields_and_never_injects_provider_html`,
   `embed_card_rejects_non_http_links`,
   `embed_card_omits_absent_tweet_lang_and_dir`, and
   `embed_card_rejects_invalid_tweet_lang_or_dir`. Use a hostile `html` field
   with out-of-blockquote script/style elements, event attributes on allowed
   nodes, tag-shaped author/date/link text, quotes in the JSON permalink, and
   `javascript:`/`data:` tweet links. Assert only generated tags survive, all
   strings/attributes (including valid `lang` / `dir`) are escaped, absent
   attributes are omitted, and unsafe schemes or invalid present attributes
   fail with URL + line + help.
5. **First-class fragment and contract routing — `domain.rs`, `mapping.rs`,
   `check.rs`, `plain.rs`, and compiler-reported exhaustive sites.** Red:
   `source_fragment_embed_card_preserves_html_and_plain_text`,
   `maps_embed_card_to_body_slot`, `accepts_embed_card_in_blocks_slot`,
   `rejects_embed_card_in_image_slot_with_contract_error`, and
   `embed_card_text_enters_manifest_body`. Add the typed variant/constructor
   and every explicit arm. Assert the mismatch is the existing line-numbered
   accepts error, while screenshot images still route to image slots.
6. **HTML rendering and conditional CSS — `render.rs` and
   `assets/embed-card.css`.** Red:
   `render_block_slot_splices_embed_card_between_markdown_runs`,
   `render_revealed_embed_card_stamps_its_wrapper`,
   `rendered_deck_prepends_card_css_before_theme_only_when_used`, and
   `rendered_deck_with_math_and_card_keeps_theme_last`. Before changing CSS
   assembly, pin `render_deck_without_embed_cards_keeps_existing_bytes` over
   plain, math-only, and screenshot-only fixtures. Assert every color/font
   declaration uses a named custom property and no font/image asset is emitted.
7. **Transform dispatch and override boundary — `code_images.rs`.** Red:
   `builtin_card_mode_fetches_without_rendering_png`,
   `builtin_screenshot_modes_never_fetch_oembed`,
   `legacy_embed_without_options_matches_explicit_screenshot_bytes`, and
   `explicit_embed_override_receives_option_lines_verbatim`. Thread the new
   fetcher through all transforms, preserve reveal spans in both modes, and
   keep `cache_or_render_embed`, `builtin_embed_cache_key`, and
   `EmbedRenderParams` unchanged.
8. **CLI curl runner — `main.rs`.** Red:
   `oembed_curl_runner_targets_publish_x_without_redirect_following`,
   `oembed_curl_runner_uses_fixed_flags_encoded_endpoint_and_no_stdin`,
   `oembed_curl_runner_returns_complete_stdout_after_successful_exit`,
   `oembed_curl_runner_reports_exit_status_and_stderr`,
   `oembed_curl_runner_reports_deleted_or_private_post_for_http_failure`,
   `oembed_curl_runner_reports_timeout_and_stderr`, and
   `oembed_curl_runner_reports_missing_curl`. Assert the exact host is
   `publish.x.com` and argv contains neither `-L` nor `--location`. Drive an
   invoker seam for argv and outcome tests, then implement `CliOEmbedFetcher`
   with the shared one-shot runner. The timeout fixture asserts error mapping
   and stderr; the shared runner's pre-existing tests cover deadline and
   kill/reap behavior.
9. **CLI integration, compatibility, and documentation —
   `crates/peitho/tests/tweet_embeds.rs`.** Red:
   `build_card_embed_from_cached_oembed_without_chrome`,
   `build_all_card_embeds_never_create_png_assets`,
   `build_card_cache_miss_reports_curl_failure_with_refresh_help`, and
   `build_mixed_embed_modes_use_only_their_required_backends`. Assert card text
   is selectable HTML, no provider `<script>`/blockquote/raw JSON or `.peitho`
   data enters `dist/`, card CSS appears once, and legacy no-card `peitho.css`,
   slide HTML, and cached-PNG output match checked-in pre-change bytes. Update
   `README.md`, `site/content/guide/writing-decks.md`,
   `site/content/guide/frontmatter.md`, `site/content/examples/tweet-embed.md`,
   and the resolver invariant in `CLAUDE.md`; document the external override's
   verbatim stdin and the card-vs-screenshot slot-routing difference.

Final verification: run focused tests after each task, then three consecutive
`cargo test --workspace` runs, `cargo fmt --all --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, and
`git diff --exit-code bindings/`. Expect no TypeScript binding or shell bundle
drift. Build cached card-only, screenshot-only, mixed, preview, PDF, and publish
fixtures; inspect one card in a real browser for selectable text, inherited
theme styling, working safe links, and no Chrome process on the card-only miss
path.

## Post-plan hardening (review rounds, 2026-08-05)

- Non-whitespace blockquote-level text before the first tweet paragraph or
  between tweet paragraphs is a structural error; only the attribution before
  the trailing date anchor is accepted.
- Cache reads require a regular file within the 1 MiB bound, and fetched
  responses over that bound fail before any cache write.
- The shared `stderr_excerpt` helper maps control characters to spaces and
  normalizes whitespace before truncation for all callers, including curl and
  external-command errors.
- Empty embed blocks report URL-first and `mode: card` / `mode: screenshot`
  option syntax help.

## Edge cases

Every failure below is an `ErrorKind::Asset`, layout, or accepts `BuildError`
with a source line and actionable help; CLI line-map translation continues to
identify included source files.

| Case | Behavior |
|---|---|
| No option lines | Use the existing screenshot mode and bytes. |
| `mode: screenshot` | Same PNG key, renderer parameters, image fragment, and output as the implicit default. |
| `mode: card` with valid JSON cache | Parse cached data, rebuild escaped card HTML, and call neither curl nor Chrome. |
| `mode: card` cache miss | Run curl once, validate and atomically cache raw JSON, then build the card; never invoke Chrome. |
| All embeds are cards | Build without Chrome; emit no embed PNG assets. |
| Mixed card and screenshot blocks | Resolve serially; curl is possible only for card misses, Chrome only for PNG misses, and card CSS is prepended once. |
| Explicit `code_images.embed` override | Send the entire body verbatim to the external SVG command; ignore built-in mode grammar. Existing command failures remain line-numbered. |
| Blank lines around/between fields | Ignore them while retaining physical line offsets for diagnostics. |
| Empty block | Error at the fence line with URL and option syntax help. |
| Option before URL | Error on that line; require the URL first. |
| Unknown key, unknown mode, duplicate key | Error on the offending line and list `mode: card` / `mode: screenshot`. |
| Missing colon or empty key/value | Error as a malformed option line; never ignore trailing content. |
| A second URL after the first | Error as `unknown embed option 'https'` on that line. |
| Invalid/non-X status URL | Reuse v1 validation and supported-form help; generic providers remain rejected. |
| oEmbed request construction | Target `publish.x.com` directly and never pass curl `-L` / `--location`; no redirect target is followed implicitly. |
| Cache path is a directory/unwritable, or atomic write fails | Error names the URL and `.json` cache path with writable-directory/refresh help. |
| Cache is empty, malformed JSON, or lacks a required string field | Treat as a miss; fetch once and replace only with a valid raw response. |
| Fetched body is empty, non-UTF-8, malformed JSON, or lacks a required field | Error with line, URL, cache path, and retry/refresh help; do not cache it. |
| curl is missing, exits nonzero (`-f` surfaces HTTP 4xx/5xx), or times out | Error includes line, URL, cache path, exit/timeout detail, stderr excerpt, and delete-to-refresh story. |
| Valid JSON has missing `<p>` or trailing date anchor | Error names the URL; never emit a partial card. |
| Any retained JSON/card URL or tweet link is not HTTP(S) | Error names the URL and unsafe field; protocol-relative, `javascript:`, and `data:` links are rejected. |
| Provider HTML contains scripts outside the tweet blockquote or attributes on allowed elements | Ignore them; only peitho-generated tags and validated values reach output. |
| The tweet blockquote contains an unexpected element or ambiguous structure | Error names the URL; never emit a silently partial card. |
| Tweet `<p>` has valid `lang` and/or `dir` | Retain and attribute-escape them on the generated tweet-text element; `dir` is limited to `ltr`, `rtl`, or `auto`. |
| Tweet `<p>` omits `lang`/`dir`, or supplies an invalid value | Omit absent attributes; reject present invalid `lang`/`dir` with a line-numbered structural error and help. |
| Author, handle, date, tweet text, or href contains HTML metacharacters | Escape it at card generation; it remains inert text/attribute data. |
| Layout has only image slots for a card | Card routes as body content and fails through the ordinary line-numbered mapping/check path. |
| Layout has only body slots for a screenshot | Screenshot remains an image and fails through the ordinary line-numbered image-slot path. |
| Card is inside `::: {reveal}` | Preserve its reveal span and stamp the generated wrapper, as for Math. |
| Deck contains no cards | Emit no card CSS and preserve current HTML/CSS/artifact bytes, including math-only and screenshot-only decks. |
| Cache refresh | Never automatic; deleting the named `.json` refetches, while changing peitho card markup reuses the cached data and regenerates HTML. |

## Out of scope

- A dark-theme option or any automatic light/dark selection.
- A width option or other presentation settings. The option-line grammar is
  intentionally ready for future `theme` and `width` keys.
- Generic oEmbed providers or endpoint discovery; only X status URLs are valid.
- Making card mode the default, bare-URL auto-detection, or changing remote
  Markdown image handling.
- Injecting provider HTML, running `widgets.js`, or fetching/rendering cards in
  the browser.
- Cache TTLs, conditional requests, automatic refresh, or a refresh command.
- Markdown rendering in speaker notes or any other notes/presenter change.
- New fonts, image/logo assets, an HTTP client crate, or parallel embed fetches.

## Summary

<!-- derived-from #decisions -->
<!-- derived-from #approach -->
<!-- derived-from #tdd-task-breakdown -->
<!-- derived-from #post-plan-hardening-review-rounds-2026-08-05 -->

Issue #398 adds one typed, opt-in card branch behind the existing built-in
`embed` resolver: raw oEmbed data is cached, peitho-owned HTML/CSS is regenerated
safely, and legacy screenshot/external-command behavior remains unchanged.
