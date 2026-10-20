# Distribution viewer reveal steps (Issue #722)

Date: 2026-10-20

## Scope and design

- `public_reveal: final | steps` is a typed deck setting. It defaults to
  `final`; every other value fails once during frontmatter parsing with the
  frontmatter line and focused help.
- The value follows one typed path:
  `DeckFrontmatter` -> `DeckSettings` -> `emit_distribution` ->
  `render_distribution_index` -> serialized `DistributionViewerConfig`.
  No consumer inspects YAML, HTML, or a string setting.
- Only the `Audience::Public` distribution viewer consumes the setting.
  Present keeps its behavior, preview remains fully revealed until Issue #724,
  and PDF step expansion remains Issue #723.
- There is one `dist/viewer.js` and one distribution bootstrap. Both modes use
  the same navigator; `walkRevealSteps: false` keeps the position at the
  destination slide's `revealStepCount`, moves next/previous by whole slides,
  and removes rather than writes a `step` query parameter.
- `dist/index.html` may change in both modes because it embeds the rebuilt
  viewer and a serialized config line. Across otherwise identical builds,
  absent and explicit `final` keep `dist/slides/*` and `manifest.json`
  byte-identical, while `final` and `steps` keep PDF and lint HTML
  byte-identical. There is no second viewer runtime.
- `stepState.ts` owns reveal visibility, equality-based stepped emphasis, and
  their CSS. Its `ParentNode` API works with the present shell's `ShadowRoot`,
  the distribution viewer's light DOM, and the preview stage `ShadowRoot` in
  Issue #724 without copying logic.
- In distribution `steps` mode, install the shared style exactly once in
  `document.head`. Head placement reaches light-DOM slides. Final mode installs
  no step style and never calls the DOM-state mutator, so nothing is hidden and
  stepped emphasis remains inactive as it is today.
- Distribution deliberately ignores manifest `skip`: skipped slides remain
  navigable in both modes. The viewer navigator projects every input to
  `{ revealSteps }` before calling `resolveStepTarget`, so `skip` can never
  enter the distribution step path.

## Task 1: Parse and carry the typed frontmatter setting

**Goal:** Accept only `final` and `steps`, default to `final`, and retain
the validated value across `Parsed -> Mapped -> Checked -> Rendered`.

**Files:**

- `crates/peitho-core/src/phase.rs`
- `crates/peitho-core/src/parser.rs`
- `crates/peitho-core/src/code_images.rs`
- `crates/peitho-core/src/render.rs`

**Test (code):** Add parser coverage beside the `page_numbers` tests and extend
the existing typestate-retention test.

```rust
#[test]
fn public_reveal_is_typed_and_defaults_to_final() {
    let cases = [
        ("# Intro", PublicReveal::Final),
        ("---\npublic_reveal: final\n---\n# Intro", PublicReveal::Final),
        ("---\npublic_reveal: steps\n---\n# Intro", PublicReveal::Steps),
    ];

    for (source, expected) in cases {
        let deck =
            parse_markdown(source, &crate::highlight::Highlighter::defaults()).unwrap();
        assert_eq!(deck.settings().public_reveal(), expected);
    }
}

#[test]
fn invalid_public_reveal_has_its_line_and_focused_help() {
    for value in ["all", "true", "false"] {
        let source = format!("---\npublic_reveal: {value}\n---\n# Intro");
        let err =
            parse_markdown(&source, &crate::highlight::Highlighter::defaults()).unwrap_err();
        assert_eq!(err.kind, ErrorKind::Parse);
        assert_eq!(err.line, Some(2));
        assert!(err.to_string().contains("public_reveal"));
        assert_eq!(err.help, r#"use "final" or "steps""#);
    }
}

// In render::tests::deck_settings_survive_all_typestate_transitions:
assert_eq!(rendered.settings().public_reveal(), PublicReveal::Steps);
```

**Implementation (code):**

- Add `PublicReveal::{Final, Steps}` in `phase.rs`, with snake-case serde
  names, `Final` as `Default`, and `Copy + Eq` semantics. Do not generate a
  TypeScript binding; this setting crosses the Rust/JavaScript boundary only as
  viewer config.
- Add the field, constructor argument, and accessor to `DeckSettings`. Update
  every `DeckSettings::new` call in the four listed files.
