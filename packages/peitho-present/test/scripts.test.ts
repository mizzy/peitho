// jsdom never executes <script> because vitest does not opt into runScripts: "dangerously";
// these tests are structural only: real execution and execution order are not observable here.
// The real-Chrome checklist is in docs/plans/2026-09-23-layout-scripts.md.
import { expect, it } from "vitest";
import { executeInlineScripts } from "../src/scripts";

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

  const newScripts = Array.from(root.querySelectorAll("script"));
  expect(newScripts).toHaveLength(oldScripts.length);
  newScripts.forEach((newScript, index) => {
    expect(newScript).not.toBe(oldScripts[index]);
  });
  expect(newScripts.map((script) => script.textContent)).toEqual(originalTexts);
});

it("keeps external SVG script text byte-identical", () => {
  const root = document.createElement("div");
  root.innerHTML =
    '<svg><script href="ext.js"></script><script xlink:href="legacy.js"></script></svg>';
  const oldScripts = Array.from(root.querySelectorAll("svg script"));

  executeInlineScripts(root, document);

  const newScripts = Array.from(root.querySelectorAll("svg script"));
  expect(newScripts[0]).not.toBe(oldScripts[0]);
  expect(newScripts[1]).not.toBe(oldScripts[1]);
  expect(newScripts.map((script) => script.textContent)).toEqual(["", ""]);
  expect(newScripts[0].getAttribute("href")).toBe("ext.js");
  expect(newScripts[1].getAttribute("xlink:href")).toBe("legacy.js");
});

it("clears the force-async flag that createElement sets", () => {
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
    createElement: () => replacements[replacementIndex++]
  } as unknown as Document;

  executeInlineScripts(root, forceAsyncDocument);

  const [orderedScript, asyncScript, deferScript] = Array.from(root.querySelectorAll("script"));
  expect(orderedScript.async).toBe(false);
  expect(orderedScript.hasAttribute("async")).toBe(false);
  expect(asyncScript.async).toBe(true);
  expect(asyncScript.hasAttribute("async")).toBe(true);
  expect(deferScript.async).toBe(false);
  expect(deferScript.hasAttribute("async")).toBe(false);
  expect(deferScript.hasAttribute("defer")).toBe(true);
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
