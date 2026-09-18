import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  createRehearsalAudioQueue,
  installRehearsalAudio,
  type RehearsalMediaRecorder
} from "../src/rehearsalAudio";
import type { BeforeCloseDetail } from "../src/sync";

type Deferred<T> = {
  promise: Promise<T>;
  resolve(value: T): void;
  reject(reason: unknown): void;
};

type FetchMock = ReturnType<typeof vi.fn> & typeof fetch;

function mutableShell(initial: {
  elapsedMs?: number;
  startedAt?: number | null;
  paused?: boolean;
} = {}) {
  const state = {
    elapsedMs: initial.elapsedMs ?? 0,
    startedAt: initial.startedAt ?? null,
    paused: initial.paused ?? false
  };
  return {
    state,
    shell: {
      elapsedMs: () => state.elapsedMs,
      startedAt: () => state.startedAt,
      isPaused: () => state.paused
    }
  };
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

function response(status: number, json: unknown = {}): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    json: async () => json
  } as Response;
}

async function flushPromises(): Promise<void> {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}

class FakeRecorder extends EventTarget implements RehearsalMediaRecorder {
  state: RecordingState = "inactive";
  readonly starts: number[] = [];
  pauseCalls = 0;
  resumeCalls = 0;
  stopCalls = 0;

  start(timeslice?: number): void {
    this.starts.push(timeslice ?? 0);
    this.state = "recording";
  }

  pause(): void {
    this.pauseCalls += 1;
    this.state = "paused";
  }

  resume(): void {
    this.resumeCalls += 1;
    this.state = "recording";
  }

  stop(): void {
    this.stopCalls += 1;
    this.state = "inactive";
  }

  emitData(data: Blob): void {
    const event = new Event("dataavailable") as BlobEvent;
    Object.defineProperty(event, "data", { value: data });
    this.dispatchEvent(event);
  }

  emitStop(): void {
    this.dispatchEvent(new Event("stop"));
  }

  emitError(error: Error): void {
    const event = new Event("error") as Event & { error: Error };
    Object.defineProperty(event, "error", { value: error });
    this.dispatchEvent(event);
  }
}

const cleanups: Array<() => void> = [];

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  while (cleanups.length > 0) cleanups.pop()?.();
  vi.clearAllTimers();
  vi.useRealTimers();
  vi.restoreAllMocks();
  document.body.replaceChildren();
});

it("serializes uploads and advances after a lost-response 409 proves persistence", async () => {
  const pending: Array<Deferred<Response>> = [];
  const statuses: Array<string | null> = [];
  const fetcher = vi.fn(() => {
    const request = deferred<Response>();
    pending.push(request);
    return request.promise;
  }) as unknown as FetchMock;
  const queue = createRehearsalAudioQueue({
    fetcher,
    window,
    onUploadError: (message) => statuses.push(message)
  });
  cleanups.push(() => queue.destroy());

  queue.beginTake("take-a", 8_123);
  const firstBlob = new Blob(["A"]);
  queue.enqueue(firstBlob, false);
  expect(fetcher).toHaveBeenCalledTimes(1);
  expect(fetcher.mock.calls[0]?.[0]).toBe(
    "/rehearsal/audio?take=take-a&seq=0&startMs=8123"
  );
  expect(fetcher.mock.calls[0]?.[1]).toMatchObject({
    method: "POST",
    body: firstBlob
  });
  expect(fetcher.mock.calls[0]?.[1]).not.toHaveProperty("keepalive");

  pending[0].resolve(response(200));
  await flushPromises();
  const secondBlob = new Blob(["B"]);
  const thirdBlob = new Blob(["C"]);
  queue.enqueue(secondBlob, false);
  queue.enqueue(thirdBlob, false);
  expect(fetcher).toHaveBeenCalledTimes(2);
  pending[1].reject(new TypeError("response lost"));
  await flushPromises();
  expect(statuses.at(-1)).toContain("audio upload failed");
  expect(statuses.at(-1)).toContain("retrying");

  await vi.advanceTimersByTimeAsync(1_000);
  expect(fetcher).toHaveBeenCalledTimes(3);
  expect(fetcher.mock.calls[2]?.[1]).toMatchObject({ body: secondBlob });
  pending[2].resolve(response(409, { take: "take-a", nextSeq: 2 }));
  await flushPromises();

  expect(fetcher).toHaveBeenCalledTimes(4);
  expect(fetcher.mock.calls[3]?.[0]).toBe(
    "/rehearsal/audio?take=take-a&seq=2&startMs=8123"
  );
  expect(fetcher.mock.calls[3]?.[1]).toMatchObject({ body: thirdBlob });
  expect(statuses.at(-1)).toBeNull();
});

