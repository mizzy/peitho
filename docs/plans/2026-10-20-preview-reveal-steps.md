# Preview stage reveal steps (Issue #724)

Date: 2026-10-20

## File map

| Path | Planned change |
| --- | --- |
| `packages/peitho-present/src/preview.ts` | Add preview step position, shared step navigation, stage-only DOM state/style application, edit-aware step transitions, and `{mode,index,step}` session persistence. Keep the keyboard installer as a request-only component. |
| `packages/peitho-present/test/preview.test.ts` | Pin step walking, skip behavior, direct/full-reveal moves, stage-versus-grid/thumbnail rendering, interactive target ownership, editing settlement, note flushing, reload restoration, legacy state, and announcement order. |
| `packages/peitho-present/dist/preview.js` | Rebuild the committed browser bundle from `src/preview.ts`; the other three committed bundles must remain drift-free. |
| `README.md` | Describe preview stage stepping, the vertical/direct full-reveal split, and step-preserving rebuilds. |
| `site/content/guide/getting-started.md` | Update the short preview walkthrough and reload-state description. |
| `site/content/guide/cli.md` | Document the exact preview keys and how step transitions interact with notes, inline editing, and whole-slide source editing. |
| `site/content/guide/writing-decks.md` | Replace the incremental-reveal and stepped-emphasis statements that say preview is final-only: the stage walks steps, while filmstrip/grid and lint remain final. |
| `site/content/guide/frontmatter.md` | Clarify that `public_reveal` controls published/PDF output, not the always-step-aware preview stage. |
| `site/content/examples/incremental-reveal.md` | Clarify that the preview stage walks reveal steps while filmstrip/grid, lint, and gallery screenshots remain final. |
| `site/content/examples/code-emphasis.md` | Clarify that stepped emphasis follows the preview stage step while filmstrip/grid, lint, and gallery screenshots remain final. |
| `CLAUDE.md` | Update the preview, skip, interactive-target, incremental-reveal, code-emphasis, editable-notes, and inline-edit invariants for Issue #724. |

The following shared modules are dependencies, not edit targets:

- `packages/peitho-present/src/stepnav.ts` remains the only sequential-step
  algorithm. Preview must call `revealStepCount`, `clampStep`, and
  `resolveStepTarget`; it must not reproduce their branches or call
  `nextNonSkippedIndex` for horizontal/Page navigation.
- `packages/peitho-present/src/stepState.ts` remains the only reveal/emphasis
  DOM implementation. Preview must call `createStepStateStyle` and
  `applyStepState` without copying their selectors or comparisons.
- `packages/peitho-present/src/interactiveTarget.ts` remains the only key and
  pointer ownership policy. Its existing `keyBelongsToTarget` call stays ahead
  of preview key mapping, so slide controls keep their keys.
- `crates/peitho-core/src/render.rs` keeps the existing preview `pagehide`
  disposal and `pageshow.persisted` reload bootstrap. No Rust contract,
  manifest, preview-cache, binding, example-deck source, filmstrip, or
  grid-output change is needed.

## Task 1: Make the single-mode stage a shared step navigator

**Goal:** Walk reveal and stepped-emphasis positions only on the large
single-mode stage. Horizontal arrows and PageUp/PageDown use the present
semantics, while ArrowUp/ArrowDown remain skip-aware slide moves that land fully
revealed, every direct/open operation lands fully revealed, grid navigation is
unchanged, and decks with zero reveal steps behave exactly as before.

**Files:**

- `packages/peitho-present/src/preview.ts`
- `packages/peitho-present/test/preview.test.ts`

**Test (concrete code):** First extend the existing manifest helper rather than
building a preview-only navigator fixture:

```ts
function manifestWithSlides(
  slides: Array<{ key: string; skip?: boolean; revealSteps?: number }>
): typeof manifest {
  return {
    ...manifest,
    slideCount: slides.length,
    slides: slides.map((slide, index) => ({
      index,
      key: slide.key,
      src: `slides/${String(index).padStart(3, "0")}-${slide.key}.html`,
      hasNotes: false,
      skip: slide.skip ?? false,
      revealSteps: slide.revealSteps ?? 0,
      text: { title: "", body: "", code: "" }
    }))
  };
}
```

Add a controller-level test that proves preview is using the shared position
contract, including skipped slides and the key split:

```ts
it("walks steps horizontally while vertical and direct moves land fully revealed", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  sessionStorage.setItem(
    "peitho:preview-state",
    JSON.stringify({ mode: "single", index: 0, step: 0 })
  );
  const deck = manifestWithSlides([
    { key: "intro", revealSteps: 2 },
    { key: "skip", skip: true, revealSteps: 4 },
    { key: "middle", revealSteps: 1 }
  ]);
  const shell = await mountPreviewShell({
    root,
    bus,
    fetcher: fetchForManifest(deck),
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);

  shell.navigate("next");
  expect([shell.currentIndex, shell.currentStep]).toEqual([0, 1]);
  shell.navigate("next");
  expect([shell.currentIndex, shell.currentStep]).toEqual([0, 2]);
  shell.navigate("next");
  expect([shell.currentIndex, shell.currentStep]).toEqual([2, 0]);
  shell.navigate("prev");
  expect([shell.currentIndex, shell.currentStep]).toEqual([0, 2]);

  shell.navigate("down");
  expect([shell.currentIndex, shell.currentStep]).toEqual([2, 1]);
  shell.navigate("up");
  expect([shell.currentIndex, shell.currentStep]).toEqual([0, 2]);
  shell.navigate("last");
  expect([shell.currentIndex, shell.currentStep]).toEqual([2, 1]);
  shell.navigate("first");
  expect([shell.currentIndex, shell.currentStep]).toEqual([0, 2]);
});
```

Pin present-compatible event details across step, overview, and grid index
transitions:

