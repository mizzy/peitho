import type { MeasuredBox } from "../../../bindings/MeasuredBox";
import type { MeasuredBoxStyle } from "../../../bindings/MeasuredBoxStyle";
import type { MeasuredDeck } from "../../../bindings/MeasuredDeck";
import type { MeasuredImage } from "../../../bindings/MeasuredImage";
import type { MeasuredParagraph } from "../../../bindings/MeasuredParagraph";
import type { MeasuredRect } from "../../../bindings/MeasuredRect";
import type { MeasuredRun } from "../../../bindings/MeasuredRun";
import type { MeasuredSlide } from "../../../bindings/MeasuredSlide";

const MARKER_ID = "peitho-measure";
const SLOT_CLASS = /(^|\s)slot-/;
const PARAGRAPH_SELECTOR = "p,h1,h2,h3,h4,h5,h6,li,pre";
const LIST_ITEM_BLOCK_SELECTOR = "p,h1,h2,h3,h4,h5,h6,pre,blockquote";

export function measureDeck(document: Document = globalThis.document): MeasuredDeck {
  const sections = Array.from(document.querySelectorAll<HTMLElement>("section[data-slide-key]"));
  return {
    canvasWidth: readCanvasDimension(document, "--peitho-canvas-width", sections[0]?.getBoundingClientRect().width ?? 0),
    canvasHeight: readCanvasDimension(document, "--peitho-canvas-height", sections[0]?.getBoundingClientRect().height ?? 0),
    slides: sections.map((section) => measureSlide(section))
  };
}

export async function appendMeasurement(document: Document = globalThis.document): Promise<MeasuredDeck> {
  const fonts = (document as Document & { fonts?: { ready?: Promise<unknown> } }).fonts;
  if (fonts?.ready) {
    await fonts.ready;
  }
  await waitForImages(document);
  const measured = measureDeck(document);
  document.getElementById(MARKER_ID)?.remove();
  const script = document.createElement("script");
  script.type = "application/json";
  script.id = MARKER_ID;
  script.textContent = JSON.stringify(measured).replace(/</g, "\\u003c");
  document.body.appendChild(script);
  return measured;
}

async function waitForImages(document: Document): Promise<void> {
  const images = Array.from(document.querySelectorAll<HTMLImageElement>("img"));
  await Promise.all(images.map(waitForImage));
}

async function waitForImage(image: HTMLImageElement): Promise<void> {
  if (image.complete) {
    return;
  }
  if (typeof image.decode === "function") {
    await image.decode().catch(() => undefined);
    return;
  }
  await new Promise<void>((resolve) => {
    const settle = () => {
      image.removeEventListener("load", settle);
      image.removeEventListener("error", settle);
      resolve();
    };
    image.addEventListener("load", settle, { once: true });
    image.addEventListener("error", settle, { once: true });
  });
}

function measureSlide(section: HTMLElement): MeasuredSlide {
  const style = viewFor(section.ownerDocument).getComputedStyle(section);
  return {
    key: section.getAttribute("data-slide-key") ?? "",
    backgroundColor: style.backgroundColor,
    boxes: Array.from(section.querySelectorAll<HTMLElement>("[class]"))
      .filter((element) => SLOT_CLASS.test(element.className))
      .map((box) => measureBox(section, box)),
    images: Array.from(section.querySelectorAll<HTMLImageElement>("img")).map((image) => measureImage(section, image))
  };
}

function measureBox(section: HTMLElement, box: HTMLElement): MeasuredBox {
  return {
    slot: slotName(box),
    rect: relativeRect(section, box),
    style: measureBoxStyle(box),
    paragraphs: collectParagraphs(box)
  };
}

function measureImage(section: HTMLElement, image: HTMLImageElement): MeasuredImage {
  return {
    src: image.getAttribute("src") ?? "",
    alt: image.getAttribute("alt") ?? "",
    rect: relativeRect(section, image)
  };
}

