// jsdom never executes <script> because vitest does not opt into runScripts: "dangerously";
// the tests are structural only; real execution and order are covered by the real-Chrome
// checklist in docs/plans/2026-09-23-layout-scripts.md.
// The embedded distribution-viewer copy in crates/peitho-core/src/render.rs is the
// hand-mirrored twin; it runs as a plain <script> and cannot import this module.

const CLASSIC_JAVASCRIPT_TYPES = new Set(["text/javascript", "application/javascript"]);
const HTML_NAMESPACE = "http://www.w3.org/1999/xhtml";

function isHtmlScriptElement(
  script: HTMLScriptElement | SVGScriptElement
): script is HTMLScriptElement {
  return script.namespaceURI === HTML_NAMESPACE && script.localName === "script";
}

function isClassicType(script: HTMLScriptElement | SVGScriptElement): boolean {
  const type = script.getAttribute("type");
  if (type === null || type === "") return true;

  // A non-empty `type` is a whole-string essence match, not a parsed MIME type. A parameter
  // therefore stays non-classic, and trimming must not manufacture the executable empty case.
  return CLASSIC_JAVASCRIPT_TYPES.has(type.trim().toLowerCase());
}

function needsScopeWrap(script: HTMLScriptElement | SVGScriptElement): boolean {
  // SVG scripts are re-created in their own namespace so href/xlink:href still load; keep an
  // external script's empty body byte-identical rather than fabricating an inline body for it.
  if (
    script.hasAttribute("src") ||
    script.hasAttribute("href") ||
    script.hasAttribute("xlink:href")
  ) {
    return false;
  }

  return isClassicType(script);
}

function isParserBlocking(script: HTMLScriptElement | SVGScriptElement): boolean {
  return (
    isHtmlScriptElement(script) &&
    script.hasAttribute("src") &&
    isClassicType(script) &&
    !script.hasAttribute("async") &&
    !script.hasAttribute("defer")
  );
}

export function executeInlineScripts(root: ParentNode, doc: Document): void {
  const scripts = Array.from(
    root.querySelectorAll<HTMLScriptElement | SVGScriptElement>("script")
  );

  const replaceFrom = (startIndex: number): void => {
    for (let index = startIndex; index < scripts.length; index += 1) {
      const oldScript = scripts[index];
      const newScript = doc.createElementNS(
        oldScript.namespaceURI,
        oldScript.localName
      ) as HTMLScriptElement | SVGScriptElement;
      for (const attr of Array.from(oldScript.attributes)) {
        newScript.setAttributeNode(attr.cloneNode() as Attr);
      }
      // Dynamically created HTML scripts start force-async, and setting attributes cannot clear that
      // flag. `defer` only affects parser-inserted scripts, so only explicit `async` preserves it.
      // SVGScriptElement has no async IDL attribute, so external SVG scripts run in load order, not
      // document order (measured in Chrome); this is accepted because they previously never loaded.
      if (isHtmlScriptElement(newScript) && !oldScript.hasAttribute("async")) {
        newScript.async = false;
      }
      // Browsers hide the nonce content attribute after parsing, so getAttribute alone yields "".
      if (oldScript.nonce) {
        newScript.nonce = oldScript.nonce;
      }

      const text = oldScript.textContent ?? "";
      newScript.textContent = needsScopeWrap(oldScript)
        ? `(function () {\n${text}\n})();`
        : text;

      const parserBlocking = isParserBlocking(oldScript);
      if (parserBlocking) {
        // Parsed documents pause at blocking classic external scripts; PDF and lint get that
        // ordering natively. Re-created scripts are dynamic, so reproduce the parser's pause here.
        let resumed = false;
        const resume = (): void => {
          if (resumed) return;
          resumed = true;
          newScript.removeEventListener("load", resume);
          newScript.removeEventListener("error", resume);
          replaceFrom(index + 1);
        };
        newScript.addEventListener("load", resume, { once: true });
        newScript.addEventListener("error", resume, { once: true });
      }

      oldScript.replaceWith(newScript);
      if (parserBlocking) return;
    }
  };

  replaceFrom(0);
}