```ts
it("dispatches present-compatible slide and step change details", async () => {
  const bus = new EventTarget();
  const slideChanges: SlideChangeDetail[] = [];
  const stepChanges: StepChangeDetail[] = [];
  bus.addEventListener("peitho:slidechange", (event) => {
    slideChanges.push((event as CustomEvent<SlideChangeDetail>).detail);
  });
  bus.addEventListener("peitho:stepchange", (event) => {
    stepChanges.push((event as CustomEvent<StepChangeDetail>).detail);
  });
  sessionStorage.setItem(
    "peitho:preview-state",
    JSON.stringify({ mode: "single", index: 0, step: 0 })
  );
  const root = document.createElement("main");
  const deck = manifestWithSlides([
    { key: "intro", revealSteps: 2 },
    { key: "middle", revealSteps: 1 }
  ]);
  const shell = await mountPreviewShell({
    root,
    bus,
    fetcher: fetchForManifest(deck),
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);

  expect(slideChanges).toEqual([
    {
      key: "intro",
      index: 0,
      total: 2,
      previousIndex: null,
      step: 0,
      stepCount: 2
    }
  ]);
  expect(stepChanges).toEqual([]);

  shell.navigate("next");
  expect(slideChanges).toHaveLength(1);
  expect(stepChanges).toEqual([{ index: 0, step: 1, stepCount: 2 }]);

  bus.dispatchEvent(
    new CustomEvent("peitho:overviewrequest", { detail: { action: "enter" } })
  );
  expect(stepChanges).toEqual([
    { index: 0, step: 1, stepCount: 2 },
    { index: 0, step: 2, stepCount: 2 }
  ]);
  shell.navigate("next");
  expect(slideChanges.at(-1)).toEqual({
    key: "middle",
    index: 1,
    total: 2,
    previousIndex: 0,
    step: 1,
    stepCount: 1
  });
  expect(stepChanges).toHaveLength(2);
});
```

Make the same-index mode-transition edge explicit: entering grid from a
partial step reports the newly committed full step, while exiting at that same
full step emits nothing further.

```ts
it("dispatches the fully revealed step when entering grid from a partial step", async () => {
  const bus = new EventTarget();
  const stepChanges: StepChangeDetail[] = [];
  bus.addEventListener("peitho:stepchange", (event) => {
    stepChanges.push((event as CustomEvent<StepChangeDetail>).detail);
  });
  sessionStorage.setItem(
    "peitho:preview-state",
    JSON.stringify({ mode: "single", index: 0, step: 1 })
  );
  const root = document.createElement("main");
  const deck = manifestWithSlides([{ key: "intro", revealSteps: 2 }]);
  const shell = await mountPreviewShell({
    root,
    bus,
    fetcher: fetchForManifest(deck),
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);

  bus.dispatchEvent(
    new CustomEvent("peitho:overviewrequest", { detail: { action: "enter" } })
  );
  expect([shell.mode, shell.currentStep]).toEqual(["grid", 2]);
  expect(stepChanges).toEqual([{ index: 0, step: 2, stepCount: 2 }]);

  bus.dispatchEvent(
    new CustomEvent("peitho:overviewrequest", { detail: { action: "exit" } })
  );
  expect([shell.mode, shell.currentStep]).toEqual(["single", 2]);
  expect(stepChanges).toEqual([{ index: 0, step: 2, stepCount: 2 }]);
});
```

Pin the two UI open paths and the zero-step compatibility case explicitly:

```ts
it("filmstrip clicks and grid opens land fully revealed", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  sessionStorage.setItem(
    "peitho:preview-state",
    JSON.stringify({ mode: "single", index: 0, step: 0 })
  );
  const deck = manifestWithSlides([
    { key: "intro", revealSteps: 2 },
    { key: "middle", revealSteps: 1 }
  ]);
  const shell = await mountPreviewShell({
    root,
    bus,
    fetcher: fetchForManifest(deck),
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);

  mockSelection(true);
  stageTile(root, "intro").click();
  expect([shell.currentIndex, shell.currentStep]).toEqual([0, 0]);

  root.querySelectorAll<HTMLElement>(".peitho-preview-thumb")[1].click();
  expect([shell.currentIndex, shell.currentStep]).toEqual([1, 1]);

  bus.dispatchEvent(
    new CustomEvent("peitho:overviewrequest", { detail: { action: "enter" } })
  );
  bus.dispatchEvent(
    new CustomEvent("peitho:navigate", { detail: { to: { index: 0 } } })
  );
  bus.dispatchEvent(
    new CustomEvent("peitho:overviewrequest", { detail: { action: "activate" } })
  );
  expect([shell.mode, shell.currentIndex, shell.currentStep]).toEqual([
    "single",
    0,
    2
  ]);
});

it("clicking the current slide thumbnail at a partial step lands fully revealed", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  sessionStorage.setItem(
    "peitho:preview-state",
    JSON.stringify({ mode: "single", index: 0, step: 1 })
  );
  const deck = manifestWithSlides([{ key: "intro", revealSteps: 2 }]);
  const shell = await mountPreviewShell({
    root,
    bus,
    fetcher: fetchForManifest(deck),
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);

  root.querySelector<HTMLElement>(".peitho-preview-thumb")!.click();

  expect([shell.mode, shell.currentIndex, shell.currentStep]).toEqual([
    "single",
    0,
    2
  ]);
});

it("keeps whole-slide skip navigation for a deck without reveal steps", async () => {
  const root = document.createElement("main");
  sessionStorage.setItem(
    "peitho:preview-state",
    JSON.stringify({ mode: "single", index: 0 })
  );
  const deck = manifestWithSlides([
    { key: "intro" },
    { key: "appendix", skip: true },
    { key: "summary" }
  ]);
  const shell = await mountPreviewShell({
    root,
    bus: new EventTarget(),
    fetcher: fetchForManifest(deck),
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);

  shell.navigate("next");
  expect([shell.currentIndex, shell.currentStep]).toEqual([2, 0]);
  shell.navigate("prev");
  expect([shell.currentIndex, shell.currentStep]).toEqual([0, 0]);
});
```

Also add `currentStep === 0` assertions to the existing zero-step skip tests;
the dedicated test above and those unchanged paths jointly pin today's
one-slide-at-a-time behavior.

**Implementation (concrete code):**

- Add `currentStep: number` to `PreviewShell` and
  `PreviewShellController`. Import `StepPosition`, `clampStep`,
  `revealStepCount`, and `resolveStepTarget` from `stepnav.ts`.
- Change `resolveTarget` to return `StepPosition | null`. In single mode,
  `next`/`prev` must be exactly:

  ```ts
  resolveStepTarget(
    this.slides.map((slide) => slide.meta),
    { index: this.currentIndex, step: this.currentStep },
    direction
  );
  ```

  Grid `next`/`prev` and grid vertical movement keep their current index math.
  Single-mode `up`/`down` keep calling the existing skip-aware
  `resolveSequentialTarget`, but project the returned index to
  `{index, step: revealStepCount(slide.meta)}`. `first`, `last`, `{index}`,
  `{key}`, thumbnail clicks, grid activation, and mode changes use that same
  direct/full-reveal projection. Ignore an optional `NavigateTarget.index.step`
  in preview: it is a direct jump and therefore final by contract.
- Split the shared tile click by its current role: when `mode === "grid"`, it
  opens the chosen tile at that slide's full step count; when the tile is the
  already-open single-mode stage and no inline edit starts, pass
  `this.currentStep` so an ordinary background click remains the existing
  no-op instead of becoming an undocumented reveal-all gesture.
