import type { Manifest } from "../../../bindings/Manifest";
import type { Notes } from "../../../bindings/Notes";
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
  sections: [],
  slides: [{ index: 0, key: "intro", src: "slides/000-intro.html", hasNotes: false }]
};

const notes: Notes = { version: 1, notes: {} };

function standardFetch(overrides: Partial<Manifest> = {}): typeof fetch {
  return vi.fn(async (url: string) => {
    if (url === "manifest.json") return okJson(Object.assign({}, manifest, overrides));
    if (url === "peitho.css") return okText("");
    if (url === "slides/000-intro.html") return okText("<section><h1>Intro</h1></section>");
    return { ok: false, status: 404, text: async () => "" } as Response;
  }) as typeof fetch;
}

afterEach(() => {
  vi.doUnmock("../src/agenda");
  vi.resetModules();
  vi.restoreAllMocks();
  document.body.replaceChildren();
});

it("delegates empty section handling to the agenda installer", async () => {
  const installAgenda = vi.fn<(options: AgendaOptions) => () => void>(() => () => undefined);
  vi.doMock("../src/agenda", () => ({ installAgenda }));
  const { mountPresenterView } = await import("../src/presenter");
  const root = document.createElement("main");
  const view = await mountPresenterView({
    root,
    notes,
    fetcher: standardFetch({ sections: [] }),
    window,
    now: () => 1000
  });

  expect(installAgenda).toHaveBeenCalledTimes(1);
  expect(installAgenda.mock.calls[0]?.[0].sections).toEqual([]);

  view.destroy();
});
