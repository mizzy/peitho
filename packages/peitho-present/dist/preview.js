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
function isGenerationSyncMessage(value) {
  return isRecord(value) && typeof value.generation === "number";
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
    const deliverReplayState = (body) => {
      if (typeof body.index === "number") {
        onmessage?.({ data: { index: body.index } });
      }
      if (typeof body.swapped === "boolean") {
        onmessage?.({ data: { swapped: body.swapped } });
      }
      if (typeof body.generation === "number") {
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
        seq = body.seq;
        deliverReplayState(body);
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
          deliverReplayState(body);
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

// src/preview.ts
var PREVIEW_STATE_KEY = "peitho:preview-state";
var GRID_TILE_WIDTH = 320;
var GRID_GAP = 18;
var previewNavigationKeyMap = /* @__PURE__ */ new Map([
  ["ArrowRight", "next"],
  ["PageDown", "next"],
  ["ArrowLeft", "prev"],
  ["PageUp", "prev"],
  ["Home", "first"],
  ["End", "last"]
]);
function installPreviewKeyboard(win = window, bus = win) {
  const onKeyDown = (event) => {
    if (hasChordModifier(event)) return;
    if (event.key === "o") {
      event.preventDefault();
      dispatchOverviewRequest(bus, "toggle");
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      dispatchOverviewRequest(bus, "exit");
      return;
    }
    if (event.key === "Enter") {
      event.preventDefault();
      dispatchOverviewRequest(bus, "activate");
      return;
    }
    const to = previewNavigationKeyMap.get(event.key);
    if (!to) return;
    event.preventDefault();
    bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to } }));
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
    if (!isGenerationSyncMessage(event.data)) return;
    if (event.data.generation === shell.generation) return;
    shell.saveState();
    reload();
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
  mode = "single";
  generation = 0;
  root;
  fetcher;
  win;
  doc;
  log;
  bus;
  storage;
  syncUrl;
  viewport;
  restoredState;
  slides = [];
  dimensions = { width: 1280, height: 720 };
  onNavigate = (event) => {
    if (!this.isLoaded()) return;
    const detail = event.detail;
    if (!detail || !("to" in detail)) {
      this.log.error("Invalid peitho:navigate event");
      return;
    }
    this.navigate(detail.to);
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
  onResize = () => this.applyLayout();
  constructor(options) {
    this.root = options.root;
    this.fetcher = options.fetcher ?? fetch.bind(globalThis);
    this.win = options.window ?? window;
    this.doc = options.document ?? document;
    this.log = options.console ?? console;
    this.bus = options.bus ?? this.win;
    this.storage = options.storage ?? this.win.sessionStorage;
    this.syncUrl = options.syncUrl ?? "/sync";
    this.viewport = options.viewport;
    this.restoredState = this.readState();
    this.root.classList.add("peitho-preview-root");
    const rootPosition = this.win.getComputedStyle(this.root).position;
    if (rootPosition === "static" || rootPosition === "") {
      this.root.style.position = "relative";
    }
    this.root.style.background = "#000";
    this.bus.addEventListener("peitho:navigate", this.onNavigate);
    this.bus.addEventListener("peitho:overviewrequest", this.onOverviewRequest);
    this.win.addEventListener("resize", this.onResize);
  }
  async load() {
    try {
      this.generation = await this.fetchGeneration();
      const manifest = await this.fetchJson("manifest.json");
      this.dimensions = {
        width: manifest.canvasWidth,
        height: manifest.canvasHeight
      };
      const cssAspect = manifest.aspectRatio.replace(":", " / ");
      this.setCanvasRootProperties(this.dimensions, cssAspect);
      const css = await this.fetchText("peitho.css");
      const pending = await Promise.all(
        manifest.slides.map(async (slide) => {
          const html = await this.fetchText(slide.src);
          return this.createSlideView(slide, html, css);
        })
      );
      this.manifest = manifest;
      this.root.replaceChildren();
      for (const view of pending) {
        this.root.appendChild(view.tile);
        this.slides.push(view);
      }
      const restored = this.restoredState;
      const restoredIndex = this.clampIndex(restored?.index ?? 0);
      this.currentIndex = restoredIndex;
      this.selectedIndex = restoredIndex;
      this.mode = restored?.mode ?? "single";
      this.applyLayout();
      this.dispatchSlideChange(null);
    } catch (error) {
      this.clearCanvasRootProperties();
      this.root.replaceChildren();
      this.root.textContent = error instanceof Error ? error.message : String(error);
    }
  }
  navigate(to) {
    if (!this.isLoaded()) return;
    const index = this.resolveTarget(to);
    if (index === null) return;
    this.setIndex(index);
  }
  saveState() {
    const index = this.clampIndex(this.selectedIndex >= 0 ? this.selectedIndex : this.currentIndex);
    try {
      this.storage?.setItem(PREVIEW_STATE_KEY, JSON.stringify({ mode: this.mode, index }));
    } catch (error) {
      this.log.error(`Failed to save preview state: ${String(error)}`);
    }
  }
  destroy() {
    this.bus.removeEventListener("peitho:navigate", this.onNavigate);
    this.bus.removeEventListener("peitho:overviewrequest", this.onOverviewRequest);
    this.win.removeEventListener("resize", this.onResize);
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
  async fetchGeneration() {
    const response = await this.fetchOk(this.syncUrl);
    const body = await response.json();
    if (typeof body.generation !== "number") {
      throw new Error("Invalid peitho sync generation");
    }
    return body.generation;
  }
  createSlideView(slide, html, css) {
    const tile = this.doc.createElement("div");
    tile.classList.add("peitho-preview-tile");
    tile.dataset.slideKey = slide.key;
    tile.dataset.slideIndex = String(slide.index);
    tile.addEventListener("click", () => {
      this.setIndex(slide.index);
      this.exitGrid();
    });
    const host = this.doc.createElement("section");
    host.classList.add("peitho-preview-slide");
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
    tile.appendChild(host);
    return { meta: slide, tile, host };
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
  toggleOverview() {
    if (this.mode === "grid") this.exitGrid();
    else this.enterGrid();
  }
  enterGrid() {
    if (this.mode === "grid") return;
    this.mode = "grid";
    this.selectedIndex = this.clampIndex(this.currentIndex);
    this.applyLayout();
    this.saveState();
  }
  exitGrid() {
    if (this.mode === "single") return;
    this.mode = "single";
    this.selectedIndex = this.clampIndex(this.selectedIndex);
    this.currentIndex = this.selectedIndex;
    this.applyLayout();
    this.saveState();
  }
  activateSelection() {
    if (this.mode !== "grid") return;
    this.exitGrid();
  }
  setIndex(index) {
    const next = this.clampIndex(index);
    if (next === this.currentIndex && next === this.selectedIndex) return;
    const previousIndex = this.currentIndex < 0 ? null : this.currentIndex;
    this.currentIndex = next;
    this.selectedIndex = next;
    this.applyLayout();
    this.dispatchSlideChange(previousIndex);
    this.saveState();
  }
  resolveTarget(to) {
    if (to === "first") return 0;
    if (to === "last") return this.slides.length - 1;
    if (to === "next") return Math.min(this.selectedIndex + 1, this.slides.length - 1);
    if (to === "prev") return Math.max(this.selectedIndex - 1, 0);
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
  clampIndex(index) {
    if (this.slides.length === 0) return 0;
    return Math.min(Math.max(index, 0), this.slides.length - 1);
  }
  applyLayout() {
    this.root.dataset.peithoPreviewMode = this.mode;
    if (this.mode === "grid") this.applyGridLayout();
    else this.applySingleLayout();
  }
  applySingleLayout() {
    const viewport = this.viewport?.() ?? {
      width: this.win.innerWidth,
      height: this.win.innerHeight
    };
    const fit = calculateCanvasFit(viewport, this.dimensions.width, this.dimensions.height);
    this.root.style.display = "block";
    this.root.style.overflow = "hidden";
    this.root.style.padding = "0";
    this.root.style.gap = "0";
    this.root.style.gridTemplateColumns = "";
    this.slides.forEach((slide, index) => {
      const active = index === this.currentIndex;
      slide.tile.hidden = !active;
      slide.tile.classList.toggle("is-selected", active);
      slide.tile.style.position = "absolute";
      slide.tile.style.left = "0";
      slide.tile.style.top = "0";
      slide.tile.style.width = "100%";
      slide.tile.style.height = "100%";
      slide.tile.style.overflow = "hidden";
      slide.tile.style.border = "0";
      slide.tile.style.borderRadius = "0";
      slide.tile.style.background = "transparent";
      slide.host.hidden = !active;
      this.applyHostFrame(slide.host, fit.left, fit.top, fit.scale);
    });
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
    this.root.style.padding = "24px";
    this.root.style.boxSizing = "border-box";
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
      slide.tile.style.border = selected ? "3px solid #7dd3fc" : "1px solid rgba(255,255,255,0.24)";
      slide.tile.style.borderRadius = "6px";
      slide.tile.style.background = "#000";
      slide.tile.style.cursor = "pointer";
      slide.tile.style.boxSizing = "content-box";
      slide.host.hidden = false;
      this.applyHostFrame(slide.host, 0, 0, scale);
    });
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
        return { mode: parsed.mode, index: parsed.index };
      }
    } catch (_error) {
      return null;
    }
    return null;
  }
};
export {
  installPreviewKeyboard,
  installPreviewReload,
  mountPreviewShell
};
//# sourceMappingURL=preview.js.map
