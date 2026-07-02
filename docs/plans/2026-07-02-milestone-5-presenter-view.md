# Peitho Milestone 5 Presenter View Plan

## Purpose

Milestone 5 adds the two-window presenter view on top of the M4 presentation shell. The output remains fetch-based: `present.html` and `presenter.html` both read the same `manifest.json`, `slides/`, `peitho.css`, `notes.json`, and `shell.js`. No slide HTML is duplicated into either entrypoint.

## M5 Design Decisions

- The shell owns timer and presentation session state because `peitho:timercontrol` is a UI-to-shell request in spec section 16.
- `peitho:presentationstart` `{ total, startedAt }` is dispatched after `mountPresentShell()` finishes loading and initial display is ready.
- `peitho:presentationend` `{ endedAt, elapsedMs }` is dispatched on `pagehide` and `destroy()`. A guard prevents duplicate end events.
- A presenter window mounts its own main shell. Presenter UI sends only `peitho:navigate` and `peitho:timercontrol` requests. Read-only display state comes from the same-window `PresentShell` public API: `elapsedMs()`, `isPaused()`, and `startedAt()`.
- `ShellOptions.bus?: EventTarget` is introduced. The default remains the provided window. Shell navigate subscription and shell notifications use `bus`. `installKeyboardNavigation()` and `installSyncBridge()` accept the same bus while preserving their current defaults.
- A second shell instance for next-slide preview is mounted in the same window with a private `EventTarget` bus. This prevents preview navigation from changing the main shell.

Out of scope: notes Markdown syntax, `publish`, `--watch`, presenter-view visual polish beyond a functional two-column layout.

## File Structure Map

| Path | Responsibility | Depends on |
| --- | --- | --- |
| `packages/peitho-present/src/shell.ts` | bus option, session lifecycle events, timer state, public getters | generated `Manifest` bindings |
| `packages/peitho-present/src/keyboard.ts` | keyboard emits navigate requests to injected bus | `NavigateTarget` |
| `packages/peitho-present/src/sync.ts` | BroadcastChannel bridge listens and dispatches through injected bus | DOM events |
| `packages/peitho-present/src/presenter.ts` | presenter view UI, main shell, preview shell, notes, timer controls | shell, keyboard, sync, generated `Notes` binding |
| `packages/peitho-present/src/index.ts` | exports presenter API and new shell event types | shell, presenter |
| `packages/peitho-present/test/session.test.ts` | shell bus/session/timer lifecycle tests | vitest, jsdom |
| `packages/peitho-present/test/presenter.test.ts` | presenter view tests for preview, notes, buttons, timer display | vitest, jsdom |
| `packages/peitho-present/test/sync.test.ts` | update existing bridge tests for bus argument | sync bridge |
| `packages/peitho-present/test/loads-handles-navigates-invalid-previousIndex-keyboard-fetch.test.ts` | update keyboard tests for bus default and no regressions | shell, keyboard |
| `crates/peitho-core/src/render.rs` | `render_presenter_index()` and presenter link in `render_present_index()` | TS shell API |
| `crates/peitho-core/src/lib.rs` | export `render_presenter_index` | render module |
| `crates/peitho/src/main.rs` | emit `presenter.html` into `.peitho/present-cache/` | peitho-core render |
| `crates/peitho/tests/present.rs` | assert present cache contains presenter.html | CLI |

Dependency direction: Rust emits HTML entrypoints and copies the prebuilt `shell.js`. TypeScript owns runtime shell behavior. Presenter UI talks to shell only through DOM events and public shell getters.

## Implementation Tasks

### Task 1 - Add Shell Bus Isolation Tests

Goal: prove two shells in one window can use separate buses without navigate collisions.

Files:

- `packages/peitho-present/test/session.test.ts`

Test:

```ts
// packages/peitho-present/test/session.test.ts
import { afterEach, expect, it, vi } from "vitest";
import { mountPresentShell } from "../src/index";
import type { PresentShell } from "../src/index";

function okJson(value: unknown): Response {
  return { ok: true, status: 200, json: async () => value } as Response;
}

function okText(value: string): Response {
  return { ok: true, status: 200, text: async () => value } as Response;
}

const manifest = {
  version: 1,
  peithoVersion: "0.1.0",
  title: "Demo",
  slideCount: 2,
  slides: [
    { index: 0, key: "intro", src: "slides/000-intro.html", hasNotes: false },
    { index: 1, key: "details", src: "slides/001-details.html", hasNotes: false }
  ]
};

function standardFetch(): typeof fetch {
  return vi.fn(async (url: string) => {
    if (url === "manifest.json") return okJson(manifest);
    if (url === "peitho.css") return okText(".slot-title { color: red; }");
    if (url === "slides/000-intro.html") return okText("<section><h1>Intro</h1></section>");
    if (url === "slides/001-details.html") return okText("<section><h1>Details</h1></section>");
    return { ok: false, status: 404, text: async () => "" } as Response;
  }) as typeof fetch;
}

const shells: PresentShell[] = [];

afterEach(() => {
  while (shells.length > 0) shells.pop()?.destroy();
});

it("isolates navigate events by shell bus", async () => {
  const rootA = document.createElement("main");
  const rootB = document.createElement("main");
  const busA = new EventTarget();
  const busB = new EventTarget();
  const shellA = await mountPresentShell({ root: rootA, fetcher: standardFetch(), window, bus: busA });
  const shellB = await mountPresentShell({ root: rootB, fetcher: standardFetch(), window, bus: busB });
  shells.push(shellA, shellB);

  busA.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));

  expect(shellA.currentIndex).toBe(1);
  expect(shellB.currentIndex).toBe(0);
});
```

