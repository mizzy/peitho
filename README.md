# peitho

An HTML-native presentation tool that treats Markdown as the source of truth.

Peitho is the Greek goddess who presides over the power to move people's hearts with words rather than force — a fitting match for the essence of presentation.

Demo: **[peitho.gosu.ke](https://peitho.gosu.ke/)** (builds and serves `examples/` as-is. Deploy with `envchain peitho make deploy-demo`)

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
- **Keynote-style presenting** — `peitho present` puts the slides full-screen on an external display and automatically places a presenter view (current/next slide, notes, timer) on your machine. Space starts/pauses the timer; arrows navigate; Esc closes everything

![Presenter view: current and next slide, speaker notes, timer with slide progress, and a per-section agenda](docs/images/presenter-view.png)

## Usage

```sh
# Generate the distribution (dist/ with slides/ fragments + manifest.json + index.html + peitho.css)
peitho build deck.md

# Rebuild on every save
peitho build deck.md --watch

# Present (generates a volatile cache + local server + launches the browser. Auto-places across two displays)
peitho present deck.md

# Debug: open in a normal window instead of full-screen (Chrome restores the previous position/size. On a single display the slides open in a window too)
peitho present deck.md --presenter-windowed

# Publish (inspects, then delegates to your existing deploy command. Don't reinvent the deploy)
peitho publish -- aws s3 sync dist/ s3://your-bucket/
```

Layouts, themes, and the presentation shell use defaults embedded in the binary, so a single deck file works in any directory. Pass `--layout`/`--base-css`/`--overrides-css`/`--shell` only when you want to swap them out.

When developing the presentation shell (TS), rebuild `dist/shell.js` with `cd packages/peitho-present && npm ci && npm run build` (it's committed; CI checks for drift).

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

## Multiple layouts

Passing a directory to `--layouts` turns every `*.html` inside it into a layout (name is the file stem, order is deterministic by filename). Each slide's layout is chosen in the following order (a hybrid approach inspired by the page settings in [k1LoW/deck](https://github.com/k1LoW/deck)):

1. **Explicit** — if a page-settings comment `<!-- {"layout":"cover"} -->` is present, use that layout (an unknown name is a build error with a candidate list)
2. **Single layout, unconditional** — if there is only one layout, always use it (contract violations still error with line numbers, as usual)
3. **Type-driven dispatch** — with multiple layouts, each slide is routed to the layout whose slot contract matches the shape of its content (title only / has body / has code, etc.). Exactly one match is required; **multiple matches (ambiguous) and zero matches are both build errors** rather than silently resolved, prompting an explicit choice

## Examples

`examples/` holds samples that differ entirely in content, layout structure, and theme. Each directory is self-contained with `deck.md` + `layouts/` + `css/`.

| Sample | Content | Design | Contract highlight |
|---|---|---|---|
| `examples/deck.md` | Minimal demo | Default theme | Works as-is with default flags |
| `examples/lightning-talk/` | Japanese LT | Dark, poster-style with large type | No code slot — writing code is a build error |
| `examples/code-walkthrough/` | Rust typestate walkthrough | Terminal-style two-column | `code` has `arity="1"` — every slide requires code. A practical keyed-override example |
| `examples/keynote/` | Japanese keynote | Cream background, serif, centered | Two-layout setup. Title-only slides go to `cover`, slides with a body go to `statement` via type-driven dispatch |
| `examples/feature-tour/` | Peitho's own feature tour | Light product-tour style, indigo accents | Four layouts. One slide is deliberately ambiguous between two of them and resolves it with an explicit `{"layout":"agenda"}`; also exercises the `list` slot type, multi-language highlighting, sections, and multi-comment speaker notes |

Same tool, same Markdown conventions — entirely different decks:

| | |
|---|---|
| ![Minimal demo with the default theme](docs/images/example-minimal.png) | ![Japanese lightning talk: dark poster-style with large type](docs/images/example-lightning-talk.png) |
| ![Rust typestate walkthrough: terminal-style two-column](docs/images/example-code-walkthrough.png) | ![Japanese keynote: cream background, serif, centered](docs/images/example-keynote.png) |

The feature tour turns the tool on itself — one deck exercising explicit layout requests, the `list` slot type, multi-language highlighting, sections, and speaker notes:

![Peitho feature tour: light product-tour style with indigo accents](docs/images/example-feature-tour.png)

```sh
# Each sample has its layouts/ and css/ alongside it, so no flags are needed by convention
peitho present examples/keynote/deck.md
```

The Makefile targets are handy for smoke-testing (`make help` for a list; `make keynote`, `make lightning-talk`, etc. They `cargo run` including the shell-bundle build).

## Architecture

```
Markdown ─→ peitho build (parse, map, 4-stage check. Deterministic, pure functions)
              ├─ emit distribute → dist/ (distribution only; no shell or notes mixed in)
              └─ emit present    → .peitho/present-cache/ (presentation shell; volatile)
```

- The build core is Rust (typestate: `Parsed→Mapped→Checked→Rendered`. Unchecked slides can't reach the renderer)
- The presentation shell is TypeScript. The contract (domain types like the manifest) has Rust as its single source; TS types are generated into `bindings/` and CI checks for drift
- See `docs/PEITHO_KICKOFF.md` for the detailed design

## License

MIT