it("keeps the queue head for every unproven or malformed conflict", async () => {
  const conflicts = [
    { take: "take-b", nextSeq: 1 },
    { take: "take-a", nextSeq: 7 },
    { take: "take-a" },
    null
  ];

  for (const conflict of conflicts) {
    const fetcher = vi
      .fn()
      .mockResolvedValueOnce(response(409, conflict))
      .mockResolvedValue(response(500)) as unknown as FetchMock;
    const statuses: Array<string | null> = [];
    const queue = createRehearsalAudioQueue({
      fetcher,
      window,
      onUploadError: (message) => statuses.push(message)
    });
    queue.beginTake("take-a", 0);
    const blob = new Blob(["A"]);
    queue.enqueue(blob, false);
    await flushPromises();
    expect(fetcher).toHaveBeenCalledTimes(1);
    expect(statuses.at(-1)).toContain("retrying");

    await vi.advanceTimersByTimeAsync(1_000);
    expect(fetcher).toHaveBeenCalledTimes(2);
    expect(fetcher.mock.calls[1]?.[0]).toBe(
      "/rehearsal/audio?take=take-a&seq=0&startMs=0"
    );
    expect(fetcher.mock.calls[1]?.[1]).toMatchObject({ body: blob });
    queue.reset();
    expect(statuses.at(-1)).toBeNull();
    queue.destroy();
  }
});

it.each([
  [
    { take: "take-b", nextSeq: 0 },
    "audio upload failed: another presenter window took over the recording (retrying)"
  ],
  [
    { take: "take-a", nextSeq: 7 },
    "audio upload failed: recording out of order (restart the rehearsal) (retrying)"
  ],
  [
    { take: null, nextSeq: 0 },
    "audio upload failed: rehearsal session is not ready (retrying)"
  ]
])("turns a %j conflict into a user-actionable failure", async (conflict, expected) => {
  const statuses: Array<string | null> = [];
  const fetcher = vi.fn(async () => response(409, conflict)) as unknown as FetchMock;
  const queue = createRehearsalAudioQueue({
    fetcher,
    window,
    onUploadError: (message) => statuses.push(message)
  });
  cleanups.push(() => queue.destroy());
  queue.beginTake("take-a", 0);
  queue.enqueue(new Blob(["A"]), false);

  await flushPromises();

  expect(statuses.at(-1)).toBe(expected);
});

it("drops reset generations but waits for an active old request to settle", async () => {
  const pending: Array<Deferred<Response>> = [];
  const fetcher = vi.fn(() => {
    const request = deferred<Response>();
    pending.push(request);
    return request.promise;
  }) as unknown as FetchMock;
  const queue = createRehearsalAudioQueue({ fetcher, window });
  cleanups.push(() => queue.destroy());

  queue.beginTake("take-a", 0);
  queue.enqueue(new Blob(["A"]), false);
  queue.enqueue(new Blob(["B"]), false);
  queue.reset();
  queue.beginTake("take-b", 7_000);
  queue.enqueue(new Blob(["C"]), false);
  expect(fetcher).toHaveBeenCalledTimes(1);

  pending[0].reject(new TypeError("old request failed"));
  await flushPromises();
  expect(fetcher).toHaveBeenCalledTimes(2);
  expect(fetcher.mock.calls[1]?.[0]).toBe(
    "/rehearsal/audio?take=take-b&seq=0&startMs=7000"
  );
  await vi.advanceTimersByTimeAsync(1_000);
  expect(fetcher).toHaveBeenCalledTimes(2);
});