Expected Red before implementation:

```text
Object literal may only specify known properties, and 'bus' does not exist in type 'ShellOptions'
```

Implementation:

```ts
// packages/peitho-present/src/shell.ts
export type ShellOptions = {
  root: HTMLElement;
  fetcher?: typeof fetch;
  window?: Window;
  document?: Document;
  console?: Pick<Console, "error">;
  bus?: EventTarget;
  now?: () => number;
};

class PresentShellController implements PresentShell {
  private readonly bus: EventTarget;

  constructor(options: ShellOptions) {
    this.root = options.root;
    this.fetcher = options.fetcher ?? fetch.bind(globalThis);
    this.win = options.window ?? window;
    this.doc = options.document ?? document;
    this.log = options.console ?? console;
    this.bus = options.bus ?? this.win;
    this.bus.addEventListener("peitho:navigate", this.onNavigate);
  }

  destroy(): void {
    this.bus.removeEventListener("peitho:navigate", this.onNavigate);
  }

  private show(index: number): void {
    if (index < 0 || index >= this.slides.length) {
      this.log.error(`Unknown slide target: ${index}`);
      return;
    }
    if (index === this.currentIndex) return;
    this.slides.forEach((slide, slideIndex) => {
      slide.host.hidden = slideIndex !== index;
    });
    const previousIndex = this.currentIndex < 0 ? null : this.currentIndex;
    this.currentIndex = index;
    const slide = this.slides[index];
    this.bus.dispatchEvent(
      new CustomEvent<SlideChangeDetail>("peitho:slidechange", {
        detail: {
          key: slide.meta.key,
          index: slide.meta.index,
          total: this.slides.length,
          previousIndex
        }
      })
    );
  }
}
```

Verification:

```bash
cd packages/peitho-present
npm test -- session
npm run typecheck
```

### Task 2 - Add Presentation Lifecycle and Timer Tests

Goal: define start/end events, deterministic timer control, and public timer getters before implementation.

Files:

- `packages/peitho-present/test/session.test.ts`

Test:

```ts
// append to packages/peitho-present/test/session.test.ts
it("dispatches presentationstart after mount with total and startedAt", async () => {
  let now = 1000;
  const starts: unknown[] = [];
  const bus = new EventTarget();
  bus.addEventListener("peitho:presentationstart", (event) => starts.push((event as CustomEvent).detail));

  const shell = await mountPresentShell({
    root: document.createElement("main"),
    fetcher: standardFetch(),
    window,
    bus,
    now: () => now
  });
  shells.push(shell);

  expect(starts).toEqual([{ total: 2, startedAt: 1000 }]);
  expect(shell.startedAt()).toBe(1000);
});

it("pauses resumes and resets elapsed time from timercontrol events", async () => {
  let now = 1000;
  const bus = new EventTarget();
  const shell = await mountPresentShell({
    root: document.createElement("main"),
    fetcher: standardFetch(),
    window,
    bus,
    now: () => now
  });
  shells.push(shell);

  now = 1500;
  expect(shell.elapsedMs()).toBe(500);
  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "pause" } }));
  expect(shell.isPaused()).toBe(true);
  now = 2500;
  expect(shell.elapsedMs()).toBe(500);
  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "resume" } }));
  expect(shell.isPaused()).toBe(false);
  now = 3000;
  expect(shell.elapsedMs()).toBe(1000);
  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "reset" } }));
  expect(shell.startedAt()).toBe(3000);
  expect(shell.elapsedMs()).toBe(0);
});

it("dispatches presentationend once for pagehide and destroy", async () => {
  let now = 1000;
  const bus = new EventTarget();
  const ends: unknown[] = [];
  bus.addEventListener("peitho:presentationend", (event) => ends.push((event as CustomEvent).detail));
  const shell = await mountPresentShell({
    root: document.createElement("main"),
    fetcher: standardFetch(),
    window,
    bus,
    now: () => now
  });

  now = 1750;
  window.dispatchEvent(new Event("pagehide"));
  shell.destroy();

  expect(ends).toEqual([{ endedAt: 1750, elapsedMs: 750 }]);
});
```

Expected Red before implementation:

