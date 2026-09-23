"use strict";
var PeithoViewer = (() => {
  var __defProp = Object.defineProperty;
  var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
  var __getOwnPropNames = Object.getOwnPropertyNames;
  var __hasOwnProp = Object.prototype.hasOwnProperty;
  var __export = (target, all) => {
    for (var name in all)
      __defProp(target, name, { get: all[name], enumerable: true });
  };
  var __copyProps = (to, from, except, desc) => {
    if (from && typeof from === "object" || typeof from === "function") {
      for (let key of __getOwnPropNames(from))
        if (!__hasOwnProp.call(to, key) && key !== except)
          __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
    }
    return to;
  };
  var __toCommonJS = (mod) => __copyProps(__defProp({}, "__esModule", { value: true }), mod);

  // src/viewer.ts
  var viewer_exports = {};
  __export(viewer_exports, {
    announceShadowMounted: () => announceShadowMounted,
    dropDisconnectedShadowMounted: () => dropDisconnectedShadowMounted,
    executeInlineScripts: () => executeInlineScripts,
    keyBelongsToTarget: () => keyBelongsToTarget,
    pointerBelongsToTarget: () => pointerBelongsToTarget,
    shadowMountedBacklog: () => shadowMountedBacklog
  });

  // src/scripts.ts
  var CLASSIC_JAVASCRIPT_TYPES = /* @__PURE__ */ new Set(["text/javascript", "application/javascript"]);
  var HTML_NAMESPACE = "http://www.w3.org/1999/xhtml";
  var SHADOW_MOUNTED_EVENT = "peitho:shadow-mounted";
  function shadowMountedBacklog(win) {
    const backlogWindow = win;
    if (!("__peithoShadowRoots" in backlogWindow)) {
      const backlog2 = [];
      backlogWindow.__peithoShadowRoots = backlog2;
      return backlog2;
    }
    const backlog = backlogWindow.__peithoShadowRoots;
    if (!Array.isArray(backlog)) {
      throw new TypeError("window.__peithoShadowRoots must be an array");
    }
    return backlog;
  }
  function dropDisconnectedShadowMounted(win) {
    const backlog = shadowMountedBacklog(win);
    let writeIndex = 0;
    for (const detail of backlog) {
      if (!detail.root.isConnected) continue;
      backlog[writeIndex] = detail;
      writeIndex += 1;
    }
    backlog.length = writeIndex;
  }
  function announceShadowMounted(target, detail, win) {
    shadowMountedBacklog(win).push(detail);
    target.dispatchEvent(
      new CustomEvent(SHADOW_MOUNTED_EVENT, {
        detail,
        bubbles: true,
        composed: true
      })
    );
  }
  function isHtmlScriptElement(script) {
    return script.namespaceURI === HTML_NAMESPACE && script.localName === "script";
  }
  function isClassicType(script) {
    const type = script.getAttribute("type");
    if (type === null || type === "") return true;
    return CLASSIC_JAVASCRIPT_TYPES.has(type.trim().toLowerCase());
  }
  function needsScopeWrap(script) {
    if (script.hasAttribute("src") || script.hasAttribute("href") || script.hasAttribute("xlink:href")) {
      return false;
    }
    return isClassicType(script);
  }
  function isParserBlocking(script) {
    return isHtmlScriptElement(script) && script.hasAttribute("src") && isClassicType(script) && !script.hasAttribute("async") && !script.hasAttribute("defer");
  }
  function executeInlineScripts(root, doc) {
    const scripts = Array.from(
      root.querySelectorAll("script")
    );
    const replaceFrom = (startIndex) => {
      for (let index = startIndex; index < scripts.length; index += 1) {
        const oldScript = scripts[index];
        const newScript = doc.createElementNS(
          oldScript.namespaceURI,
          oldScript.localName
        );
        for (const attr of Array.from(oldScript.attributes)) {
          newScript.setAttributeNode(attr.cloneNode());
        }
        if (isHtmlScriptElement(newScript) && !oldScript.hasAttribute("async")) {
          newScript.async = false;
        }
        if (oldScript.nonce) {
          newScript.nonce = oldScript.nonce;
        }
        const text = oldScript.textContent ?? "";
        newScript.textContent = needsScopeWrap(oldScript) ? `(function () {
${text}
})();` : text;
        const parserBlocking = isParserBlocking(oldScript);
        if (parserBlocking) {
          let resumed = false;
          const resume = () => {
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

  // src/interactiveTarget.ts
  var CONTENTEDITABLE_SELECTOR = "[contenteditable]";
  var INPUT_SELECTOR = "input";
  var ENTER_ACTIVATABLE_SELECTOR = "a[href], button, summary";
  var SPACE_ACTIVATABLE_SELECTOR = "button, summary";
  var CLICK_INTERACTIVE_SELECTOR = "a, button, summary, input, textarea, select, label";
  var TEXT_ENTRY_INPUT_TYPES = "text search email url tel password number date datetime-local month week time";
  var SPACE_ACTIVATABLE_INPUT_TYPES = "checkbox radio button submit reset image color file";
  var ENTER_ACTIVATABLE_INPUT_TYPES = "button submit reset image color file";
  var ARROW_ACTIVATABLE_INPUT_TYPES = "radio range";
  var ARROW_KEYS = "ArrowLeft ArrowRight ArrowUp ArrowDown";
  var RANGE_KEYS = "Home End PageUp PageDown";
  var textEntryInputTypes = new Set(TEXT_ENTRY_INPUT_TYPES.split(" "));
  var spaceActivatableInputTypes = new Set(SPACE_ACTIVATABLE_INPUT_TYPES.split(" "));
  var enterActivatableInputTypes = new Set(ENTER_ACTIVATABLE_INPUT_TYPES.split(" "));
  var arrowActivatableInputTypes = new Set(ARROW_ACTIVATABLE_INPUT_TYPES.split(" "));
  var arrowKeys = new Set(ARROW_KEYS.split(" "));
  var rangeKeys = new Set(RANGE_KEYS.split(" "));
  function isInsideSlide(origin) {
    let current = origin;
    while (true) {
      if (current instanceof Element && current.closest("[data-slide-key]") !== null) {
        return true;
      }
      const root = current.getRootNode();
      if (!(root instanceof ShadowRoot)) return false;
      if (root.host.hasAttribute("data-slide-key")) return true;
      current = root.host;
    }
  }
  function isEditableTarget(event) {
    const target = event.composedPath()[0];
    if (target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement) {
      return true;
    }
    if (target instanceof HTMLInputElement) return textEntryInputTypes.has(target.type);
    if (!(target instanceof HTMLElement)) return false;
    if (target.isContentEditable) return true;
    const editable = target.closest(CONTENTEDITABLE_SELECTOR);
    return editable !== null && editable.getAttribute("contenteditable") !== "false";
  }
  function keyBelongsToTarget(event) {
    if (isEditableTarget(event) && (event.shiftKey || event.key !== "PageUp" && event.key !== "PageDown")) {
      return true;
    }
    const origin = event.composedPath()[0];
    if (!(origin instanceof Element) || !isInsideSlide(origin)) return false;
    if (event.key === "Enter" && origin.closest(ENTER_ACTIVATABLE_SELECTOR) !== null) {
      return true;
    }
    if (event.key === " " && origin.closest(SPACE_ACTIVATABLE_SELECTOR) !== null) {
      return true;
    }
    const input = origin.closest(INPUT_SELECTOR);
    if (input === null) return false;
    if (event.key === " ") return spaceActivatableInputTypes.has(input.type);
    if (event.key === "Enter") return enterActivatableInputTypes.has(input.type);
    if (arrowKeys.has(event.key)) return arrowActivatableInputTypes.has(input.type);
    return input.type === "range" && rangeKeys.has(event.key);
  }
  function pointerBelongsToTarget(event) {
    const origin = event.composedPath()[0];
    if (origin instanceof Element && origin.closest(CLICK_INTERACTIVE_SELECTOR) !== null) {
      return true;
    }
    return isEditableTarget(event);
  }
  return __toCommonJS(viewer_exports);
})();