- Change `navigateToTarget`, `setIndex`, and `commitTransition` to carry
  `{index,step}`. Clamp only through `clampStep(step, revealStepCount(meta))`.
  Include `step` in the no-op comparison, update `currentStep` inside the
  transition commit. Import `SlideChangeDetail` and `StepChangeDetail` from
  `shell.ts` rather than defining preview-local versions. An index change
  dispatches `peitho:slidechange` with present's complete
  `{key,index,total,previousIndex,step,stepCount}` detail. When the index is
  unchanged but the committed step changes, dispatch `peitho:stepchange` with
  present's `{index,step,stepCount}` detail regardless of whether the mode also
  changed. Thus entering grid from a partial step reports the full step;
  exiting the same grid selection emits nothing because full remains full.
  Index changes, including grid index moves, keep only their slide event. Pin
  these exact details in
  `dispatches present-compatible slide and step change details`. UI and
  keyboard code continue to emit only `peitho:navigate`; the shell remains the
  sole transition owner under the §16 event contract.
  `activateSelection`'s same-position edit settlement must pass
  `this.currentStep` so focusing notes cannot turn a partial stage into a
  direct/full-reveal jump; entering or exiting grid still passes the selected
  slide's full step count.

**Verification command:**

```sh
(cd packages/peitho-present && npx vitest run test/preview.test.ts test/stepnav.test.ts && npm run typecheck)
```

## Task 2: Give every stage host its own shared-module step style

**Goal:** Append one reveal/emphasis style from `stepState.ts` to every stage
host when `createStageHost` creates it. Keep those styles attached when stage
hosts become grid tiles, never install them in filmstrip thumbnail hosts, apply
the active single-stage step without remounting, and reset stage attributes to
the one-past-final state exactly once on each transition into grid. Preserve all
`interactiveTarget.ts` ownership rules.

**Files:**

- `packages/peitho-present/src/preview.ts`
- `packages/peitho-present/test/preview.test.ts`

**Test (concrete code):**

Pin the style ownership and active/final attribute states:

```ts
it("installs one step style per stage host and applies state only to the active stage", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  sessionStorage.setItem(
    "peitho:preview-state",
    JSON.stringify({ mode: "single", index: 0, step: 0 })
  );
  const deck = manifestWithSlides([
    { key: "intro", revealSteps: 3 },
    { key: "end", revealSteps: 3 }
  ]);
  const base = previewFetchFixture(deck);
  const fetcher = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    if (String(input).startsWith("slides/")) {
      return okText(
        '<section><p data-reveal-step="1">A</p>' +
          '<p data-reveal-step="2">B</p>' +
          '<span data-emphasis-step="3">code</span></section>'
      );
    }
    return base.fetcher(input, init);
  }) as unknown as typeof fetch;
  const shell = await mountPreviewShell({
    root,
    bus,
    fetcher,
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);

  const stage = slideShadow(root, "intro");
  const otherStage = slideShadow(root, "end");
  const thumb = slideShadow(root, "intro", true);
  const stepStyleCount = (shadow: ShadowRoot): number =>
    [...shadow.querySelectorAll("style")].filter((style) =>
      style.textContent?.includes("[data-reveal-hidden]")
    ).length;
  const hidden = (shadow: ShadowRoot): boolean[] =>
    [...shadow.querySelectorAll<HTMLElement>("[data-reveal-step]")].map((node) =>
      node.hasAttribute("data-reveal-hidden")
    );

  expect(stepStyleCount(stage)).toBe(1);
  expect(stepStyleCount(otherStage)).toBe(1);
  expect(hidden(stage)).toEqual([true, true]);
  expect(stepStyleCount(thumb)).toBe(0);
  expect(hidden(thumb)).toEqual([false, false]);
  expect(thumb.querySelector("[data-emphasis-active]")).toBeNull();

  bus.dispatchEvent(
    new CustomEvent("peitho:overviewrequest", { detail: { action: "enter" } })
  );
  expect(shell.mode).toBe("grid");
  expect(shell.currentStep).toBe(3);
  for (const host of root.querySelectorAll<HTMLElement>(".peitho-preview-slide")) {
    expect(stepStyleCount(host.shadowRoot!)).toBe(1);
    expect(host.shadowRoot!.querySelector("[data-reveal-hidden]")).toBeNull();
    expect(host.shadowRoot!.querySelector("[data-emphasis-active]")).toBeNull();
  }

  bus.dispatchEvent(
    new CustomEvent("peitho:overviewrequest", { detail: { action: "activate" } })
  );
  expect(shell.mode).toBe("single");
  expect(shell.currentStep).toBe(3);
  expect(stepStyleCount(stage)).toBe(1);
  expect(hidden(stage)).toEqual([false, false]);
  expect(stage.querySelector("[data-emphasis-active]")?.textContent).toBe("code");
});
```

Prove that entering grid, rather than each grid layout or resize, owns the
one-past-final attribute reset:

```ts
it("resets stage step attributes only when entering grid", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  sessionStorage.setItem(
    "peitho:preview-state",
    JSON.stringify({ mode: "single", index: 0, step: 0 })
  );
  const deck = manifestWithSlides([{ key: "intro", revealSteps: 1 }]);
  const base = previewFetchFixture(deck);
  const fetcher = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    if (String(input).startsWith("slides/")) {
      return okText('<section><p data-reveal-step="1">A</p></section>');
    }
    return base.fetcher(input, init);
  }) as unknown as typeof fetch;
  const shell = await mountPreviewShell({
    root,
    bus,
    fetcher,
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);
  const marker = slideShadow(root, "intro").querySelector<HTMLElement>(
    "[data-reveal-step]"
  )!;

  bus.dispatchEvent(
    new CustomEvent("peitho:overviewrequest", { detail: { action: "enter" } })
  );
  expect(marker.hasAttribute("data-reveal-hidden")).toBe(false);

  marker.setAttribute("data-reveal-hidden", "");
  window.dispatchEvent(new Event("resize"));
  expect(marker.hasAttribute("data-reveal-hidden")).toBe(true);

  bus.dispatchEvent(
    new CustomEvent("peitho:overviewrequest", { detail: { action: "exit" } })
  );
  bus.dispatchEvent(
    new CustomEvent("peitho:overviewrequest", { detail: { action: "enter" } })
  );
  expect(marker.hasAttribute("data-reveal-hidden")).toBe(false);
});
```

Keep one keyboard integration assertion for `interactiveTarget.ts`:

