import type { MeasuredBox } from "../../../bindings/MeasuredBox";
import type { MeasuredBoxStyle } from "../../../bindings/MeasuredBoxStyle";
import type { MeasuredDeck } from "../../../bindings/MeasuredDeck";
import type { MeasuredImage } from "../../../bindings/MeasuredImage";
import type { MeasuredParagraph } from "../../../bindings/MeasuredParagraph";
import type { MeasuredRect } from "../../../bindings/MeasuredRect";
import type { MeasuredRun } from "../../../bindings/MeasuredRun";
import type { MeasuredSlide } from "../../../bindings/MeasuredSlide";

const MARKER_ID = "peitho-measure";
const ERROR_MARKER_ID = "peitho-measure-error";
const SLOT_CLASS = /(^|\s)slot-/;
const PARAGRAPH_SELECTOR = "p,h1,h2,h3,h4,h5,h6,li,pre";
const LIST_ITEM_BLOCK_SELECTOR = "p,h1,h2,h3,h4,h5,h6,pre,blockquote";
const SPECIAL_FONT_FAMILIES = new Set([
  "monospace",
  "ui-monospace",
  "sans-serif",
  "serif",
  "system-ui",
  "-apple-system",
  "blinkmacsystemfont",
  "ui-sans-serif",
  "ui-serif",
  "ui-rounded",
  "cursive",
  "fantasy",
  "emoji",
  "math",
  "fangsong"
]);
let colorContext: CanvasRenderingContext2D | null = null;

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

export function appendMeasurementError(document: Document, error: unknown): void {
  document.getElementById(ERROR_MARKER_ID)?.remove();
  const script = document.createElement("script");
  script.type = "application/json";
  script.id = ERROR_MARKER_ID;
  script.textContent = JSON.stringify({ message: errorMessage(error) }).replace(/</g, "\\u003c");
  document.body.appendChild(script);
}

async function waitForImages(document: Document): Promise<void> {
  const images = Array.from(document.querySelectorAll<HTMLImageElement>("img")).filter(isResolvedContentImage);
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
  return {
    key: section.getAttribute("data-slide-key") ?? "",
    backgroundColor: effectiveBackgroundColor(section),
    boxes: Array.from(section.querySelectorAll<HTMLElement>("[class]"))
      .filter((element) => SLOT_CLASS.test(element.className))
      .filter(isVisibleElement)
      .map((box) => measureBox(section, box)),
    images: Array.from(section.querySelectorAll<HTMLImageElement>("img"))
      .filter(isResolvedContentImage)
      .filter(isVisibleElement)
      .map((image) => measureImage(section, image))
  };
}

function effectiveBackgroundColor(section: HTMLElement): string {
  let current: HTMLElement | null = section;
  while (current) {
    const color = viewFor(current.ownerDocument).getComputedStyle(current).backgroundColor;
    if (!isTransparentColor(color)) {
      return normalizeColor(color, section.ownerDocument);
    }
    if (current === current.ownerDocument.documentElement) {
      break;
    }
    current = current.parentElement;
  }
  return "rgb(255, 255, 255)";
}

function isTransparentColor(color: string): boolean {
  const normalized = color.trim().toLowerCase();
  if (normalized === "" || normalized === "transparent") {
    return true;
  }
  const functional = normalized.match(/^rgba?\((.*)\)$/);
  if (!functional) {
    return false;
  }
  const args = functional[1] ?? "";
  if (args.includes("/")) {
    return alphaIsZero(args.split("/").pop() ?? "");
  }
  const parts = args.split(",").map((part) => part.trim());
  return parts.length >= 4 && alphaIsZero(parts[3] ?? "");
}

