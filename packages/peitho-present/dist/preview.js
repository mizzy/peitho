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

// src/clickNavigationGuard.ts
var DEFAULT_MOVE_THRESHOLD_PX = 5;
function createClickNavigationGuard(options) {
  const win = options.window ?? window;
  const moveThresholdPx = options.moveThresholdPx ?? DEFAULT_MOVE_THRESHOLD_PX;
  let clickStart = null;
  const onMouseDown = (event) => {
    clickStart = { x: event.clientX, y: event.clientY };
  };
  options.target.addEventListener("mousedown", onMouseDown);
  return {
    shouldIgnoreClick(event) {
      const start = clickStart;
      clickStart = null;
      const origin = event.composedPath()[0];
      if (origin instanceof Element && origin.closest("a") !== null) return true;
      if (hasNonCollapsedSelection(win)) return true;
      if (start === null) return false;
      return Math.hypot(event.clientX - start.x, event.clientY - start.y) > moveThresholdPx;
    },
    destroy() {
      options.target.removeEventListener("mousedown", onMouseDown);
    }
  };
}
function hasNonCollapsedSelection(win) {
  const selection = win.getSelection();
  return selection !== null && !selection.isCollapsed;
}

// src/fontscope.ts
var FONT_SCOPE_ATTRIBUTE = "data-peitho-font-scope";
var FONT_SCOPE_SELECTOR = `style[${FONT_SCOPE_ATTRIBUTE}]`;
var fontScopeStates = /* @__PURE__ */ new WeakMap();
function extractFontScopeCss(css) {
  return [
    ...extractLeadingImports(css),
    ...extractTopLevelFontFaces(css).map(forceFontDisplayBlock)
  ].join("\n");
}
function forceFontDisplayBlock(block) {
  const stripped = block.replace(/font-display\s*:[^;}]*;?/gi, "");
  const close = stripped.lastIndexOf("}");
  if (close === -1) return block;
  return `${stripped.slice(0, close)}font-display:block;${stripped.slice(close)}`;
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

// src/fontsReady.ts
function deckText(htmlSources) {
  const characters = /* @__PURE__ */ new Set();
  for (const html of htmlSources) {
    for (const char of html.replace(/<[^>]*>/g, " ")) {
      if (char > " ") characters.add(char);
    }
  }
  return [...characters].join("");
}
var MAX_FONT_READY_PASSES = 5;
async function waitForFontsReady(doc, win, options) {
  const fonts = doc.fonts;
  if (fonts == null) return;
  const timeoutMs = options?.timeoutMs ?? 3e3;
  const deadline = Date.now() + timeoutMs;
  const kicked = /* @__PURE__ */ new Set();
  for (let pass = 0; pass < MAX_FONT_READY_PASSES; pass += 1) {
    const hasNewFace = kickVisibleFontFaces(fonts, kicked, options?.text);
    if (!hasNewFace && pass > 0) return;
    const remainingMs = deadline - Date.now();
    if (remainingMs <= 0 || await raceReadyWithTimeout(fonts, win, remainingMs)) {
      const log = options?.log ?? console;
      log.warn(`document.fonts.ready timed out after ${timeoutMs}ms`);
      return;
    }
  }
}
function kickVisibleFontFaces(fonts, kicked, text) {
  if (text === void 0 || text === "") return false;
  let hasNewFamily = false;
  fonts.forEach((face) => {
    const key = `${face.family}\0${face.weight}\0${face.style}`;
    if (kicked.has(key)) return;
    kicked.add(key);
    hasNewFamily = true;
    try {
      fonts.load(`${face.style} ${face.weight} 1em ${face.family}`, text).catch(() => void 0);
    } catch {
    }
  });
  return hasNewFamily;
}
async function raceReadyWithTimeout(fonts, win, ms) {
  let timeoutId;
  const timeout = new Promise((resolve) => {
    timeoutId = win.setTimeout(() => resolve("timeout"), ms);
  });
  const ready = fonts.ready.then(
    () => "ready",
    () => "ready"
  );
  const result = await Promise.race([ready, timeout]);
  if (timeoutId !== void 0) win.clearTimeout(timeoutId);
  return result === "timeout";
}

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
function hasChordModifier(event) {
  return event.metaKey || event.ctrlKey || event.altKey;
}
function isComposingKey(event) {
  return event.isComposing || event.keyCode === 229;
}

// src/previewHttp.ts
var inflightKeepaliveBytes = 0;
function postJson(fetcher, url, payload, keepalive) {
  const body = JSON.stringify(payload);
  const bodyBytes = new TextEncoder().encode(body).length;
  const useKeepalive = keepalive && inflightKeepaliveBytes + bodyBytes <= 6e4;
  const init = {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body,
    keepalive: useKeepalive
  };
  if (!useKeepalive) return fetcher(url, init);
  let charged = false;
  let released = false;
  const release = () => {
    if (!charged || released) return;
    released = true;
    inflightKeepaliveBytes -= bodyBytes;
  };
  try {
    const request = fetcher(url, init);
    inflightKeepaliveBytes += bodyBytes;
    charged = true;
    return request.finally(release);
  } catch (error) {
    release();
    throw error;
  }
}
async function readErrorResponse(response, fallbackLabel) {
  const body = await response.text();
  try {
    const error = JSON.parse(body).error;
    if (typeof error === "string") return error;
  } catch {
  }
  return fallbackLabel === void 0 ? body : `${fallbackLabel} failed (HTTP ${response.status})`;
}

