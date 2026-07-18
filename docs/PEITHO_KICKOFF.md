# Peitho — Kickoff Spec

> For handoff to Claude Code. This document condenses the decisions settled during the design phase.
> Implementation starts from here. If the design feels like it's wavering, don't reinterpret it on your own — refer to the "Undecided Items" section or check with the author.

---

## 0. Project Overview

- **Name**: `peitho` (all lowercase, to align with the mythology line argus / iris)
- **Origin**: Peitho = the goddess of "persuasion" in Greek mythology. Maps to the core function: presenting = the act of persuading an audience.
- **Repository**: `mizzy/peitho`
- **Publish target**: `peitho.gosu.ke`
- **Identity**: a web/HTML-native presentation tool. It treats **Markdown as the source of truth**, converts it via a deterministic renderer into stable-keyed HTML, and projects it in the browser.
- **Implementation language (settled)**: the build core is Rust (to make serious use of typestate, and for conceptual consistency with Carina). The presentation runtime (the presentation shell) is TS. **The contract (domain types, manifest schema) has Rust's `peitho-core` as the sole source of truth, generating TS types via `ts-rs` / `schemars` to prevent drift** (details in §17). The point isn't "unifying the language" but "keeping a single source of truth for the contract."

---

## 1. The Three Pillars of Design

1. **Separation of content and design** (derived from k1LoW/deck)
   Content is Markdown, design is templates and CSS. The two are never mixed.
2. **HTML-native, version-controllable templates**
   Design artifacts are HTML/CSS. They can be version-controlled, diffed, and reviewed. Whereas deck locks design into Google Slides' proprietary world, Peitho differentiates itself here.
3. **Type-checked slot contracts and keyed overrides** (derived from Carina)
   Slot shortages/excesses, type mismatches, and broken references are detected at build time. The polar opposite of deck's "silently drops content when placeholders are insufficient."

---

## 2. Pipeline

```
Markdown (source of truth)
    │  ← Hand-written by the author. Content and structure only.
    ▼
Peitho renderer (deterministic, pure function)
    │  ← Same input always yields the same HTML. No hidden state.
    ▼
HTML (with data-slide-key + .slot-*)
    │  ← base theme CSS applied.
    ▼
Projected in the browser / hosted at peitho.gosu.ke
```

**Where Claude Design fits in**: it is not involved in rendering each deck. Upstream, as an "occasionally-called designer," it creates (a) a base theme with slots, and (b) per-slide override CSS keyed on slide keys. **It only touches CSS, never the HTML structure** (details in §7).

---

## 3. Slot Contract

**The template itself doubles as the schema.** The contract is not split into a separate file (dual management always drifts). The analogy is Web Components' `<slot name>`. Think of a layout as "a custom element with named slots."

Template example:

```html
<!-- templates/title-body-code.html -->
<section class="slide">
  <h1><slot name="title" accepts="inline" arity="1"></slot></h1>
  <div class="body">
    <slot name="body" accepts="blocks" arity="0..*"></slot>
  </div>
  <figure class="code">
    <slot name="code" accepts="code" arity="0..1"></slot>
  </figure>
</section>
```

Peitho parses the template, extracts the contract from the `<slot>` elements, and type-checks the content being poured in against it.

### Content types (`accepts`)

A small set suffices:

- `inline` — inline only (emphasis, code spans, links allowed; block-level line breaks not allowed)
- `blocks` — block flow (paragraphs, lists, blockquotes, etc.)
- `text` — raw text with no markup
- `code` — a language-tagged code block
- `image` — an image reference
- `list` — list only

### Arity (Rust-style range notation)

- `1` — required, exactly one
- `0..1` — optional
- `1..*` — one or more
- `0..*` — any number

---

## 4. Mapping (Markdown → Slots)

A two-tier approach: **convention defaults + explicit escape hatch**.

- **Convention (inherited from deck)**: within a slide, the shallowest heading level is the title, the next is the subtitle, and the rest is the body. The vast majority of slides — title plus body — work with plain Markdown alone.
- **Explicit assignment**: for layouts like two-column ones where convention alone is ambiguous, use a fenced div to assign explicitly.

```markdown
# Architecture

::: {slot=left}
- Markdown = source of truth
- Deterministic rendering
:::

::: {slot=right}
​```rust
enum Phase { Parsed, Mapped, Checked }
​```
:::
```