- Add `public_reveal` to `DeckFrontmatter`, `frontmatter_key_lines`, the
  supported-key help, and a focused `frontmatter_help` branch. Serde performs
  the sole value validation; downstream phases receive only `PublicReveal`.
- Parse `public_reveal: steps` in
  `deck_settings_survive_all_typestate_transitions` and assert the same enum
  after rendering.

**Verification (exact commands):**

```sh
cargo test -p peitho-core public_reveal
cargo test -p peitho-core deck_settings_survive_all_typestate_transitions
git diff --check -- crates/peitho-core/src/phase.rs crates/peitho-core/src/parser.rs crates/peitho-core/src/code_images.rs crates/peitho-core/src/render.rs
```

## Task 2: Extract reusable step DOM state from the present shell

**Goal:** Give present, distribution, and the later preview work one DOM API
for cumulative reveal visibility, equality-based stepped emphasis, and their
CSS.

**Files:**

- `packages/peitho-present/src/stepState.ts`
- `packages/peitho-present/src/shell.ts`
- `packages/peitho-present/test/stepState.test.ts`
- `packages/peitho-present/test/loads-handles-navigates-invalid-previousIndex-keyboard-fetch.test.ts`

**Test (code):** Add a root-agnostic unit test before moving the shell logic.

```ts
import { expect, it } from "vitest";
import { applyStepState, createStepStateStyle } from "../src/stepState";

it("applies cumulative reveal and equality emphasis in light and shadow DOM", () => {
  const light = document.createElement("section");
  const host = document.createElement("section");
  const shadow = host.attachShadow({ mode: "open" });

  for (const root of [light, shadow]) {
    root.innerHTML =
      '<p data-reveal-step="1"></p><p data-reveal-step="2"></p>' +
      '<span data-emphasis-step="1 2"></span><span data-emphasis-step="2"></span>';

    applyStepState(root, 1);
    const reveals = [...root.querySelectorAll<HTMLElement>("[data-reveal-step]")];
    const emphasis = [...root.querySelectorAll<HTMLElement>("[data-emphasis-step]")];
    expect(reveals.map((node) => node.hasAttribute("data-reveal-hidden"))).toEqual([
      false,
      true
    ]);
    expect(emphasis.map((node) => node.hasAttribute("data-emphasis-active"))).toEqual([
      true,
      false
    ]);

    applyStepState(root, 2);
    expect(reveals.map((node) => node.hasAttribute("data-reveal-hidden"))).toEqual([
      false,
      false
    ]);
    expect(emphasis.map((node) => node.hasAttribute("data-emphasis-active"))).toEqual([
      true,
      true
    ]);
  }

  const css = createStepStateStyle(document).textContent ?? "";
  expect(css).toContain("[data-reveal-hidden]{visibility:hidden}");
  expect(css).toContain("[data-emphasis-active]{");
  expect(css).toContain("pre:has([data-emphasis-active])");
});
```

**Implementation (code):**

```ts
export function createStepStateStyle(doc: Document): HTMLStyleElement;
export function applyStepState(root: ParentNode, step: number): void;
```

- Move `REVEAL_HIDDEN_CSS`, `EMPHASIS_ACTIVE_CSS`,
  `Shell.applyRevealState`, and `Shell.applyEmphasisState` into these exports
  without changing selectors or comparisons.
- Have `Shell.createSlideHost` append `createStepStateStyle(this.doc)` to its
  slide shadow root and have `Shell.show` call
  `applyStepState(slide.shadow, nextStep)`. Delete the shell copies.
- Keep this module independent of manifests, navigation, URLs, events, and
  shadow-host discovery. That is the API boundary that lets Issue #724 call it
  on only the preview stage.

**Verification (exact commands):**

```sh
(cd packages/peitho-present && npx vitest run test/stepState.test.ts test/loads-handles-navigates-invalid-previousIndex-keyboard-fetch.test.ts && npm run typecheck)
```

## Task 3: Add a testable distribution navigator and URL contract

**Goal:** Put all distribution position, sequential movement, and URL semantics
in TypeScript, leaving the Rust-embedded JavaScript responsible only for DOM
and event wiring.

**Files:**

- `packages/peitho-present/src/viewer.ts`
- `packages/peitho-present/test/viewer.test.ts`

**Test (code):** Drive the exported navigator directly in Vitest.

