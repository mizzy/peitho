// src/canvas.ts
var CANVAS_WIDTH = 1280;
var CANVAS_HEIGHT = 720;
function calculateCanvasFit(viewport, canvasWidth = CANVAS_WIDTH, canvasHeight = CANVAS_HEIGHT) {
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
  const canvasWidth = options.canvasWidth ?? CANVAS_WIDTH;
  const canvasHeight = options.canvasHeight ?? CANVAS_HEIGHT;
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

// src/timeTracker.ts
var clamp01 = (ratio) => Math.min(Math.max(ratio, 0), 1);
function isOverrun(elapsedMs, plannedDurationMs) {
  return elapsedMs > plannedDurationMs;
}
function formatMinuteSeconds(ms) {
  const totalSeconds = Math.round(ms / 1e3);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = (totalSeconds % 60).toString().padStart(2, "0");
  return `${minutes}:${seconds}`;
}
function timeScaleLabels(plannedDurationMs) {
  return Array.from(
    { length: 5 },
    (_, index) => formatMinuteSeconds(plannedDurationMs * index / 4)
  );
}
function isValidSlideChangeDetail(detail) {
  if (typeof detail !== "object" || detail === null) return false;
  const candidate = detail;
  const { index, previousIndex, total } = candidate;
  return typeof index === "number" && Number.isFinite(index) && index >= 0 && typeof total === "number" && Number.isFinite(total) && total > 0 && (previousIndex === null || typeof previousIndex === "number" && Number.isFinite(previousIndex) && previousIndex >= 0);
}
function installTimeTracker(options) {
  if (!Number.isFinite(options.plannedDurationMs) || options.plannedDurationMs <= 0) {
    throw new Error("plannedDurationMs must be a positive finite number");
  }
  const win = options.window ?? window;
  const doc = options.document ?? document;
  const log = options.console ?? console;
  const bus = options.bus ?? win;
  const variant = options.variant ?? "present";
  const track = doc.createElement("div");
  track.className = "peitho-time-tracker";
  track.dataset.peithoTimeTracker = variant;
  if (variant === "presenter") {
    track.innerHTML = [
      '<div class="tracker-legend"><span>Slide progress</span><span>Time</span></div>',
      '<div class="tracker">',
      '<div class="tracker-fill"></div>',
      '<span data-peitho-marker="rabbit" aria-label="slide progress">\u{1F430}</span>',
      '<span data-peitho-marker="turtle" aria-label="time progress">\u{1F422}</span>',
      "</div>",
      `<div class="tracker-scale mono">${timeScaleLabels(options.plannedDurationMs).map((label) => `<span>${label}</span>`).join("")}</div>`
    ].join("");
  } else {
    track.innerHTML = [
      '<span data-peitho-marker="rabbit" aria-label="slide progress">\u{1F430}</span>',
      '<span data-peitho-marker="turtle" aria-label="time progress">\u{1F422}</span>'
    ].join("");
  }
  options.root.appendChild(track);
  const rabbit = track.querySelector('[data-peitho-marker="rabbit"]');
  const turtle = track.querySelector('[data-peitho-marker="turtle"]');
  const fill = track.querySelector(".tracker-fill");
  let autoStarted = false;
  const setMarker = (element, ratio) => {
    element.style.left = `${Math.round(ratio * 1e4) / 100}%`;
    element.style.transform = `translateX(${-Math.round(ratio * 1e4) / 100}%)`;
  };
  const updateSlides = (index, total) => {
    const ratio = total <= 1 ? 0 : index / (total - 1);
    setMarker(rabbit, clamp01(ratio));
  };
  const tick = () => {
    const elapsedMs = options.shell.elapsedMs();
    const ratio = elapsedMs / options.plannedDurationMs;
    const clampedRatio = clamp01(ratio);
    setMarker(turtle, clampedRatio);
    if (fill) fill.style.width = `${Math.round(clampedRatio * 1e4) / 100}%`;
    track.toggleAttribute(
      "data-peitho-overrun",
      isOverrun(elapsedMs, options.plannedDurationMs)
    );
  };
  const onSlideChange = (event) => {
    const detail = event.detail;
    if (!isValidSlideChangeDetail(detail)) {
      log.error("Invalid peitho:slidechange event");
      return;
    }
    updateSlides(detail.index, detail.total);
    if (!autoStarted && detail.previousIndex !== null && detail.index > detail.previousIndex) {
      autoStarted = true;
      bus.dispatchEvent(
        new CustomEvent("peitho:timercontrol", {
          detail: { action: "start" }
        })
      );
    }
  };
  updateSlides(options.shell.currentIndex, options.shell.manifest?.slideCount ?? 0);
  tick();
  bus.addEventListener("peitho:slidechange", onSlideChange);
  const interval = win.setInterval(tick, 250);
  return () => {
    win.clearInterval(interval);
    bus.removeEventListener("peitho:slidechange", onSlideChange);
    track.remove();
  };
}

// src/agenda.ts
var EM_DASH = "\u2014";
var MINUS_SIGN = "\u2212";
function installAgenda(options) {
  if (options.sections.length === 0) return () => void 0;
  const win = options.window ?? window;
  const doc = options.document ?? document;
  const bus = options.bus ?? win;
  const host = doc.createElement("section");
  host.dataset.peithoAgenda = "true";
  host.innerHTML = [
    "<div data-peitho-agenda-head>",
    "<span data-peitho-agenda-title>Agenda</span>",
    "<span data-peitho-agenda-hint>Actual / Planned</span>",
    "</div>",
    "<div data-peitho-agenda-list></div>"
  ].join("");
  options.root.appendChild(host);
  const list = host.querySelector("[data-peitho-agenda-list]");
  const rows = options.sections.map((section) => createRow(doc, section));
  list.append(...rows.map(({ row }) => row));
  const actualMs = new Array(options.sections.length).fill(0);
  let lastElapsedMs = options.shell.elapsedMs();
  function sectionIndexForSlide(slideIndex) {
    return options.sections.findIndex(
      (section) => slideIndex >= section.startIndex && slideIndex <= section.endIndex
    );
  }
  function render() {
    const currentSection = sectionIndexForSlide(options.shell.currentIndex);
    rows.forEach((row, index) => updateRow(row, index, currentSection, actualMs[index]));
  }
  function flushElapsedToSlide(slideIndex) {
    if (slideIndex === null || options.shell.startedAt() === null) return;
    const elapsedMs = options.shell.elapsedMs();
    const delta = Math.max(0, elapsedMs - lastElapsedMs);
    const sectionIndex = sectionIndexForSlide(slideIndex);
    if (sectionIndex >= 0) actualMs[sectionIndex] += delta;
    lastElapsedMs = elapsedMs;
  }
  function onSlideChange(event) {
    const previousIndex = event.detail?.previousIndex ?? null;
    flushElapsedToSlide(previousIndex);
    render();
  }
  function onTimerControl(event) {
    const action = event.detail?.action;
    if (action !== "reset") return;
    actualMs.fill(0);
    lastElapsedMs = 0;
    render();
  }
  function tick() {
    if (options.shell.startedAt() === null) {
      actualMs.fill(0);
      lastElapsedMs = 0;
      render();
      return;
    }
    flushElapsedToSlide(options.shell.currentIndex);
    render();
  }
  render();
  bus.addEventListener("peitho:slidechange", onSlideChange);
  bus.addEventListener("peitho:timercontrol", onTimerControl);
  const interval = win.setInterval(tick, 250);
  return () => {
    win.clearInterval(interval);
    bus.removeEventListener("peitho:slidechange", onSlideChange);
    bus.removeEventListener("peitho:timercontrol", onTimerControl);
    host.remove();
  };
}
function createRow(doc, section) {
  const row = doc.createElement("div");
  row.dataset.peithoAgendaRow = "true";
  row.innerHTML = [
    '<span data-peitho-agenda-marker aria-hidden="true"></span>',
    "<span data-peitho-agenda-label><span data-peitho-agenda-name></span><span data-peitho-agenda-range></span></span>",
    "<span data-peitho-agenda-time></span>",
    "<span data-peitho-agenda-delta></span>"
  ].join("");
  row.querySelector("[data-peitho-agenda-name]").textContent = section.name;
  row.querySelector("[data-peitho-agenda-range]").textContent = formatSlideRange(section);
  return {
    row,
    section,
    time: row.querySelector("[data-peitho-agenda-time]"),
    delta: row.querySelector("[data-peitho-agenda-delta]")
  };
}
function updateRow(view, index, currentSection, actual) {
  const state = agendaState(index, currentSection);
  view.row.dataset.peithoAgendaState = state;
  if (state === "done") {
    view.row.dataset.peithoAgendaOutcome = outcomeFor(actual, view.section.plannedDurationMs);
  } else {
    delete view.row.dataset.peithoAgendaOutcome;
  }
  view.time.textContent = `${actualText(state, actual)} / ${formatMinuteSeconds(
    view.section.plannedDurationMs
  )}`;
  view.delta.textContent = deltaText(state, actual, view.section.plannedDurationMs);
}
function agendaState(index, currentSection) {
  if (index < currentSection) return "done";
  if (index === currentSection) return "current";
  return "upcoming";
}
function actualText(state, actual) {
  return state === "upcoming" ? EM_DASH : formatMinuteSeconds(actual);
}
function formatSlideRange(section) {
  const start = String(section.startIndex + 1).padStart(2, "0");
  const end = String(section.endIndex + 1).padStart(2, "0");
  return section.startIndex === section.endIndex ? start : `${start}\u2013${end}`;
}
function deltaText(state, actual, planned) {
  if (state !== "done") return "\xB7";
  const diffSec = diffSeconds(actual, planned);
  const sign = diffSec > 0 ? "+" : MINUS_SIGN;
  return `${sign}${formatMinuteSeconds(Math.abs(diffSec) * 1e3)}`;
}
function outcomeFor(actual, planned) {
  return diffSeconds(actual, planned) > 0 ? "over" : "under";
}
function diffSeconds(actual, planned) {
  return Math.round((actual - planned) / 1e3);
}

// src/presentDisplay.ts
var PRESENTER_URL = "presenter.html";
var PRESENTER_TARGET = "peitho-presenter";
function fallbackFeatures() {
  return "popup=yes,width=1200,height=800,left=80,top=80";
}
function openPresenterPopup(options = {}) {
  const win = options.window ?? window;
  const url = options.url ?? PRESENTER_URL;
  const features = options.features ?? fallbackFeatures();
  const openWindow = options.openWindow ?? ((nextUrl, target, nextFeatures) => win.open(nextUrl, target, nextFeatures));
  return openWindow(url, PRESENTER_TARGET, features);
}

// src/controls.ts
function installPresentationControls(options) {
  const win = options.window ?? window;
  const doc = options.document ?? document;
  const bus = options.bus ?? win;
  const idleMs = options.idleMs ?? 3e3;
  const openPresenter = options.openPresenter ?? (() => openPresenterPopup({
    window: win,
    openWindow: options.openPresenterWindow
  }));
  const bar = doc.createElement("nav");
  bar.dataset.peithoControlBar = "true";
  bar.className = "peitho-control-bar";
  bar.hidden = true;
  bar.innerHTML = [
    '<button type="button" data-peitho-action="prev" aria-label="Previous slide">\u25C0</button>',
    '<button type="button" data-peitho-action="next" aria-label="Next slide">\u25B6</button>',
    '<output data-peitho-control="counter">\u2013 / \u2013</output>',
    '<button type="button" data-peitho-action="fullscreen" aria-label="Toggle fullscreen">\u26F6</button>',
    '<button type="button" data-peitho-action="presenter">Presenter</button>',
    '<button type="button" data-peitho-action="close" aria-label="Close presentation">\u2715</button>'
  ].join("");
  options.root.appendChild(bar);
  let hideTimer = null;
  const clearHideTimer = () => {
    if (hideTimer !== null) win.clearTimeout(hideTimer);
    hideTimer = null;
  };
  const show = () => {
    bar.hidden = false;
    clearHideTimer();
    hideTimer = win.setTimeout(() => {
      bar.hidden = true;
      hideTimer = null;
    }, idleMs);
  };
  const dispatchNavigate2 = (to) => {
    bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to } }));
  };
  const onClick = (event) => {
    event.stopPropagation();
    const action = event.target.closest("[data-peitho-action]")?.dataset.peithoAction;
    if (action === "prev" || action === "next") dispatchNavigate2(action);
    if (action === "presenter") void openPresenter();
    if (action === "fullscreen") toggleFullscreen(doc);
    if (action === "close") bus.dispatchEvent(new CustomEvent("peitho:closerequest"));
  };
  const onSlideChange = (event) => {
    const detail = event.detail;
    const counter = bar.querySelector('[data-peitho-control="counter"]');
    if (counter) counter.textContent = `${detail.index + 1} / ${detail.total}`;
  };
  win.addEventListener("mousemove", show);
  bar.addEventListener("click", onClick);
  bus.addEventListener("peitho:slidechange", onSlideChange);
  return () => {
    clearHideTimer();
    win.removeEventListener("mousemove", show);
    bar.removeEventListener("click", onClick);
    bus.removeEventListener("peitho:slidechange", onSlideChange);
    bar.remove();
  };
}
function installCanvasClickNavigation(options) {
  const win = options.window ?? window;
  const bus = options.bus ?? win;
  const onClick = (event) => {
    if (event.target.closest('[data-peitho-control-bar="true"]')) return;
    const to = event.clientX < win.innerWidth / 4 ? "prev" : "next";
    bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to } }));
  };
  options.root.addEventListener("click", onClick);
  return () => options.root.removeEventListener("click", onClick);
}
function installFullscreenShortcut(options = {}) {
  const win = options.window ?? window;
  const doc = options.document ?? document;
  const onKeyDown = (event) => {
    if (event.key !== "f") return;
    event.preventDefault();
    toggleFullscreen(doc);
  };
  win.addEventListener("keydown", onKeyDown);
  return () => win.removeEventListener("keydown", onKeyDown);
}
function toggleFullscreen(doc = document) {
  if (doc.fullscreenElement) {
    void doc.exitFullscreen?.();
    return;
  }
  void doc.documentElement.requestFullscreen?.();
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
function dispatchNavigate(bus, to) {
  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to } }));
}
function installKeyboardNavigation(win = window, bus = win) {
  const onKeyDown = (event) => {
    const to = keyMap.get(event.key);
    if (!to) return;
    event.preventDefault();
    dispatchNavigate(bus, to);
  };
  win.addEventListener("keydown", onKeyDown);
  return () => win.removeEventListener("keydown", onKeyDown);
}
function installPresenterKeyboard(win, bus, onPlaypause) {
  const onKeyDown = (event) => {
    const to = navigationKeyMap.get(event.key);
    if (to) {
      event.preventDefault();
      dispatchNavigate(bus, to);
      return;
    }
    if (event.key !== " ") return;
    event.preventDefault();
    if (event.repeat) return;
    onPlaypause();
  };
  win.addEventListener("keydown", onKeyDown);
  return () => win.removeEventListener("keydown", onKeyDown);
}
function installCloseOnEscape(win = window, bus = win) {
  const onKeyDown = (event) => {
    if (event.key !== "Escape") return;
    event.preventDefault();
    bus.dispatchEvent(new CustomEvent("peitho:closerequest"));
  };
  win.addEventListener("keydown", onKeyDown);
  return () => win.removeEventListener("keydown", onKeyDown);
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
  win;
  doc;
  log;
  bus;
  now;
  viewport;
  canvasCleanups = [];
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
      const manifest = await this.fetchJson("manifest.json");
      const css = await this.fetchText("peitho.css");
      const pending = [];
      for (const slide of manifest.slides) {
        const html = await this.fetchText(slide.src);
        const host = this.createSlideHost(slide, html, css);
        pending.push({ meta: slide, host });
      }
      this.manifest = manifest;
      for (const view of pending) {
        this.root.appendChild(view.host);
        this.slides.push(view);
      }
      this.show(0);
    } catch (error) {
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
  destroy() {
    this.endPresentation();
    while (this.canvasCleanups.length > 0) this.canvasCleanups.pop()?.();
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
  createSlideHost(slide, html, css) {
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
        viewport: this.viewport
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
    if (to === "next") return Math.min(this.currentIndex + 1, this.slides.length - 1);
    if (to === "prev") return Math.max(this.currentIndex - 1, 0);
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
  }
  resumeTimer() {
    if (this.pausedAtValue === null) return;
    this.pausedTotalMs += this.now() - this.pausedAtValue;
    this.pausedAtValue = null;
  }
  resetTimer() {
    this.startedAtValue = null;
    this.pausedAtValue = null;
    this.pausedTotalMs = 0;
    this.ended = false;
  }
};

// src/sync.ts
function isRecord(value) {
  return typeof value === "object" && value !== null;
}
function isCloseSyncMessage(value) {
  return isRecord(value) && value.close === true;
}
function isIndexSyncMessage(value) {
  return isRecord(value) && typeof value.index === "number";
}
function defaultChannelFactory(name) {
  const channel = new BroadcastChannel(name);
  let onmessage = null;
  channel.onmessage = (event) => {
    onmessage?.({ data: event.data });
  };
  return {
    get onmessage() {
      return onmessage;
    },
    set onmessage(next) {
      onmessage = next;
    },
    postMessage(message) {
      channel.postMessage(message);
    },
    close() {
      channel.close();
    }
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
    let abortController = null;
    let retryTimer = null;
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
        seq = body.seq;
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
      while (!closed && !await handshake()) {
        continue;
      }
      while (!closed) {
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
          onmessage?.({ data: body.message });
        } catch (error) {
          if (!closed) {
            console.error(`Failed to poll sync message: ${String(error)}`);
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
        void fetcher(url, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(message),
          keepalive: true
        }).then((response) => {
          if (!response.ok) console.error(`Failed to post sync message: ${response.status}`);
        }).catch((error) => {
          console.error(`Failed to post sync message: ${String(error)}`);
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
function installSyncBridge(win = window, channelFactory = defaultChannelFactory, bus = win, closeWindow = () => win.close()) {
  const channel = channelFactory("peitho-sync");
  const onSlideChange = (event) => {
    const detail = event.detail;
    if (typeof detail?.index !== "number") return;
    channel.postMessage({ index: detail.index });
  };
  const onCloseRequest = () => {
    channel.postMessage({ close: true });
  };
  channel.onmessage = (event) => {
    const data = event.data;
    if (isCloseSyncMessage(data)) {
      closeWindow();
      return;
    }
    if (isIndexSyncMessage(data)) {
      bus.dispatchEvent(
        new CustomEvent("peitho:navigate", { detail: { to: { index: data.index } } })
      );
      return;
    }
    console.error("Invalid peitho sync message");
  };
  bus.addEventListener("peitho:slidechange", onSlideChange);
  bus.addEventListener("peitho:closerequest", onCloseRequest);
  return () => {
    bus.removeEventListener("peitho:slidechange", onSlideChange);
    bus.removeEventListener("peitho:closerequest", onCloseRequest);
    channel.onmessage = null;
    channel.close();
  };
}

// src/presenter.ts
function formatSeconds(totalSeconds) {
  const minutes = Math.floor(totalSeconds / 60).toString().padStart(2, "0");
  const seconds = (totalSeconds % 60).toString().padStart(2, "0");
  return `${minutes}:${seconds}`;
}
function formatElapsed(ms) {
  return formatSeconds(Math.floor(ms / 1e3));
}
function formatOverrun(ms) {
  return formatSeconds(Math.ceil(ms / 1e3));
}
var STATE_LABELS = {
  stopped: "Stopped",
  running: "Running",
  paused: "Paused"
};
var PLAY_LABELS = {
  stopped: "Start",
  running: "Pause",
  paused: "Resume"
};
function deriveTimerState(shell) {
  if (shell.startedAt() === null) return "stopped";
  return shell.isPaused() ? "paused" : "running";
}
function playpauseActionFor(state) {
  if (state === "stopped") return "start";
  if (state === "paused") return "resume";
  return "pause";
}
function formatSlideNumber(value) {
  return value.toString().padStart(2, "0");
}
function renderPresenterTimer(doc, root, elapsedMs, plannedDurationMs) {
  if (plannedDurationMs == null) {
    root.textContent = formatElapsed(elapsedMs);
    return;
  }
  root.replaceChildren(doc.createTextNode(formatElapsed(elapsedMs)));
  const planned = doc.createElement("span");
  planned.className = "planned";
  planned.textContent = ` / ${formatElapsed(plannedDurationMs)}`;
  root.appendChild(planned);
  if (!isOverrun(elapsedMs, plannedDurationMs)) return;
  const overrun = doc.createElement("span");
  overrun.className = "overrun";
  overrun.textContent = ` +${formatOverrun(elapsedMs - plannedDurationMs)}`;
  root.appendChild(overrun);
}
function paneViewport(pane) {
  return () => ({
    width: pane.clientWidth,
    height: pane.clientHeight
  });
}
function validateAgendaSections(sections, log) {
  for (const [index, section] of sections.entries()) {
    const plannedDurationMs = section.plannedDurationMs;
    if (!Number.isFinite(plannedDurationMs) || plannedDurationMs <= 0 || !Number.isSafeInteger(plannedDurationMs)) {
      log.error(
        `Invalid plannedDurationMs for manifest section ${index + 1} "${section.name}" in manifest.json`
      );
      return [];
    }
  }
  return sections;
}
async function mountPresenterView(options) {
  const win = options.window ?? window;
  const doc = options.document ?? document;
  const fetcher = options.fetcher ?? fetch.bind(globalThis);
  const now = options.now ?? Date.now;
  const log = options.console ?? console;
  const bus = win;
  const previewBus = new EventTarget();
  options.root.innerHTML = `
    <section class="peitho-presenter app" data-screen-label="Presenter view">
      <section class="left" aria-label="Current slide and notes">
        <div class="stage">
          <header class="colhead">
            <div class="status-line">
              <span class="now">Now</span>
              <span class="sep"></span>
              <span data-peitho-presenter="position">Slide 00 of 00</span>
            </div>
            <div class="deck-title" data-peitho-presenter="title">Peitho Deck</div>
          </header>

          <div class="slide-frame">
            <div
              class="peitho-presenter-pane slide-pane"
              data-peitho-presenter="current"
              role="img"
              aria-label="Current slide preview"
            ></div>
          </div>

          <div class="kbdbar">
            <div class="pos" data-peitho-presenter="position-short">00 / 00</div>
            <div>
              <span class="grp"><span class="kbd">\u2190</span><span class="kbd">\u2192</span> navigate</span>
              <span class="grp"><span class="kbd">Space</span> start / pause</span>
              <span class="grp"><span class="kbd">Esc</span> close</span>
            </div>
          </div>

          <section class="notes" aria-label="Speaker notes">
            <div class="notes-head">
              <span>Notes</span>
              <span class="badge" data-peitho-presenter="notes-slide">Slide 00</span>
            </div>
            <div class="notes-body" data-peitho-presenter="notes"></div>
          </section>
        </div>
      </section>

      <aside class="right">
        <section class="card" aria-label="Next slide">
          <div class="card-head">
            <span>Next</span>
            <span class="badge mono" data-peitho-presenter="next-position">00 / 00</span>
          </div>
          <div class="next-wrap">
            <div class="next-preview">
              <div class="peitho-presenter-pane" data-peitho-presenter="preview"></div>
              <p data-peitho-presenter="preview-end" hidden>End of deck</p>
            </div>
          </div>
        </section>

        <section class="card clock" data-peitho-presenter="clock" data-peitho-state="stopped" aria-label="Timer">
          <div class="clock-row">
            <output class="timer mono" data-peitho-presenter="timer">00:00</output>
            <span class="state-pill" data-peitho-presenter="state-pill" data-peitho-state="stopped">
              <span class="state-dot"></span>
              <span data-peitho-presenter="state-label">Stopped</span>
            </span>
          </div>

          <div class="tracker-wrap" data-peitho-presenter="tracker-slot"></div>
          <div data-peitho-presenter="agenda-slot"></div>

          <div class="controls">
            <button class="btn play primary" type="button" data-peitho-action="playpause"><span data-peitho-presenter="play-label">Start</span> <span class="k">Space</span></button>
            <button class="btn" type="button" data-peitho-action="prev">Prev <span class="k">\u2190</span></button>
            <button class="btn" type="button" data-peitho-action="next">Next <span class="k">\u2192</span></button>
            <button class="btn" type="button" data-peitho-action="reset">Reset</button>
            <button class="btn danger" type="button" data-peitho-action="close">Close <span class="k">Esc</span></button>
          </div>
        </section>
      </aside>
    </section>`;
  const currentRoot = options.root.querySelector('[data-peitho-presenter="current"]');
  const previewRoot = options.root.querySelector('[data-peitho-presenter="preview"]');
  const previewEnd = options.root.querySelector(
    '[data-peitho-presenter="preview-end"]'
  );
  const notesRoot = options.root.querySelector('[data-peitho-presenter="notes"]');
  const timerRoot = options.root.querySelector('[data-peitho-presenter="timer"]');
  const clockRoot = options.root.querySelector('[data-peitho-presenter="clock"]');
  const statePill = options.root.querySelector(
    '[data-peitho-presenter="state-pill"]'
  );
  const stateLabel = options.root.querySelector(
    '[data-peitho-presenter="state-label"]'
  );
  const playLabel = options.root.querySelector(
    '[data-peitho-presenter="play-label"]'
  );
  const playButton = options.root.querySelector(
    '[data-peitho-action="playpause"]'
  );
  const trackerSlot = options.root.querySelector(
    '[data-peitho-presenter="tracker-slot"]'
  );
  const agendaSlot = options.root.querySelector(
    '[data-peitho-presenter="agenda-slot"]'
  );
  const deckTitle = options.root.querySelector('[data-peitho-presenter="title"]');
  const positionLong = options.root.querySelector(
    '[data-peitho-presenter="position"]'
  );
  const positionShort = options.root.querySelector(
    '[data-peitho-presenter="position-short"]'
  );
  const notesSlide = options.root.querySelector(
    '[data-peitho-presenter="notes-slide"]'
  );
  const nextPosition = options.root.querySelector(
    '[data-peitho-presenter="next-position"]'
  );
  const mainShell = await mountPresentShell({
    root: currentRoot,
    fetcher,
    window: win,
    document: doc,
    bus,
    now,
    viewport: paneViewport(currentRoot)
  });
  const previewShell = await mountPresentShell({
    root: previewRoot,
    fetcher,
    window: win,
    document: doc,
    bus: previewBus,
    now,
    viewport: paneViewport(previewRoot)
  });
  const keyboardCleanup = installPresenterKeyboard(win, bus, dispatchPlaypause);
  const syncCleanup = installSyncBridge(win, options.syncChannelFactory, bus);
  const rawPlannedDurationMs = mainShell.manifest?.plannedDurationMs ?? null;
  const plannedDurationMs = rawPlannedDurationMs != null && Number.isFinite(rawPlannedDurationMs) && rawPlannedDurationMs > 0 ? rawPlannedDurationMs : null;
  if (rawPlannedDurationMs != null && plannedDurationMs == null) {
    log.error("Invalid plannedDurationMs in manifest.json");
  }
  const trackerCleanup = plannedDurationMs == null ? () => void 0 : installTimeTracker({
    root: trackerSlot,
    shell: mainShell,
    plannedDurationMs,
    bus,
    window: win,
    document: doc,
    variant: "presenter"
  });
  const sections = validateAgendaSections(mainShell.manifest?.sections ?? [], log);
  const agendaCleanup = installAgenda({
    root: agendaSlot,
    shell: mainShell,
    sections,
    bus,
    window: win,
    document: doc
  });
  const rippleTimeouts = /* @__PURE__ */ new Set();
  function setTimerStateChrome(state) {
    clockRoot.dataset.peithoState = state;
    statePill.dataset.peithoState = state;
    stateLabel.textContent = STATE_LABELS[state];
    playLabel.textContent = PLAY_LABELS[state];
    playButton.setAttribute("aria-label", PLAY_LABELS[state]);
  }
  function tick() {
    const elapsedMs = mainShell.elapsedMs();
    renderPresenterTimer(doc, timerRoot, elapsedMs, plannedDurationMs);
    timerRoot.toggleAttribute(
      "data-peitho-overrun",
      plannedDurationMs != null && isOverrun(elapsedMs, plannedDurationMs)
    );
    setTimerStateChrome(deriveTimerState(mainShell));
  }
  function updateFromSlide(detail) {
    notesRoot.textContent = options.notes.notes[detail.key] ?? "No notes for this slide.";
    const slideNumber = detail.index + 1;
    const slide = formatSlideNumber(slideNumber);
    const total = formatSlideNumber(detail.total);
    deckTitle.textContent = mainShell.manifest?.title ?? "Peitho Deck";
    positionLong.textContent = `Slide ${slide} of ${total}`;
    positionShort.textContent = `${slide} / ${total}`;
    notesSlide.textContent = `Slide ${slide}`;
    const nextIndex = detail.index + 1;
    if (nextIndex < detail.total) {
      previewRoot.hidden = false;
      previewEnd.hidden = true;
      nextPosition.textContent = `${formatSlideNumber(nextIndex + 1)} / ${total}`;
      previewBus.dispatchEvent(
        new CustomEvent("peitho:navigate", { detail: { to: { index: nextIndex } } })
      );
    } else {
      previewRoot.hidden = true;
      previewEnd.hidden = false;
      nextPosition.textContent = "End";
    }
    tick();
  }
  const onSlideChange = (event) => {
    updateFromSlide(event.detail);
  };
  bus.addEventListener("peitho:slidechange", onSlideChange);
  const firstSlide = mainShell.manifest?.slides[mainShell.currentIndex];
  if (firstSlide) {
    updateFromSlide({
      key: firstSlide.key,
      index: firstSlide.index,
      total: mainShell.manifest?.slideCount ?? 0,
      previousIndex: null
    });
  }
  const buttonCleanups = [];
  const addButtonListener = (action, listener) => {
    const button = options.root.querySelector(`[data-peitho-action="${action}"]`);
    if (!button) return;
    button.addEventListener("click", listener);
    buttonCleanups.push(() => button.removeEventListener("click", listener));
  };
  function dispatchTimerControl(action) {
    bus.dispatchEvent(
      new CustomEvent("peitho:timercontrol", { detail: { action } })
    );
    tick();
  }
  function dispatchPlaypause() {
    dispatchTimerControl(playpauseActionFor(deriveTimerState(mainShell)));
  }
  addButtonListener("prev", () => {
    bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "prev" } }));
  });
  addButtonListener("next", () => {
    bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: "next" } }));
  });
  addButtonListener("playpause", dispatchPlaypause);
  addButtonListener("reset", () => {
    dispatchTimerControl("reset");
  });
  addButtonListener("close", () => {
    bus.dispatchEvent(new CustomEvent("peitho:closerequest"));
  });
  const onPointerDown = (event) => {
    const pointer = event;
    const target = pointer.target;
    if (!target || !("closest" in target)) return;
    const button = target.closest(".btn");
    if (!button || !options.root.contains(button)) return;
    const rect = button.getBoundingClientRect();
    const width = rect.width || 1;
    const height = rect.height || 1;
    const x = (pointer.clientX - rect.left) / width * 100;
    const y = (pointer.clientY - rect.top) / height * 100;
    button.style.setProperty("--rx", `${x}%`);
    button.style.setProperty("--ry", `${y}%`);
    button.classList.remove("pressed");
    void button.offsetWidth;
    button.classList.add("pressed");
    const timeout = win.setTimeout(() => {
      button.classList.remove("pressed");
      rippleTimeouts.delete(timeout);
    }, 550);
    rippleTimeouts.add(timeout);
  };
  options.root.addEventListener("pointerdown", onPointerDown);
  const interval = win.setInterval(tick, 250);
  tick();
  return {
    mainShell,
    previewShell,
    tick,
    destroy() {
      win.clearInterval(interval);
      for (const timeout of rippleTimeouts) win.clearTimeout(timeout);
      rippleTimeouts.clear();
      options.root.removeEventListener("pointerdown", onPointerDown);
      while (buttonCleanups.length > 0) buttonCleanups.pop()?.();
      agendaCleanup();
      trackerCleanup();
      bus.removeEventListener("peitho:slidechange", onSlideChange);
      keyboardCleanup();
      syncCleanup();
      previewShell.destroy();
      mainShell.destroy();
    }
  };
}
export {
  CANVAS_HEIGHT,
  CANVAS_WIDTH,
  PRESENTER_URL,
  calculateCanvasFit,
  fallbackFeatures,
  formatMinuteSeconds,
  installAgenda,
  installCanvasClickNavigation,
  installCanvasScaler,
  installCloseOnEscape,
  installFullscreenShortcut,
  installKeyboardNavigation,
  installPresentationControls,
  installPresenterKeyboard,
  installSyncBridge,
  installTimeTracker,
  isOverrun,
  mountPresentShell,
  mountPresenterView,
  openPresenterPopup,
  serverSyncChannelFactory,
  toggleFullscreen
};
//# sourceMappingURL=shell.js.map