function measureBoxStyle(box: HTMLElement): MeasuredBoxStyle {
  const style = viewFor(box.ownerDocument).getComputedStyle(box);
  return {
    backgroundColor: style.backgroundColor,
    borderColor: style.borderTopColor,
    borderWidth: parsePx(style.borderTopWidth),
    borderRadius: firstPositivePx(
      style.borderTopLeftRadius,
      style.borderRadius,
      box.style.borderTopLeftRadius,
      box.style.borderRadius,
      inlineStyleValue(box, "border-radius")
    )
  };
}

function collectParagraphs(box: HTMLElement): MeasuredParagraph[] {
  if (box.matches("pre")) {
    return collectPreParagraphs(box);
  }
  const candidates = box.matches(PARAGRAPH_SELECTOR)
    ? [box]
    : Array.from(box.querySelectorAll<HTMLElement>(PARAGRAPH_SELECTOR));
  const paragraphElements = candidates.filter((candidate) => {
    if (candidate.matches("li")) {
      return true;
    }
    return !hasParagraphAncestor(candidate, box);
  });

  if (paragraphElements.length === 0) {
    return [measureParagraph(box, null)];
  }

  return paragraphElements.flatMap((element) => {
    if (element.matches("li")) {
      return collectListItemParagraphs(element);
    }
    if (element.matches("pre")) {
      return collectPreParagraphs(element);
    }
    return [measureParagraph(element, bulletLevel(element))];
  });
}

function collectListItemParagraphs(item: HTMLElement): MeasuredParagraph[] {
  const level = bulletLevel(item);
  const directBlocks = Array.from(item.children)
    .filter((child) => child.matches(LIST_ITEM_BLOCK_SELECTOR))
    .map((child) => child as HTMLElement);
  if (directBlocks.length === 0) {
    return [measureParagraph(item, level)];
  }
  return directBlocks.flatMap((block) => {
    if (block.matches("pre")) {
      return collectPreParagraphs(block, level);
    }
    return [measureParagraph(block, level)];
  });
}

function collectPreParagraphs(pre: HTMLElement, level: number | null = null): MeasuredParagraph[] {
  const align = textAlign(pre);
  const paragraphs: MeasuredParagraph[] = [{ align, bulletLevel: level, runs: [] }];
  for (const run of collectRuns(pre, true)) {
    const parts = run.text.split("\n");
    parts.forEach((part, index) => {
      if (part.length > 0) {
        paragraphs[paragraphs.length - 1]?.runs.push({ ...run, text: part });
      }
      if (index < parts.length - 1) {
        paragraphs.push({ align, bulletLevel: level, runs: [] });
      }
    });
  }
  return paragraphs;
}

function measureParagraph(element: HTMLElement, level: number | null): MeasuredParagraph {
  return {
    align: textAlign(element),
    bulletLevel: level,
    runs: collectRuns(element, false)
  };
}

function collectRuns(root: HTMLElement, preserveWhitespace: boolean): MeasuredRun[] {
  const walker = root.ownerDocument.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  const runs: MeasuredRun[] = [];
  while (walker.nextNode()) {
    const node = walker.currentNode as Text;
    const text = node.nodeValue ?? "";
    if (preserveWhitespace ? text.length === 0 : text.trim().length === 0) {
      continue;
    }
    if (hasNestedParagraphAncestor(node, root)) {
      continue;
    }
    const element = node.parentElement ?? root;
    runs.push(measureRun(text, element, root));
  }
  return runs;
}

function measureRun(text: string, element: HTMLElement, root: HTMLElement): MeasuredRun {
  const style = viewFor(element.ownerDocument).getComputedStyle(element);
  const fontFamily = firstFontFamily(style.fontFamily);
  return {
    text,
    color: style.color,
    fontFamily,
    fontSizePx: parsePx(style.fontSize),
    bold: fontWeightIsBold(style.fontWeight),
    italic: style.fontStyle === "italic" || style.fontStyle === "oblique",
    underline: hasUnderline(element, root),
    monospace: fontFamily.toLowerCase().includes("mono") || element.closest("pre,code") !== null
  };
}

