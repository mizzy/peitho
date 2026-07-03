export type SyncMessage = { index: number } | { close: true };

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

type ServerSyncPollResponse = {
  seq: number;
  message: unknown;
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function isCloseSyncMessage(value: unknown): value is { close: true } {
  return isRecord(value) && value.close === true;
}

function isIndexSyncMessage(value: unknown): value is { index: number } {
  return isRecord(value) && typeof value.index === "number";
}

function defaultChannelFactory(name: string): SyncChannel {
  const channel = new BroadcastChannel(name);
  let onmessage: ((event: { data: unknown }) => void) | null = null;
  channel.onmessage = (event: MessageEvent): void => {
    onmessage?.({ data: event.data });
  };
  return {
    get onmessage() {
      return onmessage;
    },
    set onmessage(next) {
      onmessage = next;
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
    let abortController: AbortController | null = null;
    let retryTimer: number | null = null;

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
      while (!closed && !(await handshake())) {
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
          const body = (await response.json()) as Partial<ServerSyncPollResponse>;
          if (typeof body.seq !== "number" || !("message" in body)) {
            console.error("Invalid peitho server sync message");
            await delay();
            continue;
          }
          seq = body.seq;
          onmessage?.({ data: body.message });
        } catch (error: unknown) {
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
      postMessage(message: SyncMessage): void {
        void fetcher(url, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(message),
          keepalive: true
        })
          .then((response) => {
            if (!response.ok) console.error(`Failed to post sync message: ${response.status}`);
          })
          .catch((error: unknown) => {
            console.error(`Failed to post sync message: ${String(error)}`);
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
  closeWindow: () => void = () => win.close()
): () => void {
  const channel = channelFactory("peitho-sync");
  const onSlideChange = (event: Event): void => {
    const detail = (event as CustomEvent<{ index: number }>).detail;
    if (typeof detail?.index !== "number") return;
    channel.postMessage({ index: detail.index });
  };
  const onCloseRequest = (): void => {
    channel.postMessage({ close: true });
  };
  channel.onmessage = (event: { data: unknown }): void => {
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
