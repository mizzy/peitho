// jsdom never executes <script> because vitest does not opt into runScripts: "dangerously";
// these tests are structural only, with parser-blocking progress simulated by dispatching events.
// The real-Chrome checklist is in docs/plans/2026-09-23-layout-scripts.md.
import { expect, it } from "vitest";
import { executeInlineScripts } from "../src/scripts";

const HTML_NAMESPACE = "http://www.w3.org/1999/xhtml";
const SVG_NAMESPACE = "http://www.w3.org/2000/svg";
const XLINK_NAMESPACE = "http://www.w3.org/1999/xlink";

it("replaces a classic inline script in place with all attributes and a scoped body", () => {
  const root = document.createElement("div");
  root.innerHTML =
    '<span id="before"></span><script type="text/javascript" data-layout="hero" async>let answer = 42;</script><span id="after"></span>';
  const before = root.firstChild;
  const oldScript = root.querySelector("script")!;
  const after = root.lastChild;
  const oldAttributes = Array.from(oldScript.attributes, ({ name, value }) => [name, value]);

  executeInlineScripts(root, document);

  const newScript = root.querySelector("script")!;
  expect(newScript).not.toBe(oldScript);
  expect(root.childNodes).toHaveLength(3);
  expect(root.childNodes[0]).toBe(before);
  expect(root.childNodes[1]).toBe(newScript);
  expect(root.childNodes[2]).toBe(after);
  expect(Array.from(newScript.attributes, ({ name, value }) => [name, value])).toEqual(
    oldAttributes
  );
  expect(newScript.hasAttribute("nonce")).toBe(false);
  expect(newScript.outerHTML).not.toContain("nonce");
  expect(newScript.textContent).toBe("(function () {\nlet answer = 42;\n})();");
});

it("preserves null-namespace colon attributes and continues replacing scripts", () => {
  const root = document.createElement("div");
  root.innerHTML =
    '<script x-on:load="f()" xml:lang="en">let a = 1;</script><script>let b = 2;</script>';
  const oldScripts = Array.from(root.querySelectorAll("script"));

  executeInlineScripts(root, document);

  const newScripts = Array.from(root.querySelectorAll("script"));
  expect(newScripts).toHaveLength(2);
  expect(newScripts[0]).not.toBe(oldScripts[0]);
  expect(newScripts[1]).not.toBe(oldScripts[1]);
  expect(newScripts[0].getAttribute("x-on:load")).toBe("f()");
  expect(newScripts[0].getAttribute("xml:lang")).toBe("en");
  expect(newScripts.map((script) => script.textContent)).toEqual([
    "(function () {\nlet a = 1;\n})();",
    "(function () {\nlet b = 2;\n})();"
  ]);
});

it("copies a nonce from the IDL property when the content attribute is hidden", () => {
  const root = document.createElement("div");
  const oldScript = document.createElement("script");
  oldScript.setAttribute("nonce", "");
  // Browsers retain the nonce on the IDL property when getAttribute returns an empty string.
  Object.defineProperty(oldScript, "nonce", { configurable: true, value: "property-nonce" });
  root.appendChild(oldScript);

  executeInlineScripts(root, document);

  expect(oldScript.getAttribute("nonce")).toBe("");
  expect(root.querySelector("script")?.nonce).toBe("property-nonce");
});

it("wraps only supported classic types with exact empty-value semantics", () => {
  const root = document.createElement("div");
  const normalizedTypeScript = document.createElement("script");
  normalizedTypeScript.setAttribute("type", " TEXT/JavaScript ");
  normalizedTypeScript.textContent = 'const label = "mixed case";';
  const whitespaceTypeScript = document.createElement("script");
  whitespaceTypeScript.setAttribute("type", "  ");
  whitespaceTypeScript.textContent = "\r\ninert whitespace type\r\n";
  const emptyTypeScript = document.createElement("script");
  emptyTypeScript.setAttribute("type", "");
  emptyTypeScript.textContent = "let emptyType = 1;";
  const legacyTypeScript = document.createElement("script");
  legacyTypeScript.setAttribute("type", "application/ecmascript");
  legacyTypeScript.textContent = "inert legacy type";
  root.append(normalizedTypeScript, whitespaceTypeScript, emptyTypeScript, legacyTypeScript);

  executeInlineScripts(root, document);

  expect(Array.from(root.querySelectorAll("script"), (script) => script.textContent)).toEqual([
    '(function () {\nconst label = "mixed case";\n})();',
    "\r\ninert whitespace type\r\n",
    "(function () {\nlet emptyType = 1;\n})();",
    "inert legacy type"
  ]);
});

