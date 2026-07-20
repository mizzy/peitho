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
function timeScaleLabels(plannedDurationMs) {
  return Array.from(
    { length: 5 },
    (_, index) => formatMinuteSeconds(plannedDurationMs * index / 4)
  );
}
function timeScaleLabelTransform(index, labelCount) {
  if (index === 0) return "translateX(0%)";
  if (index === labelCount - 1) return "translateX(-100%)";
  return "translateX(-50%)";
}
function timeScaleLabelStyle(index, labelCount) {
  const left = Math.round(index / (labelCount - 1) * 1e4) / 100;
  const transform = timeScaleLabelTransform(index, labelCount);
  return `left: ${left}%; transform: ${transform}`;
}
function isValidSlideChangeDetail(detail) {
  if (typeof detail !== "object" || detail === null) return false;
  const candidate = detail;
  const { index, previousIndex, total } = candidate;
  return typeof index === "number" && Number.isFinite(index) && index >= 0 && typeof total === "number" && Number.isFinite(total) && total > 0 && (previousIndex === null || typeof previousIndex === "number" && Number.isFinite(previousIndex) && previousIndex >= 0);
}
function installTimeTracker(options) {
  if (!isValidDurationMs(options.plannedDurationMs)) {
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
    const scaleLabels = timeScaleLabels(options.plannedDurationMs);
    track.innerHTML = [
      '<div class="tracker-legend"><span>Slide progress</span><span>Time</span></div>',
      '<div class="tracker">',
      '<div class="tracker-fill"></div>',
      '<span data-peitho-marker="rabbit" aria-label="slide progress">\u{1F430}</span>',
      '<span data-peitho-marker="turtle" aria-label="time progress">\u{1F422}</span>',
      "</div>",
      `<div class="tracker-scale mono">${scaleLabels.map(
        (label, index) => `<span style="${timeScaleLabelStyle(index, scaleLabels.length)}">${label}</span>`
      ).join("")}</div>`
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

// src/sections.ts
function sectionIndexForSlide(sections, slideIndex) {
  return sections.findIndex(
    (section) => slideIndex >= section.startIndex && slideIndex <= section.endIndex
  );
}
function validateSections(sections, log) {
  let expectedStartIndex = 0;
  for (const [index, section] of sections.entries()) {
    const label = `manifest section ${index + 1} "${section.name}"`;
    if (!isValidDurationMs(section.plannedDurationMs)) {
      log.error(`Invalid plannedDurationMs for ${label} in manifest.json`);
      return false;
    }
    if (!isValidSlideIndex(section.startIndex) || !isValidSlideIndex(section.endIndex)) {
      log.error(
        `Invalid ${label}: startIndex and endIndex must be non-negative integers`
      );
      return false;
    }
    if (section.endIndex < section.startIndex) {
      log.error(`Invalid ${label}: endIndex must be greater than or equal to startIndex`);
      return false;
    }
    if (section.startIndex !== expectedStartIndex) {
      log.error(
        `Invalid ${label}: expected startIndex ${expectedStartIndex}, got ${section.startIndex}`
      );
      return false;
    }
    expectedStartIndex = section.endIndex + 1;
  }
  return true;
}
function isValidSlideIndex(index) {
  return Number.isSafeInteger(index) && index >= 0;
}

// src/agenda.ts
var EM_DASH = "\u2014";
var MINUS_SIGN = "\u2212";
function installAgenda(options) {
  if (options.sections.length === 0) return () => void 0;
  const win = options.window ?? window;
  const doc = options.document ?? document;
  const bus = options.bus ?? win;
  const rawLog = options.log ?? console;
  const log = { error: rawLog.error };
  if (!validateSections(options.sections, log)) return () => void 0;
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
  function render() {
    const currentSection = sectionIndexForSlide(options.sections, options.shell.currentIndex);
    const actuals = options.actuals.actualMs();
    rows.forEach(
      (row, index) => updateRow(row, index, currentSection, Math.max(0, actuals[index] ?? 0))
    );
  }
  function onSlideChange(event) {
    void event;
    render();
  }
  function tick() {
    render();
  }
  render();
  bus.addEventListener("peitho:slidechange", onSlideChange);
  bus.addEventListener("peitho:timercontrol", onSlideChange);
  bus.addEventListener("peitho:timeradopt", onSlideChange);
  const interval = win.setInterval(tick, 250);
  return () => {
    win.clearInterval(interval);
    bus.removeEventListener("peitho:slidechange", onSlideChange);
    bus.removeEventListener("peitho:timercontrol", onSlideChange);
    bus.removeEventListener("peitho:timeradopt", onSlideChange);
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
  const outcome = state === "upcoming" ? null : outcomeFor(actual, view.section.plannedDurationMs);
  if (outcome === "over" || state === "done" && outcome === "under") {
    view.row.dataset.peithoAgendaOutcome = outcome;
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
  return actual > 0 || state !== "upcoming" ? formatMinuteSeconds(actual) : EM_DASH;
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

// src/rehearsalBridge.ts
function installRehearsalBridge(win, bus = win, fetcher = win.fetch.bind(win)) {
  function onReport(event) {
    const detail = event.detail;
    void fetcher("/rehearsal", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      keepalive: true,
      body: JSON.stringify(detail)
    }).then((response) => {
      if (!response.ok) {
        console.error(`failed to POST rehearsal snapshot: ${response.status}`);
      }
    }).catch((error) => {
      console.error("failed to POST rehearsal snapshot", error);
    });
  }
  bus.addEventListener("peitho:rehearsalreport", onReport);
  return () => {
    bus.removeEventListener("peitho:rehearsalreport", onReport);
  };
}

// src/rehearsalReporter.ts
function installRehearsalReporter(options) {
  if (options.sections.length === 0) return () => void 0;
  const win = options.window ?? window;
  const bus = options.bus ?? win;
  let hasStarted = options.shell.startedAt() !== null;
  function markStarted() {
    if (options.shell.startedAt() !== null) hasStarted = true;
  }
  function snapshot() {
    const actualMs = options.actuals.actualMs();
    return {
      version: 1,
      elapsedMs: roundNonNegativeMs(options.shell.elapsedMs()),
      sections: options.sections.map((section, index) => ({
        name: section.name,
        plannedDurationMs: section.plannedDurationMs,
        actualMs: roundNonNegativeMs(actualMs[index] ?? 0)
      }))
    };
  }
  function report() {
    markStarted();
    if (!hasStarted) return;
    options.actuals.flush();
    bus.dispatchEvent(
      new CustomEvent("peitho:rehearsalreport", {
        detail: snapshot()
      })
    );
  }
  function onSlideChange() {
    report();
  }
  function onTimerControl(event) {
    const action = event.detail?.action;
    if (action === "start" || action === "resume") {
      markStarted();
      return;
    }
    if (action === "pause" || action === "reset") report();
  }
  function onTimerAdopt(event) {
    const detail = event.detail;
    if (!isValidTimerAdoptDetail(detail)) return;
    if (detail.running || detail.elapsedMs > 0) hasStarted = true;
    if (!detail.running && detail.elapsedMs === 0 && hasStarted) report();
  }
  function onCloseRequest() {
    report();
  }
  function tick() {
    markStarted();
    if (!hasStarted || options.shell.startedAt() === null || options.shell.isPaused()) return;
    report();
  }
  bus.addEventListener("peitho:slidechange", onSlideChange);
  bus.addEventListener("peitho:timercontrol", onTimerControl);
  bus.addEventListener("peitho:timeradopt", onTimerAdopt);
  bus.addEventListener("peitho:closerequest", onCloseRequest);
  const interval = win.setInterval(tick, 5e3);
  return () => {
    win.clearInterval(interval);
    bus.removeEventListener("peitho:slidechange", onSlideChange);
    bus.removeEventListener("peitho:timercontrol", onTimerControl);
    bus.removeEventListener("peitho:timeradopt", onTimerAdopt);
    bus.removeEventListener("peitho:closerequest", onCloseRequest);
  };
}
function isValidTimerAdoptDetail(detail) {
  return typeof detail?.running === "boolean" && typeof detail.elapsedMs === "number" && Number.isFinite(detail.elapsedMs) && detail.elapsedMs >= 0 && typeof detail.previousElapsedMs === "number" && Number.isFinite(detail.previousElapsedMs) && detail.previousElapsedMs >= 0;
}
function roundNonNegativeMs(ms) {
  return Math.max(0, Math.round(ms));
}

// src/sectionActuals.ts
function installSectionActuals(options) {
  if (options.sections.length === 0) {
    return {
      actualMs: () => [],
      flush: () => void 0,
      destroy: () => void 0
    };
  }
  const win = options.window ?? window;
  const bus = options.bus ?? win;
  const log = options.log ?? console;
  const actualMs = new Array(options.sections.length).fill(0);
  let lastElapsedMs = options.shell.elapsedMs();
  function flushElapsedToSectionOf(slideIndex) {
    if (slideIndex === null || options.shell.startedAt() === null) return;
    const elapsedMs = options.shell.elapsedMs();
    const delta = Math.max(0, elapsedMs - lastElapsedMs);
    const sectionIndex = sectionIndexForSlide(options.sections, slideIndex);
    if (sectionIndex >= 0) actualMs[sectionIndex] += delta;
    lastElapsedMs = elapsedMs;
  }
  function onSlideChange(event) {
    const previousIndex = event.detail?.previousIndex ?? null;
    flushElapsedToSectionOf(previousIndex);
  }
  function onTimerControl(event) {
    const action = event.detail?.action;
    if (action !== "reset") return;
    actualMs.fill(0);
    lastElapsedMs = 0;
  }
  function onTimerAdopt(event) {
    const detail = event.detail;
    if (typeof detail?.running !== "boolean" || typeof detail.elapsedMs !== "number" || !Number.isFinite(detail.elapsedMs) || detail.elapsedMs < 0 || typeof detail.previousElapsedMs !== "number" || !Number.isFinite(detail.previousElapsedMs) || detail.previousElapsedMs < 0) {
      log.error("Invalid peitho:timeradopt event");
      return;
    }
    if (!detail.running && detail.elapsedMs === 0) {
      actualMs.fill(0);
      lastElapsedMs = 0;
      return;
    }
    if (options.shell.startedAt() !== null) {
      const sectionIndex = sectionIndexForSlide(options.sections, options.shell.currentIndex);
      if (sectionIndex >= 0) {
        actualMs[sectionIndex] += Math.max(0, detail.previousElapsedMs - lastElapsedMs);
      }
    }
    lastElapsedMs = detail.elapsedMs;
  }
  function tick() {
    if (options.shell.startedAt() === null) {
      actualMs.fill(0);
      lastElapsedMs = 0;
      return;
    }
    flush();
  }
  function flush() {
    flushElapsedToSectionOf(options.shell.currentIndex);
  }
  bus.addEventListener("peitho:slidechange", onSlideChange);
  bus.addEventListener("peitho:timercontrol", onTimerControl);
  bus.addEventListener("peitho:timeradopt", onTimerAdopt);
  const interval = win.setInterval(tick, 250);
  return {
    actualMs: () => actualMs.slice(),
    flush,
    destroy() {
      win.clearInterval(interval);
      bus.removeEventListener("peitho:slidechange", onSlideChange);
      bus.removeEventListener("peitho:timercontrol", onTimerControl);
      bus.removeEventListener("peitho:timeradopt", onTimerAdopt);
    }
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
function dispatchNavigate(bus, to) {
  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to } }));
}
function installKeyboardNavigation(win = window, bus = win) {
  const onKeyDown = (event) => {
    if (hasChordModifier(event)) return;
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
    if (hasChordModifier(event)) return;
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
    if (hasChordModifier(event)) return;
    if (event.key !== "Escape") return;
    event.preventDefault();
    bus.dispatchEvent(new CustomEvent("peitho:closerequest"));
  };
  win.addEventListener("keydown", onKeyDown);
  return () => win.removeEventListener("keydown", onKeyDown);
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
  const clickGuard = createClickNavigationGuard({ target: options.root, window: win });
  const onClick = (event) => {
    if (clickGuard.shouldIgnoreClick(event)) return;
    if (event.target.closest('[data-peitho-control-bar="true"]')) return;
    const to = event.clientX < win.innerWidth / 4 ? "prev" : "next";
    bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to } }));
  };
  options.root.addEventListener("click", onClick);
  return () => {
    clickGuard.destroy();
    options.root.removeEventListener("click", onClick);
  };
}
function installSwipeNavigation(options) {
  const win = options.window ?? window;
  const bus = options.bus ?? win;
  const minHorizontalPx = options.minHorizontalPx ?? 50;
  const maxDurationMs = options.maxDurationMs ?? 800;
  const minRatio = options.minRatio ?? 1.5;
  const clickSuppressPx = minHorizontalPx / 2;
  let active = false;
  let x0 = 0;
  let y0 = 0;
  let t0 = 0;
  const onTouchStart = (event) => {
    if (active) return;
    if (event.touches.length !== 1) return;
    if (event.target.closest('[data-peitho-control-bar="true"]')) return;
    const touch = event.touches[0];
    x0 = touch.clientX;
    y0 = touch.clientY;
    t0 = win.performance.now();
    active = true;
  };
  const onTouchEnd = (event) => {
    if (!active) return;
    active = false;
    const touch = event.changedTouches[0];
    if (!touch) return;
    const dx = touch.clientX - x0;
    const dy = touch.clientY - y0;
    const dt = win.performance.now() - t0;
    if (Math.abs(dx) >= clickSuppressPx) event.preventDefault();
    if (Math.abs(dx) < minHorizontalPx) return;
    if (Math.abs(dx) / Math.max(Math.abs(dy), 1) <= minRatio) return;
    if (dt > maxDurationMs) return;
    bus.dispatchEvent(
      new CustomEvent("peitho:navigate", {
        detail: { to: dx < 0 ? "next" : "prev" }
      })
    );
  };
  const onTouchCancel = () => {
    active = false;
  };
  options.root.addEventListener("touchstart", onTouchStart, { passive: true });
  options.root.addEventListener("touchend", onTouchEnd, { passive: false });
  options.root.addEventListener("touchcancel", onTouchCancel);
  return () => {
    options.root.removeEventListener("touchstart", onTouchStart);
    options.root.removeEventListener("touchend", onTouchEnd);
    options.root.removeEventListener("touchcancel", onTouchCancel);
  };
}
function installFullscreenShortcut(options = {}) {
  const win = options.window ?? window;
  const doc = options.document ?? document;
  const onKeyDown = (event) => {
    if (hasChordModifier(event)) return;
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
function installPointerOverlay(options) {
  const win = options.window ?? window;
  const bus = options.bus ?? win;
  const fetcher = options.fetcher ?? fetch.bind(globalThis);
  const now = options.now ?? Date.now;
  const log = options.console ?? console;
  const canvas = options.canvas;
  const ctx = canvas2dContext(canvas);
  const state = { x: 0, y: 0, visible: false, lastUpAt: -Infinity };
  let closed = false;
  let seq = 0;
  let session = null;
  let frame = null;
  let retryTimer = null;
  const requestFrame = (callback) => {
    if (typeof win.requestAnimationFrame === "function") {
      return win.requestAnimationFrame(callback);
    }
    return win.setTimeout(() => callback(now()), 16);
  };
  const cancelFrame = (handle) => {
    if (typeof win.cancelAnimationFrame === "function") {
      win.cancelAnimationFrame(handle);
      return;
    }
    win.clearTimeout(handle);
  };
  const resizeCanvas = () => {
    const rect = canvas.getBoundingClientRect();
    const fallbackWidth = win.innerWidth || 1;
    const fallbackHeight = win.innerHeight || 1;
    const cssWidth = rect.width > 0 ? rect.width : fallbackWidth;
    const cssHeight = rect.height > 0 ? rect.height : fallbackHeight;
    const scale = win.devicePixelRatio || 1;
    canvas.width = Math.max(1, Math.round(cssWidth * scale));
    canvas.height = Math.max(1, Math.round(cssHeight * scale));
    draw();
  };
  const clearCanvas = () => {
    if (ctx == null) return;
    ctx.clearRect(0, 0, canvas.width, canvas.height);
  };
  const requestDraw = () => {
    if (frame !== null) return;
    frame = requestFrame(() => {
      frame = null;
      draw();
      if (!closed && fadeOpacity(state, now()) > 0) {
        requestDraw();
      }
    });
  };
  const resetState = () => {
    state.visible = false;
    state.lastUpAt = -Infinity;
    clearCanvas();
  };
  const setSession = (nextSession) => {
    if (session !== null && session !== nextSession) {
      resetState();
      session = nextSession;
      return true;
    }
    session = nextSession;
    return false;
  };
  const applyEvent = (event, options2 = {}) => {
    if (event.kind === "move") {
      state.x = event.x;
      state.y = event.y;
      state.visible = true;
      state.lastUpAt = -Infinity;
      requestDraw();
      return;
    }
    if (options2.fadeUp === false) {
      resetState();
      return;
    }
    state.visible = false;
    state.lastUpAt = now();
    requestDraw();
  };
  const delay = () => new Promise((resolve) => {
    retryTimer = win.setTimeout(() => {
      retryTimer = null;
      resolve();
    }, 1e3);
  });
  const handshake = async () => {
    try {
      const response = await fetcher("/pointer");
      if (closed) return false;
      if (!response.ok) {
        log.error(`Failed to start pointer polling: ${response.status}`);
        await delay();
        return false;
      }
      const body = await response.json();
      if (!isPointerHandshakeResponse(body)) {
        log.error("Invalid peitho pointer handshake");
        await delay();
        return false;
      }
      seq = body.seq;
      setSession(body.session);
      return true;
    } catch (error) {
      if (!closed) {
        log.error(`Failed to start pointer polling: ${String(error)}`);
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
      try {
        const response = await fetcher(`/pointer?seq=${seq}`);
        if (closed) return;
        if (response.status === 204) continue;
        if (!response.ok) {
          log.error(`Failed to poll pointer message: ${response.status}`);
          await delay();
          continue;
        }
        const body = pointerPollResponse(await response.json());
        if (body == null) {
          log.error("Invalid peitho pointer message");
          await delay();
          continue;
        }
        seq = body.seq;
        const sessionChanged = setSession(body.session);
        applyEvent(body.event, { fadeUp: !(sessionChanged && body.event.kind === "up") });
      } catch (error) {
        if (!closed) {
          log.error(`Failed to poll pointer message: ${String(error)}`);
          needsHandshake = true;
          await delay();
        }
      }
    }
  };
  const onNavigate = () => resetState();
  if (ctx != null) {
    resizeCanvas();
    win.addEventListener("resize", resizeCanvas);
    bus.addEventListener("peitho:navigate", onNavigate);
    void poll();
  }
  return () => {
    closed = true;
    bus.removeEventListener("peitho:navigate", onNavigate);
    win.removeEventListener("resize", resizeCanvas);
    if (frame !== null) {
      cancelFrame(frame);
      frame = null;
    }
    if (retryTimer !== null) {
      win.clearTimeout(retryTimer);
      retryTimer = null;
    }
    clearCanvas();
  };
  function draw() {
    if (ctx == null) return;
    clearCanvas();
    const opacity = fadeOpacity(state, now());
    if (opacity <= 0) return;
    const x = state.x * canvas.width;
    const y = state.y * canvas.height;
    const radius = 0.012 * Math.min(canvas.width, canvas.height);
    const ringRadius = radius + 2;
    ctx.save();
    ctx.globalAlpha = opacity;
    ctx.fillStyle = "#ffffff";
    ctx.beginPath();
    ctx.arc(x, y, ringRadius, 0, Math.PI * 2);
    ctx.fill();
    ctx.fillStyle = "#ff2a2a";
    ctx.beginPath();
    ctx.arc(x, y, radius, 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();
  }
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
  pointerCleanup = null;
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
      this.mountPointerOverlay();
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
    this.pointerCleanup?.();
    this.pointerCleanup = null;
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
  mountPointerOverlay() {
    if (this.viewport != null) return;
    const canvas = this.doc.createElement("canvas");
    canvas.dataset.peithoPointerOverlay = "true";
    canvas.style.position = "absolute";
    canvas.style.inset = "0";
    canvas.style.zIndex = "4";
    canvas.style.pointerEvents = "none";
    canvas.style.width = "100%";
    canvas.style.height = "100%";
    this.root.appendChild(canvas);
    this.pointerCleanup = installPointerOverlay({
      canvas,
      fetcher: this.fetcher,
      bus: this.bus,
      window: this.win,
      now: this.now,
      console: this.log
    });
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
function fadeOpacity(state, nowMs) {
  if (state.visible) return 1;
  if (!Number.isFinite(state.lastUpAt)) return 0;
  return Math.max(0, Math.min(1, 1 - (nowMs - state.lastUpAt) / 150));
}
function canvas2dContext(canvas) {
  try {
    return canvas.getContext("2d");
  } catch (_error) {
    return null;
  }
}
function isPointerHandshakeResponse(value) {
  return hasExactKeys(value, ["seq", "session"]) && typeof value.seq === "number" && Number.isFinite(value.seq) && typeof value.session === "string";
}
function pointerPollResponse(value) {
  if (!hasExactKeys(value, ["seq", "event", "session"]) || typeof value.seq !== "number" || !Number.isFinite(value.seq) || typeof value.session !== "string") {
    return null;
  }
  const event = pointerOverlayEvent(value.event);
  if (event == null) return null;
  return { seq: value.seq, event, session: value.session };
}
function pointerOverlayEvent(value) {
  if (!isRecord(value)) return null;
  if (hasExactKeys(value, ["up"])) {
    return value.up === true ? { kind: "up" } : null;
  }
  const keys = Object.keys(value);
  if (keys.length !== 1 || !Object.hasOwn(value, "move")) {
    return null;
  }
  const move = value.move;
  if (!hasExactKeys(move, ["x", "y"])) {
    return null;
  }
  if (!isUnitCoordinate(move.x) || !isUnitCoordinate(move.y)) {
    return null;
  }
  return { kind: "move", x: move.x, y: move.y };
}
function hasExactKeys(value, keys) {
  if (!isRecord(value)) return false;
  const actual = Object.keys(value);
  return actual.length === keys.length && keys.every((key) => Object.hasOwn(value, key));
}
function isRecord(value) {
  return typeof value === "object" && value !== null;
}
function isUnitCoordinate(value) {
  return typeof value === "number" && Number.isFinite(value) && value >= 0 && value <= 1;
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
function swapRoute(pathname) {
  return SWAP_ROUTES[pathname] ?? null;
}
function installSwapShortcut(win = window, bus = win) {
  const onKeyDown = (event) => {
    if (hasChordModifier(event)) return;
    if (event.key !== "s" && event.key !== "S") return;
    if (event.repeat) return;
    event.preventDefault();
    bus.dispatchEvent(new CustomEvent("peitho:swaprequest"));
  };
  win.addEventListener("keydown", onKeyDown);
  return () => win.removeEventListener("keydown", onKeyDown);
}

// src/sync.ts
function isRecord2(value) {
  return typeof value === "object" && value !== null;
}
function isCloseSyncMessage(value) {
  return isRecord2(value) && value.close === true;
}
function isIndexSyncMessage(value) {
  return isRecord2(value) && typeof value.index === "number" && Number.isFinite(value.index);
}
function isSwappedSyncMessage(value) {
  return isRecord2(value) && typeof value.swapped === "boolean";
}
function isSyncedSyncMessage(value) {
  return isRecord2(value) && value.synced === true;
}
function isSessionChangedSyncMessage(value) {
  return isRecord2(value) && value.sessionChanged === true;
}
function isNonNegativeFiniteNumber(value) {
  return typeof value === "number" && Number.isFinite(value) && value >= 0;
}
function isTimerSyncMessage(value) {
  return isRecord2(value) && isRecord2(value.timer) && typeof value.timer.running === "boolean" && isNonNegativeFiniteNumber(value.timer.elapsedMs);
}
function isTimerReplaySyncMessage(value) {
  return isRecord2(value) && isRecord2(value.timer) && typeof value.timer.running === "boolean" && isNonNegativeFiniteNumber(value.timer.elapsedMs) && isNonNegativeFiniteNumber(value.timer.atMs) && isNonNegativeFiniteNumber(value.nowMs);
}
function isGenerationSyncMessage(value) {
  return isRecord2(value) && typeof value.generation === "number" && Number.isFinite(value.generation);
}
function defaultChannelFactory(name) {
  const channel = new BroadcastChannel(name);
  let onmessage = null;
  let syncedDelivered = false;
  const deliverSynced = () => {
    if (syncedDelivered || onmessage == null) return;
    syncedDelivered = true;
    onmessage({ data: { synced: true } });
  };
  queueMicrotask(deliverSynced);
  channel.onmessage = (event) => {
    onmessage?.({ data: event.data });
  };
  return {
    get onmessage() {
      return onmessage;
    },
    set onmessage(next) {
      onmessage = next;
      deliverSynced();
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
function installSyncBridge(win = window, channelFactory = defaultChannelFactory, bus = win, hooks = {}) {
  const channel = channelFactory("peitho-sync");
  const closeWindow = hooks.closeWindow ?? (() => win.close());
  const pathname = hooks.pathname ?? (() => win.location.pathname);
  const navigate = hooks.navigate ?? ((url) => win.location.replace(url));
  let synced = false;
  const onSlideChange = (event) => {
    const detail = event.detail;
    if (typeof detail?.index !== "number") return;
    channel.postMessage({ index: detail.index });
  };
  const onCloseRequest = () => {
    channel.postMessage({ close: true });
  };
  const onSwapRequest = () => {
    const route = swapRoute(pathname());
    if (route == null) {
      console.error("peitho: swap unavailable on this route");
      return;
    }
    channel.postMessage({ swapped: !route.swapped });
  };
  const onTimerChange = (event) => {
    if (!synced) return;
    const detail = event.detail;
    if (typeof detail?.running !== "boolean" || !isNonNegativeFiniteNumber(detail.elapsedMs)) {
      console.error("Invalid peitho:timerchange event");
      return;
    }
    channel.postMessage({
      timer: { running: detail.running, elapsedMs: Math.round(detail.elapsedMs) }
    });
  };
  channel.onmessage = (event) => {
    const data = event.data;
    if (isSyncedSyncMessage(data)) {
      synced = true;
      return;
    }
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
    if (isSwappedSyncMessage(data)) {
      const route = swapRoute(pathname());
      if (route == null) {
        console.error("peitho: swap unavailable on this route");
        return;
      }
      if (data.swapped === route.swapped) return;
      navigate(route.counterpart);
      return;
    }
    if (isGenerationSyncMessage(data)) {
      return;
    }
    if (isSessionChangedSyncMessage(data)) {
      return;
    }
    if (isTimerReplaySyncMessage(data)) {
      if (hooks.adoptTimerState) {
        const serverElapsed = data.timer.elapsedMs + (data.timer.running ? Math.max(0, data.nowMs - data.timer.atMs) : 0);
        hooks.adoptTimerState({ running: data.timer.running, elapsedMs: serverElapsed });
      }
      return;
    }
    if (isTimerSyncMessage(data)) {
      return;
    }
    console.error("Invalid peitho sync message");
  };
  bus.addEventListener("peitho:slidechange", onSlideChange);
  bus.addEventListener("peitho:closerequest", onCloseRequest);
  bus.addEventListener("peitho:swaprequest", onSwapRequest);
  bus.addEventListener("peitho:timerchange", onTimerChange);
  return () => {
    bus.removeEventListener("peitho:slidechange", onSlideChange);
    bus.removeEventListener("peitho:closerequest", onCloseRequest);
    bus.removeEventListener("peitho:swaprequest", onSwapRequest);
    bus.removeEventListener("peitho:timerchange", onTimerChange);
    channel.onmessage = null;
    channel.close();
  };
}

// src/timerUrgency.ts
function urgencyFor(elapsedMs, plannedDurationMs) {
  if (plannedDurationMs == null) return "normal";
  if (isOverrun(elapsedMs, plannedDurationMs)) return "overrun";
  const remainingMs = plannedDurationMs - elapsedMs;
  if (remainingMs <= 6e4) return "urgent";
  if (remainingMs <= 18e4) return "warning";
  return "normal";
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
async function mountPresenterView(options) {
  const win = options.window ?? window;
  const doc = options.document ?? document;
  const fetcher = options.fetcher ?? fetch.bind(globalThis);
  const now = options.now ?? Date.now;
  const rawLog = options.console ?? console;
  const log = { error: rawLog.error };
  const bus = win;
  const previewBus = new EventTarget();
  options.root.innerHTML = `
    <section class="peitho-presenter app" data-screen-label="Presenter view">
      <section class="left" aria-label="Current slide and notes">
        <div class="stage">
          <header class="colhead">
            <div class="status-line">
              <span data-peitho-presenter="position">Slide 00 of 00</span>
              <span class="sep" data-peitho-presenter="section-sep" hidden></span>
              <span data-peitho-presenter="section" hidden></span>
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
              <span class="grp"><span class="kbd">S</span> swap</span>
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
              <div data-peitho-presenter="preview-end" hidden>
                <div class="eod-top mono"><span>Peitho</span></div>
                <div class="eod-center">
                  <div class="eod-fin mono"><span class="eod-rule"></span><span>Fin</span><span class="eod-rule"></span></div>
                  <div class="eod-title">End of deck</div>
                </div>
                <div class="eod-bottom mono"><span>&mdash;</span><span>&mdash;</span></div>
              </div>
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
            <button class="btn" type="button" data-peitho-action="swap">Swap <span class="k">S</span></button>
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
  const sectionLabel = options.root.querySelector(
    '[data-peitho-presenter="section"]'
  );
  const sectionSep = options.root.querySelector(
    '[data-peitho-presenter="section-sep"]'
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
  const syncCleanup = installSyncBridge(
    win,
    options.syncChannelFactory,
    bus,
    {
      adoptTimerState: (state) => {
        mainShell.adoptTimerState(state);
        tick();
      }
    }
  );
  const rawPlannedDurationMs = mainShell.manifest?.plannedDurationMs ?? null;
  const plannedDurationMs = rawPlannedDurationMs != null && isValidDurationMs(rawPlannedDurationMs) ? rawPlannedDurationMs : null;
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
  const manifestSections = mainShell.manifest?.sections ?? [];
  const sections = validateSections(manifestSections, log) ? manifestSections : [];
  const sectionActuals = installSectionActuals({
    shell: mainShell,
    sections,
    bus,
    window: win,
    log
  });
  const agendaCleanup = installAgenda({
    root: agendaSlot,
    shell: mainShell,
    sections,
    actuals: sectionActuals,
    bus,
    window: win,
    document: doc,
    log
  });
  const rehearsalReporterCleanup = installRehearsalReporter({
    actuals: sectionActuals,
    shell: mainShell,
    sections,
    bus,
    window: win
  });
  const rehearsalBridgeCleanup = installRehearsalBridge(win, bus, fetcher);
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
    const nextUrgency = urgencyFor(elapsedMs, plannedDurationMs);
    if (clockRoot.dataset.peithoUrgency !== nextUrgency) {
      clockRoot.dataset.peithoUrgency = nextUrgency;
    }
    setTimerStateChrome(deriveTimerState(mainShell));
  }
  function updateFromSlide(detail) {
    const slideNotes = options.notes.notes[detail.key];
    notesRoot.textContent = slideNotes ?? "No notes for this slide.";
    notesRoot.classList.toggle("is-empty", slideNotes == null);
    const slideNumber = detail.index + 1;
    const slide = formatSlideNumber(slideNumber);
    const total = formatSlideNumber(detail.total);
    deckTitle.textContent = mainShell.manifest?.title ?? "Peitho Deck";
    positionLong.textContent = `Slide ${slide} of ${total}`;
    positionShort.textContent = `${slide} / ${total}`;
    notesSlide.textContent = `Slide ${slide}`;
    const currentSectionIndex = sectionIndexForSlide(sections, detail.index);
    if (currentSectionIndex >= 0) {
      sectionLabel.textContent = `Section \u2014 \u201C${sections[currentSectionIndex].name}\u201D`;
      sectionLabel.hidden = false;
      sectionSep.hidden = false;
    } else {
      sectionLabel.textContent = "";
      sectionLabel.hidden = true;
      sectionSep.hidden = true;
    }
    const nextIndex = nextNonSkippedIndex(mainShell.manifest?.slides ?? [], detail.index, 1);
    if (nextIndex !== null) {
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
  addButtonListener("swap", () => {
    bus.dispatchEvent(new CustomEvent("peitho:swaprequest"));
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
      rehearsalBridgeCleanup();
      rehearsalReporterCleanup();
      agendaCleanup();
      sectionActuals.destroy();
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
  PRESENTER_URL,
  calculateCanvasFit,
  fallbackFeatures,
  formatMinuteSeconds,
  hasChordModifier,
  installAgenda,
  installCanvasClickNavigation,
  installCanvasScaler,
  installCloseOnEscape,
  installFullscreenShortcut,
  installKeyboardNavigation,
  installPointerOverlay,
  installPresentationControls,
  installPresenterKeyboard,
  installRehearsalBridge,
  installRehearsalReporter,
  installSectionActuals,
  installSwapShortcut,
  installSwipeNavigation,
  installSyncBridge,
  installTimeTracker,
  isOverrun,
  isValidDurationMs,
  mountPresentShell,
  mountPresenterView,
  openPresenterPopup,
  serverSyncChannelFactory,
  swapRoute,
  toggleFullscreen
};
//# sourceMappingURL=shell.js.map
