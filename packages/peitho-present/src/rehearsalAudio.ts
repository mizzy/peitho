import {
  isValidTimerAdoptDetail,
  roundNonNegativeMs,
  type TimerAdoptDetail,
  type TimerControlDetail
} from "./shell";
import type { BeforeCloseDetail } from "./sync";

const AUDIO_TIMESLICE_MS = 5_000;
const AUDIO_RETRY_MS = 1_000;
const AUDIO_UPLOAD_TIMEOUT_MS = 10_000;
const INVALID_RESPONSE_REASON = "server returned an invalid response";
const SESSION_NOT_READY_REASON = "rehearsal session is not ready";
const OUT_OF_ORDER_REASON = "recording out of order; restart the run";
const TAKEOVER_REASON = "another window took over the recording";
const RECORDER_ERROR_REASON = "MediaRecorder error";
export const REHEARSAL_AUDIO_DETAIL_MAX_CHARS = 72;

function serverStatusReason(status: number): string {
  return `server returned ${status}`;
}

function uploadFailureMessage(reason: string, retrying: boolean): string {
  return `audio upload failed: ${reason}${retrying ? " (retrying)" : ""}`;
}

function micUnavailableMessage(reason: string): string {
  return `mic unavailable: ${reason}`;
}

type AudioQueueItem = {
  take: string;
  seq: number;
  startMs: number;
  blob: Blob;
  keepalive: boolean;
  generation: number;
};

type UploadResult =
  | { accepted: true }
  | { accepted: false; reason: string; retryable: boolean };

export type RehearsalAudioQueue = {
  beginTake(take: string, startMs: number): void;
  enqueue(blob: Blob, keepalive: boolean): void;
  drain(): Promise<void>;
  reset(): void;
  destroy(): void;
};

export type RehearsalAudioQueueOptions = {
  fetcher?: typeof fetch;
  window?: Window;
  onUploadError?: (message: string | null) => void;
};

export type RehearsalMediaRecorder = EventTarget & {
  readonly state: RecordingState;
  start(timeslice?: number): void;
  pause(): void;
  resume(): void;
  stop(): void;
};

type RehearsalAudioShell = {
  elapsedMs(): number;
  startedAt(): number | null;
  isPaused(): boolean;
};

export type RehearsalAudioOptions = {
  indicator: HTMLElement;
  indicatorLabel: Node;
  detail: HTMLElement;
  shell: RehearsalAudioShell;
  bus?: EventTarget;
  window?: Window;
  fetcher?: typeof fetch;
  getUserMedia?: (constraints: MediaStreamConstraints) => Promise<MediaStream>;
  createMediaRecorder?: (
    stream: MediaStream,
    options: MediaRecorderOptions
  ) => RehearsalMediaRecorder;
  takeIdFactory?: () => string;
  console?: Pick<Console, "error">;
};

export type RehearsalAudioMediaState =
  | "pending"
  | "ready"
  | "recording"
  | "paused"
  | "unavailable";

export type RehearsalAudioPresentation = {
  state: Exclude<RehearsalAudioMediaState, "unavailable"> | "error";
  label: "REC" | "ERR";
  detail: string;
};

export function rehearsalAudioAuthoredDetails(status: number): string[] {
  const uploadReasons = [
    serverStatusReason(status),
    INVALID_RESPONSE_REASON,
    SESSION_NOT_READY_REASON,
    OUT_OF_ORDER_REASON,
    TAKEOVER_REASON
  ];
  return [
    ...uploadReasons.flatMap((reason) => [
      uploadFailureMessage(reason, false),
      uploadFailureMessage(reason, true)
    ]),
    micUnavailableMessage(RECORDER_ERROR_REASON)
  ];
}

export function rehearsalAudioPresentation(
  mediaState: RehearsalAudioMediaState,
  mediaError: string | null,
  uploadError: string | null
): RehearsalAudioPresentation {
  if (mediaState === "unavailable") {
    return {
      state: "error",
      label: "ERR",
      detail: mediaError!
    };
  }
  if (uploadError !== null) {
    return {
      state: mediaState,
      label: "ERR",
      detail: uploadError
    };
  }
  return { state: mediaState, label: "REC", detail: "" };
}

