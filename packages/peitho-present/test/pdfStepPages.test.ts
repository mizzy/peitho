import { expect, it, vi } from "vitest";
import { expandRevealStepPages } from "../src/pdfStepPages";

it("expands stepped wraps in order and preserves logical page numbers", () => {
  document.body.innerHTML = `
    <div id="stepped" class="peitho-slide-wrap">
      <section class="peitho-slide"
        data-slide-key="stepped"
        data-reveal-steps="2"
        data-peitho-page-number="1"
        data-peitho-page-total="2">
        <p data-reveal-step="1">first</p>
        <p data-reveal-step="2">second</p>
        <span data-emphasis-step="1">one</span>
        <span data-emphasis-step="2">two</span>
      </section>
    </div>
    <div id="plain" class="peitho-slide-wrap">
      <section class="peitho-slide"
        data-slide-key="plain"
        data-peitho-page-number="2"
        data-peitho-page-total="2">
        <p>plain</p>
      </section>
    </div>`;
  const originalStepped = document.querySelector<HTMLElement>("#stepped")!;
  const originalPlain = document.querySelector<HTMLElement>("#plain")!;

  expandRevealStepPages(document, [
    { key: "stepped", revealSteps: 2 },
    { key: "plain", revealSteps: 0 }
  ]);

  const wraps = Array.from(
    document.querySelectorAll<HTMLElement>(".peitho-slide-wrap")
  );
  expect(wraps).toHaveLength(4);
  expect(wraps[2]).toBe(originalStepped);
  expect(wraps[3]).toBe(originalPlain);
  expect(
    wraps.map((wrap) =>
      wrap.querySelector<HTMLElement>("section.peitho-slide")!.dataset
        .peithoPageNumber
    )
  ).toEqual(["1", "1", "1", "2"]);
  expect(
    wraps.map((wrap) =>
      wrap.querySelector<HTMLElement>("section.peitho-slide")!.dataset
        .peithoPageTotal
    )
  ).toEqual(["2", "2", "2", "2"]);

  const steppedPages = wraps.slice(0, 3);
  expect(
    steppedPages.map((page) =>
      Array.from(page.querySelectorAll<HTMLElement>("[data-reveal-step]")).map(
        (node) => node.hasAttribute("data-reveal-hidden")
      )
    )
  ).toEqual([
    [true, true],
    [false, true],
    [false, false]
  ]);
  expect(
    steppedPages.map((page) =>
      Array.from(page.querySelectorAll<HTMLElement>("[data-emphasis-step]")).map(
        (node) => node.hasAttribute("data-emphasis-active")
      )
    )
  ).toEqual([
    [false, false],
    [true, false],
    [false, true]
  ]);
});

it("copies each source canvas bitmap into every cloned step page", () => {
  document.body.innerHTML = `
    <div class="peitho-slide-wrap">
      <section class="peitho-slide" data-slide-key="canvas" data-reveal-steps="2">
        <canvas width="320" height="180"></canvas>
      </section>
    </div>`;
  const sourceCanvas = document.querySelector<HTMLCanvasElement>("canvas")!;
  const drawImage = vi.fn();
  const contextOwners: HTMLCanvasElement[] = [];
  const getContext = vi.fn(function (this: HTMLCanvasElement) {
    contextOwners.push(this);
    return { drawImage };
  });
  const originalGetContext = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext =
    getContext as unknown as typeof HTMLCanvasElement.prototype.getContext;

  try {
    expandRevealStepPages(document, [{ key: "canvas", revealSteps: 2 }]);
  } finally {
    HTMLCanvasElement.prototype.getContext = originalGetContext;
  }

  const cloneCanvases = Array.from(
    document.querySelectorAll<HTMLCanvasElement>("canvas")
  ).filter((canvas) => canvas !== sourceCanvas);
  expect(getContext).toHaveBeenCalledTimes(2);
  expect(contextOwners).toEqual(cloneCanvases);
  expect(contextOwners).not.toContain(sourceCanvas);
  expect(drawImage).toHaveBeenCalledTimes(2);
  expect(drawImage).toHaveBeenNthCalledWith(1, sourceCanvas, 0, 0);
  expect(drawImage).toHaveBeenNthCalledWith(2, sourceCanvas, 0, 0);
});

it("skips source canvases with a zero width or height", () => {
  document.body.innerHTML = `
    <div class="peitho-slide-wrap">
      <section class="peitho-slide" data-slide-key="canvas" data-reveal-steps="1">
        <canvas width="0" height="180"></canvas>
        <canvas width="320" height="0"></canvas>
      </section>
    </div>`;
  const getContext = vi.fn();
  const originalGetContext = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext =
    getContext as unknown as typeof HTMLCanvasElement.prototype.getContext;

  try {
    expect(() =>
      expandRevealStepPages(document, [{ key: "canvas", revealSteps: 1 }])
    ).not.toThrow();
  } finally {
    HTMLCanvasElement.prototype.getContext = originalGetContext;
  }

  expect(getContext).not.toHaveBeenCalled();
  expect(document.querySelectorAll(".peitho-slide-wrap")).toHaveLength(2);
});