it("uses keepalive only when requested and suppresses empty chunks", async () => {
  const fetcher = vi.fn(async () => response(200)) as unknown as FetchMock;
  const queue = createRehearsalAudioQueue({ fetcher, window });
  cleanups.push(() => queue.destroy());
  queue.beginTake("take-a", 0);

  queue.enqueue(new Blob([]), false);
  queue.enqueue(new Blob(["A"]), false);
  await flushPromises();
  queue.enqueue(new Blob(["B"]), true);
  await flushPromises();

  expect(fetcher).toHaveBeenCalledTimes(2);
  expect(fetcher.mock.calls[0]?.[1]).not.toHaveProperty("keepalive");
  expect(fetcher.mock.calls[0]?.[1]?.signal).toBeInstanceOf(AbortSignal);
  expect(fetcher.mock.calls[1]?.[1]).toMatchObject({ keepalive: true });
  expect(fetcher.mock.calls[1]?.[1]).not.toHaveProperty("signal");
});

it("times out a hung non-keepalive upload and retries it visibly", async () => {
  const statuses: Array<string | null> = [];
  const timeout = vi.spyOn(AbortSignal, "timeout").mockImplementation((delay) => {
    const controller = new AbortController();
    window.setTimeout(() => controller.abort(new Error("upload timed out")), delay);
    return controller.signal;
  });
  const fetcher = vi.fn((_input: RequestInfo | URL, init?: RequestInit) => {
    return new Promise<Response>((_resolve, reject) => {
      init?.signal?.addEventListener("abort", () => reject(init.signal?.reason), { once: true });
    });
  }) as unknown as FetchMock;
  const queue = createRehearsalAudioQueue({
    fetcher,
    window,
    onUploadError: (message) => statuses.push(message)
  });
  cleanups.push(() => queue.destroy());
  queue.beginTake("take-a", 0);
  queue.enqueue(new Blob(["A"]), false);

  expect(timeout).toHaveBeenCalledWith(10_000);
  await vi.advanceTimersByTimeAsync(9_999);
  expect(statuses).toEqual([]);

  await vi.advanceTimersByTimeAsync(1);
  await flushPromises();
  expect(statuses.at(-1)).toBe("audio upload failed: upload timed out (retrying)");

  await vi.advanceTimersByTimeAsync(1_000);
  expect(fetcher).toHaveBeenCalledTimes(2);
  expect(fetcher.mock.calls[1]?.[1]?.body).toBe(fetcher.mock.calls[0]?.[1]?.body);
});

it.each([400, 404, 413])(
  "retains a permanent %s failure without claiming or scheduling a retry",
  async (status) => {
    const statuses: Array<string | null> = [];
    const fetcher = vi.fn(async () => response(status)) as unknown as FetchMock;
    const queue = createRehearsalAudioQueue({
      fetcher,
      window,
      onUploadError: (message) => statuses.push(message)
    });
    cleanups.push(() => queue.destroy());
    queue.beginTake("take-a", 0);
    queue.enqueue(new Blob(["A"]), false);
    await flushPromises();

    expect(statuses.at(-1)).toBe(`audio upload failed: server returned ${status}`);
    await vi.advanceTimersByTimeAsync(10_000);
    queue.enqueue(new Blob(["B"]), false);
    await flushPromises();
    expect(fetcher).toHaveBeenCalledTimes(1);

    queue.reset();
    queue.beginTake("take-b", 7_000);
    queue.enqueue(new Blob(["C"]), false);
    await flushPromises();
    expect(fetcher).toHaveBeenCalledTimes(2);
    expect(fetcher.mock.calls[1]?.[0]).toBe(
      "/rehearsal/audio?take=take-b&seq=0&startMs=7000"
    );
  }
);

it("settles drain waiters when a permanent failure blocks the queue", async () => {
  const request = deferred<Response>();
  const fetcher = vi.fn(() => request.promise) as unknown as FetchMock;
  const queue = createRehearsalAudioQueue({ fetcher, window });
  cleanups.push(() => queue.destroy());
  queue.beginTake("take-a", 0);
  queue.enqueue(new Blob(["A"]), false);

  const waitingDrain = vi.fn();
  void queue.drain().then(waitingDrain);
  await flushPromises();
  expect(waitingDrain).not.toHaveBeenCalled();

  request.resolve(response(400));
  await flushPromises();
  expect(waitingDrain).toHaveBeenCalledTimes(1);

  const blockedDrain = vi.fn();
  void queue.drain().then(blockedDrain);
  await flushPromises();
  expect(blockedDrain).toHaveBeenCalledTimes(1);
});