export function createRehearsalAudioQueue(
  options: RehearsalAudioQueueOptions = {}
): RehearsalAudioQueue {
  const win = options.window ?? window;
  const fetcher = options.fetcher ?? win.fetch.bind(win);
  const onUploadError = options.onUploadError ?? (() => undefined);
  let generation = 0;
  let currentTake: { id: string; startMs: number } | null = null;
  let nextSeq = 0;
  let queued: AudioQueueItem[] = [];
  let active: AudioQueueItem | null = null;
  let retryTimer: number | null = null;
  let retryBlocked = false;
  let destroyed = false;
  const drainWaiters = new Set<() => void>();

  function settleDrainWaiters(): void {
    if (
      !destroyed &&
      !retryBlocked &&
      (active !== null || queued.length > 0 || retryTimer !== null)
    ) {
      return;
    }
    for (const resolve of drainWaiters) resolve();
    drainWaiters.clear();
  }

  function beginTake(take: string, startMs: number): void {
    if (destroyed) return;
    currentTake = { id: take, startMs };
    nextSeq = 0;
  }

  function enqueue(blob: Blob, keepalive: boolean): void {
    if (destroyed || currentTake === null || blob.size === 0) return;
    queued.push({
      take: currentTake.id,
      seq: nextSeq,
      startMs: currentTake.startMs,
      blob,
      keepalive,
      generation
    });
    nextSeq += 1;
    pump();
  }

  function drain(): Promise<void> {
    if (
      destroyed ||
      retryBlocked ||
      (active === null && queued.length === 0 && retryTimer === null)
    ) {
      return Promise.resolve();
    }
    return new Promise((resolve) => {
      drainWaiters.add(resolve);
    });
  }

  function reset(): void {
    if (destroyed) return;
    generation += 1;
    currentTake = null;
    nextSeq = 0;
    queued = [];
    retryBlocked = false;
    if (retryTimer !== null) {
      win.clearTimeout(retryTimer);
      retryTimer = null;
    }
    onUploadError(null);
    settleDrainWaiters();
  }

  function destroy(): void {
    if (destroyed) return;
    destroyed = true;
    generation += 1;
    currentTake = null;
    queued = [];
    if (retryTimer !== null) {
      win.clearTimeout(retryTimer);
      retryTimer = null;
    }
    settleDrainWaiters();
  }

  function pump(): void {
    if (destroyed || active !== null || retryTimer !== null || retryBlocked) return;
    const item = queued[0];
    if (item == null) {
      settleDrainWaiters();
      return;
    }
    active = item;
    void upload(item).then((result) => {
      if (active !== item) return;
      active = null;
      if (destroyed || item.generation !== generation) {
        pump();
        return;
      }
      if (result.accepted) {
        if (queued[0] === item) queued.shift();
        onUploadError(null);
        pump();
        return;
      }
      if (!result.retryable) {
        retryBlocked = true;
        onUploadError(uploadFailureMessage(result.reason, false));
        settleDrainWaiters();
        return;
      }
      onUploadError(uploadFailureMessage(result.reason, true));
      retryTimer = win.setTimeout(() => {
        retryTimer = null;
        pump();
      }, AUDIO_RETRY_MS);
    });
  }

  async function upload(item: AudioQueueItem): Promise<UploadResult> {
    try {
      const response = await fetcher(
        `/rehearsal/audio?take=${encodeURIComponent(item.take)}&seq=${item.seq}&startMs=${item.startMs}`,
        {
          method: "POST",
          headers: { "Content-Type": "audio/webm" },
          body: item.blob,
          ...(item.keepalive
            ? { keepalive: true }
            : { signal: AbortSignal.timeout(AUDIO_UPLOAD_TIMEOUT_MS) })
        }
      );
      if (response.ok) return { accepted: true };
      if (response.status !== 409) {
        return {
          accepted: false,
          reason: serverStatusReason(response.status),
          retryable: !isPermanentUploadStatus(response.status)
        };
      }
      const conflict = (await response.json()) as unknown;
      if (isPersistenceAcknowledgement(conflict, item)) return { accepted: true };
      return { accepted: false, reason: conflictReason(conflict, item.take), retryable: true };
    } catch (error: unknown) {
      return { accepted: false, reason: errorReason(error), retryable: true };
    }
  }

  return { beginTake, enqueue, drain, reset, destroy };
}

function isPermanentUploadStatus(status: number): boolean {
  return status === 400 || status === 404 || status === 413;
}