The 90% that convention handles is plain Markdown; only the elaborate 10% needs explicit tags.

---

## 5. The Checking Pass (4 Stages, Anti-Silent-Drop)

deck silently discards the overflow when placeholders run short. **Peitho turns this into a build-time error instead of discarding anything.**

1. Assign content fragments to slots (convention or explicit tags)
2. Check whether each slot's contents satisfies `accepts` (the type)
3. Check whether `arity` is satisfied
4. **Check whether any unassigned content remains** (← the heart of anti-silent-drop) + whether required slots are filled

Errors come with line numbers and point to the next action:

```
error: slide 4 needs 2 code slots but layout 'title-body-code' only allows up to 0..1
  = help: use layout 'code-2col', or split the content across explicit slots
```

### Typestate

Express the build as phases via types:

```
Parsed → Mapped → Checked → Rendered
```

Make it so that **an unchecked slide cannot be passed to the rendering function** (a compile-time guarantee). Carina's phase-based structs are applied directly to slides. "Skip checking and render anyway" becomes impossible at the type level.

---

## 6. Stable Keys

- **An author-specified key is the primary source**: `<!-- {"key":"arch-1"} -->`
- If none is specified, a derived key (e.g., a slug) is assigned, but the operating rule is that **any slide worth targeting with an override must have an explicit key**.
- If a derived key depends on content (e.g., slugifying the title), then the moment you tweak a single character in the title, the key changes and the CSS you applied loses its target — exactly the "silently comes unstuck" failure mode we want to avoid. That's why override targets are pinned with explicit keys.
- Keys serve double duty as a **CSS targeting hook** and a **handle for a future AI to reference a slide**.
- Type checking: a CSS selector targeting a nonexistent key is a build error. If a manual tweak comes unstuck, it stops with a red error instead of silently breaking.

---

## 7. Per-Page Position/Size Adjustment (Model B — the adopted approach)

