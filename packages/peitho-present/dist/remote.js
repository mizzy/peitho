// src/sections.ts
function sectionIndexForSlide(sections, slideIndex) {
  return sections.findIndex(
    (section) => slideIndex >= section.startIndex && slideIndex <= section.endIndex
  );
}

// src/canvas.ts
function calculateCanvasFit(viewport, canvasWidth, canvasHeight) {
  const scale = Math.min(viewport.width / canvasWidth, viewport.height / canvasHeight);
  const width = canvasWidth * scale;
  const height = canvasHeight * scale;
  return {
    scale,
    width,
    height,
    left: (viewport.width - width) / 2,
    top: (viewport.height - height) / 2
  };
}
function installCanvasScaler(options) {
  const win = options.window ?? window;
  const canvasWidth = options.canvasWidth;
  const canvasHeight = options.canvasHeight;
  const viewport = options.viewport ?? (() => ({
    width: win.innerWidth,
    height: win.innerHeight
  }));
  function apply() {
    const fit = calculateCanvasFit(viewport(), canvasWidth, canvasHeight);
    options.target.style.width = `${canvasWidth}px`;
    options.target.style.height = `${canvasHeight}px`;
    options.target.style.transformOrigin = "top left";
    options.target.style.transform = `translate(${fit.left}px, ${fit.top}px) scale(${fit.scale})`;
  }
  apply();
  win.addEventListener("resize", apply);
  return () => win.removeEventListener("resize", apply);
}