```ts
it("lets a stage range input own reveal-navigation keys", async () => {
  const bus = new EventTarget();
  const root = document.createElement("main");
  document.body.appendChild(root);
  cleanups.push(() => root.remove());
  sessionStorage.setItem(
    "peitho:preview-state",
    JSON.stringify({ mode: "single", index: 0, step: 0 })
  );
  const deck = manifestWithSlides([{ key: "intro", revealSteps: 1 }]);
  const base = previewFetchFixture(deck);
  const fetcher = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    if (String(input).startsWith("slides/")) {
      return okText('<section><input id="range" type="range"></section>');
    }
    return base.fetcher(input, init);
  }) as unknown as typeof fetch;
  const shell = await mountPreviewShell({
    root,
    bus,
    fetcher,
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);
  cleanups.push(installPreviewKeyboard(window, bus));

  const range = slideShadow(root, "intro").querySelector<HTMLInputElement>("#range")!;
  expect(press(range, "ArrowRight").defaultPrevented).toBe(false);
  expect(shell.currentStep).toBe(0);
  expect(press(window, "ArrowRight").defaultPrevented).toBe(true);
  expect(shell.currentStep).toBe(1);
});
```

**Implementation (concrete code):**

- Import `applyStepState` and `createStepStateStyle` from `stepState.ts`.
- In `createStageHost`, append `createStepStateStyle(this.doc)` to the newly
  built stage shadow root after `buildSlideHost` has installed reset/deck CSS
  and before inline scripts execute and the slide fragment is appended:

  ```ts
  const { host, shadow, fragment } = this.buildSlideHost(
    slide,
    html,
    css,
    "peitho-preview-slide"
  );
  shadow.appendChild(createStepStateStyle(this.doc));
  executeInlineScripts(fragment, this.doc);
  shadow.appendChild(fragment);
  ```

  Every stage host therefore owns one independent style for its lifetime.
  `createThumbHost` must not append a step style. There is no controller-level
  `stepStateStyle` field, style movement, fixed `childNodes` insertion seam, or
  style removal in grid mode or `destroy`.
- At the end of `applySingleLayout`, call
  `applyStepState(active.shadow, this.currentStep)` on the active stage only.
  A step-only transition uses that already-mounted shadow root; it does not
  replace HTML, run `executeInlineScripts`, or announce another
  `peitho:shadow-mounted`.
- Add `resetGridStepState` and call it from the committed transition only when
  `previousMode !== "grid" && mode === "grid"`:

  ```ts
  private resetGridStepState(): void {
    this.slides.forEach((slide) => {
      // One past final reveals everything and clears equality-based emphasis.
      applyStepState(slide.shadow, revealStepCount(slide.meta) + 1);
    });
  }
  ```

  Keep `applyGridLayout` free of step-state mutation so resize/layout passes do
  not repeat the reset. The stage hosts keep their styles in grid; only their
  reveal/emphasis attributes are reset. The one-past-final value reveals all
  markers and clears equality-based emphasis while `currentStep` stays at the
  selected slide's clamped manifest count.
- Keep `previewNavigationKeyMap`, `installPreviewKeyboard`,
  `interactiveTarget.ts`, `stepnav.ts`, and `stepState.ts` otherwise
  unchanged. This preserves IME/chord filtering and range/radio/text-entry key
  ownership without a preview-specific interactive check.

**Verification command:**

```sh
(cd packages/peitho-present && npx vitest run test/preview.test.ts test/stepnav.test.ts test/stepState.test.ts test/interactiveTarget.test.ts && npm run typecheck)
```

## Task 3: Preserve the editing and notes transition contract across steps

**Goal:** Treat a real step move as a transition that first settles the one
open inline or source edit, because stepping backward may hide the edited
element. A step-only move (same index and mode, different step) is the only new
case that skips the notes flush; it still settles an open edit first. Preserve
the previous notes-flush behavior for every other transition, including Enter
with an open inline/source edit: commit the edit, flush dirty notes, and only
then focus notes. A boundary no-op must not close an edit. A failed edit save
must block the step just as it blocks a slide or mode change.

**Files:**

- `packages/peitho-present/src/preview.ts`
- `packages/peitho-present/test/preview.test.ts`

**Test (concrete code):** Add a stepped inline fixture that still delegates
all POST handling to the existing edit fixture:

```ts
function steppedInlineFixture(): InlineEditFetchFixture {
  const base = inlineEditFetchFixture();
  const deck = manifestWithSlides([
    { key: "intro", revealSteps: 1 },
    { key: "middle" }
  ]);
  const fetcher = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url === "manifest.json") return okJson(deck);
    if (url === "slides/000-intro.html") {
      return okText(
        '<section><p id="always" data-peitho-src="1-7" data-peitho-md="Always">Always</p>' +
          '<p id="stepped-edit" data-reveal-step="1" data-peitho-src="8-15" ' +
          'data-peitho-md="Visible">Visible</p></section>'
      );
    }
    return base.fetcher(input, init);
  }) as unknown as typeof fetch;
  return { ...base, fetcher };
}
```

Prove edit-before-hide ordering and the step-only no-note-flush rule:

```ts
it("settles an inline edit before hiding its step without flushing same-slide notes", async () => {
  const fixture = steppedInlineFixture();
  const { root, shell } = await mountInlineEditForTest({ fixture });
  shell.navigate("next");
  expect(shell.currentStep).toBe(1);
  const note = root.querySelector<HTMLTextAreaElement>('[data-peitho-preview="note"]')!;
  const block = slideShadow(root, "intro").querySelector<HTMLElement>("#stepped-edit")!;

  note.value = "dirty note stays on this slide";
  dispatchShadowClick(block);
  block.textContent = "Saved before hide";
  shell.navigate("prev");

  await vi.waitFor(() => expect(fixture.slideEditPosts()).toHaveLength(1));
  expect(shell.currentStep).toBe(1);
  expect(block.hasAttribute("data-reveal-hidden")).toBe(false);
  expect(fixture.notesPosts()).toHaveLength(0);

  fixture.resolveSlideEditPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(shell.currentStep).toBe(0));
  expect(block.hasAttribute("data-reveal-hidden")).toBe(true);
  expect(fixture.notesPosts()).toHaveLength(0);
  expect(note.value).toBe("dirty note stays on this slide");

  shell.navigate("next");
  shell.navigate("next");
  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(1));
  expect(shell.currentIndex).toBe(0);
  fixture.resolveNotesPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(shell.currentIndex).toBe(1));
});
```

Add the failure and no-op counterparts:

