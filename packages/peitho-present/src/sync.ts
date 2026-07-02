export type SyncMessage = { index: number };

export type SyncChannel = {
  onmessage: ((event: { data: unknown }) => void) | null;
  postMessage(message: SyncMessage): void;
  close(): void;
};

export type SyncChannelFactory = (name: string) => SyncChannel;

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

export function installSyncBridge(
  win: Window = window,
  channelFactory: SyncChannelFactory = defaultChannelFactory
): () => void {
  const channel = channelFactory("peitho-sync");
  const onSlideChange = (event: Event): void => {
    const detail = (event as CustomEvent<{ index: number }>).detail;
    if (typeof detail?.index !== "number") return;
    channel.postMessage({ index: detail.index });
  };
  channel.onmessage = (event: { data: unknown }): void => {
    const data = event.data as Partial<SyncMessage>;
    if (typeof data.index !== "number") {
      console.error("Invalid peitho sync message");
      return;
    }
    win.dispatchEvent(new CustomEvent("peitho:navigate", { detail: { to: { index: data.index } } }));
  };
  win.addEventListener("peitho:slidechange", onSlideChange);
  return () => {
    win.removeEventListener("peitho:slidechange", onSlideChange);
    channel.onmessage = null;
    channel.close();
  };
}