// src/fontscope.ts
var FONT_SCOPE_ATTRIBUTE = "data-peitho-font-scope";
var FONT_SCOPE_SELECTOR = `style[${FONT_SCOPE_ATTRIBUTE}]`;
var fontScopeStates = /* @__PURE__ */ new WeakMap();
function extractFontScopeCss(css) {
  return [...extractLeadingImports(css), ...extractTopLevelFontFaces(css)].join("\n");
}
function installDocumentFontScope(doc, css) {
  const fontCss = extractFontScopeCss(css);
  if (fontCss.trim() === "") return () => {
  };
  const tracked = fontScopeStates.get(doc);
  if (tracked) {
    tracked.references += 1;
    return cleanupDocumentFontScope(doc, tracked.style);
  }
  const existing = doc.head.querySelector(FONT_SCOPE_SELECTOR);
  if (existing) return () => {
  };
  const style = doc.createElement("style");
  style.setAttribute(FONT_SCOPE_ATTRIBUTE, "");
  style.textContent = fontCss;
  doc.head.appendChild(style);
  fontScopeStates.set(doc, { style, references: 1 });
  return cleanupDocumentFontScope(doc, style);
}
function cleanupDocumentFontScope(doc, style) {
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
function extractLeadingImports(css) {
  const imports = [];
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
function extractTopLevelFontFaces(css) {
  const blocks = [];
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
function consumeStatement(css, start) {
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
function consumeBlock(css, start) {
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
function skipWhitespaceAndComments(css, start) {
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
function skipCommentOrString(css, index) {
  if (css.startsWith("/*", index)) return skipComment(css, index);
  const char = css[index];
  if (char === '"' || char === "'") return skipString(css, index);
  return index;
}
function skipComment(css, index) {
  const end = css.indexOf("*/", index + 2);
  return end < 0 ? css.length : end + 2;
}
function skipString(css, index) {
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
function startsWithAtRule(css, index, rule) {
  if (css.slice(index, index + rule.length).toLowerCase() !== rule) return false;
  const next = css[index + rule.length];
  return next === void 0 || !/[a-zA-Z0-9_-]/.test(next);
}
function isCssWhitespace(char) {
  return char === " " || char === "\n" || char === "\r" || char === "	" || char === "\f";
}

// src/skipnav.ts
function nextNonSkippedIndex(slides, from, direction) {
  let index = from + direction;
  while (index >= 0 && index < slides.length) {
    if (slides[index].skip !== true) return index;
    index += direction;
  }
  return null;
}
function initialSlideIndex(slides) {
  if (slides.length === 0) return null;
  return nextNonSkippedIndex(slides, -1, 1) ?? 0;
}

// src/shell.ts
async function mountPresentShell(options) {
  const shell = new PresentShellController(options);
  await shell.load();
  return shell;
}
var PresentShellController = class {
  manifest = null;
  currentIndex = -1;
  slides = [];
  root;
  fetcher;
  injectedManifest;
  win;
  doc;
  log;
  bus;
  now;
  viewport;
  canvasCleanups = [];
  fontScopeCleanup = null;
  startedAtValue = null;
  pausedAtValue = null;
  pausedTotalMs = 0;
  ended = false;
  onNavigate = (event) => {
    const detail = event.detail;
    if (!detail || !("to" in detail)) {
      this.log.error("Invalid peitho:navigate event");
      return;
    }
    this.navigate(detail.to);
  };
  onTimerControl = (event) => {
    const action = event.detail?.action;
    if (action === "start") this.startPresentation();
    else if (action === "pause") this.pauseTimer();
    else if (action === "resume") this.resumeTimer();
    else if (action === "reset") this.resetTimer();
    else this.log.error("Invalid peitho:timercontrol event");
  };
  onPageHide = () => this.endPresentation();
  constructor(options) {
    this.root = options.root;
    this.injectedManifest = options.manifest;
    this.fetcher = options.fetcher ?? fetch.bind(globalThis);
    this.win = options.window ?? window;
    this.doc = options.document ?? document;
    this.log = options.console ?? console;
    this.bus = options.bus ?? this.win;
    this.now = options.now ?? Date.now;
    this.viewport = options.viewport;
    this.root.classList.add("peitho-shell-viewport");
    const rootPosition = this.win.getComputedStyle(this.root).position;
    if (rootPosition === "static" || rootPosition === "") {
      this.root.style.position = "relative";
    }
    this.root.style.overflow = "hidden";
    this.root.style.background = "#000";
    this.bus.addEventListener("peitho:navigate", this.onNavigate);
    this.bus.addEventListener("peitho:timercontrol", this.onTimerControl);
    this.win.addEventListener("pagehide", this.onPageHide);
  }
  async load() {
    try {
      const manifest = this.injectedManifest ?? await this.fetchJson("manifest.json");
      const dimensions = {
        width: manifest.canvasWidth,
        height: manifest.canvasHeight
      };
      const cssAspect = manifest.aspectRatio.replace(":", " / ");
      this.setCanvasRootProperties(dimensions, cssAspect);
      const css = await this.fetchText("peitho.css");
      this.fontScopeCleanup = installDocumentFontScope(this.doc, css);
      const pending = [];
      for (const slide of manifest.slides) {
        const html = await this.fetchText(slide.src);
        const host = this.createSlideHost(slide, html, css, dimensions);
        pending.push({ meta: slide, host });
      }
      this.manifest = manifest;
      for (const view of pending) {
        this.root.appendChild(view.host);
        this.slides.push(view);
      }
      this.show(initialSlideIndex(pending.map((view) => view.meta)) ?? 0);
    } catch (error) {
      this.clearCanvasRootProperties();
      this.root.replaceChildren();
      this.root.textContent = error instanceof Error ? error.message : String(error);
    }
  }
  navigate(to) {
    const index = this.resolveTarget(to);
    if (index === null) return;
    this.show(index);
  }
  elapsedMs() {
    if (this.startedAtValue === null) return 0;
    const current = this.now();
    const pausedNow = this.pausedAtValue === null ? 0 : current - this.pausedAtValue;
    return Math.max(0, current - this.startedAtValue - this.pausedTotalMs - pausedNow);
  }
  isPaused() {
    return this.pausedAtValue !== null;
  }
  startedAt() {
    return this.startedAtValue;
  }
  adoptTimerState(state) {
    const elapsedMs = Math.max(0, state.elapsedMs);
    const previousElapsedMs = this.elapsedMs();
    if (!state.running && elapsedMs === 0) {
      this.startedAtValue = null;
      this.pausedAtValue = null;
      this.pausedTotalMs = 0;
      this.ended = false;
      this.dispatchTimerAdopt(elapsedMs, state.running, previousElapsedMs);
      return;
    }
    const now = this.now();
    this.startedAtValue = now - elapsedMs;
    this.pausedAtValue = state.running ? null : now;
    this.pausedTotalMs = 0;
    this.ended = false;
    this.dispatchTimerAdopt(elapsedMs, state.running, previousElapsedMs);
  }
  destroy() {
    this.endPresentation();
    this.fontScopeCleanup?.();
    this.fontScopeCleanup = null;
    while (this.canvasCleanups.length > 0) this.canvasCleanups.pop()?.();
    this.clearCanvasRootProperties();
    this.bus.removeEventListener("peitho:navigate", this.onNavigate);
    this.bus.removeEventListener("peitho:timercontrol", this.onTimerControl);
    this.win.removeEventListener("pagehide", this.onPageHide);
  }
  async fetchJson(url) {
    const response = await this.fetchOk(url);
    return response.json();
  }
  async fetchText(url) {
    const response = await this.fetchOk(url);
    return response.text();
  }
  async fetchOk(url) {
    const response = await this.fetcher(url);
    if (!response.ok) throw new Error(`Failed to load ${url}: ${response.status}`);
    return response;
  }
  setCanvasRootProperties(dimensions, cssAspect) {
    this.root.style.setProperty("--peitho-canvas-width", `${dimensions.width}px`);
    this.root.style.setProperty("--peitho-canvas-height", `${dimensions.height}px`);
    this.root.style.setProperty("--peitho-canvas-aspect", cssAspect);
  }
  clearCanvasRootProperties() {
    this.root.style.removeProperty("--peitho-canvas-width");
    this.root.style.removeProperty("--peitho-canvas-height");
    this.root.style.removeProperty("--peitho-canvas-aspect");
  }
  createSlideHost(slide, html, css, dimensions) {
    const host = this.doc.createElement("section");
    host.classList.add("peitho-slide");
    host.dataset.slideKey = slide.key;
    host.dataset.slideIndex = String(slide.index);
    host.dataset.peithoCanvas = "slide";
    host.style.position = "absolute";
    host.style.left = "0";
    host.style.top = "0";
    this.canvasCleanups.push(
      installCanvasScaler({
        window: this.win,
        target: host,
        viewport: this.viewport,
        canvasWidth: dimensions.width,
        canvasHeight: dimensions.height
      })
    );
    const shadow = host.attachShadow({ mode: "open" });
    const style = this.doc.createElement("style");
    style.textContent = css;
    shadow.appendChild(style);
    const template = this.doc.createElement("template");
    template.innerHTML = html;
    shadow.appendChild(template.content.cloneNode(true));
    return host;
  }
  resolveTarget(to) {
    if (to === "first") return 0;
    if (to === "last") return this.slides.length - 1;
    if (to === "next") return this.resolveSequentialTarget(1);
    if (to === "prev") return this.resolveSequentialTarget(-1);
    if ("index" in to) {
      if (to.index < 0 || to.index >= this.slides.length) {
        this.log.error(`Unknown slide index: ${to.index}`);
        return null;
      }
      return to.index;
    }
    const index = this.slides.findIndex((slide) => slide.meta.key === to.key);
    if (index < 0) {
      this.log.error(`Unknown slide key: ${to.key}`);
      return null;
    }
    return index;
  }
  resolveSequentialTarget(direction) {
    return nextNonSkippedIndex(
      this.slides.map((slide) => slide.meta),
      this.currentIndex,
      direction
    );
  }
  show(index) {
    if (index < 0 || index >= this.slides.length) {
      this.log.error(`Unknown slide target: ${index}`);
      return;
    }
    if (index === this.currentIndex) return;
    this.slides.forEach((slide2, slideIndex) => {
      slide2.host.hidden = slideIndex !== index;
    });
    const previousIndex = this.currentIndex < 0 ? null : this.currentIndex;
    this.currentIndex = index;
    const slide = this.slides[index];
    this.bus.dispatchEvent(
      new CustomEvent("peitho:slidechange", {
        detail: {
          key: slide.meta.key,
          index: slide.meta.index,
          total: this.slides.length,
          previousIndex
        }
      })
    );
  }
  startPresentation() {
    if (this.startedAtValue !== null) return;
    this.startedAtValue = this.now();
    this.pausedAtValue = null;
    this.pausedTotalMs = 0;
    this.ended = false;
    this.bus.dispatchEvent(
      new CustomEvent("peitho:presentationstart", {
        detail: { total: this.slides.length, startedAt: this.startedAtValue }
      })
    );
    this.dispatchTimerChange();
  }
  endPresentation() {
    if (this.ended || this.startedAtValue === null) return;
    const endedAt = this.now();
    const elapsedMs = this.elapsedMs();
    this.ended = true;
    this.bus.dispatchEvent(
      new CustomEvent("peitho:presentationend", {
        detail: { endedAt, elapsedMs }
      })
    );
  }
  pauseTimer() {
    if (this.startedAtValue === null || this.pausedAtValue !== null) return;
    this.pausedAtValue = this.now();
    this.dispatchTimerChange();
  }
  resumeTimer() {
    if (this.pausedAtValue === null) return;
    this.pausedTotalMs += this.now() - this.pausedAtValue;
    this.pausedAtValue = null;
    this.dispatchTimerChange();
  }
  resetTimer() {
    this.startedAtValue = null;
    this.pausedAtValue = null;
    this.pausedTotalMs = 0;
    this.ended = false;
    this.dispatchTimerChange();
  }
  dispatchTimerChange() {
    this.bus.dispatchEvent(
      new CustomEvent("peitho:timerchange", {
        detail: {
          running: this.startedAtValue !== null && this.pausedAtValue === null,
          elapsedMs: this.elapsedMs()
        }
      })
    );
  }
  dispatchTimerAdopt(elapsedMs, running, previousElapsedMs) {
    this.bus.dispatchEvent(
      new CustomEvent("peitho:timeradopt", {
        detail: { running, elapsedMs, previousElapsedMs }
      })
    );
  }
};

// src/keyboard.ts
var navigationKeyMap = /* @__PURE__ */ new Map([
  ["ArrowRight", "next"],
  ["PageDown", "next"],
  ["ArrowLeft", "prev"],
  ["PageUp", "prev"],
  ["Home", "first"],
  ["End", "last"]
]);
var keyMap = new Map([...navigationKeyMap, [" ", "next"]]);

// src/swap.ts
var SWAP_ROUTES = Object.freeze({
  "/present.html": Object.freeze({ swapped: false, counterpart: "presenter-swapped" }),
  "/": Object.freeze({ swapped: false, counterpart: "presenter-swapped" }),
  "/presenter": Object.freeze({ swapped: false, counterpart: "present-swapped" }),
  "/presenter.html": Object.freeze({ swapped: false, counterpart: "present-swapped" }),
  "/present-swapped": Object.freeze({ swapped: true, counterpart: "presenter" }),
  "/presenter-swapped": Object.freeze({ swapped: true, counterpart: "present.html" })
});

// src/sync.ts
function isRecord(value) {
  return typeof value === "object" && value !== null;
}
function isCloseSyncMessage(value) {
  return isRecord(value) && value.close === true;
}
function isIndexSyncMessage(value) {
  return isRecord(value) && typeof value.index === "number" && Number.isFinite(value.index);
}
function isSwappedSyncMessage(value) {
  return isRecord(value) && typeof value.swapped === "boolean";
}
function isSyncedSyncMessage(value) {
  return isRecord(value) && value.synced === true;
}
function isSessionChangedSyncMessage(value) {
  return isRecord(value) && value.sessionChanged === true;
}
function isNonNegativeFiniteNumber(value) {
  return typeof value === "number" && Number.isFinite(value) && value >= 0;
}
function isTimerSyncMessage(value) {
  return isRecord(value) && isRecord(value.timer) && typeof value.timer.running === "boolean" && isNonNegativeFiniteNumber(value.timer.elapsedMs);
}
function isTimerReplaySyncMessage(value) {
  return isRecord(value) && isRecord(value.timer) && typeof value.timer.running === "boolean" && isNonNegativeFiniteNumber(value.timer.elapsedMs) && isNonNegativeFiniteNumber(value.timer.atMs) && isNonNegativeFiniteNumber(value.nowMs);
}
function isGenerationSyncMessage(value) {
  return isRecord(value) && typeof value.generation === "number" && Number.isFinite(value.generation);
}
function serverSyncChannelFactory(options = {}) {
  const url = options.url ?? "/sync";
  const fetcher = options.fetcher ?? fetch.bind(globalThis);
  const retryMs = options.retryMs ?? 1e3;
  const setTimeoutFn = options.setTimeoutFn ?? window.setTimeout.bind(window);
  const clearTimeoutFn = options.clearTimeoutFn ?? window.clearTimeout.bind(window);
  const AbortControllerCtor = options.AbortControllerCtor ?? AbortController;
  return () => {
    let onmessage = null;
    let closed = false;
    let seq = 0;
    let synced = false;
    let session = null;
    let highestAckedPostSeq = 0;
    let pendingTimerPosts = 0;
    let bufferedTimerReplay = null;
    let abortController = null;
    let retryTimer = null;
    const flushBufferedTimerReplay = () => {
      if (closed || pendingTimerPosts > 0 || bufferedTimerReplay == null) return;
      const replay = bufferedTimerReplay;
      bufferedTimerReplay = null;
      if (replay.seq >= highestAckedPostSeq) {
        onmessage?.({ data: replay.data });
      }
    };
    const deliverReplayState = (body, options2 = {}) => {
      const skipAbsoluteState = options2.skipAbsoluteState === true;
      const responseSeq = typeof body.seq === "number" && Number.isFinite(body.seq) ? body.seq : 0;
      if (isTimerReplaySyncMessage(body)) {
        if (skipAbsoluteState) {
          bufferedTimerReplay = null;
        } else if (options2.deferTimerReplay === true) {
          bufferedTimerReplay = {
            seq: responseSeq,
            data: { timer: body.timer, nowMs: body.nowMs }
          };
        } else {
          onmessage?.({ data: { timer: body.timer, nowMs: body.nowMs } });
        }
      }
      if (!skipAbsoluteState && isIndexSyncMessage(body)) {
        onmessage?.({ data: { index: body.index } });
      }
      if (!skipAbsoluteState && isSwappedSyncMessage(body)) {
        onmessage?.({ data: { swapped: body.swapped } });
      }
      if (isGenerationSyncMessage(body)) {
        onmessage?.({ data: { generation: body.generation } });
      }
    };
    const delay = () => new Promise((resolve) => {
      retryTimer = setTimeoutFn(() => {
        retryTimer = null;
        resolve();
      }, retryMs);
    });
    const handshake = async () => {
      try {
        const response = await fetcher(url);
        if (closed) return false;
        if (!response.ok) {
          console.error(`Failed to start sync polling: ${response.status}`);
          await delay();
          return false;
        }
        const body = await response.json();
        if (typeof body.seq !== "number") {
          console.error("Invalid peitho sync handshake");
          await delay();
          return false;
        }
        if (typeof body.session === "string") {
          if (session === null) {
            session = body.session;
          } else if (body.session !== session) {
            session = body.session;
            onmessage?.({ data: { sessionChanged: true } });
          }
        }
        seq = body.seq;
        deliverReplayState(body, {
          skipAbsoluteState: body.seq < highestAckedPostSeq,
          deferTimerReplay: pendingTimerPosts > 0
        });
        if (!synced) {
          synced = true;
          onmessage?.({ data: { synced: true } });
        }
        return true;
      } catch (error) {
        if (!closed) {
          console.error(`Failed to start sync polling: ${String(error)}`);
          await delay();
        }
        return false;
      }
    };
    const poll = async () => {
      let needsHandshake = true;
      while (!closed) {
        while (!closed && needsHandshake && !await handshake()) {
          continue;
        }
        if (closed) return;
        needsHandshake = false;
        abortController = new AbortControllerCtor();
        try {
          const response = await fetcher(`${url}?seq=${seq}`, {
            signal: abortController.signal
          });
          if (closed) return;
          if (response.status === 204) continue;
          if (!response.ok) {
            console.error(`Failed to poll sync message: ${response.status}`);
            await delay();
            continue;
          }
          const body = await response.json();
          if (typeof body.seq !== "number" || !("message" in body)) {
            console.error("Invalid peitho server sync message");
            await delay();
            continue;
          }
          seq = body.seq;
          if (body.message != null) {
            onmessage?.({ data: body.message });
          }
          deliverReplayState(body, {
            skipAbsoluteState: body.seq < highestAckedPostSeq,
            deferTimerReplay: pendingTimerPosts > 0
          });
        } catch (error) {
          if (!closed) {
            console.error(`Failed to poll sync message: ${String(error)}`);
            needsHandshake = true;
            await delay();
          }
        }
      }
    };
    void poll();
    return {
      get onmessage() {
        return onmessage;
      },
      set onmessage(next) {
        onmessage = next;
      },
      postMessage(message) {
        const isTimerPost = isTimerSyncMessage(message);
        if (isTimerPost) pendingTimerPosts += 1;
        const completeTimerPost = () => {
          if (!isTimerPost) return;
          pendingTimerPosts = Math.max(0, pendingTimerPosts - 1);
          flushBufferedTimerReplay();
        };
        let request;
        try {
          request = fetcher(url, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify(message),
            keepalive: true
          });
        } catch (error) {
          completeTimerPost();
          console.error(`Failed to post sync message: ${String(error)}`);
          return;
        }
        void request.then(async (response) => {
          if (!response.ok) {
            console.error(`Failed to post sync message: ${response.status}`);
            return;
          }
          try {
            const body = await response.json();
            if (typeof body.seq === "number" && Number.isFinite(body.seq)) {
              highestAckedPostSeq = Math.max(highestAckedPostSeq, body.seq);
            }
          } catch (_error) {
          }
        }).catch((error) => {
          console.error(`Failed to post sync message: ${String(error)}`);
        }).finally(() => {
          completeTimerPost();
        });
      },
      close() {
        closed = true;
        abortController?.abort();
        if (retryTimer !== null) {
          clearTimeoutFn(retryTimer);
          retryTimer = null;
        }
      }
    };
  };
}

// src/timeTracker.ts
var clamp01 = (ratio) => Math.min(Math.max(ratio, 0), 1);
function isOverrun(elapsedMs, plannedDurationMs) {
  return elapsedMs > plannedDurationMs;
}
function isValidDurationMs(ms) {
  return Number.isSafeInteger(ms) && ms > 0;
}
function formatMinuteSeconds(ms) {
  const totalSeconds = Math.round(ms / 1e3);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = (totalSeconds % 60).toString().padStart(2, "0");
  return `${minutes}:${seconds}`;
}

// src/remote.ts
var isReadOnly = (state) => state.kind === "ended";
var canInteract = (state) => state.kind === "active";
async function mountRemoteView(options) {
  const view = new RemoteController(options);
  await view.load();
  return view;
}
function installRemoteControls(options) {
  const doc = options.document ?? document;
  const bus = options.bus ?? window;
  const root = options.root;
  root.classList.remove("peitho-remote-error");
  root.replaceChildren();
  const container = doc.createElement("section");
  container.className = "peitho-remote";
  container.dataset.peithoEnded = "false";
  const preview = createDimmableRow(doc, "div", "peitho-remote-preview");
  preview.dataset.peithoRemote = "preview";
  const titlebar = createDimmableRow(doc, "div", "peitho-remote-titlebar");
  const title = doc.createElement("div");
  title.className = "peitho-remote-title";
  title.dataset.peithoRemote = "title";
  title.textContent = "Loading";
  const counter = doc.createElement("div");
  counter.className = "peitho-remote-counter";
  counter.dataset.peithoRemote = "counter";
  counter.textContent = "\u2013 / \u2013";
  titlebar.append(title, counter);
  const chase = createDimmableRow(doc, "div", "peitho-remote-chase");
  chase.dataset.peithoRemote = "chase";
  chase.dataset.peithoChase = "slide";
  const chaseTrack = doc.createElement("div");
  chaseTrack.className = "peitho-remote-chase-track";
  chaseTrack.dataset.peithoRemote = "chase-track";
  const chaseFill = doc.createElement("div");
  chaseFill.className = "peitho-remote-chase-fill";
  chaseFill.dataset.peithoRemote = "chase-fill";
  chaseTrack.append(chaseFill);
  const rabbit = doc.createElement("span");
  rabbit.className = "peitho-remote-chase-marker";
  rabbit.dataset.peithoRemote = "marker-rabbit";
  rabbit.setAttribute("aria-label", "slide progress");
  rabbit.textContent = "\u{1F430}";
  const turtle = doc.createElement("span");
  turtle.className = "peitho-remote-chase-marker";
  turtle.dataset.peithoRemote = "marker-turtle";
  turtle.setAttribute("aria-label", "time progress");
  turtle.textContent = "\u{1F422}";
  chase.append(chaseTrack, rabbit, turtle);
  const pace = createDimmableRow(doc, "div", "peitho-remote-pace");
  const timerButton = doc.createElement("button");
  timerButton.type = "button";
  timerButton.className = "peitho-remote-timer-button";
  timerButton.dataset.peithoAction = "timer";
  timerButton.dataset.peithoRunning = "false";
  timerButton.dataset.peithoTimerAction = "start";
  timerButton.disabled = true;
  timerButton.setAttribute("aria-label", "Start timer");
  const timerIcon = doc.createElement("span");
  timerIcon.className = "peitho-remote-timer-icon";
  timerIcon.dataset.peithoIcon = "play";
  timerButton.append(timerIcon);
  const resetButton = doc.createElement("button");
  resetButton.type = "button";
  resetButton.className = "peitho-remote-reset-button";
  resetButton.dataset.peithoAction = "timer-reset";
  resetButton.disabled = true;
  resetButton.setAttribute("aria-label", "Reset timer");
  resetButton.textContent = "\u21BA";
  const elapsedRow = doc.createElement("div");
  elapsedRow.className = "peitho-remote-elapsed-row";
  elapsedRow.dataset.peithoRemote = "elapsed-row";
  const elapsed = doc.createElement("span");
  elapsed.className = "peitho-remote-elapsed";
  elapsed.dataset.peithoRemote = "elapsed";
  elapsed.textContent = "0:00";
  const separator = doc.createElement("span");
  separator.className = "peitho-remote-time-separator";
  separator.dataset.peithoRemote = "time-separator";
  separator.textContent = "/";
  separator.hidden = true;
  const planned = doc.createElement("span");
  planned.className = "peitho-remote-planned";
  planned.dataset.peithoRemote = "planned";
  planned.hidden = true;
  elapsedRow.append(elapsed, separator, planned);
  const delta = doc.createElement("span");
  delta.className = "peitho-remote-pace-delta";
  delta.dataset.peithoRemote = "pace-delta";
  delta.hidden = true;
  pace.append(timerButton, resetButton, elapsedRow, delta);
  const notesPanel = createDimmableRow(doc, "section", "peitho-remote-notes");
  const notesCaption = doc.createElement("div");
  notesCaption.className = "peitho-remote-notes-caption";
  notesCaption.textContent = "NOTES";
  const notesBody = doc.createElement("div");
  notesBody.className = "peitho-remote-notes-body";
  notesBody.dataset.peithoRemote = "notes";
  notesPanel.append(notesCaption, notesBody);
  const actions = doc.createElement("div");
  actions.className = "peitho-remote-actions";
  const prev = remoteButton(doc, "prev", "Previous");
  const next = remoteButton(doc, "next", "Next");
  actions.append(prev, next);
  const onPrev = () => dispatchNavigate(bus, "prev");
  const onNext = () => dispatchNavigate(bus, "next");
  const onTimer = () => {
    const action = timerButton.dataset.peithoTimerAction;
    if (action === "start" || action === "pause" || action === "resume") {
      dispatchTimerControl(bus, action);
    }
  };
  const onReset = () => dispatchTimerControl(bus, "reset");
  prev.addEventListener("click", onPrev);
  next.addEventListener("click", onNext);
  timerButton.addEventListener("click", onTimer);
  resetButton.addEventListener("click", onReset);
  const rows = [
    { kind: "dimmable", element: preview },
    { kind: "dimmable", element: titlebar },
    { kind: "dimmable", element: chase },
    { kind: "dimmable", element: pace },
    { kind: "dimmable", element: notesPanel },
    { kind: "actions", element: actions }
  ];
  container.append(...rows.map((row) => row.element));
  root.append(container);
  return () => {
    prev.removeEventListener("click", onPrev);
    next.removeEventListener("click", onNext);
    timerButton.removeEventListener("click", onTimer);
    resetButton.removeEventListener("click", onReset);
    container.remove();
  };
}
function createDimmableRow(doc, tag, ...classNames) {
  const el = doc.createElement(tag);
  el.classList.add("peitho-remote-dim-on-end", ...classNames);
  return el;
}
function installRemoteSyncBridge(options) {
  const bus = options.bus ?? window;
  const log = options.console ?? console;
  const now = options.now ?? Date.now;
  const channel = (options.channelFactory ?? serverSyncChannelFactory())("peitho-sync");
  let synced = false;
  const onNavigate = (event) => {
    if (!synced) return;
    const to = event.detail?.to;
    if (to !== "next" && to !== "prev") return;
    const target = resolveRemoteTarget(options.slides, options.getCurrentIndex(), to);
    if (target === null) return;
    channel.postMessage({ index: target });
    options.setCurrentIndex(target);
  };
  const onTimerControl = (event) => {
    if (!synced) return;
    const action = event.detail?.action;
    if (action !== "start" && action !== "pause" && action !== "resume" && action !== "reset") {
      log.error("Invalid peitho:timercontrol event");
      return;
    }
    const next = nextTimerStateForAction(action, options.getTimerState(), now());
    if (next === null) return;
    channel.postMessage({
      timer: { running: next.running, elapsedMs: Math.round(next.elapsedMs) }
    });
    options.setTimerState(next);
  };
  channel.onmessage = (event) => {
    const data = event.data;
    if (isSyncedSyncMessage(data)) {
      synced = true;
      options.setSynced();
      return;
    }
    if (isCloseSyncMessage(data)) {
      options.setEnded();
      return;
    }
    if (isIndexSyncMessage(data)) {
      options.setCurrentIndex(data.index);
      return;
    }
    if (isTimerReplaySyncMessage(data)) {
      const serverAdvance = data.timer.running ? Math.max(0, data.nowMs - data.timer.atMs) : 0;
      options.setTimerState({
        running: data.timer.running,
        elapsedMs: data.timer.elapsedMs + serverAdvance,
        receivedAtMs: now()
      });
      return;
    }
    if (isSessionChangedSyncMessage(data)) {
      options.onSessionChange();
      return;
    }
    if (isSwappedSyncMessage(data) || isGenerationSyncMessage(data) || isTimerSyncMessage(data)) {
      return;
    }
    log.error("Invalid peitho remote sync message");
  };
  bus.addEventListener("peitho:navigate", onNavigate);
  bus.addEventListener("peitho:timercontrol", onTimerControl);
  return () => {
    bus.removeEventListener("peitho:navigate", onNavigate);
    bus.removeEventListener("peitho:timercontrol", onTimerControl);
    channel.onmessage = null;
    channel.close();
  };
}
var RemoteController = class {
  manifest = null;
  currentIndex = null;
  root;
  manifestUrl;
  notesUrl;
  fetcher;
  channelFactory;
  mountPresentShell;
  win;
  doc;
  bus;
  previewBus = new EventTarget();
  log;
  now;
  reload;
  state = { kind: "loading" };
  notes = { version: 1, notes: {} };
  renderedNotesValue = null;
  slides = [];
  timerState = null;
  controlsCleanup = null;
  syncCleanup = null;
  previewShell = null;
  timerInterval = null;
  constructor(options) {
    this.root = options.root;
    this.manifestUrl = options.manifestUrl ?? "manifest.json";
    this.notesUrl = options.notesUrl ?? "notes.json";
    this.fetcher = options.fetcher ?? fetch.bind(globalThis);
    this.channelFactory = options.syncChannelFactory ?? options.channelFactory;
    this.mountPresentShell = options.mountPresentShell ?? mountPresentShell;
    this.win = options.window ?? window;
    this.doc = options.document ?? document;
    this.bus = options.bus ?? this.win;
    this.log = options.console ?? console;
    this.now = options.now ?? Date.now;
    this.reload = options.reload ?? (() => this.win.location.reload());
  }
  async load() {
    try {
      const manifest = await this.fetchJson(this.manifestUrl);
      this.manifest = manifest;
      this.notes = await this.fetchNotes();
      this.slides = manifest.slides.map((slide) => ({
        key: slide.key,
        skip: slide.skip === true,
        title: slide.text.title
      }));
      this.currentIndex = initialSlideIndex(this.slides);
      this.controlsCleanup = installRemoteControls({
        root: this.root,
        document: this.doc,
        bus: this.bus
      });
      const previewRoot = this.root.querySelector('[data-peitho-remote="preview"]');
      if (previewRoot != null) {
        this.previewShell = await this.mountPresentShell({
          root: previewRoot,
          fetcher: this.fetcher,
          window: this.win,
          document: this.doc,
          bus: this.previewBus,
          manifest,
          now: this.now,
          viewport: paneViewport(previewRoot)
        });
      }
      this.render();
      this.syncCleanup = installRemoteSyncBridge({
        slides: this.slides,
        channelFactory: this.channelFactory,
        bus: this.bus,
        now: this.now,
        getCurrentIndex: () => this.currentIndex,
        setCurrentIndex: (index) => this.setCurrentIndex(index),
        getTimerState: () => this.timerState,
        setTimerState: (state) => this.setTimerState(state),
        setSynced: () => this.setSynced(),
        setEnded: () => this.setEnded(),
        onSessionChange: () => this.reload(),
        console: this.log
      });
    } catch (error) {
      this.showError(error instanceof Error ? error.message : String(error));
    }
  }
  destroy() {
    this.clearTimerInterval();
    this.syncCleanup?.();
    this.syncCleanup = null;
    this.previewShell?.destroy();
    this.previewShell = null;
    this.controlsCleanup?.();
    this.controlsCleanup = null;
  }
  async fetchJson(url) {
    const response = await this.fetcher(url);
    if (!response.ok) throw new Error(`Failed to load ${url}: ${response.status}`);
    return response.json();
  }
  async fetchNotes() {
    try {
      return await this.fetchJson(this.notesUrl);
    } catch (error) {
      this.log.error(
        `Failed to load ${this.notesUrl}: ${error instanceof Error ? error.message : String(error)}`
      );
      return { version: 1, notes: {} };
    }
  }
  setCurrentIndex(index) {
    this.currentIndex = clampIndex(index, this.slides.length);
    this.render();
  }
  setTimerState(state) {
    this.timerState = {
      running: state.running,
      elapsedMs: Math.max(0, state.elapsedMs),
      receivedAtMs: state.receivedAtMs
    };
    this.render();
  }
  setSynced() {
    if (this.state.kind !== "loading") return;
    this.state = { kind: "active", synced: true };
    this.render();
  }
  setEnded() {
    this.state = { kind: "ended" };
    this.clearTimerInterval();
    this.render();
  }
  render() {
    const manifest = this.manifest;
    if (manifest == null) return;
    const container = this.root.querySelector(".peitho-remote");
    if (container == null) return;
    container.dataset.peithoEnded = isReadOnly(this.state) ? "true" : "false";
    const currentIndex = this.currentIndex;
    const slide = currentIndex == null ? null : manifest.slides[currentIndex];
    const total = this.slides.length;
    setText(this.root, "title", slideTitle(slide?.text.title));
    setText(this.root, "counter", currentIndex == null ? `\u2013 / ${total}` : `${currentIndex + 1} / ${total}`);
    this.renderChase(manifest, currentIndex);
    this.renderPaceStatic(manifest);
    this.renderTimeDependentChrome(manifest, currentIndex);
    this.renderSection(manifest, currentIndex);
    this.renderNotes(slide?.key);
    this.renderButtons(currentIndex);
    this.syncPreview(currentIndex);
    this.updateTimerInterval();
  }
  renderChase(manifest, currentIndex) {
    const rabbit = this.root.querySelector('[data-peitho-remote="marker-rabbit"]');
    if (rabbit == null) return;
    const plannedDurationMs = validPlannedDurationMs(manifest);
    const planned = plannedDurationMs != null;
    rabbit.hidden = !planned;
    if (planned) setChaseMarker(rabbit, slideFraction(manifest, currentIndex));
  }
  renderPaceStatic(manifest) {
    const elapsedRow = this.root.querySelector('[data-peitho-remote="elapsed-row"]');
    if (elapsedRow == null) return;
    const separator = elapsedRow.querySelector('[data-peitho-remote="time-separator"]');
    const planned = elapsedRow.querySelector('[data-peitho-remote="planned"]');
    const plannedDurationMs = validPlannedDurationMs(manifest);
    if (separator != null) separator.hidden = plannedDurationMs == null;
    if (planned != null) {
      planned.hidden = plannedDurationMs == null;
      planned.textContent = plannedDurationMs == null ? "" : formatMinuteSeconds(plannedDurationMs);
    }
  }
  renderTimeDependentChrome(manifest, currentIndex) {
    const timerButton = this.root.querySelector('[data-peitho-action="timer"]');
    const resetButton = this.root.querySelector(
      '[data-peitho-action="timer-reset"]'
    );
    const elapsed = this.root.querySelector('[data-peitho-remote="elapsed"]');
    if (timerButton == null || resetButton == null || elapsed == null) return;
    const elapsedMs = this.currentElapsedMs();
    const state = timerVisualState(this.timerState, elapsedMs);
    timerButton.disabled = !canInteract(this.state);
    resetButton.disabled = !canInteract(this.state) || state === "stopped";
    timerButton.dataset.peithoRunning = state === "running" ? "true" : "false";
    timerButton.dataset.peithoTimerAction = playpauseActionFor(state);
    timerButton.setAttribute("aria-label", timerAriaLabel(state));
    const icon = timerButton.querySelector(".peitho-remote-timer-icon");
    if (icon != null) icon.dataset.peithoIcon = state === "running" ? "pause" : "play";
    elapsed.textContent = formatMinuteSeconds(elapsedMs);
    this.updateChaseTime(manifest, currentIndex, elapsedMs, state);
  }
  updateChaseTime(manifest, currentIndex, elapsedMs, state) {
    const chase = this.root.querySelector('[data-peitho-remote="chase"]');
    const fill = this.root.querySelector('[data-peitho-remote="chase-fill"]');
    const rabbit = this.root.querySelector('[data-peitho-remote="marker-rabbit"]');
    const turtle = this.root.querySelector('[data-peitho-remote="marker-turtle"]');
    const delta = this.root.querySelector('[data-peitho-remote="pace-delta"]');
    if (chase == null || fill == null || rabbit == null || turtle == null || delta == null) return;
    const plannedDurationMs = validPlannedDurationMs(manifest);
    if (plannedDurationMs == null) {
      chase.dataset.peithoChase = "slide";
      chase.classList.remove("peitho-remote-chase-overrun");
      rabbit.hidden = true;
      turtle.hidden = true;
      delta.hidden = true;
      delta.textContent = "";
      delete delta.dataset.peithoPace;
      setChaseFill(fill, slideFraction(manifest, currentIndex));
      return;
    }
    chase.dataset.peithoChase = "time";
    rabbit.hidden = false;
    turtle.hidden = false;
    const overrun = isOverrun(elapsedMs, plannedDurationMs);
    chase.classList.toggle("peitho-remote-chase-overrun", overrun);
    const turtleFraction = clamp01(elapsedMs / plannedDurationMs);
    setChaseMarker(turtle, turtleFraction);
    setChaseFill(fill, turtleFraction);
    if (currentIndex == null) {
      delta.hidden = true;
      delta.textContent = "";
      delete delta.dataset.peithoPace;
      return;
    }
    const paceState = remotePaceState(manifest, currentIndex, elapsedMs, state === "running");
    if (paceState == null) {
      delta.hidden = true;
      delta.textContent = "";
      delete delta.dataset.peithoPace;
      return;
    }
    delta.hidden = false;
    delta.dataset.peithoPace = paceState.kind;
    delta.textContent = paceState.label;
  }
  renderSection(manifest, currentIndex) {
    const existing = this.root.querySelector('[data-peitho-remote="section"]');
    if (currentIndex == null || manifest.sections.length === 0) {
      existing?.remove();
      return;
    }
    const sectionIndex = sectionIndexForSlide(manifest.sections, currentIndex);
    if (sectionIndex < 0) {
      existing?.remove();
      return;
    }
    const section = manifest.sections[sectionIndex];
    const sectionSlideCount = section.endIndex - section.startIndex + 1;
    const sectionOffset = currentIndex - section.startIndex + 1;
    const sectionLine = existing ?? createDimmableRow(this.doc, "div", "peitho-remote-section");
    sectionLine.dataset.peithoRemote = "section";
    const name = this.doc.createElement("b");
    name.textContent = section.name;
    sectionLine.replaceChildren(
      name,
      this.doc.createTextNode(` \xB7 slide ${sectionOffset} / ${sectionSlideCount} in section`)
    );
    if (existing == null) {
      const notes = this.root.querySelector(".peitho-remote-notes");
      notes?.before(sectionLine);
    }
  }
  renderNotes(slideKey) {
    const notes = this.root.querySelector('[data-peitho-remote="notes"]');
    if (notes == null) return;
    const value = slideKey == null ? null : this.notes.notes[slideKey];
    if (value == null || value.length === 0) {
      this.setNotesText(notes, "No notes for this slide");
      notes.dataset.peithoEmpty = "true";
      return;
    }
    this.setNotesText(notes, value);
    notes.dataset.peithoEmpty = "false";
  }
  setNotesText(notes, value) {
    if (this.renderedNotesValue === value) return;
    notes.textContent = value;
    this.renderedNotesValue = value;
  }
  renderButtons(currentIndex) {
    const prev = this.root.querySelector('[data-peitho-action="prev"]');
    const next = this.root.querySelector('[data-peitho-action="next"]');
    if (prev == null || next == null) return;
    prev.disabled = !canInteract(this.state) || resolveRemoteTarget(this.slides, currentIndex, "prev") === null;
    next.disabled = !canInteract(this.state) || resolveRemoteTarget(this.slides, currentIndex, "next") === null;
  }
  syncPreview(currentIndex) {
    if (currentIndex == null) return;
    this.previewBus.dispatchEvent(
      new CustomEvent("peitho:navigate", { detail: { to: { index: currentIndex } } })
    );
  }
  currentElapsedMs() {
    return currentTimerElapsedMs(this.timerState, this.now());
  }
  updateTimerInterval() {
    if (isReadOnly(this.state) || this.timerState?.running !== true) {
      this.clearTimerInterval();
      return;
    }
    if (this.timerInterval != null) return;
    this.timerInterval = this.win.setInterval(() => {
      const manifest = this.manifest;
      if (manifest == null) return;
      this.renderTimeDependentChrome(manifest, this.currentIndex);
    }, 1e3);
  }
  clearTimerInterval() {
    if (this.timerInterval == null) return;
    this.win.clearInterval(this.timerInterval);
    this.timerInterval = null;
  }
  showError(message) {
    this.destroy();
    this.root.replaceChildren();
    this.root.className = "peitho-remote-error";
    this.root.textContent = message;
  }
};
function remoteButton(doc, action, label) {
  const button = doc.createElement("button");
  button.type = "button";
  button.disabled = true;
  button.dataset.peithoAction = action;
  button.dataset.peithoDirection = action;
  const arrow = doc.createElement("span");
  arrow.className = "peitho-remote-action-arrow";
  arrow.setAttribute("aria-hidden", "true");
  arrow.textContent = action === "prev" ? "\u2039" : "\u203A";
  const labelSpan = doc.createElement("span");
  labelSpan.className = "peitho-remote-action-label";
  labelSpan.textContent = label;
  if (action === "prev") {
    button.append(arrow, labelSpan);
  } else {
    button.append(labelSpan, arrow);
  }
  return button;
}
function dispatchNavigate(bus, to) {
  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to } }));
}
function dispatchTimerControl(bus, action) {
  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action } }));
}
function resolveRemoteTarget(slides, currentIndex, to) {
  const base = currentIndex ?? initialSlideIndex(slides);
  if (base === null) return null;
  return nextNonSkippedIndex(slides, base, to === "next" ? 1 : -1);
}
function clampIndex(index, total) {
  if (total === 0) return null;
  return Math.max(0, Math.min(Math.trunc(index), total - 1));
}
function slideFraction(manifest, currentIndex) {
  if (currentIndex == null) return 0;
  if (manifest.slideCount <= 1) return 1;
  return clamp01(currentIndex / (manifest.slideCount - 1));
}
function chasePercent(ratio) {
  return Math.round(clamp01(ratio) * 1e4) / 100;
}
function setChaseMarker(element, ratio) {
  const percent = chasePercent(ratio);
  element.style.left = `${percent}%`;
  element.style.transform = `translateX(${-percent}%)`;
}
function setChaseFill(element, ratio) {
  element.style.width = `${chasePercent(ratio)}%`;
}
function setText(root, key, value) {
  const element = root.querySelector(`[data-peitho-remote="${key}"]`);
  if (element != null) element.textContent = value;
}
function slideTitle(title) {
  return title == null || title.length === 0 ? "Untitled slide" : title;
}
function validPlannedDurationMs(manifest) {
  const plannedDurationMs = manifest.plannedDurationMs;
  return plannedDurationMs != null && isValidDurationMs(plannedDurationMs) ? plannedDurationMs : null;
}
function expectedElapsedAtSlide(manifest, index) {
  const plannedDurationMs = validPlannedDurationMs(manifest);
  if (plannedDurationMs == null) return null;
  const slideCount = Math.max(1, manifest.slideCount);
  const requestedIndex = Number.isFinite(index) ? Math.trunc(index) : 0;
  if (requestedIndex <= 0) return 0;
  if (requestedIndex >= slideCount) return plannedDurationMs;
  if (manifest.sections.length === 0) {
    return plannedDurationMs * requestedIndex / slideCount;
  }
  const sectionIndex = sectionIndexForSlide(manifest.sections, requestedIndex);
  if (sectionIndex < 0) return plannedDurationMs * requestedIndex / slideCount;
  let elapsed = 0;
  for (let i = 0; i < sectionIndex; i += 1) {
    elapsed += manifest.sections[i].plannedDurationMs;
  }
  const section = manifest.sections[sectionIndex];
  const sectionSlideCount = section.endIndex - section.startIndex + 1;
  return elapsed + section.plannedDurationMs * ((requestedIndex - section.startIndex) / sectionSlideCount);
}
function remotePaceState(manifest, index, elapsedMs, running) {
  const expectedStart = expectedElapsedAtSlide(manifest, index);
  const expectedEnd = expectedElapsedAtSlide(manifest, index + 1);
  if (expectedStart == null || expectedEnd == null) return null;
  if (!running) {
    return elapsedMs > 0 ? { kind: "paused", label: "Paused" } : null;
  }
  const plannedDurationMs = validPlannedDurationMs(manifest);
  if (plannedDurationMs != null && isOverrun(elapsedMs, plannedDurationMs)) {
    return {
      kind: "overrun",
      label: `+${formatMinuteSeconds(elapsedMs - plannedDurationMs)} over`
    };
  }
  if (elapsedMs < expectedStart) {
    return {
      kind: "ahead",
      label: `${formatMinuteSeconds(expectedStart - elapsedMs)} ahead`
    };
  }
  if (elapsedMs <= expectedEnd) {
    return { kind: "onpace", label: "on pace" };
  }
  return {
    kind: "behind",
    label: `${formatMinuteSeconds(elapsedMs - expectedEnd)} behind`
  };
}
function currentTimerElapsedMs(timer, now) {
  if (timer == null) return 0;
  return Math.max(0, timer.elapsedMs + (timer.running ? now - timer.receivedAtMs : 0));
}
function timerVisualState(timer, elapsedMs) {
  if (timer == null || !timer.running && elapsedMs === 0) return "stopped";
  return timer.running ? "running" : "paused";
}
function playpauseActionFor(state) {
  if (state === "running") return "pause";
  if (state === "paused") return "resume";
  return "start";
}
function timerAriaLabel(state) {
  if (state === "running") return "Pause timer";
  if (state === "paused") return "Resume timer";
  return "Start timer";
}
function nextTimerStateForAction(action, current, now) {
  const elapsedMs = currentTimerElapsedMs(current, now);
  const state = timerVisualState(current, elapsedMs);
  if (action === "start") {
    if (state !== "stopped") return null;
    return { running: true, elapsedMs: 0, receivedAtMs: now };
  }
  if (action === "pause") {
    if (state !== "running") return null;
    return { running: false, elapsedMs, receivedAtMs: now };
  }
  if (action === "resume") {
    if (state !== "paused") return null;
    return { running: true, elapsedMs, receivedAtMs: now };
  }
  return { running: false, elapsedMs: 0, receivedAtMs: now };
}
function paneViewport(pane) {
  return () => ({
    width: pane.clientWidth,
    height: pane.clientHeight
  });
}
export {
  createDimmableRow,
  expectedElapsedAtSlide,
  installRemoteControls,
  installRemoteSyncBridge,
  mountPresentShell,
  mountRemoteView,
  remotePaceState,
  serverSyncChannelFactory
};
//# sourceMappingURL=remote.js.map