it("keeps parameterized script types byte-identical", () => {
  const root = document.createElement("div");
  const classicScript = document.createElement("script");
  classicScript.setAttribute("type", "text/javascript; charset=utf-8");
  classicScript.textContent = "\nlet parameterized = 1;\r\n";
  const dataScript = document.createElement("script");
  dataScript.setAttribute("type", "application/json; charset=utf-8");
  dataScript.textContent = '\r\n{ "value": "<&>" }\r\n';
  const moduleScript = document.createElement("script");
  moduleScript.setAttribute("type", "module; x=1");
  moduleScript.textContent = '\nconst value = "module";\r\n';
  const originalTexts = [classicScript.textContent, dataScript.textContent, moduleScript.textContent];
  root.append(classicScript, dataScript, moduleScript);

  executeInlineScripts(root, document);

  expect(Array.from(root.querySelectorAll("script"), (script) => script.textContent)).toEqual(
    originalTexts
  );
});

it("keeps module, source, and non-javascript script text byte-identical", () => {
  const root = document.createElement("div");
  const moduleScript = document.createElement("script");
  moduleScript.type = "module";
  moduleScript.textContent = '\r\nconst moduleValue = "<&>";\r\n';
  const sourceScript = document.createElement("script");
  sourceScript.src = "layout.js";
  sourceScript.textContent = "\nsource fallback\r\n";
  const classicSourceScript = document.createElement("script");
  classicSourceScript.type = "text/javascript";
  classicSourceScript.src = "classic-layout.js";
  classicSourceScript.textContent = "let sourceValue = 1;\n";
  const dataScript = document.createElement("script");
  dataScript.type = "application/json";
  dataScript.textContent = '\n{ "title": "<&>" }\r\n';
  const oldScripts = [moduleScript, sourceScript, classicSourceScript, dataScript];
  const originalTexts = oldScripts.map((script) => script.textContent);
  root.append(...oldScripts);

  executeInlineScripts(root, document);
  root.querySelectorAll("script")[1].dispatchEvent(new Event("load"));
  root.querySelectorAll("script")[2].dispatchEvent(new Event("load"));

  const newScripts = Array.from(root.querySelectorAll("script"));
  expect(newScripts).toHaveLength(oldScripts.length);
  newScripts.forEach((newScript, index) => {
    expect(newScript).not.toBe(oldScripts[index]);
  });
  expect(newScripts.map((script) => script.textContent)).toEqual(originalTexts);
});

it("re-creates an href SVG script in the SVG namespace with byte-identical text", () => {
  const root = document.createElement("div");
  root.innerHTML = '<svg><script href="ext.js"></script></svg>';
  const oldScript = root.querySelector("svg script")!;

  executeInlineScripts(root, document);

  const newScript = root.querySelector("svg script")!;
  expect(newScript).not.toBe(oldScript);
  expect(newScript.namespaceURI).toBe(SVG_NAMESPACE);
  expect(newScript.getAttribute("href")).toBe("ext.js");
  expect(newScript.textContent).toBe("");
});

it("preserves namespaced and null-namespace colon attributes on an SVG script", () => {
  const root = document.createElement("div");
  root.innerHTML = '<svg><script xlink:href="ext.js" data-a:b="c"></script></svg>';

  executeInlineScripts(root, document);

  const newScript = root.querySelector("svg script")!;
  const xlinkHref = newScript.getAttributeNodeNS(XLINK_NAMESPACE, "href");
  expect(newScript.namespaceURI).toBe(SVG_NAMESPACE);
  expect(xlinkHref?.namespaceURI).toBe(XLINK_NAMESPACE);
  expect(xlinkHref?.localName).toBe("href");
  expect(newScript.getAttributeNS(XLINK_NAMESPACE, "href")).toBe("ext.js");
  expect(newScript.getAttribute("data-a:b")).toBe("c");
});

it("re-creates an inline SVG script in the SVG namespace with a scoped body", () => {
  const root = document.createElement("div");
  root.innerHTML = "<svg><script>let svgValue = 1;</script></svg>";

  executeInlineScripts(root, document);

  const newScript = root.querySelector("svg script")!;
  expect(newScript.namespaceURI).toBe(SVG_NAMESPACE);
  expect(newScript.textContent).toBe("(function () {\nlet svgValue = 1;\n})();");
});

