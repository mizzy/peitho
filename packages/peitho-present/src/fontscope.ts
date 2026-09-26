const FONT_SCOPE_ATTRIBUTE = "data-peitho-font-scope";
const FONT_SCOPE_SELECTOR = `style[${FONT_SCOPE_ATTRIBUTE}]`;

type FontScopeState = {
  style: HTMLStyleElement;
  references: number;
};

const fontScopeStates = new WeakMap<Document, FontScopeState>();

export function installDocumentFontScope(doc: Document, fontCss: string): () => void {
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
