import { swapRoute } from "./swap";

export type TimerSyncState = { running: boolean; elapsedMs: number };
export type TimerSyncSnapshot = TimerSyncState & { atMs: number };
export type TimerSyncMessage = { timer: TimerSyncState };
export type TimerReplaySyncMessage = { timer: TimerSyncSnapshot; nowMs: number };
export type SyncedSyncMessage = { synced: true };

export type SyncMessage =
  | { index: number }
  | { swapped: boolean }
  | TimerSyncMessage
  | { close: true };

export type SyncChannel = {
  onmessage: ((event: { data: unknown }) => void) | null;
  postMessage(message: SyncMessage): void;
  close(): void;
};

export type SyncChannelFactory = (name: string) => SyncChannel;

export type ServerSyncOptions = {
  url?: string;
  fetcher?: typeof fetch;
  retryMs?: number;
  setTimeoutFn?: Window["setTimeout"];
  clearTimeoutFn?: Window["clearTimeout"];
  AbortControllerCtor?: typeof AbortController;
};

export type SyncBridgeHooks = {
  closeWindow?: () => void;
  pathname?: () => string;
  navigate?: (url: string) => void;
  adoptTimerState?: (state: TimerSyncState) => void;
};

type ServerSyncPollResponse = {
  seq: number;
  message: unknown;
  index?: unknown;
  swapped?: unknown;
  generation?: unknown;
  timer?: unknown;
  nowMs?: unknown;
};

type BufferedTimerReplay = {
  seq: number;
  data: TimerReplaySyncMessage;
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

export function isCloseSyncMessage(value: unknown): value is { close: true } {
  return isRecord(value) && value.close === true;
}

export function isIndexSyncMessage(value: unknown): value is { index: number } {
  return isRecord(value) && typeof value.index === "number" && Number.isFinite(value.index);
}

export function isSwappedSyncMessage(value: unknown): value is { swapped: boolean } {
  return isRecord(value) && typeof value.swapped === "boolean";
}

export function isSyncedSyncMessage(value: unknown): value is SyncedSyncMessage {
  return isRecord(value) && value.synced === true;
}

function isNonNegativeFiniteNumber(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value) && value >= 0;
}

export function isTimerSyncMessage(value: unknown): value is TimerSyncMessage {
  return (
    isRecord(value) &&
    isRecord(value.timer) &&
    typeof value.timer.running === "boolean" &&
    isNonNegativeFiniteNumber(value.timer.elapsedMs)
  );
}

export function isTimerReplaySyncMessage(value: unknown): value is TimerReplaySyncMessage {
  return (
    isRecord(value) &&
    isRecord(value.timer) &&
    typeof value.timer.running === "boolean" &&
    isNonNegativeFiniteNumber(value.timer.elapsedMs) &&
    isNonNegativeFiniteNumber(value.timer.atMs) &&
    isNonNegativeFiniteNumber(value.nowMs)
  );
}

export function isGenerationSyncMessage(value: unknown): value is { generation: number } {
  return (
    isRecord(value) &&
    typeof value.generation === "number" &&
    Number.isFinite(value.generation)
  );
}

function defaultChannelFactory(name: string): SyncChannel {
  const channel = new BroadcastChannel(name);
  let onmessage: ((event: { data: unknown }) => void) | null = null;
  let syncedDelivered = false;
  const deliverSynced = (): void => {
    if (syncedDelivered || onmessage == null) return;
    syncedDelivered = true;
    onmessage({ data: { synced: true } });
  };
  queueMicrotask(deliverSynced);
  channel.onmessage = (event: MessageEvent): void => {
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
    postMessage(message: SyncMessage): void {
      channel.postMessage(message);
    },
    close(): void {
      channel.close();
    }
  };
}

