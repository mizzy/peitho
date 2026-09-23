// jsdom never executes scripts because vitest does not opt into runScripts: "dangerously";
// these tests are structural only, and real execution is covered by the real-Chrome
// checklist in docs/plans/2026-09-23-layout-scripts.md.

import { afterEach, beforeEach, expect, it, vi } from "vitest";
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
  slideCount: 1,
  plannedDurationMs: null,
  aspectRatio: "16:9",
  canvasWidth: 1280,
  canvasHeight: 720,
  sections: [],
  slides: [
    {
      index: 0,
      key: "intro",
      src: "slides/000-intro.html",
      hasNotes: false,
      skip: false,
      revealSteps: 0,
      text: { title: "", body: "", code: "" }
    }
  ]
};

const deckCss = ".slot-title { color: rebeccapurple; }";
const mountedShells: PresentShell[] = [];

beforeEach(() => {
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockImplementation(
    (() => null) as HTMLCanvasElement["getContext"]
  );
});

afterEach(() => {
  while (mountedShells.length > 0) {
    mountedShells.pop()?.destroy();
  }
  vi.restoreAllMocks();
});

function fetcherFor(html: string): typeof fetch {
  return vi.fn(async (url: string) => {
    if (url === "manifest.json") return okJson(manifest);
    if (url === "peitho.css") return okText(deckCss);
    if (url === "slides/000-intro.html") return okText(html);
    throw new Error(`unexpected ${url}`);
  }) as unknown as typeof fetch;
}

async function mountForTest(root: HTMLElement, html: string): Promise<void> {
  const shell = await mountPresentShell({
    root,
    fetcher: fetcherFor(html),
    window,
    document
  });
  mountedShells.push(shell);
}

it("layout_script_is_re_created_so_it_can_execute", async () => {
  const root = document.createElement("main");
  await mountForTest(root, "<section><script>let n = 0</script></section>");

  const host = root.querySelector<HTMLElement>('[data-slide-key="intro"]');
  const shadow = host?.shadowRoot;
  expect(shadow).not.toBeNull();
  expect(shadow?.querySelectorAll("script")).toHaveLength(1);
  expect(shadow?.querySelector("script")?.textContent).toBe(
    "(function () {\nlet n = 0\n})();"
  );
});

it("deck_without_a_script_builds_an_unchanged_shadow_root", async () => {
  const html = "<section><h1>Intro</h1><!-- note --><p>Body <em>x</em></p></section>";
  const root = document.createElement("main");
  await mountForTest(root, html);

  const host = root.querySelector<HTMLElement>('[data-slide-key="intro"]');
  const shadow = host?.shadowRoot;
  expect(shadow).not.toBeNull();

  const styles = Array.from(shadow?.querySelectorAll("style") ?? []);
  expect(styles).toHaveLength(2);
  expect(styles[0]?.textContent).toBe(deckCss);
  const expected = document.createElement("div");
  for (const [index, mountedStyle] of styles.entries()) {
    const style = document.createElement("style");
    style.textContent = index === 0 ? deckCss : mountedStyle.textContent;
    expected.appendChild(style);
  }
  const template = document.createElement("template");
  template.innerHTML = html;
  expected.appendChild(template.content.cloneNode(true));

  expect(shadow?.innerHTML).toBe(expected.innerHTML);
});