it("continues to re-create an HTML script as an HTMLScriptElement", () => {
  const root = document.createElement("div");
  root.innerHTML = "<script>let htmlValue = 1;</script>";

  executeInlineScripts(root, document);

  const newScript = root.querySelector("script")!;
  expect(newScript).toBeInstanceOf(HTMLScriptElement);
  expect(newScript.namespaceURI).toBe(HTML_NAMESPACE);
});

it("clears the force-async flag that dynamic HTML script creation sets", () => {
  const root = document.createElement("div");
  root.innerHTML =
    '<script src="chart-lib.js"></script><script src="chart-async.js" async></script><script src="chart-defer.js" defer></script>';
  // jsdom lacks HTMLScriptElement.async, so the fake document is the only way to observe the
  // assignment; real execution order is covered by the real-Chrome checklist named above.
  const replacements = [
    document.createElement("script"),
    document.createElement("script"),
    document.createElement("script")
  ];
  for (const script of replacements) {
    let forceAsync = true;
    Object.defineProperty(script, "async", {
      configurable: true,
      get: () => forceAsync,
      set: (value: boolean) => {
        forceAsync = value;
      }
    });
  }
  let replacementIndex = 0;
  const forceAsyncDocument = {
    createElementNS: () => replacements[replacementIndex++]
  } as unknown as Document;

  executeInlineScripts(root, forceAsyncDocument);
  replacements[0].dispatchEvent(new Event("load"));

  const [orderedScript, asyncScript, deferScript] = Array.from(root.querySelectorAll("script"));
  expect(orderedScript.async).toBe(false);
  expect(orderedScript.hasAttribute("async")).toBe(false);
  expect(asyncScript.async).toBe(true);
  expect(asyncScript.hasAttribute("async")).toBe(true);
  expect(deferScript.async).toBe(false);
  expect(deferScript.hasAttribute("async")).toBe(false);
  expect(deferScript.hasAttribute("defer")).toBe(true);
});

it("waits for a parser-blocking external script to load before re-creating the next script", () => {
  const root = document.createElement("div");
  root.innerHTML = '<script src="lib.js"></script><script>use()</script>';
  const [oldExternal, oldInline] = Array.from(root.querySelectorAll("script"));

  executeInlineScripts(root, document);

  const [newExternal, pendingInline] = Array.from(root.querySelectorAll("script"));
  expect(newExternal).not.toBe(oldExternal);
  expect(pendingInline).toBe(oldInline);
  expect(pendingInline.textContent).toBe("use()");

  newExternal.dispatchEvent(new Event("load"));

  const newInline = root.querySelectorAll("script")[1];
  expect(newInline).not.toBe(oldInline);
  expect(newInline.textContent).toBe("(function () {\nuse()\n})();");
});

it("continues after a parser-blocking external script fails to load", () => {
  const root = document.createElement("div");
  root.innerHTML = '<script src="lib.js"></script><script>use()</script>';
  const [oldExternal, oldInline] = Array.from(root.querySelectorAll("script"));

  executeInlineScripts(root, document);

  const [newExternal, pendingInline] = Array.from(root.querySelectorAll("script"));
  expect(newExternal).not.toBe(oldExternal);
  expect(pendingInline).toBe(oldInline);

  newExternal.dispatchEvent(new Event("error"));

  const newInline = root.querySelectorAll("script")[1];
  expect(newInline).not.toBe(oldInline);
  expect(newInline.textContent).toBe("(function () {\nuse()\n})();");
});

it("waits for parser-blocking external scripts one at a time in document order", () => {
  const root = document.createElement("div");
  root.innerHTML =
    '<script src="first.js"></script><script src="second.js"></script><script>use()</script>';
  const [oldFirst, oldSecond, oldInline] = Array.from(root.querySelectorAll("script"));

  executeInlineScripts(root, document);

  let [newFirst, pendingSecond, pendingInline] = Array.from(root.querySelectorAll("script"));
  expect(newFirst).not.toBe(oldFirst);
  expect(pendingSecond).toBe(oldSecond);
  expect(pendingInline).toBe(oldInline);

  newFirst.dispatchEvent(new Event("load"));

  const scriptsAfterFirstLoad = Array.from(root.querySelectorAll("script"));
  const newSecond = scriptsAfterFirstLoad[1];
  newFirst = scriptsAfterFirstLoad[0];
  pendingInline = scriptsAfterFirstLoad[2];
  expect(newFirst).not.toBe(oldFirst);
  expect(newSecond).not.toBe(oldSecond);
  expect(pendingInline).toBe(oldInline);

  newSecond.dispatchEvent(new Event("load"));

  const newInline = root.querySelectorAll("script")[2];
  expect(newInline).not.toBe(oldInline);
  expect(newInline.textContent).toBe("(function () {\nuse()\n})();");
});