it("keeps drain pending while a retryable failure is retrying", async () => {
  const fetcher = vi.fn(async () => response(500)) as unknown as FetchMock;
  const queue = createRehearsalAudioQueue({ fetcher, window });
  cleanups.push(() => queue.destroy());
  queue.beginTake("take-a", 0);
  queue.enqueue(new Blob(["A"]), false);

  const drained = vi.fn();
  void queue.drain().then(drained);
  await flushPromises();
  expect(drained).not.toHaveBeenCalled();

  await vi.advanceTimersByTimeAsync(1_000);
  await flushPromises();
  expect(fetcher).toHaveBeenCalledTimes(2);
  expect(drained).not.toHaveBeenCalled();

  queue.reset();
  await flushPromises();
  expect(drained).toHaveBeenCalledTimes(1);
});

it("acquires immediately and follows local timer state even when start precedes permission", async () => {
  const permission = deferred<MediaStream>();
  const track = { stop: vi.fn() } as unknown as MediaStreamTrack;
  const stream = { getTracks: () => [track] } as unknown as MediaStream;
  const recorder = new FakeRecorder();
  const recorderOptions: MediaRecorderOptions[] = [];
  const indicator = document.createElement("span");
  const bus = new EventTarget();
  const timer = mutableShell();
  const fetcher = vi.fn(async () => response(200)) as unknown as FetchMock;
  const cleanup = installRehearsalAudio({
    indicator,
    shell: timer.shell,
    bus,
    window,
    fetcher,
    getUserMedia: vi.fn((constraints) => {
      expect(constraints).toEqual({ audio: true });
      return permission.promise;
    }),
    createMediaRecorder: (_stream, options) => {
      expect(_stream).toBe(stream);
      recorderOptions.push(options);
      return recorder;
    },
    takeIdFactory: () => "take-a"
  });
  cleanups.push(cleanup);

  expect(indicator.textContent).toBe("… REC");
  timer.state.startedAt = 100;
  timer.state.elapsedMs = 8_000;
  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "start" } }));
  timer.state.elapsedMs = 8_123.6;
  permission.resolve(stream);
  await flushPromises();
  expect(recorderOptions).toEqual([{ mimeType: "audio/webm" }]);
  expect(recorder.starts).toEqual([5_000]);
  expect(indicator.textContent).toBe("● REC");

  recorder.emitData(new Blob(["head"]));
  await flushPromises();
  expect(fetcher.mock.calls[0]?.[0]).toBe(
    "/rehearsal/audio?take=take-a&seq=0&startMs=8124"
  );

  timer.state.paused = true;
  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "pause" } }));
  expect(recorder.pauseCalls).toBe(1);
  expect(indicator.textContent).toBe("❙❙ REC");
  timer.state.paused = false;
  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "resume" } }));
  expect(recorder.resumeCalls).toBe(1);
  expect(indicator.textContent).toBe("● REC");
});

it("reconciles valid adopted start pause resume and reset states", async () => {
  const recorder = new FakeRecorder();
  const indicator = document.createElement("span");
  const bus = new EventTarget();
  const timer = mutableShell();
  const stream = { getTracks: () => [] } as unknown as MediaStream;
  const cleanup = installRehearsalAudio({
    indicator,
    shell: timer.shell,
    bus,
    window,
    getUserMedia: async () => stream,
    createMediaRecorder: () => recorder,
    takeIdFactory: () => "take-a"
  });
  cleanups.push(cleanup);
  await flushPromises();
  expect(indicator.textContent).toBe("○ REC");

  const adopt = (running: boolean, previousElapsedMs: number, elapsedMs: number): void => {
    timer.state.elapsedMs = elapsedMs;
    timer.state.startedAt = running || elapsedMs > 0 ? 100 : null;
    timer.state.paused = !running && elapsedMs > 0;
    bus.dispatchEvent(
      new CustomEvent("peitho:timeradopt", {
        detail: { running, previousElapsedMs, elapsedMs }
      })
    );
  };
  adopt(true, 0, 7_000);
  expect(recorder.starts).toEqual([5_000]);
  adopt(false, 7_000, 8_000);
  expect(recorder.pauseCalls).toBe(1);
  adopt(true, 8_000, 8_000);
  expect(recorder.resumeCalls).toBe(1);
  adopt(false, 8_000, 0);
  expect(recorder.stopCalls).toBe(1);

  bus.dispatchEvent(
    new CustomEvent("peitho:timeradopt", {
      detail: { running: "yes", previousElapsedMs: 0, elapsedMs: 0 }
    })
  );
  expect(recorder.starts).toEqual([5_000]);
});

