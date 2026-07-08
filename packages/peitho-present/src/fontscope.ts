const FONT_SCOPE_ATTRIBUTE = "data-peitho-font-scope";
const FONT_SCOPE_SELECTOR = `style[${FONT_SCOPE_ATTRIBUTE}]`;

type FontScopeState = {
  style: HTMLStyleElement;
  references: number;
};

const fontScopeStates = new WeakMap<Document, FontScopeState>();

export function extractFontScopeCss(css: string): string {
  return [...extractLeadingImports(css), ...extractTopLevelFontFaces(css)].join("\n");
}

export function installDocumentFontScope(doc: Document, css: string): () => void {
  const fontCss = extractFontScopeCss(css);
  if (fontCss.trim() === "") return () => {};

  const tracked = fontScopeStates.get(doc);
  if (tracked) {
    tracked.references += 1;
    return cleanupDocumentFontScope(doc, tracked.style);
  }

  const existing = doc.head.querySelector<HTMLStyleElement>(FONT_SCOPE_SELECTOR);
  if (existing) return () => {};

  const style = doc.createElement("style");
  style.setAttribute(FONT_SCOPE_ATTRIBUTE, "");
  style.textContent = fontCss;
  doc.head.appendChild(style);
  fontScopeStates.set(doc, { style, references: 1 });
  return cleanupDocumentFontScope(doc, style);
}

function cleanupDocumentFontScope(doc: Document, style: HTMLStyleElement): () => void {
  let active = true;
  return () => {
    if (!active) return;
    active = false;

    const state = fontScopeStates.get(doc);
    if (!state || state.style !== style) return;
    state.references -= 1;
    if (state.references > 0) return;

    state.style.remove();
    fontScopeStates.delete(doc);
  };
}

function extractLeadingImports(css: string): string[] {
  const imports: string[] = [];
  let index = skipWhitespaceAndComments(css, 0);
  while (startsWithAtRule(css, index, "@charset")) {
    const end = consumeStatement(css, index);
    if (end === null) return imports;
    index = skipWhitespaceAndComments(css, end);
  }

  while (startsWithAtRule(css, index, "@import")) {
    const end = consumeStatement(css, index);
    if (end === null) return imports;
    imports.push(css.slice(index, end).trim());
    index = skipWhitespaceAndComments(css, end);
  }

  return imports;
}

function extractTopLevelFontFaces(css: string): string[] {
  const blocks: string[] = [];
  let depth = 0;
  let index = 0;

  while (index < css.length) {
    const next = skipCommentOrString(css, index);
    if (next !== index) {
      index = next;
      continue;
    }

    if (depth === 0 && startsWithAtRule(css, index, "@font-face")) {
      const end = consumeBlock(css, index);
      if (end === null) return blocks;
      blocks.push(css.slice(index, end).trim());
      index = end;
      continue;
    }

    const char = css[index];
    if (char === "{") depth += 1;
    else if (char === "}") depth = Math.max(0, depth - 1);
    index += 1;
  }

  return blocks;
}

function consumeStatement(css: string, start: number): number | null {
  let index = start;
  while (index < css.length) {
    const next = skipCommentOrString(css, index);
    if (next !== index) {
      index = next;
      continue;
    }
    if (css[index] === ";") return index + 1;
    index += 1;
  }
  return null;
}

function consumeBlock(css: string, start: number): number | null {
  let index = start;
  while (index < css.length) {
    const next = skipCommentOrString(css, index);
    if (next !== index) {
      index = next;
      continue;
    }
    if (css[index] === "{") break;
    index += 1;
  }
  if (index >= css.length) return null;

  let depth = 0;
  while (index < css.length) {
    const next = skipCommentOrString(css, index);
    if (next !== index) {
      index = next;
      continue;
    }

    const char = css[index];
    if (char === "{") depth += 1;
    else if (char === "}") {
      depth -= 1;
      if (depth === 0) return index + 1;
    }
    index += 1;
  }

  return null;
}

function skipWhitespaceAndComments(css: string, start: number): number {
  let index = start;
  while (index < css.length) {
    const char = css[index];
    if (isCssWhitespace(char)) {
      index += 1;
      continue;
    }
    if (css.startsWith("/*", index)) {
      index = skipComment(css, index);
      continue;
    }
    break;
  }
  return index;
}

function skipCommentOrString(css: string, index: number): number {
  if (css.startsWith("/*", index)) return skipComment(css, index);
  const char = css[index];
  if (char === '"' || char === "'") return skipString(css, index);
  return index;
}

function skipComment(css: string, index: number): number {
  const end = css.indexOf("*/", index + 2);
  return end < 0 ? css.length : end + 2;
}

function skipString(css: string, index: number): number {
  const quote = css[index];
  index += 1;
  while (index < css.length) {
    const char = css[index];
    if (char === "\\") {
      index += 2;
      continue;
    }
    if (char === quote) return index + 1;
    index += 1;
  }
  return index;
}

function startsWithAtRule(css: string, index: number, rule: string): boolean {
  if (css.slice(index, index + rule.length).toLowerCase() !== rule) return false;
  const next = css[index + rule.length];
  return next === undefined || !/[a-zA-Z0-9_-]/.test(next);
}

function isCssWhitespace(char: string): boolean {
  return char === " " || char === "\n" || char === "\r" || char === "\t" || char === "\f";
}
