# Generic oEmbed cards for `embed` blocks

<!-- constrained-by ../../CLAUDE.md -->
<!-- derived-from ./2026-08-05-embed-card-mode.md -->

Issue: #399
Date: 2026-08-05

## Goal

Extend bare built-in `embed` fences from X status URLs to arbitrary HTTP(S)
pages that advertise a JSON oEmbed endpoint. Generic providers always become a
Peitho-owned static card: a locally cached thumbnail when the oEmbed response
offers one, otherwise a text-only card. Discovery and downloads happen at build
time, raw provider HTML is never injected, and cached JSON plus cached image
bytes make subsequent builds deterministic and offline.

Issue #398's X behavior is a compatibility boundary. X screenshots, X
`mode=card`, their cache keys, generated HTML/CSS/assets, and the X curl argv
must remain byte-identical. The new path begins only after URL dispatch has
proved that the author-supplied URL is not an X/Twitter URL.

## Author decisions (settled 2026-08-05)

1. Generic providers render as a static card, NEVER a screenshot and NEVER injected provider HTML (measured: YouTube/Mastodon oEmbed html is iframe/script — uninjectable; generic iframes have no `rendered`-equivalent completion signal so generic screenshots would be heuristic and flaky). With `thumbnail_url`: thumbnail card = build-time-downloaded image + title/author/provider caption + permalink. Without: text card = title/author/provider + permalink (author-approved fallback; measured: Mastodon oEmbed is type=rich with NO thumbnail_url and its html contains no status text — a text card matches the fidelity of Mastodon's own script-less embed. Mastodon status pages also lack og:image/og:description, measured 2026-08-05).
2. X handling is UNCHANGED: x.com/twitter.com status URLs keep the dedicated screenshot and mode=card paths byte-identical; non-status X URLs stay the existing unsupported-URL error and never fall through to discovery; any `mode=` option with a non-X URL is a line-numbered error (generic embeds are always the card, X-only options must say so).
3. Provider identification is generic oEmbed discovery: fetch the page URL, extract `<link rel="alternate" type="application/json+oembed">` (measured on real pages: the href attribute is HTML-entity-encoded — `&amp;` — and must be decoded; first matching link wins), fetch that endpoint, validate. No curated provider registry.
4. Reuse the #398 infrastructure: curl one-shot seam, raw-JSON cache discipline (regular-file gate, size cap, atomic write, self-heal, no TTL, offline hits), strict escaping/validation, embedded card CSS.

These decisions are fixed inputs to the implementation and are not reopened by
later task details.

## Measured facts (2026-08-05)

- Real YouTube watch page: 1,332,364 bytes — LARGER than MAX_OEMBED_RESPONSE_BYTES (1 MiB). The discovery page fetch needs its own larger cap (propose 8 MiB constant) and so does the thumbnail fetch (YouTube hqdefault.jpg measured 21,011 bytes; propose a generous image cap, e.g. 8 MiB).
- Discovery link measured on mastodon.social: `<link href="https://mastodon.social/api/oembed?format=json&amp;url=..." rel="alternate" type="application/json+oembed">` (attribute order varies; rel/type matching must be attribute-based, not string-shape).
- YouTube oEmbed: type=video, thumbnail_url present, title + author_name present. Mastodon oEmbed: type=rich, no thumbnail_url, author_name + provider_name present, html useless for text extraction.
- Committed fixtures already at crates/peitho-core/tests/fixtures/: youtube-oembed-response.json, mastodon-oembed-response.json, mastodon-status-page.html (57,886-byte real page with the discovery link), youtube-thumbnail.jpg (real 480x360 JPEG). The 1.33 MB YouTube page is NOT committed (size); its measured link tag appears inline in tests.

The concrete byte bounds are:

```rust
pub const MAX_OEMBED_RESPONSE_BYTES: usize = 1024 * 1024; // existing, unchanged
pub const MAX_OEMBED_DISCOVERY_PAGE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_OEMBED_THUMBNAIL_BYTES: usize = 8 * 1024 * 1024;
```

Both new bounds are enforced once by curl's `--max-filesize` and again after
the bytes cross the core trait seam. The second gate keeps fixture fetchers and
future non-curl callers under the same contract.

## Approach

### Typed URL dispatch and explicit option presence

Add the `url` crate to the workspace dependencies and `url.workspace = true`
to `peitho-core`. URL parsing and relative-reference resolution remain pure
core operations; no HTTP client crate is added. Hand-rolling RFC 3986
relative-reference resolution for discovery `href`s is error-prone and
security-relevant; the servo/rust-url `url` crate is pure parsing with no I/O,
so the no-HTTP-crate stance remains intact.

In
[`crates/peitho-core/src/code_images.rs`](../../crates/peitho-core/src/code_images.rs),
grow the private target enum to:

```rust
enum EmbedTarget {
    X(TweetStatusUrl),
    Generic(GenericPageUrl),
}
```

`GenericPageUrl` owns the `url::Url` plus its serialized normalized form. It
accepts only `http` and `https`, retains the path and query, removes the fragment
because fragments are not part of an HTTP request, and uses `Url`'s normalized
scheme/host/path serialization for fetch identity, cache identity, and the
author-supplied permalink. That permalink is never replaced with an oEmbed
response URL.

Keep X dispatch first. Preserve the existing case-insensitive
`https://x.com/` / `https://twitter.com/` prefix recognition and pass the
untouched path to `parse_x_status_url`. After generic URL parsing, reject an
ASCII-case-insensitive host that equals `x.com` / `twitter.com` or ends in
`.x.com` / `.twitter.com` with the existing unsupported-X error before
constructing `GenericPageUrl`. Trim exactly one trailing `.` from the
lowercased host for this X-specific comparison, so `x.com.` and `www.x.com.`
are guarded while a non-X FQDN such as `example.com.` still dispatches as
`Generic`. This also prevents `www.x.com`, `mobile.twitter.com`, other
subdomains, and HTTP or malformed non-status X pages from reaching discovery.
Every other valid HTTP(S) URL becomes `EmbedTarget::Generic`; every other
scheme or malformed URL is a line-numbered unsupported-embed error whose help
says that generic providers require an HTTP(S) page URL.

The parser currently collapses an absent mode and explicit `mode=screenshot`
to the same `EmbedMode`. Replace that attachment with an `EmbedOptions` value
whose `mode: Option<EmbedMode>` retains presence:

- for `EmbedTarget::X`, `None` still means `Screenshot`, and explicit
  `Screenshot` / `Card` enter the existing branches unchanged;
- for `EmbedTarget::Generic`, `Some(_)` is an error at the opening-fence line
  with help that `mode=card` and `mode=screenshot` are X-only;
- for `EmbedTarget::Generic`, `None` always selects the new static-card path;
- explicit `code_images.embed` overrides continue to reject all `mode=` tokens
  during parsing and receive a bare fence body verbatim.

This is presence retention, not a grammar change. Unknown/duplicate keys,
unknown mode values, bare tokens, braced emphasis tails, and options on other
languages retain the #398 behavior and diagnostics.

### Pure discovery and generic response model

Add
[`crates/peitho-core/src/generic_oembed.rs`](../../crates/peitho-core/src/generic_oembed.rs)
and register it privately from `lib.rs`. Keep X-specific extraction and markup
in `embed_card.rs` unchanged.

`discover_oembed_endpoint(page_url, html)` uses `lol_html` element handlers,
not substring matching. For each `link` element, it:

1. matches `rel` as an ASCII-whitespace-separated, ASCII-case-insensitive token
   list containing `alternate`;
2. matches the trimmed `type` attribute ASCII-case-insensitively against
   `application/json+oembed`;
3. stops at the first element whose `rel` and `type` match, even if its `href`
   is missing or invalid;
4. decodes HTML entities in that element's `href` exactly once, including the
   measured `&amp;` query separators;
5. resolves an absolute, protocol-relative, root-relative, or path-relative
   href with `page_url.join(...)`; and
6. requires the resolved discovery endpoint to use HTTP(S).

An absent match, missing/empty href on the first match, URL-join failure, or
non-HTTP(S) endpoint becomes a line-numbered error naming the author-supplied
page URL and explaining the required discovery link. Attribute order never
matters.

Deserialize provider JSON into an all-optional private shape:

```rust
struct GenericOEmbedDocument {
    kind: Option<GenericOEmbedType>, // serde rename = "type"
    title: Option<String>,
    author_name: Option<String>,
    provider_name: Option<String>,
    thumbnail_url: Option<String>,
    url: Option<String>,
}

enum GenericOEmbedType {
    Photo,
    Video,
    Link,
    Rich,
}
```

Missing optional fields are accepted. A present unknown `type`, a wrong JSON
field type, or malformed JSON is invalid data. Trim display fields and treat an
empty string as absent. Require at least a nonempty `title` or nonempty
`author_name`; `provider_name` alone cannot make a useful card.

All four standard oEmbed types are accepted. Choose the image URL in this
order:

1. a present `thumbnail_url`, for every type;
2. for `type=photo` only, the spec's `url` field when `thumbnail_url` is absent;
3. no image for `video`, `link`, `rich`, or an absent type without a thumbnail.

Thus a photo response without `thumbnail_url` uses its required `url` as the
card image; a photo missing both fields is invalid rather than silently losing
its defining content. Every selected image URL must be HTTP(S). For every other
type, the response `url` is ignored. The response's `html`, `author_url`,
`provider_url`, dimensions, and every unknown field are ignored and can never
reach output.

`build_generic_embed_card` takes the normalized author-supplied page URL and
the validated document. It produces only Peitho-owned caption fields:

- optional escaped title text;
- optional escaped author text;
- optional escaped provider text;
- an escaped permalink attribute containing the normalized author-supplied
  page URL;
- an escaped thumbnail alt attribute derived from title, then author; and
- manifest plain text containing present title and author, in that order,
  joined by one newline and never including provider HTML.

`GenericEmbedCardParts` contains only those escaped caption/attribute fields
and `plain_text`; it does not duplicate `GenericOEmbedData::image_url`, which
remains the sole image-selection source for thumbnail caching.

Use `html_escape::encode_text` for text nodes and
`encode_double_quoted_attribute` for href/alt attributes. Hostile title,
author, provider, and page-URL characters must remain inert. The builder never
accepts or parses provider `html`.

### Fetch seam and backend isolation

Extend the existing public core seam without changing its X method:

```rust
pub trait OEmbedFetcher {
    fn fetch(&self, normalized_url: &str) -> crate::Result<String>; // existing X
    fn fetch_discovery_page(&self, page_url: &str) -> crate::Result<Vec<u8>>;
    fn fetch_discovered_oembed(&self, endpoint_url: &str) -> crate::Result<Vec<u8>>;
    fn fetch_thumbnail(&self, image_url: &str) -> crate::Result<Vec<u8>>;
}
```

Do not give the new methods permissive defaults: every production/test
implementation must state its behavior. Core owns dispatch, discovery,
validation, caching, image sniffing, card construction, and line context. The
CLI owns only one-shot byte retrieval. Fixture implementations return the
committed bodies and record calls; panic spies make incorrect cross-provider
traffic fail immediately.

The executable matrix is:

| Target/path | Allowed backend calls |
|---|---|
| X screenshot cache hit | none |
| X screenshot cache miss | `EmbedRenderer::render` only |
| X card JSON hit | none |
| X card JSON miss | existing `OEmbedFetcher::fetch` only |
| Generic JSON + thumbnail hits | none |
| Generic JSON hit + thumbnail miss | `fetch_thumbnail` only |
| Generic JSON miss + thumbnail response | discovery page, discovered endpoint, then thumbnail |
| Generic JSON miss + text response | discovery page and discovered endpoint only |

In particular, X never calls any new generic operation; generic never calls the
existing X `fetch` or `EmbedRenderer`; and an invalid generic `mode=` fails
before any backend call.

### Independent raw JSON and thumbnail caches

Keep all artifacts under `.peitho/embeds-cache/`, but give generic data separate
key domains. Pin these exact SHA-256 byte streams in shared test vectors:

```text
generic JSON:      \0peitho-generic-oembed\0<normalized-author-page-url>
generic thumbnail: \0peitho-generic-oembed-thumbnail\0<normalized-author-page-url>
```

No crate version, discovered endpoint, provider image URL, response bytes, or
card-markup version enters either key. The paths are:

```text
.peitho/embeds-cache/<generic-json-key>.json
.peitho/embeds-cache/<generic-thumbnail-key>.<jpg|png|webp|gif>
```

Discovery HTML is never cached. On a JSON lookup, use `symlink_metadata` and
require a regular file no larger than `MAX_OEMBED_RESPONSE_BYTES`, read its
exact bytes, and rerun the full generic serde/semantic validation. A valid hit
skips both page and endpoint fetches. A missing, unreadable, oversized,
malformed, semantically invalid, or symlink entry is a miss; a successful
refetch atomically replaces the path with the exact raw JSON bytes without
following a symlink. A failed/invalid refetch never overwrites the old entry;
an entry such as a directory that blocks atomic replacement reports the exact
cache-path error.

After JSON validation, derive whether an image is required. For an image card,
probe the four canonical cache candidates (`.jpg`, `.png`, `.webp`, `.gif`).
Run the `symlink_metadata` regular-file, nonempty, and
`MAX_OEMBED_THUMBNAIL_BYTES` gates before reading any candidate. Exactly one
existing candidate is a hit only after those gates pass and its magic agrees
with its extension:

- JPEG: `ff d8 ff`, canonical extension `.jpg`;
- PNG: `89 50 4e 47 0d 0a 1a 0a`, extension `.png`;
- WebP: `RIFF` plus `WEBP` at bytes 8–11, extension `.webp`;
- GIF: `GIF87a` or `GIF89a`, extension `.gif`.

Anything else is not a usable hit. On a miss, fetch only the selected image
URL, enforce the 8 MiB core gate, detect one of those four formats from magic,
write atomically to the matching extension, and remove stale sibling candidates
for the same key so the cache converges to one file. Corrupt regular entries
self-heal after a successful fetch. Non-regular/unwritable entries and cleanup
failures produce cache-path errors instead of bypassing the safety boundary.

A valid JSON hit with a missing image fetches only the image. A text-card JSON
hit never performs discovery or an image request. There is no TTL, conditional
request, or implicit refresh.

Refresh help always prints concrete paths. Metadata-only refresh deletes the
named `.json`. A complete thumbnail-card refresh deletes that `.json` and any
of the four explicitly named thumbnail candidates for the thumbnail key; after
format detection, errors name the one exact extension as well. This matters
because the image key intentionally follows the author-supplied page URL rather
than a provider-controlled image URL.

### Typed generic card fragment and image resolution

Do not turn the thumbnail into a prebuilt `<img>` string. In
[`domain.rs`](../../crates/peitho-core/src/domain.rs), add a distinct
`FragmentKind::GenericEmbedCard` variant whose generic image member participates
in typestate:

```rust
GenericEmbedCard {
    image: Option<S>,
    image_alt_attr: String,
    title_html: Option<String>,
    author_html: Option<String>,
    provider_html: Option<String>,
    permalink_attr: String,
}
```

Only the crate-private `SourceFragment::generic_embed_card` constructor receives
the builder's escaped parts and raw manifest text. `try_map_image_src` maps
`Some(S)` through its closure and preserves `None` without calling the closure;
recursive slot groups retain the same behavior. The cached path is constructed
through a typed/internal `RawImagePath` embeds-cache constructor that accepts
only a SHA-256 key plus a magic-derived image-format enum.

This deliberately sends thumbnail bytes through the ordinary
`resolve_image_paths` transition and CLI `ImageResolver`. The resolver
canonicalizes the cache file inside the deck, hashes/copies it to
`dist/assets/<hash>-<basename>`, deduplicates it, and supplies a
`ResolvedImagePath` before rendering. It also keeps the image in the manifest
and under publish's contained-distribution validation. No raw `.peitho` path,
provider URL, cache JSON, or cache directory can bypass `ImageResolver` or the
existing publish contamination check.

`GenericEmbedCard` defaults to and maps as `Accepts::Blocks`, beside `Math` and
the existing X `EmbedCard`, whether or not it has an image. Add it explicitly to
the matrices in `mapping.rs`, `check.rs`, and `render.rs`, plus all compiler-
reported exhaustive matches; do not add wildcard arms. An image-only layout
therefore rejects a generic card through the normal line-numbered accepts
error. Plain manifest text is exactly title/author from the constructor.

### Owned rendering and conditional CSS

`render_block_slot` flushes a Markdown run, writes the existing outer
`<div class="peitho-embed-card">`, and emits a Peitho-owned generic `<article>`
containing one permalink anchor around the optional image and caption. The
anchor owns class `peitho-embed-card__permalink` and
`rel="noopener noreferrer"`:

- optional `<img>` uses only the mapped `ResolvedImagePath` and escaped alt;
- title, author, and provider are emitted only when present;
- separators are generated by Peitho only between present caption fields;
- the link href is always the escaped normalized author-supplied page URL; and
- the text-card shape is identical except that the `<img>` element is omitted.

The reveal path stamps the same outer wrapper with `data-reveal-step`, as Math
and X cards do. No provider HTML, iframe, script, inline style, or response URL
is concatenated into the fragment.

Preserve
[`assets/embed-card.css`](../../crates/peitho-core/assets/embed-card.css)
byte-for-byte so X-only card decks keep #398 CSS bytes. Add a supplemental
`assets/generic-embed-card.css` that is embedded and prepended only when at
least one `GenericEmbedCard` exists. Generic-only decks receive the existing
base card CSS plus the supplemental thumbnail/title/meta rules; mixed X/generic
decks receive the base once and the supplement once. The deterministic order
is KaTeX CSS, existing base card CSS, generic card CSS, then deck/theme CSS.

The supplemental rules reuse `--peitho-embed-card-color`, `-background`,
`-border-color`, `-link-color`, `-muted-color`, and `-font-family`; add no brand
assets, fonts, hard-coded provider colors, scripts, or companion files. Decks
without generic cards preserve their current CSS bytes, including X-card-only,
X-screenshot-only, math-only, and plain decks.

### CLI generic curl operations

In
[`crates/peitho/src/main.rs`](../../crates/peitho/src/main.rs), keep
`fetch_oembed_with_invoker` and its X argv byte-identical:

```text
curl -fsS --max-time 30 --max-filesize 1048576 <publish.x.com endpoint>
```

It still has no `-L` and retains all current status/stderr/UTF-8 mapping.

Implement the three new `CliOEmbedFetcher` methods through the same
`SystemOEmbedCurlInvoker`, `run_child_with_timeout`, pipe-drain, timeout, and
kill/reap seam. Each generic operation uses this pinned argv shape, substituting
only its cap and URL:

```text
curl -fsS -L --max-redirs 5 \
  --proto =http,https --proto-redir =http,https \
  --max-time 30 --max-filesize <cap> <url>
```

The discovery page and thumbnail use `8388608`; the discovered JSON endpoint
uses `1048576`. `--proto` and `--proto-redir` ensure a provider redirect cannot
leave HTTP(S). The outer runner retains the five-second margin over curl's
30-second deadline. It sends no stdin and returns complete stdout bytes only
after a successful exit. Missing curl, redirect exhaustion, HTTP failure,
size failure, timeout, pipe/process errors, and nonzero exit with normalized
`stderr_excerpt` become core errors without a line; the cache/transform layer
adds the fence line, author URL, operation, cache paths, offline rule, and
refresh help.

### Example and documentation

Extend [`examples/tweet-embed/deck.md`](../../examples/tweet-embed/deck.md)
with slide 3 using a bare fence and the measured YouTube page:

````markdown
```embed
https://www.youtube.com/watch?v=dQw4w9WgXcQ
```
````

Copy the exact YouTube fixture JSON and JPEG bytes into the two computed cache
paths under `examples/tweet-embed/.peitho/embeds-cache/`. Do not commit the
1.33 MiB discovery page: the valid raw JSON cache hit skips discovery, and the
valid magic-checked JPEG cache hit skips the thumbnail request, so CI and the
demo-site build remain offline and Chrome-free for slide 3. Keep slide 1 first
and unchanged so it remains the gallery face. Update example CSS/layout only as
needed for the new generic card without changing slide 1 output.

Update these user-facing surfaces in the implementation PR:

- `README.md`: rename the X-only description to built-in embeds, document bare
  generic discovery/static cards, image-vs-text fallback, X-only `mode=`, cache
  files, curl redirects/caps, and slot routing;
- `site/content/guide/writing-decks.md` and
  `site/content/guide/frontmatter.md`: distinguish bare generic cards from X
  screenshot/card modes and retain the external override contract;
- `site/content/examples/tweet-embed.md`: describe slide 3, its committed raw
  JSON/JPEG caches, locally published thumbnail asset, and offline build;
- `CLAUDE.md`: extend the code-images resolver invariant with typed X-vs-generic
  dispatch, generic fetch-operation isolation, cache domains, no provider HTML,
  and `GenericEmbedCard`'s typed optional image path.

## Error contract and cache help

Every generic failure is an `ErrorKind::Asset` `BuildError` with the embed fence
line (translated through includes by the CLI), the normalized author page URL,
and actionable help. Required distinct failures are:

- page fetch failure and page larger than 8 MiB;
- missing discovery link, missing/invalid href, or non-HTTP(S) endpoint;
- discovered-endpoint fetch failure and JSON larger than 1 MiB;
- malformed JSON, wrong field types, unknown oEmbed type, or a photo missing
  both image fields;
- neither nonempty title nor nonempty author;
- non-HTTP(S) `thumbnail_url` or photo `url`;
- thumbnail fetch failure, bytes larger than 8 MiB, unsupported/wrong magic, or
  a cache extension/magic mismatch;
- cache directory/read/write/rename/cleanup failures; and
- any explicit `mode=` on a generic author URL.

Network errors identify the failed operation (page, discovered JSON endpoint,
or thumbnail) without suggesting Chrome. Generic errors never suggest X post
privacy or `publish.x.com`; X errors retain their current text. Cache help
states that valid JSON and required-thumbnail hits work offline, prints the
exact JSON and thumbnail candidate paths, and explains which files to delete
for metadata-only versus complete refresh.

## TDD task breakdown

Every task begins with the named red tests, makes the smallest production
change that turns them green, and reruns the focused filter before continuing.
Existing X compatibility assertions are kept in the focused set throughout.

1. **Typed URL dispatch and option presence — `Cargo.toml`, `Cargo.lock`,
   `crates/peitho-core/Cargo.toml`, `parser.rs`, `domain.rs`, and
   `code_images.rs`.** Red:
   `embed_block_dispatches_non_x_http_and_https_urls_to_generic`,
   `generic_page_url_normalizes_host_path_query_and_removes_fragment`,
   `embed_block_keeps_x_and_twitter_status_targets_unchanged`,
   `embed_block_rejects_non_status_x_before_generic_discovery`,
   `embed_block_rejects_x_and_twitter_subdomains_before_generic_discovery`,
   `embed_block_rejects_non_http_generic_urls`,
   `generic_embed_rejects_mode_card_and_mode_screenshot_at_fence_line`, and
   `bare_generic_embed_retains_absent_mode_for_static_card_dispatch`. Add
   `GenericPageUrl`, the enum arm,
   URL-crate normalization, X-domain/subdomain guard, and `EmbedOptions { mode:
   Option<EmbedMode> }`. Keep all existing option-grammar and X URL tests green.
   Run `cargo test -p peitho-core embed_block` and
   `cargo test -p peitho-core embed_info`.

2. **Attribute-based discovery — `generic_oembed.rs` and the supplied page
   fixtures.** Red:
   `discovery_extracts_real_mastodon_oembed_link_and_decodes_ampersands`,
   `discovery_matches_rel_and_type_in_any_attribute_order`,
   `discovery_resolves_root_and_path_relative_hrefs_against_page_url`,
   `discovery_uses_first_matching_link`,
   `discovery_ignores_nonmatching_link_elements`,
   `discovery_missing_link_is_line_numbered`, and
   `discovery_rejects_missing_href_or_non_http_endpoint`. Use the real
   57,886-byte Mastodon page plus small synthetic attribute-order/relative
   cases. Put the measured YouTube link tag inline; do not add its full page.
   Run `cargo test -p peitho-core discovery_`.

3. **Generic JSON validation, photo choice, and escaping —
   `generic_oembed.rs` and the supplied YouTube/Mastodon JSON fixtures.** Red:
   `youtube_video_oembed_builds_thumbnail_card_data`,
   `mastodon_rich_oembed_builds_text_card_without_reading_html`,
   `generic_oembed_accepts_photo_video_link_and_rich_types`,
   `photo_url_is_image_fallback_when_thumbnail_is_absent`,
   `generic_oembed_rejects_unknown_type_or_photo_without_image`,
   `generic_oembed_requires_nonempty_title_or_author`,
   `generic_oembed_rejects_non_http_thumbnail_and_photo_urls`,
   `generic_card_escapes_hostile_title_author_provider_alt_and_permalink`,
   `generic_card_permalink_is_always_normalized_author_url`, and
   `generic_card_plain_text_is_title_then_author`. Include hostile `html` with
   iframe/script/event attributes and assert none appears in output. Run
   `cargo test -p peitho-core generic_oembed_` and
   `cargo test -p peitho-core generic_card_`.

4. **Generic key identity and raw JSON cache — `generic_oembed.rs`,
   `code_images.rs`, and
   `crates/peitho-core/tests/support/generic_oembed_cache_key.rs` (new).** Red:
   `generic_json_and_thumbnail_keys_use_distinct_pinned_domains`,
   `generic_json_cache_miss_fetches_page_then_endpoint_once_and_writes_raw_bytes_atomically`,
   `generic_json_cache_hit_skips_discovery_and_endpoint_fetch`,
   `generic_json_cache_invalid_or_oversized_entry_self_heals`,
   `generic_json_fetch_over_limit_is_not_cached`, and
   `generic_json_fetch_failure_names_line_url_path_and_refresh`. Pin both exact
   64-hex vectors after the key code exists. Fixture fetchers record operation
   order; assert regular-file/size gates, exact-byte preservation, no temp
   residue, and no page cache. Run `cargo test -p peitho-core generic_json_cache`.

5. **Thumbnail magic, cache, and typed raw path — `generic_oembed.rs`,
   `code_images.rs`, and `domain.rs`.** Red:
   `thumbnail_cache_miss_fetches_once_and_writes_magic_derived_extension_atomically`,
   `thumbnail_cache_hit_skips_thumbnail_fetch`,
   `json_hit_with_thumbnail_miss_fetches_only_thumbnail`,
   `text_card_never_fetches_or_maps_thumbnail`,
   `thumbnail_cache_invalid_magic_or_extension_self_heals`,
   `thumbnail_magic_accepts_jpeg_png_webp_and_gif`,
   `thumbnail_fetch_rejects_oversize_or_unknown_magic_without_cache_write`,
   `thumbnail_refresh_removes_stale_extension_siblings`, and
   `embeds_cache_image_path_accepts_only_key_and_typed_format`. Use the real
   21,011-byte JPEG plus minimal signature fixtures for the other formats.
   Assert all error help enumerates concrete cache paths. Run
   `cargo test -p peitho-core thumbnail_`.

6. **Generic curl operations without X argv drift —
   `crates/peitho/src/main.rs`.** Before refactoring, strengthen the
   compatibility pin
   `x_oembed_curl_argv_remains_byte_identical_without_redirects`. Add red tests
   `generic_page_curl_uses_redirect_limit_protocol_guard_and_8_mib_cap`,
   `generic_endpoint_curl_uses_redirect_limit_and_1_mib_cap`,
   `generic_thumbnail_curl_uses_redirect_limit_and_8_mib_cap`,
   `generic_curl_sends_no_stdin_and_returns_complete_bytes`,
   `generic_curl_reports_redirect_http_exit_timeout_and_stderr`, and
   `generic_curl_reports_missing_curl`. Reuse `OEmbedCurlInvoker` and the
   one-shot process runner; do not route X through the new argv builder. Run
   `cargo test -p peitho oembed_curl` and `cargo test -p peitho generic_curl`.

7. **First-class fragment, mapping, resolver, and manifest text — `domain.rs`,
   `mapping.rs`, `check.rs`, `phase.rs`, `plain.rs`, and compiler-reported
   exhaustive sites.** Red:
   `source_fragment_generic_card_preserves_escaped_parts_and_plain_text`,
   `generic_card_maps_optional_image_src_exactly_once`,
   `generic_text_card_does_not_call_image_mapper`,
   `maps_generic_card_to_blocks_slot_beside_math_and_x_card`,
   `accepts_generic_card_only_in_blocks_slot`,
   `generic_thumbnail_resolves_to_hashed_asset_and_manifest_image`, and
   `generic_card_manifest_text_contains_title_and_author_only`. Add the typed
   variant/constructor and explicit arms; no renderer bypass or wildcard match.
   Run `cargo test -p peitho-core generic_card` and
   `cargo test -p peitho-core resolve_image_paths`.

8. **Owned HTML rendering and conditional CSS — `render.rs`,
   `assets/generic-embed-card.css`, and `embed_card.rs`.** First pin existing
   X-card HTML/CSS bytes with
   `x_card_only_render_keeps_issue_398_html_and_css_bytes`. Then add red tests
   `render_generic_thumbnail_card_uses_only_resolved_src_and_author_permalink`,
   `render_generic_text_card_omits_img_but_keeps_caption_and_link`,
   `render_generic_card_splices_between_markdown_runs`,
   `render_revealed_generic_card_stamps_outer_wrapper`,
   `generic_card_css_is_conditional_and_follows_base_card_css`,
   `math_x_and_generic_css_order_keeps_theme_last`, and
   `decks_without_generic_cards_keep_existing_css_bytes`. Keep the existing CSS
   asset untouched; assert the supplemental asset uses only existing card color
   and font variables. Run `cargo test -p peitho-core render_generic` and the
   X no-drift pin.

9. **Transform dispatch and panic-spy isolation — `code_images.rs`.** Red:
   `bare_generic_thumbnail_embed_uses_only_generic_fetch_operations`,
   `bare_generic_text_embed_never_calls_thumbnail_or_x_backends`,
   `generic_cache_hits_call_no_fetch_backend`,
   `generic_mode_error_calls_no_backend`,
   `x_screenshot_paths_never_call_any_oembed_operation`,
   `x_card_paths_never_call_generic_fetch_operations_or_chrome`, and
   `legacy_x_screenshot_and_card_fragments_match_issue_398_bytes`. Thread the
   new operations through recursive transforms, preserve reveal spans, and
   leave X cache/key/builder calls intact. Run
   `cargo test -p peitho-core builtin_generic_embed` and all existing
   `builtin_*embed*` tests.

10. **CLI offline integration and publish containment —
    `crates/peitho/tests/tweet_embeds.rs`.** Red:
    `build_cached_youtube_generic_embed_offline_without_chrome_or_curl`,
    `build_cached_mastodon_text_card_offline_without_image_asset`,
    `build_generic_json_hit_thumbnail_miss_fetches_only_image`,
    `build_generic_card_publishes_hashed_thumbnail_not_cache_path`,
    `build_generic_provider_html_never_enters_dist`,
    `build_generic_failures_report_translated_line_and_exact_refresh_files`, and
    `build_mixed_x_and_generic_embeds_touch_only_required_backends`. Assert raw
    JSON, discovery HTML, `.peitho`, provider iframe/script, and remote image
    URLs never enter `dist`; the thumbnail is under `assets/` and referenced by
    slide HTML/manifest; `peitho publish -- true` accepts the result. Retain all
    current tweet integration tests byte-for-byte. Run
    `cargo test -p peitho --test tweet_embeds`.

11. **Offline example and documentation — `examples/tweet-embed`, `README.md`,
    both site guides, the example page, and `CLAUDE.md`.** Red:
    `tweet_embed_example_builds_three_slides_offline_without_chrome_or_curl` and
    `tweet_embed_example_generic_slide_uses_committed_json_and_jpeg_caches`.
    Add slide 3 and the two computed cache files, keep slide 1 first/unchanged,
    then update the documented syntax, fallback, routing, security, cache, and
    refresh contracts. Build the example and demo-site fixture with a PATH that
    cannot find curl and a missing `PEITHO_CHROME_PATH`; slide 1 succeeds from
    its existing PNG, slide 2 from its existing X JSON, and slide 3 from its new
    generic JSON/JPEG. Run documentation link checks available in the
    repository.

## Post-plan hardening (review rounds, 2026-08-05)

Implementation review produced five security and coherence hardenings without
changing cache keys or rendered output bytes:

- Deliberate X-path deviation: `valid_cached_oembed_json` now uses
  `symlink_metadata`. A symlinked X JSON cache entry was a valid hit on main;
  it is now a miss. This is a security fix, while X cache keys and output bytes
  remain unchanged.
- Generic JSON lookup likewise treats a symlink as a miss. A successful fetch
  atomically replaces the symlink itself rather than following it; only an
  entry that blocks replacement, such as a directory, reports the exact
  cache-path error.
- X-domain comparison trims exactly one trailing dot, so `x.com.` and
  `www.x.com.` receive the unsupported-X status-form guidance while
  `example.com.` remains a generic target.
- Thumbnail cache lookup runs the regular-file, nonzero, and maximum-size
  metadata gates before any read, preventing an oversized sparse candidate
  from being loaded into memory.
- Unsupported-X and empty-block help now describe the shipped generic oEmbed
  path: X requires a status URL, while any other HTTP(S) page uses discovery.
  Generic providers are no longer described as a future follow-up.

## Final verification

After the focused red/green loop for every task:

1. Run three consecutive `cargo test --workspace` passes to catch shared cache
   or process-runner leakage.
2. Run `cargo fmt --all --check` and
   `cargo clippy --workspace --all-targets -- -D warnings`.
3. Run `git diff --exit-code bindings/`; no TypeScript contract drift is
   expected because the new fragment remains an internal Rust typestate.
4. Build cached X-screenshot-only, X-card-only, YouTube-thumbnail-only,
   Mastodon-text-only, and mixed decks. Compare the first two against checked-in
   #398 HTML/CSS/assets and curl-argv pins.
5. Build, preview, export PDF, and publish the offline three-slide example.
   Confirm no Chrome/curl process is started on complete cache hits, no remote
   request occurs, and no `.peitho` or JSON file enters `dist`.
6. Inspect thumbnail and text cards in a real browser: the thumbnail loads from
   a hashed local asset, text is selectable, captions inherit the deck theme,
   links point to the normalized author-supplied URL, and no
   iframe/script/provider HTML exists in the DOM.

## Edge cases

Every error below is line-numbered and carries actionable X-only, discovery,
cache, network, or layout help as appropriate.

| Case | Behavior |
|---|---|
| Bare valid X/Twitter status URL | Existing screenshot branch, cache key, PNG, alt text, routing, and bytes. |
| X status with `mode=screenshot` | Same existing screenshot bytes as the implicit default. |
| X status with `mode=card` | Same existing X JSON cache, extraction, HTML, CSS, and no-redirect curl argv. |
| x.com/twitter.com non-status URL or any subdomain such as www.x.com/mobile.twitter.com | Existing unsupported-X error with status-URL help; never fetch discovery HTML. |
| Trailing-dot FQDN host | `x.com.`, `www.x.com.`, and `twitter.com.` remain behind the X guard; a non-X host such as `example.com.` dispatches as `Generic`. |
| Bare non-X HTTP(S) URL | Normalize it and build a generic static card. |
| Non-X URL with either `mode=card` or `mode=screenshot` | Error at the opening-fence line; both modes are X-only and no backend is called. |
| Malformed URL or non-HTTP(S) scheme | Error with supported X forms plus generic HTTP(S)-page help. |
| Author URL contains a fragment | Remove the fragment from fetch/cache/permalink identity; retain path and query. |
| Explicit `code_images.embed` override | Continue rejecting `mode=`; a bare fence body remains verbatim stdin on the external SVG/image path. |
| Discovery page redirects | Follow HTTP(S) redirects only, at most five. |
| Redirect limit exceeded or redirect leaves HTTP(S) | Line-numbered page/endpoint/thumbnail fetch error naming the operation. |
| Discovery page is 1.33 MiB YouTube HTML | Accepted below the independent 8 MiB page cap. |
| Discovery page exceeds 8 MiB | Error before parsing; do not cache the page. |
| Link attributes appear in any order | Match by decoded attributes, not serialized tag shape. |
| `rel` contains multiple tokens | Match when one ASCII-case-insensitive token is `alternate`. |
| Multiple matching discovery links | The first rel/type match wins; a broken first match is an error rather than fallback to a later link. |
| Entity-encoded or relative discovery href | Decode once, then resolve against the normalized author-supplied page URL. |
| Missing discovery link/href or unsafe endpoint | Error naming the required JSON oEmbed link and author-supplied URL. |
| Generic JSON cache is valid | Skip page and endpoint fetches; continue directly to card/image cache resolution. |
| Generic JSON cache is malformed, semantically invalid, oversized, unreadable, or a symlink | Treat as a miss; a successful bounded fetch atomically self-heals corrupt regular data or replaces the symlink itself without following it. Only a rename-blocking entry such as a directory reports the exact cache error. |
| Discovered JSON exceeds 1 MiB or is invalid | Error without overwriting the cache. |
| `type` is photo/video/link/rich or absent | Accept; unknown present values are invalid data. |
| Both title and author are absent/blank | Error even when provider name or HTML exists. |
| `thumbnail_url` is present | Prefer it for every type and require HTTP(S). |
| Photo has no thumbnail but has HTTP(S) `url` | Use `url` as the image source, never as the card permalink. |
| Photo has neither thumbnail nor `url` | Error as an invalid photo response. |
| Non-photo has no thumbnail | Emit the author-approved text card and make no image request. |
| Mastodon fixture | Emit author/provider plus the author-supplied permalink; ignore its script/blockquote HTML and emit no image. |
| Provider fields contain markup, quotes, ampersands, or control-looking text | Escape every retained text/attribute value; provider HTML remains wholly ignored. |
| Generic JSON and thumbnail caches both hit | Build offline with no curl or Chrome. |
| JSON hits but required thumbnail misses | Fetch only the thumbnail URL from cached JSON. |
| Thumbnail cache is oversized, wrong-extension, wrong-magic, empty, or corrupt | Gate regular-file status, nonzero size, and maximum size before any read; then miss and replace atomically after a successful valid fetch. |
| Thumbnail bytes are JPEG/PNG/WebP/GIF | Detect by magic and use canonical `.jpg`/`.png`/`.webp`/`.gif`. |
| Thumbnail bytes use any other format or exceed 8 MiB | Line-numbered error; do not create a cache/image fragment. |
| Stale alternate-extension siblings exist | A successful image refresh removes them and leaves one magic-consistent cache file. |
| Thumbnail card reaches image resolution | Copy the cache file to a hashed `dist/assets/` path and expose only the resolved path to rendering/manifest. |
| Text card reaches image resolution | Preserve `image: None` and never call `ImageResolver`. |
| Layout has only image slots | Generic card routes as Blocks and fails through the normal accepts error. |
| Generic card is inside `::: {reveal}` | Preserve its reveal span and stamp the owned outer wrapper. |
| Deck contains no generic cards | Emit no supplemental generic CSS; preserve all existing CSS bytes. |
| X-card-only deck | Emit the original base card CSS only and preserve #398 bytes. |
| Cache refresh | No TTL. Delete the exact JSON path for metadata; delete JSON plus the explicitly named image candidates for a complete image refresh. |

## Out of scope

- Per-provider rich extraction, including Mastodon status text through an
  instance API.
- Screenshot mode for generic URLs or any heuristic generic screenshot
  completion signal.
- Video/audio playback, provider iframe activation, or browser-side provider
  scripts.
- Curated provider registries, provider allowlists, or provider-specific
  endpoint templates.
- Open Graph enrichment (`og:image`, `og:description`, or other page metadata)
  when oEmbed fields are sparse.
- Injecting any provider `html`, sanitizing provider HTML for partial reuse, or
  extracting fallback prose from it.
- Cache TTLs, conditional requests, automatic refresh, or a refresh command.
- Parallel discovery/image fetches, data-URI thumbnails, SVG thumbnails, new
  fonts, or provider logos.

## Summary

<!-- derived-from #goal -->
<!-- derived-from #author-decisions-settled-2026-08-05 -->
<!-- derived-from #measured-facts-2026-08-05 -->
<!-- derived-from #approach -->
<!-- derived-from #error-contract-and-cache-help -->
<!-- derived-from #tdd-task-breakdown -->
<!-- derived-from #post-plan-hardening-review-rounds-2026-08-05 -->
<!-- derived-from #final-verification -->
<!-- derived-from #edge-cases -->
<!-- derived-from #out-of-scope -->

Issue #399 adds one typed `EmbedTarget::Generic` branch behind the unchanged X
dispatcher. It discovers JSON oEmbed endpoints from author pages, caches and
validates raw JSON plus optional magic-checked image bytes, constructs only
escaped Peitho-owned cards, and carries thumbnails through the existing typed
image resolver into hashed publishable assets. Bare generic URLs always use
this static-card path, while every X mode and byte-level compatibility contract
from #398 remains intact.