function alphaIsZero(raw: string): boolean {
  const alpha = Number.parseFloat(raw.trim());
  return Number.isFinite(alpha) && alpha <= 0;
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
    backgroundColor: normalizeColor(style.backgroundColor, box.ownerDocument),
    borderColor: normalizeColor(style.borderTopColor, box.ownerDocument),
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
  const numbered = listItemIsNumbered(item);
  const paragraphs: MeasuredParagraph[] = [];
  const directRuns = collectRuns(item, false);
  if (directRuns.length > 0) {
    paragraphs.push({
      align: textAlign(item),
      bulletLevel: level,
      numbered,
      bulletContinuation: false,
      runs: directRuns
    });
  }
  const directBlocks = Array.from(item.children)
    .filter((child) => child.matches(LIST_ITEM_BLOCK_SELECTOR))
    .map((child) => child as HTMLElement);
  paragraphs.push(...directBlocks.flatMap((block) => collectListItemBlockParagraphs(block, level, numbered)));
  if (paragraphs.length === 0) {
    return [measureParagraph(item, level, numbered)];
  }
  return paragraphs.map((paragraph, index) => {
    if (index === 0) {
      return paragraph;
    }
    return {
      ...paragraph,
      numbered: false,
      bulletContinuation: true
    };
  });
}

function collectListItemBlockParagraphs(block: HTMLElement, level: number | null, numbered: boolean): MeasuredParagraph[] {
  if (block.matches("pre")) {
    return collectPreParagraphs(block, level, numbered);
  }
  const candidates = block.matches(PARAGRAPH_SELECTOR)
    ? [block]
    : Array.from(block.querySelectorAll<HTMLElement>(PARAGRAPH_SELECTOR));
  const paragraphElements = candidates.filter((candidate) => {
    if (candidate.matches("li")) {
      return true;
    }
    return !hasParagraphAncestor(candidate, block);
  });

  if (paragraphElements.length === 0) {
    return [measureParagraph(block, level, numbered)];
  }

  return paragraphElements.flatMap((element) => {
    if (element.matches("li")) {
      return collectListItemParagraphs(element);
    }
    if (element.matches("pre")) {
      return collectPreParagraphs(element, level, numbered);
    }
    return [measureParagraph(element, level, numbered)];
  });
}

function collectPreParagraphs(pre: HTMLElement, level: number | null = null, numbered = false): MeasuredParagraph[] {
  const align = textAlign(pre);
  const paragraphs: MeasuredParagraph[] = [{ align, bulletLevel: level, numbered, bulletContinuation: false, runs: [] }];
  for (const run of collectRuns(pre, true)) {
    const parts = run.text.split("\n");
    parts.forEach((part, index) => {
      if (part.length > 0) {
        paragraphs[paragraphs.length - 1]?.runs.push({ ...run, text: part });
      }
      if (index < parts.length - 1) {
        paragraphs.push({ align, bulletLevel: level, numbered, bulletContinuation: false, runs: [] });
      }
    });
  }
  return paragraphs;
}

function measureParagraph(element: HTMLElement, level: number | null, numbered = false): MeasuredParagraph {
  return {
    align: textAlign(element),
    bulletLevel: level,
    numbered,
    bulletContinuation: false,
    runs: collectRuns(element, false)
  };
}

function collectRuns(root: HTMLElement, preserveWhitespace: boolean): MeasuredRun[] {
  const walker = root.ownerDocument.createTreeWalker(root, NodeFilter.SHOW_TEXT | NodeFilter.SHOW_ELEMENT);
  const runs: MeasuredRun[] = [];
  let pendingSpace = false;
  let pendingBreak = false;
  while (walker.nextNode()) {
    const node = walker.currentNode;
    if (!preserveWhitespace && node.nodeType === 1) {
      const element = node as Element;
      if (element.tagName.toLowerCase() === "br" && !hasNestedParagraphAncestor(element, root)) {
        pendingSpace = false;
        pendingBreak = runs.length > 0;
      }
      continue;
    }
    if (node.nodeType !== 3) {
      continue;
    }
    const textNode = node as Text;
    const text = textNode.nodeValue ?? "";
    if (hasNestedParagraphAncestor(textNode, root)) {
      continue;
    }
    const element = textNode.parentElement ?? root;
    if (preserveWhitespace) {
      if (text.length > 0) {
        runs.push(measureRun(text, element, root));
      }
      continue;
    }

    const normalized = text.replace(/\s+/g, " ");
    const trimmed = normalized.trim();
    if (trimmed.length === 0) {
      if (!pendingBreak && runs.length > 0) {
        pendingSpace = true;
      }
      continue;
    }
    const breakBefore = pendingBreak;
    pendingBreak = false;
    if (!breakBefore && (pendingSpace || normalized.startsWith(" ")) && runs.length > 0) {
      appendTrailingSpace(runs);
    }
    runs.push(measureRun(trimmed, element, root, breakBefore));
    pendingSpace = normalized.endsWith(" ");
  }
  return runs;
}