```ts
it("a failed inline save blocks the requested step", async () => {
  const fixture = steppedInlineFixture();
  const { root, shell } = await mountInlineEditForTest({ fixture });
  shell.navigate("next");
  expect(shell.currentStep).toBe(1);
  const block = slideShadow(root, "intro").querySelector<HTMLElement>("#stepped-edit")!;
  dispatchShadowClick(block);
  block.textContent = "Rejected before hide";
  shell.navigate("prev");
  await vi.waitFor(() => expect(fixture.slideEditPosts()).toHaveLength(1));

  fixture.resolveSlideEditPost(errorJson(422, "replacement changes the slide"));
  await vi.waitFor(() =>
    expect(root.querySelector('[data-peitho-preview="status"]')?.textContent)
      .toContain("replacement changes the slide")
  );
  expect(shell.currentStep).toBe(1);
  expect(block.hasAttribute("data-reveal-hidden")).toBe(false);
  expect(block.getAttribute("contenteditable")).toBe("plaintext-only");
});

it("a boundary no-op does not settle an open edit", async () => {
  const fixture = steppedInlineFixture();
  const { root, shell } = await mountInlineEditForTest({ fixture });
  shell.navigate("prev");
  expect(shell.currentStep).toBe(0);
  const always = slideShadow(root, "intro").querySelector<HTMLElement>("#always")!;
  dispatchShadowClick(always);
  always.textContent = "Still editing";
  shell.navigate("prev");
  expect(shell.currentStep).toBe(0);
  expect(fixture.slideEditPosts()).toHaveLength(0);
  expect(always.getAttribute("contenteditable")).toBe("plaintext-only");
});
```

Use the existing `sourceEditFetchFixture` with a manifest whose first slide has
one step and assert that `e` still replaces the stage, Escape restores the
same step, and unshifted PageUp/PageDown still take the existing
commit-before-transition path:

```ts
it("whole-slide source edit preserves and reapplies the current step", async () => {
  const bus = new EventTarget();
  const base = sourceEditFetchFixture();
  const deck = manifestWithSlides([
    { key: "intro", revealSteps: 1 },
    { key: "middle" },
    { key: "end" }
  ]);
  const fetcher = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input);
    if (url === "manifest.json") return okJson(deck);
    if (url === "slides/000-intro.html") {
      return okText('<section><p data-reveal-step="1">Visible</p></section>');
    }
    return base.fetcher(input, init);
  }) as unknown as typeof fetch;
  const fixture: SourceEditFetchFixture = { ...base, fetcher };
  const { root, shell } = await mountInlineEditForTest({
    bus,
    fixture
  });
  cleanups.push(installPreviewKeyboard(window, bus));
  shell.navigate("next");
  expect(shell.currentStep).toBe(1);

  const editor = openSourceEditor(root, bus);
  expect(shell.currentStep).toBe(1);
  expect(stageTile(root).querySelector<HTMLElement>(".peitho-preview-slide")!.hidden)
    .toBe(true);
  press(editor, "Escape");
  expect(shell.currentStep).toBe(1);
  expect(slideShadow(root, "intro").querySelector("[data-reveal-hidden]")).toBeNull();

  const committing = openSourceEditor(root, bus);
  committing.value = "# Intro edited";
  expect(press(committing, "PageUp").defaultPrevented).toBe(true);
  await vi.waitFor(() => expect(fixture.sourceEditPosts()).toHaveLength(1));
  expect(shell.currentStep).toBe(1);
  fixture.resolveSourceEditPost(okJson({ key: "intro", body: "# Intro edited" }));
  await vi.waitFor(() => expect(shell.currentStep).toBe(0));
  expect(
    slideShadow(root, "intro")
      .querySelector<HTMLElement>("[data-reveal-step]")!
      .hasAttribute("data-reveal-hidden")
  ).toBe(true);
});
```

Pin the pre-existing Enter path separately so the step-only optimization
cannot suppress its dirty-note flush:

```ts
it("activate selection commits the source edit then flushes dirty notes before focusing", async () => {
  const bus = new EventTarget();
  const fixture = sourceEditFetchFixture();
  const { root } = await mountInlineEditForTest({ bus, fixture });
  const note = root.querySelector<HTMLTextAreaElement>(
    '[data-peitho-preview="note"]'
  )!;
  note.value = "Flush me before focus";
  const editor = openSourceEditor(root, bus);
  editor.value = "# Saved before notes";

  bus.dispatchEvent(
    new CustomEvent("peitho:overviewrequest", { detail: { action: "activate" } })
  );

  await vi.waitFor(() => expect(fixture.sourceEditPosts()).toHaveLength(1));
  expect(fixture.notesPosts()).toHaveLength(0);
  expect(document.activeElement).toBe(editor);
  fixture.resolveSourceEditPost(okJson({ key: "intro", body: "# Saved before notes" }));

  await vi.waitFor(() => expect(fixture.notesPosts()).toHaveLength(1));
  expect(JSON.parse(fixture.notesPosts()[0][1].body as string)).toEqual({
    key: "intro",
    text: "Flush me before focus"
  });
  expect(document.activeElement).not.toBe(note);
  fixture.resolveNotesPost(okJson({ saved: true }));
  await vi.waitFor(() => expect(document.activeElement).toBe(note));
});
```

**Implementation (concrete code):**

- Compute the step-only classification before changing state, then preserve the
  existing edit/key/mode flush conditions behind that one exception:

  ```ts
  const stepOnlyMove =
    index === this.currentIndex && mode === this.mode && step !== this.currentStep;
  const editOpen = this.isEditOpen();
  const needsFlush =
    !stepOnlyMove &&
    (editOpen ||
      (mode === "grid" && this.mode === "single") ||
      (mode === "single" &&
        this.slides[index]?.meta.key !== this.notesTextareaKey));
  ```

- Pass `needsFlush` into `settleForTransition`. That method always awaits
  `commitActiveEdit()` first for every accepted index/mode/step transition,
  checks the transition sequence, and returns immediately when
  `needsFlush` is false. Only when it is true does it run the existing
  `while (!this.notesSettled()) await this.flushNotes()` loop.
- Keep `activateSelection`'s transition call for an open edit even when its
  index, mode, and step are unchanged. Such a request is not `stepOnlyMove`;
  `editOpen` makes `needsFlush` true, so Enter commits the edit, flushes the
  dirty note, and then runs the focus callback.
- Do not special-case inline versus source editing in the navigation resolver.
  The closed `ActiveEdit` union and `commitActiveEdit` remain the single
  settlement path. Apply the new step only inside the transition's `commit`
  callback, after a successful edit settlement; this guarantees an edited
  node is never hidden while its save is still pending.
- Keep resolver boundary results as `null`. They never call
  `commitTransition`, so an edit is not closed when no movement exists.
