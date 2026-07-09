# peitho

An HTML-native presentation tool that treats Markdown as the source of truth.

Peitho is the Greek goddess who presides over the power to move people's hearts with words rather than force — a fitting match for the essence of presentation.

Docs & demos: **[peitho.gosu.ke](https://peitho.gosu.ke/)** — a guide, an examples gallery, and the example decks built and served under `/demo/<name>/`. Auto-deployed on push to `main`; `envchain peitho make deploy-demo` for manual deploys.

## Features

- **Separation of content and design** — content is Markdown, design is layout HTML and CSS. The two are never mixed
- **Git-manageable layouts** — design artifacts are just HTML/CSS. They diff and review cleanly
- **Type-checked slot contracts** — the layout itself is the schema. Missing or extra slots, type mismatches, dangling references, and leftover content become build errors with line numbers and hints instead of being silently dropped

```
error: slide 2 ('code-slide'), line 7: slot 'code' got 2 item(s), but layout 'title-body-code' allows 0..1
  = help: use a layout with more code capacity or remove one code block
```

- **Per-slide tweaks anchored on stable keys** — pin a key with `<!-- {"key":"arch-1"} -->` and target it from CSS. Edit the title and the CSS still holds; target a key that doesn't exist and the build stops

```css
[data-slide-key="arch-1"] .slot-code {
  grid-column: 2 / 3;
  width: 60%;
}
```

- **Build-time syntax highlighting** — code blocks with a language tag are turned into `hl-*` class spans by syntect at build time. There is no runtime JS; colors are defined in theme CSS. An unknown language tag is a build error with a line number (no tag means plain rendering)
- **Time tracking with agenda sections** — declare a planned time in frontmatter (`time: 15m`) and, optionally, per-section budgets on page-settings comments (`{"section":"Setup","time":"1m"}`). The presenter agenda shows planned vs. actual per section in real time. Section totals must equal the deck's planned time — mismatches are build errors with line numbers
- **Speaker notes as HTML comments** — non-JSON HTML comments in a slide body become that slide's speaker note (Marp / k1LoW/deck-style). Notes ride only into the presenter view; `dist/` never contains them (the publish contamination check enforces this)
- **One-command preview loop** — `peitho preview` watches the deck and assets, rebuilds into a volatile preview cache, serves it locally, and reloads the browser while preserving the current slide and overview mode
- **Keynote-style presenting** — `peitho present` puts the slides full-screen on an external display and automatically places a presenter view (current/next slide, notes, timer) on your machine. Space starts/pauses the timer; arrows navigate; Esc closes everything

![Presenter view: current and next slide, speaker notes, timer with slide progress, and a per-section agenda](docs/images/presenter-view.png)

## Install

### Homebrew (macOS / Linux)

```sh
brew install mizzy/tap/peitho
```

Shell completions for bash/zsh/fish are installed automatically.

### Prebuilt binaries

Grab a prebuilt binary from the [Releases page](https://github.com/mizzy/peitho/releases). Each release ships a tarball per target with a single `peitho` binary — everything (layouts, base theme, presentation shell) is embedded, so Node.js and npm are not needed at runtime.

```sh
# macOS arm64 (Apple Silicon) — replace vX.Y.Z with the version you want
curl -LO https://github.com/mizzy/peitho/releases/download/vX.Y.Z/peitho-vX.Y.Z-aarch64-apple-darwin.tar.gz
curl -LO https://github.com/mizzy/peitho/releases/download/vX.Y.Z/peitho-vX.Y.Z-aarch64-apple-darwin.tar.gz.sha256
shasum -a 256 -c peitho-vX.Y.Z-aarch64-apple-darwin.tar.gz.sha256
tar xzf peitho-vX.Y.Z-aarch64-apple-darwin.tar.gz
mv peitho-vX.Y.Z-aarch64-apple-darwin/peitho /usr/local/bin/
```

Available targets: `aarch64-apple-darwin`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu` (Intel Mac is currently not supported because the GitHub Actions macos-13 runner queue wait times are too long). Or build from source with `cargo install --path crates/peitho` after cloning (run `cd packages/peitho-present && npm ci && npm run build` first so the shell is up to date).

## Usage

The deck argument defaults to `deck.md` in the current directory, so it can be omitted when the file follows the convention.

```sh
# Generate the distribution (dist/ with slides/ fragments + manifest.json + index.html + peitho.css)
peitho build            # same as: peitho build deck.md

# Daily editing loop: watch, serve, open, and reload on every successful rebuild
peitho preview

# Preview controls: o, Enter, or Esc toggles single-slide and tile overview modes; arrows move horizontally and vertically in overview; click opens the selected tile

# Rebuild on every save for an external server or pipeline
peitho build --watch

# Present (generates a volatile cache + local server + launches the browser. Auto-places across two displays)
peitho present

# Debug: open in a normal window instead of full-screen (Chrome restores the previous position/size. On a single display the slides open in a window too)
peitho present --presenter-windowed

# Export a PDF
peitho export pdf -o deck.pdf

# A deck with a non-default name is passed explicitly
peitho build slides.md

# Publish (inspects, then delegates to your existing deploy command. Don't reinvent the deploy)
peitho publish -- aws s3 sync dist/ s3://your-bucket/

# Generate a shell completion script (bash / zsh / fish / powershell / elvish)
peitho completions zsh
```

Layouts, themes, and the presentation shell use defaults embedded in the binary, so a single deck file works in any directory. Point at your own assets from the deck's frontmatter (`layouts:`, `css:`, `syntaxes:`, `fonts:`) or drop `layouts/`, `css/`, `syntaxes/`, and `fonts/` next to the deck for zero-config pickup. Only `--shell` remains as a CLI-side dev/debug swap for the presentation shell bundle itself.

When developing the presentation shell (TS), rebuild `dist/shell.js` and `dist/preview.js` with `cd packages/peitho-present && npm ci && npm run build` (both are committed; CI checks for drift).

## Writing a deck

Convention mapping turns plain Markdown into slides as-is. Slides are separated by `---`, the shallowest heading is the title, code blocks go to the code slot, and the rest goes to the body. Deck-level settings live in YAML frontmatter at the top; per-slide settings live in a `<!-- { ... } -->` JSON comment; non-JSON HTML comments become speaker notes.

````markdown
---
time: 15m
---

<!-- {"key":"intro","section":"Setup","time":"3m"} -->
# Title

A body paragraph.

<!-- Speaker note: introduce yourself, then move fast. -->

- Lists work
- too

---

<!-- {"section":"Deep dive","time":"12m"} -->
# Next slide

```rust
enum Phase { Parsed, Mapped, Checked, Rendered }
```
````

Markdown images are local files written as an image-only paragraph:

```markdown
![Architecture diagram](img/arch.png)
```

Image paths are deck-relative and must use supported local image extensions (`png`, `jpg`, `jpeg`, `gif`, `webp`). Remote URLs, absolute paths, parent-directory escapes, query strings, fragments, and backslash separators are build errors. A slide with an image must map to a layout with exactly one unambiguous `accepts="image"` slot; style the rendered `<img>` through normal layout CSS, for example `.slot-hero img { max-width: 100%; }`.

## Deck frontmatter

All deck-intrinsic settings live in YAML frontmatter at the top of the deck. Supported keys:

| Key | Purpose | Value |
|---|---|---|
| `time` | Planned presentation time | `15m` / `90s` / `1h30m` / bare integer (minutes) |
| `aspect_ratio` | Slide canvas aspect ratio | `16:9` (default) / `4:3` |
| `resolution` | PDF-only physical page size | `WxH` CSS px, e.g. `1920x1080` (must match `aspect_ratio`) |
| `layouts` | Layout HTML file or directory | Deck-relative path, e.g. `./layouts` |
| `css` | Theme CSS file or directory | Deck-relative path, e.g. `./css` |
| `syntaxes` | Custom syntect syntaxes | Deck-relative path, e.g. `./syntaxes` |
| `fonts` | Font files copied into the output | Deck-relative path, e.g. `./fonts` |

Absent asset keys fall back to a deck-adjacent directory of the same name (zero-config), then to the binary's built-in default (fonts simply add nothing when absent). A key that points at a non-existent path is a build error with the frontmatter line number. Asset values may be a file or a directory: `layouts`/`css`/`syntaxes` read `*.html` / `*.css` / `*.sublime-syntax` in filename order, while `fonts` copies files verbatim without an extension filter, so `.woff2`, `.ttf`, and `@font-face` CSS files can sit side by side.

## Custom syntaxes

Point `syntaxes:` in the deck's frontmatter at a `.sublime-syntax` file or a directory of `*.sublime-syntax` files, or drop a `syntaxes/` directory next to the deck for zero-config pickup. Both augment the built-in set, so built-in tags like `rust` and `js` still work. Unknown language tags remain build errors with line numbers.

## Multiple layouts

Point `layouts:` in the deck's frontmatter at an HTML file or a directory of `*.html` files; a directory turns every `*.html` inside it into a layout (name is the file stem, order is deterministic by filename). Zero-config: a `layouts/` directory next to the deck is picked up automatically. Each slide's layout is chosen in the following order (a hybrid approach inspired by the page settings in [k1LoW/deck](https://github.com/k1LoW/deck)):

1. **Explicit** — if a page-settings comment `<!-- {"layout":"cover"} -->` is present, use that layout (an unknown name is a build error with a candidate list)
2. **Single layout, unconditional** — if there is only one layout, always use it (contract violations still error with line numbers, as usual)
3. **Type-driven dispatch** — with multiple layouts, each slide is routed to the layout whose slot contract matches the shape of its content (title only / has body / has code, etc.). Exactly one match is required; **multiple matches (ambiguous) and zero matches are both build errors** rather than silently resolved, prompting an explicit choice

## Examples

`examples/` holds samples that differ entirely in content, layout structure, and theme. Each directory is self-contained: `deck.md`, plus `layouts/` and `css/` when the deck brings its own design. All of them except the `pdf-export` fixture are built and browsable on the [examples gallery](https://peitho.gosu.ke/examples/).

| Sample | Content | Design | Contract highlight |
|---|---|---|---|
| `examples/minimal/` | Minimal demo | Default theme | Works as-is with built-in defaults |
| `examples/lightning-talk/` | Five-minute LT on decks-as-code | Dark, poster-style with large type | No code slot — writing code is a build error |
| `examples/code-walkthrough/` | Rust typestate walkthrough | Terminal-style two-column | `code` has `arity="1"` — every slide requires code. A practical keyed-override example |
| `examples/keynote/` | Product keynote | Cream background, serif, centered | Two-layout setup. Title-only slides go to `cover`, slides with a body go to `statement` via type-driven dispatch |
| `examples/peitho-tour/` | Peitho's own product tour | Dark space theme with cyan/purple accents | Four layouts (`cover`, `topic`, `code`, `shot`), a full six-section agenda, an image slide that lands on the `shot` layout by type-driven dispatch, and multi-comment speaker notes throughout |
| `examples/two-column/` | Explicit slot syntax demo | Two-column layout | `::: {slot=left}` / `::: {slot=right}` route content where convention mapping can't decide between two `accepts="blocks"` slots |
| `examples/image-showcase/` | Markdown image slide | Framed visual layout | `accepts="image"` receives `![alt](img/arch.png)` and CSS styles `.image-showcase img` |
| `examples/aspect-ratio-4-3/` | 4:3 canvas demo | Default theme | `aspect_ratio: 4:3` frontmatter switches the slide canvas to 960x720 |
| `examples/pdf-export/` | PDF export fixture | Default theme | `aspect_ratio` + `resolution` frontmatter set the PDF page size; speaker notes stay out of the exported PDF |

Same tool, same Markdown conventions — entirely different decks:

| | |
|---|---|
| ![Minimal demo with the default theme](docs/images/example-minimal.png) | ![Lightning talk: dark poster-style with large type](docs/images/example-lightning-talk.png) |
| ![Rust typestate walkthrough: terminal-style two-column](docs/images/example-code-walkthrough.png) | ![Keynote: cream background, serif, centered](docs/images/example-keynote.png) |

The Peitho tour turns the tool on itself — one deck walking through the concept, three pillars, and the write/preview/present loop across four custom layouts, type-driven dispatch, agenda sections, and speaker notes:

![Peitho tour: dark space theme with cyan and purple accents](docs/images/example-peitho-tour.png)

```sh
# Each sample has its layouts/ and css/ alongside it, so no flags are needed by convention
peitho present examples/keynote/deck.md
```

The Makefile targets are handy for smoke-testing (`make help` for a list; `make keynote`, `make lightning-talk`, etc. They `cargo run` including the shell-bundle build).

## Architecture

```
Markdown ─→ peitho build (parse, map, 4-stage check. Deterministic, pure functions)
              ├─ emit distribute → dist/ (distribution only; no shell or notes mixed in)
              ├─ emit preview    → .peitho/preview-cache/ (preview shell; volatile)
              └─ emit present    → .peitho/present-cache/ (presentation shell; volatile)
```

- The build core is Rust (typestate: `Parsed→Mapped→Checked→Rendered`. Unchecked slides can't reach the renderer)
- The presentation shell is TypeScript. The contract (domain types like the manifest) has Rust as its single source; TS types are generated into `bindings/` and CI checks for drift
- See `docs/PEITHO_KICKOFF.md` for the detailed design

## License

MIT