```ts
import {
  createViewerNavigator,
  readViewerPosition,
  writeViewerPosition
} from "../src/viewer";

const stepped = createViewerNavigator({
  slides: [{ revealSteps: 1 }, { revealSteps: 2 }],
  walkRevealSteps: true
});
expect(stepped.move({ index: 0, step: 0 }, 1)).toEqual({ index: 0, step: 1 });
expect(stepped.move({ index: 0, step: 1 }, 1)).toEqual({ index: 1, step: 0 });
expect(stepped.move({ index: 1, step: 0 }, -1)).toEqual({ index: 0, step: 1 });
expect(stepped.move({ index: 0, step: 0 }, -1)).toEqual({ index: 0, step: 0 });
expect(stepped.move({ index: 1, step: 2 }, 1)).toEqual({ index: 1, step: 2 });

expect(readViewerPosition(new URL("https://deck.test/?slide=2"), stepped))
  .toEqual({ index: 1, step: 2 });
expect(readViewerPosition(new URL("https://deck.test/?slide=2&step=bad"), stepped))
  .toEqual({ index: 1, step: 2 });
expect(readViewerPosition(new URL("https://deck.test/?slide=2&step=99"), stepped))
  .toEqual({ index: 1, step: 2 });
expect(readViewerPosition(new URL("https://deck.test/?slide=2&step=1"), stepped))
  .toEqual({ index: 1, step: 1 });
expect(readViewerPosition(new URL("https://deck.test/#slide=2"), stepped))
  .toEqual({ index: 1, step: 2 });
for (const raw of ["", "-1", "1.5", "x", "NaN", "Infinity", "9007199254740992"]) {
  expect(readViewerPosition(
    new URL(`https://deck.test/?slide=2&step=${encodeURIComponent(raw)}`),
    stepped
  )).toEqual({ index: 1, step: 2 });
}

const final = createViewerNavigator({
  slides: [{ revealSteps: 1 }, { revealSteps: 2 }],
  walkRevealSteps: false
});
expect(readViewerPosition(new URL("https://deck.test/?slide=2&step=0"), final))
  .toEqual({ index: 1, step: 2 });
expect(final.direct(0, 0)).toEqual({ index: 0, step: 1 });
expect(final.move({ index: 0, step: 0 }, 1)).toEqual({ index: 1, step: 2 });
expect(final.move({ index: 1, step: 0 }, 1)).toEqual({ index: 1, step: 2 });
expect(writeViewerPosition(
  new URL("https://deck.test/?theme=dark&step=0#notes"),
  { index: 1, step: 2 },
  final
).href).toBe("https://deck.test/?theme=dark&slide=2#notes");
expect(writeViewerPosition(
  new URL("https://deck.test/?theme=dark#notes"),
  { index: 1, step: 1 },
  stepped
).href).toBe("https://deck.test/?theme=dark&slide=2&step=1#notes");

const skipMetadata = [
  { revealSteps: 0 },
  { revealSteps: 0, skip: true },
  { revealSteps: 0 }
];
const doesNotSkip = createViewerNavigator({
  slides: skipMetadata,
  walkRevealSteps: true
});
expect(doesNotSkip.move({ index: 0, step: 0 }, 1)).toEqual({ index: 1, step: 0 });
```

The missing-step case plus the table cover missing, empty, negative,
fractional, nonnumeric, non-finite, and unsafe values. The two write assertions
pin unrelated query/hash retention and mode-specific `step` output; the hash
assertion pins the existing slide fallback.

**Implementation (code):**

```ts
export type ViewerSlide = { revealSteps?: unknown };
export type ViewerPosition = { index: number; step: number };
export type ViewerNavigator = {
  readonly walkRevealSteps: boolean;
  direct(index: number, requestedStep?: number): ViewerPosition;
  move(position: ViewerPosition, direction: 1 | -1): ViewerPosition;
};