export function serverSyncChannelFactory(options: ServerSyncOptions = {}): SyncChannelFactory {
  const url = options.url ?? "/sync";
  const fetcher = options.fetcher ?? fetch.bind(globalThis);
  const retryMs = options.retryMs ?? 1000;
  const setTimeoutFn = options.setTimeoutFn ?? window.setTimeout.bind(window);
  const clearTimeoutFn = options.clearTimeoutFn ?? window.clearTimeout.bind(window);
  const AbortControllerCtor = options.AbortControllerCtor ?? AbortController;

  return () => {
    let onmessage: ((event: { data: unknown }) => void) | null = null;
    let closed = false;
    let seq = 0;
    let synced = false;
    let highestAckedPostSeq = 0;
    let pendingTimerPosts = 0;
    let bufferedTimerReplay: BufferedTimerReplay | null = null;
    let abortController: AbortController | null = null;
    let retryTimer: number | null = null;

    const flushBufferedTimerReplay = (): void => {
      if (closed || pendingTimerPosts > 0 || bufferedTimerReplay == null) return;
      const replay = bufferedTimerReplay;
      bufferedTimerReplay = null;
      if (replay.seq >= highestAckedPostSeq) {
        onmessage?.({ data: replay.data });
      }
    };

    const resetSessionState = (): void => {
      synced = false;
      highestAckedPostSeq = 0;
      bufferedTimerReplay = null;
      pendingTimerPosts = 0;
    };

    const deliverReplayState = (
      body: Partial<ServerSyncPollResponse>,
      options: { skipAbsoluteState?: boolean; deferTimerReplay?: boolean } = {}
    ): void => {
      const skipAbsoluteState = options.skipAbsoluteState === true;
      const responseSeq = typeof body.seq === "number" && Number.isFinite(body.seq) ? body.seq : 0;
      if (isTimerReplaySyncMessage(body)) {
        if (skipAbsoluteState) {
          bufferedTimerReplay = null;
        } else if (options.deferTimerReplay === true) {
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

    const delay = (): Promise<void> =>
      new Promise((resolve) => {
        retryTimer = setTimeoutFn(() => {
          retryTimer = null;
          resolve();
        }, retryMs);
      });

    const handshake = async (): Promise<boolean> => {
      try {
        const response = await fetcher(url);
        if (closed) return false;
        if (!response.ok) {
          console.error(`Failed to start sync polling: ${response.status}`);
          await delay();
          return false;
        }
        const body = (await response.json()) as Partial<ServerSyncPollResponse>;
        if (typeof body.seq !== "number") {
          console.error("Invalid peitho sync handshake");
          await delay();
          return false;
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
      } catch (error: unknown) {
        if (!closed) {
          console.error(`Failed to start sync polling: ${String(error)}`);
          await delay();
        }
        return false;
      }
    };

    const poll = async (): Promise<void> => {
      let needsHandshake = true;
      while (!closed) {
        while (!closed && needsHandshake && !(await handshake())) {
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
          const body = (await response.json()) as Partial<ServerSyncPollResponse>;
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
        } catch (error: unknown) {
          if (!closed) {
            console.error(`Failed to poll sync message: ${String(error)}`);
            needsHandshake = true;
            resetSessionState();
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
      postMessage(message: SyncMessage): void {
        const isTimerPost = isTimerSyncMessage(message);
        if (isTimerPost) pendingTimerPosts += 1;
        const completeTimerPost = (): void => {
          if (!isTimerPost) return;
          pendingTimerPosts = Math.max(0, pendingTimerPosts - 1);
          flushBufferedTimerReplay();
        };
        let request: Promise<Response>;
        try {
          request = fetcher(url, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify(message),
            keepalive: true
          });
        } catch (error: unknown) {
          completeTimerPost();
          console.error(`Failed to post sync message: ${String(error)}`);
          return;
        }
        void request
          .then(async (response) => {
            if (!response.ok) {
              console.error(`Failed to post sync message: ${response.status}`);
              return;
            }
            try {
              const body = (await response.json()) as Partial<{ seq: unknown }>;
              if (typeof body.seq === "number" && Number.isFinite(body.seq)) {
                highestAckedPostSeq = Math.max(highestAckedPostSeq, body.seq);
              }
            } catch (_error: unknown) {
              // Older dev servers returned 204; lack of an ack simply disables stale-replay filtering.
            }
          })
          .catch((error: unknown) => {
            console.error(`Failed to post sync message: ${String(error)}`);
          })
          .finally(() => {
            completeTimerPost();
          });
      },
      close(): void {
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

export function installSyncBridge(
  win: Window = window,
  channelFactory: SyncChannelFactory = defaultChannelFactory,
  bus: EventTarget = win,
  hooks: SyncBridgeHooks = {}
): () => void {
  const channel = channelFactory("peitho-sync");
  const closeWindow = hooks.closeWindow ?? (() => win.close());
  const pathname = hooks.pathname ?? (() => win.location.pathname);
  const navigate = hooks.navigate ?? ((url) => win.location.replace(url));
  let synced = false;
  const onSlideChange = (event: Event): void => {
    const detail = (event as CustomEvent<{ index: number }>).detail;
    if (typeof detail?.index !== "number") return;
    channel.postMessage({ index: detail.index });
  };
  const onCloseRequest = (): void => {
    channel.postMessage({ close: true });
  };
  const onSwapRequest = (): void => {
    const route = swapRoute(pathname());
    if (route == null) {
      console.error("peitho: swap unavailable on this route");
      return;
    }
    channel.postMessage({ swapped: !route.swapped });
  };
  const onTimerChange = (event: Event): void => {
    if (!synced) return;
    const detail = (event as CustomEvent<TimerSyncState>).detail;
    if (
      typeof detail?.running !== "boolean" ||
      !isNonNegativeFiniteNumber(detail.elapsedMs)
    ) {
      console.error("Invalid peitho:timerchange event");
      return;
    }
    channel.postMessage({
      timer: { running: detail.running, elapsedMs: Math.round(detail.elapsedMs) }
    });
  };
  channel.onmessage = (event: { data: unknown }): void => {
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
    if (isTimerReplaySyncMessage(data)) {
      if (hooks.adoptTimerState) {
        const serverElapsed =
          data.timer.elapsedMs +
          (data.timer.running ? Math.max(0, data.nowMs - data.timer.atMs) : 0);
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