export function installRehearsalAudio(options: RehearsalAudioOptions): () => void {
  const win = options.window ?? window;
  const bus = options.bus ?? win;
  const log = options.console ?? console;
  const getUserMedia =
    options.getUserMedia ??
    ((constraints: MediaStreamConstraints) => win.navigator.mediaDevices.getUserMedia(constraints));
  const createMediaRecorder =
    options.createMediaRecorder ??
    ((stream: MediaStream, recorderOptions: MediaRecorderOptions) =>
      new MediaRecorder(stream, recorderOptions));
  const takeIdFactory = options.takeIdFactory ?? (() => win.crypto.randomUUID());
  let desiredState: RecordingState =
    options.shell.startedAt() == null
      ? "inactive"
      : options.shell.isPaused()
        ? "paused"
        : "recording";
  let recorder: RehearsalMediaRecorder | null = null;
  let stream: MediaStream | null = null;
  let waitingForStop = false;
  let stopMode: "reset" | "close" | "destroy" | null = null;
  let uploadError: string | null = null;
  let mediaState: RehearsalAudioMediaState = "pending";
  let mediaError: string | null = null;
  let closing = false;
  let destroyed = false;
  let closeKeepalive = false;
  let closePromise: Promise<void> | null = null;
  let resolveCloseStop: (() => void) | null = null;

  const queue = createRehearsalAudioQueue({
    fetcher: options.fetcher,
    window: win,
    onUploadError(message) {
      uploadError = message;
      renderIndicator();
    }
  });

  function renderIndicator(): void {
    const presentation = rehearsalAudioPresentation(mediaState, mediaError, uploadError);
    options.indicator.dataset.peithoAudioState = presentation.state;
    options.indicatorLabel.textContent = presentation.label;
    options.detail.textContent = presentation.detail;
    if (presentation.detail === "") {
      options.detail.removeAttribute("title");
    } else {
      options.detail.title = presentation.detail;
    }
  }

  function setMediaState(state: Exclude<RehearsalAudioMediaState, "unavailable">): void {
    mediaState = state;
    renderIndicator();
  }

  function setUnavailable(error: unknown): void {
    mediaState = "unavailable";
    mediaError = micUnavailableMessage(errorReason(error));
    renderIndicator();
  }

  function stopTracks(): void {
    const currentStream = stream;
    if (currentStream == null) return;
    stream = null;
    for (const track of currentStream.getTracks()) track.stop();
  }

  function recorderFailure(error: unknown): void {
    setUnavailable(error);
    desiredState = "inactive";
    stopTracks();
  }

  function startTake(): void {
    const current = recorder;
    if (current == null || current.state !== "inactive") return;
    try {
      const take = takeIdFactory();
      current.start(AUDIO_TIMESLICE_MS);
      queue.beginTake(take, roundNonNegativeMs(options.shell.elapsedMs()));
      if (desiredState === "paused") {
        current.pause();
        setMediaState("paused");
      } else {
        setMediaState("recording");
      }
    } catch (error: unknown) {
      recorderFailure(error);
    }
  }

  function requestStop(mode: "reset" | "close" | "destroy"): void {
    const current = recorder;
    if (current == null || current.state === "inactive") {
      if (mode === "close" || mode === "destroy") stopTracks();
      return;
    }
    waitingForStop = true;
    stopMode = mode;
    try {
      current.stop();
    } catch (error: unknown) {
      waitingForStop = false;
      stopMode = null;
      recorderFailure(error);
    }
  }

  function reconcile(): void {
    const current = recorder;
    if (
      current == null ||
      destroyed ||
      closing ||
      waitingForStop ||
      mediaState === "unavailable"
    ) {
      return;
    }
    if (desiredState === "inactive") {
      if (current.state === "inactive") {
        setMediaState("ready");
      } else {
        requestStop("reset");
      }
      return;
    }
    if (current.state === "inactive") {
      startTake();
      return;
    }
    try {
      if (desiredState === "paused" && current.state === "recording") {
        current.pause();
        setMediaState("paused");
      } else if (desiredState === "recording" && current.state === "paused") {
        current.resume();
        setMediaState("recording");
      } else if (current.state === "paused") {
        setMediaState("paused");
      } else {
        setMediaState("recording");
      }
    } catch (error: unknown) {
      recorderFailure(error);
    }
  }

  function reset(): void {
    desiredState = "inactive";
    queue.reset();
    reconcile();
  }

  function reconcileWithShell(): void {
    if (options.shell.startedAt() == null) {
      reset();
      return;
    }
    desiredState = options.shell.isPaused() ? "paused" : "recording";
    reconcile();
  }

  function onTimerControl(event: Event): void {
    const action = (event as CustomEvent<TimerControlDetail>).detail?.action;
    if (action !== "start" && action !== "pause" && action !== "resume" && action !== "reset") {
      return;
    }
    reconcileWithShell();
  }

  function onTimerAdopt(event: Event): void {
    const detail = (event as CustomEvent<TimerAdoptDetail>).detail;
    if (!isValidTimerAdoptDetail(detail)) {
      log.error("Invalid peitho:timeradopt event for rehearsal audio");
      return;
    }
    reconcileWithShell();
  }

  function stopForClose(): Promise<void> {
    const current = recorder;
    if (current == null || current.state === "inactive") {
      stopTracks();
      return Promise.resolve();
    }
    return new Promise((resolve) => {
      resolveCloseStop = resolve;
      requestStop("close");
      if (!waitingForStop) {
        resolveCloseStop = null;
        resolve();
      }
    });
  }

  function close(keepalive: boolean): Promise<void> {
    if (closePromise !== null) return closePromise;
    if (destroyed) return Promise.resolve();
    closing = true;
    closeKeepalive = keepalive;
    desiredState = "inactive";
    closePromise = stopForClose().then(() => queue.drain());
    return closePromise;
  }

  function onBeforeClose(event: Event): void {
    const detail = (event as CustomEvent<BeforeCloseDetail>).detail;
    if (typeof detail?.waitUntil !== "function") return;
    detail.waitUntil(close(false));
  }

  function onPageHide(): void {
    void close(true);
  }

  function onDataAvailable(event: Event): void {
    const data = (event as BlobEvent).data;
    if (destroyed || stopMode === "reset" || stopMode === "destroy") return;
    queue.enqueue(data, stopMode === "close" && closeKeepalive);
  }

  function onStop(): void {
    const completedMode = stopMode;
    stopMode = null;
    waitingForStop = false;
    if (completedMode === "close" || completedMode === "destroy") {
      stopTracks();
      if (completedMode === "close") {
        const resolve = resolveCloseStop;
        resolveCloseStop = null;
        resolve?.();
      }
      return;
    }
    if (mediaState === "unavailable") {
      stopTracks();
      return;
    }
    if (!destroyed) {
      setMediaState("ready");
      reconcile();
    }
  }

  function onRecorderError(event: Event): void {
    const error = (event as Event & { error?: unknown }).error ?? RECORDER_ERROR_REASON;
    recorderFailure(error);
  }

  renderIndicator();
  bus.addEventListener("peitho:timercontrol", onTimerControl);
  bus.addEventListener("peitho:timeradopt", onTimerAdopt);
  bus.addEventListener("peitho:beforeclose", onBeforeClose);
  win.addEventListener("pagehide", onPageHide);

  void getUserMedia({ audio: true })
    .then((mediaStream) => {
      if (destroyed || closing) {
        for (const track of mediaStream.getTracks()) track.stop();
        return;
      }
      stream = mediaStream;
      try {
        recorder = createMediaRecorder(mediaStream, { mimeType: "audio/webm" });
      } catch (error: unknown) {
        setUnavailable(error);
        stopTracks();
        return;
      }
      recorder.addEventListener("dataavailable", onDataAvailable);
      recorder.addEventListener("stop", onStop);
      recorder.addEventListener("error", onRecorderError);
      setMediaState("ready");
      reconcile();
    })
    .catch((error: unknown) => {
      if (!destroyed && !closing) setUnavailable(error);
    });

  return () => {
    if (destroyed) return;
    destroyed = true;
    bus.removeEventListener("peitho:timercontrol", onTimerControl);
    bus.removeEventListener("peitho:timeradopt", onTimerAdopt);
    bus.removeEventListener("peitho:beforeclose", onBeforeClose);
    win.removeEventListener("pagehide", onPageHide);
    queue.destroy();
    const resolve = resolveCloseStop;
    resolveCloseStop = null;
    resolve?.();
    requestStop("destroy");
    recorder?.removeEventListener("dataavailable", onDataAvailable);
    recorder?.removeEventListener("stop", onStop);
    recorder?.removeEventListener("error", onRecorderError);
    stopTracks();
  };
}

function isPersistenceAcknowledgement(conflict: unknown, item: AudioQueueItem): boolean {
  if (typeof conflict !== "object" || conflict === null) return false;
  const candidate = conflict as { take?: unknown; nextSeq?: unknown };
  return candidate.take === item.take && candidate.nextSeq === item.seq + 1;
}

function conflictReason(conflict: unknown, requestTake: string): string {
  if (typeof conflict !== "object" || conflict === null) {
    return INVALID_RESPONSE_REASON;
  }
  const candidate = conflict as { take?: unknown };
  if (candidate.take === null) return SESSION_NOT_READY_REASON;
  if (typeof candidate.take !== "string") return INVALID_RESPONSE_REASON;
  return candidate.take === requestTake ? OUT_OF_ORDER_REASON : TAKEOVER_REASON;
}

function errorReason(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