it("reset discards old chunks and waits for stop before minting a new take", async () => {
  const recorder = new FakeRecorder();
  const indicator = document.createElement("span");
  const bus = new EventTarget();
  const timer = mutableShell();
  const fetcher = vi.fn(async () => response(200)) as unknown as FetchMock;
  const takes = ["take-a", "take-b"];
  const cleanup = installRehearsalAudio({
    indicator,
    shell: timer.shell,
    bus,
    window,
    fetcher,
    getUserMedia: async () => ({ getTracks: () => [] }) as unknown as MediaStream,
    createMediaRecorder: () => recorder,
    takeIdFactory: () => takes.shift()!
  });
  cleanups.push(cleanup);
  await flushPromises();

  timer.state.startedAt = 100;
  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "start" } }));
  timer.state.startedAt = null;
  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "reset" } }));
  recorder.emitData(new Blob(["discarded"]));
  timer.state.startedAt = 200;
  timer.state.elapsedMs = 250;
  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "start" } }));
  expect(recorder.starts).toEqual([5_000]);
  recorder.emitStop();
  expect(recorder.starts).toEqual([5_000, 5_000]);
  recorder.emitData(new Blob(["new"]));
  await flushPromises();

  expect(fetcher).toHaveBeenCalledTimes(1);
  expect(fetcher.mock.calls[0]?.[0]).toBe(
    "/rehearsal/audio?take=take-b&seq=0&startMs=250"
  );
});

it("awaits the final close chunk upload and does not duplicate it on pagehide", async () => {
  const recorder = new FakeRecorder();
  const indicator = document.createElement("span");
  const bus = new EventTarget();
  const upload = deferred<Response>();
  const fetcher = vi.fn(() => upload.promise) as unknown as FetchMock;
  const track = { stop: vi.fn() } as unknown as MediaStreamTrack;
  const timer = mutableShell();
  const cleanup = installRehearsalAudio({
    indicator,
    shell: timer.shell,
    bus,
    window,
    fetcher,
    getUserMedia: async () => ({ getTracks: () => [track] }) as unknown as MediaStream,
    createMediaRecorder: () => recorder,
    takeIdFactory: () => "take-a"
  });
  cleanups.push(cleanup);
  await flushPromises();
  timer.state.startedAt = 100;
  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "start" } }));

  const registered: PromiseLike<unknown>[] = [];
  bus.dispatchEvent(
    new CustomEvent<BeforeCloseDetail>("peitho:beforeclose", {
      detail: { waitUntil: (promise) => registered.push(promise) }
    })
  );
  expect(recorder.stopCalls).toBe(1);
  expect(registered).toHaveLength(1);
  expect(fetcher).not.toHaveBeenCalled();

  recorder.emitData(new Blob(["final"]));
  recorder.emitStop();
  await flushPromises();

  expect(fetcher).toHaveBeenCalledTimes(1);
  expect(fetcher.mock.calls[0]?.[1]).not.toHaveProperty("keepalive");
  expect(track.stop).toHaveBeenCalledTimes(1);

  let drained = false;
  void registered[0]?.then(() => {
    drained = true;
  });
  await flushPromises();
  expect(drained).toBe(false);

  upload.resolve(response(200));
  await registered[0];
  expect(drained).toBe(true);

  window.dispatchEvent(new Event("pagehide"));
  await flushPromises();
  expect(recorder.stopCalls).toBe(1);
  expect(fetcher).toHaveBeenCalledTimes(1);
});

it("uses keepalive for the pagehide fallback final chunk", async () => {
  const recorder = new FakeRecorder();
  const indicator = document.createElement("span");
  const bus = new EventTarget();
  const timer = mutableShell({ startedAt: 100, elapsedMs: 12_000 });
  const fetcher = vi.fn(async () => response(200)) as unknown as FetchMock;
  const cleanup = installRehearsalAudio({
    indicator,
    shell: timer.shell,
    bus,
    window,
    fetcher,
    getUserMedia: async () => ({ getTracks: () => [] }) as unknown as MediaStream,
    createMediaRecorder: () => recorder,
    takeIdFactory: () => "take-a"
  });
  cleanups.push(cleanup);
  await flushPromises();

  window.dispatchEvent(new Event("pagehide"));
  recorder.emitData(new Blob(["final"]));
  recorder.emitStop();
  await flushPromises();

  expect(fetcher).toHaveBeenCalledTimes(1);
  expect(fetcher.mock.calls[0]?.[1]).toMatchObject({ keepalive: true });
});