- Do not add a click guard for unrevealed content. Real CSS visibility makes a
  future-step element untargetable; once revealed, the existing composed-path
  inline-edit logic applies. Keep `e` independent of reveal visibility because
  it replaces the complete stage.

**Verification command:**

```sh
(cd packages/peitho-present && npx vitest run test/preview.test.ts -t 'step|source edit|inline edit|notes' && npm run typecheck)
```

## Task 4: Persist, restore, and remount the exact step

**Goal:** Save `{mode,index,step}` for normal saves, page exit, deferred
generation reloads, and BFCache-triggered reloads. Restore a valid
non-negative integer step and clamp it to the rebuilt slide; treat missing or
invalid values as fully revealed so legacy `{mode,index}` state keeps working.
Apply the restored state on the newly mounted stage after its scripts have
been rehydrated and before its `peitho:shadow-mounted` announcement observes
the final layout.

**Files:**

- `packages/peitho-present/src/preview.ts`
- `packages/peitho-present/test/preview.test.ts`

**Test (concrete code):** Add a table test for every restoration branch:

```ts
it.each([
  ["legacy missing", "single", undefined, 2],
  ["null", "single", null, 2],
  ["string", "single", "1", 2],
  ["negative", "single", -1, 2],
  ["fraction", "single", 1.5, 2],
  ["beyond maximum", "single", 99, 2],
  ["exact zero", "single", 0, 0],
  ["exact middle", "single", 1, 1],
  ["grid ignores a partial step", "grid", 0, 2]
])("restores %s as step %s", async (_case, mode, savedStep, expectedStep) => {
  const state: Record<string, unknown> = { mode, index: 0 };
  if (savedStep !== undefined) state.step = savedStep;
  sessionStorage.setItem("peitho:preview-state", JSON.stringify(state));
  const root = document.createElement("main");
  const deck = manifestWithSlides([{ key: "intro", revealSteps: 2 }]);
  const shell = await mountPreviewShell({
    root,
    fetcher: fetchForManifest(deck),
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);

  expect(shell.currentIndex).toBe(0);
  expect(shell.currentStep).toBe(expectedStep);
});
```

Change `generation changes save preview state before reloading` into a stepped
single-mode test and require the complete state:

```ts
it("generation reload saves and remounts the current step", async () => {
  const bus = new EventTarget();
  const deck = manifestWithSlides([{ key: "intro", revealSteps: 2 }]);
  sessionStorage.setItem(
    "peitho:preview-state",
    JSON.stringify({ mode: "single", index: 0, step: 0 })
  );
  const firstRoot = document.createElement("main");
  const first = await mountPreviewShell({
    root: firstRoot,
    bus,
    fetcher: fetchForManifest(deck),
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(first);
  const channel = mockChannel();
  const reload = vi.fn();
  cleanups.push(installPreviewReload(first, () => channel, reload));

  first.navigate("next");
  channel.onmessage?.({ data: { generation: first.generation + 1 } });
  expect(JSON.parse(sessionStorage.getItem("peitho:preview-state")!)).toEqual({
    mode: "single",
    index: 0,
    step: 1
  });
  expect(reload).toHaveBeenCalledTimes(1);

  first.destroy();
  shells.pop();
  const second = await mountPreviewShell({
    root: document.createElement("main"),
    bus: new EventTarget(),
    fetcher: fetchForManifest(deck),
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(second);
  expect(second.currentStep).toBe(1);

  window.dispatchEvent(new Event("pagehide"));
  expect(JSON.parse(sessionStorage.getItem("peitho:preview-state")!)).toMatchObject({
    mode: "single",
    index: 0,
    step: 1
  });
});
```

Also pin grid persistence to the selected slide's final step, regardless of a
stale partial value in stored grid state:

```ts
it("generation reload saves a grid selection at its fully revealed step", async () => {
  const deck = manifestWithSlides([
    { key: "intro", revealSteps: 1 },
    { key: "middle", revealSteps: 3 }
  ]);
  sessionStorage.setItem(
    "peitho:preview-state",
    JSON.stringify({ mode: "grid", index: 1, step: 0 })
  );
  const shell = await mountPreviewShell({
    root: document.createElement("main"),
    fetcher: fetchForManifest(deck),
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);
  const reload = vi.fn();

  shell.requestGenerationReload(shell.generation + 1, reload);

  expect(JSON.parse(sessionStorage.getItem("peitho:preview-state")!)).toEqual({
    mode: "grid",
    index: 1,
    step: 3
  });
  expect(reload).toHaveBeenCalledTimes(1);
});
```

Remount a second shell against that stored state and assert `currentStep === 1`
and the matching reveal attributes. Dispatch `pagehide` at another step and
assert the same three position fields are written. Preserve the existing
`pagehide_before_load_keeps_the_stored_state` assertion byte-for-byte: an
unloaded shell must not replace even legacy state.

Pin the script/layout/announcement order with a fresh-remount test:

```ts
it("applies a restored step before the remounted stage announcement", async () => {
  sessionStorage.setItem(
    "peitho:preview-state",
    JSON.stringify({ mode: "single", index: 0, step: 1 })
  );
  const deck = manifestWithSlides([{ key: "intro", revealSteps: 2 }]);
  const base = previewFetchFixture(deck);
  const fetcher = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    if (String(input).startsWith("slides/")) {
      return okText(
        '<section><script>window.previewMounted = true</script>' +
          '<p data-reveal-step="1">A</p><p data-reveal-step="2">B</p></section>'
      );
    }
    return base.fetcher(input, init);
  }) as unknown as typeof fetch;
  let stateAtAnnouncement: boolean[] | null = null;
  let announcements = 0;
  const listener: EventListener = (event) => {
    const detail = (event as CustomEvent<ShadowMountedDetail>).detail;
    if (detail.key !== "intro") return;
    announcements += 1;
    expect(detail.root.querySelector("script")?.textContent).toContain("(function () {");
    stateAtAnnouncement = [
      ...detail.root.querySelectorAll<HTMLElement>("[data-reveal-step]")
    ].map((node) => node.hasAttribute("data-reveal-hidden"));
  };
  document.addEventListener(SHADOW_MOUNTED_EVENT, listener);
  cleanups.push(() => document.removeEventListener(SHADOW_MOUNTED_EVENT, listener));

  const shell = await mountPreviewShell({
    root: document.createElement("main"),
    fetcher,
    window,
    storage: sessionStorage,
    viewport: () => ({ width: 1280, height: 720 })
  });
  shells.push(shell);
  expect(stateAtAnnouncement).toEqual([false, true]);
  expect(announcements).toBe(1);
  shell.navigate("next");
  expect(shell.currentStep).toBe(2);
  expect(announcements).toBe(1);
});
```