Peitho stays a **pure renderer** (it does not adopt deck's stateful-target approach). Per-page geometry adjustments are made via stable-key-based CSS overrides.

```css
/* themes/overrides.css */
[data-slide-key="arch-1"] .slot-code {
  grid-column: 2 / 3;
  width: 60%;
}
[data-slide-key="intro"] .slot-title {
  font-size: 4rem;
  align-self: end;
}
```

- The content (Markdown) is never touched. Adjustments are left to the cascade.
- Everything is version-controlled, diffable, and fully reproducible. Re-rendering naturally reapplies it (achieving "the adjustment survives" even though it's a pure function, via source-side CSS).
- This is only possible because it's HTML/CSS-native. Since deck's Google Slides has no cascade, it had no choice but to accumulate state on the target. Peitho produces the same result without accumulating state.
- **Override checking**: an override targeting a nonexistent slot is a compile error. A reference to a nonexistent slide key is also an error. Even the escape hatch is protected by types.

---

## 8. The Handoff Boundary to Claude Design

For B to hold, "Claude Design touches **only CSS** and never the HTML structure" must be true. If the structure is touched, the slot contract breaks. The handoff is pinned down as follows:

- **What Claude Design receives**: rendered HTML (containing `data-slide-key` and `.slot-*`) plus a short vocabulary sheet listing "available slide keys and slot classes." **Read-only.**
- **What Claude Design returns**: **a single override stylesheet only.** It is never asked to generate markup.
- **Peitho**: layers override.css on top of the base theme, and checks every selector's key and slot against the known contract.

CSS work splits into two layers:

- **base theme** — shared across all slides. An occasional, high-value redesign job.
- **per-slide override** — deck-specific tweaks, key-based.

Both are "just CSS," so both fit naturally into Model B.

---

## 9. Milestone 1 — The Minimal Vertical Slice

**Goal**: get one minimal path running end-to-end that proves out the whole architecture.

- One Markdown file with one explicit stable key
- Poured into one base template that has a slot contract
- Emit HTML (with `data-slide-key` and `.slot-*`)
- Apply a single line of `override.css` targeting that key, and confirm it takes effect
- Confirm the checking pass works (a contract violation properly produces an error)

If this passes, all three pillars are proven. Prioritize this above everything else.

### Task Breakdown

1. `parser.rs`: Markdown → intermediate content model (retaining heading level / blocks / code blocks)
2. `template.rs`: parse the HTML template and extract the contract (name / accepts / arity) from `<slot>`
3. `mapping.rs`: assign content fragments to slots (convention only first; explicit fenced divs come next)
4. `check.rs`: the 4-stage check + express the typestate phases (`Parsed → Mapped → Checked → Rendered`) via types
5. `render.rs`: checked model → emit HTML carrying `data-slide-key` and `.slot-*`
6. `theme.rs`: layer `overrides.css` on `base.css`, checking selectors' keys and slots against the contract
7. `main.rs`: get `peitho build <md>` working. `--watch` comes after the vertical slice passes.
8. Place `examples/deck.md` plus a minimal template and CSS, and run it through manually

---

## 10. Initial Repository Layout (Draft)

```
peitho/
  Cargo.toml
  src/
    main.rs           # CLI entry point (build, watch)
    parser.rs         # markdown → intermediate content model
    slot.rs           # slot contract types (accepts, arity)
    template.rs       # HTML template → slot contract extraction
    mapping.rs        # content → slot assignment (convention + explicit)
    check.rs          # 4-stage check, typestate phases
    render.rs         # mapped+checked → HTML (data-slide-key, .slot-*)
    theme.rs          # layering and validation of base theme + override.css
  templates/
    title-body-code.html
  themes/
    base.css
    overrides.css
  examples/
    deck.md
  tests/
```

---

## 11. Positioning (Differentiation from Existing Tools)

- **deck**: borrows the content/design separation concept. However deck depends on Google Slides (proprietary, OAuth, stateful). Peitho differentiates itself by being HTML-native, version-controllable, and a pure renderer.
- **Slidev / Marp**: part of the same lineage of Markdown-based, developer-oriented presentation tools. Peitho's distinctiveness is its **type-checked slot contracts + keyed overrides** (no silent drops, broken references rejected at build time) — bringing Carina's type philosophy into slides.

In short: **deck's content/design separation × HTML-native, version-controllable templates × Carina-derived type-checked slot contracts and keyed overrides**.

---

## 12. CLI Surface and the Three-Way Split of Responsibilities

The concerns "generate / present / publish" are split into three commands. Although build and present are different languages and different commands, they co-evolve against the same contract (manifest + generated types), so they are **bundled into a single repository (workspace)**.

```
peitho build      → dist/ (slide bodies as HTML/CSS + slides/ + manifest.json + notes.json)
                    ※ Distributed artifacts = slide bodies only. Non-inclusion of the presentation shell and notes is the default.
                    ※ With --watch, rebuilds on every Markdown save.
peitho present    → generates present.html + the presentation shell + notes from the intermediate representation into a volatile area, then opens it.
                    ※ .peitho/present-cache/ etc. (gitignored). Never left in the persistent distributed artifacts.
peitho publish    → ships dist/ as-is to the publish target (no exclusions, no branching — the thinnest possible wrapper).
                    ※ The actual work is delegated to existing IaC/CI (S3+CloudFront, etc.). Deployment is not reinvented.
```

- **build and present are separate commands, separate languages** (build = a Rust binary, present = TS). But they're tied together by the contract, so they don't drift (§17).
- **Claude Design is a low-frequency loop outside the CLI.** It's the "occasionally-called designer" that creates the base theme and override CSS. It doesn't insert itself into every build/present.

---

## 13. Projecting via emit from a Shared Intermediate Representation

Parsing, mapping, and checking happen **once, in one form.** From there, multiple output targets are projected. Since branching happens only at the **output stage (emit)**, not at the input, the source of truth never splits.

```
Markdown + template + theme + notes
        │
   peitho build            ← parsing, mapping, checking (§5). This happens once.
        ▼
  intermediate representation (manifest + checked slide model)   ← the sole source of truth
        │
   ├── emit distribute  →  slides/ + manifest.json + index.html (slide bodies only)
   └── emit present     →  present.html pointing at the above + presentation shell + notes.json (volatile)
```

Even if emit targets increase in the future (PDF export, static thumbnails, etc.), it's just a matter of adding an emit against the same intermediate representation. The three-way build/present/publish split becomes the extension point as-is.

---

## 14. Two-Entry-Point Structure (Single Underlying Substance)

For distribution and presentation, the **entry points (entry HTML) differ, but the slide substance is a single one.** Both entry points "point to" slides/, rather than "owning" it.

```
dist/
  index.html        ← entry point for distribution. Reads the manifest and just displays slides/.
  slides/           ← the slide bodies (HTML fragments + CSS + images). The substance lives here. Just one copy.
  manifest.json     ← the contract (order, keys, src, hasNotes).

(only when present runs, in a volatile area)
  present.html      ← entry point for presenting. Reads the same slides/ and manifest, and boots the presentation shell.
  notes.json        ← notes. Only present.html reads this.
```

- **Shared**: slides/ and the manifest (a single source).
- **index.html-specific**: minimal display and navigation only. Neither the presentation shell nor notes are included.
- **present.html-specific**: it loads the presentation shell (TS) and also reads notes.json. **This isn't a persistent build artifact — present generates it as volatile output.**
- **publish**: only index.html, slides/, and manifest.json are distributed. present.html and notes.json were never in dist to begin with.

**Each slide fragment is its own separate file** (`slides/001-arch-1.html`). This matches the fetch unit, the shadow-root-insertion unit, and the incremental-rebuild unit (§15).

---

## 15. Connection Method: fetch + Shadow DOM

present.html **fetches** the shared slides/ and loads it, placing each slide into a **Shadow DOM** for display.

- **fetch**: the slide substance stays singular; present just points at it and reads it (no duplication).
- **Shadow DOM**: confines a slide's CSS to its shadow root, preventing interference with the presentation shell UI's CSS (gaining iframe's CSS isolation benefit without iframe's awkward manipulation).
- **Handles are exposed on the host element**, so even when isolated by Shadow, the shell can still grab it by key.
- **Presenter view (two screens)**: two windows synced in position via BroadcastChannel (channel `peitho-sync`). Each window displays a single slide using the same fetch+Shadow mechanism.

(The bundling approach — folding everything into a single file at build time — was rejected because it breaks the singular substance of slides and duplicates the source.)

---

## 16. The Contract Between the Body and the Presentation Shell (Handles and Events)

**The shell targets things from outside the slide, by key, and broadcasts state via events. The slide only listens to events; it doesn't know the shell exists** (one-directional dependency).

### Handles (the surface the slide body exposes)

```html
<section class="peitho-slide" data-slide-key="arch-1" data-slide-index="1">
  <!-- Everything below is inside the Shadow. The shell doesn't reach in here. -->
  <h1 class="slot-title">...</h1>
</section>
```

- `data-slide-key` — attached to the Shadow host element. The primary key the shell uses to grab its target. Matches the manifest's `key` (guaranteed by build).
- `data-slide-index` — likewise. Used for ordering/progress calculations. Matches the manifest's `index`.
- `.peitho-slide` — the host's class. Namespaced with `peitho-` to avoid collisions with author classes.

### Custom Events (DOM, within-window, all prefixed `peitho:`)

**shell → everyone (notification)**
- `peitho:slidechange` — payload `{ key, index, total, previousIndex }`. Fires right after a switch. Triggers progress updates, two-window sync, and presenter-view updates.
- `peitho:presentationstart` — payload `{ total, startedAt }`. Presentation start / timer start.
- `peitho:presentationend` — payload `{ endedAt, elapsedMs }`. Presentation end / timer stop.

**UI → shell (request)**
- `peitho:navigate` — payload `{ to: "next"|"prev"|"first"|"last" | {key} | {index} }`. A move request.
- `peitho:timercontrol` — payload `{ action: "pause"|"resume"|"reset" }`.

### Invariants (Prevent Coupling — Most Important)

- **Only the shell ever executes transitions.** UI components (remote control, clicks, presenter-view buttons) only request via `navigate`. The shell executes it and then broadcasts the result as `slidechange`. Even as more input sources are added, state stays managed in one place.
- **The slide body may listen to events, but may not issue requests** (fire `navigate`, etc.). If a slide assumed the shell's existence, it would break in distributed artifacts (which ship without the shell).
- **The presentation shell is an independent entry point with a one-directional dependency on the slide body + manifest.** It is not mixed into the body's bundle. "Include it or not" is just an emit-time injection switch — it never requires a rebuild.

### Synchronization Layering

```
"Next" pressed in the presenter window
  → peitho:navigate {to:"next"} (DOM, within-window)
  → the shell executes the transition → peitho:slidechange (DOM, local UI update)
  → sent over the BroadcastChannel 'peitho-sync'
  → the audience window's shell receives it → moves to the same position → rebroadcasts slidechange locally
```

DOM events (within-window) and BroadcastChannel (between-window) are kept as separate layers, bridged by the shell. UI components only need to know about within-window events.

---

## 17. The manifest / notes Schema and the Single Source of Truth for the Contract

### manifest.json (holds no content, only references and metadata)

```jsonc
{
  "version": 1,                 // the schema's own version, for the shell to judge whether it can read the format
  "peithoVersion": "0.3.1",     // the generator's version, recorded for reproducibility
  "title": "...",
  "slideCount": 40,
  "slides": [
    { "index": 0, "key": "title",  "src": "slides/000-title.html",  "hasNotes": true  },
    { "index": 1, "key": "arch-1", "src": "slides/001-arch-1.html", "hasNotes": false }
  ]
}
```

- `src` **is a reference to the fragment, not the content itself** (embedding the content would create duplication — a hard constraint).
- `key` is the catalog entry. The actual handle is the body DOM's `data-slide-key`. build guarantees the two match.
- `hasNotes` is **only a presence flag.** The note text itself is not included (since the manifest can also be read from the distribution-side index.html).

### notes.json (isolated into a separate file)

```jsonc
{ "version": 1, "notes": { "title": "...", "arch-3": "...", "conclusion": "..." } }
```

- **Linked by key** (not by index). Even if slides are inserted and the ordering shifts, notes stay attached to the correct slide.
- **A physically separate file.** It is not hidden in the DOM with `display:none` (since that would leak via View Source). Only present fetches it; publish never distributes it.

### The Single Source of Truth for the Contract (Preventing Drift)

- The domain types (Slide, SlideKey, Notes, etc.) and the manifest schema have **`peitho-core` (Rust) as their sole source.**
- **TS types are generated from Rust** via `ts-rs` / `schemars` (through JSON Schema). The presentation shell (TS) never hand-writes the contract — it references the generated artifact.
- If the manifest on the Rust side changes, the TS types fall out of sync and the compile turns red — **drift is stopped by the type system.** This is the runtime version of §3's "don't dual-manage the contract."

### Initial Repository Layout (Workspace, an Update to §10)

```
peitho/                     # a single git repo (workspace)
  crates/
    peitho-core/            # domain/manifest schema = the contract's source of truth
    peitho/                 # the build CLI (Rust binary). present/publish subcommands live here too
  packages/
    peitho-present/         # the presentation shell (TS). loaded by present.html
  bindings/                 # TS types generated from peitho-core (referenced by peitho-present)
  templates/                # layout HTML (carrying slot contracts)
  themes/                   # base.css / overrides.css (shared theme; never placed on the deck side)
  examples/
    deck.md
  tests/
```

**The slide Markdown bodies are not placed in the peitho repo** (a separate repository, e.g. `mizzy/decks`, is recommended, pinning a peitho version). Generated HTML is never committed anywhere (`.gitignore`d, regenerated). The base theme/shared layouts live in the peitho repo; deck-specific override CSS lives on the deck side.

---

## 18. Undecided Items (Claude Code should not decide these unilaterally — stop here or check with the author)

- **The layout selection method**: for MVP, stop at "explicit designation or rule-based selection → check against the chosen layout." **Type-driven dispatch** (automatically searching for a layout whose contract satisfies the content shape) is a future option. Since ambiguity resolution for when multiple layouts match hasn't been decided, MVP does not go there.
- **The final form of explicit slot notation**: fenced div (`::: {slot=...}`) is the leading candidate, but comparison with Slidev-style component slots is undecided. For MVP, get convention-based mapping working first; explicit notation comes after.
- **Handling of code blocks**: since it's HTML-native, a syntax highlighter can render directly (it's expected that image-based rendering via an external command, as deck does, won't be needed). Highlighter selection happens at implementation time.
- **The presentation cache (`.peitho/present-cache/`) policy**: whether to rebuild it from scratch every time (clean, but a slightly slower startup) or cache it and update incrementally (faster, but stale artifacts may linger) is undecided. Decide in combination with watch.
- **Implementation language**: settled (build = Rust / present = TS / the contract is generated from Rust). See §0, §17. This item is closed.