// src/previewSourceEdit.ts
var INVALID_RESPONSE_MESSAGE = "slide source save returned an invalid response";
function openPreviewSourceEdit(options) {
  const textarea = options.document.createElement("textarea");
  textarea.dataset.peithoPreview = "source";
  textarea.setAttribute("aria-label", "Slide Markdown source");
  textarea.spellcheck = false;
  textarea.wrap = "off";
  textarea.value = options.body;
  textarea.style.position = "absolute";
  textarea.style.boxSizing = "border-box";
  textarea.style.margin = "0";
  textarea.style.resize = "none";
  textarea.style.fontFamily = "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace";
  textarea.style.tabSize = "2";
  textarea.style.whiteSpace = "pre";
  textarea.style.overflow = "auto";
  let closed = false;
  let commitPromise = null;
  let committingBody = null;
  let sentForPageHide = false;
  const onKeyDown = (event) => {
    if (isComposingKey(event)) return;
    if (event.key === "Enter") {
      event.stopPropagation();
      if (!event.metaKey && !event.ctrlKey) return;
      event.preventDefault();
      if (commitPromise === null) options.onCommitRequest();
      return;
    }
    if (event.key !== "Escape") return;
    event.preventDefault();
    event.stopPropagation();
    if (commitPromise === null) options.onCancelRequest();
  };
  const onBlur = () => {
    if (commitPromise === null) options.onCommitRequest();
  };
  textarea.addEventListener("keydown", onKeyDown);
  textarea.addEventListener("blur", onBlur);
  options.tile.appendChild(textarea);
  textarea.focus({ preventScroll: true });
  textarea.setSelectionRange(0, 0);
  const close = () => {
    if (closed) return;
    closed = true;
    textarea.removeEventListener("keydown", onKeyDown);
    textarea.removeEventListener("blur", onBlur);
    textarea.remove();
  };
  const fail = (message) => {
    textarea.readOnly = false;
    if (!closed) textarea.focus({ preventScroll: true });
    return { status: "failed", message };
  };
  const post = async (newBody) => {
    let response;
    try {
      response = await postJson(
        options.fetcher,
        "/slide-source",
        { key: options.key, old: options.body, new: newBody },
        false
      );
    } catch (error) {
      return fail(`failed to save slide source: ${String(error)}`);
    }
    if (!response.ok) {
      try {
        return fail(await readErrorResponse(response, "slide source save"));
      } catch (error) {
        return fail(`failed to save slide source: ${String(error)}`);
      }
    }
    let value;
    try {
      value = await response.json();
    } catch {
      return fail(INVALID_RESPONSE_MESSAGE);
    }
    if (typeof value !== "object" || value === null || typeof value.key !== "string" || typeof value.body !== "string") {
      return fail(INVALID_RESPONSE_MESSAGE);
    }
    const saved = value;
    close();
    return {
      status: "saved",
      previousKey: options.key,
      key: saved.key,
      body: saved.body
    };
  };
  const commit = () => {
    if (closed) return Promise.resolve({ status: "closed" });
    if (commitPromise !== null) return commitPromise;
    const newBody = textarea.value;
    if (newBody === options.body) {
      close();
      return Promise.resolve({ status: "unchanged", key: options.key, body: options.body });
    }
    committingBody = newBody;
    textarea.readOnly = true;
    const request = post(newBody);
    commitPromise = request;
    const clearCommit = () => {
      if (commitPromise !== request) return;
      commitPromise = null;
      committingBody = null;
    };
    void request.then(clearCommit, clearCommit);
    return request;
  };
  const saveForPageHide = () => {
    if (closed || sentForPageHide) return;
    const newBody = committingBody ?? textarea.value;
    if (newBody === options.body) return;
    sentForPageHide = true;
    try {
      const request = postJson(
        options.fetcher,
        "/slide-source",
        { key: options.key, old: options.body, new: newBody },
        true
      );
      void request.then(
        (response) => {
          if (!response.ok) sentForPageHide = false;
        },
        () => {
          sentForPageHide = false;
        }
      );
    } catch {
      sentForPageHide = false;
    }
  };
  return {
    textarea,
    commit,
    saveForPageHide,
    cancel: close,
    setFrame(frame) {
      textarea.style.left = `${frame.left}px`;
      textarea.style.top = `${frame.top}px`;
      textarea.style.width = `${frame.width}px`;
      textarea.style.height = `${frame.height}px`;
    },
    destroy: close
  };
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
function isFiniteNumber(value) {
  return typeof value === "number" && Number.isFinite(value);
}
function isIndexSyncMessage(value) {
  return isRecord(value) && isFiniteNumber(value.index) && isFiniteNumber(value.step);
}
function isSwappedSyncMessage(value) {
  return isRecord(value) && typeof value.swapped === "boolean";
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
function isBuildErrorSyncMessage(value) {
  return isRecord(value) && (typeof value.buildError === "string" || value.buildError === null);
}
function serverIndexReplayMessage(value) {
  if (!isFiniteNumber(value.index)) return null;
  return {
    index: value.index,
    step: isFiniteNumber(value.step) ? value.step : 0
  };
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
      if (isBuildErrorSyncMessage(body)) {
        onmessage?.({ data: { buildError: body.buildError } });
      }
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
      const indexReplay = serverIndexReplayMessage(body);
      if (!skipAbsoluteState && indexReplay !== null) {
        onmessage?.({ data: indexReplay });
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
          const skipAbsoluteState = body.seq < highestAckedPostSeq;
          if (body.message != null && !(skipAbsoluteState && isIndexSyncMessage(body.message))) {
            onmessage?.({ data: body.message });
          }
          deliverReplayState(body, {
            skipAbsoluteState,
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

// src/preview.ts
var PREVIEW_STATE_KEY = "peitho:preview-state";
var DEFAULT_PREVIEW_MODE = "grid";
var GRID_TILE_WIDTH = 320;
var GRID_GAP = 18;
var GRID_PADDING = 24;
var PREVIEW_NOTES_HEIGHT = 160;
var PREVIEW_STRIP_WIDTH = 200;
var STRIP_PADDING = 12;
var STRIP_GAP = 10;
var NO_NOTES_PLACEHOLDER = "No notes for this slide.";
var INLINE_EDIT_OUTLINE = "2px solid #38bdf8";
var NESTED_LIST_ITEM_BLOCKS = /* @__PURE__ */ new Set([
  "BLOCKQUOTE",
  "DIV",
  "H1",
  "H2",
  "H3",
  "H4",
  "H5",
  "H6",
  "OL",
  "P",
  "PRE",
  "TABLE",
  "UL"
]);
function isStringRecord(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value) && Object.values(value).every((entry) => typeof entry === "string");
}
function isSlideSources(value) {
  if (typeof value !== "object" || value === null) return false;
  const sources = value;
  return typeof sources.version === "number" && isStringRecord(sources.sources) && isStringRecord(sources.unavailable);
}
function isPreviewDraft(value) {
  if (typeof value !== "object" || value === null) return false;
  const draft = value;
  return typeof draft.key === "string" && (draft.text === void 0 || typeof draft.text === "string") && typeof draft.selectionStart === "number" && Number.isFinite(draft.selectionStart) && draft.selectionStart >= 0 && typeof draft.selectionEnd === "number" && Number.isFinite(draft.selectionEnd) && draft.selectionEnd >= 0 && typeof draft.focused === "boolean";
}
function parseEditableSourceRange(value) {
  const match = /^(\d+)-(\d+)$/.exec(value);
  if (match === null) return null;
  const start = Number(match[1]);
  const end = Number(match[2]);
  if (!Number.isSafeInteger(start) || !Number.isSafeInteger(end) || start >= end) return null;
  return { start, end };
}
function isNestedListItemBlock(node) {
  return node instanceof Element && NESTED_LIST_ITEM_BLOCKS.has(node.tagName);
}
function placeCaretAtEnd(win, editor) {
  const selection = win.getSelection();
  if (selection === null) return;
  const range = editor.ownerDocument.createRange();
  range.selectNodeContents(editor);
  range.collapse(false);
  selection.removeAllRanges();
  selection.addRange(range);
}
function selectionBelongsToEditor(range, editor) {
  const container = range.commonAncestorContainer;
  return container === editor || editor.contains(container);
}
function selectionRange(selection, editor) {
  if (selection === null || selection.rangeCount === 0) return null;
  const range = selection.getRangeAt(0);
  if (!selectionBelongsToEditor(range, editor)) return null;
  return {
    range,
    select(nextRange) {
      selection.removeAllRanges();
      selection.addRange(nextRange);
    }
  };
}
function rangeFromStaticRange(editor, source) {
  try {
    const range = editor.ownerDocument.createRange();
    range.setStart(source.startContainer, source.startOffset);
    range.setEnd(source.endContainer, source.endOffset);
    return selectionBelongsToEditor(range, editor) ? range : null;
  } catch {
    return null;
  }
}
function defaultSelectionRangeProvider(editor) {
  const documentSelection = editor.ownerDocument.getSelection();
  const root = editor.getRootNode();
  if (documentSelection !== null && root instanceof ShadowRoot && typeof documentSelection.getComposedRanges === "function") {
    try {
      for (const source of documentSelection.getComposedRanges({ shadowRoots: [root] })) {
        const range = rangeFromStaticRange(editor, source);
        if (range !== null) {
          return {
            range,
            select(nextRange) {
              documentSelection.removeAllRanges();
              documentSelection.addRange(nextRange);
            }
          };
        }
      }
    } catch {
    }
  }
  if (root instanceof ShadowRoot) {
    const rootSelection = root.getSelection?.();
    const selected = selectionRange(rootSelection ?? null, editor);
    if (selected !== null) return selected;
  }
  return selectionRange(documentSelection, editor);
}
function rangeEndsAtTextEnd(range, editor) {
  try {
    const trailing = editor.ownerDocument.createRange();
    trailing.selectNodeContents(editor);
    trailing.setStart(range.endContainer, range.endOffset);
    return trailing.toString() === "";
  } catch {
    return false;
  }
}
function insertSourceNewline(win, editor, rangeProvider) {
  const selected = rangeProvider(editor);
  if (selected === null || !selectionBelongsToEditor(selected.range, editor)) {
    editor.append("\n");
    placeCaretAtEnd(win, editor);
    return false;
  }
  const range = selected.range;
  range.deleteContents();
  range.collapse(true);
  const addSentinel = rangeEndsAtTextEnd(range, editor) && !(editor.textContent ?? "").endsWith("\n");
  const newline = editor.ownerDocument.createTextNode(addSentinel ? "\n\n" : "\n");
  range.insertNode(newline);
  if (addSentinel) range.setStart(newline, 1);
  else range.setStartAfter(newline);
  range.collapse(true);
  selected.select(range);
  return addSentinel;
}
function isEditableTarget(event) {
  const target = event.composedPath()[0];
  if (target instanceof HTMLTextAreaElement || target instanceof HTMLInputElement || target instanceof HTMLSelectElement) {
    return true;
  }
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable) return true;
  const editable = target.closest("[contenteditable]");
  return editable !== null && editable.getAttribute("contenteditable") !== "false";
}
function previewGridColumnCount(rootWidth) {
  const columns = Math.floor(
    (rootWidth - GRID_PADDING * 2 + GRID_GAP) / (GRID_TILE_WIDTH + GRID_GAP)
  );
  return Math.max(1, columns);
}
var previewNavigationKeyMap = /* @__PURE__ */ new Map([
  ["ArrowRight", "next"],
  ["PageDown", "next"],
  ["ArrowLeft", "prev"],
  ["PageUp", "prev"],
  ["ArrowUp", "up"],
  ["ArrowDown", "down"],
  ["Home", "first"],
  ["End", "last"]
]);
var verticalPreviewNavigationTargets = /* @__PURE__ */ new Set(["up", "down"]);
function installPreviewKeyboard(win = window, bus = win) {
  const onKeyDown = (event) => {
    if (hasChordModifier(event) || isComposingKey(event)) return;
    const editable = isEditableTarget(event);
    if (editable && (event.shiftKey || event.key !== "PageUp" && event.key !== "PageDown")) {
      return;
    }
    if (event.key === "e" && !event.shiftKey) {
      event.preventDefault();
      bus.dispatchEvent(new CustomEvent("peitho:sourceeditrequest"));
      return;
    }
    if (event.key === "o") {
      event.preventDefault();
      dispatchOverviewRequest(bus, "toggle");
      return;
    }
    if (event.key === "Escape") {
      if (event.repeat) return;
      event.preventDefault();
      dispatchOverviewRequest(bus, "enter");
      return;
    }
    if (event.key === "Enter") {
      if (event.repeat) return;
      const target = event.composedPath()[0];
      if (target instanceof Element && target.closest("a") !== null) return;
      event.preventDefault();
      dispatchOverviewRequest(bus, "activate");
      return;
    }
    const to = previewNavigationKeyMap.get(event.key);
    if (!to) return;
    const request = new CustomEvent("peitho:navigate", {
      cancelable: true,
      detail: { to }
    });
    bus.dispatchEvent(request);
    if (request.defaultPrevented || !editable && !verticalPreviewNavigationTargets.has(to)) {
      event.preventDefault();
    }
  };
  win.addEventListener("keydown", onKeyDown);
  return () => win.removeEventListener("keydown", onKeyDown);
}
function dispatchOverviewRequest(bus, action) {
  bus.dispatchEvent(
    new CustomEvent("peitho:overviewrequest", {
      detail: { action }
    })
  );
}
async function mountPreviewShell(options) {
  const shell = new PreviewShellController(options);
  await shell.load();
  return shell;
}
function installPreviewReload(shell, channelFactory = serverSyncChannelFactory(), reload = () => window.location.reload()) {
  const channel = channelFactory("peitho-sync");
  channel.onmessage = (event) => {
    if (isSessionChangedSyncMessage(event.data)) return;
    if (isBuildErrorSyncMessage(event.data)) {
      shell.setBuildError(event.data.buildError);
    }
    if (isGenerationSyncMessage(event.data)) {
      shell.requestGenerationReload(event.data.generation, reload);
    }
  };
  return () => {
    channel.onmessage = null;
    channel.close();
  };
}
var PreviewShellController = class {
  manifest = null;
  currentIndex = -1;
  selectedIndex = -1;
  mode = DEFAULT_PREVIEW_MODE;
  generation = 0;
  firstLayoutDone = false;
  root;
  fetcher;
  win;
  doc;
  log;
  bus;
  storage;
  syncUrl;
  viewport;
  selectionRangeProvider;
  restoredState;
  slides = [];
  notes = { version: 1, notes: {} };
  sources = { version: 1, sources: {}, unavailable: {} };
  notesPanel;
  notesTextarea;
  notesStatus;
  panelStatuses = /* @__PURE__ */ new Map();
  notesPositionText;
  buildErrorBanner;
  activeEdit = null;
  notesTextareaKey = null;
  swallowEnterRepeat = false;
  flushChain = Promise.resolve(true);
  flushesInFlight = 0;
  transitionSequence = 0;
  pendingTransitionSettlements = 0;
  deferredReload = null;
  strip;
  tileListenerCleanups = [];
  fontScopeCleanup = null;
  dimensions = { width: 1280, height: 720 };
  onNavigate = (event) => {
    if (!this.isLoaded()) return;
    const detail = event.detail;
    if (!detail || !("to" in detail)) {
      this.log.error("Invalid peitho:navigate event");
      return;
    }
    if (this.navigateToTarget(detail.to)) {
      event.preventDefault();
    }
  };
  onOverviewRequest = (event) => {
    if (!this.isLoaded()) return;
    const action = event.detail?.action;
    if (action === "toggle") this.toggleOverview();
    else if (action === "enter") this.enterGrid();
    else if (action === "exit") this.exitGrid();
    else if (action === "activate") this.activateSelection();
    else this.log.error("Invalid peitho:overviewrequest event");
  };
  onSourceEditRequest = () => this.tryStartSourceEdit();
  onResize = () => this.applyLayout();
  onNotesKeyDown = (event) => {
    if (event.key === "Enter" && event.repeat && this.swallowEnterRepeat) {
      event.preventDefault();
      return;
    }
    if (!event.repeat) this.swallowEnterRepeat = false;
    if (isComposingKey(event)) return;
    if (hasChordModifier(event) || event.key !== "Escape") return;
    event.preventDefault();
    this.notesTextarea.blur();
  };
  onNotesBlur = () => {
    this.swallowEnterRepeat = false;
    void this.flushNotes();
  };
  onPageHide = () => {
    this.saveState();
    const active = this.activeEdit;
    if (active?.kind === "inline") {
      const edit = active.edit;
      const text2 = this.slideEditText(edit);
      if (text2 !== edit.old) {
        void this.sendSlideEdit(edit, text2, true).catch(() => void 0);
      }
    } else if (active?.kind === "source") {
      active.edit.saveForPageHide();
    }
    const key = this.notesTextareaKey;
    const text = this.notesTextarea.value;
    void this.doFlush(key, text, true);
  };
  constructor(options) {
    this.root = options.root;
    this.fetcher = options.fetcher ?? fetch.bind(globalThis);
    this.win = options.window ?? window;
    this.doc = options.document ?? document;
    const log = options.console ?? console;
    this.log = { error: log.error, warn: log.warn ?? console.warn.bind(console) };
    this.bus = options.bus ?? this.win;
    this.storage = options.storage ?? this.win.sessionStorage;
    this.syncUrl = options.syncUrl ?? "/sync";
    this.viewport = options.viewport;
    this.selectionRangeProvider = options.selectionRangeProvider ?? defaultSelectionRangeProvider;
    this.restoredState = this.readState();
    this.root.classList.add("peitho-preview-root");
    const rootPosition = this.win.getComputedStyle(this.root).position;
    if (rootPosition === "static" || rootPosition === "") {
      this.root.style.position = "relative";
    }
    this.notesPanel = this.createNotesPanel();
    this.notesTextarea = this.notesPanel.querySelector(
      '[data-peitho-preview="note"]'
    );
    this.notesStatus = this.notesPanel.querySelector(
      '[data-peitho-preview="status"]'
    );
    this.notesPositionText = this.notesPanel.querySelector(
      '[data-peitho-preview="position"]'
    );
    this.buildErrorBanner = this.createBuildErrorBanner();
    this.strip = this.createStrip();
    this.notesTextarea.addEventListener("keydown", this.onNotesKeyDown);
    this.notesTextarea.addEventListener("blur", this.onNotesBlur);
    this.bus.addEventListener("peitho:navigate", this.onNavigate);
    this.bus.addEventListener("peitho:overviewrequest", this.onOverviewRequest);
    this.bus.addEventListener("peitho:sourceeditrequest", this.onSourceEditRequest);
    this.win.addEventListener("resize", this.onResize);
    this.win.addEventListener("pagehide", this.onPageHide);
  }
  async load() {
    try {
      const initialSyncState = await this.fetchInitialSyncState();
      this.generation = initialSyncState.generation;
      this.setBuildError(initialSyncState.buildError);
      const manifest = await this.fetchJson("manifest.json");
      this.notes = await this.fetchJson("notes.json");
      const loadedSources = await this.fetchJson("sources.json");
      if (!isSlideSources(loadedSources)) throw new Error("Invalid sources.json");
      this.sources = loadedSources;
      this.dimensions = {
        width: manifest.canvasWidth,
        height: manifest.canvasHeight
      };
      const cssAspect = manifest.aspectRatio.replace(":", " / ");
      this.setCanvasRootProperties(this.dimensions, cssAspect);
      const css = await this.fetchText("peitho.css");
      this.fontScopeCleanup = installDocumentFontScope(this.doc, css);
      const sources = await Promise.all(
        manifest.slides.map(async (slide) => ({ slide, html: await this.fetchText(slide.src) }))
      );
      await waitForFontsReady(this.doc, this.win, {
        log: this.log,
        text: deckText(sources.map((source) => source.html))
      });
      const pending = sources.map(({ slide, html }) => this.createSlideView(slide, html, css));
      this.manifest = manifest;
      this.doc.title = manifest.title;
      this.root.replaceChildren();
      this.strip.replaceChildren();
      for (const view of pending) {
        this.root.appendChild(view.tile);
        this.strip.appendChild(view.thumb);
        this.slides.push(view);
      }
      this.root.appendChild(this.notesPanel);
      this.root.appendChild(this.strip);
      this.root.appendChild(this.buildErrorBanner);
      const restored = this.restoredState;
      const restoredIndex = restored === null ? this.clampIndex(initialSlideIndex(pending.map((view) => view.meta)) ?? 0) : this.clampIndex(restored.index);
      this.currentIndex = restoredIndex;
      this.selectedIndex = restoredIndex;
      this.mode = restored?.mode ?? DEFAULT_PREVIEW_MODE;
      this.applyLayout();
      if (this.notesTextareaKey === null) this.renderNotes();
      this.restoreDraft(restored?.draft);
      this.markReady();
      this.dispatchSlideChange(null);
    } catch (error) {
      this.clearCanvasRootProperties();
      this.root.replaceChildren();
      this.root.textContent = error instanceof Error ? error.message : String(error);
      this.markReady();
    }
  }
  /**
   * Paint the black ground only now that the root has content (or an error message).
   * Until this runs the new document paints nothing, so a reload holds the previous
   * frame instead of flashing black for the duration of the mount.
   */
  markReady() {
    this.root.style.background = "#000";
    this.doc.documentElement.dataset.peithoReady = "";
  }
  navigate(to) {
    if (!this.isLoaded()) return;
    this.navigateToTarget(to);
  }
  setBuildError(error) {
    this.buildErrorBanner.textContent = error ?? "";
    this.buildErrorBanner.hidden = error === null;
  }
  requestGenerationReload(generation, reload) {
    if (generation === this.generation) return;
    this.saveState();
    if (this.isEditOpen() || this.pendingTransitionSettlements > 0) {
      this.deferredReload = reload;
      return;
    }
    reload();
  }
  flushNotes() {
    const key = this.notesTextareaKey;
    const text = this.notesTextarea.value;
    this.flushesInFlight += 1;
    this.flushChain = this.flushChain.then(
      () => this.doFlush(key, text, false),
      () => this.doFlush(key, text, false)
    ).finally(() => {
      this.flushesInFlight -= 1;
    });
    return this.flushChain;
  }
  async doFlush(key, text, keepalive) {
    if (key === null) return true;
    if (!this.isDirty(key, text)) {
      this.setNotesStatus("");
      return true;
    }
    try {
      const response = await postJson(this.fetcher, "/notes", { key, text }, keepalive);
      if (response.ok) {
        if (text === "") delete this.notes.notes[key];
        else this.notes.notes[key] = text;
        this.setNotesStatus("");
        return true;
      }
      this.setNotesStatus(await readErrorResponse(response));
    } catch (error) {
      this.setNotesStatus(error instanceof Error ? error.message : String(error));
    }
    return false;
  }
  navigateToTarget(to) {
    const index = this.resolveTarget(to);
    if (index === null) return false;
    this.setIndex(index);
    return true;
  }
  saveState() {
    if (!this.isLoaded()) return;
    const state = { mode: this.mode, index: this.stateIndex() };
    const focused = this.doc.activeElement === this.notesTextarea;
    const dirty = !this.notesSettled();
    if (this.notesTextareaKey !== null && (dirty || focused)) {
      const draft = {
        key: this.notesTextareaKey,
        selectionStart: this.notesTextarea.selectionStart,
        selectionEnd: this.notesTextarea.selectionEnd,
        focused
      };
      if (dirty) draft.text = this.notesTextarea.value;
      state.draft = draft;
    }
    this.writeState(state);
  }
  destroy() {
    this.transitionSequence += 1;
    this.notesTextarea.removeEventListener("keydown", this.onNotesKeyDown);
    this.notesTextarea.removeEventListener("blur", this.onNotesBlur);
    this.bus.removeEventListener("peitho:navigate", this.onNavigate);
    this.bus.removeEventListener("peitho:overviewrequest", this.onOverviewRequest);
    this.bus.removeEventListener("peitho:sourceeditrequest", this.onSourceEditRequest);
    this.win.removeEventListener("resize", this.onResize);
    this.win.removeEventListener("pagehide", this.onPageHide);
    while (this.tileListenerCleanups.length > 0) this.tileListenerCleanups.pop()?.();
    const active = this.activeEdit;
    if (active !== null) {
      this.activeEdit = null;
      if (active.kind === "inline") {
        active.edit.removeListeners();
        this.restoreSlideEdit(active.edit);
      } else {
        active.edit.destroy();
        this.setSlideSourceStatus("");
        this.applyLayout();
      }
    }
    this.deferredReload = null;
    this.fontScopeCleanup?.();
    this.fontScopeCleanup = null;
    this.clearCanvasRootProperties();
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
  async fetchInitialSyncState() {
    const response = await this.fetchOk(this.syncUrl);
    const body = await response.json();
    if (!isGenerationSyncMessage(body)) {
      throw new Error("Invalid peitho sync generation");
    }
    if (!isBuildErrorSyncMessage(body)) {
      throw new Error("Invalid peitho sync build error");
    }
    return { generation: body.generation, buildError: body.buildError };
  }
  createSlideView(slide, html, css) {
    const tile = this.doc.createElement("div");
    tile.classList.add("peitho-preview-tile");
    tile.dataset.slideKey = slide.key;
    tile.dataset.slideIndex = String(slide.index);
    const clickGuard = createClickNavigationGuard({ target: tile, window: this.win });
    this.tileListenerCleanups.push(() => clickGuard.destroy());
    const onTileClick = (event) => {
      if (clickGuard.shouldIgnoreClick(event)) return;
      if (this.tryStartSlideEdit(slide, host, event)) return;
      this.commitTransition(slide.index, "single");
    };
    tile.addEventListener("click", onTileClick);
    this.tileListenerCleanups.push(() => tile.removeEventListener("click", onTileClick));
    const host = this.createSlideHost(slide, html, css, "peitho-preview-slide");
    tile.appendChild(host);
    const tileNumber = this.createSlideNumber(slide);
    tile.appendChild(tileNumber);
    const thumb = this.doc.createElement("div");
    thumb.classList.add("peitho-preview-thumb");
    thumb.dataset.slideKey = slide.key;
    thumb.dataset.slideIndex = String(slide.index);
    thumb.setAttribute("role", "button");
    thumb.setAttribute("aria-label", `Slide ${slide.index + 1}`);
    const onThumbClick = () => this.setIndex(slide.index);
    thumb.addEventListener("click", onThumbClick);
    this.tileListenerCleanups.push(() => thumb.removeEventListener("click", onThumbClick));
    const thumbHost = this.createSlideHost(slide, html, css, "peitho-preview-thumb-slide");
    thumbHost.style.pointerEvents = "none";
    thumb.appendChild(thumbHost);
    thumb.appendChild(this.createSlideNumber(slide));
    return { meta: slide, sourceKey: slide.key, tile, host, thumb, thumbHost, tileNumber };
  }
  tryStartSourceEdit() {
    if (this.mode === "grid" || this.isEditOpen() || this.pendingTransitionSettlements > 0) {
      return;
    }
    const view = this.slides[this.currentIndex];
    if (view === void 0) return;
    const unavailable = this.sources.unavailable[view.sourceKey];
    if (unavailable !== void 0) {
      this.setSlideSourceStatus(unavailable);
      return;
    }
    const body = this.sources.sources[view.sourceKey];
    if (body === void 0) {
      this.setSlideSourceStatus("this slide cannot be edited from preview");
      return;
    }
    let edit;
    edit = openPreviewSourceEdit({
      document: this.doc,
      fetcher: this.fetcher,
      tile: view.tile,
      key: view.sourceKey,
      body,
      onCommitRequest: () => this.commitActiveEditAndRelease(),
      onCancelRequest: () => this.cancelSourceEdit(edit)
    });
    this.activeEdit = { kind: "source", edit, view, commitPromise: null };
    this.setSlideSourceStatus("");
    this.applyLayout();
  }
  finishSourceEditCommit(edit, result) {
    const active = this.activeEdit;
    if (active?.kind !== "source" || active.edit !== edit) return false;
    if (result.status === "closed") return true;
    if (result.status === "failed") {
      this.setSlideSourceStatus(result.message);
      return false;
    }
    if (result.status === "saved") {
      delete this.sources.sources[result.previousKey];
      this.sources.sources[result.key] = result.body;
      active.view.sourceKey = result.key;
    }
    this.activeEdit = null;
    this.setSlideSourceStatus("");
    if (this.pendingTransitionSettlements === 0) this.applyLayout();
    return true;
  }
  cancelSourceEdit(edit) {
    const active = this.activeEdit;
    if (active?.kind !== "source" || active.edit !== edit) return;
    edit.cancel();
    this.activeEdit = null;
    this.setSlideSourceStatus("");
    this.applyLayout();
    this.releaseDeferredReload();
  }
  tryStartSlideEdit(slide, host, event) {
    if (this.isEditOpen() || this.pendingTransitionSettlements > 0) return true;
    if (this.mode !== "single" || slide.index !== this.currentIndex) return false;
    const shadow = host.shadowRoot;
    if (shadow === null) return false;
    const path = event.composedPath();
    const boundary = path.indexOf(shadow);
    if (boundary <= 0) return false;
    let target = null;
    for (let index = 0; index < boundary; index += 1) {
      const candidate = path[index];
      if (candidate instanceof HTMLElement && candidate.hasAttribute("data-peitho-src") && candidate.hasAttribute("data-peitho-md")) {
        target = candidate;
        break;
      }
    }
    if (target === null || target.getRootNode() !== shadow) return false;
    const encodedRange = target.getAttribute("data-peitho-src");
    const old = target.getAttribute("data-peitho-md");
    if (encodedRange === null || old === null) return false;
    const sourceRange = parseEditableSourceRange(encodedRange);
    if (sourceRange === null) return false;
    let editor = target;
    let originalNodes;
    if (target.tagName === "LI") {
      const children = Array.from(target.childNodes);
      const nestedBlockIndex = children.findIndex(isNestedListItemBlock);
      const inlineEnd = nestedBlockIndex < 0 ? children.length : nestedBlockIndex;
      originalNodes = children.slice(0, inlineEnd);
      const insertionPoint = children[inlineEnd] ?? null;
      editor = this.doc.createElement("span");
      for (const node of originalNodes) target.removeChild(node);
      target.insertBefore(editor, insertionPoint);
    } else {
      originalNodes = Array.from(target.childNodes);
      target.replaceChildren();
    }
    const originalContenteditable = editor.getAttribute("contenteditable");
    const originalStyle = editor.getAttribute("style");
    editor.textContent = old;
    editor.setAttribute("contenteditable", "plaintext-only");
    editor.style.outline = INLINE_EDIT_OUTLINE;
    editor.style.outlineOffset = "2px";
    const editableStyle = editor.getAttribute("style");
    let edit;
    const onKeyDown = (keyboardEvent) => {
      this.handleSlideEditKeyDown(edit, keyboardEvent);
    };
    const onBlur = () => {
      this.commitActiveEditAndRelease();
    };
    edit = {
      key: slide.key,
      start: sourceRange.start,
      end: sourceRange.end,
      old,
      target,
      editor,
      originalNodes,
      originalContenteditable,
      originalStyle,
      editableStyle,
      trailingNewlineSentinel: false,
      commitPromise: null,
      removeListeners: () => {
        editor.removeEventListener("keydown", onKeyDown);
        editor.removeEventListener("blur", onBlur);
      }
    };
    editor.addEventListener("keydown", onKeyDown);
    editor.addEventListener("blur", onBlur);
    this.activeEdit = { kind: "inline", edit };
    this.setSlideEditStatus("");
    editor.focus({ preventScroll: true });
    placeCaretAtEnd(this.win, editor);
    return true;
  }
  handleSlideEditKeyDown(edit, event) {
    if (this.activeEdit?.kind !== "inline" || this.activeEdit.edit !== edit) return;
    if (edit.commitPromise !== null) {
      if (event.key === "Escape" || event.key === "Enter") {
        event.preventDefault();
        event.stopPropagation();
      }
      return;
    }
    if (hasChordModifier(event) || isComposingKey(event)) return;
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      this.cancelSlideEdit(edit);
      return;
    }
    if (event.key !== "Enter") return;
    event.preventDefault();
    event.stopPropagation();
    if (event.shiftKey) {
      edit.trailingNewlineSentinel = insertSourceNewline(this.win, edit.editor, this.selectionRangeProvider) || edit.trailingNewlineSentinel;
      return;
    }
    this.commitActiveEditAndRelease();
  }
  cancelSlideEdit(edit) {
    if (!this.closeSlideEdit(edit)) return;
    this.releaseDeferredReload();
  }
  closeSlideEdit(edit) {
    if (this.activeEdit?.kind !== "inline" || this.activeEdit.edit !== edit) return false;
    edit.removeListeners();
    this.activeEdit = null;
    this.restoreSlideEdit(edit);
    this.setSlideEditStatus("");
    return true;
  }
  commitActiveEditAndRelease() {
    void this.commitActiveEdit().then((committed) => {
      if (committed && this.pendingTransitionSettlements === 0) {
        this.releaseDeferredReload();
      }
    });
  }
  commitActiveEdit() {
    const active = this.activeEdit;
    if (active === null) return Promise.resolve(true);
    if (active.kind === "source") {
      if (active.commitPromise !== null) return active.commitPromise;
      const commit2 = active.edit.commit().then((result) => this.finishSourceEditCommit(active.edit, result)).finally(() => {
        active.commitPromise = null;
      });
      active.commitPromise = commit2;
      return commit2;
    }
    const edit = active.edit;
    if (edit.commitPromise !== null) return edit.commitPromise;
    const newText = this.slideEditText(edit);
    if (newText === edit.old) {
      this.closeSlideEdit(edit);
      return Promise.resolve(true);
    }
    const commit = this.postSlideEdit(edit, newText).finally(() => {
      edit.commitPromise = null;
    });
    edit.commitPromise = commit;
    this.lockSlideEdit(edit);
    return commit;
  }
  async postSlideEdit(edit, newText) {
    try {
      const response = await this.sendSlideEdit(edit, newText, false);
      if (response.ok) {
        if (this.activeEdit?.kind === "inline" && this.activeEdit.edit === edit) {
          this.finishSlideEdit(edit, newText);
        }
        return true;
      }
      const error = await readErrorResponse(response, "slide edit");
      if (this.activeEdit?.kind === "inline" && this.activeEdit.edit === edit) {
        this.setSlideEditStatus(error);
        this.unlockSlideEdit(edit);
      }
    } catch (error) {
      if (this.activeEdit?.kind === "inline" && this.activeEdit.edit === edit) {
        this.setSlideEditStatus(`failed to save slide edit: ${String(error)}`);
        this.unlockSlideEdit(edit);
      }
    }
    return false;
  }
  finishSlideEdit(edit, newText) {
    edit.removeListeners();
    this.activeEdit = null;
    edit.editor.textContent = newText;
    this.restoreSlideEditorAttributes(edit);
    edit.target.removeAttribute("data-peitho-src");
    edit.target.removeAttribute("data-peitho-md");
    this.setSlideEditStatus("");
  }
  sendSlideEdit(edit, newText, keepalive) {
    return postJson(
      this.fetcher,
      "/slide-edit",
      {
        key: edit.key,
        start: edit.start,
        end: edit.end,
        old: edit.old,
        new: newText
      },
      keepalive
    );
  }
  releaseDeferredReload() {
    const reload = this.deferredReload;
    if (reload === null) return;
    this.deferredReload = null;
    this.saveState();
    reload();
  }
  restoreSlideEdit(edit) {
    if (edit.editor !== edit.target) {
      edit.editor.replaceWith(...edit.originalNodes);
      return;
    }
    edit.target.replaceChildren(...edit.originalNodes);
    this.restoreSlideEditorAttributes(edit);
  }
  slideEditText(edit) {
    const text = edit.editor.textContent ?? "";
    if (edit.trailingNewlineSentinel && text.endsWith("\n")) return text.slice(0, -1);
    return text;
  }
  lockSlideEdit(edit) {
    edit.editor.setAttribute("contenteditable", "false");
    edit.editor.style.opacity = "0.65";
  }
  unlockSlideEdit(edit) {
    edit.editor.setAttribute("contenteditable", "plaintext-only");
    if (edit.editableStyle === null) edit.editor.removeAttribute("style");
    else edit.editor.setAttribute("style", edit.editableStyle);
    edit.editor.focus({ preventScroll: true });
  }
  restoreSlideEditorAttributes(edit) {
    if (edit.originalContenteditable === null) edit.editor.removeAttribute("contenteditable");
    else edit.editor.setAttribute("contenteditable", edit.originalContenteditable);
    if (edit.originalStyle === null) edit.editor.removeAttribute("style");
    else edit.editor.setAttribute("style", edit.originalStyle);
  }
  createSlideNumber(slide) {
    const badge = this.doc.createElement("span");
    badge.classList.add("peitho-preview-number");
    badge.textContent = String(slide.index + 1);
    const style = badge.style;
    style.position = "absolute";
    style.left = "6px";
    style.bottom = "6px";
    style.zIndex = "1";
    style.padding = "1px 7px";
    style.borderRadius = "4px";
    style.background = "rgba(0,0,0,0.65)";
    style.color = "#fff";
    style.font = "600 12px/1.5 system-ui, sans-serif";
    style.fontVariantNumeric = "tabular-nums";
    style.pointerEvents = "none";
    return badge;
  }
  createSlideHost(slide, html, css, className) {
    const host = this.doc.createElement("section");
    host.classList.add(className);
    host.dataset.slideKey = slide.key;
    host.dataset.slideIndex = String(slide.index);
    host.dataset.peithoCanvas = "slide";
    const shadow = host.attachShadow({ mode: "open" });
    const style = this.doc.createElement("style");
    style.textContent = css;
    shadow.appendChild(style);
    const template = this.doc.createElement("template");
    template.innerHTML = html;
    shadow.appendChild(template.content.cloneNode(true));
    return host;
  }
  createStrip() {
    const strip = this.doc.createElement("nav");
    strip.classList.add("peitho-preview-strip");
    strip.dataset.peithoPreview = "strip";
    strip.setAttribute("aria-label", "Slides");
    strip.hidden = true;
    const style = strip.style;
    style.position = "absolute";
    style.left = "0";
    style.top = "0";
    style.bottom = "0";
    style.width = `${PREVIEW_STRIP_WIDTH}px`;
    style.boxSizing = "border-box";
    style.overflowY = "auto";
    style.padding = `${STRIP_PADDING}px`;
    style.scrollPadding = `${STRIP_PADDING}px`;
    style.flexDirection = "column";
    style.gap = `${STRIP_GAP}px`;
    style.borderRight = "1px solid rgba(255,255,255,0.16)";
    style.background = "#15181e";
    return strip;
  }
  createBuildErrorBanner() {
    const banner = this.doc.createElement("div");
    banner.dataset.peithoPreview = "build-error";
    banner.setAttribute("role", "alert");
    banner.hidden = true;
    const style = banner.style;
    style.position = "fixed";
    style.left = "0";
    style.right = "0";
    style.top = "0";
    style.maxHeight = "40vh";
    style.boxSizing = "border-box";
    style.overflowY = "auto";
    style.zIndex = "2147483647";
    style.padding = "12px 18px";
    style.borderBottom = "2px solid #ef4444";
    style.background = "#450a0a";
    style.color = "#fee2e2";
    style.font = "14px/1.5 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace";
    style.whiteSpace = "pre-wrap";
    style.overflowWrap = "anywhere";
    return banner;
  }
  createNotesPanel() {
    const panel = this.doc.createElement("aside");
    panel.classList.add("peitho-preview-notes");
    panel.dataset.peithoPreview = "notes";
    panel.setAttribute("aria-label", "Speaker notes");
    panel.hidden = true;
    const style = panel.style;
    style.position = "absolute";
    style.left = `${PREVIEW_STRIP_WIDTH}px`;
    style.right = "0";
    style.bottom = "0";
    style.height = `${PREVIEW_NOTES_HEIGHT}px`;
    style.boxSizing = "border-box";
    style.overflow = "auto";
    style.padding = "14px 24px";
    style.borderTop = "1px solid rgba(255,255,255,0.16)";
    style.background = "#15181e";
    style.color = "#e5e7eb";
    style.font = "18px/1.5 system-ui, sans-serif";
    style.flexDirection = "column";
    const positionRow = this.doc.createElement("div");
    positionRow.style.display = "flex";
    positionRow.style.font = "600 13px/1.5 system-ui, sans-serif";
    positionRow.style.color = "#9ca3af";
    positionRow.style.fontVariantNumeric = "tabular-nums";
    positionRow.style.marginBottom = "4px";
    const positionText = this.doc.createElement("span");
    positionText.dataset.peithoPreview = "position";
    positionText.style.flexShrink = "0";
    positionRow.appendChild(positionText);
    const status = this.doc.createElement("span");
    status.dataset.peithoPreview = "status";
    status.setAttribute("role", "alert");
    status.style.marginLeft = "auto";
    status.style.color = "#f87171";
    status.style.whiteSpace = "pre-wrap";
    status.style.overflowWrap = "anywhere";
    positionRow.appendChild(status);
    panel.appendChild(positionRow);
    const textarea = this.doc.createElement("textarea");
    textarea.dataset.peithoPreview = "note";
    textarea.setAttribute("aria-label", "Speaker notes");
    textarea.placeholder = NO_NOTES_PLACEHOLDER;
    textarea.style.background = "transparent";
    textarea.style.color = "inherit";
    textarea.style.font = "inherit";
    textarea.style.border = "none";
    textarea.style.resize = "none";
    textarea.style.flex = "1";
    textarea.style.minHeight = "0";
    textarea.style.width = "100%";
    textarea.style.padding = "0";
    textarea.style.outlineOffset = "4px";
    panel.appendChild(textarea);
    return panel;
  }
  setNotesStatus(message) {
    this.setPanelStatus("notes", message);
  }
  setSlideEditStatus(message) {
    this.setPanelStatus("slide-edit", message);
  }
  setSlideSourceStatus(message) {
    this.setPanelStatus("slide-source", message);
  }
  /**
   * Save channels clear only their own status so one successful write cannot hide an
   * unrelated failure. Any remaining failure keeps the entire panel visibly alerting.
   */
  setPanelStatus(source, message) {
    if (message === "") this.panelStatuses.delete(source);
    else this.panelStatuses.set(source, message);
    const combined = ["notes", "slide-edit", "slide-source"].map((statusSource) => this.panelStatuses.get(statusSource)).filter((status) => status !== void 0).join("\n");
    this.notesStatus.textContent = combined;
    const failed = combined !== "";
    this.notesStatus.style.background = failed ? "#7f1d1d" : "";
    this.notesStatus.style.color = failed ? "#fee2e2" : "#f87171";
    this.notesStatus.style.padding = failed ? "2px 10px" : "";
    this.notesStatus.style.borderRadius = failed ? "999px" : "";
    this.notesPanel.style.borderTop = failed ? "3px solid #ef4444" : "1px solid rgba(255,255,255,0.16)";
    this.notesPanel.style.background = failed ? "#241416" : "#15181e";
  }
  renderNotes() {
    const slide = this.slides[this.currentIndex];
    const key = slide?.meta.key ?? null;
    this.notesPositionText.textContent = `${this.currentIndex + 1} / ${this.slides.length}`;
    if (this.notesTextareaKey !== key) {
      this.notesTextareaKey = key;
      this.notesTextarea.value = key === null ? "" : this.notes.notes[key] ?? "";
    }
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
  isLoaded() {
    return this.manifest !== null;
  }
  isEditOpen() {
    return this.activeEdit !== null;
  }
  toggleOverview() {
    if (this.mode === "grid") this.exitGrid();
    else this.enterGrid();
  }
  enterGrid() {
    this.commitTransition(this.currentIndex, "grid");
  }
  exitGrid() {
    this.commitTransition(this.selectedIndex, "single");
  }
  activateSelection() {
    if (this.mode === "grid") {
      this.exitGrid();
      return;
    }
    if (this.isEditOpen()) {
      this.commitTransition(this.currentIndex, "single", () => this.focusNotes());
      return;
    }
    this.focusNotes();
  }
  focusNotes() {
    const length = this.notesTextarea.value.length;
    this.notesTextarea.setSelectionRange(length, length);
    this.notesTextarea.focus();
    this.swallowEnterRepeat = true;
  }
  setIndex(index) {
    this.commitTransition(index, this.mode);
  }
  commitTransition(index, mode, afterCommit) {
    index = this.clampIndex(index);
    if (afterCommit === void 0 && index === this.currentIndex && index === this.selectedIndex && mode === this.mode) {
      return;
    }
    const sequence = ++this.transitionSequence;
    const needsFlush = mode === "grid" && this.mode === "single" || mode === "single" && this.slides[index]?.meta.key !== this.notesTextareaKey;
    const commit = () => {
      const previousIndex = this.currentIndex < 0 ? null : this.currentIndex;
      const previousMode = this.mode;
      this.currentIndex = index;
      this.selectedIndex = index;
      this.mode = mode;
      if (previousIndex !== index || previousMode !== mode) this.setSlideSourceStatus("");
      if (mode === "grid" && this.doc.activeElement === this.notesTextarea) {
        this.notesTextarea.blur();
      }
      this.applyLayout();
      if (previousIndex !== index) this.dispatchSlideChange(previousIndex);
      this.saveState();
      afterCommit?.();
    };
    if (!this.isEditOpen() && (!needsFlush || this.notesSettled())) {
      commit();
      this.releaseDeferredReload();
      return;
    }
    this.pendingTransitionSettlements += 1;
    void (async () => {
      try {
        const settled = await this.settleForTransition(sequence);
        if (sequence !== this.transitionSequence) return;
        if (!settled) {
          if (!this.isEditOpen()) this.releaseDeferredReload();
          return;
        }
        commit();
        this.releaseDeferredReload();
      } finally {
        this.pendingTransitionSettlements -= 1;
      }
    })();
  }
  async settleForTransition(sequence) {
    if (!await this.commitActiveEdit()) return false;
    if (sequence !== this.transitionSequence) return false;
    while (!this.notesSettled()) {
      if (!await this.flushNotes()) return false;
      if (sequence !== this.transitionSequence) return false;
    }
    return true;
  }
  notesAreDirty() {
    return this.isDirty(this.notesTextareaKey, this.notesTextarea.value);
  }
  notesSettled() {
    return this.flushesInFlight === 0 && !this.notesAreDirty();
  }
  isDirty(key, text) {
    return key !== null && text !== (this.notes.notes[key] ?? "");
  }
  resolveTarget(to) {
    if (to === "first") return 0;
    if (to === "last") return this.slides.length - 1;
    if (to === "next") {
      if (this.mode === "grid") return Math.min(this.selectedIndex + 1, this.slides.length - 1);
      return this.resolveSequentialTarget(1);
    }
    if (to === "prev") {
      if (this.mode === "grid") return Math.max(this.selectedIndex - 1, 0);
      return this.resolveSequentialTarget(-1);
    }
    if (to === "up" || to === "down") {
      if (this.mode === "grid") return this.resolveGridVerticalTarget(to);
      return this.resolveSequentialTarget(to === "up" ? -1 : 1);
    }
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
      this.selectedIndex,
      direction
    );
  }
  resolveGridVerticalTarget(direction) {
    const columns = previewGridColumnCount(this.gridRootWidth());
    const selected = this.clampIndex(this.selectedIndex);
    const next = selected + (direction === "up" ? -columns : columns);
    if (next < 0 || next > this.slides.length - 1) return null;
    return next;
  }
  gridRootWidth() {
    if (this.root.clientWidth > 0) return this.root.clientWidth;
    return this.viewport?.().width ?? this.win.innerWidth;
  }
  clampIndex(index) {
    if (this.slides.length === 0) return 0;
    return Math.min(Math.max(index, 0), this.slides.length - 1);
  }
  applyLayout() {
    this.root.dataset.peithoPreviewMode = this.mode;
    if (this.mode === "grid") this.applyGridLayout();
    else this.applySingleLayout();
    this.firstLayoutDone = true;
  }
  /**
   * `nearest` is right for stepping between neighbours (it holds the scroll still when the
   * target is already visible), but wrong for the first layout after a load: the strip starts
   * at scrollTop 0, so the minimal scroll pins the current slide to the bottom edge no matter
   * where the pre-reload scroll had it. Centre it once, then hand over to `nearest`.
   */
  scrollIntoViewOnLayout(element) {
    element?.scrollIntoView?.({ block: this.firstLayoutDone ? "nearest" : "center" });
  }
  applySingleLayout() {
    const viewport = this.viewport?.() ?? {
      width: this.win.innerWidth,
      height: this.win.innerHeight
    };
    const fit = calculateCanvasFit(
      {
        width: Math.max(0, viewport.width - PREVIEW_STRIP_WIDTH),
        height: Math.max(0, viewport.height - PREVIEW_NOTES_HEIGHT)
      },
      this.dimensions.width,
      this.dimensions.height
    );
    const thumbWidth = PREVIEW_STRIP_WIDTH - STRIP_PADDING * 2 - 2;
    const thumbScale = thumbWidth / this.dimensions.width;
    const thumbHeight = this.dimensions.height * thumbScale;
    this.notesPanel.hidden = false;
    this.notesPanel.style.display = "flex";
    this.strip.hidden = false;
    this.strip.style.display = "flex";
    this.renderNotes();
    this.root.style.display = "block";
    this.root.style.overflow = "hidden";
    this.root.style.padding = "0";
    this.root.style.gap = "0";
    this.root.style.gridTemplateColumns = "";
    this.root.style.removeProperty("scroll-padding-top");
    this.root.style.removeProperty("scroll-padding-bottom");
    this.slides.forEach((slide, index) => {
      const active = index === this.currentIndex;
      const sourceEdit = active && this.activeEdit?.kind === "source" && this.activeEdit.view === slide ? this.activeEdit.edit : null;
      slide.tile.hidden = !active;
      slide.tile.classList.toggle("is-selected", active);
      slide.tile.style.position = "absolute";
      slide.tile.style.left = `${PREVIEW_STRIP_WIDTH}px`;
      slide.tile.style.top = "0";
      slide.tile.style.width = `calc(100% - ${PREVIEW_STRIP_WIDTH}px)`;
      slide.tile.style.height = "100%";
      slide.tile.style.overflow = "hidden";
      slide.tile.style.border = "0";
      slide.tile.style.borderRadius = "0";
      slide.tile.style.outlineWidth = "";
      slide.tile.style.outlineStyle = "";
      slide.tile.style.outlineColor = "";
      slide.tile.style.outlineOffset = "";
      slide.tile.style.background = "transparent";
      slide.host.hidden = !active || sourceEdit !== null;
      slide.tileNumber.hidden = true;
      if (sourceEdit === null) {
        this.applyHostFrame(slide.host, fit.left, fit.top, fit.scale);
      } else {
        sourceEdit.setFrame({
          left: fit.left,
          top: fit.top,
          width: this.dimensions.width * fit.scale,
          height: this.dimensions.height * fit.scale
        });
      }
      slide.thumb.classList.toggle("is-selected", active);
      slide.thumb.setAttribute("aria-current", active ? "true" : "false");
      slide.thumb.style.position = "relative";
      slide.thumb.style.flexShrink = "0";
      slide.thumb.style.width = `${thumbWidth}px`;
      slide.thumb.style.height = `${thumbHeight}px`;
      slide.thumb.style.overflow = "hidden";
      slide.thumb.style.border = "1px solid rgba(255,255,255,0.24)";
      slide.thumb.style.borderRadius = "4px";
      slide.thumb.style.outline = active ? "3px solid #7dd3fc" : "";
      slide.thumb.style.outlineOffset = active ? "1px" : "";
      slide.thumb.style.background = "#000";
      slide.thumb.style.cursor = "pointer";
      this.applyHostFrame(slide.thumbHost, 0, 0, thumbScale);
    });
    this.scrollIntoViewOnLayout(this.slides[this.currentIndex]?.thumb);
  }
  applyGridLayout() {
    const scale = GRID_TILE_WIDTH / this.dimensions.width;
    const tileHeight = this.dimensions.height * scale;
    this.root.style.display = "grid";
    this.root.style.gridTemplateColumns = `repeat(auto-fit, minmax(${GRID_TILE_WIDTH}px, ${GRID_TILE_WIDTH}px))`;
    this.root.style.gap = `${GRID_GAP}px`;
    this.root.style.alignContent = "start";
    this.root.style.justifyContent = "center";
    this.root.style.overflow = "auto";
    this.root.style.padding = `${GRID_PADDING}px`;
    this.root.style.setProperty("scroll-padding-top", `${GRID_PADDING}px`);
    this.root.style.setProperty("scroll-padding-bottom", `${GRID_PADDING}px`);
    this.root.style.boxSizing = "border-box";
    this.notesPanel.hidden = true;
    this.notesPanel.style.display = "";
    this.strip.hidden = true;
    this.strip.style.display = "";
    this.slides.forEach((slide, index) => {
      const selected = index === this.selectedIndex;
      slide.tile.hidden = false;
      slide.tile.classList.toggle("is-selected", selected);
      slide.tile.setAttribute("aria-selected", String(selected));
      slide.tile.style.position = "relative";
      slide.tile.style.left = "";
      slide.tile.style.top = "";
      slide.tile.style.width = `${GRID_TILE_WIDTH}px`;
      slide.tile.style.height = `${tileHeight}px`;
      slide.tile.style.overflow = "hidden";
      slide.tile.style.border = "1px solid rgba(255,255,255,0.24)";
      slide.tile.style.borderRadius = "6px";
      slide.tile.style.outlineWidth = selected ? "3px" : "";
      slide.tile.style.outlineStyle = selected ? "solid" : "";
      slide.tile.style.outlineColor = selected ? "#7dd3fc" : "";
      slide.tile.style.outlineOffset = selected ? "1px" : "";
      slide.tile.style.background = "#000";
      slide.tile.style.cursor = "pointer";
      slide.tileNumber.hidden = false;
      slide.tile.style.boxSizing = "content-box";
      slide.host.hidden = false;
      this.applyHostFrame(slide.host, 0, 0, scale);
    });
    this.scrollSelectedTileIntoView();
  }
  scrollSelectedTileIntoView() {
    this.scrollIntoViewOnLayout(this.slides[this.selectedIndex]?.tile);
  }
  applyHostFrame(host, left, top, scale) {
    host.style.position = "absolute";
    host.style.left = "0";
    host.style.top = "0";
    host.style.width = `${this.dimensions.width}px`;
    host.style.height = `${this.dimensions.height}px`;
    host.style.transformOrigin = "top left";
    host.style.transform = `translate(${left}px, ${top}px) scale(${scale})`;
  }
  dispatchSlideChange(previousIndex) {
    const slide = this.slides[this.currentIndex];
    if (!slide) return;
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
  readState() {
    let raw = null;
    try {
      raw = this.storage?.getItem(PREVIEW_STATE_KEY) ?? null;
    } catch (error) {
      this.log.error(`Failed to read preview state: ${String(error)}`);
      return null;
    }
    if (raw == null) return null;
    try {
      const parsed = JSON.parse(raw);
      if ((parsed.mode === "single" || parsed.mode === "grid") && typeof parsed.index === "number") {
        const state = { mode: parsed.mode, index: parsed.index };
        if (isPreviewDraft(parsed.draft)) state.draft = parsed.draft;
        return state;
      }
    } catch (_error) {
      return null;
    }
    return null;
  }
  restoreDraft(draft) {
    if (draft === void 0) return;
    const key = this.slides[this.currentIndex]?.meta.key;
    if (draft.key === key) {
      if (draft.text !== void 0) this.notesTextarea.value = draft.text;
      const selectionStart = Math.min(draft.selectionStart, this.notesTextarea.value.length);
      const selectionEnd = Math.min(draft.selectionEnd, this.notesTextarea.value.length);
      this.notesTextarea.setSelectionRange(selectionStart, selectionEnd);
      if (draft.focused && this.mode === "single") this.notesTextarea.focus();
    }
    this.writeState({ mode: this.mode, index: this.stateIndex() });
  }
  stateIndex() {
    return this.clampIndex(this.selectedIndex >= 0 ? this.selectedIndex : this.currentIndex);
  }
  writeState(state) {
    try {
      this.storage?.setItem(PREVIEW_STATE_KEY, JSON.stringify(state));
    } catch (error) {
      this.log.error(`Failed to save preview state: ${String(error)}`);
    }
  }
};
export {
  PREVIEW_NOTES_HEIGHT,
  PREVIEW_STRIP_WIDTH,
  installPreviewKeyboard,
  installPreviewReload,
  mountPreviewShell,
  previewGridColumnCount
};
//# sourceMappingURL=preview.js.map