it("derives recorder transitions from shell state instead of timer requests", async () => {
  const recorder = new FakeRecorder();
  const indicator = document.createElement("span");
  const bus = new EventTarget();
  const timer = mutableShell();
  const cleanup = installRehearsalAudio({
    indicator,
    shell: timer.shell,
    bus,
    window,
    getUserMedia: async () => ({ getTracks: () => [] }) as unknown as MediaStream,
    createMediaRecorder: () => recorder,
    takeIdFactory: () => "take-a"
  });
  cleanups.push(cleanup);
  await flushPromises();

  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "resume" } }));
  expect(recorder.starts).toEqual([]);

  timer.state.startedAt = 100;
  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "start" } }));
  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "start" } }));
  expect(recorder.starts).toEqual([5_000]);

  timer.state.paused = true;
  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "pause" } }));
  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "start" } }));
  expect(recorder.pauseCalls).toBe(1);
  expect(recorder.resumeCalls).toBe(0);

  timer.state.paused = false;
  bus.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "resume" } }));
  expect(recorder.resumeCalls).toBe(1);
});

it("renders permission construction recorder and sustained upload failures", async () => {
  const deniedIndicator = document.createElement("span");
  const deniedTimer = mutableShell();
  const denied = installRehearsalAudio({
    indicator: deniedIndicator,
    shell: deniedTimer.shell,
    window,
    getUserMedia: async () => {
      throw new DOMException("permission denied", "NotAllowedError");
    }
  });
  cleanups.push(denied);
  await flushPromises();
  expect(deniedIndicator.textContent).toContain("mic unavailable:");
  expect(deniedIndicator.textContent).toContain("permission denied");

  const constructionIndicator = document.createElement("span");
  const constructionTimer = mutableShell();
  const construction = installRehearsalAudio({
    indicator: constructionIndicator,
    shell: constructionTimer.shell,
    window,
    getUserMedia: async () => ({ getTracks: () => [] }) as unknown as MediaStream,
    createMediaRecorder: () => {
      throw new Error("unsupported recorder");
    }
  });
  cleanups.push(construction);
  await flushPromises();
  expect(constructionIndicator.textContent).toContain("mic unavailable: unsupported recorder");

  const recorder = new FakeRecorder();
  const errorIndicator = document.createElement("span");
  const errorTimer = mutableShell();
  const upload = installRehearsalAudio({
    indicator: errorIndicator,
    shell: errorTimer.shell,
    window,
    fetcher: vi.fn(async () => response(500)) as unknown as FetchMock,
    getUserMedia: async () => ({ getTracks: () => [] }) as unknown as MediaStream,
    createMediaRecorder: () => recorder,
    takeIdFactory: () => "take-a"
  });
  cleanups.push(upload);
  await flushPromises();
  errorTimer.state.startedAt = 100;
  window.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "start" } }));
  recorder.emitData(new Blob(["chunk"]));
  await flushPromises();
  expect(errorIndicator.textContent).toBe(
    "● REC — audio upload failed: server returned 500 (retrying)"
  );

  errorTimer.state.paused = true;
  window.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "pause" } }));
  expect(errorIndicator.textContent).toBe(
    "❙❙ REC — audio upload failed: server returned 500 (retrying)"
  );

  recorder.emitError(new Error("encoder stopped"));
  expect(errorIndicator.textContent).toContain("mic unavailable: encoder stopped");
  recorder.state = "inactive";
  recorder.emitStop();
  expect(errorIndicator.textContent).toContain("mic unavailable: encoder stopped");
  errorTimer.state.startedAt = null;
  window.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "reset" } }));
  errorTimer.state.startedAt = 200;
  window.dispatchEvent(new CustomEvent("peitho:timercontrol", { detail: { action: "start" } }));
  expect(errorIndicator.textContent).toContain("mic unavailable: encoder stopped");
  expect(recorder.starts).toEqual([5_000]);
});
