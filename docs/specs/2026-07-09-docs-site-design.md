# Docs site for peitho.gosu.ke

Date: 2026-07-09
Status: approved by the author (stack, language, structure, and URL scheme confirmed in conversation)

## Goal

Turn https://peitho.gosu.ke/ from a bare list of demo decks into a full
documentation site: a landing page that explains what peitho is, a guide that
teaches how to use it, and an examples section that showcases the existing
demo decks. Comparable in richness to the documentation sites of similar
tools, while keeping peitho's own identity.

## Chosen approach: Zola with custom templates

Static site generator: [Zola](https://www.getzola.org/). Rationale, driven by
peitho's own requirements rather than by what similar tools use:

- **Full control over design.** The site inherits the hand-crafted visual
  identity of the current `demo/index.html` (serif typography, warm paper
  background, thin rules). Zola templates are plain HTML + CSS (Tera), so the
  existing page style extends naturally to the whole site. This also matches
  peitho's own pillars: HTML-native, git-manageable HTML/CSS.
- **Rust toolchain.** Zola is a single Rust binary; CI installs one tool. No
  new package ecosystem is introduced for the site.
- **syntect built in.** Zola highlights code with syntect — the same
  highlighter peitho itself uses — so code samples on the docs site and
  peitho's own output stay consistent.
- **Right-sized.** The site is 15–25 pages. Heavier docs frameworks
  (VitePress, Starlight) earn their weight through default themes and
  built-in search; we are replacing the theme anyway, and search can be added
  later with Pagefind (fully static, non-invasive) if the page count grows.

Rejected alternatives:

- **VitePress / Astro Starlight**: default-theme value is lost the moment we
  keep our own design; deep theme customization means working in Vue/Astro
  components rather than plain HTML/CSS.
- **Hand-rolled static HTML** (status quo): no shared layout, no Markdown
  pipeline; every page duplicates boilerplate and drifts.

## Key decisions

### Language: English first, Japanese-ready

The site is English-only at launch, consistent with the project's
English-only rule. The content tree uses Zola's multilingual conventions from
day one (`default_language = "en"`; pages are `content/**/pagename.md`), so
Japanese can be added later non-destructively by dropping
`pagename.ja.md` files next to the English ones and enabling
`[languages.ja]` in `config.toml` — English URLs do not change.

### Content structure: three pillars

1. **Landing page** (`/`) — hero (one-line statement of what peitho is), the
   three pillars (separation of content and design; git-manageable HTML/CSS
   layouts; type-checked slot contracts), install one-liner, and entry points
   into the guide and examples. Carries over the current `demo/index.html`
   aesthetic.
2. **Guide** (`/guide/…`) — task-oriented documentation, reconstructed from
   README.md and existing design records:
   - Getting Started — install (Homebrew / prebuilt binaries), first deck,
     `preview`, `present`
   - Writing Decks — slide splitting, convention mapping, explicit slot
     syntax (`::: {slot=name}`), speaker notes, page settings comments,
     agenda sections
   - Layouts — the layout-as-schema idea, `<slot name accepts arity>`,
     multiple layouts and hybrid dispatch, keyed CSS overrides
   - Frontmatter — reference for every supported key (`time`,
     `aspect_ratio`, `resolution`, `layouts`, `css`, `syntaxes`, `fonts`),
     asset resolution order, error behavior
   - CLI — `build`, `preview`, `present`, `export`, `publish`,
     `completions`; flags and the daily editing loop
3. **Examples** (`/examples/…`) — one page per demo deck (8 today): what the
   deck demonstrates, a link to the live demo under `/demo/<name>/`, and the
   deck's `deck.md` source (or the instructive excerpt) with highlighting.

The guide documents *usage*; internal design records stay in `docs/` in the
repository and are not part of the site.

### URL scheme: decks move under /demo/

Built decks move from the site root (`/feature-tour/`) to
`/demo/<name>/`. The root namespace belongs to the docs site (`/guide/…`,
`/examples/…`), which removes any chance of collision between future doc
pages and deck names.

Old URLs keep working via a Cloudflare Pages `_redirects` file with one rule
per deck (e.g. `/feature-tour/* /demo/feature-tour/:splat 301`). A wildcard
single rule is not possible because the old deck paths live at the root, so
the redirect list is explicit — one line per existing deck, maintained
alongside the deck list in the Makefile.

### Build composition: one static tree, same deploy flow

`make demo-site` remains the single entry point and now composes two builds
into `.demo-site/`:

1. `zola build` in `site/` → output to `.demo-site/` (landing, guide,
   examples, `_redirects`)
2. `peitho build examples/<name>/deck.md --out .demo-site/demo/<name>` for
   each deck, followed by the existing `peitho publish` contamination check

The deploy workflow (`.github/workflows/deploy-demo.yml`) gains one step:
install Zola (single binary, version pinned in the workflow). Everything else
— Cloudflare Pages project, main-push deploy, PR preview deploys and
cleanup — stays as designed in
`docs/plans/2026-07-09-demo-site-deploy-flow.md`.

`demo/index.html` is deleted; the landing page replaces it. The Japanese text
in that file (a leftover from the English-only migration, PR #199) disappears
with it.

## File structure

```
site/
  config.toml          # default_language = "en", syntect highlighting on
  content/
    _index.md          # landing page content
    guide/
      _index.md        # guide index / sidebar root
      getting-started.md
      writing-decks.md
      layouts.md
      frontmatter.md
      cli.md
    examples/
      _index.md        # examples index (the gallery, successor of today's list)
      feature-tour.md
      keynote.md
      code-walkthrough.md
      lightning-talk.md
      two-column.md
      image-showcase.md
      aspect-ratio-4-3.md
      minimal.md
  templates/
    base.html          # shared shell: header nav, footer, styles
    index.html         # landing
    guide.html / guide-page.html
    examples.html / example-page.html
  static/
    _redirects         # old root deck URLs → /demo/<name>/
    (css, any images)
```

Notes:

- Templates and CSS are hand-written and carry the current visual identity;
  no third-party Zola theme is used.
- Deck sources shown on example pages are included from `examples/` at build
  time (Zola `load_data` or a documented copy step) rather than pasted, so
  they cannot drift from the real decks. The exact mechanism is an
  implementation-plan decision; the invariant is: **example sources on the
  site must come from `examples/`, not be maintained by hand.**

## Edge cases and constraints

- **Deck/doc URL collisions**: prevented structurally by the `/demo/` prefix.
- **Redirects must not be forgotten when adding a deck**: adding an example
  already requires touching the Makefile deck list; the `_redirects` file
  only lists *pre-move* decks (the 8 that shipped at root), so it is a
  frozen, never-growing list. New decks are born under `/demo/` and need no
  redirect.
- **English-only rule**: all site content, templates, and this design doc are
  English. Future Japanese content is an explicit, additive step.
- **Publish contamination check**: unchanged — it runs per-deck against
  `.demo-site/demo/<name>` exactly as it runs today against
  `.demo-site/<name>`.
- **Zola version pinning**: the workflow pins an exact Zola version to keep
  CI reproducible; local builds document the same version in the Makefile.
- **No auto-listing regression**: adding an example still requires manual
  edits (Makefile target + a new `site/content/examples/<name>.md`). This
  matches the current documented behavior; auto-listing is out of scope.

## Out of scope

- Full-text search (add Pagefind later if needed)
- Japanese translation (structure is ready; content is not written now)
- Auto-discovery of example decks
- Publishing real (non-example) decks to peitho.gosu.ke (tracked separately
  in CLAUDE.md "Undecided")
