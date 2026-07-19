import type { Manifest } from "../../../bindings/Manifest";
import type { Notes } from "../../../bindings/Notes";
import type { RehearsalBaseline } from "../../../bindings/RehearsalBaseline";
import type { RehearsalSnapshot } from "../../../bindings/RehearsalSnapshot";
import type { AgendaOptions } from "../src/agenda";
import { afterEach, expect, it, vi } from "vitest";

function okJson(value: unknown): Response {
  return { ok: true, status: 200, json: async () => value } as Response;
}

function okText(value: string): Response {
  return { ok: true, status: 200, text: async () => value } as Response;
}

const manifest: Manifest = {
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
      text: { title: "", body: "", code: "" }
    }
  ],
  images: []
};

const notes: Notes = { version: 1, notes: {} };

function standardFetch(overrides: Partial<Manifest> = {}): typeof fetch {
  const responseManifest = Object.assign({}, manifest, overrides) as Manifest;
  return vi.fn(async (url: string) => {
    if (url === "manifest.json") return okJson(responseManifest);
    if (url === "peitho.css") return okText("");
    if (url === "/rehearsal") return okJson({ recorded: false });
    const slide = responseManifest.slides.find((item) => item.src === url);
    if (slide) return okText(`<section><h1>${slide.key}</h1></section>`);
    return { ok: false, status: 404, text: async () => "" } as Response;
  }) as typeof fetch;
}

afterEach(() => {
  vi.doUnmock("../src/agenda");
  vi.doUnmock("../src/sectionActuals");
  vi.doUnmock("../src/rehearsalReporter");
  vi.doUnmock("../src/rehearsalBridge");
  vi.resetModules();
  vi.restoreAllMocks();
  document.body.replaceChildren();
});

it("delegates empty section handling to the agenda installer", async () => {
  const installAgenda = vi.fn<(options: AgendaOptions) => () => void>(() => () => undefined);
  vi.doMock("../src/agenda", () => ({ installAgenda }));
  const { mountPresenterView } = await import("../src/presenter");
  const root = document.createElement("main");
  const rehearsal: RehearsalBaseline = { version: 1, lastRun: null };
  const view = await mountPresenterView({
    root,
    notes,
    rehearsal,
    fetcher: standardFetch({ sections: [] }),
    window,
    now: () => 1000
  });

  expect(installAgenda).toHaveBeenCalledTimes(1);
  expect(installAgenda.mock.calls[0]?.[0].sections).toEqual([]);
  expect(installAgenda.mock.calls[0]?.[0].rehearsal).toBe(rehearsal);

  view.destroy();
});

it("shares one section actuals instance between agenda and rehearsal reporter", async () => {
  const actuals = { actualMs: vi.fn(() => [0]), destroy: vi.fn() };
  const agendaCleanup = vi.fn();
  const reporterCleanup = vi.fn();
  const bridgeCleanup = vi.fn();
  const installSectionActuals = vi.fn((_options: unknown) => actuals);
  const installAgenda = vi.fn((_options: { actuals: unknown }) => agendaCleanup);
  const installRehearsalReporter = vi.fn(
    (_options: { actuals: unknown }) => reporterCleanup
  );
  const installRehearsalBridge = vi.fn(
    (_win: Window, _bus: EventTarget, _fetcher: typeof fetch) => bridgeCleanup
  );
  vi.doMock("../src/sectionActuals", () => ({ installSectionActuals }));
  vi.doMock("../src/agenda", () => ({ installAgenda }));
  vi.doMock("../src/rehearsalReporter", () => ({ installRehearsalReporter }));
  vi.doMock("../src/rehearsalBridge", () => ({ installRehearsalBridge }));
  const { mountPresenterView } = await import("../src/presenter");
  const sections = [
    { name: "Setup", startIndex: 0, endIndex: 0, plannedDurationMs: 60_000 }
  ];
  const root = document.createElement("main");
  const view = await mountPresenterView({
    root,
    notes,
    rehearsal: { version: 1, lastRun: null },
    fetcher: standardFetch({ sections }),
    window,
    now: () => 1000
  });

  expect(installSectionActuals).toHaveBeenCalledTimes(1);
  expect(installAgenda).toHaveBeenCalledTimes(1);
  expect(installRehearsalReporter).toHaveBeenCalledTimes(1);
  expect(installRehearsalBridge).toHaveBeenCalledTimes(1);
  expect(installAgenda.mock.calls[0]![0].actuals).toBe(actuals);
  expect(installRehearsalReporter.mock.calls[0]![0].actuals).toBe(actuals);

  view.destroy();
  expect(agendaCleanup).toHaveBeenCalledTimes(1);
  expect(reporterCleanup).toHaveBeenCalledTimes(1);
  expect(bridgeCleanup).toHaveBeenCalledTimes(1);
  expect(actuals.destroy).toHaveBeenCalledTimes(1);
});

