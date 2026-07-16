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
    let abortController = null;
    let retryTimer = null;
    const deliverReplayState = (body) => {
      if (isIndexSyncMessage(body)) {
        onmessage?.({ data: { index: body.index } });
      }
      if (isSwappedSyncMessage(body)) {
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

// src/remote.ts
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
  const counter = doc.createElement("div");
  counter.className = "peitho-remote-counter";
  counter.dataset.peithoRemote = "counter";
  counter.textContent = "\u2013 / \u2013";
  const status = doc.createElement("div");
  status.className = "peitho-remote-status";
  status.dataset.peithoRemote = "status";
  const actions = doc.createElement("div");
  actions.className = "peitho-remote-actions";
  const prev = remoteButton(doc, "prev", "Previous");
  const next = remoteButton(doc, "next", "Next");
  const onPrev = () => dispatchNavigate(bus, "prev");
  const onNext = () => dispatchNavigate(bus, "next");
  prev.addEventListener("click", onPrev);
  next.addEventListener("click", onNext);
  actions.append(prev, next);
  container.append(counter, status, actions);
  root.append(container);
  return () => {
    prev.removeEventListener("click", onPrev);
    next.removeEventListener("click", onNext);
    container.remove();
  };
}
function installRemoteSyncBridge(options) {
  const bus = options.bus ?? window;
  const log = options.console ?? console;
  const channel = (options.channelFactory ?? serverSyncChannelFactory())("peitho-sync");
  const onNavigate = (event) => {
    const to = event.detail?.to;
    if (to !== "next" && to !== "prev") return;
    const target = resolveRemoteTarget(options.slides, options.getCurrentIndex(), to);
    if (target === null) return;
    channel.postMessage({ index: target });
    options.setCurrentIndex(target);
  };
  channel.onmessage = (event) => {
    const data = event.data;
    if (isCloseSyncMessage(data)) {
      options.setEnded();
      return;
    }
    if (isIndexSyncMessage(data)) {
      options.setCurrentIndex(data.index);
      return;
    }
    if (isSwappedSyncMessage(data) || isGenerationSyncMessage(data)) return;
    log.error("Invalid peitho remote sync message");
  };
  bus.addEventListener("peitho:navigate", onNavigate);
  return () => {
    bus.removeEventListener("peitho:navigate", onNavigate);
    channel.onmessage = null;
    channel.close();
  };
}
var RemoteController = class {
  manifest = null;
  currentIndex = null;
  root;
  manifestUrl;
  fetcher;
  channelFactory;
  win;
  doc;
  bus;
  log;
  slides = [];
  controlsCleanup = null;
  syncCleanup = null;
  constructor(options) {
    this.root = options.root;
    this.manifestUrl = options.manifestUrl ?? "manifest.json";
    this.fetcher = options.fetcher ?? fetch.bind(globalThis);
    this.channelFactory = options.channelFactory;
    this.win = options.window ?? window;
    this.doc = options.document ?? document;
    this.bus = options.bus ?? this.win;
    this.log = options.console ?? console;
  }
  async load() {
    try {
      const manifest = await this.fetchJson(this.manifestUrl);
      this.manifest = manifest;
      this.slides = manifest.slides.map((slide) => ({ skip: slide.skip === true }));
      this.currentIndex = initialSlideIndex(this.slides);
      this.controlsCleanup = installRemoteControls({
        root: this.root,
        document: this.doc,
        bus: this.bus
      });
      this.renderCounter();
      this.syncCleanup = installRemoteSyncBridge({
        slides: this.slides,
        channelFactory: this.channelFactory,
        bus: this.bus,
        getCurrentIndex: () => this.currentIndex,
        setCurrentIndex: (index) => this.setCurrentIndex(index),
        setEnded: () => this.setEnded(),
        console: this.log
      });
    } catch (error) {
      this.showError(error instanceof Error ? error.message : String(error));
    }
  }
  destroy() {
    this.syncCleanup?.();
    this.syncCleanup = null;
    this.controlsCleanup?.();
    this.controlsCleanup = null;
  }
  async fetchJson(url) {
    const response = await this.fetcher(url);
    if (!response.ok) throw new Error(`Failed to load ${url}: ${response.status}`);
    return response.json();
  }
  setCurrentIndex(index) {
    this.currentIndex = clampIndex(index, this.slides.length);
    this.renderCounter();
  }
  setEnded() {
    const cleanup = this.syncCleanup;
    this.syncCleanup = null;
    cleanup?.();
    for (const button of this.root.querySelectorAll("[data-peitho-action]")) {
      button.disabled = true;
    }
    const status = this.root.querySelector('[data-peitho-remote="status"]');
    if (status != null) status.textContent = "Ended";
  }
  renderCounter() {
    const counter = this.root.querySelector('[data-peitho-remote="counter"]');
    if (counter == null) return;
    const total = this.slides.length;
    counter.textContent = this.currentIndex === null ? `\u2013 / ${total}` : `${this.currentIndex + 1} / ${total}`;
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
  button.dataset.peithoAction = action;
  button.textContent = label;
  return button;
}
function dispatchNavigate(bus, to) {
  bus.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to } }));
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
export {
  installRemoteControls,
  installRemoteSyncBridge,
  mountRemoteView
};
//# sourceMappingURL=remote.js.map