Update every exact `peitho:preview-state` assertion in `preview.test.ts` to
include `step`, while retaining the stronger checks that note drafts keep only
note text and that inline/source/discarded slide edits never enter storage.
In particular, deferred reload tests must assert the destination step, not the
pre-transition step.

**Implementation (concrete code):**

- Keep writes and backward-compatible reads distinct so omission cannot leak
  back into new state:

  ```ts
  type PreviewState = {
    mode: PreviewMode;
    index: number;
    step: number;
    draft?: PreviewDraft;
  };

  type RestoredPreviewState = Omit<PreviewState, "step"> & { step?: number };
  ```

  Type `restoredState` and `readState()` as `RestoredPreviewState | null`, and
  keep `writeState()` on required-step `PreviewState`. In `readState`, keep
  accepting a valid `mode`/numeric `index` even if `step` is absent or
  malformed, but copy `step` only after all validation is complete:

  ```ts
  if (
    typeof parsed.step === "number" &&
    Number.isSafeInteger(parsed.step) &&
    parsed.step >= 0
  ) {
    restored.step = parsed.step;
  }
  ```

  This keeps safe-integer/non-negative validation at the storage boundary;
  an invalid value becomes indistinguishable from a legacy missing value.
- After the manifest and slide views exist, resolve the initial index first,
  then compute its `stepCount`. In single mode, pass a present restored step
  through `clampStep`; use `stepCount` when `readState` omitted the missing or
  invalid step. Grid mode always uses `stepCount`, because grid is a
  final-state surface.
- `saveState` and the draft-clearing write in `restoreDraft` must write
  `{mode: this.mode, index: this.stateIndex(), step: this.currentStep}`. Keep
  note-draft fields exactly as today and never add active/discarded slide edit
  text.
- `requestGenerationReload` continues saving before either immediate reload or
  deferral. `releaseDeferredReload` continues saving after a successful
  transition. Because both call `saveState`, they now retain the committed
  step automatically.
- Do not add a BFCache-specific state path. The existing `pagehide` listener
  calls `saveState`, and the Rust bootstrap reloads a `pageshow` with
  `event.persisted`; the normal constructor/read/mount path must restore the
  saved step. Initial `applyLayout` calls the Task 2 stage-state helper after
  `createStageHost` has rehydrated scripts and before the existing announcement
  loop, matching present's final-layout-before-announcement contract.

**Verification command:**

```sh
(cd packages/peitho-present && npx vitest run test/preview.test.ts && npm run typecheck)
cargo test -p peitho-core preview_index_bootstrap_reloads_bfcache_restores
```

## Task 5: Rebuild the preview bundle and document the final contract

**Goal:** Ship the TypeScript change in the embedded preview bundle and make
all author-facing and maintainer-facing text distinguish the step-aware stage
from final-state filmstrip/grid/lint output. Remove every statement that says
preview as a whole is final-only.

**Files:**

- `packages/peitho-present/dist/preview.js`
- `README.md`
- `site/content/guide/getting-started.md`
- `site/content/guide/cli.md`
- `site/content/guide/writing-decks.md`
- `site/content/guide/frontmatter.md`
- `site/content/examples/incremental-reveal.md`
- `site/content/examples/code-emphasis.md`
- `CLAUDE.md`

**Test (concrete code):** Use focused contract searches before and after the
documentation edit:

```sh
rg -n 'ArrowRight|ArrowLeft|PageDown|PageUp|filmstrip|fully revealed' \
  README.md site/content/guide/getting-started.md site/content/guide/cli.md
rg -n 'preview|Preview|final state|fully revealed|stepped emphasis' \
  README.md site/content/guide/writing-decks.md site/content/guide/frontmatter.md \
  site/content/examples/incremental-reveal.md site/content/examples/code-emphasis.md CLAUDE.md
! rg -n 'Preview and lint (remain|always) fully revealed|Preview remains final pending Issue #724|peitho preview.*show the final state' \
  README.md site/content/guide site/content/examples CLAUDE.md
rg -q '\{mode,index,step\}' CLAUDE.md
rg -q 'stage.*walks.*steps' CLAUDE.md
rg -q 'filmstrip.*grid.*fully revealed|grid.*filmstrip.*fully revealed' CLAUDE.md
```

**Implementation (concrete code):**

- In the README preview section and `getting-started.md`, say rebuilds retain
  the slide, reveal step, and overview mode. Spell out that
  ArrowRight/ArrowLeft/PageDown/PageUp walk the stage sequence; Up/Down walk
  non-skipped filmstrip slides and land final; thumbnail clicks, grid opens,
  Home, and End also land final. Replace the README's statement that
  PageUp/PageDown “change slides” while notes are focused with “move to the
  previous/next reveal position, crossing a slide only at its boundary.”
- In `site/content/guide/cli.md`'s preview section, document that the single
  stage is always step-aware and independent of `public_reveal`; filmstrip and
  grid remain fully revealed. In the notes and inline-edit sections, replace
  “PageUp/PageDown change slides” with “PageUp/PageDown request the previous or
  next reveal position, possibly crossing a slide”; an actual move commits an
  open inline/source edit first, but a step-only move skips only the notes
  flush. Enter with an open edit retains the old sequence: commit the edit,
  flush a dirty note, then focus notes. State that `e` still replaces the stage
  and Escape returns to the same step.
- In both incremental-reveal passages in `writing-decks.md`, say present and
  the preview stage walk steps, while preview thumbnails/grid and lint remain
  final. Keep the published/PDF `public_reveal` behavior unchanged. In the
  code-emphasis table and prose, add the preview stage to stepped-emphasis
  consumers and replace the final-only preview statement with the
  stage-versus-overview distinction.
- In `frontmatter.md`, replace “Preview and lint remain fully revealed” with
  the exact scope: lint remains final; preview ignores `public_reveal`, always
  walks steps on its stage, and keeps filmstrip/grid final.
- In `README.md`'s incremental reveal section, make the same distinction and
  retain the existing published/PDF default wording.
- In `site/content/examples/incremental-reveal.md` and
  `site/content/examples/code-emphasis.md`, make the same stage-versus-overview
  distinction: the preview stage walks reveal/emphasis steps; filmstrip/grid,
  lint, and gallery screenshots remain final.
