# Code block titles (Issue #719)

## Goal

````markdown
```hcl title="main.tf"
name = upper(aws_vpc.main.id)
```
````

renders a caption attached to its own block:

```html
<figure class="code-block">
  <figcaption class="code-title">main.tf</figcaption>
  <pre class="slot-code"><code class="language-hcl">…</code></pre>
</figure>
```

Works with line emphasis in any order (```` ```ts {2} title="app.ts" ```` and
```` ```ts title="app.ts" {2} ````).

## Design

### One typed parse of the fence info string

`emphasis::split_info_string` returned `{language, emphasis, tail}` where `tail`
was an untouched string that each consumer re-tokenized (embed options) or
rejected. A title would have been a third positional special case. Instead the
info string is parsed once (new module `fence_info.rs`) into:

```rust
pub(crate) struct FenceInfo<'a> {
    pub(crate) language: Option<&'a str>,
    pub(crate) attributes: FenceAttributes<'a>,
}

pub(crate) struct FenceAttributes<'a> {
    pub(crate) emphasis: Option<&'a str>,      // inside the one `{…}` group
    pub(crate) title: Option<CodeTitle>,
    pub(crate) options: Vec<FenceOption<'a>>,  // remaining tokens, renderer-owned
}
```

The language split is infallible and runs first, so the unknown-language error
keeps priority over every attribute error.

Grammar after the positional language token (unchanged: first token unless it
starts with `{`):

- `{…}` — the emphasis group (may contain whitespace); at most one.
- `key="value"` — quoted value, may contain spaces, no escapes; an unclosed
  quote is an error.
- `key=value` — bare value.
- anything else — the existing `unexpected text after the code language` error.

`title` is claimed by the parser itself (at most once, non-empty after trim).
Every other key is handed to the renderer as a typed `FenceOption { key, value }`:
the built-in embed renderer consumes `mode`, an explicit `code_images.embed`
override keeps its existing "mode= options require the built-in embed renderer"
error, and every other consumer rejects any option with the existing
`unexpected text after the code language` error (its help now also mentions
`title="..."`). Error messages for existing inputs are unchanged except for one deliberate grammar
widening: option values may be double-quoted for every key through the shared
tokenizer, so `mode="card"` is equivalent to `mode=card`. Glued tokens are
errors rather than being partially consumed.

### `CodeTitle` rides the fragment

`CodeTitle` is a newtype (non-empty, trimmed) defined in `fence_info.rs` with a
constructor private to that module, so only fence-info parsing can produce one.
It is attached only through the Code-only constructor
`SourceFragment::titled_code`, so a title cannot sit on a non-code fragment, and
is carried Parsed→Mapped→Checked. It never enters the manifest or `bindings/`.

A title is a build error where it could not be shown:

- on a `code_images` block (any renderer — it renders to an image, not a code
  block), mirroring the emphasis error;
- on a code fence inside a list / blockquote / table (pulldown-cmark renders
  those), mirroring the emphasis error.

### One render seam

`render_code_block` is the only place a `<pre>` for a code fragment is emitted;
it takes `Option<&CodeTitle>`. Untitled blocks render byte-identically. A titled
block wraps in `<figure class="code-block">`, the caption is HTML-escaped text,
and a reveal step moves from the `<pre>` to the `<figure>` so the caption
appears with its block. The `slot-*` class stays on the `<pre>` so existing
theme code styling applies unchanged.

The UA gives `<figure>` a `1em 40px` margin, which would surprise every theme.
Following the static-emphasis precedent, an embedded `assets/code-title.css`
(margin reset + modest caption defaults) is placed at the theme-prelude seam only
when some slide carries a titled block; decks without titles keep their CSS bytes.

### lint

The caption is a text node inside the slide; `lint_measure.js` already walks
every text node, so overflow and font-size checks cover it with no change.

## Tasks

1. `fence_info.rs` + unit tests (order independence, quoting, errors).
2. Parser: use `FenceInfo`, attach `CodeTitle`, reject title on renderer blocks
   and in containers; embed options take `&[FenceOption]`.
3. Domain: `CodeTitle`, `SourceFragment` field + accessor.
4. Render: `render_code_block` figure, reveal on figure, conditional CSS.
5. Guide (`site/content/guide/writing-decks.md`) and CLAUDE.md bullet.
