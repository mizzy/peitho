# Layout `<script>` execution and the slide-root bridge (Issue #530)

Reported by @kfly8 with working fixes in a fork (kfly8/peitho#2, kfly8/peitho#3).
This plan reimplements both against the invariants and closes three gaps those
PRs do not cover. Tasks depend on the ones before them; implement in order.

## The two reported problems

1. **A layout's own `<script>` never runs.** The shell injects slide HTML with
   `innerHTML`; the HTML spec marks a script parsed that way "already started",
   so it silently does nothing.
2. **Even once it runs, it cannot find its slide.** Each slide lives in a Shadow
   DOM host, and `document.querySelectorAll` / `MutationObserver` never cross a
   shadow boundary, so a mounting script has no root to mount into.

## Why this does not break the pillars

Pillar ① is untouched: the `<script>` lives in layout HTML, which is design.
Nothing new appears in Markdown.

§16 permits the direction: *"The slide body may listen to events, but may not
issue requests."* `peitho:shadow-mounted` is shell → slide notification only. It
carries no transition and grants no way to request one, so "only the shell ever
executes transitions" still holds.

§16 also states the constraint this plan must satisfy: *"If a slide assumed the
shell's existence, it would break in distributed artifacts (which ship without
the shell)."* That is exactly the divergence below, and why the event fires in
every output mode rather than only where a shell exists.

## Current state: five surfaces, three behaviors

Every renderer emits each slide as
`<section class="peitho-slide" data-slide-key=… data-slide-index=…>`.
Where that section lands differs per mode:

| Surface | Injection | Layout `<script>` today |
|---|---|---|
| `present` slides window, presenter's two panes, remote preview (`shell.ts:770`) | `template.innerHTML` → shadow root | **inert** |
| `preview` stage + filmstrip thumbnail (`preview.ts:1352`) | `template.innerHTML` → shadow root, **twice per slide** | **inert** |
| `build` dist viewer (`render.rs:2039`) | `canvas.innerHTML`, light DOM, **re-injected every navigation** | **inert** |
| `export pdf` (`render.rs:2166`) | server-side concatenation into one parsed document | **already runs** |
| `lint` (`render.rs:2211`) | same | **already runs** |

PDF and lint are the baseline the other three diverge from, not the reverse.

## Author decisions (2026-09-23)

- **Preview: stage only.** `createSlideHost` is called twice per slide
  (`preview.ts:908` stage, `preview.ts:922` thumbnail). The thumbnail is
  deliberately inert chrome (`pointer-events: none`), so a component mounted
  there cannot be operated; running scripts there would only duplicate timers,
  `fetch`es, and global registrations. Accepted tradeoff: whatever a script
  draws is missing from thumbnails.
- **Present: every surface the shell mounts** — slides window, the presenter's
  current- and next-slide panes, the remote's preview pane. Accepted tradeoff:
  layout JS also runs on the phone remote.
- **Fire the event in every output mode,** PDF and lint included, so one API
  works everywhere and §16's distributed-artifact warning is answered.
- **Detail is `{ root, key, index }`** with `root: ShadowRoot | Element`.
  `key`/`index` are already §16 handles on the host; carrying them lets a script
  branch on which slide it is without re-reading the DOM, and the widened `root`
  is what lets PDF/lint share one shape.

## Design

### `executeInlineScripts(root, doc)`

Re-creates each `<script>` as a fresh element (copied attributes, copied text)
and `replaceWith`s it in place, preserving position.

A classic inline script is wrapped in an IIFE. Load-bearing, not cosmetic:
classic scripts share ONE global scope regardless of shadow boundaries, so two
slides using the same layout, or a repeat visit in the dist viewer (which
re-injects on every navigation), throw "already declared" on a top-level
`let`/`const`. Not wrapped, each already having its own scope or no source to
wrap: `type="module"`, any `src=` script, and non-JS types (an
`application/json` data island stays byte-identical).

### `peitho:shadow-mounted`

Dispatched from the host once it connects, `bubbles: true, composed: true` so it
reaches `document` from inside the shadow tree.