- Update these `CLAUDE.md` bullets in place:
  - per-slide skip: preview single `next`/`prev` are step-aware; `up`/`down`
    remain next/previous non-skipped direct moves;
  - preview loop: generation reload state is `{mode,index,step}` and legacy or
    invalid step restores final;
  - layout scripts: preview step-only moves reuse the mounted stage without
    rerunning scripts or announcing it, while a generation remount applies the
    restored state before the normal stage announcement;
  - editable notes/inline editing: step-only moves skip only the notes flush
    and still settle the one open edit before DOM state; Enter with an open
    edit still commits it and flushes dirty notes before focusing notes;
  - interactive content: preview is now the third direct consumer of
    `stepState.ts`; each stage shadow root gets its own style from that shared
    module, while `interactiveTarget.ts` still owns target deferral;
  - incremental reveal: stage-only stepping, shared navigation/state,
    direct/vertical final semantics, restore clamping, BFCache/rebuild behavior,
    and final thumbnails/grid;
  - code line emphasis: `applyStepState` drives the preview stage too, while
    filmstrip thumbnails remain style-free and final; grid hosts keep their
    per-host step styles but reset reveal/emphasis attributes one-past-final
    when preview enters grid.
- Run `npm run build` and retain only the intended generated change to
  `dist/preview.js`. Do not hand-edit bundles and do not change example deck
  sources, bindings, manifest/cache output, `shell.js`, `remote.js`, or
  `viewer.js`.

**Verification command:**

```sh
(cd packages/peitho-present && npm run build && npm test && npm run typecheck)
make demo-site
git diff --check -- packages/peitho-present/dist/preview.js README.md site/content/guide/getting-started.md site/content/guide/cli.md site/content/guide/writing-decks.md site/content/guide/frontmatter.md site/content/examples/incremental-reveal.md site/content/examples/code-emphasis.md CLAUDE.md
```

## Task 6: Run every gate and have Opus verify the two real decks in Chrome

**Goal:** Pass the complete repository gate set, then verify layout, editing,
reload, BFCache, reveal visibility, and moving emphasis in a real Chrome. The
Codex sandbox cannot perform the Chrome portion; Opus must run and record that
manual/browser-driven acceptance against temporary copies of
`examples/incremental-reveal` and `examples/code-emphasis`.

**Files:** No file is intentionally edited in this acceptance task. A gate or
Chrome failure returns to the owning task and gains a regression test before
the fix.

**Test (concrete code):** Run the full `CLAUDE.md` gate list exactly, including
three consecutive workspace test runs and all four committed-bundle drift
checks:

```sh
cargo test --workspace          # run 3 times in a row (past test-race incidents)
cargo test --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
git diff --exit-code bindings/  # contract drift
cd packages/peitho-present && npm run build && npm test && npm run typecheck
git diff --exit-code packages/peitho-present/dist/shell.js  # embedded shell drift (after npm run build)
git diff --exit-code packages/peitho-present/dist/preview.js  # embedded preview drift (after npm run build)
git diff --exit-code packages/peitho-present/dist/remote.js  # embedded remote drift (after npm run build)
git diff --exit-code packages/peitho-present/dist/viewer.js  # embedded viewer drift (after npm run build)
```

**Implementation (concrete code):** Make no production change in this task.
If a command fails, add or tighten the focused test in Tasks 1-5, fix the owning
implementation/document, rebuild generated output where applicable, and
restart Task 6 from the first command. Do not weaken an existing grid,
filmstrip, editing, BFCache, or `interactiveTarget` assertion to make the gate
pass.

**Verification command:** Opus prepares disposable decks and starts the first
preview with these exact commands:

```sh
cd /Users/mizzy/src/github.com/mizzy/peitho-issue-724
cargo build -p peitho
e2e_root="$(mktemp -d /tmp/peitho-724-e2e.XXXXXX)"
cp -R examples/incremental-reveal "$e2e_root/incremental-reveal"
cp -R examples/code-emphasis "$e2e_root/code-emphasis"
./target/debug/peitho preview "$e2e_root/incremental-reveal/deck.md" --port 5724 --no-open >"$e2e_root/incremental-reveal.log" 2>&1 &
preview_pid=$!
for attempt in {1..100}; do
  curl -fsS http://127.0.0.1:5724/manifest.json >/dev/null && break
  sleep 0.1
done
curl -fsS http://127.0.0.1:5724/manifest.json >/dev/null
open -a "Google Chrome" http://127.0.0.1:5724/
```

In the incremental-reveal preview, Opus verifies all of the following:

1. The initial grid and every filmstrip thumbnail show complete slides. Opening
   a grid tile starts that slide fully revealed.
2. On slide 1, ArrowLeft/PageUp walk backward from the direct-jump final state;
   ArrowRight/PageDown walk forward one position at a time. From the final
   position, the next move enters slide 2 at step 0; from slide 2 step 0, the
   previous move returns to slide 1 fully revealed.
3. Up/Down, Home/End, a filmstrip click, and reopening from grid all land fully
   revealed. The grid's existing arrow geometry and selection behavior remain
   unchanged.
4. At step 1, Enter focuses notes; typing `Issue 724 reload step`, then Escape,
   saves and triggers a generation reload. The rebuilt page returns to the same
   slide and step, and PageDown still works afterward.
5. Pressing `e` at a partial step replaces the stage; Escape returns to that
   same partial step. Editing the first revealed list item inline and pressing
   PageUp saves before the item becomes hidden. An unrevealed later item cannot
   be clicked.
6. At a partial step, navigate the tab to
   `http://127.0.0.1:5724/manifest.json`, then use Back. The BFCache-restored
   preview reloads, preserves the step, and remains interactive rather than
   returning as a disposed page.

Stop the first server and start the emphasis deck:

```sh
kill "$preview_pid"
wait "$preview_pid" 2>/dev/null || true
./target/debug/peitho preview "$e2e_root/code-emphasis/deck.md" --port 5725 --no-open >"$e2e_root/code-emphasis.log" 2>&1 &
preview_pid=$!
for attempt in {1..100}; do
  curl -fsS http://127.0.0.1:5725/manifest.json >/dev/null && break
  sleep 0.1
done
curl -fsS http://127.0.0.1:5725/manifest.json >/dev/null
open -a "Google Chrome" http://127.0.0.1:5725/
```

In the code-emphasis preview, Opus confirms the grid tile has no pointer, then
opens “Stepping Through a Function” directly and sees the final step's line 9
pointer on the stage. ArrowLeft walks to lines 4-6, then line 2, then step 0
with no pointer; ArrowRight reverses the sequence, with only the matching group
active. The code stays visible throughout, and its filmstrip/grid copies remain
complete and un-dimmed. Finally stop the server:

```sh
kill "$preview_pid"
wait "$preview_pid" 2>/dev/null || true
```

Issue #724 is complete only when the automated gates pass and Opus records all
of the Chrome checks above as passing.
