// jsdom never executes <script> because vitest does not opt into runScripts: "dangerously";
// script rehydration is tested structurally, while plain DOM helpers execute directly in tests.
// Real script execution and order are covered by the Chrome-backed integration tests.
// The distribution viewer consumes these helpers through the IIFE bundle built from viewer.ts.

const CLASSIC_JAVASCRIPT_TYPES = new Set(["text/javascript", "application/javascript"]);
const HTML_NAMESPACE = "http://www.w3.org/1999/xhtml";

export const SHADOW_MOUNTED_EVENT = "peitho:shadow-mounted";

export type ShadowMountedDetail = {
  root: ShadowRoot | Element;
  key: string;
  index: number;
};

interface WindowWithShadowMountedBacklog {
  __peithoShadowRoots?: unknown;
}

export function shadowMountedBacklog(win: Window): ShadowMountedDetail[] {
  const backlogWindow = win as Window & WindowWithShadowMountedBacklog;
  if (!("__peithoShadowRoots" in backlogWindow)) {
    const backlog: ShadowMountedDetail[] = [];
    backlogWindow.__peithoShadowRoots = backlog;
    return backlog;
  }

  const backlog = backlogWindow.__peithoShadowRoots;
  if (!Array.isArray(backlog)) {
    throw new TypeError("window.__peithoShadowRoots must be an array");
  }
  return backlog as ShadowMountedDetail[];
}

export function dropDisconnectedShadowMounted(win: Window): void {
  const backlog = shadowMountedBacklog(win);
  let writeIndex = 0;
  for (const detail of backlog) {
    if (!detail.root.isConnected) continue;
    backlog[writeIndex] = detail;
    writeIndex += 1;
  }
  backlog.length = writeIndex;
}

export function announceShadowMounted(
  target: EventTarget,
  detail: ShadowMountedDetail,
  win: Window
): void {
  shadowMountedBacklog(win).push(detail);
  target.dispatchEvent(
    new CustomEvent<ShadowMountedDetail>(SHADOW_MOUNTED_EVENT, {
      detail,
      bubbles: true,
      composed: true
    })
  );
}

export function announceParsedSlides(
  doc: Document,
  win: Window,
  slides: ReadonlyArray<{ key: string; index: number }>
): void {
  const sections = Array.from(doc.querySelectorAll<HTMLElement>(".peitho-slide"));
  const resolved = slides.map((slide) => ({
    slide,
    matches: sections.filter((section) => section.dataset.slideKey === slide.key)
  }));
  const failures = resolved.filter(({ matches }) => matches.length !== 1);

  if (failures.length > 0) {
    const summary = failures
      .map(({ slide, matches }) => `${JSON.stringify(slide.key)} matched ${matches.length}`)
      .join(", ");
    throw new Error(`Unable to announce parsed slides: ${summary}`);
  }

  for (const { slide, matches } of resolved) {
    const section = matches[0]!;
    const detail = { root: section, key: slide.key, index: slide.index };
    announceShadowMounted(section, detail, win);
  }
}

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
