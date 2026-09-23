// jsdom never executes <script> because vitest does not opt into runScripts: "dangerously";
// the tests are structural only; real execution and order are covered by the real-Chrome
// checklist in docs/plans/2026-09-23-layout-scripts.md.
// The embedded distribution-viewer copy in crates/peitho-core/src/render.rs is the
// hand-mirrored twin; it runs as a plain <script> and cannot import this module.

const CLASSIC_JAVASCRIPT_TYPES = new Set(["text/javascript", "application/javascript"]);

function needsScopeWrap(script: HTMLScriptElement): boolean {
  // SVG scripts use href/xlink:href; fabricating a body for one would be a silent drop.
  if (
    script.hasAttribute("src") ||
    script.hasAttribute("href") ||
    script.hasAttribute("xlink:href")
  ) {
    return false;
  }

  const type = script.getAttribute("type");
  if (type === null || type === "") return true;

  // A non-empty `type` is a whole-string essence match, not a parsed MIME type. A parameter
  // therefore stays unwrapped, and trimming must not manufacture the executable empty case.
  return CLASSIC_JAVASCRIPT_TYPES.has(type.trim().toLowerCase());
}

export function executeInlineScripts(root: ParentNode, doc: Document): void {
  const scripts = Array.from(root.querySelectorAll("script"));

  for (const oldScript of scripts) {
    const newScript = doc.createElement("script");
    for (const { name, value } of Array.from(oldScript.attributes)) {
      newScript.setAttribute(name, value);
    }
    // createElement sets a force-async flag that setAttribute cannot clear. `defer` only affects
    // parser-inserted scripts, so only an explicit `async` attribute preserves the flag here.
    if (!oldScript.hasAttribute("async")) {
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
    oldScript.replaceWith(newScript);
  }
}