A plain dispatch is insufficient alone. `load()` connects every host in one
synchronous burst while a `<script type="module">` cannot have loaded yet —
module loading is never synchronous — so every initial mount would be missed. A
`window.__peithoShadowRoots` backlog lets a script that loads later drain what
it missed. It appends, never overwrites, so two shells sharing one document (the
presenter's panes) both register.

### PDF/lint

No shadow root and no shell. A small bootstrap appended by the Rust renderer
walks `.peitho-slide` sections and fires the same event with `root` set to the
section element, filling the same backlog. A script written once against
`{ root, key, index }` then works on all five surfaces.

---

## Task 1: `executeInlineScripts`

**Goal.** One helper that makes an `innerHTML`-parsed `<script>` run, wrapping
only what needs wrapping.

**Files.** `packages/peitho-present/src/scripts.ts` (new),
`packages/peitho-present/test/scripts.test.ts` (new).

**Test (Red).** `scripts.test.ts`, asserting the structural swap: the old node
is replaced by a different node at the same position (sibling count and order
unchanged); attributes are copied; a classic inline script's text becomes
`(function () {\n…\n})();`; `type="module"`, `src=`, and
`type="application/json"` keep their text byte-identical; multiple and nested
scripts are all replaced; content with no script is untouched; a bare
`DocumentFragment` works, not only an `Element`.

State in a header comment that jsdom never executes scripts (vitest does not opt
into `runScripts: "dangerously"`), so these are structural only and real
execution is task 8's job.

**Implementation (Green).** `executeInlineScripts(root: ParentNode, doc: Document)`
iterating `Array.from(root.querySelectorAll("script"))` — materialized before
mutating, since `replaceWith` during a live `NodeList` walk skips nodes. Private
`needsScopeWrap` returning false for `src`, true only for
`CLASSIC_JAVASCRIPT_TYPES = {"", "text/javascript", "application/javascript"}`
after trim + lowercase.

**Verification.**
```sh
cd packages/peitho-present && npm test -- test/scripts.test.ts
cd packages/peitho-present && npm run typecheck
```

## Task 2: Run them in the present shell

**Goal.** Layout scripts execute in the slides window, the presenter's two
panes, and the remote's preview.

**Files.** `packages/peitho-present/src/shell.ts`,
`packages/peitho-present/test/shell.test.ts`.

**Test (Red).** `layout_script_is_re_created_so_it_can_execute` — mount a shell
whose slide HTML contains a `<script>`, assert the script inside the host's
shadow root is not the inert original (its text carries the IIFE wrap). Assert a
deck with no script produces a shadow root identical to today's.

**Implementation (Green).** In `createSlideHost`, clone the template content to
a named `DocumentFragment`, call `executeInlineScripts(fragment, this.doc)`
before `shadow.appendChild(fragment)`. Scripts must be rehydrated **before** the
host is connected so they run once, on connect.

**Verification.**
```sh
cd packages/peitho-present && npm test -- test/shell.test.ts
```

## Task 3: Run them on the preview stage only

**Goal.** The stage executes layout scripts; the filmstrip thumbnail never does.

**Files.** `packages/peitho-present/src/preview.ts`,
`packages/peitho-present/test/preview.test.ts`.

**Test (Red).** `layout_script_runs_on_the_stage_but_never_in_a_thumbnail` —
assert the stage host's shadow root holds a re-created script while the thumb
host's holds the untouched inert original. This is the regression guard for
double-mounting; it must fail if the parameter is dropped or defaulted true.

**Implementation (Green).** Add a required `executeScripts: boolean` parameter
to `createSlideHost` — required, not optional-defaulting, so a future call site
must state its intent rather than inherit a default. Pass `true` at
`preview.ts:908`, `false` at `preview.ts:922`.

**Verification.**
```sh
cd packages/peitho-present && npm test -- test/preview.test.ts -t 'layout_script_runs_on_the_stage_but_never_in_a_thumbnail'
```

## Task 3a: Shell shortcuts defer to interactive slide content

Found in task 3's review, landed with it (Issue #631). Not a new feature: one
missing invariant with symptoms on every surface.

**Cause.** Every shell shortcut and navigation gesture assumed slide content is
inert. It never was — layout HTML already ships plain `<details>`/`<input>` to
`dist/`, present, and preview — and tasks 2–3 make it common. Measured on main:
clicking a `<summary>` toggles it *and* advances; a space typed into a layout
`<input>` becomes "next slide" and never reaches the field. Preview had a
private editable check and exempted only `<a>` on Enter; present had none.

**Fix.** One module, `packages/peitho-present/src/interactiveTarget.ts`, decides
whether a key or pointer event belongs to slide content, from
`composedPath()[0]`. Every window keydown installer, the click guard, and swipe
navigation consult it; no installer carries its own check.

- Text-entry fields (textarea, select, contenteditable, text-type `<input>`) keep
  every key except unshifted PageUp/PageDown, so a clicker still navigates.
- Inside a slide — an enclosing shadow host with `data-slide-key`, walking up
  through nested shadow roots — Enter belongs to `a[href]`/`button`/`summary`/
  button-like inputs, Space to `button`/`summary`/checkbox-like inputs, arrows
  to radio/range, and Home/End/PageUp/PageDown to range (a clicker's PageDown
  moves a focused slider, not the deck). Shell chrome is outside a slide, so
  Space on a focused presenter button still toggles the timer.
- The presenter's panes and the remote's preview are mirrors: they still run
  layout scripts (author decision above) but mount with `inertSlides`, so a
  click there can never operate a copy the audience does not see.
- The distribution viewer's hand-mirrored copy in `render_distribution_index`
  follows the same rules (light DOM, so "inside a slide" is
  `closest('.peitho-slide')`), pinned by a Rust test that asserts every
  selector/type-list string from `interactiveTarget.ts` appears verbatim.

Accepted: Escape inside a slide text field does not close present; blur first.

## Task 4: The `peitho:shadow-mounted` event in the present shell

**Goal.** A layout script can obtain its own slide root, including one that
loads after mount.

**Files.** `packages/peitho-present/src/shell.ts`,
`packages/peitho-present/src/index.ts`,
`packages/peitho-present/test/shell-shadow-mounted.test.ts` (new).

**Test (Red).** Assert: one event per host, reaching `document` via
`composed: true`, in slide order; `detail` is `{root, key, index}` matching the
host's `data-slide-key` / `data-slide-index`; `window.__peithoShadowRoots` holds
every root in order; a shell whose root never joins the document still fills the
backlog without throwing; two shells sharing one document **append** to one
backlog (4 entries, not 2 overwritten) — the presenter's two panes.

**Implementation (Green).** Export `SHADOW_MOUNTED_EVENT` and
`ShadowMountedDetail` (`{root: ShadowRoot | Element; key: string; index: number}`)
from `shell.ts`, re-export from `index.ts`. `shadowRootsRegistry(win)` lazily
creates `win.__peithoShadowRoots`. Dispatch inside `load()`'s append loop,
immediately after `this.root.appendChild(view.host)` so the host is connected.

*As implemented (Issue #632), three changes from the text above, all found in
review:*
- The helpers (`SHADOW_MOUNTED_EVENT`, `ShadowMountedDetail`,
  `shadowMountedBacklog`, `announceShadowMounted`) live in `scripts.ts`, which
  both shells already import, so task 5 reuses them without adding the first
  runtime import of `shell.ts` to the preview bundle. `index.ts` re-exports them.
- The backlog holds the same `ShadowMountedDetail` objects the event carries, not
  bare roots: one shape for draining and for listening, with no second way to
  recover `key`/`index` for a light-DOM root.
- `load()` creates the backlog before the first host connects (a slide's classic
  inline script runs *during* `appendChild`, so it must already find the array),
  and announces every host in a separate pass **after** `show()`. Announcing
  mid-loop let listeners see every slide stacked and visible, and a listener that
  navigated synchronously left two slides rendered.

**Verification.**
```sh
cd packages/peitho-present && npm test -- test/shell-shadow-mounted.test.ts
```

## Task 5: The same event from the preview stage

**Goal.** Preview matches present, thumbnails excluded.

**Files.** `packages/peitho-present/src/preview.ts`,
`packages/peitho-present/test/preview.test.ts`.

**Test (Red).** `shadow_mounted_fires_for_the_stage_only` — one event per slide,
never from a thumb host, backlog contains only stage roots.

**Implementation (Green).** Reuse the task-4 helpers. Gate on the same
`executeScripts` parameter: one flag decides both, so a surface can never run
scripts while withholding their root, nor the reverse.

**Verification.**
```sh
cd packages/peitho-present && npm test -- test/preview.test.ts -t 'shadow_mounted_fires_for_the_stage_only'
```

## Task 6: The distribution viewer

**Goal.** A published deck behaves like `present`, across repeat visits.

**Files.** `crates/peitho-core/src/render.rs` (`render_distribution_index`).

**Test (Red).** Rust tests over the emitted HTML: `executeInlineScripts` and the
event dispatch are present; the rehydrate call is ordered **after**
`canvas.innerHTML = …`; the IIFE wrap literal is present; the module/`src` guard
is present. Mirror-drift guard: assert the embedded
`CLASSIC_JAVASCRIPT_TYPES` list matches `scripts.ts`'s.

**Implementation (Green).** Extend the embedded viewer script with a hand-mirrored
copy of `executeInlineScripts` (it runs as a plain `<script>` and cannot import
the bundle; both copies carry a comment naming the other). In `showSlide`, after
the `innerHTML` assignment, rehydrate and then dispatch the event per
`.peitho-slide` section — light DOM, so `root` is the section element.

Once scripts run here, the dist copy of the task-3a interactive-target rules
must also see controls inside a web component a layout script mounts: its
light-DOM `closest('.peitho-slide')` does not cross a nested shadow root, while
`interactiveTarget.ts` walks up through them. Prefer embedding one built bundle
of `scripts.ts` + `interactiveTarget.ts` over growing the hand mirror (task 2
and 3a reviews: the mirror now has to track seven script-rehydration rules and
every interactive-target rule, and a drift test can pin strings but not logic).

Note this is the one surface that re-runs a script on every visit, which is what
the IIFE wrap exists for.

Re-injection also shapes the backlog (task 4 review): each visit pushes a new
entry, so without care `window.__peithoShadowRoots` grows without bound, holds
detached sections alive, and a late module script drains stale roots from
earlier visits. Drop entries whose `root` is no longer connected before pushing
the current injection's. Create the backlog when the viewer script starts, so it
exists before any injected script runs, as it does in present. Pin the
`"peitho:shadow-mounted"` and `"__peithoShadowRoots"` literals in the drift test
alongside the mirrored strings.

**Verification.**
```sh
cargo test -p peitho-core distribution_index
```

## Task 7: PDF and lint

**Goal.** One script works on all five surfaces, so a layout cannot depend on
the shell's existence.

**Files.** `crates/peitho-core/src/render.rs` (`render_pdf_document`,
`render_lint_document`).

**Test (Red).** Both documents carry the bootstrap and fire the event per
`.peitho-slide` with `{root, key, index}`. Assert PDF still emits
`pdf_flatten.js` and that the bootstrap precedes it — the flattener rasterizes
what is on the page, so a script that draws must have run first.

**Implementation (Green).** One shared `const` holding the bootstrap, embedded by
both renderers: walk `document.querySelectorAll('.peitho-slide')`, push each to
the backlog, dispatch the event with the section as `root`. No script
rehydration here — these documents are parsed normally, so their scripts already
ran.

**Verification.**
```sh
cargo test -p peitho-core render_pdf_document
cargo test -p peitho-core render_lint_document
```

## Task 8: Real-Chrome E2E checklist

**Goal.** Prove the feature actually works. **Every assertion in tasks 1–7 is
structural** — jsdom does not execute scripts at all — so nothing above
demonstrates a script running.

**Files.** a throwaway deck with a layout carrying a counter button, plus a
`<script type="module">` slide to exercise the backlog path.

**Checklist.** Each item observed in a real browser, not asserted in jsdom:

1. `peitho present` — the button increments on click.
2. Presenter view — the current pane's button works; the next-slide pane shows
   the rendered result.
3. The remote's preview pane renders it.
4. Two slides sharing one layout — both work, no "already declared" in console.
5. `peitho preview` — the stage button works; **the thumbnail did not
   double-mount** (a script logging on mount logs once per slide, not twice).
6. `peitho build` → open `dist/index.html` — works, and **still works after
   navigating away and back** (the re-execution path).
7. `peitho export pdf` — a script that draws appears in the PDF.
8. `peitho lint` — exits normally, no hang (recall the `image.decode()` class of
   silent hang under headless Chrome).
9. A `<script type="module">` slide drains `window.__peithoShadowRoots` and
   mounts correctly — the backlog's reason for existing.
10. `<script src="lib.js">` followed by an inline script that uses the library —
    the inline script sees the library in present, preview, and `dist/`, as it
    does in PDF/lint (parser-blocking order, found in task 2's review).
11. Clicking a layout `<button>`/`<summary>` does not advance, and a space typed
    into a layout `<input>` is inserted, in present, the preview stage, and
    `dist/index.html`; PageDown from that input still advances (task 3a).
12. The presenter's panes and the remote's preview do not react to clicks,
    while the laser pointer on the remote still works (task 3a).

## Task 9: Documentation

**Goal.** The contract is discoverable and §16 stays authoritative.

**Files.** `docs/PEITHO_KICKOFF.md` (§16), the guide's layout page under
`site/content/`, `CLAUDE.md`.

**Implementation.** Add `peitho:shadow-mounted` to §16's "shell → everyone"
list, noting it is the first such event carrying a DOM handle and that it fires
in every output mode. Guide: a worked example with the
`document.currentScript.getRootNode()` caveat — a re-created script executes in
the *document* global scope, not inside the shadow root, so `document.querySelector`
in a layout script does not see its own slide.

Correction from measurement (task 2 review): `document.currentScript` is `null`
while a script inside a shadow root runs, so `currentScript.getRootNode()` is not
a way to reach the slide on any shell surface — the event and backlog are. The
guide must also cover what tasks 1–4 established:
- the backlog exists before any layout script runs, and its entries are the same
  `{root, key, index}` detail the event carries; a script drains it *and* listens;
- a layout script runs once per slide that uses the layout, and in every shell
  that mounts it (the presenter mounts two), while the event is global, so a
  script installs its handler once (a global guard) and mounts idempotently per
  root (mark the root);
- the IIFE wrap makes top-level `var`/`function` local; publish shared API as
  `window.x = …`;
- an external classic `<script src>` runs once per slide per shell, so load
  libraries as modules (the module map dedupes by URL) or guard them;
- an inline script after a blocking `<script src>` runs after it loads, as in a
  parsed document;
- external SVG scripts run in load order, not document order;
- `window.__peithoShadowRoots` must stay an array; replacing it with a shim is an
  error, not a supported pattern.

Remember: the guide is Zola, and a line starting with four backticks swallows
everything after it (see `zola-guide-fence-hazard`).

**Verification.**
```sh
make demo-site
```

---

## Deliberately not in scope

- **No opt-in frontmatter key.** Layout HTML is already trusted author-controlled
  code, and a `<script>` in it is the point of the issue; a key to enable what
  the author just typed is ceremony. Revisit if a real case appears for building
  with layout JS suppressed.
- **No sandboxing.** Same reasoning. Note `lint` and `export pdf` do run that
  code under headless Chrome.
- **No `srcset`-style multi-asset handling** — layout-referenced assets already
  refuse it (#529).

## Gate note

Tasks 2–5 change `packages/peitho-present/dist/*.js`, which is embedded and
drift-checked. Run `npm run build` before the `git diff --exit-code` gates.
