// src/measure.ts
var MARKER_ID = "peitho-measure";
var ERROR_MARKER_ID = "peitho-measure-error";
var SLOT_CLASS = /(^|\s)slot-/;
var PARAGRAPH_SELECTOR = "p,h1,h2,h3,h4,h5,h6";
var SPECIAL_FONT_FAMILIES = /* @__PURE__ */ new Set([
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
var colorContext = null;
var DEFAULT_PARAGRAPH_CONTEXT = {
  bulletLevel: null,
  numbered: false,
  bulletContinuation: false,
  numberingStartAt: null
};
function measureDeck(document2 = globalThis.document) {
  const sections = Array.from(document2.querySelectorAll("section[data-slide-key]"));
  return {
    canvasWidth: readCanvasDimension(document2, "--peitho-canvas-width", sections[0]?.getBoundingClientRect().width ?? 0),
    canvasHeight: readCanvasDimension(document2, "--peitho-canvas-height", sections[0]?.getBoundingClientRect().height ?? 0),
    slides: sections.map((section) => measureSlide(section))
  };
}
async function appendMeasurement(document2 = globalThis.document) {
  const fonts = document2.fonts;
  if (fonts?.ready) {
    await fonts.ready;
  }
  await waitForImages(document2);
  const measured = measureDeck(document2);
  document2.getElementById(MARKER_ID)?.remove();
  const script = document2.createElement("script");
  script.type = "application/json";
  script.id = MARKER_ID;
  script.textContent = JSON.stringify(measured).replace(/</g, "\\u003c");
  document2.body.appendChild(script);
  return measured;
}
function appendMeasurementError(document2, error) {
  document2.getElementById(ERROR_MARKER_ID)?.remove();
  const script = document2.createElement("script");
  script.type = "application/json";
  script.id = ERROR_MARKER_ID;
  script.textContent = JSON.stringify({ message: errorMessage(error) }).replace(/</g, "\\u003c");
  document2.body.appendChild(script);
}
async function waitForImages(document2) {
  const images = Array.from(document2.querySelectorAll("img")).filter(isResolvedContentImage);
  await Promise.all(images.map(waitForImage));
}
async function waitForImage(image) {
  if (image.complete) {
    return;
  }
  if (typeof image.decode === "function") {
    await image.decode().catch(() => void 0);
    return;
  }
  await new Promise((resolve) => {
    const settle = () => {
      image.removeEventListener("load", settle);
      image.removeEventListener("error", settle);
      resolve();
    };
    image.addEventListener("load", settle, { once: true });
    image.addEventListener("error", settle, { once: true });
  });
}
function measureSlide(section) {
  return {
    key: section.getAttribute("data-slide-key") ?? "",
    backgroundColor: effectiveBackgroundColor(section),
    boxes: Array.from(section.querySelectorAll("[class]")).filter((element) => SLOT_CLASS.test(element.className)).filter(isVisibleElement).map((box) => measureBox(section, box)),
    images: Array.from(section.querySelectorAll("img")).filter(isResolvedContentImage).filter(isVisibleElement).map((image) => measureImage(section, image))
  };
}
function effectiveBackgroundColor(section) {
  let current = section;
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
function isTransparentColor(color) {
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
function alphaIsZero(raw) {
  const alpha = Number.parseFloat(raw.trim());
  return Number.isFinite(alpha) && alpha <= 0;
}
function measureBox(section, box) {
  return {
    slot: slotName(box),
    rect: relativeRect(section, box),
    style: measureBoxStyle(box),
    paragraphs: collectParagraphs(box)
  };
}
function measureImage(section, image) {
  return {
    src: image.getAttribute("src") ?? "",
    alt: image.getAttribute("alt") ?? "",
    rect: relativeRect(section, image)
  };
}
function measureBoxStyle(box) {
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
function collectParagraphs(box) {
  if (box.matches("pre")) {
    return collectPreParagraphs(box, DEFAULT_PARAGRAPH_CONTEXT);
  }
  const paragraphs = collectContainerParagraphs(box, DEFAULT_PARAGRAPH_CONTEXT);
  return paragraphs.length > 0 ? paragraphs : [measureParagraph(box, DEFAULT_PARAGRAPH_CONTEXT)];
}
function collectContainerParagraphs(container, context) {
  const paragraphs = [];
  let inlineNodes = [];
  const flushInline = () => {
    const runs = collectRunsFromNodes(container, inlineNodes, false);
    inlineNodes = [];
    if (runs.length > 0) {
      paragraphs.push(paragraphFromRuns(container, runs, context));
    }
  };
  for (const child of Array.from(container.childNodes)) {
    if (isInlineNode(child)) {
      inlineNodes.push(child);
      continue;
    }
    flushInline();
    paragraphs.push(...collectBlockParagraphs(child, context));
  }
  flushInline();
  return paragraphs;
}
function collectBlockParagraphs(block, context) {
  if (block.matches(PARAGRAPH_SELECTOR)) {
    return [measureParagraph(block, context)];
  }
  if (block.matches("pre")) {
    return collectPreParagraphs(block, context);
  }
  if (isListElement(block)) {
    return collectListParagraphs(block);
  }
  if (block.matches("li")) {
    const parentList = block.parentElement;
    const numbered = parentList?.tagName.toLowerCase() === "ol";
    return collectListItemParagraphs(block, numbered, numbered ? orderedListStart(parentList) : null);
  }
  return collectContainerParagraphs(block, context);
}
function collectListParagraphs(list) {
  const numbered = list.tagName.toLowerCase() === "ol";
  let itemIndex = 0;
  return Array.from(list.children).filter((child) => child.tagName.toLowerCase() === "li").flatMap((child) => {
    const startAt = numbered && itemIndex === 0 ? orderedListStart(list) : null;
    itemIndex += 1;
    return collectListItemParagraphs(child, numbered, startAt);
  });
}
function collectListItemParagraphs(item, numbered, numberingStartAt) {
  const paragraphs = [];
  const level = bulletLevel(item);
  let ownParagraphCount = 0;
  let inlineNodes = [];
  const pushOwnParagraphs = (ownParagraphs) => {
    for (const paragraph of ownParagraphs) {
      const continuation = ownParagraphCount > 0;
      paragraphs.push({
        ...paragraph,
        bulletLevel: level,
        numbered: continuation ? false : numbered,
        bulletContinuation: continuation,
        numberingStartAt: continuation ? null : numberingStartAt
      });
      ownParagraphCount += 1;
    }
  };
  const flushInline = () => {
    const runs = collectRunsFromNodes(item, inlineNodes, false);
    inlineNodes = [];
    if (runs.length > 0) {
      pushOwnParagraphs([paragraphFromRuns(item, runs, DEFAULT_PARAGRAPH_CONTEXT)]);
    }
  };
  const processBlock = (block) => {
    if (block.matches(PARAGRAPH_SELECTOR)) {
      pushOwnParagraphs([measureParagraph(block, DEFAULT_PARAGRAPH_CONTEXT)]);
      return;
    }
    if (block.matches("pre")) {
      pushOwnParagraphs(collectPreParagraphs(block, DEFAULT_PARAGRAPH_CONTEXT));
      return;
    }
    if (isListElement(block)) {
      paragraphs.push(...collectListParagraphs(block));
      return;
    }
    processContainer(block);
  };
  const processContainer = (container) => {
    let nestedInlineNodes = [];
    const flushNestedInline = () => {
      const runs = collectRunsFromNodes(container, nestedInlineNodes, false);
      nestedInlineNodes = [];
      if (runs.length > 0) {
        pushOwnParagraphs([paragraphFromRuns(container, runs, DEFAULT_PARAGRAPH_CONTEXT)]);
      }
    };
    for (const child of Array.from(container.childNodes)) {
      if (isInlineNode(child)) {
        nestedInlineNodes.push(child);
        continue;
      }
      flushNestedInline();
      processBlock(child);
    }
    flushNestedInline();
  };
  for (const child of Array.from(item.childNodes)) {
    if (isInlineNode(child)) {
      inlineNodes.push(child);
      continue;
    }
    flushInline();
    processBlock(child);
  }
  flushInline();
  if (paragraphs.length === 0) {
    pushOwnParagraphs([measureParagraph(item, DEFAULT_PARAGRAPH_CONTEXT)]);
  }
  return paragraphs;
}
function collectPreParagraphs(pre, context) {
  const align = textAlign(pre);
  const paragraphs = [{ align, ...context, runs: [] }];
  for (const run of collectRuns(pre, true)) {
    const parts = run.text.split("\n");
    parts.forEach((part, index) => {
      if (part.length > 0) {
        paragraphs[paragraphs.length - 1]?.runs.push({ ...run, text: part });
      }
      if (index < parts.length - 1) {
        paragraphs.push({ align, ...context, runs: [] });
      }
    });
  }
  return paragraphs;
}
function measureParagraph(element, context) {
  return paragraphFromRuns(element, collectRuns(element, false), context);
}
function paragraphFromRuns(element, runs, context) {
  return {
    align: textAlign(element),
    ...context,
    runs
  };
}
function collectRuns(root, preserveWhitespace) {
  return collectRunsFromNodes(root, Array.from(root.childNodes), preserveWhitespace);
}
function collectRunsFromNodes(root, nodes, preserveWhitespace) {
  const runs = [];
  const state = { runs, pendingSpace: false, pendingBreaks: 0 };
  nodes.forEach((node) => visitRunNode(node, root, preserveWhitespace, state));
  return runs;
}
function visitRunNode(node, root, preserveWhitespace, state) {
  if (!preserveWhitespace && node.nodeType === 1) {
    const element2 = node;
    if (element2.tagName.toLowerCase() === "br") {
      state.pendingSpace = false;
      if (state.runs.length > 0) {
        state.pendingBreaks += 1;
      }
      return;
    }
    if (isBlockElement(element2)) {
      return;
    }
  }
  if (node.nodeType === 1) {
    Array.from(node.childNodes).forEach((child) => visitRunNode(child, root, preserveWhitespace, state));
    return;
  }
  if (node.nodeType !== 3) {
    return;
  }
  const textNode = node;
  const text = textNode.nodeValue ?? "";
  const element = textNode.parentElement ?? root;
  if (preserveWhitespace) {
    if (text.length > 0) {
      state.runs.push(measureRun(text, element, root));
    }
    return;
  }
  const normalized = text.replace(/\s+/g, " ");
  const trimmed = normalized.trim();
  if (trimmed.length === 0) {
    if (state.pendingBreaks === 0 && state.runs.length > 0) {
      state.pendingSpace = true;
    }
    return;
  }
  const breaksBefore = state.pendingBreaks;
  state.pendingBreaks = 0;
  if (breaksBefore === 0 && (state.pendingSpace || normalized.startsWith(" ")) && state.runs.length > 0) {
    appendTrailingSpace(state.runs);
  }
  state.runs.push(measureRun(trimmed, element, root, breaksBefore));
  state.pendingSpace = normalized.endsWith(" ");
}
function isInlineNode(node) {
  return node.nodeType !== 1 || !isBlockElement(node);
}
function isBlockElement(element) {
  return element.matches(PARAGRAPH_SELECTOR) || element.matches("pre,ul,ol,blockquote,li");
}
function isListElement(element) {
  return element.tagName.toLowerCase() === "ul" || element.tagName.toLowerCase() === "ol";
}
function orderedListStart(list) {
  const parsed = Number.parseInt(list?.getAttribute("start") ?? "", 10);
  return Number.isFinite(parsed) && parsed > 0 ? Math.min(parsed, 32767) : 1;
}
function appendTrailingSpace(runs) {
  const run = runs[runs.length - 1];
  if (run && !run.text.endsWith(" ")) {
    run.text += " ";
  }
}
function measureRun(text, element, root, breaksBefore = 0) {
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
    breaksBefore
  };
}
function relativeRect(section, element) {
  const sectionRect = section.getBoundingClientRect();
  const rect = element.getBoundingClientRect();
  return {
    x: rect.left - sectionRect.left,
    y: rect.top - sectionRect.top,
    w: rect.width,
    h: rect.height
  };
}
function readCanvasDimension(document2, property, fallback) {
  const value = viewFor(document2).getComputedStyle(document2.documentElement).getPropertyValue(property);
  const parsed = parsePx(value);
  return parsed > 0 ? parsed : fallback;
}
function slotName(element) {
  return Array.from(element.classList).find((className) => className.startsWith("slot-"))?.slice("slot-".length) ?? "";
}
function bulletLevel(element) {
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
function isResolvedContentImage(image) {
  return (image.getAttribute("src") ?? "").startsWith("assets/");
}
function isVisibleElement(element) {
  const rect = element.getBoundingClientRect();
  if (rect.width === 0 && rect.height === 0) {
    return false;
  }
  const style = viewFor(element.ownerDocument).getComputedStyle(element);
  return style.display !== "none" && style.visibility !== "hidden";
}
function normalizeColor(raw, document2) {
  const context = colorNormalizationContext(document2);
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
function colorNormalizationContext(document2) {
  if (colorContext) {
    return colorContext;
  }
  const canvas = document2.createElement("canvas");
  colorContext = canvas.getContext?.("2d") ?? null;
  return colorContext;
}
function hasUnderline(element, root) {
  let current = element;
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
function textAlign(element) {
  let current = element;
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
function firstFontFamily(raw) {
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
function normalizeFontFamily(raw) {
  return raw.trim().replace(/^["']|["']$/g, "");
}
function fontWeightIsBold(raw) {
  if (raw === "bold" || raw === "bolder") {
    return true;
  }
  const value = Number.parseInt(raw, 10);
  return Number.isFinite(value) && value >= 600;
}
function parsePx(raw) {
  const value = Number.parseFloat(raw);
  return Number.isFinite(value) ? value : 0;
}
function firstPositivePx(...values) {
  for (const value of values) {
    const parsed = parsePx(value);
    if (parsed > 0) {
      return parsed;
    }
  }
  return 0;
}
function inlineStyleValue(element, property) {
  const style = element.getAttribute("style") ?? "";
  const match = style.match(new RegExp(`${property}\\s*:\\s*([^;]+)`, "i"));
  return match?.[1]?.trim() ?? "";
}
function errorMessage(error) {
  return error instanceof Error ? error.message : String(error);
}
function viewFor(document2) {
  const view = document2.defaultView;
  if (!view) {
    throw new Error("measurement requires a document with a defaultView");
  }
  return view;
}
if (typeof document !== "undefined" && document.querySelector("section[data-slide-key]")) {
  void appendMeasurement(document).catch((error) => appendMeasurementError(document, error));
}
export {
  appendMeasurement,
  appendMeasurementError,
  measureDeck
};