it("passes empty sections to all section-dependent installers when validation fails", async () => {
  const actuals = { actualMs: vi.fn(() => []), flush: vi.fn(), destroy: vi.fn() };
  const agendaCleanup = vi.fn();
  const reporterCleanup = vi.fn();
  const bridgeCleanup = vi.fn();
  const installSectionActuals = vi.fn((_options: { sections: unknown[] }) => actuals);
  const installAgenda = vi.fn((_options: { sections: unknown[] }) => agendaCleanup);
  const installRehearsalReporter = vi.fn(
    (_options: { sections: unknown[] }) => reporterCleanup
  );
  const installRehearsalBridge = vi.fn(
    (_win: Window, _bus: EventTarget, _fetcher: typeof fetch) => bridgeCleanup
  );
  vi.doMock("../src/sectionActuals", () => ({ installSectionActuals }));
  vi.doMock("../src/agenda", () => ({ installAgenda }));
  vi.doMock("../src/rehearsalReporter", () => ({ installRehearsalReporter }));
  vi.doMock("../src/rehearsalBridge", () => ({ installRehearsalBridge }));
  const { mountPresenterView } = await import("../src/presenter");
  const log = { error: vi.fn(), warn: vi.fn() };
  const root = document.createElement("main");
  const view = await mountPresenterView({
    root,
    notes,
    rehearsal: { version: 1, lastRun: null },
    fetcher: standardFetch({
      sections: [
        { name: "Setup", startIndex: 0, endIndex: 0, plannedDurationMs: 60_000 },
        { name: "Demo", startIndex: 2, endIndex: 2, plannedDurationMs: 60_000 }
      ]
    }),
    window,
    now: () => 1000,
    console: log
  });

  expect(installSectionActuals.mock.calls[0]![0].sections).toEqual([]);
  expect(installAgenda.mock.calls[0]![0].sections).toEqual([]);
  expect(installRehearsalReporter.mock.calls[0]![0].sections).toEqual([]);
  expect(actuals.actualMs()).toEqual([]);
  expect(log.error).toHaveBeenCalledTimes(1);

  view.destroy();
});

it("treats invalid manifest sections as empty before installing agenda and rehearsal reporting", async () => {
  const { mountPresenterView } = await import("../src/presenter");
  const root = document.createElement("main");
  const reports: unknown[] = [];
  const onReport = (event: Event): void => {
    reports.push((event as CustomEvent).detail);
  };
  const log = { error: vi.fn(), warn: vi.fn() };
  window.addEventListener("peitho:rehearsalreport", onReport);
  const view = await mountPresenterView({
    root,
    notes,
    rehearsal: { version: 1, lastRun: null },
    fetcher: standardFetch({
      sections: [
        { name: "Setup", startIndex: 0, endIndex: 0, plannedDurationMs: 60_000 },
        { name: "Demo", startIndex: 2, endIndex: 2, plannedDurationMs: 60_000 }
      ]
    }),
    window,
    now: () => 1000,
    console: log
  });

  try {
    root.querySelector<HTMLButtonElement>('[data-peitho-action="playpause"]')?.click();
    root.querySelector<HTMLButtonElement>('[data-peitho-action="playpause"]')?.click();

    expect(root.querySelector("[data-peitho-agenda]")).toBeNull();
    expect(reports).toEqual([]);
    expect(log.error).toHaveBeenCalledTimes(1);
    expect(log.error.mock.calls[0]?.[0]).toContain("expected startIndex 1");
  } finally {
    window.removeEventListener("peitho:rehearsalreport", onReport);
    view.destroy();
  }
});

it("attributes slidechange rehearsal reports to the previous section", async () => {
  const { mountPresenterView } = await import("../src/presenter");
  const root = document.createElement("main");
  const reports: RehearsalSnapshot[] = [];
  const onReport = (event: Event): void => {
    reports.push((event as CustomEvent<RehearsalSnapshot>).detail);
  };
  let now = 1_000;
  window.addEventListener("peitho:rehearsalreport", onReport);
  const view = await mountPresenterView({
    root,
    notes,
    rehearsal: { version: 1, lastRun: null },
    fetcher: standardFetch({
      slideCount: 2,
      sections: [
        { name: "Setup", startIndex: 0, endIndex: 0, plannedDurationMs: 60_000 },
        { name: "Demo", startIndex: 1, endIndex: 1, plannedDurationMs: 60_000 }
      ],
      slides: [
        {
          index: 0,
          key: "intro",
          src: "slides/000-intro.html",
          hasNotes: false,
          skip: false,
          text: { title: "", body: "", code: "" }
        },
        {
          index: 1,
          key: "demo",
          src: "slides/001-demo.html",
          hasNotes: false,
          skip: false,
          text: { title: "", body: "", code: "" }
        }
      ]
    }),
    window,
    now: () => now
  });

  try {
    root.querySelector<HTMLButtonElement>('[data-peitho-action="playpause"]')?.click();
    now = 2_250;
    window.dispatchEvent(
      new CustomEvent("peitho:navigate", { detail: { to: { index: 1 } } })
    );

    expect(reports).toEqual([
      {
        version: 1,
        elapsedMs: 1_250,
        sections: [
          { name: "Setup", plannedDurationMs: 60_000, actualMs: 1_250 },
          { name: "Demo", plannedDurationMs: 60_000, actualMs: 0 }
        ]
      }
    ]);
  } finally {
    window.removeEventListener("peitho:rehearsalreport", onReport);
    view.destroy();
  }
});