it("adds the slide key and canvas index when drawImage throws", () => {
  document.body.innerHTML = `
    <div class="peitho-slide-wrap">
      <section class="peitho-slide" data-slide-key="canvas" data-reveal-steps="1">
        <canvas width="320" height="180"></canvas>
      </section>
    </div>`;
  const drawImage = vi.fn(() => {
    throw new Error("bitmap unavailable");
  });
  const originalGetContext = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = vi.fn(
    () => ({ drawImage })
  ) as unknown as typeof HTMLCanvasElement.prototype.getContext;

  try {
    expect(() =>
      expandRevealStepPages(document, [{ key: "canvas", revealSteps: 1 }])
    ).toThrow(
      'Unable to copy canvas 1 for slide "canvas": drawImage failed: bitmap unavailable'
    );
  } finally {
    HTMLCanvasElement.prototype.getContext = originalGetContext;
  }

  expect(document.querySelectorAll(".peitho-slide-wrap")).toHaveLength(1);
});

it("throws without inserting a partial page when a clone canvas has no 2d context", () => {
  document.body.innerHTML = `
    <div class="peitho-slide-wrap">
      <section class="peitho-slide" data-slide-key="canvas" data-reveal-steps="1">
        <canvas width="320" height="180"></canvas>
      </section>
    </div>`;
  const originalGetContext = HTMLCanvasElement.prototype.getContext;
  HTMLCanvasElement.prototype.getContext = vi.fn(
    () => null
  ) as unknown as typeof HTMLCanvasElement.prototype.getContext;

  try {
    expect(() =>
      expandRevealStepPages(document, [{ key: "canvas", revealSteps: 1 }])
    ).toThrow(
      'Unable to copy canvas 1 for slide "canvas": 2d context unavailable'
    );
  } finally {
    HTMLCanvasElement.prototype.getContext = originalGetContext;
  }

  expect(document.querySelectorAll(".peitho-slide-wrap")).toHaveLength(1);
});

it("ignores forged DOM step counts when Rust metadata says zero steps", () => {
  document.body.innerHTML = `
    <div id="plain" class="peitho-slide-wrap">
      <section class="peitho-slide" data-slide-key="plain" data-reveal-steps="3"></section>
    </div>`;
  const plain = document.querySelector<HTMLElement>("#plain")!;

  expandRevealStepPages(document, [{ key: "plain", revealSteps: 0 }]);

  expect(document.querySelectorAll(".peitho-slide-wrap")).toHaveLength(1);
  expect(document.querySelector("#plain")).toBe(plain);
});

it.each([-1, 2.8, Number.MAX_SAFE_INTEGER + 1])(
  "rejects invalid Rust revealSteps value %j with the slide key",
  (revealSteps) => {
    document.body.innerHTML = `
      <div class="peitho-slide-wrap">
        <section class="peitho-slide" data-slide-key="bad"></section>
      </div>`;

    expect(() =>
      expandRevealStepPages(document, [{ key: "bad", revealSteps }])
    ).toThrow(
      `Slide "bad" has invalid revealSteps ${revealSteps}; expected a non-negative safe integer`
    );
  }
);

it.each([
  { slides: [], metadataCount: 0 },
  {
    slides: [
      { key: "only", revealSteps: 0 },
      { key: "extra", revealSteps: 0 }
    ],
    metadataCount: 2
  }
])(
  "rejects one PDF wrap when Rust supplied $metadataCount slide records",
  ({ slides, metadataCount }) => {
    document.body.innerHTML = `
      <div class="peitho-slide-wrap">
        <section class="peitho-slide" data-slide-key="only"></section>
      </div>`;

    expect(() => expandRevealStepPages(document, slides)).toThrow(
      `PDF slide wrap count 1 does not match Rust slide metadata count ${metadataCount}`
    );
  }
);

it("rejects a DOM slide key that differs from Rust metadata and names both", () => {
  document.body.innerHTML = `
    <div class="peitho-slide-wrap">
      <section class="peitho-slide" data-slide-key="authored"></section>
    </div>`;

  expect(() =>
    expandRevealStepPages(document, [{ key: "expected", revealSteps: 0 }])
  ).toThrow(
    'PDF slide wrap 1 has data-slide-key "authored"; Rust metadata expects "expected"'
  );
});

it("throws with the wrap position when a PDF wrap has no slide section", () => {
  document.body.innerHTML = `
    <div class="peitho-slide-wrap">
      <section class="peitho-slide" data-slide-key="valid"></section>
    </div>
    <div class="peitho-slide-wrap">
      <p>missing section</p>
    </div>`;

  expect(() =>
    expandRevealStepPages(document, [
      { key: "valid", revealSteps: 0 },
      { key: "missing", revealSteps: 0 }
    ])
  ).toThrow(
    "PDF slide wrap 2 does not contain section.peitho-slide"
  );
});
