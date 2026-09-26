import { expect, it } from "vitest";
import { installDocumentFontScope } from "../src/fontscope";

const selector = "style[data-peitho-font-scope]";

it("installs the already-computed font scope css byte-for-byte and cleans it up", () => {
  const doc = document.implementation.createHTMLDocument();
  const fontCss =
    '@font-face { font-family: "Deck"; src: url("deck.woff2"); font-display:swap; }';

  const cleanup = installDocumentFontScope(doc, fontCss);

  expect(doc.head.querySelector<HTMLStyleElement>(selector)?.textContent).toBe(fontCss);
  cleanup();
  cleanup();
  expect(doc.head.querySelector(selector)).toBeNull();
});

it("reference-counts concurrent installs and removes the style after the last cleanup", () => {
  const doc = document.implementation.createHTMLDocument();
  const firstCleanup = installDocumentFontScope(doc, "@import url(fonts.css);");
  const secondCleanup = installDocumentFontScope(doc, "unused because the document is tracked");

  expect(doc.head.querySelectorAll(selector)).toHaveLength(1);
  firstCleanup();
  expect(doc.head.querySelectorAll(selector)).toHaveLength(1);
  secondCleanup();
  expect(doc.head.querySelectorAll(selector)).toHaveLength(0);
});

it("does not install an empty font scope style", () => {
  const doc = document.implementation.createHTMLDocument();

  const cleanup = installDocumentFontScope(doc, " \n\t");

  expect(doc.head.querySelector(selector)).toBeNull();
  cleanup();
});

it("leaves an externally installed font scope style untouched", () => {
  const doc = document.implementation.createHTMLDocument();
  const existing = doc.createElement("style");
  existing.setAttribute("data-peitho-font-scope", "");
  existing.textContent = "external";
  doc.head.appendChild(existing);

  const cleanup = installDocumentFontScope(doc, "computed");
  cleanup();

  expect(doc.head.querySelectorAll(selector)).toHaveLength(1);
  expect(existing.isConnected).toBe(true);
  expect(existing.textContent).toBe("external");
});
