import { JSDOM } from "jsdom";
import { describe, expect, it, vi } from "vitest";
import { appendMeasurement, measureDeck } from "../src/measure";

describe("measurement DOM walker", () => {
  it("measures slots, styled text-node runs, and images relative to each section", () => {
    const { document } = createDocument(`
      <section data-slide-key="intro" style="background-color: rgb(255, 255, 255);">
        <div class="slot-body" style="background-color: rgba(0, 0, 0, 0); border: 2px solid rgb(1, 2, 3); border-radius: 8px; color: rgb(10, 20, 30); font-family: Inter, sans-serif; font-size: 20px; font-weight: 700; text-align: center; text-decoration-line: underline;">
          <p>Hello <em style="color: rgb(30, 40, 50); font-family: Inter, sans-serif; font-size: 20px; font-style: italic;">world</em></p>
        </div>
        <img src="assets/0123456789abcdef-arch.png" alt="Architecture">
      </section>
    `);
    const section = document.querySelector("section")!;
    const box = document.querySelector(".slot-body")!;
    const image = document.querySelector("img")!;
    setRect(section, { x: 100, y: 200, w: 1280, h: 720 });
    setRect(box, { x: 196, y: 280, w: 420, h: 96 });
    setRect(image, { x: 700, y: 320, w: 320, h: 180 });

    const measured = measureDeck(document);

    expect(measured.canvasWidth).toBe(1280);
    expect(measured.canvasHeight).toBe(720);
    expect(measured.slides).toHaveLength(1);
    expect(measured.slides[0]?.backgroundColor).toBe("rgb(255, 255, 255)");
    expect(measured.slides[0]?.boxes[0]).toMatchObject({
      slot: "body",
      rect: { x: 96, y: 80, w: 420, h: 96 },
      style: {
        backgroundColor: "rgba(0, 0, 0, 0)",
        borderColor: "rgb(1, 2, 3)",
        borderWidth: 2,
        borderRadius: 8
      }
    });
    expect(measured.slides[0]?.boxes[0]?.paragraphs[0]).toMatchObject({
      align: "center",
      bulletLevel: null
    });
    expect(measured.slides[0]?.boxes[0]?.paragraphs[0]?.runs).toEqual([
      {
        text: "Hello ",
        color: "rgb(10, 20, 30)",
        fontFamily: "Inter",
        fontSizePx: 20,
        bold: true,
        italic: false,
        underline: true,
        monospace: false
      },
      {
        text: "world",
        color: "rgb(30, 40, 50)",
        fontFamily: "Inter",
        fontSizePx: 20,
        bold: true,
        italic: true,
        underline: true,
        monospace: false
      }
    ]);
    expect(measured.slides[0]?.images[0]).toEqual({
      src: "assets/0123456789abcdef-arch.png",
      alt: "Architecture",
      rect: { x: 600, y: 120, w: 320, h: 180 }
    });
  });

  it("measures the effective slide background from body when the section is transparent", () => {
    const { document } = createDocument(`
      <section data-slide-key="background" style="background-color: rgba(0, 0, 0, 0);">
        <div class="slot-body"><p>Body</p></div>
      </section>
    `);
    document.body.style.backgroundColor = "rgb(12, 34, 56)";

    const measured = measureDeck(document);

    expect(measured.slides[0]?.backgroundColor).toBe("rgb(12, 34, 56)");
  });

  it("normalizes measured colors through canvas when available", () => {
    const { document } = createDocument(`
      <section data-slide-key="colors" style="background-color: rgb(1, 2, 3);">
        <div class="slot-body" style="background-color: rgb(4, 5, 6); border: 2px solid rgb(7, 8, 9); color: rgb(10, 11, 12);">
          <p>Color</p>
        </div>
      </section>
    `);
    const canonical = new Map([
      ["rgb(1, 2, 3)", "#010203"],
      ["rgb(4, 5, 6)", "#040506"],
      ["rgb(7, 8, 9)", "#070809"],
      ["rgb(10, 11, 12)", "#0a0b0c"]
    ]);
    const context = {
      value: "",
      set fillStyle(raw: string) {
        this.value = canonical.get(raw) ?? raw;
      },
      get fillStyle() {
        return this.value;
      }
    };
    const createElement = document.createElement.bind(document);
    vi.spyOn(document, "createElement").mockImplementation(((tagName: string, options?: ElementCreationOptions) => {
      if (tagName.toLowerCase() === "canvas") {
        return { getContext: (kind: string) => kind === "2d" ? context : null } as unknown as HTMLCanvasElement;
      }
      return createElement(tagName, options);
    }) as Document["createElement"]);

    const measured = measureDeck(document);

    expect(measured.slides[0]?.backgroundColor).toBe("#010203");
    expect(measured.slides[0]?.boxes[0]?.style.backgroundColor).toBe("#040506");
    expect(measured.slides[0]?.boxes[0]?.style.borderColor).toBe("#070809");
    expect(measured.slides[0]?.boxes[0]?.paragraphs[0]?.runs[0]?.color).toBe("#0a0b0c");
  });

  it("falls back to a white slide background when all ancestors are transparent", () => {
    const { document } = createDocument(`
      <section data-slide-key="background" style="background-color: rgba(0, 0, 0, 0);">
        <div class="slot-body"><p>Body</p></div>
      </section>
    `);
    document.body.style.backgroundColor = "rgba(0, 0, 0, 0)";
    document.documentElement.style.backgroundColor = "rgba(0, 0, 0, 0)";

    const measured = measureDeck(document);

    expect(measured.slides[0]?.backgroundColor).toBe("rgb(255, 255, 255)");
  });

  it("splits preformatted code into paragraphs while preserving hl span colors", () => {
    const { document } = createDocument(`
      <section data-slide-key="code">
        <pre class="slot-code" style="font-family: 'JetBrains Mono', monospace; font-size: 16px; text-align: left;"><code><span class="hl-keyword" style="color: rgb(200, 0, 0); font-family: 'JetBrains Mono', monospace; font-size: 16px;">fn main()</span>
<span class="hl-string" style="color: rgb(0, 0, 200); font-family: 'JetBrains Mono', monospace; font-size: 16px;">println()</span></code></pre>
      </section>
    `);
    const section = document.querySelector("section")!;
    const pre = document.querySelector("pre")!;
    setRect(section, { x: 0, y: 0, w: 1280, h: 720 });
    setRect(pre, { x: 80, y: 120, w: 640, h: 240 });

    const measured = measureDeck(document);

    expect(measured.slides[0]?.boxes[0]?.paragraphs).toEqual([
      {
        align: "left",
        bulletLevel: null,
        numbered: false,
        runs: [
          {
            text: "fn main()",
            color: "rgb(200, 0, 0)",
            fontFamily: "JetBrains Mono",
            fontSizePx: 16,
            bold: false,
            italic: false,
            underline: false,
            monospace: true
          }
        ]
      },
      {
        align: "left",
        bulletLevel: null,
        numbered: false,
        runs: [
          {
            text: "println()",
            color: "rgb(0, 0, 200)",
            fontFamily: "JetBrains Mono",
            fontSizePx: 16,
            bold: false,
            italic: false,
            underline: false,
            monospace: true
          }
        ]
      }
    ]);
  });

  it("measures only resolved content image assets", () => {
    const { document } = createDocument(`
      <section data-slide-key="image-filter">
        <img src="assets/content.png" alt="Content">
        <img src="https://example.com/logo.png" alt="Remote">
        <img src="decor/logo.png" alt="Decor">
      </section>
    `);
    const section = document.querySelector("section")!;
    const images = Array.from(document.querySelectorAll("img"));
    setRect(section, { x: 10, y: 20, w: 1280, h: 720 });
    images.forEach((image, index) => setRect(image, { x: 30 + index * 100, y: 50, w: 80, h: 40 }));

    const measured = measureDeck(document);

    expect(measured.slides[0]?.images).toEqual([
      {
        src: "assets/content.png",
        alt: "Content",
        rect: { x: 20, y: 30, w: 80, h: 40 }
      }
    ]);
  });

  it("resolves CSS font-family stacks to concrete office-safe typefaces", () => {
    const { document } = createDocument(`
      <section data-slide-key="fonts">
        <div class="slot-body">
          <p style="font-family: ui-monospace, SFMono-Regular, Menlo, monospace;">Code stack</p>
          <p style="font-family: -apple-system, BlinkMacSystemFont, sans-serif;">System stack</p>
          <p style="font-family: monospace;">Generic mono</p>
          <p style="font-family: Inter, sans-serif;">Concrete stack</p>
        </div>
      </section>
    `);

    const measured = measureDeck(document);

    expect(measured.slides[0]?.boxes[0]?.paragraphs.map((paragraph) => paragraph.runs[0]?.fontFamily)).toEqual([
      "SFMono-Regular",
      "Arial",
      "Courier New",
      "Inter"
    ]);
  });

  it("records list item nesting as bullet levels without folding nested text upward", () => {
    const { document } = createDocument(`
      <section data-slide-key="list">
        <div class="slot-body" style="font-family: Inter; font-size: 18px;">
          <ul>
            <li>Top<ul><li>Nested</li></ul></li>
          </ul>
        </div>
      </section>
    `);
    setRect(document.querySelector("section")!, { x: 0, y: 0, w: 1280, h: 720 });
    setRect(document.querySelector(".slot-body")!, { x: 64, y: 64, w: 400, h: 200 });

    const measured = measureDeck(document);

    expect(measured.slides[0]?.boxes[0]?.paragraphs.map((paragraph) => ({
      bulletLevel: paragraph.bulletLevel,
      numbered: paragraph.numbered,
      text: paragraph.runs.map((run) => run.text).join("")
    }))).toEqual([
      { bulletLevel: 0, numbered: false, text: "Top" },
      { bulletLevel: 1, numbered: false, text: "Nested" }
    ]);
  });

  it("marks ordered list item paragraphs as numbered", () => {
    const { document } = createDocument(`
      <section data-slide-key="list">
        <div class="slot-body" style="font-family: Inter; font-size: 18px;">
          <ol><li>First</li></ol>
          <ul><li>Bullet</li></ul>
        </div>
      </section>
    `);

    const measured = measureDeck(document);

    expect(measured.slides[0]?.boxes[0]?.paragraphs.map((paragraph) => ({
      bulletLevel: paragraph.bulletLevel,
      numbered: paragraph.numbered,
      text: paragraph.runs.map((run) => run.text).join("")
    }))).toEqual([
      { bulletLevel: 0, numbered: true, text: "First" },
      { bulletLevel: 0, numbered: false, text: "Bullet" }
    ]);
  });

  it("records loose list item text from nested paragraphs", () => {
    const { document } = createDocument(`
      <section data-slide-key="list">
        <div class="slot-body" style="font-family: Inter; font-size: 18px;">
          <ul>
            <li><p>Loose item</p></li>
          </ul>
        </div>
      </section>
    `);
    setRect(document.querySelector("section")!, { x: 0, y: 0, w: 1280, h: 720 });
    setRect(document.querySelector(".slot-body")!, { x: 64, y: 64, w: 400, h: 200 });

    const measured = measureDeck(document);

    expect(measured.slides[0]?.boxes[0]?.paragraphs.map((paragraph) => ({
      bulletLevel: paragraph.bulletLevel,
      text: paragraph.runs.map((run) => run.text).join("")
    }))).toEqual([{ bulletLevel: 0, text: "Loose item" }]);
  });

  it("records multiple direct block children in one list item as separate bullet paragraphs", () => {
    const { document } = createDocument(`
      <section data-slide-key="list">
        <div class="slot-body" style="font-family: Inter; font-size: 18px;">
          <ul>
            <li><p>First paragraph</p><p>Second paragraph</p></li>
          </ul>
        </div>
      </section>
    `);
    setRect(document.querySelector("section")!, { x: 0, y: 0, w: 1280, h: 720 });
    setRect(document.querySelector(".slot-body")!, { x: 64, y: 64, w: 400, h: 200 });

    const measured = measureDeck(document);

    expect(measured.slides[0]?.boxes[0]?.paragraphs.map((paragraph) => ({
      bulletLevel: paragraph.bulletLevel,
      text: paragraph.runs.map((run) => run.text).join("")
    }))).toEqual([
      { bulletLevel: 0, text: "First paragraph" },
      { bulletLevel: 0, text: "Second paragraph" }
    ]);
  });

  it("appends escaped measurement JSON after fonts are ready", async () => {
    const { document } = createDocument(`
      <section data-slide-key="json">
        <div class="slot-title" style="font-family: Inter; font-size: 24px;"><p>&lt;tag&gt;</p></div>
      </section>
    `);
    Object.defineProperty(document, "fonts", {
      configurable: true,
      value: { ready: Promise.resolve() }
    });
    setRect(document.querySelector("section")!, { x: 0, y: 0, w: 1280, h: 720 });
    setRect(document.querySelector(".slot-title")!, { x: 0, y: 0, w: 320, h: 80 });

    await appendMeasurement(document);

    const marker = document.querySelector<HTMLScriptElement>("#peitho-measure");
    expect(marker?.type).toBe("application/json");
    expect(marker?.textContent).toContain("\\u003c");
    expect(JSON.parse(marker?.textContent ?? "{}").slides[0].key).toBe("json");
  });

  it("waits for pending images before appending measurement JSON", async () => {
    const { document } = createDocument(`
      <section data-slide-key="image">
        <img src="assets/photo.png" alt="Photo">
      </section>
    `);
    const section = document.querySelector("section")!;
    const image = document.querySelector("img")!;
    setRect(section, { x: 100, y: 200, w: 1280, h: 720 });
    setRect(image, { x: 100, y: 200, w: 0, h: 0 });

    let resolveDecode = () => {};
    const decode = new Promise<void>((resolve) => {
      resolveDecode = () => {
        setRect(image, { x: 112, y: 224, w: 320, h: 180 });
        resolve();
      };
    });
    Object.defineProperty(image, "decode", {
      configurable: true,
      value: () => decode
    });

    const pending = appendMeasurement(document);
    await Promise.resolve();

    expect(document.querySelector("#peitho-measure")).toBeNull();
    resolveDecode();
    const measured = await pending;

    expect(measured.slides[0]?.images[0]?.rect).toEqual({ x: 12, y: 24, w: 320, h: 180 });
    expect(JSON.parse(document.querySelector("#peitho-measure")?.textContent ?? "{}").slides[0].key).toBe("image");
  });
});

type Rect = { x: number; y: number; w: number; h: number };

function createDocument(body: string): { document: Document } {
  const dom = new JSDOM(
    `<!doctype html><html style="--peitho-canvas-width: 1280px; --peitho-canvas-height: 720px;"><body>${body}</body></html>`,
    { pretendToBeVisual: true, url: "http://localhost/" }
  );
  return { document: dom.window.document };
}

function setRect(element: Element, rect: Rect): void {
  Object.defineProperty(element, "getBoundingClientRect", {
    configurable: true,
    value: () => ({
      x: rect.x,
      y: rect.y,
      left: rect.x,
      top: rect.y,
      right: rect.x + rect.w,
      bottom: rect.y + rect.h,
      width: rect.w,
      height: rect.h,
      toJSON: () => rect
    })
  });
}