function relativeRect(section: HTMLElement, element: Element): MeasuredRect {
  const sectionRect = section.getBoundingClientRect();
  const rect = element.getBoundingClientRect();
  return {
    x: rect.left - sectionRect.left,
    y: rect.top - sectionRect.top,
    w: rect.width,
    h: rect.height
  };
}

function readCanvasDimension(document: Document, property: string, fallback: number): number {
  const value = viewFor(document).getComputedStyle(document.documentElement).getPropertyValue(property);
  const parsed = parsePx(value);
  return parsed > 0 ? parsed : fallback;
}

function slotName(element: HTMLElement): string {
  return Array.from(element.classList)
    .find((className) => className.startsWith("slot-"))
    ?.slice("slot-".length) ?? "";
}

function bulletLevel(element: HTMLElement): number | null {
  if (!element.matches("li")) {
    return null;
  }
  let level = 0;
  let parent = element.parentElement?.closest("li");
  while (parent) {
    level += 1;
    parent = parent.parentElement?.closest("li") ?? null;
  }
  return level;
}

function hasParagraphAncestor(element: HTMLElement, root: HTMLElement): boolean {
  let parent = element.parentElement;
  while (parent && parent !== root) {
    if (parent.matches(PARAGRAPH_SELECTOR)) {
      return true;
    }
    parent = parent.parentElement;
  }
  return false;
}

function hasNestedParagraphAncestor(node: Text, root: HTMLElement): boolean {
  let parent = node.parentElement;
  while (parent && parent !== root) {
    if (parent.matches(PARAGRAPH_SELECTOR)) {
      return true;
    }
    parent = parent.parentElement;
  }
  return false;
}

function hasUnderline(element: HTMLElement, root: HTMLElement): boolean {
  let current: HTMLElement | null = element;
  while (current) {
    const style = viewFor(current.ownerDocument).getComputedStyle(current);
    const authoredDecoration = `${inlineStyleValue(current, "text-decoration")} ${inlineStyleValue(current, "text-decoration-line")}`;
    if (`${style.textDecoration} ${style.textDecorationLine} ${authoredDecoration}`.includes("underline")) {
      return true;
    }
    if (current === current.ownerDocument.body) {
      return false;
    }
    current = current.parentElement;
  }
  return false;
}

function textAlign(element: HTMLElement): string {
  let current: HTMLElement | null = element;
  while (current) {
    const style = viewFor(current.ownerDocument).getComputedStyle(current);
    if (current.style.textAlign || inlineStyleValue(current, "text-align")) {
      return style.textAlign || current.style.textAlign || "left";
    }
    if (current === current.ownerDocument.body) {
      break;
    }
    current = current.parentElement;
  }
  return viewFor(element.ownerDocument).getComputedStyle(element).textAlign || "left";
}

function firstFontFamily(raw: string): string {
  return (raw.split(",")[0] ?? "").trim().replace(/^["']|["']$/g, "");
}

function fontWeightIsBold(raw: string): boolean {
  if (raw === "bold" || raw === "bolder") {
    return true;
  }
  const value = Number.parseInt(raw, 10);
  return Number.isFinite(value) && value >= 600;
}

function parsePx(raw: string): number {
  const value = Number.parseFloat(raw);
  return Number.isFinite(value) ? value : 0;
}

function firstPositivePx(...values: string[]): number {
  for (const value of values) {
    const parsed = parsePx(value);
    if (parsed > 0) {
      return parsed;
    }
  }
  return 0;
}

function inlineStyleValue(element: HTMLElement, property: string): string {
  const style = element.getAttribute("style") ?? "";
  const match = style.match(new RegExp(`${property}\\s*:\\s*([^;]+)`, "i"));
  return match?.[1]?.trim() ?? "";
}

function viewFor(document: Document): Window {
  const view = document.defaultView;
  if (!view) {
    throw new Error("measurement requires a document with a defaultView");
  }
  return view;
}

if (typeof document !== "undefined" && document.querySelector("section[data-slide-key]")) {
  void appendMeasurement(document);
}