```text
Property 'elapsedMs' does not exist on type 'PresentShell'
```

Implementation:

```ts
// packages/peitho-present/src/shell.ts
export type PresentationStartDetail = { total: number; startedAt: number };
export type PresentationEndDetail = { endedAt: number; elapsedMs: number };
export type TimerControlDetail = { action: "pause" | "resume" | "reset" };

export type PresentShell = {
  manifest: Manifest | null;
  currentIndex: number;
  navigate(to: NavigateTarget): void;
  elapsedMs(): number;
  isPaused(): boolean;
  startedAt(): number | null;
  destroy(): void;
};

class PresentShellController implements PresentShell {
  private readonly now: () => number;
  private startedAtValue: number | null = null;
  private pausedAtValue: number | null = null;
  private pausedTotalMs = 0;
  private ended = false;
  private readonly onTimerControl = (event: Event): void => {
    const action = (event as CustomEvent<TimerControlDetail>).detail?.action;
    if (action === "pause") this.pauseTimer();
    else if (action === "resume") this.resumeTimer();
    else if (action === "reset") this.resetTimer();
    else this.log.error("Invalid peitho:timercontrol event");
  };
  private readonly onPageHide = (): void => this.endPresentation();

  constructor(options: ShellOptions) {
    this.now = options.now ?? Date.now;
    this.win.addEventListener("pagehide", this.onPageHide);
    this.bus.addEventListener("peitho:timercontrol", this.onTimerControl);
  }

  async load(): Promise<void> {
    try {
      const manifest = await this.fetchJson<Manifest>("manifest.json");
      const css = await this.fetchText("peitho.css");
      const pending: SlideView[] = [];
      for (const slide of manifest.slides) {
        const html = await this.fetchText(slide.src);
        const host = this.createSlideHost(slide, html, css);
        pending.push({ meta: slide, host });
      }
      this.manifest = manifest;
      for (const view of pending) {
        this.root.appendChild(view.host);
        this.slides.push(view);
      }
      this.show(0);
      this.startPresentation();
    } catch (error) {
      this.root.replaceChildren();
      this.root.textContent = error instanceof Error ? error.message : String(error);
    }
  }

  elapsedMs(): number {
    if (this.startedAtValue === null) return 0;
    const pausedNow = this.pausedAtValue === null ? 0 : this.now() - this.pausedAtValue;
    return Math.max(0, this.now() - this.startedAtValue - this.pausedTotalMs - pausedNow);
  }

  isPaused(): boolean {
    return this.pausedAtValue !== null;
  }

  startedAt(): number | null {
    return this.startedAtValue;
  }

  destroy(): void {
    this.endPresentation();
    this.bus.removeEventListener("peitho:navigate", this.onNavigate);
    this.bus.removeEventListener("peitho:timercontrol", this.onTimerControl);
    this.win.removeEventListener("pagehide", this.onPageHide);
  }

  private startPresentation(): void {
    this.startedAtValue = this.now();
    this.pausedAtValue = null;
    this.pausedTotalMs = 0;
    this.ended = false;
    this.bus.dispatchEvent(
      new CustomEvent<PresentationStartDetail>("peitho:presentationstart", {
        detail: { total: this.slides.length, startedAt: this.startedAtValue }
      })
    );
  }

  private endPresentation(): void {
    if (this.ended || this.startedAtValue === null) return;
    const endedAt = this.now();
    const elapsedMs = this.elapsedMs();
    this.ended = true;
    this.bus.dispatchEvent(
      new CustomEvent<PresentationEndDetail>("peitho:presentationend", {
        detail: { endedAt, elapsedMs }
      })
    );
  }

  private pauseTimer(): void {
    if (this.startedAtValue === null || this.pausedAtValue !== null) return;
    this.pausedAtValue = this.now();
  }

  private resumeTimer(): void {
    if (this.pausedAtValue === null) return;
    this.pausedTotalMs += this.now() - this.pausedAtValue;
    this.pausedAtValue = null;
  }

  private resetTimer(): void {
    this.startedAtValue = this.now();
    this.pausedAtValue = null;
    this.pausedTotalMs = 0;
    this.ended = false;
  }
}
```

Verification:

```bash
cd packages/peitho-present
npm test -- session
npm run typecheck
```

### Task 3 - Route Keyboard and Sync Through an Injected Bus

Goal: keyboard and BroadcastChannel bridge remain UI adapters and can target a non-window bus.

Files:

- `packages/peitho-present/src/keyboard.ts`
- `packages/peitho-present/src/sync.ts`
- `packages/peitho-present/test/sync.test.ts`
- `packages/peitho-present/test/loads-handles-navigates-invalid-previousIndex-keyboard-fetch.test.ts`

Test:

```ts
// packages/peitho-present/test/sync.test.ts
it("dispatches remote sync navigation to the injected bus", () => {
  const channel = mockChannel();
  const bus = new EventTarget();
  const requests: unknown[] = [];
  bus.addEventListener("peitho:navigate", (event) => requests.push((event as CustomEvent).detail));

  const cleanup = installSyncBridge(window, () => channel, bus);
  cleanups.push(cleanup);
  channel.onmessage?.({ data: { index: 1 } });

  expect(requests).toEqual([{ to: { index: 1 } }]);
});
```

```ts
// packages/peitho-present/test/loads-handles-navigates-invalid-previousIndex-keyboard-fetch.test.ts
it("keyboard emits navigate events to an injected bus", () => {
  const bus = new EventTarget();
  const requests: unknown[] = [];
  bus.addEventListener("peitho:navigate", (event) => {
    requests.push((event as CustomEvent).detail);
  });

  const teardown = installKeyboardNavigation(window, bus);
  windowListenerCleanups.push(teardown);
  window.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight" }));

  expect(requests).toEqual([{ to: "next" }]);
});
```

Expected Red before implementation:

```text
Expected 1-2 arguments, but got 3
Expected 0-1 arguments, but got 2
```

Implementation:

```ts
// packages/peitho-present/src/keyboard.ts
export function installKeyboardNavigation(
  win: Window = window,
  bus: EventTarget = win
): () => void {
  const onKeyDown = (event: KeyboardEvent): void => {
    const to = keyMap.get(event.key);
    if (!to) return;
    event.preventDefault();
    bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to } }));
  };
  win.addEventListener("keydown", onKeyDown);
  return () => win.removeEventListener("keydown", onKeyDown);
}
```

```ts
// packages/peitho-present/src/sync.ts
export function installSyncBridge(
  win: Window = window,
  channelFactory: SyncChannelFactory = defaultChannelFactory,
  bus: EventTarget = win
): () => void {
  const channel = channelFactory("peitho-sync");
  const onSlideChange = (event: Event): void => {
    const detail = (event as CustomEvent<{ index: number }>).detail;
    if (typeof detail?.index !== "number") return;
    channel.postMessage({ index: detail.index });
  };
  channel.onmessage = (event: { data: unknown }): void => {
    const data = event.data as Partial<SyncMessage>;
    if (typeof data.index !== "number") {
      console.error("Invalid peitho sync message");
      return;
    }
    bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: { index: data.index } } }));
  };
  bus.addEventListener("peitho:slidechange", onSlideChange);
  return () => {
    bus.removeEventListener("peitho:slidechange", onSlideChange);
    channel.onmessage = null;
    channel.close();
  };
}
```

Verification:

```bash
cd packages/peitho-present
npm test -- sync keyboard
npm run typecheck
```

### Task 4 - Add Presenter View Tests

Goal: specify presenter UI behavior before implementation.

Files:

- `packages/peitho-present/test/presenter.test.ts`

Test:

```ts
// packages/peitho-present/test/presenter.test.ts
import { afterEach, expect, it, vi } from "vitest";
import { mountPresenterView } from "../src/index";
import type { PresenterView, SyncChannel, SyncChannelFactory } from "../src/index";
import type { Notes } from "../../../bindings/Notes";

function okJson(value: unknown): Response {
  return { ok: true, status: 200, json: async () => value } as Response;
}

function okText(value: string): Response {
  return { ok: true, status: 200, text: async () => value } as Response;
}

const manifest = {
  version: 1,
  peithoVersion: "0.1.0",
  title: "Demo",
  slideCount: 2,
  slides: [
    { index: 0, key: "intro", src: "slides/000-intro.html", hasNotes: false },
    { index: 1, key: "details", src: "slides/001-details.html", hasNotes: false }
  ]
};

const notes: Notes = { version: 1, notes: { intro: "Opening note" } };

function standardFetch(): typeof fetch {
  return vi.fn(async (url: string) => {
    if (url === "manifest.json") return okJson(manifest);
    if (url === "peitho.css") return okText(".slot-title { color: red; }");
    if (url === "slides/000-intro.html") return okText("<section><h1>Intro</h1></section>");
    if (url === "slides/001-details.html") return okText("<section><h1>Details</h1></section>");
    return { ok: false, status: 404, text: async () => "" } as Response;
  }) as typeof fetch;
}

function mockSyncChannelFactory() {
  const channel: SyncChannel & { sent: unknown[]; closed: boolean } = {
    sent: [],
    closed: false,
    onmessage: null,
    postMessage(message: unknown) {
      this.sent.push(message);
    },
    close() {
      this.closed = true;
    }
  };
  const factory: SyncChannelFactory = () => channel;
  return { channel, factory };
}

const views: PresenterView[] = [];
const cleanups: Array<() => void> = [];

afterEach(() => {
  while (cleanups.length > 0) cleanups.pop()?.();
  while (views.length > 0) views.pop()?.destroy();
});

it("renders current slide preview next slide note and timer", async () => {
  let now = 1000;
  const root = document.createElement("main");
  const { factory } = mockSyncChannelFactory();
  const view = await mountPresenterView({
    root,
    notes,
    fetcher: standardFetch(),
    window,
    now: () => now,
    syncChannelFactory: factory
  });
  views.push(view);

  expect(root.querySelector('[data-peitho-presenter="current"] .peitho-slide')).not.toBeNull();
  expect(root.querySelector('[data-peitho-presenter="preview"]')?.textContent).toContain("Details");
  expect(root.querySelector('[data-peitho-presenter="notes"]')?.textContent).toContain("Opening note");
  now = 65000;
  view.tick();
  expect(root.querySelector('[data-peitho-presenter="timer"]')?.textContent).toBe("01:04");
});

it("updates preview and shows end of deck on the last slide", async () => {
  const root = document.createElement("main");
  const { factory } = mockSyncChannelFactory();
  const view = await mountPresenterView({
    root,
    notes,
    fetcher: standardFetch(),
    window,
    now: () => 1000,
    syncChannelFactory: factory
  });
  views.push(view);

  window.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));

  expect(root.querySelector('[data-peitho-presenter="notes"]')?.textContent).toContain("No notes for this slide.");
  expect(root.querySelector('[data-peitho-presenter="preview-end"]')?.textContent).toContain("End of deck");
});

it("buttons emit navigate and timercontrol requests only", async () => {
  const root = document.createElement("main");
  const { channel, factory } = mockSyncChannelFactory();
  const view = await mountPresenterView({
    root,
    notes,
    fetcher: standardFetch(),
    window,
    now: () => 1000,
    syncChannelFactory: factory
  });
  views.push(view);
  const events: unknown[] = [];
  const onNavigate = (event: Event): void => events.push((event as CustomEvent).detail);
  const onTimerControl = (event: Event): void => events.push((event as CustomEvent).detail);
  window.addEventListener("peitho:navigate", onNavigate);
  window.addEventListener("peitho:timercontrol", onTimerControl);
  cleanups.push(() => window.removeEventListener("peitho:navigate", onNavigate));
  cleanups.push(() => window.removeEventListener("peitho:timercontrol", onTimerControl));

  root.querySelector<HTMLButtonElement>('[data-peitho-action="next"]')?.click();
  root.querySelector<HTMLButtonElement>('[data-peitho-action="pause"]')?.click();
  root.querySelector<HTMLButtonElement>('[data-peitho-action="reset"]')?.click();

  expect(events).toEqual([{ to: "next" }, { action: "pause" }, { action: "reset" }]);
  expect(channel.sent).toEqual([{ index: 1 }]);
});
```

Expected Red before implementation:

```text
Module '"../src/index"' has no exported member 'mountPresenterView'
```

Verification:

```bash
cd packages/peitho-present
npm test -- presenter
```

### Task 5 - Implement mountPresenterView

Goal: mount the presenter UI with main shell, isolated preview shell, notes, timer, and event-only buttons.

Files:

- `packages/peitho-present/src/presenter.ts`
- `packages/peitho-present/src/index.ts`

Implementation:

```ts
// packages/peitho-present/src/presenter.ts
import type { Notes } from "../../../bindings/Notes";
import { installKeyboardNavigation } from "./keyboard";
import { mountPresentShell, type PresentShell, type SlideChangeDetail } from "./shell";
import { installSyncBridge, type SyncChannelFactory } from "./sync";

export type PresenterOptions = {
  root: HTMLElement;
  notes: Notes;
  fetcher?: typeof fetch;
  window?: Window;
  document?: Document;
  now?: () => number;
  syncChannelFactory?: SyncChannelFactory;
};

export type PresenterView = {
  mainShell: PresentShell;
  previewShell: PresentShell;
  tick(): void;
  destroy(): void;
};

function formatElapsed(ms: number): string {
  const totalSeconds = Math.floor(ms / 1000);
  const minutes = Math.floor(totalSeconds / 60).toString().padStart(2, "0");
  const seconds = (totalSeconds % 60).toString().padStart(2, "0");
  return `${minutes}:${seconds}`;
}

export async function mountPresenterView(options: PresenterOptions): Promise<PresenterView> {
  const win = options.window ?? window;
  const doc = options.document ?? document;
  const fetcher = options.fetcher ?? fetch.bind(globalThis);
  const now = options.now ?? Date.now;
  const bus = win;
  const previewBus = new EventTarget();
  options.root.innerHTML = `
    <section class="peitho-presenter">
      <div data-peitho-presenter="current"></div>
      <aside>
        <div data-peitho-presenter="preview"></div>
        <p data-peitho-presenter="preview-end" hidden>End of deck</p>
        <section data-peitho-presenter="notes"></section>
        <output data-peitho-presenter="timer">00:00</output>
        <div class="peitho-presenter-controls">
          <button type="button" data-peitho-action="prev">Prev</button>
          <button type="button" data-peitho-action="next">Next</button>
          <button type="button" data-peitho-action="pause">Pause</button>
          <button type="button" data-peitho-action="resume">Resume</button>
          <button type="button" data-peitho-action="reset">Reset</button>
        </div>
      </aside>
    </section>`;

  const currentRoot = options.root.querySelector<HTMLElement>('[data-peitho-presenter="current"]')!;
  const previewRoot = options.root.querySelector<HTMLElement>('[data-peitho-presenter="preview"]')!;
  const previewEnd = options.root.querySelector<HTMLElement>('[data-peitho-presenter="preview-end"]')!;
  const notesRoot = options.root.querySelector<HTMLElement>('[data-peitho-presenter="notes"]')!;
  const timerRoot = options.root.querySelector<HTMLElement>('[data-peitho-presenter="timer"]')!;

  const mainShell = await mountPresentShell({ root: currentRoot, fetcher, window: win, document: doc, bus, now });
  const previewShell = await mountPresentShell({
    root: previewRoot,
    fetcher,
    window: win,
    document: doc,
    bus: previewBus,
    now
  });
  const keyboardCleanup = installKeyboardNavigation(win, bus);
  const syncCleanup = installSyncBridge(win, options.syncChannelFactory, bus);

  function updateFromSlide(detail: SlideChangeDetail): void {
    notesRoot.textContent = options.notes.notes[detail.key] ?? "No notes for this slide.";
    const nextIndex = detail.index + 1;
    if (nextIndex < detail.total) {
      previewRoot.hidden = false;
      previewEnd.hidden = true;
      previewBus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: { index: nextIndex } } }));
    } else {
      previewRoot.hidden = true;
      previewEnd.hidden = false;
    }
    tick();
  }

  function tick(): void {
    timerRoot.textContent = formatElapsed(mainShell.elapsedMs());
  }

  const onSlideChange = (event: Event): void => {
    updateFromSlide((event as CustomEvent<SlideChangeDetail>).detail);
  };
  bus.addEventListener("peitho:slidechange", onSlideChange);
  const firstSlide = mainShell.manifest?.slides[mainShell.currentIndex];
  if (firstSlide) {
    updateFromSlide({
      key: firstSlide.key,
      index: firstSlide.index,
      total: mainShell.manifest?.slideCount ?? 0,
      previousIndex: null
    });
  }

  options.root.querySelector('[data-peitho-action="prev"]')?.addEventListener("click", () => {
    bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "prev" } }));
  });
  options.root.querySelector('[data-peitho-action="next"]')?.addEventListener("click", () => {
    bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));
  });
  for (const action of ["pause", "resume", "reset"] as const) {
    options.root.querySelector(`[data-peitho-action="${action}"]`)?.addEventListener("click", () => {
      bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action } }));
      tick();
    });
  }

  const interval = win.setInterval(tick, 250);

  return {
    mainShell,
    previewShell,
    tick,
    destroy(): void {
      win.clearInterval(interval);
      bus.removeEventListener("peitho:slidechange", onSlideChange);
      keyboardCleanup();
      syncCleanup();
      previewShell.destroy();
      mainShell.destroy();
    }
  };
}
```

```ts
// packages/peitho-present/src/index.ts
export { mountPresenterView } from "./presenter";
export type { PresenterOptions, PresenterView } from "./presenter";
```

Verification:

```bash
cd packages/peitho-present
npm test -- presenter
npm run typecheck
```

### Task 6 - Bundle Presenter API

Goal: `shell.js` exports `mountPresenterView` and still does not embed slide bodies.

Files:

- `packages/peitho-present/src/index.ts`
- `packages/peitho-present/esbuild.config.mjs`

Test:

```bash
cd packages/peitho-present
npm run build
rg -n "mountPresenterView|peitho:timercontrol|peitho:presentationstart|peitho:presentationend" dist/shell.js
! rg -n "Peitho Architecture|data-slide-key=\\\"arch-1\\\"" dist/shell.js
```

Implementation:

```ts
// packages/peitho-present/src/index.ts
export { installKeyboardNavigation } from "./keyboard";
export { mountPresenterView } from "./presenter";
export { mountPresentShell } from "./shell";
export { installSyncBridge } from "./sync";
export type { PresenterOptions, PresenterView } from "./presenter";
export type {
  NavigateDetail,
  NavigateTarget,
  PresentShell,
  PresentationEndDetail,
  PresentationStartDetail,
  ShellOptions,
  SlideChangeDetail,
  TimerControlDetail
} from "./shell";
export type { SyncChannel, SyncChannelFactory, SyncMessage } from "./sync";
```

Verification:

```bash
cd packages/peitho-present
npm run build
rg -n "mountPresenterView|peitho:timercontrol|peitho:presentationstart|peitho:presentationend" dist/shell.js
```

### Task 7 - Render presenter.html

Goal: generate `presenter.html` from peitho-core with minimal two-column UI CSS and visible fetch errors.

Files:

- `crates/peitho-core/src/render.rs`
- `crates/peitho-core/src/lib.rs`

Test:

```rust
// crates/peitho-core/src/render.rs
#[test]
fn presenter_index_mounts_presenter_view_and_notes() {
    let html = render_presenter_index();

    assert!(html.contains(r#"<main id="peitho-presenter-root"></main>"#));
    assert!(html.contains(
        r#"import { mountPresenterView } from './shell.js';"#
    ));
    assert!(html.contains("fetchOk('notes.json')"));
    assert!(html.contains("await mountPresenterView({ root, notes })"));
    assert!(html.contains("grid-template-columns"));
    assert!(html.contains("Failed to load"));
    assert!(!html.contains("fetchOk(slide.src)"));
}
```

Expected Red before implementation:

```text
cannot find function `render_presenter_index` in this scope
```

Implementation:

```rust
// crates/peitho-core/src/render.rs
pub fn render_presenter_index() -> String {
    r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Peitho Presenter</title>
  <style>
    body { margin: 0; font: 14px system-ui, sans-serif; background: #111; color: #f5f5f5; }
    #peitho-presenter-root { min-height: 100vh; }
    .peitho-presenter { display: grid; grid-template-columns: minmax(0, 2fr) minmax(320px, 1fr); gap: 16px; padding: 16px; box-sizing: border-box; min-height: 100vh; }
    [data-peitho-presenter="current"], [data-peitho-presenter="preview"] { background: #000; min-height: 220px; }
    [data-peitho-presenter="notes"] { white-space: pre-wrap; line-height: 1.5; }
    [data-peitho-presenter="timer"] { display: block; font-size: 40px; font-variant-numeric: tabular-nums; margin: 16px 0; }
    .peitho-presenter-controls { display: flex; flex-wrap: wrap; gap: 8px; }
  </style>
</head>
<body>
  <main id="peitho-presenter-root"></main>
  <script type="module">
    import { mountPresenterView } from './shell.js';

    function showError(message) {
      const root = document.getElementById('peitho-presenter-root');
      root.textContent = message;
    }

    async function fetchOk(url) {
      const response = await fetch(url);
      if (!response.ok) throw new Error(`Failed to load ${url}: ${response.status}`);
      return response;
    }

    async function main() {
      const root = document.getElementById('peitho-presenter-root');
      try {
        const notes = await fetchOk('notes.json').then((response) => response.json());
        await mountPresenterView({ root, notes });
      } catch (error) {
        showError(error.message);
      }
    }

    main();
  </script>
</body>
</html>"#
        .to_owned()
}
```

```rust
// crates/peitho-core/src/lib.rs
pub use render::{render_deck, render_distribution_index, render_present_index, render_presenter_index};
```

Verification:

```bash
cargo test -p peitho-core render::tests::presenter_index_mounts_presenter_view_and_notes
```

### Task 8 - Add Presenter Link to present.html

Goal: audience `present.html` opens `presenter.html` in a separate tab without embedding presenter UI.

Files:

- `crates/peitho-core/src/render.rs`

Test:

```rust
// crates/peitho-core/src/render.rs
#[test]
fn present_index_links_to_presenter_view() {
    let html = render_present_index();

    assert!(html.contains(r#"<a id="peitho-presenter-link" href="presenter.html" target="_blank" rel="noopener">Presenter view</a>"#));
    assert!(html.contains(r#"<main id="peitho-present-root"></main>"#));
    assert!(!html.contains("mountPresenterView"));
}
```

Expected Red before implementation:

```text
assertion failed: html.contains(...)
```

Implementation:

```rust
// crates/peitho-core/src/render.rs, inside render_present_index body
<body>
  <a id="peitho-presenter-link" href="presenter.html" target="_blank" rel="noopener">Presenter view</a>
  <main id="peitho-present-root"></main>
  <script type="module">
```

Verification:

```bash
cargo test -p peitho-core render::tests::present_index_links_to_presenter_view
```

### Task 9 - Emit presenter.html in Present Cache

Goal: `peitho present --no-serve` writes `presenter.html` next to `present.html`.

Files:

- `crates/peitho/src/main.rs`
- `crates/peitho/tests/present.rs`

Test:

```rust
// crates/peitho/tests/present.rs
#[test]
fn present_no_serve_writes_presenter_html() {
    let dir = tempdir().unwrap();
    let fixture = Fixture::write(dir.path());
    let shell = dir.path().join("shell.js");
    fs::write(
        &shell,
        "export function mountPresentShell() {}\nexport function installKeyboardNavigation() {}\nexport function installSyncBridge() {}\nexport function mountPresenterView() {}\n",
    )
    .unwrap();

    Command::cargo_bin("peitho")
        .unwrap()
        .current_dir(dir.path())
        .args(fixture.present_args(&shell))
        .args(["--no-serve", "--no-open"])
        .assert()
        .success();

    let cache = dir.path().join(".peitho/present-cache");
    let presenter = fs::read_to_string(cache.join("presenter.html")).unwrap();
    assert!(presenter.contains("mountPresenterView"));
    assert!(fs::read_to_string(cache.join("present.html")).unwrap().contains("Presenter view"));
}
```

