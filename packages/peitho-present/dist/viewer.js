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
    announceParsedSlides: () => announceParsedSlides,
    announceShadowMounted: () => announceShadowMounted,
    applyStepState: () => applyStepState,
    createStepStateStyle: () => createStepStateStyle,
    createViewerNavigator: () => createViewerNavigator,
    dropDisconnectedShadowMounted: () => dropDisconnectedShadowMounted,
    executeInlineScripts: () => executeInlineScripts,
    keyBelongsToTarget: () => keyBelongsToTarget,
    pointerBelongsToTarget: () => pointerBelongsToTarget,
    readViewerPosition: () => readViewerPosition,
    shadowMountedBacklog: () => shadowMountedBacklog,
    writeViewerPosition: () => writeViewerPosition
  });

  // src/skipnav.ts
  function nextNonSkippedIndex(slides, from, direction) {
    let index = from + direction;
    while (index >= 0 && index < slides.length) {
      if (slides[index].skip !== true) return index;
      index += direction;
    }
    return null;
  }

  // src/stepnav.ts
  function revealStepCount(slide) {
    const value = slide?.revealSteps;
    if (typeof value !== "number" || !Number.isFinite(value) || value <= 0) return 0;
    return Math.floor(value);
  }
  function clampStep(step, stepCount) {
    if (!Number.isFinite(step)) return 0;
    return Math.max(0, Math.min(Math.floor(step), stepCount));
  }
  function resolveStepTarget(slides, position, direction) {
    if (direction === 1) {
      const stepCount = revealStepCount(slides[position.index]);
      if (position.step < stepCount) {
        return { index: position.index, step: position.step + 1 };
      }
      const index2 = nextNonSkippedIndex(slides, position.index, 1);
      return index2 === null ? null : { index: index2, step: 0 };
    }
    if (position.step > 0) {
      return { index: position.index, step: position.step - 1 };
    }
    const index = nextNonSkippedIndex(slides, position.index, -1);
    return index === null ? null : { index, step: revealStepCount(slides[index]) };
  }

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
  function announceParsedSlides(doc, win, slides) {
    const sections = Array.from(doc.querySelectorAll(".peitho-slide"));
    const resolved = slides.map((slide) => ({
      slide,
      matches: sections.filter((section) => section.dataset.slideKey === slide.key)
    }));
    const failures = resolved.filter(({ matches }) => matches.length !== 1);
    if (failures.length > 0) {
      const summary = failures.map(({ slide, matches }) => `${JSON.stringify(slide.key)} matched ${matches.length}`).join(", ");
      throw new Error(`Unable to announce parsed slides: ${summary}`);
    }
    for (const { slide, matches } of resolved) {
      const section = matches[0];
      const detail = { root: section, key: slide.key, index: slide.index };
      announceShadowMounted(section, detail, win);
    }
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

  // src/stepState.ts
  var REVEAL_HIDDEN_CSS = "[data-reveal-hidden]{visibility:hidden}";
  var EMPHASIS_ACTIVE_CSS = [
    ".code-line{display:inline-block;width:100%}",
    "[data-emphasis-active]{",
    "background:var(--peitho-emphasis-background,rgba(217,163,0,0.18));",
    "box-shadow:inset 3px 0 0 var(--peitho-emphasis-marker,#d9a300)",
    "}",
    "pre:has([data-emphasis-active]) .code-line:not([data-emphasis-active]){",
    "opacity:var(--peitho-emphasis-dim,0.45)",
    "}"
  ].join("");
  function createStepStateStyle(doc) {
    const style = doc.createElement("style");
    style.textContent = REVEAL_HIDDEN_CSS + EMPHASIS_ACTIVE_CSS;
    return style;
  }
  function applyEmphasisState(root, step) {
    for (const marker of root.querySelectorAll("[data-emphasis-step]")) {
      const steps = (marker.dataset.emphasisStep ?? "").trim().split(/\s+/).filter(Boolean).map(Number);
      marker.toggleAttribute("data-emphasis-active", steps.includes(step));
    }
  }
  function applyStepState(root, step) {
    for (const marker of root.querySelectorAll("[data-reveal-step]")) {
      const revealStep = Number(marker.dataset.revealStep);
      marker.toggleAttribute(
        "data-reveal-hidden",
        Number.isFinite(revealStep) && revealStep > step
      );
    }
    applyEmphasisState(root, step);
  }

  // src/viewer.ts
  function createViewerNavigator(options) {
    const slides = options.slides.map(({ revealSteps }) => ({ revealSteps }));
    function normalizedIndex(index) {
      if (slides.length === 0) return 0;
      const integer = Number.isFinite(index) ? Math.floor(index) : 0;
      return Math.max(0, Math.min(integer, slides.length - 1));
    }
    function direct(index, requestedStep) {
      const nextIndex = normalizedIndex(index);
      const stepCount = revealStepCount(slides[nextIndex]);
      if (!options.walkRevealSteps) return { index: nextIndex, step: stepCount };
      const validStep = requestedStep !== void 0 && Number.isSafeInteger(requestedStep) && requestedStep >= 0;
      return {
        index: nextIndex,
        step: validStep ? clampStep(requestedStep, stepCount) : stepCount
      };
    }
    function move(position, direction) {
      const current = direct(position.index, position.step);
      if (slides.length === 0) return current;
      if (!options.walkRevealSteps) {
        const nextIndex = current.index + direction;
        if (nextIndex < 0 || nextIndex >= slides.length) return current;
        return direct(nextIndex);
      }
      return resolveStepTarget(slides, current, direction) ?? current;
    }
    return { walkRevealSteps: options.walkRevealSteps, direct, move };
  }
  function parseUrlInteger(raw) {
    if (raw === null || !/^\d+$/.test(raw)) return void 0;
    const value = Number(raw);
    return Number.isSafeInteger(value) ? value : void 0;
  }
  function slideFromUrl(url) {
    const querySlide = url.searchParams.get("slide");
    if (querySlide !== null) return parseUrlInteger(querySlide) ?? 1;
    if (url.hash.startsWith("#slide=")) {
      return parseUrlInteger(url.hash.slice("#slide=".length).split("&")[0] ?? null) ?? 1;
    }
    if (url.hash.startsWith("#") && url.hash.length > 1) {
      return parseUrlInteger(url.hash.slice(1).split("&")[0] ?? null) ?? 1;
    }
    return 1;
  }
  function readViewerPosition(url, navigator) {
    const oneBasedSlide = slideFromUrl(url);
    const requestedStep = parseUrlInteger(url.searchParams.get("step"));
    return navigator.direct(oneBasedSlide - 1, requestedStep);
  }
  function writeViewerPosition(url, position, navigator) {
    const next = new URL(url.href);
    const normalized = navigator.direct(position.index, position.step);
    next.searchParams.set("slide", String(normalized.index + 1));
    if (navigator.walkRevealSteps) {
      next.searchParams.set("step", String(normalized.step));
    } else {
      next.searchParams.delete("step");
    }
    return next;
  }
  return __toCommonJS(viewer_exports);
})();