export function createViewerNavigator(options: {
  slides: ReadonlyArray<ViewerSlide>;
  walkRevealSteps: boolean;
}): ViewerNavigator;
export function readViewerPosition(url: URL, navigator: ViewerNavigator): ViewerPosition;
export function writeViewerPosition(
  url: URL,
  position: ViewerPosition,
  navigator: ViewerNavigator
): URL;
```

- Inside `createViewerNavigator`, immediately project
  `options.slides.map(({ revealSteps }) => ({ revealSteps }))`. Steps mode may
  then reuse `resolveStepTarget`, but it can never see `skip`; a manifest
  slide marked `skip` remains the next distribution slide.
- In steps mode, `direct` treats an omitted or invalid step as
  `revealStepCount`, clamps a valid step, and `move` converts
  `resolveStepTarget`'s `null` boundary result into the unchanged position.
  This yields next-entry step 0 and previous-entry fully revealed.
- In final mode, `direct` ignores every requested step and returns
  `revealStepCount`; `move` changes the index by exactly one and returns the
  destination count. Boundaries return the normalized current position.
- `readViewerPosition` owns integer validation, one-based URL to zero-based
  index conversion, clamping, and current hash fallbacks.
  `writeViewerPosition` clones the URL, preserves unrelated query parameters
  and the hash, always writes `slide`, writes `step` only for steps mode,
  and deletes a pre-existing `step` in final mode.
- Keep `announceParsedSlides`, `announceShadowMounted`,
  `dropDisconnectedShadowMounted`, `executeInlineScripts`,
  `shadowMountedBacklog`, `keyBelongsToTarget`, and `pointerBelongsToTarget`;
  add only `createViewerNavigator`, `readViewerPosition`, and
  `writeViewerPosition` in this task, and update the exact export assertion.
  Do not export `revealStepCount`, `clampStep`, or `resolveStepTarget` from
  `viewer.ts`; the bootstrap receives no low-level URL or step algorithms.

**Verification (exact commands):**

```sh
(cd packages/peitho-present && npx vitest run test/viewer.test.ts test/stepnav.test.ts && npm run typecheck)
```

## Task 4: Serialize typed viewer config and isolate output changes

**Goal:** Thread `PublicReveal` only into the distribution index and prove
that slide fragments, the manifest, PDF HTML, and lint HTML do not depend on
the mode.

**Files:**

- `crates/peitho-core/src/render.rs`
- `crates/peitho/src/main.rs`

**Test (code):** Assert typed config wiring in the renderer, then compare the
explicitly stable emitted artifacts across absent, final, and steps settings.
`index.html` is intentionally absent from `StablePublicOutputs`.

```rust
#[test]
fn distribution_index_embeds_typed_viewer_config() {
    let final_html = render_distribution_index(
        AspectRatio::Ratio16To9,
        &DeckLang::default(),
        PublicReveal::Final,
    );
    let steps_html = render_distribution_index(
        AspectRatio::Ratio16To9,
        &DeckLang::default(),
        PublicReveal::Steps,
    );

    assert!(final_html.contains(r#"const viewerConfig = {"walkRevealSteps":false};"#));
    assert!(steps_html.contains(r#"const viewerConfig = {"walkRevealSteps":true};"#));
    assert_eq!(final_html.matches(DISTRIBUTION_VIEWER_JS).count(), 1);
    assert_eq!(steps_html.matches(DISTRIBUTION_VIEWER_JS).count(), 1);
}

#[test]
fn public_reveal_only_changes_distribution_index() {
    let omitted = stable_public_outputs("# Intro\n");
    let final_mode =
        stable_public_outputs("---\npublic_reveal: final\n---\n# Intro\n");
    let steps_mode =
        stable_public_outputs("---\npublic_reveal: steps\n---\n# Intro\n");

    assert_eq!(omitted.manifest, final_mode.manifest);
    assert_eq!(omitted.slides, final_mode.slides);
    assert_eq!(final_mode.manifest, steps_mode.manifest);
    assert_eq!(final_mode.slides, steps_mode.slides);
    assert_eq!(omitted.pdf, final_mode.pdf);
    assert_eq!(final_mode.pdf, steps_mode.pdf);
    assert_eq!(omitted.lint, final_mode.lint);
    assert_eq!(final_mode.lint, steps_mode.lint);
}
```

Implement `stable_public_outputs` in the `main.rs` test module by building a
`WatchFixture`, calling `emit_distribution`, reading `manifest.json` as
bytes, reading every regular file under `slides/` into a filename-keyed
`BTreeMap<OsString, Vec<u8>>`, and retaining the exact strings returned by
`render_pdf_document` and `render_lint_document`.

**Implementation (code):**

- Add a private serde-serializable `DistributionViewerConfig` in `render.rs`
  with camel-case `walk_reveal_steps: bool`.
- Change `render_distribution_index` to accept `PublicReveal`, derive the
  boolean by matching the enum, serialize the config with `serde_json`, and
  embed it beside the viewer's slide state as `const viewerConfig = ...;`.
  There is no mode-specific bundle or bootstrap.
- Have `emit_distribution` pass
  `artifacts.rendered.settings().public_reveal()`. No manifest builder, slide
  renderer, PDF renderer, lint renderer, present path, or preview path reads
  the setting.
- Update existing `render_distribution_index` test calls to pass
  `PublicReveal::Final`. Do not add another runtime asset, an index hash, or a
  second `include_str!`.

**Verification (exact commands):**

```sh
cargo test -p peitho-core distribution_index_embeds_typed_viewer_config
cargo test -p peitho public_reveal_only_changes_distribution_index
cargo test -p peitho-core distribution_index
git diff --check -- crates/peitho-core/src/render.rs crates/peitho/src/main.rs
```

## Task 5: Wire one bootstrap to the shared runtime and rebuild bundles

**Goal:** Make the existing distribution bootstrap consume the navigator and
step-state APIs while preserving script rehydration, announcements,
interactive-content guards, gestures, and final-mode navigation.

**Files:**

- `packages/peitho-present/src/viewer.ts`
- `packages/peitho-present/src/stepState.ts`
- `packages/peitho-present/test/viewer.test.ts`
- `packages/peitho-present/dist/shell.js`
- `packages/peitho-present/dist/preview.js`
- `packages/peitho-present/dist/remote.js`
- `packages/peitho-present/dist/viewer.js`
- `crates/peitho-core/src/render.rs`

**Test (code):** Keep navigation and URL expectations in
`test/viewer.test.ts`. Replace Rust tests that parse inline implementations
with one structural wiring test.

```rust
#[test]
fn distribution_index_wires_one_viewer_bootstrap() {
    let html = render_distribution_index(
        AspectRatio::Ratio16To9,
        &DeckLang::default(),
        PublicReveal::Steps,
    );
    let bootstrap = html.split_once(DISTRIBUTION_VIEWER_JS).unwrap().1;

    assert_eq!(html.matches("<script>").count(), 1);
    assert_eq!(html.matches(DISTRIBUTION_VIEWER_JS).count(), 1);
    assert!(bootstrap.contains("PeithoViewer.createViewerNavigator({"));
    assert!(bootstrap.contains("PeithoViewer.readViewerPosition("));
    assert!(bootstrap.contains("PeithoViewer.writeViewerPosition("));
    assert!(bootstrap.contains("history.replaceState"));
    assert!(!bootstrap.contains("history.pushState"));
    assert!(bootstrap.contains("PeithoViewer.applyStepState("));
    assert_eq!(
        bootstrap.matches("PeithoViewer.createStepStateStyle(document)").count(),
        1,
    );
    assert!(!bootstrap.contains("function parsePositiveInt"));
    assert!(!bootstrap.contains("resolveStepTarget("));
}
```

This Rust test proves only bundle/config/bootstrap wiring. The Vitest tests from
Task 3 remain the sole tests of movement, boundaries, clamping, and URL
semantics.

**Implementation (code):**

- Add `createStepStateStyle` and `applyStepState` to `viewer.ts` and its exact
  export-surface assertion, alongside the Task 3 export surface.
- Keep one bootstrap in `render_distribution_index`. When loading the
  manifest, retain `index`, `key`, `html`, and `revealSteps`; do not retain
  or pass `skip`. Create one navigator from
  `{ slides: slides.map(({ revealSteps }) => ({ revealSteps })),
  walkRevealSteps: viewerConfig.walkRevealSteps }`.
- If `walkRevealSteps` is true, append
  `PeithoViewer.createStepStateStyle(document)` to `document.head` once after
  config is available and before the first slide is shown. Do not append it in
  final mode; its only consumers are attributes produced by `applyStepState`.
- Replace `currentIndex` with a `ViewerPosition`. Initial load and
  `popstate` use `readViewerPosition`; Home/End use `navigator.direct`;
  sequential keyboard, click, and swipe gestures use `navigator.move`; every
  accepted transition uses `writeViewerPosition`.
- On an index change, inject HTML, rehydrate scripts once, apply step state in
  steps mode, then announce each connected slide section. On a step-only
  change, apply state and update the URL without replacing DOM, rerunning
  scripts, or announcing a mount. If the navigator returns the current
  position, do nothing.
- In final mode, follow the same bootstrap and navigator path but do not call
  `applyStepState`. The navigator always holds the full step count,
  next/previous changes one slide, and URL writes remove `step`; therefore the
  light DOM remains fully visible with no active stepped emphasis.
- Remove or refocus the Rust behavior-string tests
  `distribution_index_does_not_remount_the_current_slide`,
  `distribution_index_reads_slide_query_on_load`,
  `distribution_index_supports_hash_fallback`,
  `distribution_index_updates_url_on_slide_change`,
  `distribution_index_never_uses_pushstate`,
  `distribution_index_handles_popstate`, and
  `distribution_index_preserves_query_and_hash_when_writing_slide`.
  Retain structural HTML, script-tokenizer, layout-script announcement, and
  interactive-target wiring coverage.
- Run `npm run build` and retain generated changes to the four listed bundles.
  Keep preview behavior unchanged.

**Verification (exact commands):**

```sh
(cd packages/peitho-present && npm run build && npx vitest run test/viewer.test.ts test/stepnav.test.ts test/stepState.test.ts test/loads-handles-navigates-invalid-previousIndex-keyboard-fetch.test.ts && npm run typecheck)
cargo test -p peitho-core distribution_index
git diff --check -- packages/peitho-present/src/viewer.ts packages/peitho-present/src/stepState.ts packages/peitho-present/test/viewer.test.ts packages/peitho-present/dist/shell.js packages/peitho-present/dist/preview.js packages/peitho-present/dist/remote.js packages/peitho-present/dist/viewer.js crates/peitho-core/src/render.rs
```

## Task 6: Document the public reveal contract and invariants

**Goal:** Explain the author-facing setting and record the implementation
boundaries that prevent later distribution or preview drift.

**Files:**

- `site/content/guide/frontmatter.md`
- `site/content/guide/writing-decks.md`
- `CLAUDE.md`

**Test (code):** Make these documentation contract checks fail before editing
and pass after editing.

```sh
rg -q 'public_reveal' site/content/guide/frontmatter.md
rg -q 'public_reveal: steps' site/content/guide/writing-decks.md
rg -q 'missing or invalid.*step' site/content/guide/writing-decks.md
rg -q 'stepState\.ts' CLAUDE.md
rg -q 'walkRevealSteps' CLAUDE.md
```

**Implementation (code):**

- In `frontmatter.md`, add `public_reveal` to the supported keys and table;
  document only `final` and `steps`, the `final` default, distribution-only
  scope, and Issue #723's PDF boundary.
- In `writing-decks.md`, update both incremental reveal and code-line
  emphasis: `public_reveal: steps` opts published distribution navigation
  into steps; direct jumps and missing/invalid URL steps are fully revealed;
  previous-entry is fully revealed, next-entry is step 0; final/default,
  preview, PDF, and lint retain their current final presentation; stepped
  emphasis uses equality with the current step.
- In `CLAUDE.md`, update the frontmatter, incremental-reveal,
  code-line-emphasis, layout-script distribution viewer, and
  `viewer.ts`/interactive-content bullets. Record the typed setting path,
  one-bootstrap `walkRevealSteps` split, `stepState.ts` ownership, light-DOM
  style installation, URL rules, unchanged distribution `skip` behavior,
  final-mode output boundary, Issue #723, and Issue #724's third-consumer
  requirement.
- Do not claim that `dist/index.html` is byte-identical; record one viewer
  bundle and one bootstrap for both modes.

**Verification (exact commands):**

```sh
rg -q 'public_reveal' site/content/guide/frontmatter.md
rg -q 'public_reveal: steps' site/content/guide/writing-decks.md
rg -q 'missing or invalid.*step' site/content/guide/writing-decks.md
rg -q 'stepState\.ts' CLAUDE.md
rg -q 'walkRevealSteps' CLAUDE.md
make demo-site
git diff --check -- site/content/guide/frontmatter.md site/content/guide/writing-decks.md CLAUDE.md
```

## Task 7: Run final gates and real-deck Chrome E2E

**Goal:** Pass every CLAUDE.md gate and verify reveal frames plus exact
`{1|2}` stepped-emphasis movement in real light-DOM distribution pages.

**Files:** No file is intentionally edited in this acceptance task.

**Test (code):** Run the CLAUDE.md gates exactly, including three consecutive
workspace test runs and every committed-bundle drift check.

```sh
cargo test --workspace
cargo test --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
git diff --exit-code bindings/
(cd packages/peitho-present && npm run build && npm test && npm run typecheck)
git diff --exit-code packages/peitho-present/dist/shell.js
git diff --exit-code packages/peitho-present/dist/preview.js
git diff --exit-code packages/peitho-present/dist/remote.js
git diff --exit-code packages/peitho-present/dist/viewer.js
```

**Implementation (code):** Make no production change in this task. If a gate
fails, add the regression to the owning test file from Tasks 1-5, fix that
owning implementation, and restart Task 7 from the first command. Do not
commit.

**Verification (exact commands):** Create two temporary step-mode decks and
serve both from one `python3 -m http.server`. The checked-in emphasis example
uses `{2|4-6|9}`; rewrite only its temporary copy to `{1|2}` so this E2E
checks the requested two-step equality case without changing the example.

```sh
e2e_root="$(mktemp -d /tmp/peitho-722-e2e.XXXXXX)"
rsync -a --exclude '.git' --exclude 'dist/' --exclude 'node_modules/' --exclude '.peitho/' /Users/mizzy/src/github.com/mizzy/decks-yapc-tokyo-2026/ "$e2e_root/yapc/"
perl -0pi -e 's/\A---\n/---\npublic_reveal: steps\n/' "$e2e_root/yapc/decks/yapc-tokyo-2026/deck.md"
cargo run -p peitho -- build "$e2e_root/yapc/decks/yapc-tokyo-2026/deck.md" --out "$e2e_root/yapc-dist"
yapc_slide="$(jq -er '.slides[] | select(.key == "ch4-incident-steps") | .index + 1' "$e2e_root/yapc-dist/manifest.json")"

cp examples/code-emphasis/deck.md "$e2e_root/code-emphasis.md"
perl -0pi -e 's/\A/---\npublic_reveal: steps\n---\n\n/' "$e2e_root/code-emphasis.md"
perl -0pi -e 's/\{2\|4-6\|9\}/\{1|2\}/' "$e2e_root/code-emphasis.md"
rg -n -F '{1|2}' "$e2e_root/code-emphasis.md"
cargo run -p peitho -- build "$e2e_root/code-emphasis.md" --out "$e2e_root/emphasis-dist"
emphasis_slide="$(jq -er '.slides[] | select(.key == "stepping-through-a-function") | .index + 1' "$e2e_root/emphasis-dist/manifest.json")"

python3 -m http.server 8765 --directory "$e2e_root" >"$e2e_root/http.log" 2>&1 &
e2e_server_pid=$!
for attempt in {1..50}; do curl -fsS "http://127.0.0.1:8765/yapc-dist/manifest.json" >/dev/null && break; sleep 0.1; done
curl -fsS "http://127.0.0.1:8765/yapc-dist/manifest.json" >/dev/null
curl -fsS "http://127.0.0.1:8765/emphasis-dist/manifest.json" >/dev/null
open -a "Google Chrome" "http://127.0.0.1:8765/yapc-dist/?slide=${yapc_slide}&step=0"
open -a "Google Chrome" "http://127.0.0.1:8765/emphasis-dist/?slide=${emphasis_slide}&step=0"
```

In the YAPC tab, confirm frame 1 at step 0; ArrowRight advances through steps
1, 2, and 3 and frames 2, 3, and 4; the next ArrowRight enters
`ch4-cause` at step 0; ArrowLeft returns to `ch4-incident-steps` fully
revealed at step 3. Reload step 1 and confirm frame 2. Load the same slide once
without `step` and once with `step=broken`; both land fully revealed.

In the code-emphasis tab, confirm step 0 has no active emphasis. ArrowRight to
step 1 must emphasize code line 1; ArrowRight to step 2 must remove activation
from line 1 and emphasize only line 2; ArrowLeft must restore line 1. Confirm
the URL's `step` value changes on each transition. Then stop the server:

```sh
kill "$e2e_server_pid"
```

## Completion

Issue #722 is complete when the typed setting, shared three-consumer step-state
API, TypeScript navigator and URL contract, one distribution bootstrap,
unchanged slide/manifest/PDF/lint outputs, documentation, generated bundles,
automated gates, and both Chrome walkthroughs pass. `dist/index.html` is
allowed to differ, and this plan-writing task makes no commit.