Expected Red before implementation:

```text
No such file or directory
```

Implementation:

```rust
// crates/peitho/src/main.rs, inside emit_present_cache
fs::write(
    cache.join("presenter.html"),
    peitho_core::render_presenter_index(),
)
.into_diagnostic()?;
```

Verification:

```bash
cargo test -p peitho --test present present_no_serve_writes_presenter_html
```

### Task 10 - Update Repository Present Smoke

Goal: the real repository present cache contains `presenter.html`, and the copied shell bundle exports presenter code.

Files:

- `crates/peitho/tests/present.rs`

Test:

```rust
// crates/peitho/tests/present.rs
#[test]
fn repository_example_present_cache_contains_presenter_view() {
    let shell = workspace_root().join("packages/peitho-present/dist/shell.js");
    assert!(shell.exists(), "shell bundle not built; run npm run build");

    Command::cargo_bin("peitho")
        .unwrap()
        .current_dir(workspace_root())
        .args([
            "present",
            "examples/deck.md",
            "--template",
            "templates/title-body-code.html",
            "--base-css",
            "themes/base.css",
            "--overrides-css",
            "themes/overrides.css",
            "--no-serve",
            "--no-open",
        ])
        .assert()
        .success();

    let cache = workspace_root().join(".peitho/present-cache");
    assert!(cache.join("presenter.html").exists());
    assert!(fs::read_to_string(cache.join("presenter.html")).unwrap().contains("mountPresenterView"));
    assert!(fs::read_to_string(cache.join("shell.js")).unwrap().contains("mountPresenterView"));
}
```

Expected Red before implementation:

```text
shell bundle not built; run npm run build
```

Implementation:

```bash
cd packages/peitho-present
npm run build
cd ../..
```

Verification:

```bash
test -f packages/peitho-present/dist/shell.js || (echo "shell bundle not built; run npm run build" && false)
cargo test -p peitho --test present repository_example_present_cache_contains_presenter_view
```

### Task 11 - Add Presentation Event Exports and Typecheck Gate

Goal: generated bundle consumers can import the new shell and presenter types without handwritten contracts.

Files:

- `packages/peitho-present/src/index.ts`
- `packages/peitho-present/test/generated.test.ts`

Test:

```ts
// packages/peitho-present/test/generated.test.ts
import { expect, it } from "vitest";
import type {
  PresentationEndDetail,
  PresentationStartDetail,
  PresenterOptions,
  TimerControlDetail
} from "../src/index";

it("exports presenter and presentation event types", () => {
  const start: PresentationStartDetail = { total: 3, startedAt: 1000 };
  const end: PresentationEndDetail = { endedAt: 2000, elapsedMs: 1000 };
  const control: TimerControlDetail = { action: "pause" };
  const options: Pick<PresenterOptions, "root" | "notes"> = {
    root: document.createElement("main"),
    notes: { version: 1, notes: {} }
  };

  expect(start.total).toBe(3);
  expect(end.elapsedMs).toBe(1000);
  expect(control.action).toBe("pause");
  expect(options.notes.version).toBe(1);
});
```

Expected Red before implementation:

```text
Module '"../src/index"' has no exported member 'PresenterOptions'
```

Implementation:

```ts
// packages/peitho-present/src/index.ts
export type { PresenterOptions, PresenterView } from "./presenter";
export type {
  PresentationEndDetail,
  PresentationStartDetail,
  TimerControlDetail
} from "./shell";
```

Verification:

```bash
cd packages/peitho-present
npm run typecheck
npm test -- generated
```

## Final Verification

Run the full acceptance gate after Task 11 is green:

```bash
cd packages/peitho-present
npm ci
npm run build
npm test
npm run typecheck
cd ../..
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo run -p peitho -- present examples/deck.md \
  --template templates/title-body-code.html \
  --base-css themes/base.css \
  --overrides-css themes/overrides.css \
  --no-serve \
  --no-open
test -f .peitho/present-cache/presenter.html
rg -n "Presenter view|presenter.html" .peitho/present-cache/present.html
rg -n "mountPresenterView|notes.json" .peitho/present-cache/presenter.html
rg -n "mountPresenterView|peitho:timercontrol|peitho:presentationstart|peitho:presentationend" .peitho/present-cache/shell.js
```

## Summary

This plan has 11 TDD tasks. It first adds bus isolation to the shell, then gives the shell session and timer ownership through `presentationstart`, `presentationend`, `timercontrol`, and public timer getters. Keyboard and BroadcastChannel are then routed through the injected bus. The presenter module mounts a main shell plus an isolated preview shell, renders notes and timer state, and keeps all controls as event emitters. Rust then emits `presenter.html`, links it from `present.html`, and writes it into the volatile present cache. Final verification runs the full TS and Rust gates plus a cache content check.