it.each([
  ["async", '<script src="a.js" async></script>'],
  ["defer", '<script src="b.js" defer></script>'],
  ["module", '<script type="module" src="c.js"></script>'],
  ["non-JavaScript", '<script type="application/json" src="d.json"></script>'],
  ["SVG", '<svg><script href="e.js"></script></svg>']
])("does not wait for a non-blocking %s external script", (_kind, externalMarkup) => {
  const root = document.createElement("div");
  root.innerHTML = `${externalMarkup}<script>use()</script>`;
  const [oldExternal, oldInline] = Array.from(root.querySelectorAll("script"));

  executeInlineScripts(root, document);

  const [newExternal, newInline] = Array.from(root.querySelectorAll("script"));
  expect(newExternal).not.toBe(oldExternal);
  expect(newInline).not.toBe(oldInline);
  expect(newInline.textContent).toBe("(function () {\nuse()\n})();");
});

it("resumes a parser-blocked walk only once when both completion events are dispatched", () => {
  const root = document.createElement("div");
  root.innerHTML = '<script src="lib.js"></script><script>use()</script>';
  const oldInline = root.querySelectorAll("script")[1];

  executeInlineScripts(root, document);

  const newExternal = root.querySelector("script")!;
  newExternal.dispatchEvent(new Event("load"));
  const newInline = root.querySelectorAll("script")[1];
  expect(newInline).not.toBe(oldInline);

  newExternal.dispatchEvent(new Event("error"));

  expect(root.querySelectorAll("script")[1]).toBe(newInline);
});

it("replaces every script across nested descendants", () => {
  const root = document.createElement("div");
  root.innerHTML = [
    "<script>let first = 1;</script>",
    "<section>",
    '  <script type="application/javascript">const second = 2;</script>',
    "  <div><script>var third = 3;</script></div>",
    "</section>"
  ].join("");
  const oldScripts = Array.from(root.querySelectorAll("script"));
  const oldParents = oldScripts.map((script) => script.parentNode);

  executeInlineScripts(root, document);

  const newScripts = Array.from(root.querySelectorAll("script"));
  expect(newScripts).toHaveLength(3);
  newScripts.forEach((newScript, index) => {
    expect(newScript).not.toBe(oldScripts[index]);
    expect(newScript.parentNode).toBe(oldParents[index]);
  });
  expect(newScripts.map((script) => script.textContent)).toEqual([
    "(function () {\nlet first = 1;\n})();",
    "(function () {\nconst second = 2;\n})();",
    "(function () {\nvar third = 3;\n})();"
  ]);
});

it("leaves content without scripts untouched", () => {
  const root = document.createElement("div");
  root.innerHTML = '<p data-layout="plain">No scripts <em>here</em>.</p><!-- keep -->';
  const originalHtml = root.innerHTML;
  const originalNodes = Array.from(root.childNodes);

  executeInlineScripts(root, document);

  expect(root.innerHTML).toBe(originalHtml);
  expect(Array.from(root.childNodes)).toEqual(originalNodes);
});

it("works with a bare document fragment", () => {
  const fragment = document.createDocumentFragment();
  const before = document.createElement("p");
  const oldScript = document.createElement("script");
  const after = document.createComment("after");
  oldScript.textContent = "let fragmentValue = true;";
  fragment.append(before, oldScript, after);

  executeInlineScripts(fragment, document);

  const newScript = fragment.querySelector("script")!;
  expect(newScript).not.toBe(oldScript);
  expect(fragment.childNodes).toHaveLength(3);
  expect(fragment.childNodes[0]).toBe(before);
  expect(fragment.childNodes[1]).toBe(newScript);
  expect(fragment.childNodes[2]).toBe(after);
  expect(newScript.textContent).toBe("(function () {\nlet fragmentValue = true;\n})();");
});