function appendTrailingSpace(runs: MeasuredRun[]): void {
  const run = runs[runs.length - 1];
  if (run && !run.text.endsWith(" ")) {
    run.text += " ";
  }
}

function measureRun(text: string, element: HTMLElement, root: HTMLElement, breakBefore = false): MeasuredRun {
  const style = viewFor(element.ownerDocument).getComputedStyle(element);
  const fontFamily = firstFontFamily(style.fontFamily);
  return {
    text,
    color: normalizeColor(style.color, element.ownerDocument),
    fontFamily,
    fontSizePx: parsePx(style.fontSize),
    bold: fontWeightIsBold(style.fontWeight),
    italic: style.fontStyle === "italic" || style.fontStyle === "oblique",
    underline: hasUnderline(element, root),
    breakBefore
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

function listItemIsNumbered(element: HTMLElement): boolean {
  if (!element.matches("li")) {
    return false;
  }
  return element.parentElement?.closest("ol,ul")?.tagName.toLowerCase() === "ol";
}

function isResolvedContentImage(image: HTMLImageElement): boolean {
  return (image.getAttribute("src") ?? "").startsWith("assets/");
}

function isVisibleElement(element: Element): boolean {
  const rect = element.getBoundingClientRect();
  if (rect.width === 0 && rect.height === 0) {
    return false;
  }
  const style = viewFor(element.ownerDocument).getComputedStyle(element);
  return style.display !== "none" && style.visibility !== "hidden";
}

function normalizeColor(raw: string, document: Document): string {
  const context = colorNormalizationContext(document);
  if (!context) {
    return raw;
  }
  const sentinel = "#000001";
  context.fillStyle = sentinel;
  context.fillStyle = raw;
  if (context.fillStyle === sentinel && raw.trim().toLowerCase() !== sentinel) {
    return raw;
  }
  return context.fillStyle;
}

function colorNormalizationContext(document: Document): CanvasRenderingContext2D | null {
  if (colorContext) {
    return colorContext;
  }
  const canvas = document.createElement("canvas");
  colorContext = canvas.getContext?.("2d") ?? null;
  return colorContext;
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

function hasNestedParagraphAncestor(node: Node, root: HTMLElement): boolean {
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
  const families = raw.split(",").map(normalizeFontFamily).filter((family) => family.length > 0);
  const concrete = families.find((family) => !SPECIAL_FONT_FAMILIES.has(family.toLowerCase()));
  if (concrete) {
    return concrete;
  }
  const fallbackFamilies = families.map((family) => family.toLowerCase());
  if (fallbackFamilies.some((family) => family.includes("monospace"))) {
    return "Courier New";
  }
  if (fallbackFamilies.some((family) => family === "serif" || family === "ui-serif")) {
    return "Times New Roman";
  }
  return "Arial";
}

function normalizeFontFamily(raw: string): string {
  return raw.trim().replace(/^["']|["']$/g, "");
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

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function viewFor(document: Document): Window {
  const view = document.defaultView;
  if (!view) {
    throw new Error("measurement requires a document with a defaultView");
  }
  return view;
}

if (typeof document !== "undefined" && document.querySelector("section[data-slide-key]")) {
  void appendMeasurement(document).catch((error: unknown) => appendMeasurementError(document, error));
}
